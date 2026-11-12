//! Outils partagés des tests d'intégration du moteur.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{BlockRole, EntityKind, Handler, Profile, Session, TextBlock};
use anon_engine::{Analysis, AnalyzeOptions, Engine, InputFile, NoProgress};
use serde_json::Value;
use std::path::PathBuf;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn exemples() -> PathBuf {
    root().join("exemples")
}

pub fn input(name: &str) -> InputFile {
    InputFile::new(
        name,
        std::fs::read(exemples().join(name)).expect("exemple illisible"),
    )
}

#[derive(Debug, Clone)]
pub struct Expected {
    pub kind: EntityKind,
    pub value: String,
    pub renforce: bool,
    pub ocr: bool,
}

#[derive(Debug, Clone)]
pub struct ExampleFile {
    pub name: String,
    pub entities: Vec<Expected>,
    pub traps: Vec<String>,
}

impl ExampleFile {
    /// Valeurs attendues non OCR pour un profil.
    pub fn expected(&self, profile: Profile) -> Vec<&Expected> {
        self.entities
            .iter()
            .filter(|e| !e.ocr && (profile == Profile::Renforce || !e.renforce))
            .collect()
    }
}

pub fn attendu() -> Vec<ExampleFile> {
    let raw = std::fs::read_to_string(exemples().join("attendu.json")).unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    v["fichiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| ExampleFile {
            name: f["fichier"].as_str().unwrap().to_string(),
            entities: f["entites"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|e| Expected {
                            kind: EntityKind::from_prefix(e["type"].as_str().unwrap()).unwrap(),
                            value: e["valeur"].as_str().unwrap().to_string(),
                            renforce: e["profil"] == "renforce",
                            ocr: e["ocr"] == true,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            traps: f["ne_pas_detecter"]
                .as_array()
                .map(|a| a.iter().map(|t| t.as_str().unwrap().to_string()).collect())
                .unwrap_or_default(),
        })
        .collect()
}

/// Fichiers d'exemple gérés par le moteur (hors réponse d'IA simulée).
pub fn handled(engine: &Engine) -> Vec<ExampleFile> {
    let exts = engine.supported_extensions();
    attendu()
        .into_iter()
        .filter(|f| f.name != "reponse-ia-exemple.md")
        .filter(|f| {
            f.name
                .rsplit_once('.')
                .is_some_and(|(_, e)| exts.contains(&e))
        })
        .collect()
}

pub fn opts(profile: Profile) -> AnalyzeOptions {
    let mut o = AnalyzeOptions::default();
    o.detect.profile = profile;
    o
}

pub fn analyze(engine: &Engine, files: &[InputFile], profile: Profile) -> Analysis {
    engine.analyze(files, &opts(profile), &Session::new(), &NoProgress)
}

/// Clé « écrasée » : pliée, lettres et chiffres seulement.
pub fn squash(v: &str) -> String {
    anon_replace::key(v)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Texte de tous les blocs (hors métadonnées) d'un fichier.
pub fn body_blocks(h: &dyn Handler, bytes: &[u8]) -> Vec<TextBlock> {
    h.extract(bytes)
        .unwrap()
        .blocks
        .into_iter()
        .filter(|b| b.ctx.role != BlockRole::Metadata)
        .collect()
}
