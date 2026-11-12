//! Gestionnaire PDF : extraction, caviardage réel (texte retiré du flux, pixels
//! effacés), balises écrites à l'emplacement, métadonnées, annotations, signets,
//! formulaires, contrôle de fuite. Enregistrement complet, jamais incrémental.

use std::borrow::Cow;

use anon_core::{
    BlockContext, BlockRole, Extraction, FormatError, Handler, MediaItem, Output, TextBlock,
    TextTransform, TransformOptions, Warning, WarningCode,
};
use image::{DynamicImage, ImageFormat, RgbImage};
use mupdf::pdf::{
    InsertImageOptions, PageImageSource, PdfAnnotationType, PdfDocument, PdfObject, PdfPage,
    PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions, PdfRedactTextMethod,
    PdfWriteOptions,
};
use mupdf::shape::{FinishOptions, PdfColor, Shape, TextOptions};
use mupdf::{
    Colorspace, Font, Matrix, Outline, Page, Point, Quad, Rect, TextBlockContent, TextPageFlags,
};

use crate::image_handler::{OcrHit, ocr_blocks, ocr_hits};
use crate::image_proc::{decode, encode};
use crate::ocr::{LOW_OCR_CONFIDENCE, OcrResult, OcrSession};

/// Nombre maximal de pages traitées.
pub const MAX_PDF_PAGES: i32 = 5_000;
/// Taille maximale d'un PDF en entrée (octets).
pub const MAX_PDF_BYTES: usize = 512 * 1024 * 1024;
/// Page sans texte dont une image couvre au moins cette fraction : page scannée.
pub const SCANNED_IMAGE_COVERAGE: f32 = 0.5;
/// Dans une page de texte, une image couvrant au moins cette fraction est lue par OCR.
pub const OCR_IMAGE_MIN_PAGE_FRACTION: f32 = 0.08;
/// Contrôle de fuite : toute image dont les deux côtés dépassent ce seuil (pt) est lue par OCR.
pub const DEEP_SCAN_MIN_IMAGE_SIDE_PT: f32 = 12.0;
/// Résolution du rendu envoyé à l'OCR.
pub const OCR_RENDER_DPI: f32 = 300.0;
/// Nombre maximal de pixels d'un rendu de page pour l'OCR.
pub const MAX_RENDER_PIXELS: f32 = 50_000_000.0;
/// Taille de police minimale (pt) d'une balise écrite dans le PDF.
pub const MIN_TAG_FONT_SIZE: f32 = 5.0;
/// Marge (pt) autour des mots OCR caviardés : les boîtes Tesseract sont justes.
pub const OCR_MARGIN_PT: f32 = 2.0;
/// Profondeur maximale de parcours d'un objet PDF (contrôle de fuite).
pub const MAX_OBJECT_DEPTH: usize = 24;
/// Nombre maximal d'objets parcourus par le contrôle de fuite.
pub const MAX_SCAN_OBJECTS: usize = 500_000;

const METADATA_PART: &str = "métadonnées";
const OUTLINE_PART: &str = "signets";
const DOCUMENT_PART: &str = "document";

/// Caviardage réel : texte retiré, pixels d'image effacés, dessins conservés.
const REDACT: PdfRedactOptions = PdfRedactOptions {
    black_boxes: false,
    image_method: PdfRedactImageMethod::Pixels,
    line_art: PdfRedactLineArtMethod::None,
    text: PdfRedactTextMethod::Remove,
};

/// Remplacement d'une image : celles qui touchent la zone sont retirées du fichier.
const REPLACE_IMAGE: PdfRedactOptions = PdfRedactOptions {
    black_boxes: false,
    image_method: PdfRedactImageMethod::Remove,
    line_art: PdfRedactLineArtMethod::None,
    text: PdfRedactTextMethod::Remove,
};

/// Écart minimal (pt) entre une balise qui déborde et le texte suivant.
pub const TAG_OVERFLOW_GAP_PT: f32 = 1.5;

fn page_part(index: i32) -> String {
    format!("page {}", index + 1)
}

/// Erreur MuPDF → `FormatError`, sans le message d'origine (il peut citer le fichier).
fn mu(part: &str) -> impl Fn(mupdf::Error) -> FormatError + '_ {
    move |_| FormatError::corrupt(part, "structure PDF illisible")
}

/// Ouvre un PDF depuis la mémoire. PDF chiffré → `FormatError::Encrypted`.
fn open(input: &[u8]) -> Result<PdfDocument, FormatError> {
    if input.len() > MAX_PDF_BYTES {
        return Err(FormatError::corrupt(
            DOCUMENT_PART,
            "fichier trop volumineux",
        ));
    }
    let head = input.get(..input.len().min(1024)).unwrap_or(input);
    if !head.windows(5).any(|w| w == b"%PDF-") {
        return Err(FormatError::corrupt(DOCUMENT_PART, "en-tête PDF absent"));
    }
    let doc = PdfDocument::from_bytes(input).map_err(mu(DOCUMENT_PART))?;
    if doc.needs_password().map_err(mu(DOCUMENT_PART))? {
        return Err(FormatError::Encrypted);
    }
    let n = doc.page_count().map_err(mu(DOCUMENT_PART))?;
    if !(0..=MAX_PDF_PAGES).contains(&n) {
        return Err(FormatError::corrupt(
            DOCUMENT_PART,
            "nombre de pages hors limites",
        ));
    }
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Texte positionné d'une page
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Glyph {
    rect: Rect,
    /// Espace : n'occupe pas la place libre à droite d'une balise.
    blank: bool,
    baseline: f32,
    size: f32,
    line: usize,
}

/// Bloc de texte d'une page (paragraphe MuPDF), lignes jointes par une espace,
/// espaces repliées ; chaque caractère garde son glyphe.
#[derive(Default)]
struct TextUnit {
    text: String,
    /// (octet de début, glyphe) pour chaque caractère de `text`.
    chars: Vec<(usize, Option<usize>)>,
    glyphs: Vec<Glyph>,
}

/// Morceau d'une occurrence situé sur une seule ligne.
#[derive(Debug, Clone, Copy)]
struct Segment {
    rect: Rect,
    baseline: f32,
    size: f32,
}

fn quad_rect(q: &Quad) -> Rect {
    let xs = [q.ul.x, q.ur.x, q.ll.x, q.lr.x];
    let ys = [q.ul.y, q.ur.y, q.ll.y, q.lr.y];
    Rect::new(
        xs.iter().copied().fold(f32::MAX, f32::min),
        ys.iter().copied().fold(f32::MAX, f32::min),
        xs.iter().copied().fold(f32::MIN, f32::max),
        ys.iter().copied().fold(f32::MIN, f32::max),
    )
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect::new(
        a.x0.min(b.x0),
        a.y0.min(b.y0),
        a.x1.max(b.x1),
        a.y1.max(b.y1),
    )
}

impl TextUnit {
    fn push(&mut self, ch: char, glyph: Option<usize>) {
        let ch = if ch.is_whitespace() { ' ' } else { ch };
        if ch == ' ' && (self.text.is_empty() || self.text.ends_with(' ')) {
            return;
        }
        self.chars.push((self.text.len(), glyph));
        self.text.push(ch);
    }

    fn finish(&mut self) {
        if self.text.ends_with(' ') {
            self.text.pop();
            self.chars.pop();
        }
    }

    /// Tous les blocs d'une page en un seul texte (blocs séparés par une espace) :
    /// une valeur coupée entre deux blocs est retrouvée aussi.
    fn merged(units: &[TextUnit]) -> TextUnit {
        let mut out = TextUnit::default();
        let mut line_base = 0;
        for u in units {
            if !out.text.is_empty() {
                out.push(' ', None);
            }
            let glyph_base = out.glyphs.len();
            let mut max_line = 0;
            for g in &u.glyphs {
                max_line = max_line.max(g.line);
                out.glyphs.push(Glyph {
                    line: g.line + line_base,
                    ..*g
                });
            }
            let byte_base = out.text.len();
            out.text.push_str(&u.text);
            out.chars.extend(
                u.chars
                    .iter()
                    .map(|&(b, g)| (b + byte_base, g.map(|g| g + glyph_base))),
            );
            line_base += max_line + 1;
        }
        out
    }

    /// Rectangles (un par ligne) couvrant les octets `start..end`.
    fn segments(&self, start: usize, end: usize) -> Vec<Segment> {
        let mut out: Vec<(usize, Segment)> = Vec::new();
        for &(b, g) in &self.chars {
            if b < start || b >= end {
                continue;
            }
            let Some(g) = g.and_then(|g| self.glyphs.get(g)) else {
                continue;
            };
            match out.last_mut() {
                Some((line, s)) if *line == g.line => s.rect = union(s.rect, g.rect),
                _ => out.push((
                    g.line,
                    Segment {
                        rect: g.rect,
                        baseline: g.baseline,
                        size: g.size,
                    },
                )),
            }
        }
        out.into_iter().map(|(_, s)| s).collect()
    }
}

struct PageLayout {
    bounds: Rect,
    units: Vec<TextUnit>,
    images: Vec<Rect>,
}

impl PageLayout {
    fn has_text(&self) -> bool {
        self.units
            .iter()
            .any(|u| u.text.chars().any(|c| !c.is_whitespace()))
    }

    fn area(r: &Rect) -> f32 {
        (r.x1 - r.x0).max(0.0) * (r.y1 - r.y0).max(0.0)
    }

    fn clipped(&self, r: &Rect) -> Rect {
        Rect::new(
            r.x0.max(self.bounds.x0),
            r.y0.max(self.bounds.y0),
            r.x1.min(self.bounds.x1),
            r.y1.min(self.bounds.y1),
        )
    }

    fn coverage(&self, r: &Rect) -> f32 {
        let page = Self::area(&self.bounds);
        if page <= 0.0 {
            return 0.0;
        }
        Self::area(&self.clipped(r)) / page
    }

    /// Page scannée : aucun texte, une image couvre l'essentiel.
    fn is_scanned(&self) -> bool {
        !self.has_text()
            && self
                .images
                .iter()
                .any(|r| self.coverage(r) >= SCANNED_IMAGE_COVERAGE)
    }

    /// Zones à lire par OCR. `deep` : toutes les images (contrôle de fuite).
    fn ocr_zones(&self, deep: bool) -> Vec<Rect> {
        if self.is_scanned() {
            return vec![self.bounds];
        }
        self.images
            .iter()
            .map(|r| self.clipped(r))
            .filter(|r| {
                if deep {
                    r.x1 - r.x0 >= DEEP_SCAN_MIN_IMAGE_SIDE_PT
                        && r.y1 - r.y0 >= DEEP_SCAN_MIN_IMAGE_SIDE_PT
                } else {
                    self.coverage(r) >= OCR_IMAGE_MIN_PAGE_FRACTION
                }
            })
            .collect()
    }
}

/// Texte et images d'une page. `with_annotations` : inclure l'apparence des
/// annotations et champs (contrôle de fuite) ; sinon, seulement le contenu.
fn layout(page: &Page, part: &str, with_annotations: bool) -> Result<PageLayout, FormatError> {
    let bounds = page.bounds().map_err(mu(part))?;
    let flags = TextPageFlags::PRESERVE_IMAGES;
    let tp = if with_annotations {
        page.to_text_page(flags).map_err(mu(part))?
    } else {
        page.to_display_list(false)
            .map_err(mu(part))?
            .to_text_page(flags)
            .map_err(mu(part))?
    };
    let st = tp.structured();
    let mut units = Vec::new();
    let mut images = Vec::new();
    for block in &st.blocks {
        match &block.content {
            TextBlockContent::Text { lines } => {
                let mut unit = TextUnit::default();
                for (line_id, line) in lines.iter().enumerate() {
                    if !unit.text.is_empty() {
                        unit.push(' ', None);
                    }
                    for c in &line.chars {
                        let idx = unit.glyphs.len();
                        unit.glyphs.push(Glyph {
                            rect: quad_rect(&c.quad),
                            blank: c.ch.is_whitespace(),
                            baseline: c.origin.y,
                            size: c.size,
                            line: line_id,
                        });
                        unit.push(c.ch, Some(idx));
                    }
                }
                unit.finish();
                if !unit.text.is_empty() {
                    units.push(unit);
                }
            }
            TextBlockContent::Image { .. } => images.push(block.bounds),
            TextBlockContent::Other => {}
        }
    }
    Ok(PageLayout {
        bounds,
        units,
        images,
    })
}

// ---------------------------------------------------------------------------
// OCR d'une page rendue
// ---------------------------------------------------------------------------

/// Page rendue en couleurs : image, échelle (px/pt), origine du pixmap (px).
struct Rendered {
    rgb: RgbImage,
    scale: f32,
    origin: (i32, i32),
}

impl Rendered {
    fn dpi(&self) -> u32 {
        (72.0 * self.scale).round().max(1.0) as u32
    }

    /// Rectangle en pixels (x, y, l, h) d'une zone en pt, `None` si trop petit.
    fn crop_box(&self, z: &Rect) -> Option<(u32, u32, u32, u32)> {
        let (ox, oy) = (i64::from(self.origin.0), i64::from(self.origin.1));
        let (w, h) = (i64::from(self.rgb.width()), i64::from(self.rgb.height()));
        let x0 = ((z.x0 * self.scale).floor() as i64 - ox).clamp(0, w);
        let y0 = ((z.y0 * self.scale).floor() as i64 - oy).clamp(0, h);
        let x1 = ((z.x1 * self.scale).ceil() as i64 - ox).clamp(0, w);
        let y1 = ((z.y1 * self.scale).ceil() as i64 - oy).clamp(0, h);
        if x1 - x0 < 8 || y1 - y0 < 8 {
            return None;
        }
        Some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    fn crop(&self, z: &Rect) -> Option<(RgbImage, u32, u32)> {
        let (cx, cy, cw, ch) = self.crop_box(z)?;
        Some((
            image::imageops::crop_imm(&self.rgb, cx, cy, cw, ch).to_image(),
            cx,
            cy,
        ))
    }

    /// Zone en PNG, à la résolution de l'OCR.
    fn png(&self, z: &Rect) -> Result<Option<Vec<u8>>, FormatError> {
        let Some((img, _, _)) = self.crop(z) else {
            return Ok(None);
        };
        encode(&DynamicImage::ImageRgb8(img), ImageFormat::Png).map(Some)
    }
}

/// Rendu de la page à la résolution de l'OCR (réduite pour les pages énormes).
fn render_page(page: &Page, bounds: &Rect, part: &str) -> Result<Option<Rendered>, FormatError> {
    let (w, h) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
    if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 {
        return Ok(None);
    }
    let mut scale = OCR_RENDER_DPI / 72.0;
    let px = w * h * scale * scale;
    if px > MAX_RENDER_PIXELS {
        scale *= (MAX_RENDER_PIXELS / px).sqrt();
    }
    let pix = page
        .to_pixmap(
            &Matrix::new_scale(scale, scale),
            &Colorspace::device_rgb(),
            false,
            true,
        )
        .map_err(mu(part))?;
    let (pw, ph) = (pix.width(), pix.height());
    if pix.n() != 3 || pw == 0 || ph == 0 {
        return Ok(None);
    }
    let stride =
        usize::try_from(pix.stride()).map_err(|_| FormatError::corrupt(part, "rendu invalide"))?;
    let row_len = pw as usize * 3;
    let samples = pix.samples();
    let mut rgb = RgbImage::new(pw, ph);
    for (y, row) in rgb.chunks_exact_mut(row_len).enumerate() {
        let start = y * stride;
        let Some(src) = samples.get(start..start + row_len) else {
            return Err(FormatError::corrupt(part, "rendu invalide"));
        };
        row.copy_from_slice(src);
    }
    Ok(Some(Rendered {
        rgb,
        scale,
        origin: pix.origin(),
    }))
}

/// OCR de zones (en pt) d'une page rendue ; résultats en coordonnées de page (pt).
fn ocr_rendered(
    session: &OcrSession,
    r: &Rendered,
    zones: &[Rect],
) -> Result<Vec<OcrResult>, FormatError> {
    let mut out = Vec::new();
    for z in zones {
        let Some((crop, cx, cy)) = r.crop(z) else {
            continue;
        };
        let gray = DynamicImage::ImageRgb8(crop).to_luma8();
        let mut res = session.recognize(&gray, r.dpi())?;
        let (ox, oy, scale) = (r.origin.0 as f32, r.origin.1 as f32, r.scale);
        for it in &mut res.items {
            it.x0 = (it.x0 + cx as f32 + ox) / scale;
            it.x1 = (it.x1 + cx as f32 + ox) / scale;
            it.y0 = (it.y0 + cy as f32 + oy) / scale;
            it.y1 = (it.y1 + cy as f32 + oy) / scale;
        }
        out.push(res);
    }
    Ok(out)
}

/// OCR des zones (en pt) d'une page.
fn ocr_page_zones(
    session: &OcrSession,
    page: &Page,
    bounds: &Rect,
    zones: &[Rect],
    part: &str,
) -> Result<Vec<OcrResult>, FormatError> {
    if zones.is_empty() {
        return Ok(Vec::new());
    }
    match render_page(page, bounds, part)? {
        Some(r) => ocr_rendered(session, &r, zones),
        None => Ok(Vec::new()),
    }
}

/// Images exposées d'une page, avec leur nom stable : la page entière si elle
/// est scannée (`page 3`), sinon chaque grande image (`page 3 image 2`).
fn named_zones(lay: &PageLayout, index: i32) -> Vec<(String, Rect)> {
    let part = page_part(index);
    if lay.is_scanned() {
        return vec![(part, lay.bounds)];
    }
    lay.ocr_zones(false)
        .into_iter()
        .enumerate()
        .map(|(k, z)| (format!("{part} image {}", k + 1), z))
        .collect()
}

/// Remplace des zones de la page par des images fournies : les images
/// d'origine qui les touchent sont retirées (pas seulement masquées), le texte
/// de la zone aussi, puis la nouvelle image est posée au même endroit.
fn replace_zones(
    doc: &mut PdfDocument,
    page: &mut PdfPage,
    zones: &[(Rect, Vec<u8>)],
    part: &str,
) -> Result<(), FormatError> {
    let mut pngs = Vec::with_capacity(zones.len());
    for (_, bytes) in zones {
        // Décodage borné, puis PNG propre (sans métadonnées).
        let decoded = decode(bytes)?;
        pngs.push(encode(&decoded.image, ImageFormat::Png)?);
    }
    for (z, _) in zones {
        page.add_redact_annotation(*z).map_err(mu(part))?;
    }
    page.apply_redactions_with_options(REPLACE_IMAGE)
        .map_err(mu(part))?;
    // Rectangles MuPDF (origine en haut) → espace utilisateur PDF.
    let to_pdf = page
        .ctm()
        .ok()
        .and_then(|m| m.invert())
        .ok_or_else(|| FormatError::corrupt(part, "matrice de page invalide"))?;
    for ((z, _), png) in zones.iter().zip(&pngs) {
        page.insert_image(
            doc,
            z.transform(&to_pdf),
            PageImageSource::Bytes {
                data: png,
                format_hint: Some("png"),
            },
            InsertImageOptions::default(),
        )
        .map_err(mu(part))?;
    }
    Ok(())
}

fn center_in(r: &Rect, zones: &[Rect]) -> bool {
    let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    zones
        .iter()
        .any(|z| cx >= z.x0 && cx <= z.x1 && cy >= z.y0 && cy <= z.y1)
}

fn low_confidence(part: &str) -> Warning {
    Warning::new(
        WarningCode::LowOcrConfidence,
        part,
        "Page lue par OCR avec une confiance faible : vérifiez le résultat à l'œil.",
    )
}

/// Session d'OCR ouverte à la demande, une fois par appel.
fn session(slot: &mut Option<OcrSession>) -> Result<&OcrSession, FormatError> {
    if slot.is_none() {
        *slot = Some(OcrSession::begin()?);
    }
    slot.as_ref()
        .ok_or_else(|| FormatError::Other("échec de l'OCR".into()))
}

// ---------------------------------------------------------------------------
// Métadonnées, annotations, signets, formulaires
// ---------------------------------------------------------------------------

fn info_dict(doc: &PdfDocument) -> Option<PdfObject> {
    let info = doc.trailer().ok()?.get_dict("Info").ok()??;
    let info = info.resolve().ok()??;
    info.is_dict().ok()?.then_some(info)
}

fn info_strings(doc: &PdfDocument) -> Vec<(Vec<u8>, String)> {
    let Some(info) = info_dict(doc) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let n = info.dict_len().unwrap_or(0);
    for i in 0..n {
        let Ok(idx) = i32::try_from(i) else { break };
        let (Ok(Some(k)), Ok(Some(v))) = (info.get_dict_key(idx), info.get_dict_val(idx)) else {
            continue;
        };
        let Ok(key) = k.as_name() else { continue };
        if matches!(key.as_slice(), b"CreationDate" | b"ModDate" | b"Trapped") {
            continue;
        }
        let v = v.resolve().ok().flatten().unwrap_or(v);
        if v.is_string().unwrap_or(false)
            && let Ok(s) = v.as_string()
            && !s.trim().is_empty()
        {
            out.push((key, s));
        }
    }
    out
}

fn xmp_text(doc: &PdfDocument) -> Option<String> {
    let md = doc.catalog().ok()?.get_dict("Metadata").ok()??;
    // `is_stream` ne vaut que sur la référence indirecte, pas sur l'objet résolu.
    if !md.is_stream().ok()? {
        return None;
    }
    let bytes = md.read_stream().ok()?;
    let s = String::from_utf8_lossy(&bytes).into_owned();
    (!s.trim().is_empty()).then_some(s)
}

fn outline_titles(items: &[Outline], out: &mut Vec<String>, depth: usize) {
    if depth > MAX_OBJECT_DEPTH {
        return;
    }
    for o in items {
        if !o.title.trim().is_empty() {
            out.push(o.title.clone());
        }
        outline_titles(&o.down, out, depth + 1);
    }
}

/// Applique `tr` aux titres ; renvoie le nombre de remplacements.
fn transform_outlines(items: &mut [Outline], tr: &dyn TextTransform, depth: usize) -> usize {
    if depth > MAX_OBJECT_DEPTH {
        return 0;
    }
    let ctx = BlockContext::new(OUTLINE_PART, BlockRole::Other);
    let mut n = 0;
    for o in items {
        let reps = tr.replacements(&o.title, &ctx);
        if !reps.is_empty() {
            n += reps.len();
            o.title = anon_core::text::apply_replacements(&o.title, &reps).into_owned();
        }
        n += transform_outlines(&mut o.down, tr, depth + 1);
    }
    n
}

/// URI des annotations de lien d'une page (`/Annots` → `/A` → `/URI`).
fn link_uri_objects(page: &PdfPage) -> Vec<PdfObject> {
    let mut out = Vec::new();
    let Ok(Some(annots)) = page.object().get_dict("Annots") else {
        return out;
    };
    let annots = annots.resolve().ok().flatten().unwrap_or(annots);
    let n = annots.len().unwrap_or(0);
    for i in 0..n {
        let Ok(idx) = i32::try_from(i) else { break };
        let Ok(Some(a)) = annots.get_array(idx) else {
            continue;
        };
        let Some(a) = a.resolve().ok().flatten() else {
            continue;
        };
        let is_link = a
            .get_dict("Subtype")
            .ok()
            .flatten()
            .and_then(|s| s.as_name().ok())
            .is_some_and(|n| n == b"Link");
        if !is_link {
            continue;
        }
        if let Ok(Some(action)) = a.get_dict("A")
            && let Some(action) = action.resolve().ok().flatten()
            && action.get_dict("URI").ok().flatten().is_some()
        {
            out.push(action);
        }
    }
    out
}

fn uri_of(action: &PdfObject) -> Option<String> {
    let v = action.get_dict("URI").ok()??;
    let v = v.resolve().ok().flatten().unwrap_or(v);
    let bytes = v.as_bytes().ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Blocs « annexes » d'une page : annotations, liens, champs.
fn page_side_blocks(page: &PdfPage, part: &str, blocks: &mut Vec<TextBlock>) {
    for annot in page.annotations() {
        if let Ok(Some(c)) = annot.contents()
            && !c.trim().is_empty()
        {
            blocks.push(TextBlock {
                text: c.to_string(),
                ctx: BlockContext::new(part, BlockRole::Comment),
            });
        }
        if let Ok(Some(a)) = annot.author()
            && !a.trim().is_empty()
        {
            blocks.push(TextBlock {
                text: a.to_string(),
                ctx: BlockContext::new(part, BlockRole::Metadata),
            });
        }
    }
    for action in link_uri_objects(page) {
        if let Some(uri) = uri_of(&action) {
            blocks.push(TextBlock {
                text: uri,
                ctx: BlockContext::new(part, BlockRole::Hyperlink),
            });
        }
    }
    for w in page.widgets() {
        if let Ok(Some(v)) = w.value()
            && !v.trim().is_empty()
        {
            blocks.push(TextBlock {
                text: v,
                ctx: BlockContext::new(part, BlockRole::Other),
            });
        }
    }
}

fn embedded_file_names(doc: &PdfDocument) -> Vec<String> {
    doc.embedded_files()
        .map(|v| v.into_iter().map(|f| f.name).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Contrôle de fuite : chaînes de tous les objets
// ---------------------------------------------------------------------------

fn collect_strings(obj: &PdfObject, depth: usize, out: &mut Vec<String>, budget: &mut usize) {
    if depth > MAX_OBJECT_DEPTH || *budget == 0 {
        return;
    }
    *budget -= 1;
    // Les références indirectes sont parcourues par leur propre numéro d'objet.
    if obj.is_indirect().unwrap_or(true) {
        return;
    }
    if obj.is_string().unwrap_or(false) {
        if let Ok(s) = obj.as_string()
            && !s.trim().is_empty()
        {
            out.push(s);
        }
        return;
    }
    if obj.is_array().unwrap_or(false) {
        let n = obj.len().unwrap_or(0);
        for i in 0..n {
            let Ok(idx) = i32::try_from(i) else { break };
            if let Ok(Some(v)) = obj.get_array(idx) {
                collect_strings(&v, depth + 1, out, budget);
            }
        }
    } else if obj.is_dict().unwrap_or(false) {
        let n = obj.dict_len().unwrap_or(0);
        for i in 0..n {
            let Ok(idx) = i32::try_from(i) else { break };
            if let Ok(Some(v)) = obj.get_dict_val(idx) {
                collect_strings(&v, depth + 1, out, budget);
            }
        }
    }
}

fn is_name(obj: &PdfObject, key: &str, value: &[u8]) -> bool {
    obj.get_dict(key)
        .ok()
        .flatten()
        .and_then(|v| v.as_name().ok())
        .is_some_and(|n| n == value)
}

fn object_blocks(doc: &PdfDocument) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    let n = doc.xref_len().unwrap_or(0);
    let mut budget = MAX_SCAN_OBJECTS;
    for num in 1..n {
        if budget == 0 {
            break;
        }
        let Ok(num) = i32::try_from(num) else { break };
        let Ok(Some(obj)) = doc.xref_object(num) else {
            continue;
        };
        let part = format!("objet {num}");
        let mut strings = Vec::new();
        collect_strings(&obj, 0, &mut strings, &mut budget);
        for s in strings {
            blocks.push(TextBlock {
                text: s,
                ctx: BlockContext::new(part.clone(), BlockRole::Other),
            });
        }
        // Flux XML (XMP) et fichiers joints : texte brut.
        let stream = doc.new_indirect(num, 0).ok();
        if let Some(stream) = stream
            && stream.is_stream().unwrap_or(false)
            && (is_name(&obj, "Type", b"Metadata")
                || is_name(&obj, "Subtype", b"XML")
                || is_name(&obj, "Type", b"EmbeddedFile"))
            && let Ok(bytes) = stream.read_stream()
        {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            if !s.trim().is_empty() {
                blocks.push(TextBlock {
                    text: s,
                    ctx: BlockContext::new(part, BlockRole::Metadata),
                });
            }
        }
    }
    blocks
}

// ---------------------------------------------------------------------------
// Caviardage et balises
// ---------------------------------------------------------------------------

/// Occurrence à caviarder : morceaux par ligne, balise, fond blanc (zones d'image).
struct Placement {
    segments: Vec<Segment>,
    tag: String,
    white_box: bool,
}

fn width_1pt(font: &Font, s: &str) -> f32 {
    let mut w = 0.0;
    for c in s.chars() {
        let Ok(code) = i32::try_from(u32::from(c)) else {
            continue;
        };
        if let Ok(g) = font.encode_character(code)
            && let Ok(a) = font.advance_glyph(g)
        {
            w += a;
        }
    }
    w
}

fn ocr_placements(results: &[OcrResult], tr: &dyn TextTransform, part: &str) -> Vec<Placement> {
    let mut out = Vec::new();
    for res in results {
        let hits: Vec<OcrHit> = ocr_hits(res, tr, part);
        for hit in hits {
            let segments = hit
                .rects
                .iter()
                .map(|r| {
                    let rect = Rect::new(
                        r[0] - OCR_MARGIN_PT,
                        r[1] - OCR_MARGIN_PT,
                        r[2] + OCR_MARGIN_PT,
                        r[3] + OCR_MARGIN_PT,
                    );
                    let h = rect.y1 - rect.y0;
                    Segment {
                        rect,
                        baseline: rect.y1 - h * 0.25,
                        size: h * 0.7,
                    }
                })
                .collect();
            out.push(Placement {
                segments,
                tag: hit.label,
                white_box: true,
            });
        }
    }
    out
}

/// Morceau où écrire la balise d'une occurrence : le plus large.
fn tag_segment(p: &Placement) -> Option<&Segment> {
    p.segments
        .iter()
        .max_by(|a, b| (a.rect.x1 - a.rect.x0).total_cmp(&(b.rect.x1 - b.rect.x0)))
}

/// Abscisse jusqu'où une balise posée sur `seg` peut déborder à droite sans
/// recouvrir de texte : premier glyphe conservé (ou balise voisine) sur la même
/// ligne, sinon le bord de la page.
fn free_right(seg: &Segment, others: &[Rect], glyphs: &[Rect], page_right: f32) -> f32 {
    let h = (seg.rect.y1 - seg.rect.y0).max(0.1);
    let same_line = |r: &Rect| {
        let overlap = seg.rect.y1.min(r.y1) - seg.rect.y0.max(r.y0);
        overlap > 0.5 * h.min((r.y1 - r.y0).max(0.1))
    };
    let mut limit = page_right;
    for r in glyphs.iter().chain(others) {
        if r.x0 >= seg.rect.x1 - 0.5 && same_line(r) {
            limit = limit.min(r.x0);
        }
    }
    limit
}

/// Pose les zones de caviardage, les applique, puis écrit les balises.
/// `glyphs` : glyphes visibles de la page (avant caviardage), pour savoir si
/// une balise peut déborder. Renvoie le nombre de balises écrites à la taille minimale.
fn redact_and_tag(
    doc: &mut PdfDocument,
    page: &mut PdfPage,
    plan: &[Placement],
    glyphs: &[Rect],
    page_right: f32,
    part: &str,
) -> Result<usize, FormatError> {
    if plan.is_empty() {
        return Ok(0);
    }
    // Glyphes conservés : ceux qui ne sont pas sous une zone caviardée.
    let redacted: Vec<Rect> = plan
        .iter()
        .flat_map(|p| p.segments.iter().map(|s| s.rect))
        .collect();
    let kept: Vec<Rect> = glyphs
        .iter()
        .filter(|g| !center_in(g, &redacted))
        .copied()
        .collect();
    let starts: Vec<Rect> = plan
        .iter()
        .filter_map(tag_segment)
        .map(|s| s.rect)
        .collect();
    for p in plan {
        for s in &p.segments {
            page.add_redact_annotation(s.rect).map_err(mu(part))?;
        }
    }
    page.apply_redactions_with_options(REDACT)
        .map_err(mu(part))?;

    let font = Font::new("Helvetica").map_err(mu(part))?;
    let mut too_small = 0;
    let mut shape = Shape::new(page).map_err(mu(part))?;
    for p in plan {
        if p.white_box {
            for s in &p.segments {
                shape
                    .draw_rect(&s.rect)
                    .and_then(|sh| {
                        sh.finish(&FinishOptions {
                            color: None,
                            fill: Some(PdfColor::rgb(1.0, 1.0, 1.0)),
                            ..Default::default()
                        })
                    })
                    .map_err(mu(part))?;
            }
        }
        let Some(seg) = tag_segment(p) else {
            continue;
        };
        if p.tag.is_empty() {
            continue;
        }
        let w1 = width_1pt(&font, &p.tag);
        let mut avail = seg.rect.x1 - seg.rect.x0;
        if !p.white_box {
            // Texte natif : la balise peut déborder sur la place libre à droite.
            let limit = free_right(seg, &starts, &kept, page_right);
            avail = avail.max(limit - seg.rect.x0 - TAG_OVERFLOW_GAP_PT);
        }
        let orig = if seg.size.is_finite() && seg.size > 0.0 {
            seg.size
        } else {
            MIN_TAG_FONT_SIZE
        };
        let mut size = if w1 > 0.0 { orig.min(avail / w1) } else { orig };
        if !size.is_finite() || size < MIN_TAG_FONT_SIZE {
            size = MIN_TAG_FONT_SIZE;
            too_small += 1;
        }
        shape
            .insert_text(
                Point::new(seg.rect.x0, seg.baseline),
                &p.tag,
                &TextOptions {
                    fontsize: size,
                    fontname: "helv".into(),
                    fill: Some(PdfColor::rgb(0.0, 0.0, 0.0)),
                    ..Default::default()
                },
            )
            .map_err(mu(part))?;
    }
    shape.commit(doc, true).map_err(mu(part))?;
    Ok(too_small)
}

fn apply_text<'a>(
    tr: &dyn TextTransform,
    text: &'a str,
    ctx: &BlockContext,
) -> (Cow<'a, str>, usize) {
    let reps = tr.replacements(text, ctx);
    let n = reps.len();
    (anon_core::text::apply_replacements(text, &reps), n)
}

fn save(doc: &PdfDocument) -> Result<Vec<u8>, FormatError> {
    let mut o = PdfWriteOptions::default();
    o.set_garbage_level(4)
        .set_clean(true)
        .set_compress(true)
        .set_incremental(false)
        .set_encryption(mupdf::pdf::Encryption::None);
    let mut out = Vec::new();
    doc.write_to_with_options(&mut out, o)
        .map_err(|_| FormatError::Other("enregistrement du PDF impossible".into()))?;
    Ok(out)
}

/// Supprime les métadonnées XMP d'un objet (catalogue, page…).
fn drop_metadata_key(obj: &mut PdfObject) {
    if obj.get_dict("Metadata").ok().flatten().is_some() {
        let _ = obj.dict_delete("Metadata");
    }
}

// ---------------------------------------------------------------------------
// Gestionnaire
// ---------------------------------------------------------------------------

/// PDF : texte, pages scannées (OCR), métadonnées, annotations, signets, champs.
#[derive(Debug, Clone, Copy, Default)]
pub struct PdfHandler;

pub(crate) const PDF_EXTENSIONS: &[&str] = &["pdf"];

impl PdfHandler {
    fn page_blocks(
        doc: &PdfDocument,
        index: i32,
        ocr: &mut Option<OcrSession>,
        deep: bool,
        blocks: &mut Vec<TextBlock>,
        media: &mut Vec<MediaItem>,
    ) -> Result<(), FormatError> {
        let part = page_part(index);
        let page = doc.load_pdf_page(index).map_err(mu(&part))?;
        let lay = layout(&page, &part, deep)?;
        for u in &lay.units {
            blocks.push(TextBlock {
                text: u.text.clone(),
                ctx: BlockContext::new(part.clone(), BlockRole::Body),
            });
        }
        if deep && lay.units.len() > 1 {
            // Contrôle de fuite : aussi la page d'un seul tenant (valeur coupée entre blocs).
            blocks.push(TextBlock {
                text: TextUnit::merged(&lay.units).text,
                ctx: BlockContext::new(part.clone(), BlockRole::Body),
            });
        }
        if deep {
            // Contrôle de fuite : OCR de toutes les images.
            let zones = lay.ocr_zones(true);
            if !zones.is_empty() {
                let s = session(ocr)?;
                for r in ocr_page_zones(s, &page, &lay.bounds, &zones, &part)? {
                    blocks.extend(ocr_blocks(&r, &part));
                }
            }
            return Ok(());
        }
        // Extraction : pages scannées et grandes images exposées comme images
        // (le moteur les lit par OCR et l'utilisateur peut y tracer des zones).
        let named = named_zones(&lay, index);
        if !named.is_empty()
            && let Some(r) = render_page(&page, &lay.bounds, &part)?
        {
            for (name, z) in named {
                if let Some(bytes) = r.png(&z)? {
                    media.push(MediaItem { name, bytes });
                }
            }
        }
        page_side_blocks(&page, &part, blocks);
        Ok(())
    }
}

impl Handler for PdfHandler {
    fn extensions(&self) -> &'static [&'static str] {
        PDF_EXTENSIONS
    }

    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        let doc = open(input)?;
        let mut blocks = Vec::new();
        let mut warnings = Vec::new();
        for (key, value) in info_strings(&doc) {
            let _ = key;
            blocks.push(TextBlock {
                text: value,
                ctx: BlockContext::new(METADATA_PART, BlockRole::Metadata),
            });
        }
        if let Some(x) = xmp_text(&doc) {
            blocks.push(TextBlock {
                text: x,
                ctx: BlockContext::new(METADATA_PART, BlockRole::Metadata),
            });
        }
        if let Ok(outlines) = doc.outlines() {
            let mut titles = Vec::new();
            outline_titles(&outlines, &mut titles, 0);
            blocks.extend(titles.into_iter().map(|t| TextBlock {
                text: t,
                ctx: BlockContext::new(OUTLINE_PART, BlockRole::Other),
            }));
        }
        let n = doc.page_count().map_err(mu(DOCUMENT_PART))?;
        let mut ocr = None;
        let mut media = Vec::new();
        for i in 0..n {
            Self::page_blocks(&doc, i, &mut ocr, false, &mut blocks, &mut media)?;
        }
        if !embedded_file_names(&doc).is_empty() {
            warnings.push(Warning::new(
                WarningCode::EmbeddedBinary,
                DOCUMENT_PART,
                "Fichiers joints non analysés ; ils seront retirés à l'anonymisation.",
            ));
        }
        Ok(Extraction {
            blocks,
            media,
            warnings,
        })
    }

    fn deep_scan(&self, input: &[u8]) -> Result<Vec<TextBlock>, FormatError> {
        let doc = open(input)?;
        let mut blocks = Vec::new();
        let mut media = Vec::new();
        let n = doc.page_count().map_err(mu(DOCUMENT_PART))?;
        let mut ocr = None;
        for i in 0..n {
            Self::page_blocks(&doc, i, &mut ocr, true, &mut blocks, &mut media)?;
            let part = page_part(i);
            if let Ok(page) = doc.load_pdf_page(i) {
                page_side_blocks(&page, &part, &mut blocks);
            }
        }
        if let Ok(outlines) = doc.outlines() {
            let mut titles = Vec::new();
            outline_titles(&outlines, &mut titles, 0);
            blocks.extend(titles.into_iter().map(|t| TextBlock {
                text: t,
                ctx: BlockContext::new(OUTLINE_PART, BlockRole::Other),
            }));
        }
        blocks.extend(object_blocks(&doc));
        Ok(blocks)
    }

    fn transform(
        &self,
        input: &[u8],
        tr: &dyn TextTransform,
        opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        let mut doc = open(input)?;
        let mut count = 0usize;
        let mut warnings = Vec::new();
        let mut ocr: Option<OcrSession> = None;
        let n = doc.page_count().map_err(mu(DOCUMENT_PART))?;
        let mut removed_files = 0usize;
        let mut untouched = 0usize;

        for i in 0..n {
            let part = page_part(i);
            let mut page = doc.load_pdf_page(i).map_err(mu(&part))?;
            let lay = layout(&page, &part, false)?;

            // 1. Texte natif : remplacements → zones par ligne.
            let ctx = BlockContext::new(part.clone(), BlockRole::Body);
            let mut plan = Vec::new();
            let page_text = TextUnit::merged(&lay.units);
            {
                let u = &page_text;
                for rep in tr.replacements(&u.text, &ctx) {
                    let segments = u.segments(rep.start, rep.end);
                    if segments.is_empty() {
                        continue;
                    }
                    count += 1;
                    plan.push(Placement {
                        segments,
                        tag: rep.with,
                        white_box: false,
                    });
                }
            }
            // 2. Pages scannées et grandes images : image fournie par
            //    `opts.media` (masquée par le moteur), sinon mots de l'OCR.
            let named = named_zones(&lay, i);
            let mut replaced: Vec<(Rect, Vec<u8>)> = Vec::new();
            if !named.is_empty()
                && let Some(r) = render_page(&page, &lay.bounds, &part)?
            {
                let mut internal = Vec::new();
                for (name, z) in named {
                    let new = match opts.media {
                        Some(m) => r.png(&z)?.and_then(|png| m.apply(&name, &png)),
                        None => None,
                    };
                    match new {
                        Some(bytes) => replaced.push((z, bytes)),
                        None => internal.push(z),
                    }
                }
                if !internal.is_empty() {
                    let s = session(&mut ocr)?;
                    let results = ocr_rendered(s, &r, &internal)?;
                    if results
                        .iter()
                        .any(|r| !r.items.is_empty() && r.mean_confidence() < LOW_OCR_CONFIDENCE)
                    {
                        warnings.push(low_confidence(&part));
                    }
                    let more = ocr_placements(&results, tr, &part);
                    count += more.len();
                    plan.extend(more);
                }
            }
            if !replaced.is_empty() {
                // Le texte natif sous une image remplacée part avec la zone :
                // pas de balise par-dessus l'image déjà masquée.
                let zones: Vec<Rect> = replaced.iter().map(|(z, _)| *z).collect();
                plan.retain(|p| !p.segments.iter().all(|s| center_in(&s.rect, &zones)));
                replace_zones(&mut doc, &mut page, &replaced, &part)?;
            }
            // 3. Caviardage réel puis balises.
            let glyphs: Vec<Rect> = lay
                .units
                .iter()
                .flat_map(|u| u.glyphs.iter().filter(|g| !g.blank).map(|g| g.rect))
                .collect();
            let small = redact_and_tag(&mut doc, &mut page, &plan, &glyphs, lay.bounds.x1, &part)?;
            if small > 0 {
                warnings.push(Warning::new(
                    WarningCode::TagDidNotFit,
                    part.clone(),
                    format!("{small} balise(s) écrite(s) à la taille minimale, plus large(s) que la zone d'origine."),
                ));
            }

            // 4. Annotations : contenu passé par `tr`, auteurs vidés, fichiers joints retirés.
            let annots: Vec<_> = page.annotations().collect();
            for mut annot in annots {
                let kind = annot.r#type().ok();
                if opts.scrub_metadata && kind == Some(PdfAnnotationType::FileAttachment) {
                    if page.delete_annotation(annot).is_ok() {
                        removed_files += 1;
                    } else {
                        untouched += 1;
                    }
                    continue;
                }
                let mut changed = false;
                let contents = annot.contents().ok().flatten().map(str::to_owned);
                if let Some(c) = contents {
                    let (new, k) =
                        apply_text(tr, &c, &BlockContext::new(part.clone(), BlockRole::Comment));
                    if k > 0 {
                        if annot.set_contents(&new).is_ok() {
                            count += k;
                            changed = true;
                        } else {
                            untouched += 1;
                        }
                    }
                }
                let author = annot.author().ok().flatten().map(str::to_owned);
                if let Some(a) = author {
                    if opts.scrub_metadata {
                        if !a.is_empty() && annot.set_author("").is_ok() {
                            changed = true;
                        }
                    } else {
                        let (new, k) = apply_text(
                            tr,
                            &a,
                            &BlockContext::new(part.clone(), BlockRole::Metadata),
                        );
                        if k > 0 && annot.set_author(&new).is_ok() {
                            count += k;
                            changed = true;
                        }
                    }
                }
                if changed {
                    let _ = annot.update();
                }
            }
            // 5. Liens : URI.
            for mut action in link_uri_objects(&page) {
                let Some(uri) = uri_of(&action) else { continue };
                let new = tr.apply_uri(&uri);
                if new != uri.as_str() {
                    match doc.new_string(&new).and_then(|s| action.dict_put("URI", s)) {
                        Ok(()) => count += 1,
                        Err(_) => untouched += 1,
                    }
                }
            }
            // 6. Champs de formulaire.
            let widgets: Vec<_> = page.widgets().collect();
            for mut w in widgets {
                let Some(v) = w.value().ok().flatten() else {
                    continue;
                };
                let (new, k) =
                    apply_text(tr, &v, &BlockContext::new(part.clone(), BlockRole::Other));
                if k == 0 {
                    continue;
                }
                let new = new.into_owned();
                match w.set_value(&mut doc, &new, true) {
                    Ok(_) => {
                        count += k;
                        let _ = w.update();
                    }
                    Err(_) => untouched += 1,
                }
            }
            if opts.scrub_metadata {
                let mut obj = page.object();
                drop_metadata_key(&mut obj);
            }
        }

        // Signets.
        if let Ok(mut outlines) = doc.outlines() {
            let k = transform_outlines(&mut outlines, tr, 0);
            if k > 0 {
                match doc.set_outlines(&outlines) {
                    Ok(()) => count += k,
                    Err(_) => untouched += 1,
                }
            }
        }

        // Métadonnées du document.
        if opts.scrub_metadata {
            if let Ok(mut trailer) = doc.trailer()
                && trailer.get_dict("Info").ok().flatten().is_some()
            {
                trailer.dict_delete("Info").map_err(mu(METADATA_PART))?;
            }
            if let Ok(mut catalog) = doc.catalog() {
                drop_metadata_key(&mut catalog);
                // Pièces jointes au niveau du document.
                for name in embedded_file_names(&doc) {
                    match doc.delete_embedded_file(&name) {
                        Ok(true) => removed_files += 1,
                        Ok(false) => {}
                        Err(_) => untouched += 1,
                    }
                }
            }
        } else if let Some(mut info) = info_dict(&doc) {
            for (key, value) in info_strings(&doc) {
                let (new, k) = apply_text(
                    tr,
                    &value,
                    &BlockContext::new(METADATA_PART, BlockRole::Metadata),
                );
                if k == 0 {
                    continue;
                }
                let Ok(key) = String::from_utf8(key) else {
                    continue;
                };
                match doc
                    .new_string(&new)
                    .and_then(|s| info.dict_put(key.as_str(), s))
                {
                    Ok(()) => count += k,
                    Err(_) => untouched += 1,
                }
            }
        }

        if removed_files > 0 {
            warnings.push(Warning::new(
                WarningCode::EmbeddedBinary,
                DOCUMENT_PART,
                format!("{removed_files} fichier(s) joint(s) retiré(s) sans analyse."),
            ));
        }
        if untouched > 0 {
            warnings.push(Warning::new(
                WarningCode::UntouchedLocation,
                DOCUMENT_PART,
                format!("{untouched} emplacement(s) non modifiable(s) (annotation, lien, champ ou signet)."),
            ));
        }
        drop(ocr);
        let bytes = save(&doc)?;
        Ok(Output {
            bytes,
            replacements: count,
            warnings,
        })
    }
}

/// Utilitaire de test : octets du PDF réécrit sans compression (flux décompressés).
pub fn decompressed_bytes(input: &[u8]) -> Result<Vec<u8>, FormatError> {
    let doc = open(input)?;
    let mut o = PdfWriteOptions::default();
    o.set_decompress(true)
        .set_encryption(mupdf::pdf::Encryption::None);
    let mut v = Vec::new();
    doc.write_to_with_options(&mut v, o)
        .map_err(|_| FormatError::Other("réécriture du PDF impossible".into()))?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(lines: &[&str], line_offset: usize) -> TextUnit {
        let mut u = TextUnit::default();
        for (l, text) in lines.iter().enumerate() {
            if !u.text.is_empty() {
                u.push(' ', None);
            }
            for (i, c) in text.chars().enumerate() {
                let idx = u.glyphs.len();
                let x = i as f32 * 10.0;
                let y = (l + line_offset) as f32 * 20.0;
                u.glyphs.push(Glyph {
                    rect: Rect::new(x, y, x + 10.0, y + 12.0),
                    blank: c.is_whitespace(),
                    baseline: y + 10.0,
                    size: 12.0,
                    line: l,
                });
                u.push(c, Some(idx));
            }
        }
        u.finish();
        u
    }

    #[test]
    fn espaces_repliees_et_segments_par_ligne() {
        let u = unit(&["Le  signataire, Jean", "Dupont, confirme. "], 0);
        assert_eq!(u.text, "Le signataire, Jean Dupont, confirme.");
        let start = u.text.find("Jean").unwrap();
        let segs = u.segments(start, start + "Jean Dupont".len());
        assert_eq!(segs.len(), 2, "un rectangle par ligne");
        assert!(segs[0].rect.y0 < segs[1].rect.y0);
    }

    #[test]
    fn blocs_fusionnes() {
        let a = unit(&["Contact : Jean"], 0);
        let b = unit(&["Dupont, ici."], 1);
        let m = TextUnit::merged(&[a, b]);
        assert_eq!(m.text, "Contact : Jean Dupont, ici.");
        let start = m.text.find("Jean").unwrap();
        let segs = m.segments(start, start + "Jean Dupont".len());
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn en_tete_absent_ou_vide() {
        assert!(open(b"").is_err());
        assert!(open(b"pas un pdf").is_err());
    }
}
