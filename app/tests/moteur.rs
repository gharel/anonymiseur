//! Le moteur livré : formats pris en charge et, dans l'édition complète, IA active.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_core::{DetectOptions, Profile, Session, WarningCode};
use anon_engine::{AnalyzeOptions, AnonymizeOptions, InputFile, ManualMask, NoProgress};
use anonymiseur::moteur::{self, IaStatus};
use std::path::PathBuf;

fn exemple(nom: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../exemples")
            .join(nom),
    )
    .unwrap()
}

/// Texte lu par OCR dans un fichier (contrôle indépendant du moteur).
fn lu_par_ocr(ext: &str, octets: &[u8]) -> String {
    anon_pdf::handler_for(ext)
        .unwrap()
        .deep_scan(octets)
        .unwrap()
        .into_iter()
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Une page de PDF scanné passe par le moteur complet : détection par OCR,
/// zone tracée à la main par l'utilisateur, contrôle de fuite, puis restauration.
#[test]
fn pdf_scanne_zone_manuelle_et_restauration() {
    let m = moteur::build();
    let fichier = InputFile::new("courrier-scanne.pdf", exemple("courrier-scanne.pdf"));
    let avant = lu_par_ocr("pdf", &fichier.bytes);
    assert!(avant.contains("Dupont"), "le scan d'origine doit se lire");
    assert!(avant.contains("Objet"));

    let mut session = Session::new();
    let opts = AnalyzeOptions {
        detect: DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        },
        ..Default::default()
    };
    let analyse = m
        .engine
        .analyze(std::slice::from_ref(&fichier), &opts, &session, &NoProgress);
    let image = analyse
        .images()
        .find(|i| i.file == "courrier-scanne.pdf")
        .expect("la page scannée doit être exposée comme image");
    // L'utilisateur trace une zone sur un mot que la détection ne masque pas.
    let mot = image
        .ocr
        .words
        .iter()
        .find(|w| w.text.starts_with("Objet"))
        .expect("mot « Objet » lu par l'OCR");
    let zone = ManualMask {
        media: image.name.to_owned(),
        rect: mot.rect,
    };
    m.engine.apply_selection(&analyse, &mut session);

    let sortie = m
        .engine
        .anonymize(
            &fichier,
            &analyse,
            &mut session,
            &AnonymizeOptions {
                manual_masks: vec![zone],
            },
        )
        .unwrap();
    let apres = lu_par_ocr("pdf", &sortie.bytes);
    assert!(
        !apres.contains("Dupont"),
        "nom encore lisible après anonymisation"
    );
    assert!(
        !apres.contains("Objet"),
        "la zone manuelle n'a pas été masquée"
    );
    assert!(
        apres.contains("Cordialement"),
        "le reste de la page doit rester lisible"
    );

    // Restauration : soit l'image d'origine revient, soit l'utilisateur a été
    // prévenu qu'elle ne reviendrait pas. Jamais un échec silencieux.
    let prevenu = sortie
        .report
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::ImageNotRestorable);
    let restaure = m
        .engine
        .restore(&InputFile::new(sortie.name.clone(), sortie.bytes), &session)
        .unwrap();
    let revenu = lu_par_ocr("pdf", &restaure.bytes).contains("Dupont");
    println!(
        "PDF scanné : originaux gardés = {}, prévenu non restaurable = {prevenu}, image d'origine revenue = {revenu}",
        session.media.len()
    );
    assert!(
        revenu || prevenu,
        "image non restaurée sans que l'utilisateur ait été prévenu"
    );
}

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
