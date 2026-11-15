//! Moteur d'orchestration : relie les gestionnaires de formats, les détecteurs
//! et la table de correspondance. C'est l'unique point d'entrée de l'interface.
//!
//! Parcours : [`Engine::analyze`] → l'utilisateur valide les [`Candidate`] →
//! [`Engine::assess`] (trois critères du CEPD) et ses corrections
//! ([`Engine::apply_fix`], [`Engine::undo_fix`]) → [`Engine::apply_selection`]
//! → [`Engine::anonymize`] (contrôle de fuite bloquant) → fiche d'évaluation
//! ([`fiche::render_markdown`]) → plus tard [`Engine::restore`] ou
//! [`Engine::restore_text`].
//!
//! Tout est injecté par [`EngineBuilder`] : gestionnaires, détecteurs,
//! traitement d'images et détecteur de visages. [`Engine::standard`] branche
//! `anon-office` et les détecteurs sans IA.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

mod analysis;
mod csv;
mod document;
mod engine;
mod error;
mod evaluation;
pub mod fiche;
mod media;
mod mentions;
mod structure;

pub use analysis::{
    Analysis, AnalyzeOptions, AnalyzedImage, CONTEXT_CHARS, Candidate, FileAnalysis, ImageMask,
    SELECT_MIN_CONFIDENCE, is_ambiguous,
};
pub use document::{DocBlock, DocumentView, Span};
pub use engine::{
    AnonymizeOptions, Anonymized, Engine, EngineBuilder, FaceFinder, FileReport, HandlerLookup,
    InputFile, NoProgress, Progress, Restored, output_file_name,
};
pub use error::{EngineError, Leak};
pub use evaluation::{
    COLUMN_KIND_MIN_SHARE, EVALUATION_HEADERS, FACE_COVERED_MIN_SHARE, FIX_ALL_MAX_ROUNDS, FixUndo,
    Techniques, role_for_kind,
};
pub use fiche::{FICHE_MAX_SUBJECTS, FicheDocument, FicheMeta, render_markdown};
pub use media::{LOW_OCR_CONFIDENCE, MASK_MARGIN_PX, ManualMask, sha256_hex};

pub use anon_replace::RestoreReport;

/// Consigne à coller dans l'IA avec le texte anonymisé (`assets/consigne-ia.md`).
pub fn ai_instruction() -> &'static str {
    include_str!("../../../assets/consigne-ia.md")
}

#[cfg(test)]
mod tests {
    #[test]
    fn instruction_mentions_tags() {
        let s = super::ai_instruction();
        assert!(s.contains("[PERSONNE_1]"));
        assert!(s.contains("crochets"));
    }
}
