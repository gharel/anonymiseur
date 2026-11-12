//! Robustesse : fichiers vides, inconnus, anciens, corrompus ; annulation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{Profile, Session};
use anon_engine::{AnonymizeOptions, Engine, EngineError, InputFile, Progress};
use common::*;
use std::sync::Mutex;

/// 8. Erreurs par fichier au milieu d'un lot, jamais de panique.
#[test]
fn lot_avec_fichiers_illisibles() {
    let engine = Engine::standard();
    let mut contrat = input("contrat-prestation.docx").bytes;
    contrat.truncate(contrat.len() / 2);
    let files = vec![
        input("notes.txt"),
        InputFile::new("vide.docx", Vec::new()),
        InputFile::new("inconnu.zzz", b"abc".to_vec()),
        InputFile::new("ancien.doc", vec![0xD0, 0xCF, 0x11, 0xE0]),
        InputFile::new("raccourci.gdoc", b"{\"url\":\"x\"}".to_vec()),
        InputFile::new("corrompu.docx", b"PK\x03\x04 pas un zip".to_vec()),
        InputFile::new("tronque.docx", contrat),
        InputFile::new("sans-extension", b"Jean Dupont".to_vec()),
        input("brief.md"),
    ];
    let analysis = analyze(&engine, &files, Profile::Standard);
    let errs: Vec<Option<&EngineError>> = analysis.files.iter().map(|f| f.error.as_ref()).collect();
    assert!(errs[0].is_none());
    assert!(matches!(errs[1], Some(EngineError::Empty)));
    assert!(matches!(errs[2], Some(EngineError::Unsupported { extension }) if extension == "zzz"));
    let doc = errs[3].unwrap().to_string();
    assert!(
        doc.contains("réenregistrez") && doc.contains(".docx"),
        "{doc}"
    );
    assert!(errs[4].unwrap().to_string().contains("Télécharger"));
    assert!(matches!(errs[5], Some(EngineError::Format(_))));
    assert!(matches!(errs[6], Some(EngineError::Format(_))));
    assert!(
        matches!(errs[7], Some(EngineError::Unsupported { extension }) if extension.is_empty())
    );
    assert!(errs[8].is_none());
    // Les fichiers lisibles sont analysés.
    assert!(
        analysis
            .candidates
            .iter()
            .any(|c| c.files.iter().any(|f| f == "notes.txt"))
    );
    assert!(
        analysis
            .candidates
            .iter()
            .any(|c| c.files.iter().any(|f| f == "brief.md"))
    );
    // Aucun message d'erreur ne contient de contenu.
    for e in errs.into_iter().flatten() {
        assert!(!e.to_string().contains("Dupont"));
    }

    // Les autres opérations renvoient une erreur, sans paniquer.
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    for f in &files[1..8] {
        assert!(
            engine
                .anonymize(f, &analysis, &mut session, &AnonymizeOptions::default())
                .is_err()
        );
        assert!(engine.restore(f, &session).is_err());
        assert!(engine.export_text(f, &session).is_err());
    }
}

/// Fichiers tronqués à plusieurs longueurs : jamais de panique.
#[test]
fn fichiers_tronques() {
    let engine = Engine::standard();
    for f in handled(&engine) {
        let full = input(&f.name).bytes;
        for cut in [1, 10, 100, full.len() / 3, full.len() - 1] {
            let file = InputFile::new(f.name.clone(), full[..cut.min(full.len())].to_vec());
            let analysis = analyze(&engine, std::slice::from_ref(&file), Profile::Renforce);
            let mut session = Session::new();
            engine.apply_selection(&analysis, &mut session);
            let _ = engine.anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default());
            let _ = engine.restore(&file, &session);
        }
    }
}

/// Annule après le premier fichier.
struct CancelAfterFirst {
    steps: Mutex<Vec<(usize, usize)>>,
}

impl Progress for CancelAfterFirst {
    fn step(&self, done: usize, total: usize, _label: &str) {
        self.steps.lock().unwrap().push((done, total));
    }
    fn cancelled(&self) -> bool {
        !self.steps.lock().unwrap().is_empty()
    }
}

#[test]
fn annulation_entre_deux_fichiers() {
    let engine = Engine::standard();
    let files = vec![
        input("notes.txt"),
        input("brief.md"),
        input("contrat-prestation.docx"),
    ];
    let progress = CancelAfterFirst {
        steps: Mutex::new(Vec::new()),
    };
    let analysis = engine.analyze(&files, &opts(Profile::Standard), &Session::new(), &progress);
    assert!(analysis.cancelled);
    assert!(analysis.files[0].error.is_none());
    assert!(matches!(
        analysis.files[1].error,
        Some(EngineError::Cancelled)
    ));
    assert!(matches!(
        analysis.files[2].error,
        Some(EngineError::Cancelled)
    ));
    assert!(
        analysis
            .candidates
            .iter()
            .all(|c| c.files.iter().all(|f| f == "notes.txt"))
    );
    assert_eq!(progress.steps.lock().unwrap()[0], (0, 3));
}

#[test]
fn extensions_gerees() {
    let engine = Engine::standard();
    let exts = engine.supported_extensions();
    for e in ["docx", "xlsx", "pptx", "txt", "md", "csv"] {
        assert!(exts.contains(&e), "{e}");
    }
    assert!(!exts.contains(&"doc"));
    assert!(!exts.contains(&"pdf"));
    let mut sorted = exts.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, exts);
}

/// Les identifiants et extraits ne fuient pas par `Debug`.
#[test]
fn debug_sans_contenu() {
    let engine = Engine::standard();
    let analysis = engine.analyze_text(
        "Contrat signé par Jean Dupont, jean.dupont@example.com.",
        &opts(Profile::Standard),
        &Session::new(),
    );
    assert!(!analysis.candidates.is_empty());
    let dbg = format!("{analysis:?} {engine:?}");
    assert!(
        !dbg.contains("Dupont") && !dbg.contains("example.com"),
        "{dbg}"
    );
    let file = InputFile::new("Jean Dupont.txt", b"x".to_vec());
    assert!(!format!("{file:?}").contains("Dupont"));
}
