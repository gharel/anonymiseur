//! Détection des visages dans une image (édition complète, fonctionnalité `ia`).
//!
//! Modèle : YuNet d'OpenCV Zoo (`face_detection_yunet_2026may.onnx`, 230 Ko, licence
//! MIT, voir `THIRD-PARTY.md`), versionné dans `assets/faces/` et inclus dans le
//! binaire ; préparation : [`PREPARE_COMMAND`]. Il trouve les visages d'environ 10 à
//! 300 pixels de côté.
//!
//! Chaîne : image RGBA composée sur fond blanc → pyramide de niveaux ([`LEVEL_SIDES`],
//! réduction par moyenne de surface, jamais d'agrandissement) → inférence de chaque
//! niveau sur une entrée de taille libre complétée à un multiple de 32 → décodage des
//! ancres et suppression des non-maxima repris de la référence OpenCV (`post`) →
//! rectangles en pixels de l'image d'origine, agrandis des marges [`MARGIN_SIDE`],
//! [`MARGIN_TOP`], [`MARGIN_BOTTOM`] pour couvrir cheveux et menton.
//!
//! # Branchement dans le moteur
//!
//! ```
//! use anon_ner::faces::FaceDetector;
//! # let (rgba, width, height) = (vec![255u8; 4 * 64 * 48], 64, 48);
//! // Une fois (≈ 10 ms) ; détecteur à garder et à partager entre fils (Send + Sync).
//! let detector = FaceDetector::embedded().ok();
//! // Pour chaque image décodée en RGBA 8 bits : `None` si les visages n'ont pas pu
//! // être vérifiés (édition Lite, erreur), sinon les rectangles à masquer.
//! let faces = detector.as_ref().and_then(|d| d.detect(&rgba, width, height).ok());
//! // `ImageInput { faces, .. }` ; chaque rectangle devient un `Fix::MaskRegion`.
//! assert!(faces.is_none_or(|f| f.is_empty()));
//! ```
//!
//! # Édition Lite
//!
//! Sans la fonctionnalité `ia`, le module compile sans ONNX Runtime :
//! [`FaceDetector::embedded`] et [`FaceDetector::from_bytes`] renvoient
//! [`FaceError::Unavailable`]. Le moteur met alors `ImageInput::faces` à `None`
//! (« Visages non vérifiés ») et l'évaluation ne peut pas conclure à l'anonymat d'une
//! image qui contient une photo (`FailureReason::FacesNotChecked`).
//!
//! # Confidentialité
//!
//! Aucun pixel n'est conservé, journalisé ni affiché par `Debug` ; les erreurs ne
//! contiennent que des dimensions.

// Sans `ia`, la partie pure reste compilée et testée mais n'est pas appelée.
#[cfg_attr(not(feature = "ia"), allow(dead_code))]
mod post;
#[cfg_attr(not(feature = "ia"), allow(dead_code))]
mod prep;
#[cfg(feature = "ia")]
mod session;

use anon_core::format::PixelRect;

/// Commande qui télécharge le modèle et vérifie son empreinte.
pub const PREPARE_COMMAND: &str = "uv run --project tools tools/prepare_faces.py";

/// Fichier du modèle dans `assets/faces/`.
pub const MODEL_FILE: &str = "face_detection_yunet_2026may.onnx";

/// Score minimal d'un visage (racine de `cls × obj`). Valeur par défaut de la classe
/// `YuNet` d'OpenCV Zoo (`yunet.py`, `confThreshold=0.6`). La démonstration d'OpenCV
/// Zoo et `FaceDetectorYN::create` prennent 0,9 : trop haut ici, où un visage manqué
/// coûte plus cher qu'une zone de trop, que l'utilisateur peut garder visible (les
/// portraits peints des tests sortent entre 0,90 et 0,93).
pub const SCORE_THRESHOLD: f32 = 0.6;

/// Seuil de recouvrement (IoU) de la suppression des non-maxima : 0,3, comme OpenCV
/// Zoo et `FaceDetectorYN::create`.
pub const NMS_THRESHOLD: f32 = 0.3;

/// Nombre maximal de boîtes examinées par la suppression des non-maxima : 5 000, comme
/// OpenCV Zoo et `FaceDetectorYN::create`.
pub const TOP_K: usize = 5000;

/// Plus grand côté de chaque niveau de la pyramide, en pixels (jamais plus que
/// l'image). YuNet trouve les visages de 10 à 300 pixels environ : à 1 280, ceux de
/// plus de 1 % du plus grand côté de l'image ; à 320, ceux qui l'occupent presque en
/// entier (photo d'identité, portrait). Deux niveaux quatre fois plus petits l'un que
/// l'autre couvrent toutes les tailles intermédiaires.
pub const LEVEL_SIDES: [u32; 2] = [1280, 320];

/// Marge ajoutée à gauche et à droite du visage, en part de sa largeur (oreilles,
/// cheveux).
pub const MARGIN_SIDE: f64 = 0.2;
/// Marge ajoutée au-dessus du visage, en part de sa hauteur (cheveux).
pub const MARGIN_TOP: f64 = 0.3;
/// Marge ajoutée sous le visage, en part de sa hauteur (menton, début du cou).
pub const MARGIN_BOTTOM: f64 = 0.15;

/// Octets d'un pixel RGBA.
const RGBA_BYTES: u64 = 4;

/// Erreurs de la détection des visages. Ne contiennent jamais de pixel.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FaceError {
    #[error("détection des visages indisponible : crate compilé sans la fonctionnalité « ia »")]
    Unavailable,
    #[error("image vide ({width} × {height} pixels)")]
    EmptyImage { width: u32, height: u32 },
    #[error("tampon d'image incohérent : {actual} octets pour {width} × {height} pixels RGBA")]
    BufferSize {
        width: u32,
        height: u32,
        actual: usize,
    },
    #[error("modèle de visages invalide : {0} (préparer avec : {PREPARE_COMMAND})")]
    Model(&'static str),
    #[error("ONNX Runtime : {0}")]
    Runtime(String),
    #[error("verrou de session empoisonné")]
    Poisoned,
}

/// Détecteur de visages. Le modèle est chargé à la construction ; l'inférence est
/// sérialisée par un verrou (ONNX Runtime parallélise lui-même chaque passe).
#[derive(Debug)]
pub struct FaceDetector {
    #[cfg(feature = "ia")]
    net: session::YuNet,
    #[cfg(not(feature = "ia"))]
    _unavailable: (),
}

#[cfg(feature = "ia")]
static EMBEDDED_MODEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/faces/face_detection_yunet_2026may.onnx"
));

impl FaceDetector {
    /// Vrai si le crate a été compilé avec ONNX Runtime.
    pub const fn is_available() -> bool {
        cfg!(feature = "ia")
    }

    /// Modèle inclus dans le binaire. Le fichier est versionné dans `assets/faces/`
    /// (230 Ko) : contrairement aux modèles de noms, il ne demande pas la
    /// fonctionnalité `embed`.
    pub fn embedded() -> Result<Self, FaceError> {
        #[cfg(feature = "ia")]
        {
            Self::from_bytes(EMBEDDED_MODEL)
        }
        #[cfg(not(feature = "ia"))]
        {
            Err(FaceError::Unavailable)
        }
    }

    /// Modèle YuNet lu depuis des octets (tests, autre version du modèle). Vérifie
    /// l'entrée (image `f32` de rang 4) et les sorties `cls`, `obj`, `bbox`.
    pub fn from_bytes(onnx: &[u8]) -> Result<Self, FaceError> {
        #[cfg(feature = "ia")]
        {
            Ok(Self {
                net: session::YuNet::load(onnx)?,
            })
        }
        #[cfg(not(feature = "ia"))]
        {
            let _ = onnx;
            Err(FaceError::Unavailable)
        }
    }

    /// Visages d'une image RGBA 8 bits (`width × height × 4` octets, lignes sans
    /// remplissage), en pixels de cette image, marges comprises, par score décroissant.
    /// Les pixels transparents sont lus comme du blanc. Image très grande : réduite à
    /// [`LEVEL_SIDES`], ce qui borne la mémoire de travail ; image minuscule : lue
    /// telle quelle, complétée à 32 pixels.
    pub fn detect(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<PixelRect>, FaceError> {
        if width == 0 || height == 0 {
            return Err(FaceError::EmptyImage { width, height });
        }
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(RGBA_BYTES));
        if expected != Some(rgba.len() as u64) {
            return Err(FaceError::BufferSize {
                width,
                height,
                actual: rgba.len(),
            });
        }
        #[cfg(feature = "ia")]
        {
            self.detect_checked(rgba, width, height)
        }
        #[cfg(not(feature = "ia"))]
        {
            Err(FaceError::Unavailable)
        }
    }

    #[cfg(feature = "ia")]
    fn detect_checked(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<PixelRect>, FaceError> {
        let incoherent = || FaceError::BufferSize {
            width,
            height,
            actual: rgba.len(),
        };
        let (w, h) = (
            usize::try_from(width).map_err(|_| incoherent())?,
            usize::try_from(height).map_err(|_| incoherent())?,
        );
        let source = prep::Rgba {
            data: rgba,
            width: w,
            height: h,
        };
        // Du plus grand niveau au plus petit : chacun est tiré du précédent, ce qui ne
        // relit l'image d'origine qu'une fois.
        let mut levels: Vec<prep::Planes> = Vec::new();
        for (lw, lh) in prep::level_sizes(w, h) {
            let level = match levels.last() {
                None => prep::resample(&source, lw, lh),
                Some(previous) => prep::resample(previous, lw, lh),
            }
            .ok_or_else(incoherent)?;
            levels.push(level);
        }
        let mut candidates = Vec::new();
        for level in levels {
            let scale = (
                w as f32 / level.width as f32,
                h as f32 / level.height as f32,
            );
            self.net
                .run(level, SCORE_THRESHOLD, scale, &mut candidates)?;
        }
        let kept = post::nms(&candidates, SCORE_THRESHOLD, NMS_THRESHOLD, TOP_K);
        Ok(kept
            .into_iter()
            .filter_map(|i| candidates.get(i))
            .filter_map(|c| post::mask_rect(c, width, height))
            .collect())
    }
}

#[cfg(all(test, not(feature = "ia")))]
mod tests_without_ia {
    use super::*;

    #[test]
    fn lite_edition_reports_unavailable() {
        assert!(!FaceDetector::is_available());
        assert_eq!(
            FaceDetector::embedded().unwrap_err(),
            FaceError::Unavailable
        );
        assert_eq!(
            FaceDetector::from_bytes(&[]).unwrap_err(),
            FaceError::Unavailable
        );
    }
}
