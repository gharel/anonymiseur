//! Contrats partagés de l'anonymiseur. Aucun autre code ici : tous les crates en
//! dépendent, donc ce crate doit rester petit et stable.
//!
//! Ne pas en changer la forme sans mettre à jour `AGENTS.md` et les crates
//! consommateurs.

pub mod detect;
pub mod entities;
pub mod format;
pub mod session;
pub mod text;

pub use detect::{DetectOptions, Detector, Profile};
pub use entities::{Detection, EntityKind, Family, Source, Tag};
pub use format::{
    Direction, Extraction, FormatError, Handler, ImageProcessor, MaskRegion, MediaItem,
    MediaTransform, OcrPage, OcrWord, Output, PixelRect, TransformOptions, Warning, WarningCode,
};
pub use session::{Entity, MediaOriginal, Session, Treatment};
pub use text::{BlockContext, BlockRole, Replacement, TextBlock, TextTransform};
