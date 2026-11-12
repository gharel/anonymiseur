//! Décodage sûr des images, masquage avec balise, ré-encodage sans EXIF, et
//! `MupdfImageProcessor` (contrat `anon_core::ImageProcessor`).

use std::io::Cursor;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anon_core::{FormatError, ImageProcessor, MaskRegion, OcrPage};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, Rgba, RgbaImage};

use crate::ocr::{DEFAULT_IMAGE_DPI, OcrResult, OcrSession};

/// Plus grand côté accepté (pixels).
pub const MAX_IMAGE_SIDE: u32 = 20_000;
/// Nombre maximal de pixels d'une image décodée.
pub const MAX_IMAGE_PIXELS: u64 = 80_000_000;
/// Mémoire maximale allouée par le décodeur (octets).
pub const MAX_DECODE_ALLOC: u64 = 1024 * 1024 * 1024;
/// Taille minimale (pixels) d'une balise écrite dans une image ; en dessous, rien n'est écrit.
pub const MIN_LABEL_PX: f32 = 7.0;
/// Hauteur de la balise par rapport à celle de la zone.
pub const LABEL_HEIGHT_RATIO: f32 = 0.8;
/// Couleur de l'aplat de masquage.
pub const MASK_FILL: [u8; 4] = [230, 230, 230, 255];
/// Couleur du texte de la balise.
pub const LABEL_INK: [u8; 3] = [0, 0, 0];
/// Qualité JPEG du ré-encodage.
pub const JPEG_QUALITY: u8 = 92;

static LABEL_FONT: &[u8] = include_bytes!("../../../assets/charte/fonts/Georama-600.ttf");

const PART: &str = "image";

fn corrupt(reason: &str) -> FormatError {
    FormatError::corrupt(PART, reason)
}

/// Image décodée, orientation EXIF appliquée, et son format d'origine.
pub(crate) struct Decoded {
    pub image: DynamicImage,
    pub format: ImageFormat,
}

pub(crate) fn supported_format(f: ImageFormat) -> bool {
    matches!(
        f,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
            | ImageFormat::WebP
    )
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Decoded, FormatError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| corrupt("lecture impossible"))?;
    let format = reader
        .format()
        .ok_or_else(|| FormatError::Unsupported("format d'image inconnu".into()))?;
    if !supported_format(format) {
        return Err(FormatError::Unsupported(
            "format d'image non pris en charge".into(),
        ));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let mut reader = reader;
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| corrupt("en-tête illisible"))?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err(corrupt("image vide"));
    }
    if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS {
        return Err(corrupt("dimensions trop grandes"));
    }
    let orientation = decoder.orientation().ok();
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|_| corrupt("données illisibles"))?;
    if let Some(o) = orientation {
        image.apply_orientation(o);
    }
    Ok(Decoded { image, format })
}

/// Ré-encode dans `format`, sans métadonnées (EXIF, XMP, commentaires).
pub(crate) fn encode(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, FormatError> {
    let has_alpha = image.color().has_alpha();
    let is_gray = !image.color().has_color();
    let normalized: DynamicImage = match format {
        ImageFormat::Jpeg => {
            if is_gray {
                DynamicImage::ImageLuma8(image.to_luma8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            }
        }
        ImageFormat::Png | ImageFormat::Tiff => match image {
            DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageRgb8(_)
            | DynamicImage::ImageRgba8(_)
            | DynamicImage::ImageLuma16(_)
            | DynamicImage::ImageRgb16(_)
            | DynamicImage::ImageRgba16(_) => image.clone(),
            _ if has_alpha => DynamicImage::ImageRgba8(image.to_rgba8()),
            _ => DynamicImage::ImageRgb8(image.to_rgb8()),
        },
        ImageFormat::Gif => DynamicImage::ImageRgba8(image.to_rgba8()),
        _ => {
            if has_alpha {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            }
        }
    };
    let mut out = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
            normalized
                .write_with_encoder(enc)
                .map_err(|_| FormatError::Other("encodage JPEG impossible".into()))?;
        }
        _ => normalized
            .write_to(&mut out, format)
            .map_err(|_| FormatError::Other("encodage de l'image impossible".into()))?,
    }
    Ok(out.into_inner())
}

/// Recouvre chaque zone d'un aplat opaque et y écrit la balise si elle tient.
pub(crate) fn paint_regions(image: &DynamicImage, regions: &[MaskRegion]) -> DynamicImage {
    if regions.is_empty() {
        return image.clone();
    }
    let mut rgba: RgbaImage = image.to_rgba8();
    let font = FontRef::try_from_slice(LABEL_FONT).ok();
    let (iw, ih) = rgba.dimensions();
    for region in regions {
        let x0 = region.rect.x.min(iw);
        let y0 = region.rect.y.min(ih);
        let x1 = region.rect.x.saturating_add(region.rect.w).min(iw);
        let y1 = region.rect.y.saturating_add(region.rect.h).min(ih);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                rgba.put_pixel(x, y, Rgba(MASK_FILL));
            }
        }
        if let (Some(label), Some(font)) = (region.label.as_deref(), font.as_ref()) {
            draw_label(&mut rgba, font, label, x0, y0, x1, y1);
        }
    }
    // Retour au type de couleur d'origine (8 bits).
    let out = DynamicImage::ImageRgba8(rgba);
    match image.color() {
        image::ColorType::L8 | image::ColorType::L16 => DynamicImage::ImageLuma8(out.to_luma8()),
        image::ColorType::La8 | image::ColorType::La16 => {
            DynamicImage::ImageLumaA8(out.to_luma_alpha8())
        }
        c if c.has_alpha() => out,
        _ => DynamicImage::ImageRgb8(out.to_rgb8()),
    }
}

/// Largeur d'une chaîne pour une taille de 1 px.
fn text_width_1px(font: &FontRef<'_>, text: &str) -> f32 {
    let scaled = font.as_scaled(PxScale::from(1.0));
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let g = scaled.glyph_id(c);
        if let Some(p) = prev {
            w += scaled.kern(p, g);
        }
        w += scaled.h_advance(g);
        prev = Some(g);
    }
    w
}

/// Taille (px) de la balise pour une zone `w` × `h`, `None` si elle ne tient pas.
pub(crate) fn label_size(text: &str, w: f32, h: f32) -> Option<f32> {
    let font = FontRef::try_from_slice(LABEL_FONT).ok()?;
    fit_size(&font, text, w, h)
}

fn fit_size(font: &FontRef<'_>, text: &str, w: f32, h: f32) -> Option<f32> {
    let unit = text_width_1px(font, text);
    if unit <= 0.0 {
        return None;
    }
    // Hauteur visuelle ≈ ascendante - descendante à 1 px.
    let scaled = font.as_scaled(PxScale::from(1.0));
    let line = (scaled.ascent() - scaled.descent()).max(0.1);
    let by_height = h * LABEL_HEIGHT_RATIO / line;
    let by_width = w * 0.96 / unit;
    let size = by_height.min(by_width);
    (size >= MIN_LABEL_PX).then_some(size)
}

fn draw_label(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) {
    let (w, h) = ((x1 - x0) as f32, (y1 - y0) as f32);
    let Some(size) = fit_size(font, text, w, h) else {
        return;
    };
    let scaled = font.as_scaled(PxScale::from(size));
    let text_w = text_width_1px(font, text) * size;
    let line_h = scaled.ascent() - scaled.descent();
    let mut pen_x = x0 as f32 + (w - text_w).max(0.0) / 2.0;
    let baseline = y0 as f32 + (h - line_h).max(0.0) / 2.0 + scaled.ascent();
    let (iw, ih) = img.dimensions();
    let mut prev = None;
    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(p) = prev {
            pen_x += scaled.kern(p, gid);
        }
        let glyph =
            gid.with_scale_and_position(PxScale::from(size), ab_glyph::point(pen_x, baseline));
        pen_x += scaled.h_advance(gid);
        prev = Some(gid);
        let Some(outlined) = font.outline_glyph(glyph) else {
            continue;
        };
        let bb = outlined.px_bounds();
        outlined.draw(|gx, gy, cov| {
            let px = bb.min.x as i64 + i64::from(gx);
            let py = bb.min.y as i64 + i64::from(gy);
            if px < i64::from(x0)
                || py < i64::from(y0)
                || px >= i64::from(x1)
                || py >= i64::from(y1)
            {
                return;
            }
            let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) else {
                return;
            };
            if px >= iw || py >= ih {
                return;
            }
            let cov = cov.clamp(0.0, 1.0);
            let p = img.get_pixel_mut(px, py);
            for (i, ink) in LABEL_INK.iter().enumerate() {
                if let Some(ch) = p.0.get_mut(i) {
                    *ch = (f32::from(*ch) * (1.0 - cov) + f32::from(*ink) * cov).round() as u8;
                }
            }
            if let Some(a) = p.0.get_mut(3) {
                *a = 255;
            }
        });
    }
}

/// OCR d'une image décodée, sous une session déjà ouverte.
pub(crate) fn ocr_decoded(
    session: &OcrSession,
    image: &DynamicImage,
) -> Result<OcrResult, FormatError> {
    let gray = flatten_to_gray(image);
    session.recognize(&gray, DEFAULT_IMAGE_DPI)
}

/// Niveaux de gris, la transparence composée sur fond blanc.
fn flatten_to_gray(image: &DynamicImage) -> image::GrayImage {
    if !image.color().has_alpha() {
        return image.to_luma8();
    }
    let rgba = image.to_rgba8();
    image::GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let a = f32::from(p[3]) / 255.0;
        let l = 0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]);
        image::Luma([(l * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0) as u8])
    })
}

/// OCR et masquage d'images (PNG, JPEG, GIF, BMP, TIFF, WebP).
#[derive(Debug, Clone, Copy, Default)]
pub struct MupdfImageProcessor;

impl MupdfImageProcessor {
    pub const fn new() -> Self {
        Self
    }
}

impl ImageProcessor for MupdfImageProcessor {
    fn ocr(&self, image: &[u8]) -> Result<OcrPage, FormatError> {
        let decoded = decode(image)?;
        let session = OcrSession::begin()?;
        Ok(ocr_decoded(&session, &decoded.image)?.to_page())
    }

    fn mask(&self, image: &[u8], regions: &[MaskRegion]) -> Result<Vec<u8>, FormatError> {
        let decoded = decode(image)?;
        let painted = paint_regions(&decoded.image, regions);
        encode(&painted, decoded.format)
    }
}
