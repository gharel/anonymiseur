//! Cœur de l'anonymiseur : types partagés (contrats) et logique indépendante des formats.
//!
//! Les modules `entities`, `text`, `format`, `detect` et les structures de `session`
//! sont des **contrats** : tous les autres crates en dépendent. Ne pas en changer la
//! forme sans mettre à jour `AGENTS.md` et les crates consommateurs.

pub mod detect;
pub mod entities;
pub mod format;
pub mod leakcheck;
pub mod mapping;
pub mod normalize;
pub mod replace;
pub mod restore;
pub mod risk;
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
