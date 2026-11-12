//! Aller-retour complet sur les faux fichiers d'exemple, écrit lors de la revue.
//!
//! Si la variable `ANON_REVUE_OUT` désigne un dossier, les fichiers anonymisés et
//! restaurés y sont aussi écrits, pour un contrôle par des outils externes
//! (bibliothèques Python, suite bureautique).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_core::{BlockRole, EntityKind, Session, Source, TransformOptions};
use anon_office::handler_for;
use anon_replace::{Replacer, Restorer, SessionExt};
use serde_json::Value;
use std::path::PathBuf;

const FICHIERS: &[&str] = &[
    "contrat-prestation.docx",
    "report-en.docx",
    "fichier-clients.xlsx",
    "presentation-projet.pptx",
    "notes.txt",
    "brief.md",
    "export.csv",
    "informe-es.txt",
];

fn exemples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exemples")
}

fn session_pour(attendu: &Value, fichier: &str) -> Session {
    let mut session = Session::new();
    let entree = attendu["fichiers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["fichier"] == fichier)
        .unwrap_or_else(|| panic!("{fichier} absent de attendu.json"));
    for e in entree["entites"].as_array().unwrap() {
        if e["ocr"].as_bool() == Some(true) {
            continue;
        }
        let kind = EntityKind::from_prefix(e["type"].as_str().unwrap()).unwrap();
        // Comme le fera le moteur pour les valeurs ambiguës : un prénom ou un nom
        // isolé n'est remplacé que dans sa casse d'origine, sinon « Pierre » ferait
        // remplacer « pierre angulaire ».
        let sensible_casse = matches!(kind, EntityKind::Nom | EntityKind::Prenom);
        session.intern_with_case(
            kind,
            e["valeur"].as_str().unwrap(),
            Source::Manual,
            sensible_casse,
        );
    }
    session
}

#[test]
fn aller_retour_sur_chaque_exemple() {
    let dir = exemples();
    let attendu: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("attendu.json")).unwrap()).unwrap();
    let sortie = std::env::var_os("ANON_REVUE_OUT").map(PathBuf::from);
    if let Some(d) = &sortie {
        std::fs::create_dir_all(d).unwrap();
    }

    for nom in FICHIERS {
        let ext = nom.rsplit('.').next().unwrap();
        let handler = handler_for(ext).unwrap_or_else(|| panic!("pas de gestionnaire pour {ext}"));
        let original = std::fs::read(dir.join(nom)).unwrap();
        let session = session_pour(&attendu, nom);
        assert!(
            !session.entities.is_empty(),
            "{nom} : aucune entité attendue"
        );

        let replacer = Replacer::new(&session);
        let anon = handler
            .transform(&original, &replacer, &TransformOptions::anonymize())
            .unwrap();
        assert!(anon.replacements > 0, "{nom} : aucun remplacement");

        // Contrôle de fuite : plus aucune valeur validée nulle part dans la sortie.
        let fuites: Vec<String> = handler
            .deep_scan(&anon.bytes)
            .unwrap()
            .iter()
            .flat_map(|b| {
                replacer
                    .matches(&b.text)
                    .into_iter()
                    .map(move |m| format!("{} dans {}", m.tag, b.ctx.part))
            })
            .collect();
        assert!(fuites.is_empty(), "{nom} : fuites {fuites:?}");

        // Restauration : les blocs de texte redeviennent ceux de l'original.
        let restorer = Restorer::new(&session);
        let rest = handler
            .transform(&anon.bytes, &restorer, &TransformOptions::restore())
            .unwrap();
        // Les métadonnées retirées (auteur, société) ne reviennent pas : on compare
        // tout le reste, dans l'ordre.
        let corps = |octets: &[u8]| -> Vec<String> {
            handler
                .extract(octets)
                .unwrap()
                .blocks
                .into_iter()
                .filter(|b| b.ctx.role != BlockRole::Metadata)
                .map(|b| b.text)
                .collect()
        };
        let avant = corps(&original);
        let apres = corps(&rest.bytes);
        let rapport = restorer.report();
        assert!(
            rapport.unknown.is_empty(),
            "{nom} : balises inconnues {:?}",
            rapport.unknown
        );
        println!(
            "{nom:<28} remplacements={:<4} avertissements={:<2} blocs={} restaurés={} balises non revues={}",
            anon.replacements,
            anon.warnings.len(),
            avant.len(),
            rapport.restored,
            rapport.unused.len()
        );
        for w in &anon.warnings {
            println!("    avertissement {:?} @ {} : {}", w.code, w.part, w.detail);
        }
        let differents: Vec<(&String, &String)> = avant
            .iter()
            .zip(apres.iter())
            .filter(|(a, b)| a != b)
            .collect();
        println!(
            "    blocs hors métadonnées : {} avant, {} après, {} différents",
            avant.len(),
            apres.len(),
            differents.len()
        );
        for (a, b) in differents.iter().take(6) {
            println!("      avant « {a} »\n      après « {b} »");
        }
        assert_eq!(
            avant.len(),
            apres.len(),
            "{nom} : nombre de blocs différent après restauration"
        );
        assert!(
            differents.is_empty(),
            "{nom} : {} blocs diffèrent après restauration",
            differents.len()
        );

        if let Some(d) = &sortie {
            std::fs::write(d.join(format!("anon-{nom}")), &anon.bytes).unwrap();
            std::fs::write(d.join(format!("rest-{nom}")), &rest.bytes).unwrap();
        }
    }
}
