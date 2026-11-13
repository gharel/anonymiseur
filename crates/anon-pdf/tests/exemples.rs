#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Tests pilotés par `exemples/attendu.json` : facture.pdf, courrier-scanne.pdf, carte-visite.png.

mod common;

use anon_core::{Handler, TextTransform, TransformOptions};
use anon_pdf::{ImageHandler, PdfHandler, decompressed_bytes};
use common::*;
use mupdf::pdf::PdfDocument;
use mupdf::{Quad, Rect};

/// Début de la balise (bord gauche) et milieu vertical : une balise peut
/// déborder à droite sur la place libre, mais commence là où était la valeur.
fn quad_center(q: &Quad) -> (f32, f32) {
    let xs = [q.ul.x, q.ur.x, q.ll.x, q.lr.x];
    let ys = [q.ul.y, q.ur.y, q.ll.y, q.lr.y];
    (
        xs.iter().copied().fold(f32::MAX, f32::min),
        ys.iter().sum::<f32>() / 4.0,
    )
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

/// Aucune valeur dans le texte du contrôle de fuite (sous-chaîne et `Replacer`).
fn assert_no_leak(
    blocks: &[anon_core::TextBlock],
    values: &[(anon_core::EntityKind, String)],
    what: &str,
) {
    let (s, _) = session(values);
    let rep = replacer(&s);
    let text = joined(blocks);
    let mut leaks = Vec::new();
    for (_, v) in values {
        if text.contains(&norm(v)) {
            leaks.push(v.clone());
        }
    }
    for b in blocks {
        let ctx = b.ctx.clone();
        if !rep.replacements(&b.text, &ctx).is_empty() {
            leaks.push(format!("(Replacer) bloc {} {:?}", b.ctx.part, b.ctx.role));
        }
    }
    assert!(leaks.is_empty(), "{what} : fuite {leaks:?}");
}

fn assert_no_raw_leak(pdf: &[u8], values: &[(anon_core::EntityKind, String)]) {
    let raw = decompressed_bytes(pdf).expect("décompression");
    let leaks: Vec<_> = values
        .iter()
        .filter(|(_, v)| raw_leak(&raw, v))
        .map(|(_, v)| v.clone())
        .collect();
    assert!(
        leaks.is_empty(),
        "fuite dans les flux décompressés : {leaks:?}"
    );
}

#[test]
fn facture_pdf_complet() {
    let input = exemple("facture.pdf");
    let values = expected("facture.pdf");
    let h = PdfHandler;

    // 1. Extraction : toutes les valeurs (PDF texte).
    let ex = h.extract(&input).expect("extract");
    let (read, missed) = found(&values, &joined(&ex.blocks));
    println!("facture.pdf : {}/{} valeurs lues", read.len(), values.len());
    assert!(missed.is_empty(), "valeurs manquées : {missed:?}");

    // 2. Anonymisation avec un Replacer construit sur les valeurs lues.
    let (sess, tags) = session(&read);
    let out = h
        .transform(&input, &replacer(&sess), &TransformOptions::anonymize())
        .expect("transform");
    println!(
        "facture.pdf : {} remplacements, avertissements {:?}",
        out.replacements, out.warnings
    );
    // L'auteur (métadonnée) est supprimé, pas remplacé.
    assert!(out.replacements >= read.len() - 1);

    // 3. Contrôle de fuite : deep_scan et flux bruts.
    let deep = h.deep_scan(&out.bytes).expect("deep_scan");
    assert_no_leak(&deep, &read, "facture.pdf");
    assert_no_raw_leak(&out.bytes, &read);

    // 4. La sortie se rouvre ; métadonnées vidées.
    let doc = PdfDocument::from_bytes(&out.bytes).expect("réouverture");
    assert_eq!(doc.page_count().unwrap(), 1);
    assert!(doc.trailer().unwrap().get_dict("Info").unwrap().is_none());
    assert!(
        doc.catalog()
            .unwrap()
            .get_dict("Metadata")
            .unwrap()
            .is_none()
    );

    // 5. Balises lisibles à la bonne position (valeurs sur une seule ligne).
    let src = PdfDocument::from_bytes(&input).unwrap();
    let src_page = src.load_page(0).unwrap();
    let out_page = doc.load_page(0).unwrap();
    let mut checked = 0;
    let mut misplaced = Vec::new();
    for (tag, value) in &tags {
        let tag_text = tag.to_string();
        let origin: Vec<Rect> = src_page
            .search(value, 16)
            .unwrap()
            .iter()
            .map(quad_rect)
            .collect();
        if origin.is_empty() {
            continue;
        }
        let placed: Vec<(f32, f32)> = out_page
            .search(&tag_text, 16)
            .unwrap()
            .iter()
            .map(quad_center)
            .collect();
        for r in &origin {
            checked += 1;
            let ok = placed.iter().any(|(cx, cy)| {
                *cx >= r.x0 - 1.0 && *cx <= r.x1 + 1.0 && *cy >= r.y0 - 1.0 && *cy <= r.y1 + 1.0
            });
            if !ok {
                misplaced.push(tag_text.clone());
            }
        }
    }
    println!("facture.pdf : {checked} balises vérifiées en position");
    assert!(
        checked >= read.len() - 1,
        "trop peu de positions vérifiées ({checked})"
    );
    assert!(
        misplaced.is_empty(),
        "balises absentes ou mal placées : {misplaced:?}"
    );

    // 6. Restauration.
    let restored = h
        .transform(&out.bytes, &restorer(&sess), &TransformOptions::restore())
        .expect("restauration");
    let back = h.extract(&restored.bytes).expect("extract restauré");
    let text = joined(&back.blocks);
    // Les métadonnées ont été vidées à l'anonymisation : seules les valeurs du corps reviennent.
    let body: Vec<_> = ex
        .blocks
        .iter()
        .filter(|b| b.ctx.role != anon_core::BlockRole::Metadata)
        .cloned()
        .collect();
    let (in_body, _) = found(&read, &joined(&body));
    let (_, still_missing) = found(&in_body, &text);
    println!(
        "facture.pdf restaurée : {} remplacements, manquantes {still_missing:?}",
        restored.replacements
    );
    assert!(
        still_missing.is_empty(),
        "valeurs non restaurées : {still_missing:?}"
    );
    for (tag, _) in &tags {
        assert!(!text.contains(&tag.to_string()), "balise restée : {tag}");
    }
}

#[test]
fn courrier_scanne_ocr() {
    let input = exemple("courrier-scanne.pdf");
    let values = expected("courrier-scanne.pdf");
    let h = PdfHandler;

    // La page scannée est exposée comme image : l'OCR est celui du moteur
    // (`MupdfImageProcessor`), pas des blocs `Ocr` de l'extraction.
    let ex = h.extract(&input).expect("extract");
    assert_eq!(ex.media.len(), 1);
    let page = anon_core::ImageProcessor::ocr(&anon_pdf::MupdfImageProcessor, &ex.media[0].bytes)
        .expect("OCR de la page");
    let (read, missed) = found(&values, &norm(&page.text_with_spans().0));
    let rate = read.len() as f32 / values.len() as f32;
    println!(
        "courrier-scanne.pdf : OCR {}/{} = {:.1} % ; manquées : {missed:?}",
        read.len(),
        values.len(),
        rate * 100.0
    );
    assert!(rate >= 0.8, "taux d'OCR trop faible : {rate}");

    let (sess, tags) = session(&read);
    let out = h
        .transform(&input, &replacer(&sess), &TransformOptions::anonymize())
        .expect("transform");
    println!(
        "courrier-scanne.pdf : {} remplacements, avertissements {:?}",
        out.replacements, out.warnings
    );
    assert!(out.replacements >= read.len());

    let deep = h.deep_scan(&out.bytes).expect("deep_scan");
    assert_no_leak(&deep, &read, "courrier-scanne.pdf");
    assert_no_raw_leak(&out.bytes, &read);
    // Balises écrites en texte natif, lisibles.
    let deep_text = joined(&deep);
    let missing_tags: Vec<_> = tags
        .iter()
        .map(|(t, _)| t.to_string())
        .filter(|t| !deep_text.contains(t))
        .collect();
    assert!(
        missing_tags.is_empty(),
        "balises absentes : {missing_tags:?}"
    );
    let doc = PdfDocument::from_bytes(&out.bytes).expect("réouverture");
    assert_eq!(doc.page_count().unwrap(), 1);
}

#[test]
fn carte_visite_png() {
    let input = exemple("carte-visite.png");
    let values = expected("carte-visite.png");
    let h = ImageHandler;

    let ex = h.deep_scan(&input).expect("deep_scan");
    let (read, missed) = found(&values, &joined(&ex));
    let rate = read.len() as f32 / values.len() as f32;
    println!(
        "carte-visite.png : OCR {}/{} = {:.1} % ; manquées : {missed:?}",
        read.len(),
        values.len(),
        rate * 100.0
    );
    assert!(rate >= 0.8, "taux d'OCR trop faible : {rate}");

    let (sess, tags) = session(&read);
    let out = h
        .transform(&input, &replacer(&sess), &TransformOptions::anonymize())
        .expect("transform");
    println!(
        "carte-visite.png : {} remplacements, avertissements {:?}",
        out.replacements, out.warnings
    );
    assert!(
        out.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "format conservé"
    );

    let again = h.deep_scan(&out.bytes).expect("ré-OCR");
    let text = joined(&again);
    println!("carte-visite.png ré-OCR : {text}");
    assert_no_leak(&again, &read, "carte-visite.png");
    // Au moins une balise lisible par ré-OCR (l'OCR des balises n'est pas exact).
    let prefixes: Vec<String> = tags
        .iter()
        .map(|(t, _)| t.kind.prefix().to_string())
        .collect();
    assert!(
        prefixes.iter().any(|p| text.contains(p.as_str())),
        "aucune balise relue : {prefixes:?}"
    );

    // Restauration : remise de l'original fourni par MediaTransform, sinon inchangé.
    struct Orig(Vec<u8>);
    impl anon_core::MediaTransform for Orig {
        fn apply(&self, _name: &str, _bytes: &[u8]) -> Option<Vec<u8>> {
            Some(self.0.clone())
        }
    }
    let orig = Orig(input.clone());
    let mut ropts = TransformOptions::restore();
    ropts.media = Some(&orig);
    let back = h.transform(&out.bytes, &restorer(&sess), &ropts).unwrap();
    assert_eq!(back.bytes, input);
    let same = h
        .transform(&out.bytes, &restorer(&sess), &TransformOptions::restore())
        .unwrap();
    assert_eq!(same.bytes, out.bytes);
}
