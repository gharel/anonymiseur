//! Contrat des gestionnaires de formats (`Handler`) et du traitement d'images. CONTRAT partagé.

use crate::text::{TextBlock, TextTransform};
use std::fmt;

/// Erreur de lecture ou d'écriture d'un fichier. Les messages ne contiennent
/// jamais de contenu utilisateur, seulement des emplacements.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("format non pris en charge : {0}")]
    Unsupported(String),
    #[error("fichier illisible ou corrompu ({part}) : {reason}")]
    Corrupt { part: String, reason: String },
    #[error("fichier protégé par mot de passe")]
    Encrypted,
    #[error("erreur d'entrée/sortie : {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl FormatError {
    pub fn corrupt(part: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Corrupt {
            part: part.into(),
            reason: reason.into(),
        }
    }
}

/// Nature d'un avertissement remonté à l'utilisateur dans le rapport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WarningCode {
    /// Image non analysée (format vectoriel EMF/WMF, image illisible…).
    ImageNotProcessed,
    /// Page ou image lue par OCR avec une confiance faible : à vérifier à l'œil.
    LowOcrConfidence,
    /// Page scannée non traitée (OCR indisponible).
    ScannedPage,
    /// Objet binaire embarqué (OLE, ActiveX) non analysé.
    EmbeddedBinary,
    /// Macros VBA présentes, non analysées.
    MacroPresent,
    /// Nom de feuille contenant une valeur sensible, laissé tel quel.
    SheetNameSensitive,
    /// Valeur sensible trouvée dans une zone non modifiable (signalée, pas remplacée).
    UntouchedLocation,
    /// Partie du fichier ignorée car inconnue ou illisible.
    UnsupportedPart,
    /// Balise trop longue pour la zone : écrite en plus petit ou tronquée.
    TagDidNotFit,
    /// Image masquée dont l'original ne pourra pas être remis à la restauration
    /// (le format ne conserve pas l'image octet pour octet, comme un PDF scanné).
    ImageNotRestorable,
}

/// Avertissement. `part` est un emplacement, `detail` une phrase en français,
/// tous deux **sans contenu utilisateur**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub code: WarningCode,
    pub part: String,
    pub detail: String,
}

impl Warning {
    pub fn new(code: WarningCode, part: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code,
            part: part.into(),
            detail: detail.into(),
        }
    }
}

/// Image embarquée dans un document (`word/media/image1.png`…), ou la page
/// rendue d'un PDF scanné.
#[derive(Clone, PartialEq, Eq)]
pub struct MediaItem {
    /// Chemin de la partie dans le fichier.
    pub name: String,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for MediaItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaItem")
            .field("name", &self.name)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// Remplace le contenu d'une image embarquée. `None` = laisser tel quel.
pub trait MediaTransform: Sync {
    fn apply(&self, name: &str, bytes: &[u8]) -> Option<Vec<u8>>;
}

/// Résultat de la lecture d'un fichier, avant détection.
#[derive(Debug, Clone, Default)]
pub struct Extraction {
    pub blocks: Vec<TextBlock>,
    pub media: Vec<MediaItem>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Anonymize,
    Restore,
}

/// Options d'une transformation.
#[derive(Clone, Copy)]
pub struct TransformOptions<'a> {
    pub direction: Direction,
    /// Vide auteur, société, propriétés, miniature, auteurs de commentaires…
    /// Toujours `true` en anonymisation ; `false` en restauration.
    pub scrub_metadata: bool,
    /// Remplacement des images embarquées (masquage, ou remise des originaux).
    pub media: Option<&'a dyn MediaTransform>,
}

impl fmt::Debug for TransformOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransformOptions")
            .field("direction", &self.direction)
            .field("scrub_metadata", &self.scrub_metadata)
            .field("media", &self.media.is_some())
            .finish()
    }
}

impl TransformOptions<'_> {
    pub fn anonymize() -> Self {
        Self {
            direction: Direction::Anonymize,
            scrub_metadata: true,
            media: None,
        }
    }
    pub fn restore() -> Self {
        Self {
            direction: Direction::Restore,
            scrub_metadata: false,
            media: None,
        }
    }
}

/// Fichier transformé.
#[derive(Clone)]
pub struct Output {
    pub bytes: Vec<u8>,
    /// Nombre de remplacements effectués.
    pub replacements: usize,
    pub warnings: Vec<Warning>,
}

impl fmt::Debug for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("len", &self.bytes.len())
            .field("replacements", &self.replacements)
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// Gestionnaire d'un format de fichier. Sans état : toute l'information vient
/// des octets d'entrée. Ne doit jamais paniquer sur une entrée corrompue.
pub trait Handler: Sync {
    /// Extensions gérées, en minuscules, sans point (`["docx"]`).
    fn extensions(&self) -> &'static [&'static str];

    /// Lit le fichier : blocs de texte logiques, images embarquées, avertissements.
    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError>;

    /// Lecture exhaustive pour le contrôle de fuite : **tout** texte présent dans
    /// le fichier, y compris les zones que `extract` ignore (caches, attributs,
    /// propriétés). Par défaut : les blocs de `extract`.
    fn deep_scan(&self, input: &[u8]) -> Result<Vec<TextBlock>, FormatError> {
        self.extract(input).map(|e| e.blocks)
    }

    /// Réécrit le fichier en appliquant `tr` à chaque bloc. Tout ce qui n'est pas
    /// touché est recopié à l'identique.
    fn transform(
        &self,
        input: &[u8],
        tr: &dyn TextTransform,
        opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError>;
}

/// Rectangle en pixels, origine en haut à gauche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Mot lu par OCR.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub rect: PixelRect,
    /// 0.0 à 1.0.
    pub confidence: f32,
}

/// Résultat d'OCR d'une image, mots dans l'ordre de lecture.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcrPage {
    pub width: u32,
    pub height: u32,
    pub words: Vec<OcrWord>,
}

impl OcrPage {
    /// Texte reconstitué (mots séparés par une espace) et, pour chaque mot,
    /// son intervalle d'octets dans ce texte. Sert à relier un `Replacement`
    /// aux rectangles à masquer.
    pub fn text_with_spans(&self) -> (String, Vec<std::ops::Range<usize>>) {
        let mut text = String::new();
        let mut spans = Vec::with_capacity(self.words.len());
        for (i, w) in self.words.iter().enumerate() {
            if i > 0 {
                text.push(' ');
            }
            let start = text.len();
            text.push_str(&w.text);
            spans.push(start..text.len());
        }
        (text, spans)
    }

    /// Confiance moyenne, 1.0 si aucun mot.
    pub fn mean_confidence(&self) -> f32 {
        if self.words.is_empty() {
            return 1.0;
        }
        self.words.iter().map(|w| w.confidence).sum::<f32>() / self.words.len() as f32
    }
}

/// Zone à masquer dans une image, avec la balise à écrire par-dessus si elle tient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskRegion {
    pub rect: PixelRect,
    pub label: Option<String>,
}

/// OCR et masquage d'images (implémenté dans `anon-pdf`).
pub trait ImageProcessor: Sync {
    /// Lit le texte d'une image (PNG, JPEG, GIF, BMP, TIFF, WebP).
    fn ocr(&self, image: &[u8]) -> Result<OcrPage, FormatError>;

    /// Recouvre les zones d'un aplat opaque, écrit la balise si elle tient,
    /// ré-encode dans le même format, sans métadonnées EXIF.
    fn mask(&self, image: &[u8], regions: &[MaskRegion]) -> Result<Vec<u8>, FormatError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_text_spans() {
        let page = OcrPage {
            width: 100,
            height: 20,
            words: vec![
                OcrWord {
                    text: "Jean".into(),
                    rect: PixelRect {
                        x: 0,
                        y: 0,
                        w: 30,
                        h: 10,
                    },
                    confidence: 0.9,
                },
                OcrWord {
                    text: "Dupont".into(),
                    rect: PixelRect {
                        x: 35,
                        y: 0,
                        w: 40,
                        h: 10,
                    },
                    confidence: 0.7,
                },
            ],
        };
        let (text, spans) = page.text_with_spans();
        assert_eq!(text, "Jean Dupont");
        assert_eq!(&text[spans[1].clone()], "Dupont");
        assert!((page.mean_confidence() - 0.8).abs() < 1e-6);
    }
}
