//! Le moteur livré : formats pris en charge et, dans l'édition complète, IA active.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_core::Session;
use anon_engine::{AnalyzeOptions, InputFile, NoProgress};
use anonymiseur::moteur::{self, IaStatus};

#[test]
fn tous_les_formats_sont_pris_en_charge() {
    let m = moteur::build();
    let ext = m.engine.supported_extensions();
    for attendu in [
        "docx", "xlsx", "pptx", "odt", "ods", "odp", "pdf", "png", "jpg", "txt", "csv", "md",
    ] {
        assert!(
            ext.contains(&attendu),
            "extension {attendu} absente : {ext:?}"
        );
    }
}

#[test]
fn etat_de_l_ia_selon_l_edition() {
    let m = moteur::build();
    if cfg!(feature = "ia") {
        assert_eq!(
            m.ia,
            IaStatus::Active,
            "édition complète : les modèles doivent se charger"
        );
    } else {
        assert_eq!(m.ia, IaStatus::Absente);
    }
}

/// Un nom absent des dictionnaires, sans civilité ni prénom connu : seule l'IA
/// peut le trouver. Dans l'édition Lite, le test vérifie seulement l'absence de panique.
#[test]
fn l_ia_trouve_un_nom_que_les_dictionnaires_ignorent() {
    let m = moteur::build();
    let texte = "Le dossier a été transmis hier à Wanyatro Xoxuedr, qui le signera lundi.";
    let fichier = InputFile::new("note.txt", texte.as_bytes().to_vec());
    let analyse = m.engine.analyze(
        &[fichier],
        &AnalyzeOptions::default(),
        &Session::new(),
        &NoProgress,
    );
    let trouve = analyse
        .candidates
        .iter()
        .any(|c| c.value.contains("Xoxuedr"));
    println!("IA : {:?} ; candidats : {}", m.ia, analyse.candidates.len());
    if m.ia == IaStatus::Active {
        assert!(
            trouve,
            "l'IA devrait proposer le nom inconnu des dictionnaires"
        );
    }
}
