//! Règles de détection à clé, fusion des détections.
//!
//! - [`checksums`] : contrôles de clé (Luhn, IBAN, NIR, SIREN/SIRET, DNI…).
//! - [`rules`] : [`rules::RuleDetector`], expressions régulières et contrôles de clé.
//! - [`quasi`] : [`quasi::QuasiDetector`], dates, âges, montants, fonctions, matricules.
//! - [`columns`] : [`columns::ColumnDetector`], type décidé par l'en-tête de colonne.
//! - [`lists`] : [`lists::ListDetector`], termes « toujours anonymiser ».
//! - [`pipeline`] : lancement des détecteurs et résolution des chevauchements.

pub mod checksums;
pub mod columns;
mod fold;
pub mod lists;
pub mod pipeline;
pub mod quasi;
pub mod rules;

pub use columns::ColumnDetector;
pub use lists::ListDetector;
pub use pipeline::{Pipeline, merge};
pub use quasi::QuasiDetector;
pub use rules::RuleDetector;
