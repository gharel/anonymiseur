//! Tests sur les fichiers de `exemples/`, pilotés par `attendu.json`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::text::apply_replacements;
use anon_core::{Handler, TextTransform, TransformOptions, WarningCode};
use anon_office::handler_for;
use anon_replace::{Replacer, Restorer};
use common::*;

const TEXT_FILES: &[&str] = &[
    "notes.txt",
    "export.csv",
    "brief.md",
    "informe-es.txt",
    "reponse-ia-exemple.md",
];
const OFFICE_FILES: &[&str] = &[
    "contrat-prestation.docx",
    "report-en.docx",
    "fichier-clients.xlsx",
    "presentation-projet.pptx",
];

fn handler(file: &str) -> &'static dyn Handler {
    let ext = file.rsplit('.').next().unwrap();
    handler_for(ext).unwrap()
}

/// Chaque valeur non OCR d'`attendu.json` apparaît dans un bloc d'`extract`.
#[test]
fn every_expected_value_is_extracted() {
    for file in TEXT_FILES.iter().chain(OFFICE_FILES) {
        let input = read_example(file);
        let h = handler(file);
        let blocks = h.extract(&input).unwrap().blocks;
        for (kind, value) in expected(file) {
            assert!(
                value_in_blocks(kind, &value, &blocks),
                "{file} : valeur {kind:?} « {value} » absente des blocs"
            );
        }
    }
}

/// Après anonymisation, `deep_scan` ne trouve plus aucune valeur.
#[test]
fn anonymized_output_has_no_leak() {
    for file in TEXT_FILES.iter().chain(OFFICE_FILES) {
        let input = read_example(file);
        let h = handler(file);
        let values = expected(file);
        let session = session_of(&values);
        let replacer = Replacer::new(&session);
        let out = h
            .transform(&input, &replacer, &TransformOptions::anonymize())
            .unwrap();
        assert!(
            !out.warnings
                .iter()
                .any(|w| w.code == WarningCode::SheetNameSensitive),
            "{file} : nom de feuille sensible inattendu"
        );
        let deep = h.deep_scan(&out.bytes).unwrap();
        let found = leaks(&deep, &replacer);
        assert!(found.is_empty(), "{file} : fuites\n{}", found.join("\n"));
        if !values.is_empty() {
            assert!(out.replacements > 0, "{file} : aucun remplacement");
        }
        // Contrôle de bon sens : l'original, lui, contient bien les valeurs.
        if !values.is_empty() {
            assert!(!leaks(&h.deep_scan(&input).unwrap(), &replacer).is_empty());
        }
    }
}

fn canonical(t: &dyn TextTransform, text: &str, ctx: &anon_core::BlockContext) -> String {
    apply_replacements(text, &t.replacements(text, ctx)).into_owned()
}

/// Anonymiser puis restaurer redonne les mêmes blocs (aux formes canoniques près).
#[test]
fn restore_gives_back_same_blocks() {
    for file in TEXT_FILES.iter().chain(OFFICE_FILES) {
        let input = read_example(file);
        let h = handler(file);
        let session = session_of(&expected(file));
        let replacer = Replacer::new(&session);
        let restorer = Restorer::new(&session);
        let anon = h
            .transform(&input, &replacer, &TransformOptions::anonymize())
            .unwrap();
        let back = h
            .transform(&anon.bytes, &restorer, &TransformOptions::restore())
            .unwrap();
        let original = kept_blocks(h, &input);
        let restored = kept_blocks(h, &back.bytes);
        assert_eq!(
            original.len(),
            restored.len(),
            "{file} : nombre de blocs différent"
        );
        for (o, r) in original.iter().zip(&restored) {
            let anon_text = canonical(&replacer, &o.text, &o.ctx);
            let expected = canonical(&restorer, &anon_text, &o.ctx);
            assert_eq!(r.text, expected, "{file} : bloc {}", o.ctx.part);
            assert_eq!(r.ctx.role, o.ctx.role, "{file} : rôle {}", o.ctx.part);
        }
    }
}

/// Sortie zip valide, XML bien formé, ordre conservé, parties non concernées
/// identiques octet pour octet (données compressées comprises).
#[test]
fn package_structure_is_preserved() {
    for file in OFFICE_FILES {
        let input = read_example(file);
        let h = handler(file);
        let session = session_of(&expected(file));
        let out = h
            .transform(
                &input,
                &Replacer::new(&session),
                &TransformOptions::anonymize(),
            )
            .unwrap();
        assert_well_formed(&out.bytes);
        let before = zip_entries(&input);
        let after = zip_entries(&out.bytes);
        let expected_names: Vec<_> = before
            .iter()
            .map(|e| e.0.clone())
            .filter(|n| !n.starts_with("docProps/thumbnail."))
            .collect();
        let names: Vec<_> = after.iter().map(|e| e.0.clone()).collect();
        assert_eq!(names, expected_names, "{file} : ordre des entrées");
        let mut unchanged = 0;
        for (name, data, raw, method) in &after {
            let (_, odata, oraw, omethod) = before.iter().find(|e| &e.0 == name).unwrap();
            assert_eq!(method, omethod, "{file} : méthode de {name}");
            if data == odata {
                unchanged += 1;
                assert_eq!(raw, oraw, "{file} : {name} recompressé sans raison");
            }
        }
        assert!(unchanged > 0, "{file} : aucune partie conservée");
    }
}

/// Transformation neutre (aucune valeur, pas de nettoyage) : sortie identique
/// partie par partie.
#[test]
fn identity_transform_keeps_every_part() {
    let session = session_of(&[]);
    for file in OFFICE_FILES {
        let input = read_example(file);
        let out = handler(file)
            .transform(
                &input,
                &Replacer::new(&session),
                &TransformOptions::restore(),
            )
            .unwrap();
        assert_eq!(out.replacements, 0);
        let before = zip_entries(&input);
        let after = zip_entries(&out.bytes);
        assert_eq!(before.len(), after.len());
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(b.0, a.0);
            assert_eq!(b.2, a.2, "{file} : {}", b.0);
        }
    }
}

/// Images matricielles et fichiers embarqués.
#[test]
fn media_and_embedded_files() {
    let input = read_example("presentation-projet.pptx");
    let ex = handler_for("pptx").unwrap().extract(&input).unwrap();
    assert!(ex.media.iter().any(|m| m.name == "ppt/media/image1.png"));
    assert!(ex.blocks.iter().any(|b| {
        b.ctx
            .part
            .starts_with("ppt/embeddings/Microsoft_Excel_Sheet1.xlsx/xl/sharedStrings.xml")
            || b.ctx
                .part
                .starts_with("ppt/embeddings/Microsoft_Excel_Sheet1.xlsx/xl/worksheets/")
    }));

    struct Gray;
    impl anon_core::MediaTransform for Gray {
        fn apply(&self, name: &str, _bytes: &[u8]) -> Option<Vec<u8>> {
            (name == "ppt/media/image1.png").then(|| b"masque".to_vec())
        }
    }
    let session = session_of(&[]);
    let r = Replacer::new(&session);
    let opts = TransformOptions {
        media: Some(&Gray),
        ..TransformOptions::anonymize()
    };
    let out = handler_for("pptx")
        .unwrap()
        .transform(&input, &r, &opts)
        .unwrap();
    let e = zip_entries(&out.bytes);
    assert_eq!(
        e.iter().find(|x| x.0 == "ppt/media/image1.png").unwrap().1,
        b"masque"
    );
}
