//! Spike : MuPDF (crate `mupdf` 0.8) pour le caviardage et l'OCR.
//!
//! `cargo run` exécute toutes les vérifications et affiche OK / ÉCHEC par point.
//! Prérequis : `python gen_fixtures.py` (fixtures/), `tessdata/{fra,eng}.traineddata`.

use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};

use mupdf::pdf::{
    PdfDocument, PdfPage, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod, PdfWriteOptions,
};
use mupdf::shape::{FinishOptions, PdfColor, Shape, TextOptions};
use mupdf::{
    Buffer, Document, DocumentWriter, Font, Matrix, Page, Point, Quad, Rect, TextBlockContent,
    TextExtractOptions, TextPageFlags, TextWord,
};

type R<T> = Result<T, Box<dyn StdError>>;

/// Marge (pt) ajoutée autour des boîtes de mots OCR avant caviardage de pixels.
const OCR_MARGIN: f32 = 2.0;

// ---------------------------------------------------------------------------
// Recherche d'une chaîne dans les caractères positionnés d'une page
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Glyph {
    ch: char,
    quad: Option<Quad>, // None = espace virtuel inséré entre deux lignes
    origin: Point,
    size: f32,
    line: usize,
}

/// Morceau d'une occurrence situé sur une seule ligne.
#[derive(Clone, Debug)]
struct Segment {
    rect: Rect,
    baseline: f32,
    size: f32,
}

fn page_glyphs(page: &Page) -> R<Vec<Glyph>> {
    let st = page.to_text_page(TextPageFlags::empty())?.structured();
    let mut out = Vec::new();
    let mut line_id = 0;
    for block in &st.blocks {
        if let TextBlockContent::Text { lines } = &block.content {
            for line in lines {
                if !out.is_empty() {
                    out.push(Glyph {
                        ch: ' ',
                        quad: None,
                        origin: Point::new(0.0, 0.0),
                        size: 0.0,
                        line: usize::MAX,
                    });
                }
                for c in &line.chars {
                    out.push(Glyph {
                        ch: c.ch,
                        quad: Some(c.quad.clone()),
                        origin: c.origin,
                        size: c.size,
                        line: line_id,
                    });
                }
                line_id += 1;
            }
        }
    }
    Ok(out)
}

/// Texte normalisé (espaces repliés) + index du glyphe de chaque caractère.
fn normalize(glyphs: &[Glyph]) -> (Vec<char>, Vec<usize>) {
    let mut chars = Vec::new();
    let mut idx = Vec::new();
    for (i, g) in glyphs.iter().enumerate() {
        let ch = if g.ch.is_whitespace() { ' ' } else { g.ch };
        if ch == ' ' && chars.last() == Some(&' ') {
            continue;
        }
        chars.push(ch);
        idx.push(i);
    }
    (chars, idx)
}

fn quad_rect(q: &Quad) -> Rect {
    let xs = [q.ul.x, q.ur.x, q.ll.x, q.lr.x];
    let ys = [q.ul.y, q.ur.y, q.ll.y, q.lr.y];
    Rect::new(
        xs.iter().cloned().fold(f32::MAX, f32::min),
        ys.iter().cloned().fold(f32::MAX, f32::min),
        xs.iter().cloned().fold(f32::MIN, f32::max),
        ys.iter().cloned().fold(f32::MIN, f32::max),
    )
}

/// Toutes les occurrences de `needle`, chacune découpée par ligne.
fn find_occurrences(glyphs: &[Glyph], needle: &str) -> Vec<Vec<Segment>> {
    let (hay, idx) = normalize(glyphs);
    let pat: Vec<char> = needle.chars().collect();
    let mut hits = Vec::new();
    if pat.is_empty() || hay.len() < pat.len() {
        return hits;
    }
    let mut i = 0;
    while i + pat.len() <= hay.len() {
        if hay[i..i + pat.len()] == pat[..] {
            let mut segs: Vec<(usize, Segment)> = Vec::new();
            for &gi in &idx[i..i + pat.len()] {
                let g = &glyphs[gi];
                let Some(q) = &g.quad else { continue };
                let r = quad_rect(q);
                match segs.last_mut() {
                    Some((line, s)) if *line == g.line => s.rect = s.rect.r#union(&r),
                    _ => segs.push((
                        g.line,
                        Segment {
                            rect: r,
                            baseline: g.origin.y,
                            size: g.size,
                        },
                    )),
                }
            }
            hits.push(segs.into_iter().map(|(_, s)| s).collect());
            i += pat.len();
        } else {
            i += 1;
        }
    }
    hits
}

fn page_text(page: &Page) -> R<String> {
    Ok(page.text(TextExtractOptions::default())?)
}

// ---------------------------------------------------------------------------
// Caviardage + balise
// ---------------------------------------------------------------------------

/// Largeur d'une chaîne en Helvetica pour une taille de 1 pt.
fn helvetica_width_1pt(font: &Font, s: &str) -> R<f32> {
    let mut w = 0.0;
    for c in s.chars() {
        let g = font.encode_character(c as i32)?;
        w += font.advance_glyph(g)?;
    }
    Ok(w)
}

struct Target<'a> {
    value: &'a str,
    tag: &'a str,
}

/// Pose les annotations, applique le caviardage, écrit les balises. Renvoie le nombre
/// d'occurrences caviardées.
fn redact_page(
    doc: &mut PdfDocument,
    page: &mut PdfPage,
    targets: &[Target],
    opts: PdfRedactOptions,
    white_box: bool,
    words_override: Option<&[TextWord]>,
) -> R<Vec<Rect>> {
    let mut plan: Vec<(Vec<Segment>, &str)> = Vec::new();
    match words_override {
        None => {
            let glyphs = page_glyphs(page)?;
            for t in targets {
                for hit in find_occurrences(&glyphs, t.value) {
                    plan.push((hit, t.tag));
                }
            }
        }
        Some(words) => {
            // Page scannée : on cherche la suite de mots OCR.
            for t in targets {
                let parts: Vec<&str> = t.value.split_whitespace().collect();
                for w in words.windows(parts.len()) {
                    if w.iter().zip(&parts).all(|(a, b)| a.text.trim_matches(|c: char| !c.is_alphanumeric() && c != '-') == *b) {
                        let mut r = w[0].bounds;
                        for x in &w[1..] {
                            r = r.r#union(&x.bounds);
                        }
                        // Les boîtes Tesseract rognent parfois le dernier glyphe : marge.
                        let r = Rect::new(r.x0 - OCR_MARGIN, r.y0 - OCR_MARGIN, r.x1 + OCR_MARGIN, r.y1 + OCR_MARGIN);
                        let h = r.height();
                        plan.push((
                            vec![Segment {
                                rect: r,
                                baseline: r.y1 - h * 0.2,
                                size: h,
                            }],
                            t.tag,
                        ));
                    }
                }
            }
        }
    }

    for (segs, _) in &plan {
        for s in segs {
            page.add_redact_annotation(s.rect)?;
        }
    }
    page.apply_redactions_with_options(opts)?;

    let font = Font::new("Helvetica")?;
    let mut shape = Shape::new(page)?;
    for (segs, tag) in &plan {
        let Some(first) = segs.first() else { continue };
        let w1 = helvetica_width_1pt(&font, tag)?;
        let size = (first.rect.width() / w1).min(first.size).max(1.0);
        if white_box {
            shape.draw_rect(&first.rect)?.finish(&FinishOptions {
                color: None,
                fill: Some(PdfColor::rgb(1.0, 1.0, 1.0)),
                ..Default::default()
            })?;
        }
        shape.insert_text(
            Point::new(first.rect.x0, first.baseline),
            tag,
            &TextOptions {
                fontsize: size,
                fontname: "helv".into(),
                fill: Some(PdfColor::rgb(0.0, 0.0, 0.0)),
                ..Default::default()
            },
        )?;
    }
    shape.commit(doc, true)?;
    // Rectangle du premier segment de chaque occurrence (là où la balise est écrite).
    Ok(plan.iter().filter_map(|(s, _)| s.first().map(|f| f.rect)).collect())
}

fn strip_metadata(doc: &mut PdfDocument) -> R<()> {
    let mut trailer = doc.trailer()?;
    trailer.dict_delete("Info")?;
    let mut catalog = doc.catalog()?;
    catalog.dict_delete("Metadata")?;
    Ok(())
}

fn save_clean(doc: &PdfDocument, path: &Path) -> R<()> {
    let mut o = PdfWriteOptions::default();
    o.set_garbage_level(4).set_clean(true).set_compress(true);
    doc.save_with_options(path_str(path)?, o)?;
    Ok(())
}

/// Octets du fichier réécrit sans compression : sert à chercher une fuite brute.
fn decompressed_bytes(path: &Path) -> R<Vec<u8>> {
    let doc = PdfDocument::open(path_str(path)?)?;
    let mut o = PdfWriteOptions::default();
    o.set_decompress(true);
    let mut v = Vec::new();
    doc.write_to_with_options(&mut v, o)?;
    Ok(v)
}

fn contains_bytes(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn path_str(p: &Path) -> R<&str> {
    p.to_str().ok_or_else(|| "chemin non UTF-8".into())
}

fn inject_xmp(src: &Path, dst: &Path, who: &str) -> R<()> {
    let mut doc = PdfDocument::open(path_str(src)?)?;
    let xmp = format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
<dc:creator><rdf:Seq><rdf:li>{who}</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>\
<?xpacket end=\"w\"?>"
    );
    let mut dict = doc.new_dict()?;
    dict.dict_put("Type", doc.new_name("Metadata")?)?;
    dict.dict_put("Subtype", doc.new_name("XML")?)?;
    let stream = doc.add_stream(&Buffer::from_bytes(xmp.as_bytes())?, Some(&dict), false)?;
    let mut catalog = doc.catalog()?;
    catalog.dict_put("Metadata", stream)?;
    doc.save(path_str(dst)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// OCR (Tesseract intégré à MuPDF, via l'écrivain « pdfocr » de l'API sûre)
// ---------------------------------------------------------------------------

fn ocr_words(page: &Page, tmp: &Path, tessdata: &Path) -> R<Vec<TextWord>> {
    let bounds = page.bounds()?;
    {
        let opts = format!(
            "ocr-language=fra+eng,ocr-datadir={}",
            tessdata.to_string_lossy().replace('\\', "/")
        );
        let mut w = DocumentWriter::with_ocr(path_str(tmp)?, &opts)?;
        let dev = w.begin_page(bounds)?;
        page.run(&dev, &Matrix::IDENTITY)?;
        w.end_page(dev)?;
    } // Drop = fermeture de l'écrivain
    let doc = Document::open(path_str(tmp)?)?;
    let p = doc.load_page(0)?;
    let words = p.words(TextExtractOptions::default())?;
    drop(p);
    drop(doc);
    let _ = fs::remove_file(tmp);
    Ok(words)
}

fn words_text(words: &[TextWord]) -> String {
    words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Vérifications
// ---------------------------------------------------------------------------

struct Ctx {
    fixtures: PathBuf,
    out: PathBuf,
    tessdata: PathBuf,
}

fn check(cond: bool, msg: &str) -> R<()> {
    if cond {
        println!("    ok   : {msg}");
        Ok(())
    } else {
        println!("    ÉCHEC: {msg}");
        Err(format!("échec : {msg}").into())
    }
}

fn p1_build(_c: &Ctx) -> R<()> {
    let f = Font::new("Helvetica")?;
    check(f.name().contains("Helvetica") || f.name().contains("Nimbus"), &format!("police base14 chargée : {}", f.name()))?;
    let doc = PdfDocument::new();
    check(doc.page_count()? == 0, "PdfDocument::new")
}

fn verify_text_pdf(path: &Path, forbidden: &[&str], tags: &[(&str, usize)], kept: &[&str]) -> R<()> {
    let doc = Document::open(path_str(path)?)?;
    let page = doc.load_page(0)?;
    let text = page_text(&page)?;
    println!("    texte extrait après caviardage :");
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        println!("      | {l}");
    }
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut ok = true;
    for f in forbidden {
        ok &= check(!flat.contains(f), &format!("« {f} » absent du texte extrait")).is_ok();
    }
    for (t, n) in tags {
        let c = flat.matches(t).count();
        ok &= check(c == *n, &format!("balise {t} lisible {c} fois (attendu {n})")).is_ok();
    }
    for k in kept {
        ok &= check(flat.contains(k), &format!("texte voisin conservé : « {k} »")).is_ok();
    }
    for (name, m) in [
        ("auteur", mupdf::MetadataName::Author),
        ("titre", mupdf::MetadataName::Title),
        ("sujet", mupdf::MetadataName::Subject),
        ("mots-clés", mupdf::MetadataName::Keywords),
    ] {
        let v = doc.metadata(m)?;
        ok &= check(v.is_empty(), &format!("métadonnée {name} vide (« {v} »)")).is_ok();
    }
    let pdoc = PdfDocument::open(path_str(path)?)?;
    ok &= check(pdoc.catalog()?.get_dict("Metadata")?.is_none(), "pas de flux XMP /Metadata dans le catalogue").is_ok();
    ok &= check(pdoc.trailer()?.get_dict("Info")?.is_none(), "pas de dictionnaire /Info").is_ok();
    let raw = decompressed_bytes(path)?;
    for f in forbidden {
        let latin1: Vec<u8> = f.chars().map(|c| c as u32 as u8).collect();
        let utf16: Vec<u8> = f.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let hit = contains_bytes(&raw, f.as_bytes()) || contains_bytes(&raw, &latin1) || contains_bytes(&raw, &utf16);
        ok &= check(!hit, &format!("« {f} » absent des octets bruts décompressés (UTF-8, Latin-1, UTF-16BE)")).is_ok();
    }
    if ok { Ok(()) } else { Err("vérification incomplète".into()) }
}

/// Chaque zone caviardée porte une balise lisible, au même endroit (centre de la balise
/// dans le rectangle d'origine).
fn check_tag_positions(path: &Path, placed: &[Rect]) -> R<()> {
    let doc = Document::open(path_str(path)?)?;
    let page = doc.load_page(0)?;
    let mut found: Vec<Rect> = Vec::new();
    for tag in ["[PERSONNE_1]", "[PERSONNE_2]"] {
        for q in page.search(tag, 50)?.iter() {
            found.push(quad_rect(q));
        }
    }
    let mut ok = true;
    for r in placed {
        let hit = found.iter().find(|f| {
            let (cx, cy) = ((f.x0 + f.x1) / 2.0, (f.y0 + f.y1) / 2.0);
            cx >= r.x0 - 1.0 && cx <= r.x1 + 1.0 && cy >= r.y0 - 1.0 && cy <= r.y1 + 1.0
        });
        ok &= check(
            hit.is_some(),
            &format!("balise posée dans la zone [{:.1} {:.1} {:.1} {:.1}]{}", r.x0, r.y0, r.x1, r.y1,
                hit.map(|h| format!(" (hauteur balise {:.1})", h.height())).unwrap_or_default()),
        )
        .is_ok();
    }
    if ok { Ok(()) } else { Err("balise mal placée".into()) }
}

fn p2_text(c: &Ctx) -> R<()> {
    let with_xmp = c.out.join("texte_xmp.pdf");
    inject_xmp(&c.fixtures.join("texte.pdf"), &with_xmp, "Jean Dupont")?;
    let before = PdfDocument::open(path_str(&with_xmp)?)?;
    check(before.metadata(mupdf::MetadataName::Author)? == "Jean Dupont", "entrée : auteur = Jean Dupont")?;
    check(before.catalog()?.get_dict("Metadata")?.is_some(), "entrée : flux XMP présent")?;
    drop(before);

    let mut doc = PdfDocument::open(path_str(&with_xmp)?)?;
    let mut page = doc.load_pdf_page(0)?;
    let glyphs = page_glyphs(&page)?;
    println!("    {} caractères positionnés extraits ; ex. : {:?}", glyphs.len(),
        glyphs.iter().filter(|g| g.ch == 'H' && g.quad.is_some()).take(1).map(|g| (g.ch, g.origin, g.size)).collect::<Vec<_>>());
    let targets = [
        Target { value: "Jean Dupont", tag: "[PERSONNE_1]" },
        Target { value: "Hélène Müller-Lefèvre", tag: "[PERSONNE_2]" },
    ];
    let placed = redact_page(&mut doc, &mut page, &targets, PdfRedactOptions::mupdf_default(), false, None)?;
    let n = placed.len();
    check(n == 3, &format!("{n} occurrences caviardées (attendu 3)"))?;
    strip_metadata(&mut doc)?;
    let out = c.out.join("texte_anon.pdf");
    save_clean(&doc, &out)?;
    println!("    écrit : {}", out.display());
    check_tag_positions(&out, &placed)?;
    verify_text_pdf(
        &out,
        &["Dupont", "Jean", "Hélène", "Müller", "Lefèvre"],
        &[("[PERSONNE_1]", 2), ("[PERSONNE_2]", 1)],
        &["Entre", "demeurant 12 rue des Lilas", "s'engage à verser", "née le 3 mars 1980"],
    )
}

fn p3_trap(c: &Ctx) -> R<()> {
    let src = c.fixtures.join("piege.pdf");
    let mut doc = PdfDocument::open(path_str(&src)?)?;
    let mut page = doc.load_pdf_page(0)?;
    println!("    texte source :");
    for l in page_text(&page)?.lines().filter(|l| !l.trim().is_empty()) {
        println!("      | {l}");
    }
    let glyphs = page_glyphs(&page)?;
    let hits = find_occurrences(&glyphs, "Jean Dupont");
    let split = hits.iter().filter(|h| h.len() > 1).count();
    check(hits.len() == 5, &format!("{} occurrences trouvées (attendu 5 : ', TJ crénelé, hexa, \", coupure)", hits.len()))?;
    check(split == 1, &format!("{split} occurrence coupée sur deux lignes"))?;
    let placed = redact_page(&mut doc, &mut page, &[Target { value: "Jean Dupont", tag: "[PERSONNE_1]" }], PdfRedactOptions::mupdf_default(), false, None)?;
    let n = placed.len();
    check(n == 5, &format!("{n} occurrences caviardées"))?;
    strip_metadata(&mut doc)?;
    let out = c.out.join("piege_anon.pdf");
    save_clean(&doc, &out)?;
    println!("    écrit : {}", out.display());
    check_tag_positions(&out, &placed)?;
    verify_text_pdf(
        &out,
        &["Dupont", "Jean", "upont", "4A65616E"],
        &[("[PERSONNE_1]", 5)],
        &["Dossier suivi par", "conseiller.", "Le signataire,", "confirme.", "Contact :", "en hexa.", "Fin du dossier"],
    )
}

fn p4_ocr(c: &Ctx) -> R<()> {
    let tmp = c.out.join("ocr_tmp.pdf");
    let mut ok = true;
    for (label, file) in [("PDF image seule", "scan.pdf"), ("PNG isolé", "scan.png")] {
        let doc = Document::open(path_str(&c.fixtures.join(file))?)?;
        let page = doc.load_page(0)?;
        let src_text = page_text(&page)?;
        check(src_text.trim().is_empty(), &format!("{label} : aucun texte natif avant OCR"))?;
        let t0 = std::time::Instant::now();
        let words = ocr_words(&page, &tmp, &c.tessdata)?;
        println!("    {label} : {} mots en {:.1} s", words.len(), t0.elapsed().as_secs_f32());
        for w in words.iter().take(40) {
            println!("      {:<18} [{:6.1} {:6.1} {:6.1} {:6.1}]", w.text, w.bounds.x0, w.bounds.y0, w.bounds.x1, w.bounds.y1);
        }
        let all = words_text(&words);
        for needle in ["Jean", "Dupont", "Hélène", "Müller-Lefèvre", "Lilas", "Attestation"] {
            ok &= check(all.contains(needle), &format!("{label} : « {needle} » reconnu")).is_ok();
        }
    }
    if ok { Ok(()) } else { Err("OCR incomplet".into()) }
}

fn p5_pixels(c: &Ctx) -> R<()> {
    let tmp = c.out.join("ocr_tmp.pdf");
    let src = c.fixtures.join("scan.pdf");
    let mut doc = PdfDocument::open(path_str(&src)?)?;
    let mut page = doc.load_pdf_page(0)?;
    let words = ocr_words(&page, &tmp, &c.tessdata)?;
    // Les mots OCR sont comparés sans ponctuation de bord (« Dupont, » -> « Dupont »).
    let targets = [
        Target { value: "Jean Dupont", tag: "[PERSONNE_1]" },
        Target { value: "Hélène Müller-Lefèvre", tag: "[PERSONNE_2]" },
    ];
    let opts = PdfRedactOptions {
        black_boxes: false,
        image_method: PdfRedactImageMethod::Pixels,
        line_art: PdfRedactLineArtMethod::None,
        text: PdfRedactTextMethod::Remove,
    };
    let n = redact_page(&mut doc, &mut page, &targets, opts, true, Some(&words))?.len();
    check(n == 2, &format!("{n} zones de pixels caviardées (attendu 2)"))?;
    strip_metadata(&mut doc)?;
    let out = c.out.join("scan_anon.pdf");
    save_clean(&doc, &out)?;
    println!("    écrit : {}", out.display());
    drop(page);
    drop(doc);

    // Vérification 1 : ré-OCR de la page entière (image caviardée + surcouche balise).
    let doc2 = Document::open(path_str(&out)?)?;
    let page2 = doc2.load_page(0)?;
    let words2 = ocr_words(&page2, &tmp, &c.tessdata)?;
    let all = words_text(&words2);
    println!("    ré-OCR : {all}");
    let mut ok = true;
    for f in ["Dupont", "Jean", "Hélène", "Müller", "Lefèvre"] {
        ok &= check(!all.contains(f), &format!("ré-OCR : « {f} » absent")).is_ok();
    }
    for k in ["Attestation", "Lilas", "soussigné"] {
        ok &= check(all.contains(k), &format!("ré-OCR : « {k} » toujours lu")).is_ok();
    }
    ok &= check(all.contains("PERSONNE"), "ré-OCR : balise PERSONNE lue").is_ok();

    // Vérification 2 : l'image seule (sans la surcouche balise) ne contient plus le nom.
    let tp = page2.to_text_page(TextPageFlags::PRESERVE_IMAGES)?;
    let img = tp.blocks().find_map(|b| b.image());
    ok &= check(img.is_some(), "image de la page récupérée").is_ok();
    if let Some(img) = img {
        let pix = img.to_pixmap()?;
        let img_path = c.out.join("scan_anon_image.png");
        pix.save_as(path_str(&img_path)?, mupdf::ImageFormat::PNG)?;
        println!("    image extraite : {} ({}x{})", img_path.display(), pix.width(), pix.height());
        let idoc = Document::open(path_str(&img_path)?)?;
        let ip = idoc.load_page(0)?;
        let iw = words_text(&ocr_words(&ip, &tmp, &c.tessdata)?);
        println!("    OCR de l'image extraite seule : {iw}");
        for f in ["Dupont", "Hélène", "Müller"] {
            ok &= check(!iw.contains(f), &format!("image extraite : « {f} » absent")).is_ok();
        }
    }
    if ok { Ok(()) } else { Err("caviardage de pixels incomplet".into()) }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let c = Ctx {
        fixtures: root.join("fixtures"),
        out: root.join("out"),
        tessdata: root.join("tessdata"),
    };
    let _ = fs::create_dir_all(&c.out);

    let checks: [(&str, fn(&Ctx) -> R<()>); 5] = [
        ("1. Compilation / liaison MuPDF", p1_build),
        ("2. Caviardage PDF texte + balises + métadonnées", p2_text),
        ("3. Cas piège (', \", TJ, hexa, coupure de ligne)", p3_trap),
        ("4. OCR Tesseract (PDF image, PNG)", p4_ocr),
        ("5. Caviardage de pixels + ré-OCR", p5_pixels),
    ];
    let mut summary = Vec::new();
    for (name, f) in checks {
        println!("\n== {name}");
        let t0 = std::time::Instant::now();
        let r = f(&c);
        let el = t0.elapsed().as_secs_f32();
        match &r {
            Ok(()) => println!("=> OK ({el:.1} s)"),
            Err(e) => println!("=> ÉCHEC ({el:.1} s) : {e}"),
        }
        summary.push((name, r.is_ok()));
    }
    // Rendus PNG des fichiers produits, pour contrôle visuel.
    for f in ["texte_anon", "piege_anon", "scan_anon"] {
        let pdf = c.out.join(format!("{f}.pdf"));
        let render = || -> R<()> {
            let doc = Document::open(path_str(&pdf)?)?;
            let pix = doc.load_page(0)?.to_pixmap(&Matrix::new_scale(1.5, 1.5), &mupdf::Colorspace::device_rgb(), false, false)?;
            pix.save_as(path_str(&c.out.join(format!("{f}.png")))?, mupdf::ImageFormat::PNG)?;
            Ok(())
        };
        if let Err(e) = render() {
            println!("rendu {f} impossible : {e}");
        }
    }
    println!("\n== Résumé");
    for (name, ok) in &summary {
        println!("{:<6} {name}", if *ok { "OK" } else { "ÉCHEC" });
    }
    println!("(6. Taille release / CRT statique : voir NOTES.md, mesuré hors programme)");
    if summary.iter().any(|(_, ok)| !ok) {
        std::process::exit(1);
    }
}
