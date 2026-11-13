//! Détection de noms (personnes, sociétés, lieux, adresses) par modèles ONNX embarqués.
//!
//! Deux modèles (choix mesuré, voir `assets/ner/LISEZMOI.md`) :
//! - [`Bundle::Fr`] : DistilCamemBERT-NER, pour le français ;
//! - [`Bundle::Multi`] : newmean pii-medium (54 langues), pour les autres langues.
//!
//! Aiguillage par bloc ([`whatlang`]) : français fiable → `Fr`, autre langue fiable →
//! `Multi`, détection non fiable (cellule, texte court) → les deux, union des résultats.
//!
//! Chaîne par modèle : tokenisation (`tokenizers`, pur Rust) → fenêtres chevauchantes de
//! [`WINDOW_TOKENS`] jetons → inférence ONNX Runtime (`ort`, lié statiquement) →
//! moyenne des probabilités sur les chevauchements → agrégation des sous-mots →
//! offsets en octets ajustés aux frontières de mot. Référence Python :
//! `tools/model/pipeline.py` (mêmes constantes, mêmes règles).
//!
//! Aucun accès réseau : les modèles sont lus dans un dossier ([`NerDetector::from_dir`])
//! ou inclus dans le binaire ([`NerDetector::embedded`], fonctionnalité `embed`).
//! Sans la fonctionnalité `ia`, le crate compile sans ONNX Runtime et les
//! constructeurs renvoient [`NerError::Unavailable`].

// Sans `ia`, la partie pure reste compilée et testée mais n'est pas appelée.
#[cfg_attr(not(feature = "ia"), allow(dead_code))]
mod aggregate;
#[cfg(feature = "ia")]
mod filter;
#[cfg_attr(not(feature = "ia"), allow(dead_code))]
mod labels;
#[cfg(feature = "ia")]
mod model;

pub use aggregate::{MIN_CONFIDENCE, Span, WINDOW_OVERLAP, WINDOW_TOKENS};
#[cfg(feature = "ia")]
pub use filter::{SURE_CONFIDENCE, UNSURE_CONFIDENCE};

/// Nombre minimal de lettres d'un bloc pour qu'il soit lu par le modèle.
const MIN_BLOCK_LETTERS: usize = 2;

/// Vrai si le bloc peut contenir un nom propre : au moins deux lettres, et une
/// majuscule ou une lettre d'écriture sans casse (arabe, chinois…). Les autres
/// blocs (montants, dates, codes, texte en minuscules) ne sont pas envoyés au modèle.
pub fn worth_reading(text: &str) -> bool {
    let mut letters = 0usize;
    let mut upper = false;
    for c in text.chars() {
        if c.is_alphabetic() {
            letters += 1;
            upper |= c.is_uppercase() || !c.is_lowercase();
        }
    }
    letters >= MIN_BLOCK_LETTERS && upper
}

use anon_core::detect::{DetectOptions, Detector};
use anon_core::entities::{Detection, Source};
use anon_core::text::TextBlock;
use std::path::{Path, PathBuf};

/// Commande qui prépare `assets/ner/` (téléchargement, conversion, vérification).
pub const PREPARE_COMMAND: &str = "uv run --project tools/model tools/model/prepare_assets.py";

/// Un des deux modèles. Fichiers : `<nom>.onnx`, `<nom>.tokenizer.json`, `<nom>.config.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bundle {
    Fr,
    Multi,
}

impl Bundle {
    pub const ALL: [Bundle; 2] = [Bundle::Fr, Bundle::Multi];

    pub fn file_stem(self) -> &'static str {
        match self {
            Bundle::Fr => "fr",
            Bundle::Multi => "multi",
        }
    }

    fn files(self) -> [String; 3] {
        let s = self.file_stem();
        [
            format!("{s}.onnx"),
            format!("{s}.tokenizer.json"),
            format!("{s}.config.json"),
        ]
    }

    fn index(self) -> usize {
        match self {
            Bundle::Fr => 0,
            Bundle::Multi => 1,
        }
    }
}

/// Erreurs du détecteur. Ne contiennent jamais de contenu utilisateur.
#[derive(Debug, Clone, thiserror::Error)]
pub enum NerError {
    #[error("détection par IA indisponible : crate compilé sans la fonctionnalité « ia »")]
    Unavailable,
    #[error("fichier de modèle manquant : {0} (préparer avec : {PREPARE_COMMAND})")]
    MissingFile(PathBuf),
    #[error("modèle {0:?} absent de ce détecteur")]
    MissingBundle(Bundle),
    #[error("lecture impossible : {path} ({kind})")]
    Io {
        path: PathBuf,
        kind: std::io::ErrorKind,
    },
    #[error("configuration du modèle invalide : {0}")]
    Config(&'static str),
    #[error("échec de la tokenisation")]
    Tokenize,
    #[error("ONNX Runtime : {0}")]
    Runtime(String),
    #[error("verrou de session empoisonné")]
    Poisoned,
}

/// Origine des octets des modèles.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "ia"), allow(dead_code))]
enum Files {
    Dir(PathBuf),
    #[cfg(feature = "embed")]
    Embedded,
}

#[cfg(feature = "ia")]
type Slot = std::sync::OnceLock<Result<model::Model, NerError>>;

/// Détecteur NER. Chaque modèle est chargé paresseusement à son premier usage (une seule
/// fois, même avec plusieurs fils) ; l'inférence d'un modèle est sérialisée par un verrou.
#[derive(Debug)]
pub struct NerDetector {
    #[cfg_attr(not(feature = "ia"), allow(dead_code))]
    files: Files,
    #[cfg_attr(not(feature = "ia"), allow(dead_code))]
    present: [bool; 2],
    #[cfg(feature = "ia")]
    slots: [Slot; 2],
}

#[cfg(feature = "embed")]
mod embedded {
    macro_rules! asset {
        ($f:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/ner/",
                $f
            ))
        };
    }
    pub static FR: [&[u8]; 3] = [
        asset!("fr.onnx"),
        asset!("fr.tokenizer.json"),
        asset!("fr.config.json"),
    ];
    pub static MULTI: [&[u8]; 3] = [
        asset!("multi.onnx"),
        asset!("multi.tokenizer.json"),
        asset!("multi.config.json"),
    ];
}

/// Aiguillage d'un bloc vers les modèles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Only(Bundle),
    Both,
}

/// Choisit le(s) modèle(s) pour un texte : langue détectée de façon fiable → un modèle ;
/// sinon (texte court, cellule, mélange) → les deux.
#[cfg(feature = "ia")]
pub fn route(text: &str) -> Route {
    match whatlang::detect(text) {
        Some(info) if info.is_reliable() => {
            if info.lang() == whatlang::Lang::Fra {
                Route::Only(Bundle::Fr)
            } else {
                Route::Only(Bundle::Multi)
            }
        }
        _ => Route::Both,
    }
}

impl NerDetector {
    /// Vrai si le crate a été compilé avec ONNX Runtime.
    pub const fn is_available() -> bool {
        cfg!(feature = "ia")
    }

    /// Dossier contenant `fr.*` et/ou `multi.*` (développement). Au moins un modèle
    /// complet est exigé ; un modèle incomplet est une erreur. Chargement paresseux.
    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self, NerError> {
        if !Self::is_available() {
            return Err(NerError::Unavailable);
        }
        let dir = dir.as_ref().to_path_buf();
        let mut present = [false; 2];
        for b in Bundle::ALL {
            let paths = b.files().map(|f| dir.join(f));
            let n = paths.iter().filter(|p| p.is_file()).count();
            if n == 3 {
                present[b.index()] = true;
            } else if let Some(missing) = paths.into_iter().find(|p| !p.is_file())
                && n > 0
            {
                return Err(NerError::MissingFile(missing));
            }
        }
        if !present.iter().any(|&p| p) {
            return Err(NerError::MissingFile(dir.join("fr.onnx")));
        }
        Ok(Self::with_files(Files::Dir(dir), present))
    }

    /// Modèles inclus dans le binaire (fonctionnalité `embed`).
    #[cfg(feature = "embed")]
    pub fn embedded() -> Self {
        Self::with_files(Files::Embedded, [true, true])
    }

    fn with_files(files: Files, present: [bool; 2]) -> Self {
        Self {
            files,
            present,
            #[cfg(feature = "ia")]
            slots: [Slot::new(), Slot::new()],
        }
    }

    /// Vrai si ce modèle est disponible dans ce détecteur.
    pub fn has(&self, bundle: Bundle) -> bool {
        self.present.get(bundle.index()).copied().unwrap_or(false)
    }

    /// Force le chargement des modèles présents (écran de démarrage, test).
    pub fn warm_up(&self) -> Result<(), NerError> {
        for b in Bundle::ALL {
            if self.has(b) {
                self.find_with(b, "")?;
            }
        }
        Ok(())
    }

    /// Entités trouvées dans `text` (offsets en octets, sur frontières de caractères),
    /// selon l'aiguillage par langue, triées par début, après le tri des propositions
    /// (`filter` : fragments, emails, communes, titres, libellés ; confiance calibrée),
    /// sans filtrage par les réglages.
    pub fn find(&self, text: &str) -> Result<Vec<Span>, NerError> {
        #[cfg(feature = "ia")]
        {
            if !worth_reading(text) {
                return Ok(Vec::new());
            }
            let wanted: Vec<Bundle> = match route(text) {
                Route::Only(b) if self.has(b) => vec![b],
                _ => Bundle::ALL.into_iter().filter(|&b| self.has(b)).collect(),
            };
            let mut out: Vec<Span> = Vec::new();
            for b in wanted {
                for s in self.find_with(b, text)? {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
            let mut out = filter::refine(text, out);
            out.sort_by_key(|s| (s.start, s.end));
            out.dedup_by(|a, b| a.kind == b.kind && a.start == b.start && a.end == b.end);
            Ok(out)
        }
        #[cfg(not(feature = "ia"))]
        {
            let _ = text;
            Err(NerError::Unavailable)
        }
    }

    /// Entités brutes d'un seul modèle, sans tri des propositions (concordance avec
    /// la référence Python, mesures).
    pub fn find_with(&self, bundle: Bundle, text: &str) -> Result<Vec<Span>, NerError> {
        #[cfg(feature = "ia")]
        {
            if !self.has(bundle) {
                return Err(NerError::MissingBundle(bundle));
            }
            let slot = self
                .slots
                .get(bundle.index())
                .ok_or(NerError::MissingBundle(bundle))?;
            let model = slot
                .get_or_init(|| {
                    let [m, t, c] = self.read_files(bundle)?;
                    model::Model::load(&m, &t, &c)
                })
                .as_ref()
                .map_err(Clone::clone)?;
            model.predict(text)
        }
        #[cfg(not(feature = "ia"))]
        {
            let _ = (bundle, text);
            Err(NerError::Unavailable)
        }
    }

    #[cfg(feature = "ia")]
    fn read_files(&self, bundle: Bundle) -> Result<[std::borrow::Cow<'static, [u8]>; 3], NerError> {
        use std::borrow::Cow;
        match &self.files {
            Files::Dir(dir) => {
                let read = |name: &str| {
                    let path = dir.join(name);
                    std::fs::read(&path)
                        .map(Cow::Owned)
                        .map_err(|e| match e.kind() {
                            std::io::ErrorKind::NotFound => NerError::MissingFile(path),
                            kind => NerError::Io { path, kind },
                        })
                };
                let [m, t, c] = bundle.files();
                Ok([read(&m)?, read(&t)?, read(&c)?])
            }
            #[cfg(feature = "embed")]
            Files::Embedded => {
                let src = match bundle {
                    Bundle::Fr => &embedded::FR,
                    Bundle::Multi => &embedded::MULTI,
                };
                Ok(src.map(Cow::Borrowed))
            }
        }
    }
}

impl Detector for NerDetector {
    fn name(&self) -> &'static str {
        "ner"
    }

    /// En cas d'échec du modèle, ne renvoie rien : les autres détecteurs prennent le
    /// relais et l'interface signale l'indisponibilité via [`NerDetector::warm_up`].
    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        let Ok(spans) = self.find(&block.text) else {
            return Vec::new();
        };
        spans
            .into_iter()
            .filter(|s| opts.wants(s.kind))
            .map(|s| Detection {
                kind: s.kind,
                start: s.start,
                end: s.end,
                source: Source::Ner,
                confidence: s.confidence,
            })
            .collect()
    }
}

#[cfg(all(test, not(feature = "ia")))]
mod tests_without_ia {
    use super::*;

    #[test]
    fn constructors_report_unavailable() {
        assert!(!NerDetector::is_available());
        assert!(matches!(
            NerDetector::from_dir("."),
            Err(NerError::Unavailable)
        ));
    }
}

#[cfg(all(test, feature = "ia"))]
mod tests_routing {
    use super::*;

    #[test]
    fn routes_by_language() {
        assert_eq!(
            route(
                "Le contrat est signé entre la société et son gérant, qui en accepte les conditions générales."
            ),
            Route::Only(Bundle::Fr)
        );
        assert_eq!(
            route(
                "The invoice was approved by the head of procurement after the board meeting last week."
            ),
            Route::Only(Bundle::Multi)
        );
        assert_eq!(route("Garnier"), Route::Both);
    }

    #[test]
    fn skips_blocks_without_capital_or_letters() {
        assert!(worth_reading("Jean"));
        assert!(!worth_reading("12 450 000 € — 2026-11-12"));
        assert!(!worth_reading("le paiement est dû"));
        assert!(!worth_reading("A 1"));
        assert!(!worth_reading(""));
        assert!(worth_reading("وقّع السيد خالد المنصوري"));
    }

    #[test]
    fn missing_dir_reports_prepare_command() {
        let err = NerDetector::from_dir("dossier-inexistant").unwrap_err();
        assert!(err.to_string().contains(PREPARE_COMMAND));
    }
}
