//! Débit (mots par seconde) sur les blocs de `exemples/contrat-prestation.docx`
//! (extraits dans `tools/model/controle/exemples_blocs.jsonl`), répétés jusqu'à
//! ~10 000 mots, bloc par bloc comme le moteur.
//!
//! `cargo run -p anon-ner --release --example debit`

#![allow(clippy::print_stdout, clippy::print_stderr)]

use anon_ner::{Bundle, NerDetector};
use std::time::Instant;

const TARGET_WORDS: usize = 10_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let raw = std::fs::read_to_string(format!("{root}/tools/model/controle/exemples_blocs.jsonl"))?;
    let mut base = Vec::new();
    for line in raw.lines() {
        let v: serde_json::Value = serde_json::from_str(line)?;
        if v["id"] == "contrat-prestation.docx" {
            base.push(v["text"].as_str().unwrap_or_default().to_string());
        }
    }
    let mut blocks = Vec::new();
    let mut words = 0;
    while words < TARGET_WORDS && !base.is_empty() {
        for b in &base {
            words += b.split_whitespace().count();
            blocks.push(b.clone());
        }
    }
    let d = NerDetector::from_dir(format!("{root}/assets/ner"))?;
    d.warm_up()?;
    println!("{} blocs, {words} mots", blocks.len());
    let run = |label: &str, f: &dyn Fn(&str) -> usize| {
        let t = Instant::now();
        let n: usize = blocks.iter().map(|b| f(b)).sum();
        let s = t.elapsed().as_secs_f64();
        println!(
            "{label:<10} {:>8.0} mots/s  ({s:.2} s, {n} entités)",
            words as f64 / s
        );
    };
    run("fr", &|b| {
        d.find_with(Bundle::Fr, b).map(|v| v.len()).unwrap_or(0)
    });
    run("multi", &|b| {
        d.find_with(Bundle::Multi, b).map(|v| v.len()).unwrap_or(0)
    });
    run("aiguillé", &|b| d.find(b).map(|v| v.len()).unwrap_or(0));
    Ok(())
}
