//! Gestionnaires des formats bureautiques : texte brut, CSV, Word, Excel,
//! PowerPoint (Office Open XML) et OpenDocument.
//!
//! Organisation, pensée pour accueillir les formats OpenDocument :
//! - `package` : lecture bornée et réécriture d'un paquet zip (entrées non
//!   modifiées recopiées brutes, ordre et compression conservés) ;
//! - `xml` : lecture XML en flux avec positions, noms canoniques par espace de
//!   noms, réécriture par correctifs ponctuels ;
//! - `para` : recollage des runs d'un paragraphe et répartition des
//!   remplacements sur les runs ;
//! - `rules` : listes blanches de ce qui passe par `tr` ou est neutralisé ;
//! - `part`, `ooxml`, `xlsx` : spécificités OOXML ;
//! - `textfile` : texte brut.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

mod limits;
mod ooxml;
mod package;
mod para;
mod part;
pub mod rules;
mod textfile;
mod xlsx;
mod xml;

use anon_core::Handler;

pub use limits::{
    DEEP_SCAN_CHUNK, MAX_EMBED_DEPTH, MAX_ENTRIES, MAX_ENTRY_UNCOMPRESSED, MAX_TOTAL_UNCOMPRESSED,
    MAX_XML_DEPTH,
};
pub use ooxml::{DocxHandler, OdfHandler, PptxHandler, XlsxHandler};
pub use textfile::{CsvHandler, TextHandler};

static HANDLERS: &[&dyn Handler] = &[
    &TextHandler,
    &CsvHandler,
    &DocxHandler,
    &XlsxHandler,
    &PptxHandler,
    &OdfHandler,
];

/// Gestionnaire d'une extension (`"docx"`, `".XLSX"`…), `None` si non gérée.
pub fn handler_for(extension: &str) -> Option<&'static dyn Handler> {
    let ext = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    HANDLERS
        .iter()
        .copied()
        .find(|h| h.extensions().contains(&ext.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_lookup() {
        assert!(handler_for("docx").is_some());
        assert!(handler_for(".XLSM").is_some());
        assert!(handler_for("potx").is_some());
        assert!(handler_for("csv").is_some());
        assert!(handler_for("pdf").is_none());
        assert_eq!(handler_for("tsv").unwrap().extensions(), &["csv", "tsv"]);
        assert!(handler_for("ODS").is_some() && handler_for("otp").is_some());
        assert_eq!(handler_for("pptx").unwrap().extensions()[0], "pptx");
    }
}
