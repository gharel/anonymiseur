//! Mesure de détection sur `exemples/`, contre `exemples/attendu.json`.
//!
//! Pour chaque fichier, profil Standard puis Renforcé : rappel et précision par
//! type, sur la sélection par défaut. Les chiffres sont comparés à
//! `tests/baseline.json` (racine du dépôt) ; le test échoue si un score baisse
//! de plus de [`TOLERANCE`]. Mise à jour : `ANON_BASELINE_UPDATE=1 cargo test -p
//! anon-engine --test baseline -- --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

mod common;

use anon_core::{EntityKind, Profile, Session};
use anon_engine::Engine;
use anon_replace::{Replacer, SessionExt};
use common::*;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Baisse tolérée d'un score (rappel ou précision) par rapport à la référence.
const TOLERANCE: f64 = 0.02;
/// Variable d'environnement qui autorise la réécriture de la référence.
const UPDATE_VAR: &str = "ANON_BASELINE_UPDATE";

#[derive(Default, Clone, Copy)]
struct Score {
    expected: usize,
    found: usize,
    wrong_type: usize,
    selected: usize,
    true_pos: usize,
}

impl Score {
    fn recall(&self) -> Option<f64> {
        (self.expected > 0).then(|| self.found as f64 / self.expected as f64)
    }
    fn precision(&self) -> Option<f64> {
        (self.selected > 0).then(|| self.true_pos as f64 / self.selected as f64)
    }
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn fmt(x: Option<f64>) -> String {
    x.map_or_else(|| "  -  ".into(), |v| format!("{v:.3}"))
}

/// Vrai si tous les caractères alphanumériques de `text` sont couverts.
fn fully_covered(text: &str, ranges: &[(usize, usize)]) -> bool {
    text.char_indices()
        .filter(|(_, c)| c.is_alphanumeric())
        .all(|(i, _)| ranges.iter().any(|(s, e)| *s <= i && i < *e))
}

struct Outcome {
    scores: BTreeMap<EntityKind, Score>,
    misses: Vec<String>,
    false_pos: Vec<String>,
    type_gaps: Vec<String>,
    trap_hits: Vec<String>,
}

fn measure(engine: &Engine, file: &ExampleFile, profile: Profile) -> Outcome {
    let analysis = analyze(engine, &[input(&file.name)], profile);
    assert!(
        analysis.files[0].error.is_none(),
        "{} : {:?}",
        file.name,
        analysis.files[0].error
    );
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let replacer = Replacer::new(&session);
    let mut scores: BTreeMap<EntityKind, Score> = BTreeMap::new();
    let mut misses = Vec::new();
    let mut type_gaps = Vec::new();

    for e in file.expected(profile) {
        let s = scores.entry(e.kind).or_default();
        s.expected += 1;
        let target = squash(&e.value);
        // Remplacée entièrement par le Replacer de la session…
        let ms = replacer.matches(&e.value);
        let ranges: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        let mut kinds: Vec<EntityKind> = Vec::new();
        if !ms.is_empty() && fully_covered(&e.value, &ranges) {
            kinds.extend(ms.iter().map(|m| m.tag.kind));
        }
        // … ou contenue dans une valeur sélectionnée plus large.
        for c in analysis.selected() {
            if std::iter::once(&c.value)
                .chain(c.variants.iter())
                .any(|v| squash(v).contains(&target))
            {
                kinds.push(c.kind);
            }
        }
        if kinds.is_empty() {
            misses.push(format!("{} {:?} « {} »", file.name, e.kind, e.value));
            continue;
        }
        s.found += 1;
        if !kinds.contains(&e.kind) {
            s.wrong_type += 1;
            type_gaps.push(format!(
                "{} « {} » attendu {:?}, trouvé {:?}",
                file.name, e.value, e.kind, kinds
            ));
        }
    }

    // Précision : une valeur sélectionnée est juste si elle recoupe une valeur
    // attendue du fichier (tout type, tout profil, OCR compris).
    let all: Vec<String> = file.entities.iter().map(|e| squash(&e.value)).collect();
    let mut false_pos = Vec::new();
    for c in analysis.selected() {
        let s = scores.entry(c.kind).or_default();
        s.selected += 1;
        let k = squash(&c.value);
        if !k.is_empty() && all.iter().any(|a| a.contains(&k) || k.contains(a.as_str())) {
            s.true_pos += 1;
        } else {
            false_pos.push(format!("{} {:?} « {} »", file.name, c.kind, c.value));
        }
    }

    // Pièges : jamais sélectionnés, jamais remplacés.
    let mut trap_hits = Vec::new();
    for t in file.traps.iter().filter(|t| !t.contains('[')) {
        let tk = anon_replace::key(t);
        let selected = analysis
            .selected()
            .any(|c| anon_replace::key(&c.value) == tk);
        if selected
            || !replacer.matches(t).is_empty()
            || session.tag_for(EntityKind::Autre, t).is_some()
        {
            trap_hits.push(format!("{} : piège « {} » ({profile:?})", file.name, t));
        }
    }
    Outcome {
        scores,
        misses,
        false_pos,
        type_gaps,
        trap_hits,
    }
}

fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::Standard => "standard",
        Profile::Renforce => "renforce",
    }
}

#[test]
fn mesure_de_detection() {
    let engine = Engine::standard();
    let files = handled(&engine);
    assert!(!files.is_empty());
    let mut report = Map::new();
    let mut totals: BTreeMap<&str, BTreeMap<EntityKind, Score>> = BTreeMap::new();
    let mut all_traps = Vec::new();

    for profile in [Profile::Standard, Profile::Renforce] {
        let name = profile_name(profile);
        let mut agg: BTreeMap<EntityKind, Score> = BTreeMap::new();
        println!("\n=== Profil {name} : valeurs manquées, en trop, écarts de type ===");
        for f in &files {
            let out = measure(&engine, f, profile);
            for (k, s) in out.scores {
                let a = agg.entry(k).or_default();
                a.expected += s.expected;
                a.found += s.found;
                a.wrong_type += s.wrong_type;
                a.selected += s.selected;
                a.true_pos += s.true_pos;
            }
            for m in &out.misses {
                println!("  manquée : {m}");
            }
            for p in &out.false_pos {
                println!("  en trop : {p}");
            }
            for g in &out.type_gaps {
                println!("  type    : {g}");
            }
            all_traps.extend(out.trap_hits);
        }
        totals.insert(name, agg);
    }

    println!("\n{:<12} | {:^27} | {:^27}", "", "Standard", "Renforcé");
    println!(
        "{:<12} | {:>5} {:>6} {:>6} {:>7} | {:>5} {:>6} {:>6} {:>7}",
        "type", "att.", "rapp.", "préc.", "écart", "att.", "rapp.", "préc.", "écart"
    );
    let kinds: std::collections::BTreeSet<EntityKind> =
        totals.values().flat_map(|m| m.keys().copied()).collect();
    for k in &kinds {
        let s = totals["standard"].get(k).copied().unwrap_or_default();
        let r = totals["renforce"].get(k).copied().unwrap_or_default();
        println!(
            "{:<12} | {:>5} {:>6} {:>6} {:>7} | {:>5} {:>6} {:>6} {:>7}",
            k.prefix(),
            s.expected,
            fmt(s.recall()),
            fmt(s.precision()),
            s.wrong_type,
            r.expected,
            fmt(r.recall()),
            fmt(r.precision()),
            r.wrong_type
        );
    }

    for (name, agg) in &totals {
        let mut m = Map::new();
        let mut global = Score::default();
        for (k, s) in agg {
            global.expected += s.expected;
            global.found += s.found;
            global.selected += s.selected;
            global.true_pos += s.true_pos;
            m.insert(
                k.prefix().to_string(),
                json!({
                    "attendus": s.expected,
                    "rappel": s.recall().map(round),
                    "precision": s.precision().map(round),
                }),
            );
        }
        println!(
            "Global {name} : rappel {} précision {}",
            fmt(global.recall()),
            fmt(global.precision())
        );
        m.insert(
            "_GLOBAL".into(),
            json!({
                "attendus": global.expected,
                "rappel": global.recall().map(round),
                "precision": global.precision().map(round),
            }),
        );
        report.insert((*name).to_string(), Value::Object(m));
    }
    let current = json!({
        "tolerance": TOLERANCE,
        "profils": Value::Object(report),
    });

    assert!(
        all_traps.is_empty(),
        "pièges sélectionnés ou remplacés :\n{}",
        all_traps.join("\n")
    );

    let path = root().join("tests").join("baseline.json");
    if std::env::var(UPDATE_VAR).is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&current).unwrap() + "\n",
        )
        .unwrap();
        println!("Référence mise à jour : {}", path.display());
        return;
    }
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("référence absente : lancez une fois avec {UPDATE_VAR}=1"));
    let base: Value = serde_json::from_str(&raw).unwrap();
    let mut drops = Vec::new();
    for (profile, kinds) in base["profils"].as_object().unwrap() {
        for (kind, scores) in kinds.as_object().unwrap() {
            for metric in ["rappel", "precision"] {
                let Some(old) = scores[metric].as_f64() else {
                    continue;
                };
                let new = current["profils"][profile][kind][metric].as_f64();
                match new {
                    Some(n) if n + TOLERANCE >= old => {}
                    _ => drops.push(format!("{profile} {kind} {metric} : {old} -> {new:?}")),
                }
            }
        }
    }
    assert!(
        drops.is_empty(),
        "scores en baisse par rapport à tests/baseline.json :\n{}",
        drops.join("\n")
    );
}
