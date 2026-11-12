//! Règles de détection à clé, fusion des détections.
//!
//! - [`checksums`] : contrôles de clé (Luhn, IBAN, NIR, SIREN/SIRET, DNI…).
//! - [`rules`] : [`rules::RuleDetector`], expressions régulières et contrôles de clé.
//! - [`pipeline`] : lancement des détecteurs et résolution des chevauchements.

pub mod checksums;
pub mod pipeline;
pub mod rules;

pub use pipeline::{Pipeline, merge};
pub use rules::RuleDetector;
