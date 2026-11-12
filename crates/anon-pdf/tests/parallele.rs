#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! `Handler` et `ImageProcessor` sont `Sync` : plusieurs traitements en parallèle
//! (MuPDF par fil, OCR sérialisé) donnent les mêmes résultats qu'en série.

mod common;

use std::sync::Arc;

use anon_core::{ImageProcessor, TransformOptions};
use anon_pdf::{MupdfImageProcessor, handler_for};
use common::*;

fn run(job: usize) -> String {
    match job % 4 {
        0 => {
            let values = expected("facture.pdf");
            let (s, _) = session(&values);
            let h = handler_for("pdf").unwrap();
            let out = h
                .transform(
                    &exemple("facture.pdf"),
                    &replacer(&s),
                    &TransformOptions::anonymize(),
                )
                .unwrap();
            let deep = h.deep_scan(&out.bytes).unwrap();
            format!("{} {}", out.replacements, joined(&deep).contains("Dupont"))
        }
        1 => {
            let h = handler_for("pdf").unwrap();
            joined(&h.extract(&exemple("courrier-scanne.pdf")).unwrap().blocks)
        }
        2 => {
            let page = MupdfImageProcessor
                .ocr(&exemple("carte-visite.png"))
                .unwrap();
            page.text_with_spans().0
        }
        _ => {
            let (s, _) = session(&[(anon_core::EntityKind::Personne, "Jean Dupont".into())]);
            let out = handler_for("pdf")
                .unwrap()
                .transform(
                    &fixture("piege.pdf"),
                    &replacer(&s),
                    &TransformOptions::anonymize(),
                )
                .unwrap();
            out.replacements.to_string()
        }
    }
}

#[test]
fn traitements_paralleles() {
    let sequential: Vec<String> = (0..4).map(run).collect();
    let results = Arc::new(std::sync::Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for job in 0..12 {
            let results = Arc::clone(&results);
            scope.spawn(move || {
                let r = run(job);
                results.lock().unwrap().push((job % 4, r));
            });
        }
    });
    let results = results.lock().unwrap();
    assert_eq!(results.len(), 12);
    for (kind, r) in results.iter() {
        assert_eq!(
            r, &sequential[*kind],
            "résultat différent en parallèle (tâche {kind})"
        );
    }
    assert!(
        sequential[0].ends_with("false"),
        "fuite : {}",
        sequential[0]
    );
}
