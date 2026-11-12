//! Mesure de bout en bout écrite lors de la revue : que reste-t-il dans les
//! fichiers anonymisés quand l'utilisateur **ne corrige rien** à l'écran de
//! validation (sélection par défaut) ? C'est le pire cas réaliste.
//!
//! Si `ANON_REVUE_OUT` désigne un dossier, les fichiers produits y sont écrits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_core::{DetectOptions, Profile, Session};
use anon_engine::{AnalyzeOptions, AnonymizeOptions, Engine, InputFile, NoProgress};
use serde_json::Value;
use std::path::PathBuf;

/// Part maximale des valeurs attendues encore lisibles après anonymisation
/// sans aucune correction humaine. Garde-fou de non-régression.
const MAX_RESIDUAL_STANDARD: f64 = 0.03;
const MAX_RESIDUAL_RENFORCE: f64 = 0.06;

fn exemples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exemples")
}

fn mesure(profile: Profile) -> (usize, usize) {
    let dir = exemples();
    let attendu: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("attendu.json")).unwrap()).unwrap();
    let engine = Engine::standard();
    let gere = engine.supported_extensions();

    let entrees: Vec<&Value> = attendu["fichiers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            let nom = f["fichier"].as_str().unwrap();
            nom != "reponse-ia-exemple.md" && gere.contains(&nom.rsplit('.').next().unwrap())
        })
        .collect();
    let files: Vec<InputFile> = entrees
        .iter()
        .map(|f| {
            let nom = f["fichier"].as_str().unwrap();
            InputFile::new(nom, std::fs::read(dir.join(nom)).unwrap())
        })
        .collect();

    // Une seule session pour tous les fichiers, sélection par défaut, aucune retouche.
    let mut session = Session::new();
    let opts = AnalyzeOptions {
        detect: DetectOptions {
            profile,
            ..Default::default()
        },
        ..Default::default()
    };
    let analysis = engine.analyze(&files, &opts, &session, &NoProgress);
    engine.apply_selection(&analysis, &mut session);

    let sortie = std::env::var_os("ANON_REVUE_OUT").map(PathBuf::from);
    let (mut total, mut restantes) = (0usize, 0usize);
    println!("\nProfil {profile:?} — sélection par défaut, sans correction humaine");
    for (file, entree) in files.iter().zip(&entrees) {
        let ext = file.extension();
        let anon =
            match engine.anonymize(file, &analysis, &mut session, &AnonymizeOptions::default()) {
                Ok(a) => a,
                Err(e) => {
                    println!("  {:<28} REFUSÉ : {e}", file.name);
                    continue;
                }
            };
        if let Some(d) = &sortie {
            let d = d.join(format!("{profile:?}").to_lowercase());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(&anon.name), &anon.bytes).unwrap();
        }
        let handler = anon_office::handler_for(&ext).unwrap();
        let mut texte = handler
            .deep_scan(&anon.bytes)
            .unwrap()
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        // Un piège (« Martinique ») n'est pas une fuite de « Martin ».
        for piege in entree["ne_pas_detecter"].as_array().into_iter().flatten() {
            texte = texte.replace(&piege.as_str().unwrap().to_lowercase(), "");
        }
        let mut manquees = Vec::new();
        let mut n = 0usize;
        for e in entree["entites"].as_array().unwrap() {
            if e["ocr"].as_bool() == Some(true) {
                continue;
            }
            if profile == Profile::Standard && e["profil"] == "renforce" {
                continue;
            }
            n += 1;
            let v = e["valeur"].as_str().unwrap();
            if texte.contains(&v.to_lowercase()) {
                manquees.push(format!("{} « {v} »", e["type"].as_str().unwrap()));
            }
        }
        total += n;
        restantes += manquees.len();
        println!(
            "  {:<28} {:>3} valeurs, {:>2} encore lisibles | risque {:?} | {} avertissements",
            file.name,
            n,
            manquees.len(),
            anon.report.risk.level,
            anon.report.warnings.len()
        );
        for m in &manquees {
            println!("      reste : {m}");
        }
    }
    let part = restantes as f64 / total.max(1) as f64;
    println!(
        "  TOTAL : {restantes}/{total} valeurs encore lisibles ({:.1} %)",
        part * 100.0
    );
    (restantes, total)
}

#[test]
fn valeurs_restantes_sans_correction_humaine() {
    let (r, t) = mesure(Profile::Standard);
    assert!(t >= 150, "vérité terrain trop pauvre : {t}");
    assert!(
        (r as f64 / t as f64) <= MAX_RESIDUAL_STANDARD,
        "profil Standard : {r}/{t} valeurs restent lisibles"
    );
    let (r, t) = mesure(Profile::Renforce);
    assert!(
        (r as f64 / t as f64) <= MAX_RESIDUAL_RENFORCE,
        "profil Renforcé : {r}/{t} valeurs restent lisibles"
    );
}
