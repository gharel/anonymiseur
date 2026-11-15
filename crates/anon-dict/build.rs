//! Construit un FST par liste de `data/` dans `OUT_DIR`. Les listes sont déjà
//! pliées par `tools/prepare_dicts.py` ; le build ne fait que les trier,
//! dédoublonner (palier maximal) et les compiler.
//!
//! Populations des communes (`tools/prepare_population.py`) : un FST dont la clé
//! est `nom plié + 0x00 + rang de l'homonyme` et la valeur
//! `population << 16 | rang de la zone dans population_zones.txt`.
//!
//! Population de chaque zone (`population_zone_totaux.txt`) : somme des
//! communes de la zone, une seule fois chacune. Le fichier des communes répète
//! une commune sous plusieurs noms (avec et sans article, graphies
//! calédoniennes : « le mont dore » et « mont dore », « bouloupari » et
//! « boulouparis ») et donne Paris, Lyon et Marseille avec leurs
//! arrondissements : les arrondissements sont écartés, et deux lignes de même
//! zone et de même population dont l'un des noms (sans article) commence
//! l'autre comptent pour une.
// Les directives `cargo:` passent par la sortie standard.
#![allow(clippy::print_stdout)]

use fst::MapBuilder;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

const LISTS: &[&str] = &["prenoms", "noms", "communes", "mots_courants"];

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    println!("cargo:rerun-if-changed=build.rs");
    for name in LISTS {
        let src = manifest.join("data").join(format!("{name}.txt"));
        println!("cargo:rerun-if-changed={}", src.display());
        build_one(&src, &out.join(format!("{name}.fst")))?;
    }
    let zones = manifest.join("data").join("population_zones.txt");
    let communes = manifest.join("data").join("population_communes.txt");
    println!("cargo:rerun-if-changed={}", zones.display());
    println!("cargo:rerun-if-changed={}", communes.display());
    build_population(&zones, &communes, &out.join("population.fst"))?;
    build_zone_totals(&zones, &communes, &out.join("population_zone_totaux.txt"))?;
    Ok(())
}

/// Nom sans article initial.
fn without_article(name: &str) -> &str {
    ["le ", "la ", "les ", "l'"]
        .iter()
        .find_map(|a| name.strip_prefix(a))
        .unwrap_or(name)
}

/// Vrai pour un arrondissement municipal (« lyon 3e arrondissement »).
fn is_arrondissement(name: &str) -> bool {
    name.strip_suffix(" arrondissement")
        .and_then(|head| head.rsplit(' ').next())
        .is_some_and(|rank| {
            let digits = rank.trim_end_matches("er").trim_end_matches('e');
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
        })
}

fn build_zone_totals(zones: &Path, communes: &Path, dst: &Path) -> Result<(), Box<dyn Error>> {
    let zones_text = fs::read_to_string(zones)?;
    let codes: Vec<&str> = data_lines(&zones_text)
        .map(|l| l.split('\t').next().ok_or("zone sans code"))
        .collect::<Result<_, _>>()?;
    let text = fs::read_to_string(communes)?;
    // (zone, population) -> noms sans article déjà comptés.
    let mut seen: BTreeMap<(&str, u64), Vec<&str>> = BTreeMap::new();
    let mut totals: BTreeMap<&str, u64> = BTreeMap::new();
    for line in data_lines(&text) {
        let mut f = line.split('\t');
        let (Some(name), Some(pop), Some(zone)) = (f.next(), f.next(), f.next()) else {
            return Err(format!("ligne de commune invalide : {line}").into());
        };
        if is_arrondissement(name) {
            continue;
        }
        let pop: u64 = pop.parse()?;
        let bare = without_article(name);
        let names = seen.entry((zone, pop)).or_default();
        if names
            .iter()
            .any(|n| n.starts_with(bare) || bare.starts_with(n))
        {
            continue;
        }
        names.push(bare);
        *totals.entry(zone).or_insert(0) += pop;
    }
    let mut out = String::new();
    for code in codes {
        let total = totals.get(code).copied().unwrap_or(0);
        out.push_str(&format!("{code}\t{total}\n"));
    }
    fs::write(dst, out)?;
    Ok(())
}

/// Lignes utiles d'un fichier de données (ni vides ni commentaires).
fn data_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

fn build_population(zones: &Path, communes: &Path, dst: &Path) -> Result<(), Box<dyn Error>> {
    let zones_text = fs::read_to_string(zones)?;
    let mut zone_index: BTreeMap<&str, u64> = BTreeMap::new();
    for (i, line) in data_lines(&zones_text).enumerate() {
        let code = line.split('\t').next().ok_or("zone sans code")?;
        zone_index.insert(code, u64::try_from(i)?);
    }
    let text = fs::read_to_string(communes)?;
    let mut by_name: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for line in data_lines(&text) {
        let mut f = line.split('\t');
        let (Some(name), Some(pop), Some(zone)) = (f.next(), f.next(), f.next()) else {
            return Err(format!("ligne de commune invalide : {line}").into());
        };
        let pop: u64 = pop.parse()?;
        if pop > u64::from(u32::MAX) {
            return Err("population hors limites".into());
        }
        let zone = *zone_index.get(zone).ok_or("zone inconnue")?;
        by_name.entry(name).or_default().push(pop << 16 | zone);
    }
    let mut entries: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    for (name, values) in by_name {
        for (rank, value) in values.into_iter().enumerate() {
            let mut key = name.as_bytes().to_vec();
            key.push(0);
            key.push(u8::try_from(rank)?);
            entries.insert(key, value);
        }
    }
    let writer = BufWriter::new(fs::File::create(dst)?);
    let mut builder = MapBuilder::new(writer)?;
    for (key, value) in &entries {
        builder.insert(key, *value)?;
    }
    builder.finish()?;
    Ok(())
}

fn build_one(src: &Path, dst: &Path) -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(src)?;
    let mut entries: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (form, tier) = match line.split_once('\t') {
            Some((form, tier)) => (form, tier.trim().parse::<u64>()?),
            None => (line, 1),
        };
        let slot = entries.entry(form.as_bytes().to_vec()).or_insert(0);
        *slot = (*slot).max(tier);
    }
    let writer = BufWriter::new(fs::File::create(dst)?);
    let mut builder = MapBuilder::new(writer)?;
    for (key, tier) in &entries {
        builder.insert(key, *tier)?;
    }
    builder.finish()?;
    Ok(())
}
