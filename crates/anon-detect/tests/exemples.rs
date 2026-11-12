//! Mesure des règles sur les faux fichiers texte de `exemples/`, contre la vérité
//! terrain `exemples/attendu.json`. Garde-fou : le rappel par type ne doit pas baisser.

use anon_core::{BlockContext, BlockRole, DetectOptions, EntityKind, TextBlock};
use anon_detect::{Pipeline, RuleDetector};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Types que les règles sont censées trouver seules.
const RULE_KINDS: &[EntityKind] = &[
    EntityKind::Email,
    EntityKind::Tel,
    EntityKind::Iban,
    EntityKind::Nir,
    EntityKind::Siret,
    EntityKind::Ridet,
    EntityKind::Cb,
    EntityKind::Ip,
    EntityKind::Url,
    EntityKind::Adresse,
    EntityKind::DateNaiss,
    EntityKind::Plaque,
];

/// Rappel minimal exigé, tous types de règles confondus, sur les fichiers texte.
const MIN_RECALL: f64 = 0.90;
/// Nombre maximal de détections qui ne correspondent à aucune valeur attendue.
const MAX_FALSE_POSITIVES: usize = 3;

fn exemples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exemples")
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

#[test]
fn rappel_des_regles_sur_les_exemples_texte() {
    let dir = exemples_dir();
    let attendu: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("attendu.json")).unwrap()).unwrap();
    let pipeline = Pipeline::new(vec![Box::new(RuleDetector::new())]);
    let opts = DetectOptions::default();

    let mut found: BTreeMap<&str, usize> = BTreeMap::new();
    let mut expected: BTreeMap<&str, usize> = BTreeMap::new();
    let mut misses = Vec::new();
    let mut false_positives = Vec::new();
    let mut trap_hits = Vec::new();

    for f in attendu["fichiers"].as_array().unwrap() {
        let name = f["fichier"].as_str().unwrap();
        if !["txt", "md", "csv"]
            .iter()
            .any(|e| name.ends_with(&format!(".{e}")))
        {
            continue;
        }
        if name == "reponse-ia-exemple.md" {
            continue;
        }
        let text = std::fs::read_to_string(dir.join(name)).unwrap();

        // Détections par type, sous forme compacte.
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
        for kind in RULE_KINDS {
            let values: Vec<String> = entites
                .iter()
                .filter(|e| e["type"].as_str() == Some(kind.prefix()))
                .map(|e| squash(e["valeur"].as_str().unwrap()))
                .collect();
            for v in &values {
                *expected.entry(kind.prefix()).or_default() += 1;
                let hit = detected
                    .iter()
                    .any(|(k, s)| k == kind && (s == v || s.contains(v.as_str())));
                if hit {
                    *found.entry(kind.prefix()).or_default() += 1;
                } else {
                    misses.push(format!("{name}: {} « {v} »", kind.prefix()));
                }
            }
            for (k, s) in &detected {
                if k == kind
                    && !values
                        .iter()
                        .any(|v| s == v || s.contains(v.as_str()) || v.contains(s.as_str()))
                {
                    false_positives.push(format!("{name}: {} « {s} »", k.prefix()));
                }
            }
        }

        for piege in f["ne_pas_detecter"].as_array().into_iter().flatten() {
            let p = squash(piege.as_str().unwrap());
            if detected
                .iter()
                .any(|(_, s)| s.contains(&p) || p.contains(s.as_str()))
            {
                trap_hits.push(format!("{name}: piège « {p} » détecté"));
            }
        }
    }

    let total_expected: usize = expected.values().sum();
    let total_found: usize = found.values().sum();
    let recall = total_found as f64 / total_expected.max(1) as f64;
    println!(
        "\nRègles sur les exemples texte : rappel {total_found}/{total_expected} = {recall:.2}"
    );
    for (k, e) in &expected {
        println!("  {k:<12} {}/{e}", found.get(k).copied().unwrap_or(0));
    }
    println!("Manqués ({}) :", misses.len());
    misses.iter().for_each(|m| println!("  - {m}"));
    println!("Faux positifs ({}) :", false_positives.len());
    false_positives.iter().for_each(|m| println!("  - {m}"));
    println!("Pièges touchés ({}) :", trap_hits.len());
    trap_hits.iter().for_each(|m| println!("  - {m}"));

    assert!(
        total_expected >= 20,
        "vérité terrain trop pauvre : {total_expected}"
    );
    assert!(recall >= MIN_RECALL, "rappel {recall:.2} < {MIN_RECALL}");
    assert!(
        false_positives.len() <= MAX_FALSE_POSITIVES,
        "{} faux positifs",
        false_positives.len()
    );
    assert!(trap_hits.is_empty(), "pièges détectés : {trap_hits:?}");
}
