//! Le moteur tel qu'il est livré : tous les formats (bureautique, PDF, images),
//! l'OCR, et tous les détecteurs — dont l'IA de noms dans l'édition complète.
//!
//! C'est le seul endroit où les crates sont assemblés. L'interface et les tests
//! de bout en bout passent par [`build`] pour mesurer exactement ce qui est livré.

use anon_engine::Engine;

/// État de l'IA de noms dans cette édition de l'application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IaStatus {
    /// Modèles chargés : la détection de noms par IA est active.
    Active,
    /// Édition Lite (ou macOS Intel) : l'application est compilée sans IA.
    Absente,
    /// Édition complète, mais les modèles n'ont pas pu être chargés ; la
    /// détection continue avec les règles et les dictionnaires.
    Indisponible,
}

impl IaStatus {
    /// Phrase affichée dans l'interface.
    pub fn label(self) -> &'static str {
        match self {
            IaStatus::Active => "Détection de noms par IA : active",
            IaStatus::Absente => "Édition Lite : détection par règles et dictionnaires",
            IaStatus::Indisponible => {
                "IA de noms indisponible : détection par règles et dictionnaires"
            }
        }
    }
}

/// Moteur assemblé et état de l'IA.
#[derive(Debug)]
pub struct Moteur {
    pub engine: Engine,
    pub ia: IaStatus,
}

/// Assemble le moteur. Le chargement des modèles d'IA prend environ une seconde :
/// à appeler hors du fil de l'interface.
pub fn build() -> Moteur {
    let builder = Engine::standard_builder()
        .handlers_from(anon_pdf::handler_for)
        .image_processor(Box::new(anon_pdf::MupdfImageProcessor::new()));

    #[cfg(feature = "ia")]
    let (builder, ia) = {
        let detector = anon_ner::NerDetector::embedded();
        match detector.warm_up() {
            Ok(()) => (builder.detector(Box::new(detector)), IaStatus::Active),
            Err(_) => (builder, IaStatus::Indisponible),
        }
    };
    #[cfg(not(feature = "ia"))]
    let ia = IaStatus::Absente;

    Moteur {
        engine: builder.build(),
        ia,
    }
}
