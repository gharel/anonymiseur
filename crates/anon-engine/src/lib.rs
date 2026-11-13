//! Moteur d'orchestration : relie les gestionnaires de formats, les détecteurs
//! et la table de correspondance. C'est l'unique point d'entrée de l'interface.
//!
//! Parcours : [`Engine::analyze`] → l'utilisateur valide les [`Candidate`] →
//! [`Engine::apply_selection`] → [`Engine::anonymize`] (contrôle de fuite
//! bloquant) → plus tard [`Engine::restore`] ou [`Engine::restore_text`].
//!
//! Tout est injecté par [`EngineBuilder`] : gestionnaires, détecteurs et
//! traitement d'images. [`Engine::standard`] branche `anon-office` et les
//! détecteurs sans IA.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

mod analysis;
mod csv;
mod engine;
mod error;
mod media;
mod risk;

pub use analysis::{
    Analysis, AnalyzeOptions, AnalyzedImage, CONTEXT_CHARS, Candidate, FileAnalysis,
    SELECT_MIN_CONFIDENCE, is_ambiguous,
};
pub use engine::{
    AnonymizeOptions, Anonymized, Engine, EngineBuilder, FileReport, HandlerLookup, InputFile,
    NoProgress, Progress, Restored,
};
pub use error::{EngineError, Leak};
pub use media::{LOW_OCR_CONFIDENCE, MASK_MARGIN_PX, ManualMask, sha256_hex};
pub use risk::{
    RISK_HIGH_KINDS, RISK_HIGH_TOTAL, RISK_MODERATE_TOTAL, RISK_REMINDER, RiskLevel, RiskReport,
};

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
