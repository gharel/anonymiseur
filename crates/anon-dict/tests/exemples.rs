//! Mesure du détecteur à dictionnaires (avec les règles, comme en production) sur
//! les faux fichiers texte de `exemples/`, contre `exemples/attendu.json`.
//! Garde-fou : rappel et faux positifs ne doivent pas se dégrader.

use anon_core::{BlockContext, BlockRole, DetectOptions, EntityKind, Family, Profile, TextBlock};
use anon_detect::{Pipeline, RuleDetector};
use anon_dict::DictionaryDetector;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Rappel minimal sur les personnes (PERSONNE, NOM, PRENOM) dans les textes rédigés.
const MIN_PERSON_RECALL: f64 = 0.80;
/// Rappel minimal sur les sociétés.
const MIN_ORG_RECALL: f64 = 0.70;
/// Faux positifs tolérés sur l'ensemble des fichiers mesurés.
const MAX_FALSE_POSITIVES: usize = 2;

fn exemples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exemples")
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn dict_kind(kind: EntityKind) -> bool {
    matches!(kind.family(), Family::Personnes | Family::Organisations) || kind == EntityKind::Lieu
}

#[test]
fn rappel_des_dictionnaires_sur_les_exemples_texte() {
    let dir = exemples_dir();
    let attendu: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("attendu.json")).unwrap()).unwrap();
    let pipeline = Pipeline::new(vec![
        Box::new(RuleDetector::new()),
        Box::new(DictionaryDetector::new()),
    ]);
    let opts = DetectOptions {
        profile: Profile::Renforce,
        ..Default::default()
    };

    let mut found: BTreeMap<&str, usize> = BTreeMap::new();
    let mut expected: BTreeMap<&str, usize> = BTreeMap::new();
    let mut misses = Vec::new();
    let mut false_positives = Vec::new();
    let mut trap_hits = Vec::new();

    for f in attendu["fichiers"].as_array().unwrap() {
        let name = f["fichier"].as_str().unwrap();
        // Textes rédigés seulement : le CSV relève du détecteur de colonnes.
        if !(name.ends_with(".txt") || name.ends_with(".md")) || name == "reponse-ia-exemple.md" {
            continue;
        }
        let text = std::fs::read_to_string(dir.join(name)).unwrap();
        let mut detected: Vec<(EntityKind, String)> = Vec::new();
        for line in text.lines() {
            let block = TextBlock {
                text: line.to_string(),
                ctx: BlockContext::new(name, BlockRole::Body),
            };
            for d in pipeline.detect(&block, &opts) {
                assert!(line.is_char_boundary(d.start) && line.is_char_boundary(d.end));
                detected.push((d.kind, squash(&line[d.start..d.end])));
            }
        }

        let entites = f["entites"].as_array().unwrap();
        // Toutes les valeurs attendues, tous types confondus : une détection qui
        // recoupe l'une d'elles n'est pas un faux positif (seul le type diffère).
        let all_values: Vec<String> = entites
            .iter()
            .map(|e| squash(e["valeur"].as_str().unwrap()))
            .collect();

        for e in entites {
            let Some(kind) = EntityKind::from_prefix(e["type"].as_str().unwrap()) else {
                continue;
            };
            if !dict_kind(kind) {
                continue;
            }
            let v = squash(e["valeur"].as_str().unwrap());
            *expected.entry(kind.prefix()).or_default() += 1;
            // Trouvée si une détection de la même famille couvre la valeur entière.
            let hit = detected
                .iter()
                .any(|(k, s)| k.family() == kind.family() && s.contains(v.as_str()));
            if hit {
                *found.entry(kind.prefix()).or_default() += 1;
            } else {
                misses.push(format!(
                    "{name}: {} « {} »",
                    kind.prefix(),
                    e["valeur"].as_str().unwrap()
                ));
            }
        }
        for (k, s) in &detected {
            if dict_kind(*k)
                && !all_values
                    .iter()
                    .any(|v| v.contains(s.as_str()) || s.contains(v.as_str()))
            {
                false_positives.push(format!("{name}: {} « {s} »", k.prefix()));
            }
        }
        for piege in f["ne_pas_detecter"].as_array().into_iter().flatten() {
            let p = squash(piege.as_str().unwrap());
            if detected
                .iter()
                .any(|(_, s)| s.contains(&p) || p.contains(s.as_str()))
            {
                trap_hits.push(format!(
                    "{name}: piège « {} » détecté",
                    piege.as_str().unwrap()
                ));
            }
        }
    }

    let ratio = |kinds: &[&str]| {
        let e: usize = kinds
            .iter()
            .map(|k| expected.get(k).copied().unwrap_or(0))
            .sum();
        let f: usize = kinds
            .iter()
            .map(|k| found.get(k).copied().unwrap_or(0))
            .sum();
        (f, e, f as f64 / e.max(1) as f64)
    };
    let (pf, pe, person_recall) = ratio(&["PERSONNE", "NOM", "PRENOM"]);
    let (of, oe, org_recall) = ratio(&["SOCIETE"]);
    let (lf, le, lieu_recall) = ratio(&["LIEU"]);
    println!("\nDictionnaires + règles sur les exemples rédigés :");
    println!(
        "  personnes {pf}/{pe} = {person_recall:.2} | sociétés {of}/{oe} = {org_recall:.2} | lieux {lf}/{le} = {lieu_recall:.2}"
    );
    println!("Manqués ({}) :", misses.len());
    misses.iter().for_each(|m| println!("  - {m}"));
    println!("Faux positifs ({}) :", false_positives.len());
    false_positives.iter().for_each(|m| println!("  - {m}"));
    println!("Pièges touchés ({}) :", trap_hits.len());
    trap_hits.iter().for_each(|m| println!("  - {m}"));

    assert!(pe >= 10, "vérité terrain trop pauvre : {pe} personnes");
    assert!(
        person_recall >= MIN_PERSON_RECALL,
        "rappel personnes {person_recall:.2} < {MIN_PERSON_RECALL}"
    );
    assert!(
        org_recall >= MIN_ORG_RECALL,
        "rappel sociétés {org_recall:.2} < {MIN_ORG_RECALL}"
    );
    assert!(
        false_positives.len() <= MAX_FALSE_POSITIVES,
        "{} faux positifs",
        false_positives.len()
    );
    assert!(trap_hits.is_empty(), "pièges détectés : {trap_hits:?}");
}
