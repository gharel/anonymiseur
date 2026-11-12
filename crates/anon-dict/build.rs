//! Construit un FST par liste de `data/` dans `OUT_DIR`. Les listes sont déjà
//! pliées par `tools/prepare_dicts.py` ; le build ne fait que les trier,
//! dédoublonner (palier maximal) et les compiler.
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
