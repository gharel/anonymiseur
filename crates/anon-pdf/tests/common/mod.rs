//! Outils partagés des tests d'intégration de `anon-pdf`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use anon_core::{EntityKind, Session, Source, Tag, TextBlock};
use anon_replace::{Replacer, Restorer, SessionExt};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn exemple(name: &str) -> Vec<u8> {
    std::fs::read(root().join("exemples").join(name)).expect("exemple")
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("fixture")
}

/// Entités attendues (type, valeur) d'un fichier de `exemples/attendu.json`.
pub fn expected(file: &str) -> Vec<(EntityKind, String)> {
    let raw = std::fs::read_to_string(root().join("exemples/attendu.json")).expect("attendu.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let files = v["fichiers"].as_array().expect("fichiers");
    let f = files
        .iter()
        .find(|f| f["fichier"] == file)
        .unwrap_or_else(|| panic!("{file} absent de attendu.json"));
    f["entites"]
        .as_array()
        .expect("entites")
        .iter()
        .map(|e| {
            let kind = EntityKind::from_prefix(e["type"].as_str().expect("type")).expect("kind");
            (kind, e["valeur"].as_str().expect("valeur").to_string())
        })
        .collect()
}

/// Espaces repliées, pour comparer des textes reconstitués.
pub fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn joined(blocks: &[TextBlock]) -> String {
    norm(
        &blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Valeurs retrouvées / manquées dans un texte.
pub fn found(
    values: &[(EntityKind, String)],
    text: &str,
) -> (Vec<(EntityKind, String)>, Vec<String>) {
    let mut ok = Vec::new();
    let mut missed = Vec::new();
    for (k, v) in values {
        if text.contains(&norm(v)) {
            ok.push((*k, v.clone()));
        } else {
            missed.push(v.clone());
        }
    }
    (ok, missed)
}

pub fn session(values: &[(EntityKind, String)]) -> (Session, Vec<(Tag, String)>) {
    let mut s = Session::new();
    let mut tags = Vec::new();
    for (k, v) in values {
        if let Some(t) = s.intern(*k, v, Source::Manual) {
            tags.push((t, v.clone()));
        }
    }
    (s, tags)
}

pub fn replacer(s: &Session) -> Replacer {
    Replacer::new(s)
}

pub fn restorer(s: &Session) -> Restorer {
    Restorer::new(s)
}

pub fn contains_bytes(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Recherche brute UTF-8, Latin-1 et UTF-16BE.
pub fn raw_leak(hay: &[u8], value: &str) -> bool {
    let latin1: Option<Vec<u8>> = value
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect();
    let utf16: Vec<u8> = value.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
    contains_bytes(hay, value.as_bytes())
        || latin1.is_some_and(|l| contains_bytes(hay, &l))
        || contains_bytes(hay, &utf16)
}
