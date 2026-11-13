//! Aller-retour sur les exemples PDF et image, écrit lors de la revue.
//!
//! Si `ANON_REVUE_OUT` désigne un dossier, les fichiers anonymisés et restaurés y
//! sont écrits pour un contrôle par des outils externes et à l'œil.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_core::{BlockRole, EntityKind, Session, Source, TransformOptions};
use anon_pdf::handler_for;
use anon_replace::{Replacer, Restorer, SessionExt};
use serde_json::Value;
use std::path::PathBuf;

const FICHIERS: &[&str] = &["facture.pdf", "courrier-scanne.pdf", "carte-visite.png"];

fn exemples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exemples")
}

#[test]
fn aller_retour_pdf_et_image() {
    let dir = exemples();
    let attendu: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("attendu.json")).unwrap()).unwrap();
    let sortie = std::env::var_os("ANON_REVUE_OUT").map(PathBuf::from);
    if let Some(d) = &sortie {
        std::fs::create_dir_all(d).unwrap();
    }

    for nom in FICHIERS {
        let ext = nom.rsplit('.').next().unwrap();
        let handler = handler_for(ext).unwrap();
        let original = std::fs::read(dir.join(nom)).unwrap();

        // Toutes les valeurs attendues, y compris celles qui n'existent que dans l'image.
        let mut session = Session::new();
        let entree = attendu["fichiers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["fichier"] == *nom)
            .unwrap();
        for e in entree["entites"].as_array().unwrap() {
            let kind = EntityKind::from_prefix(e["type"].as_str().unwrap()).unwrap();
            session.intern(kind, e["valeur"].as_str().unwrap(), Source::Manual);
        }

        let replacer = Replacer::new(&session);
        // Occurrences dans le texte du document, hors métadonnées : l'auteur est
        // retiré à l'anonymisation et ne revient pas. Une image seule ou une page
        // scannée n'a pas de bloc de texte (elle est rendue comme image) : 0.
        let occurrences = |octets: &[u8]| -> usize {
            handler
                .extract(octets)
                .unwrap()
                .blocks
                .iter()
                .filter(|b| b.ctx.role != BlockRole::Metadata)
                .map(|b| replacer.matches(&b.text).len())
                .sum()
        };
        let lu = occurrences(&original);
        let anon = handler
            .transform(&original, &replacer, &TransformOptions::anonymize())
            .unwrap();

        let fuites: Vec<String> = handler
            .deep_scan(&anon.bytes)
            .unwrap()
            .iter()
            .flat_map(|b| {
                replacer
                    .matches(&b.text)
                    .into_iter()
                    .map(move |m| format!("{} @ {}", m.tag, b.ctx.part))
            })
            .collect();
        println!(
            "{nom:<22} valeurs={:<3} occurrences lues={lu:<3} remplacements={:<3} avertissements={} fuites={}",
            session.entities.len(),
            anon.replacements,
            anon.warnings.len(),
            fuites.len()
        );
        for w in &anon.warnings {
            println!("    avertissement {:?} @ {} : {}", w.code, w.part, w.detail);
        }
        assert!(anon.replacements > 0, "{nom} : aucun remplacement");
        assert!(fuites.is_empty(), "{nom} : fuites {fuites:?}");

        if let Some(d) = &sortie {
            std::fs::write(d.join(format!("anon-{nom}")), &anon.bytes).unwrap();
        }
        if *nom == "facture.pdf" {
            let restorer = Restorer::new(&session);
            let rest = handler
                .transform(&anon.bytes, &restorer, &TransformOptions::restore())
                .unwrap();
            let rapport = restorer.report();
            let relu = occurrences(&rest.bytes);
            println!(
                "    restauration : {} balises remises, {} inconnues, {relu} occurrences relues",
                rapport.restored,
                rapport.unknown.len()
            );
            assert!(rapport.unknown.is_empty());
            assert_eq!(
                relu, lu,
                "{nom} : {relu} occurrences après restauration, {lu} avant"
            );
            if let Some(d) = &sortie {
                std::fs::write(d.join(format!("rest-{nom}")), &rest.bytes).unwrap();
            }
        }
    }
}
