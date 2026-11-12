#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Pages scannées exposées comme images (`Extraction::media`), remplacement par
//! `TransformOptions::media`, balises qui débordent sur la place libre.

mod common;

use std::io::Cursor;

use anon_core::{
    BlockRole, Handler, ImageProcessor, MediaTransform, TransformOptions, WarningCode,
};
use anon_pdf::{MupdfImageProcessor, PdfHandler};
use common::*;
use mupdf::pdf::PdfDocument;
use mupdf::{TextBlockContent, TextPageFlags};

#[test]
fn page_scannee_exposee_comme_image() {
    let ex = PdfHandler.extract(&exemple("courrier-scanne.pdf")).unwrap();
    let names: Vec<_> = ex.media.iter().map(|m| m.name.as_str()).collect();
    println!("MediaItem : {names:?}");
    assert_eq!(names, ["page 1"]);
    assert!(ex.blocks.iter().all(|b| b.ctx.role != BlockRole::Ocr));
    let img = image::load_from_memory(&ex.media[0].bytes).unwrap();
    assert_eq!(
        image::guess_format(&ex.media[0].bytes).unwrap(),
        image::ImageFormat::Png
    );
    // A4 à la résolution de l'OCR (300 dpi).
    assert!(
        img.width() > 2000 && img.height() > 3000,
        "{}x{}",
        img.width(),
        img.height()
    );
    // PDF texte : aucune image exposée.
    assert!(
        PdfHandler
            .extract(&exemple("facture.pdf"))
            .unwrap()
            .media
            .is_empty()
    );
}

/// Rend une image entièrement blanche de même taille, et note les noms reçus.
struct White(std::sync::Mutex<Vec<String>>);

impl MediaTransform for White {
    fn apply(&self, name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        self.0.lock().unwrap().push(name.to_string());
        let src = image::load_from_memory(bytes).ok()?;
        let white = image::RgbImage::from_pixel(src.width(), src.height(), image::Rgb([255; 3]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(white)
            .write_to(&mut out, image::ImageFormat::Png)
            .ok()?;
        Some(out.into_inner())
    }
}

/// Flux décodés de toutes les images du fichier.
fn image_streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let doc = PdfDocument::from_bytes(pdf).unwrap();
    let mut out = Vec::new();
    for num in 1..doc.xref_len().unwrap() {
        let Ok(Some(obj)) = doc.xref_object(num as i32) else {
            continue;
        };
        let is_image = obj
            .get_dict("Subtype")
            .ok()
            .flatten()
            .and_then(|s| s.as_name().ok())
            .is_some_and(|n| n == b"Image");
        let stream = doc.new_indirect(num as i32, 0).unwrap();
        if is_image && stream.is_stream().unwrap() {
            out.push(stream.read_stream().unwrap());
        }
    }
    out
}

#[test]
fn image_remplacee_par_media() {
    let input = exemple("courrier-scanne.pdf");
    let before = image_streams(&input);
    assert_eq!(before.len(), 1);

    let white = White(Default::default());
    let (s, _) = session(&[]);
    let mut opts = TransformOptions::anonymize();
    opts.media = Some(&white);
    let out = PdfHandler.transform(&input, &replacer(&s), &opts).unwrap();
    assert_eq!(*white.0.lock().unwrap(), ["page 1"]);

    // Le PDF se rouvre ; l'image est à la même place, sur toute la page.
    let doc = PdfDocument::from_bytes(&out.bytes).unwrap();
    assert_eq!(doc.page_count().unwrap(), 1);
    let page = doc.load_page(0).unwrap();
    let bounds = page.bounds().unwrap();
    let st = page
        .to_text_page(TextPageFlags::PRESERVE_IMAGES)
        .unwrap()
        .structured();
    let imgs: Vec<_> = st
        .blocks
        .iter()
        .filter(|b| matches!(b.content, TextBlockContent::Image { .. }))
        .map(|b| b.bounds)
        .collect();
    assert_eq!(imgs.len(), 1, "une seule image sur la page");
    let r = imgs[0];
    for (a, b) in [
        (r.x0, bounds.x0),
        (r.y0, bounds.y0),
        (r.x1, bounds.x1),
        (r.y1, bounds.y1),
    ] {
        assert!(
            (a - b).abs() < 1.5,
            "image déplacée : {r:?} / page {bounds:?}"
        );
    }

    // L'ancienne image n'est plus dans le fichier : une seule image, toute blanche.
    let after = image_streams(&out.bytes);
    assert_eq!(after.len(), 1, "image d'origine restée dans un objet");
    assert!(after[0].iter().all(|&b| b == 255), "image non blanche");
    assert!(!after.contains(&before[0]));

    // Plus aucun texte lisible par OCR, ni dans la page ni dans l'image seule.
    let deep = PdfHandler.deep_scan(&out.bytes).unwrap();
    let ocr: Vec<_> = deep
        .iter()
        .filter(|b| b.ctx.role == BlockRole::Ocr && b.text.chars().any(char::is_alphanumeric))
        .collect();
    assert!(ocr.is_empty(), "texte encore lu : {} bloc(s)", ocr.len());
    let ex = PdfHandler.extract(&out.bytes).unwrap();
    let page_ocr = MupdfImageProcessor.ocr(&ex.media[0].bytes).unwrap();
    assert!(page_ocr.words.is_empty());
}

#[test]
fn sans_media_traitement_interne() {
    // `opts.media` absent, ou `apply` qui rend `None` : OCR et effacement internes.
    struct Nothing;
    impl MediaTransform for Nothing {
        fn apply(&self, _: &str, _: &[u8]) -> Option<Vec<u8>> {
            None
        }
    }
    let input = exemple("courrier-scanne.pdf");
    let values = vec![(anon_core::EntityKind::Personne, "Jean Dupont".to_string())];
    let (s, tags) = session(&values);
    let mut opts = TransformOptions::anonymize();
    opts.media = Some(&Nothing);
    for o in [TransformOptions::anonymize(), opts] {
        let out = PdfHandler.transform(&input, &replacer(&s), &o).unwrap();
        assert_eq!(out.replacements, 1);
        let text = joined(&PdfHandler.deep_scan(&out.bytes).unwrap());
        assert!(!text.contains("Dupont"));
        assert!(text.contains(&tags[0].0.to_string()));
    }
}

/// Mots de la page avec leur rectangle.
fn words(pdf: &[u8]) -> Vec<(String, mupdf::Rect)> {
    let doc = PdfDocument::from_bytes(pdf).unwrap();
    let page = doc.load_page(0).unwrap();
    page.words(Default::default())
        .unwrap()
        .into_iter()
        .map(|w| (w.text, w.bounds))
        .collect()
}

#[test]
fn balises_debordent_sur_la_place_libre() {
    let input = exemple("facture.pdf");
    let values = expected("facture.pdf");
    let (s, _) = session(&values);
    let out = PdfHandler
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    let too_small: Vec<_> = out
        .warnings
        .iter()
        .filter(|w| w.code == WarningCode::TagDidNotFit)
        .collect();
    println!("avertissements TagDidNotFit : {too_small:?}");
    // Avant : [FONCTION_1] (et [LIEU_1]) à la taille minimale.
    assert!(too_small.is_empty(), "balises encore à la taille minimale");

    // Aucune balise ne recouvre un mot voisin.
    let w = words(&out.bytes);
    let (tags, others): (Vec<_>, Vec<_>) = w.iter().partition(|(t, _)| t.contains('['));
    assert!(!tags.is_empty());
    for (t, r) in &tags {
        for (o, q) in &others {
            let ix = (r.x1.min(q.x1) - r.x0.max(q.x0)).max(0.0);
            let iy = (r.y1.min(q.y1) - r.y0.max(q.y0)).max(0.0);
            assert!(ix * iy < 1.0, "« {t} » recouvre « {o} » ({r:?} / {q:?})");
        }
    }
    // Les balises restent lisibles : au moins 6 pt.
    let doc = PdfDocument::from_bytes(&out.bytes).unwrap();
    let st = doc
        .load_page(0)
        .unwrap()
        .to_text_page(TextPageFlags::empty())
        .unwrap()
        .structured();
    for b in &st.blocks {
        if let TextBlockContent::Text { lines } = &b.content {
            for l in lines {
                if l.text.contains('[') {
                    for c in &l.chars {
                        assert!(
                            c.size >= 6.0,
                            "balise trop petite ({}) : {}",
                            c.size,
                            l.text
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn xmp_lu_et_controle() {
    // Régression : `is_stream` testé sur l'objet résolu ignorait les flux XMP.
    let mut doc = PdfDocument::new();
    doc.new_page((200.0f32, 200.0f32)).unwrap();
    let xmp =
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><dc:creator>Jean Dupont</dc:creator></x:xmpmeta>";
    let mut dict = doc.new_dict().unwrap();
    dict.dict_put("Type", doc.new_name("Metadata").unwrap())
        .unwrap();
    dict.dict_put("Subtype", doc.new_name("XML").unwrap())
        .unwrap();
    let stream = doc
        .add_stream(
            &mupdf::Buffer::from_bytes(xmp.as_bytes()).unwrap(),
            Some(&dict),
            false,
        )
        .unwrap();
    doc.catalog().unwrap().dict_put("Metadata", stream).unwrap();
    let mut input = Vec::new();
    doc.write_to(&mut input).unwrap();

    let ex = PdfHandler.extract(&input).unwrap();
    assert!(
        joined(&ex.blocks).contains("Jean Dupont"),
        "XMP non extrait"
    );
    assert!(joined(&PdfHandler.deep_scan(&input).unwrap()).contains("Jean Dupont"));
    let (s, _) = session(&[(anon_core::EntityKind::Personne, "Jean Dupont".into())]);
    let out = PdfHandler
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    assert!(!joined(&PdfHandler.deep_scan(&out.bytes).unwrap()).contains("Dupont"));
}

#[test]
fn grande_image_dans_une_page_de_texte() {
    let mut doc = PdfDocument::new();
    let mut page = doc.new_page((595.0f32, 842.0f32)).unwrap();
    {
        let mut shape = mupdf::shape::Shape::new(&mut page).unwrap();
        shape
            .insert_text(
                mupdf::Point::new(72.0, 60.0),
                "Texte de la page.",
                &mupdf::shape::TextOptions {
                    fontsize: 12.0,
                    fontname: "helv".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        shape.commit(&mut doc, true).unwrap();
    }
    let gray = image::RgbImage::from_pixel(300, 300, image::Rgb([120, 120, 120]));
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(gray)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    page.insert_image(
        &mut doc,
        mupdf::Rect::new(100.0, 100.0, 400.0, 400.0),
        mupdf::pdf::PageImageSource::Bytes {
            data: &png,
            format_hint: Some("png"),
        },
        Default::default(),
    )
    .unwrap();
    drop(page);
    let mut input = Vec::new();
    doc.write_to(&mut input).unwrap();

    let ex = PdfHandler.extract(&input).unwrap();
    let names: Vec<_> = ex.media.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["page 1 image 1"]);
    assert!(joined(&ex.blocks).contains("Texte de la page."));

    let white = White(Default::default());
    let (s, _) = session(&[]);
    let mut opts = TransformOptions::anonymize();
    opts.media = Some(&white);
    let out = PdfHandler.transform(&input, &replacer(&s), &opts).unwrap();
    assert_eq!(*white.0.lock().unwrap(), ["page 1 image 1"]);
    let after = image_streams(&out.bytes);
    assert_eq!(after.len(), 1);
    assert!(after[0].iter().all(|&b| b == 255));
    // Le texte hors de l'image est intact.
    assert!(joined(&PdfHandler.extract(&out.bytes).unwrap().blocks).contains("Texte de la page."));
}
