#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Pièges du prototype (opérateurs `'`, `"`, `TJ` crénelé, chaîne hexadécimale,
//! nom coupé sur deux lignes) et structures annexes (annotations, signets,
//! métadonnées, fichiers joints, PDF chiffré).

mod common;

use anon_core::{EntityKind, FormatError, Handler, TransformOptions, WarningCode};
use anon_pdf::{PdfHandler, decompressed_bytes};
use common::*;
use mupdf::pdf::{EmbeddedFileOptions, Encryption, PdfDocument, PdfWriteOptions};
use mupdf::shape::{PdfColor, Shape, TextOptions};
use mupdf::{Outline, Point, Rect};

fn page_text(pdf: &[u8]) -> String {
    let doc = PdfDocument::from_bytes(pdf).unwrap();
    let mut all = String::new();
    for i in 0..doc.page_count().unwrap() {
        let p = doc.load_page(i).unwrap();
        all.push_str(&p.text(Default::default()).unwrap());
        all.push('\n');
    }
    norm(&all)
}

#[test]
fn piege_operateurs_et_coupure() {
    let input = fixture("piege.pdf");
    let h = PdfHandler;
    let values = vec![(EntityKind::Personne, "Jean Dupont".to_string())];
    let (s, tags) = session(&values);
    let tag = tags[0].0.to_string();

    // Les 5 occurrences sont visibles par le Replacer (dont la coupure de ligne).
    let deep_in = h.deep_scan(&input).unwrap();
    let page_block = deep_in
        .iter()
        .filter(|b| b.ctx.part == "page 1" && b.ctx.role == anon_core::BlockRole::Body)
        .max_by_key(|b| b.text.len())
        .unwrap();
    assert_eq!(
        page_block.text.matches("Jean Dupont").count(),
        5,
        "{}",
        page_block.text
    );

    let out = h
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    assert_eq!(out.replacements, 5);
    let text = page_text(&out.bytes);
    println!("piège caviardé : {text}");
    for f in ["Jean", "Dupont", "upont"] {
        assert!(!text.contains(f), "« {f} » encore présent");
    }
    assert_eq!(text.matches(tag.as_str()).count(), 5, "5 balises lisibles");
    for kept in [
        "Dossier suivi par",
        "conseiller.",
        "Le signataire,",
        "confirme.",
        "Contact :",
        "en hexa.",
        "Fin du dossier",
    ] {
        assert!(text.contains(kept), "texte voisin perdu : « {kept} »");
    }
    let raw = decompressed_bytes(&out.bytes).unwrap();
    for f in ["Dupont", "Jean", "4A65616E", "upont"] {
        assert!(!raw_leak(&raw, f), "« {f} » dans les flux bruts");
    }
    // Auteur de /Info supprimé.
    let doc = PdfDocument::from_bytes(&out.bytes).unwrap();
    assert!(doc.trailer().unwrap().get_dict("Info").unwrap().is_none());
    let deep = h.deep_scan(&out.bytes).unwrap();
    assert!(!joined(&deep).contains("Dupont"));
}

#[test]
fn texte_accents_et_restauration() {
    let input = fixture("texte.pdf");
    let h = PdfHandler;
    let values = vec![
        (EntityKind::Personne, "Jean Dupont".to_string()),
        (EntityKind::Personne, "Hélène Müller-Lefèvre".to_string()),
    ];
    let (s, _) = session(&values);
    let out = h
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    assert_eq!(out.replacements, 3);
    let text = page_text(&out.bytes);
    for f in ["Dupont", "Hélène", "Müller", "Lefèvre"] {
        assert!(!text.contains(f), "{f}");
    }
    let back = h
        .transform(&out.bytes, &restorer(&s), &TransformOptions::restore())
        .unwrap();
    let text = page_text(&back.bytes);
    println!("restauré : {text}");
    assert_eq!(text.matches("Jean Dupont").count(), 2);
    assert!(text.contains("Hélène Müller-Lefèvre"));
}

/// PDF construit : texte, annotation (contenu + auteur), signet, Info, XMP, fichier joint.
fn build_rich_pdf() -> Vec<u8> {
    let mut doc = PdfDocument::new();
    let mut page = doc.new_page((595.0f32, 842.0f32)).unwrap();
    {
        let mut shape = Shape::new(&mut page).unwrap();
        shape
            .insert_text(
                Point::new(72.0, 100.0),
                "Dossier de Jean Dupont, suivi.",
                &TextOptions {
                    fontsize: 12.0,
                    fontname: "helv".into(),
                    fill: Some(PdfColor::rgb(0.0, 0.0, 0.0)),
                    ..Default::default()
                },
            )
            .unwrap();
        shape.commit(&mut doc, true).unwrap();
    }
    let mut annot = page
        .add_text_annotation(
            Rect::new(300.0, 300.0, 320.0, 320.0),
            "Rappeler Jean Dupont demain",
        )
        .unwrap();
    annot.set_author("Jean Dupont").unwrap();
    annot.update().unwrap();
    drop(page);
    doc.set_outlines(&[Outline {
        title: "Fiche Jean Dupont".into(),
        uri: None,
        dest: None,
        down: vec![],
    }])
    .unwrap();
    let mut info = doc.new_dict().unwrap();
    info.dict_put("Author", doc.new_string("Jean Dupont").unwrap())
        .unwrap();
    info.dict_put("Title", doc.new_string("Dossier Dupont").unwrap())
        .unwrap();
    let info = doc.add_object(&info).unwrap();
    doc.trailer().unwrap().dict_put("Info", info).unwrap();
    doc.add_embedded_file(
        "piece.txt",
        b"Annexe confidentielle de Jean Dupont",
        EmbeddedFileOptions::new("piece.txt"),
    )
    .unwrap();
    let mut out = Vec::new();
    doc.write_to(&mut out).unwrap();
    out
}

#[test]
fn annotations_signets_metadonnees_pieces_jointes() {
    let input = build_rich_pdf();
    let h = PdfHandler;
    let ex = h.extract(&input).unwrap();
    let roles: Vec<_> = ex
        .blocks
        .iter()
        .filter(|b| b.text.contains("Dupont"))
        .map(|b| (b.ctx.part.clone(), b.ctx.role))
        .collect();
    println!("blocs : {roles:?}");
    use anon_core::BlockRole::*;
    for role in [Body, Comment, Metadata, Other] {
        assert!(
            roles.iter().any(|(_, r)| *r == role),
            "rôle {role:?} absent"
        );
    }
    assert!(roles.iter().any(|(p, _)| p == "signets"));
    assert!(
        ex.warnings
            .iter()
            .any(|w| w.code == WarningCode::EmbeddedBinary)
    );

    let (s, _) = session(&[(EntityKind::Personne, "Jean Dupont".to_string())]);
    let out = h
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    println!("avertissements : {:?}", out.warnings);
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::EmbeddedBinary)
    );
    let deep = h.deep_scan(&out.bytes).unwrap();
    let all = joined(&deep);
    assert!(!all.contains("Dupont"), "fuite : {all}");
    assert!(
        all.contains("Rappeler [PERSONNE_1] demain"),
        "annotation transformée : {all}"
    );
    assert!(all.contains("Fiche [PERSONNE_1]"), "signet transformé");
    let raw = decompressed_bytes(&out.bytes).unwrap();
    assert!(!raw_leak(&raw, "Dupont"));
    let doc = PdfDocument::from_bytes(&out.bytes).unwrap();
    assert!(doc.embedded_files().unwrap().is_empty());
}

#[test]
fn pdf_chiffre() {
    let mut doc = PdfDocument::new();
    doc.new_page((200.0f32, 200.0f32)).unwrap();
    let mut o = PdfWriteOptions::default();
    o.set_encryption(Encryption::Aes256)
        .set_user_password("secret")
        .set_owner_password("proprio");
    let mut bytes = Vec::new();
    doc.write_to_with_options(&mut bytes, o).unwrap();
    let h = PdfHandler;
    assert!(matches!(h.extract(&bytes), Err(FormatError::Encrypted)));
    assert!(matches!(h.deep_scan(&bytes), Err(FormatError::Encrypted)));
    let (s, _) = session(&[]);
    assert!(matches!(
        h.transform(&bytes, &replacer(&s), &TransformOptions::anonymize()),
        Err(FormatError::Encrypted)
    ));
}

#[test]
fn balise_trop_longue_avertit() {
    // « Li » (2 lettres) devient une balise longue : elle ne tient pas.
    let mut doc = PdfDocument::new();
    let mut page = doc.new_page((300.0f32, 200.0f32)).unwrap();
    {
        let mut shape = Shape::new(&mut page).unwrap();
        shape
            .insert_text(
                Point::new(20.0, 50.0),
                "Nom : Li et suite du texte.",
                &TextOptions {
                    fontsize: 6.0,
                    fontname: "helv".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        shape.commit(&mut doc, true).unwrap();
    }
    drop(page);
    let mut input = Vec::new();
    doc.write_to(&mut input).unwrap();
    let (s, _) = session(&[(EntityKind::Nom, "Li".to_string())]);
    let out = PdfHandler
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    assert_eq!(out.replacements, 1);
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::TagDidNotFit)
    );
    // Avertissements sans contenu utilisateur.
    for w in &out.warnings {
        assert!(!w.detail.contains("Li ") && !w.part.contains("Li"));
    }
}
