//! Contrôle de fuite et identifiants structurels du format (numéro de ligne,
//! référence de cellule, priorité de style) : ils ne comptent pas comme une
//! fuite, mais une vraie valeur restée dans le contenu bloque toujours le
//! fichier.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{
    EntityKind, Extraction, FormatError, Handler, Output, Profile, Session, TextBlock,
    TextTransform, TransformOptions,
};
use anon_engine::{AnonymizeOptions, Engine, EngineError, InputFile};
use std::io::{Cursor, Read, Write};

/// Ajoute une valeur absente du contenu, la valide, et anonymise.
fn anonymize_with(name: &str, kind: EntityKind, value: &str) -> Result<Vec<u8>, EngineError> {
    let engine = Engine::standard();
    let file = common::input(name);
    let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    analysis.add_manual(kind, value).unwrap();
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    engine
        .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
        .map(|a| a.bytes)
}

/// Anomalie relevée sur `patients.xlsx` : un âge « 19 » était « trouvé » dans
/// le numéro de la ligne 19 (`<row r="19">`) et le fichier était refusé.
#[test]
fn numero_de_ligne_n_est_pas_une_fuite() {
    let out = anonymize_with("patients.xlsx", EntityKind::Age, "19");
    assert!(out.is_ok(), "{:?}", out.err());
}

/// Même anomalie dans un document Word : `w:uiPriority="35"` des styles.
#[test]
fn priorite_de_style_n_est_pas_une_fuite() {
    for value in ["35", "62", "73"] {
        let out = anonymize_with("contrat-prestation.docx", EntityKind::Age, value);
        assert!(out.is_ok(), "{value} : {:?}", out.err());
    }
}

/// Classeur `patients.xlsx` dont le premier âge vaut 19 : la valeur figure à la
/// fois dans le contenu (`<v>19</v>`) et dans la structure (`<row r="19">`).
fn patients_with_age_19() -> Vec<u8> {
    let bytes = std::fs::read(common::exemples().join("patients.xlsx")).unwrap();
    let mut zin = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut e = zin.by_index(i).unwrap();
        let name = e.name().unwrap().into_owned();
        let mut data = Vec::new();
        e.read_to_end(&mut data).unwrap();
        if name == "xl/worksheets/sheet1.xml" {
            let xml = String::from_utf8(data).unwrap();
            assert!(xml.contains("<v>54</v>") && xml.contains(r#"<row r="19""#));
            data = xml.replacen("<v>54</v>", "<v>19</v>", 1).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// Gestionnaire Excel dont la réécriture oublie tout (fuite simulée).
struct Forgetful;

impl Handler for Forgetful {
    fn extensions(&self) -> &'static [&'static str] {
        &["xlsx"]
    }
    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        anon_office::XlsxHandler.extract(input)
    }
    fn deep_scan(&self, input: &[u8]) -> Result<Vec<TextBlock>, FormatError> {
        anon_office::XlsxHandler.deep_scan(input)
    }
    fn transform(
        &self,
        input: &[u8],
        _: &dyn TextTransform,
        _: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        Ok(Output {
            bytes: input.to_vec(),
            replacements: 0,
            warnings: Vec::new(),
        })
    }
}

static FORGETFUL: Forgetful = Forgetful;

/// La détection d'une vraie valeur n'est jamais affaiblie : un âge 19 resté
/// dans une cellule est signalé, même si la ligne 19 existe aussi.
#[test]
fn vraie_valeur_toujours_signalee() {
    let file = InputFile::new("patients.xlsx", patients_with_age_19());
    let mut session = Session::new();

    // Réécriture normale : la cellule est remplacée, aucune fuite.
    let engine = Engine::standard();
    let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    analysis.add_manual(EntityKind::Age, "19").unwrap();
    engine.apply_selection(&analysis, &mut session);
    let ok = engine.anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default());
    assert!(ok.is_ok(), "{:?}", ok.err());

    // Réécriture qui oublie la cellule : la fuite est signalée.
    let lazy = Engine::builder().handler("xlsx", &FORGETFUL).build();
    let err = lazy
        .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
        .expect_err("fuite attendue");
    let EngineError::Leak { leaks } = err else {
        panic!("fuite attendue : {err}");
    };
    let age = session
        .entities
        .iter()
        .find(|e| e.value == "19")
        .map(|e| e.tag)
        .unwrap();
    assert!(
        leaks
            .iter()
            .any(|l| l.tag == age && l.part.starts_with("xl/worksheets/sheet1.xml")),
        "{leaks:?}"
    );
}
