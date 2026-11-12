//! Normalisation du texte, remplacement des valeurs sensibles par des balises
//! (`Replacer`), restauration (`Restorer`) et enregistrement des valeurs dans
//! la session (`SessionExt`).
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

pub mod normalize;
pub mod replace;
pub mod restore;
pub mod session_ext;
mod tags;
mod uri;

pub use normalize::{Folded, fold, fold_keep_case, key};
pub use replace::{Match, Replacer};
pub use restore::{RestoreReport, Restorer};
pub use session_ext::{CASE_SENSITIVE_MAX_CHARS, SessionExt, has_variable_separators};
