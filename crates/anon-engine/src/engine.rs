//! Orchestration : gestionnaires de formats, détecteurs, table de correspondance.

use crate::analysis::{
    Aggregator, Analysis, AnalyzeOptions, FileAnalysis, FileData, ImageData, UnreadMedia,
};
use crate::error::{EngineError, Leak, extension_of};
use crate::evaluation::{self, FIX_ALL_MAX_ROUNDS, FixUndo};
use crate::media::{LOW_OCR_CONFIDENCE, ManualMask, Masker, Unmasker, sha256_hex};
use crate::structure::structural_counts;
use anon_core::text::apply_replacements;
use anon_core::{
    AssessInput, AssessSettings, Assessment, BlockContext, BlockRole, DetectOptions, Detection,
    Detector, Fix, Goal, Handler, ImageProcessor, MediaTransform, PixelRect, Profile, Replacement,
    Session, Tag, TextBlock, TextTransform, TransformOptions, Verdict, Warning, WarningCode,
};
use anon_detect::{ColumnDetector, ListDetector, QuasiDetector, RuleDetector, merge};
use anon_dict::DictionaryDetector;
use anon_replace::{Replacer, RestoreReport, Restorer};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

/// Fichier à traiter : nom (avec extension, sans chemin) et contenu.
#[derive(Clone, PartialEq, Eq)]
pub struct InputFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl InputFile {
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }

    /// Extension en minuscules, sans point (vide si absente).
    pub fn extension(&self) -> String {
        extension_of(&self.name)
    }
}

impl fmt::Debug for InputFile {
    /// Ni le nom (il peut désigner une personne) ni le contenu.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputFile")
            .field("extension", &self.extension())
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// Avancement et annulation d'une analyse.
pub trait Progress: Sync {
    fn step(&self, done: usize, total: usize, label: &str);
    fn cancelled(&self) -> bool {
        false
    }
}

/// `Progress` qui ne fait rien.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn step(&self, _done: usize, _total: usize, _label: &str) {}
}

/// Détection des visages dans une image (édition complète). Injecté par
/// [`EngineBuilder::face_finder`] ; sans lui, les visages ne sont pas vérifiés.
pub trait FaceFinder: Sync {
    /// Visages d'une image encodée (PNG, JPEG…), en pixels de l'image.
    /// `None` : non vérifiés (image illisible, détection en échec).
    fn faces(&self, image: &[u8]) -> Option<Vec<PixelRect>>;
}

/// Réglages d'une anonymisation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnonymizeOptions {
    /// Zones d'image à masquer en plus de celles trouvées par l'OCR et de
    /// celles gardées dans l'analyse (`Analysis::masks`).
    pub manual_masks: Vec<ManualMask>,
}

/// Bilan d'un fichier anonymisé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    /// Remplacements de texte et zones d'image masquées.
    pub replacements: usize,
    pub warnings: Vec<Warning>,
}

/// Fichier anonymisé, contrôle de fuite passé.
#[derive(Clone)]
pub struct Anonymized {
    pub name: String,
    pub bytes: Vec<u8>,
    pub report: FileReport,
}

impl fmt::Debug for Anonymized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anonymized")
            .field("len", &self.bytes.len())
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

/// Fichier restauré.
#[derive(Clone)]
pub struct Restored {
    pub name: String,
    pub bytes: Vec<u8>,
    pub report: RestoreReport,
}

impl fmt::Debug for Restored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Restored")
            .field("len", &self.bytes.len())
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

/// Recherche d'un gestionnaire par extension (`anon_office::handler_for`…).
pub type HandlerLookup = fn(&str) -> Option<&'static dyn Handler>;

/// Extensions essayées sur les fonctions de recherche pour lister les formats gérés.
const PROBED_EXTENSIONS: &[&str] = &[
    "txt", "md", "csv", "tsv", "json", "log", "docx", "docm", "dotx", "xlsx", "xlsm", "xltx",
    "pptx", "pptm", "potx", "odt", "ods", "odp", "pdf", "png", "jpg", "jpeg", "gif", "bmp", "tif",
    "tiff", "webp", "rtf", "html", "htm", "xml", "eml",
];

/// Nom du détecteur d'IA de noms (`anon-ner`), non relancé pour les indices.
const NER_DETECTOR: &str = "ner";

/// Extensions dont les lignes sont découpées en cellules pour la détection.
const CSV_EXTENSIONS: &[&str] = &["csv", "tsv"];

/// Suffixe des fichiers restaurés.
const RESTORED_SUFFIX: &str = "_restaure";
/// Suffixes des fichiers produits retirés à la restauration, du plus long au
/// plus court (`_anonymise` : fichiers de la V1).
const PRODUCED_SUFFIXES: &[&str] = &["_masque_sans_cle", "_anonymise", "_anonyme", "_masque"];
/// Emplacement signalé quand une valeur validée subsiste dans le nom du
/// fichier produit (jamais le nom lui-même : il peut désigner une personne).
pub const FILE_NAME_PART: &str = "nom du fichier";
/// Caractères interdits dans un nom de fichier sous Windows ou macOS (dont les
/// séparateurs de chemin) : remplacés par `_` dans le texte inséré dans un nom.
const FILE_NAME_FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Construction d'un [`Engine`] par injection.
#[derive(Default)]
pub struct EngineBuilder {
    handlers: Vec<(String, &'static dyn Handler)>,
    lookups: Vec<HandlerLookup>,
    detectors: Vec<Box<dyn Detector + Send>>,
    image_processor: Option<Box<dyn ImageProcessor + Send>>,
    face_finder: Option<Box<dyn FaceFinder + Send>>,
}

impl fmt::Debug for EngineBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineBuilder")
            .field(
                "handlers",
                &self.handlers.iter().map(|(e, _)| e).collect::<Vec<_>>(),
            )
            .field("lookups", &self.lookups.len())
            .field(
                "detectors",
                &self.detectors.iter().map(|d| d.name()).collect::<Vec<_>>(),
            )
            .field("image_processor", &self.image_processor.is_some())
            .field("face_finder", &self.face_finder.is_some())
            .finish()
    }
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gestionnaire d'une extension. Prioritaire sur les fonctions de recherche.
    pub fn handler(mut self, extension: &str, handler: &'static dyn Handler) -> Self {
        let ext = extension
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        self.handlers.retain(|(e, _)| *e != ext);
        self.handlers.push((ext, handler));
        self
    }

    /// Fonction de recherche de gestionnaire (essayées dans l'ordre d'ajout).
    pub fn handlers_from(mut self, lookup: HandlerLookup) -> Self {
        self.lookups.push(lookup);
        self
    }

    pub fn detector(mut self, detector: Box<dyn Detector + Send>) -> Self {
        self.detectors.push(detector);
        self
    }

    pub fn image_processor(mut self, processor: Box<dyn ImageProcessor + Send>) -> Self {
        self.image_processor = Some(processor);
        self
    }

    /// Détecteur de visages (édition complète).
    pub fn face_finder(mut self, finder: Box<dyn FaceFinder + Send>) -> Self {
        self.face_finder = Some(finder);
        self
    }

    pub fn build(self) -> Engine {
        Engine {
            handlers: self.handlers,
            lookups: self.lookups,
            detectors: self.detectors,
            image_processor: self.image_processor,
            face_finder: self.face_finder,
        }
    }
}

/// Moteur d'anonymisation. `Send + Sync` : partageable entre l'interface et
/// un fil de travail.
pub struct Engine {
    handlers: Vec<(String, &'static dyn Handler)>,
    lookups: Vec<HandlerLookup>,
    detectors: Vec<Box<dyn Detector + Send>>,
    image_processor: Option<Box<dyn ImageProcessor + Send>>,
    face_finder: Option<Box<dyn FaceFinder + Send>>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("extensions", &self.supported_extensions())
            .field(
                "detectors",
                &self.detectors.iter().map(|d| d.name()).collect::<Vec<_>>(),
            )
            .field("image_processor", &self.image_processor.is_some())
            .field("face_finder", &self.face_finder.is_some())
            .finish()
    }
}

/// Clé du cache de détection : deux blocs identiques ne sont analysés qu'une fois.
type BlockKey = (String, BlockRole, Option<String>);

/// Fichier lu, prêt pour la détection.
struct Loaded {
    data: FileData,
    detect_blocks: Vec<TextBlock>,
    warnings: Vec<Warning>,
}

/// Nom sans l'extension, et l'extension (sans le point).
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (name, None),
    }
}

/// Texte sûr dans un nom de fichier : ni séparateur de chemin, ni caractère
/// interdit, ni caractère de contrôle.
fn file_name_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || FILE_NAME_FORBIDDEN.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Nom de fichier dont la partie avant l'extension passe par `transform`
/// (valeurs remplacées par leurs balises, ou balises restaurées) ; le texte
/// inséré est rendu sûr pour un nom de fichier.
fn transform_stem(name: &str, transform: &impl TextTransform) -> String {
    let (stem, ext) = split_name(name);
    let ctx = BlockContext::new(FILE_NAME_PART, BlockRole::Other);
    let reps: Vec<Replacement> = transform
        .replacements(stem, &ctx)
        .into_iter()
        .map(|r| Replacement {
            with: file_name_safe(&r.with),
            ..r
        })
        .collect();
    let stem = apply_replacements(stem, &reps);
    match ext {
        Some(e) => format!("{stem}.{e}"),
        None => stem.into_owned(),
    }
}

fn output_name(name: &str, suffix: &str, strip: &[&str]) -> String {
    let (stem, ext) = split_name(name);
    let stem = strip
        .iter()
        .find_map(|s| stem.strip_suffix(s))
        .unwrap_or(stem);
    match ext {
        Some(e) => format!("{stem}{suffix}.{e}"),
        None => format!("{stem}{suffix}"),
    }
}

/// Nom d'un fichier produit : suffixe de l'objectif (`Goal::output_suffix`).
/// `_anonyme` seulement en objectif « Anonymiser définitivement » et si la
/// dernière évaluation conclut à l'anonymat (invariant 9) ; sinon
/// `_masque_sans_cle` ; `_masque` en objectif « Masquer pour l'IA ».
pub fn output_file_name(name: &str, goal: Goal, verdict: Option<Verdict>) -> String {
    let anonymous = verdict == Some(Verdict::Anonymous);
    output_name(name, goal.output_suffix(anonymous), &[])
}

/// Nom d'un fichier produit à partir du nom d'origine, sans ses valeurs
/// validées : chacune est remplacée dans le nom comme dans le contenu, par la
/// même balise (`CR_Mme_Dupont.docx` → `CR_Mme_[NOM_1]_masque.docx`), puis le
/// suffixe de l'objectif est ajouté ([`output_file_name`]).
pub fn masked_file_name(name: &str, session: &Session, verdict: Option<Verdict>) -> String {
    masked_name_with(name, &Replacer::new(session), session.goal, verdict)
}

fn masked_name_with(
    name: &str,
    replacer: &Replacer,
    goal: Goal,
    verdict: Option<Verdict>,
) -> String {
    output_file_name(&transform_stem(name, replacer), goal, verdict)
}

/// Nom d'un fichier restauré : suffixe de production retiré, `_restaure`
/// ajouté, balises du nom remises à leurs valeurs.
fn restored_file_name(name: &str, session: &Session) -> String {
    // Restaurateur à part : le nom n'entre pas dans le bilan du contenu.
    let restorer = Restorer::new(session);
    transform_stem(
        &output_name(name, RESTORED_SUFFIX, PRODUCED_SUFFIXES),
        &restorer,
    )
}

fn text_blocks(text: &str) -> Vec<TextBlock> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(n, l)| TextBlock {
            text: l.to_string(),
            ctx: BlockContext::new(format!("ligne {}", n + 1), BlockRole::Body),
        })
        .collect()
}

impl Engine {
    pub fn builder() -> EngineBuilder {
        EngineBuilder::new()
    }

    /// Constructeur standard, à compléter (PDF, images, détecteur d'IA) avant `build`.
    pub fn standard_builder() -> EngineBuilder {
        EngineBuilder::new()
            .handlers_from(anon_office::handler_for)
            .detector(Box::new(RuleDetector::new()))
            .detector(Box::new(QuasiDetector::new()))
            .detector(Box::new(ColumnDetector::new()))
            .detector(Box::new(DictionaryDetector::new()))
    }

    /// Gestionnaires d'`anon-office`, règles, quasi-identifiants, colonnes, dictionnaires.
    pub fn standard() -> Engine {
        Self::standard_builder().build()
    }

    /// Extensions gérées, triées, sans doublon.
    pub fn supported_extensions(&self) -> Vec<&'static str> {
        let mut out: BTreeSet<&'static str> = BTreeSet::new();
        for (_, h) in &self.handlers {
            out.extend(h.extensions().iter().copied());
        }
        for lookup in &self.lookups {
            for ext in PROBED_EXTENSIONS {
                if let Some(h) = lookup(ext) {
                    out.extend(h.extensions().iter().copied());
                }
            }
        }
        // Une extension enregistrée explicitement mais absente des `extensions()` de son gestionnaire.
        for (e, h) in &self.handlers {
            if let Some(s) = h.extensions().iter().find(|x| **x == e.as_str()) {
                out.insert(s);
            }
        }
        out.into_iter().collect()
    }

    fn handler_for(&self, ext: &str) -> Option<&'static dyn Handler> {
        if ext.is_empty() {
            return None;
        }
        if let Some((_, h)) = self.handlers.iter().find(|(e, _)| e == ext) {
            return Some(*h);
        }
        self.lookups.iter().find_map(|l| l(ext))
    }

    fn handler_of(&self, file: &InputFile) -> Result<&'static dyn Handler, EngineError> {
        let ext = file.extension();
        self.handler_for(&ext)
            .ok_or(EngineError::Unsupported { extension: ext })
    }

    fn load(&self, file: &InputFile) -> Result<Loaded, EngineError> {
        let handler = self.handler_of(file)?;
        if file.bytes.is_empty() {
            return Err(EngineError::Empty);
        }
        let ex = handler.extract(&file.bytes)?;
        let mut warnings = ex.warnings;
        let mut images = Vec::new();
        let mut unread_media = Vec::new();
        let faces = |bytes: &[u8]| self.face_finder.as_deref().and_then(|f| f.faces(bytes));
        match self.image_processor.as_deref() {
            Some(p) => {
                for m in &ex.media {
                    match p.ocr(&m.bytes) {
                        Ok(page) => {
                            if !page.words.is_empty() && page.mean_confidence() < LOW_OCR_CONFIDENCE
                            {
                                warnings.push(Warning::new(
                                    WarningCode::LowOcrConfidence,
                                    m.name.clone(),
                                    "Lecture de l'image incertaine : vérifiez-la à l'œil.",
                                ));
                            }
                            images.push(ImageData {
                                name: m.name.clone(),
                                sha256: sha256_hex(&m.bytes),
                                bytes: m.bytes.clone(),
                                ocr: page,
                                faces: faces(&m.bytes),
                            });
                        }
                        Err(_) => {
                            warnings.push(Warning::new(
                                WarningCode::ImageNotProcessed,
                                m.name.clone(),
                                "Image illisible par l'OCR, non analysée.",
                            ));
                            unread_media.push(UnreadMedia {
                                name: m.name.clone(),
                                faces: faces(&m.bytes),
                            });
                        }
                    }
                }
            }
            None => {
                if !ex.media.is_empty() {
                    warnings.push(Warning::new(
                        WarningCode::ImageNotProcessed,
                        "images",
                        format!(
                            "{} image(s) non analysée(s) : lecture des images indisponible.",
                            ex.media.len()
                        ),
                    ));
                }
                unread_media.extend(ex.media.iter().map(|m| UnreadMedia {
                    name: m.name.clone(),
                    faces: faces(&m.bytes),
                }));
            }
        }
        // Tableaux pour l'évaluation : une feuille illisible n'empêche pas l'analyse.
        let tables = match handler.tables(&file.bytes) {
            Ok(t) => t,
            Err(_) => {
                warnings.push(Warning::new(
                    WarningCode::UnsupportedPart,
                    "tableaux",
                    "Tableaux illisibles : ils ne sont pas évalués.",
                ));
                Vec::new()
            }
        };
        // Un gestionnaire qui rend déjà des cellules n'a pas besoin du découpage.
        let has_cells = ex.blocks.iter().any(|b| b.ctx.role == BlockRole::Cell);
        let mut detect_blocks = if !has_cells && CSV_EXTENSIONS.contains(&file.extension().as_str())
        {
            crate::csv::cell_blocks(&ex.blocks).unwrap_or_else(|| ex.blocks.clone())
        } else {
            ex.blocks.clone()
        };
        detect_blocks.extend(images.iter().map(|i| TextBlock {
            text: i.ocr.text_with_spans().0,
            ctx: BlockContext::new(i.name.clone(), BlockRole::Ocr),
        }));
        Ok(Loaded {
            data: FileData {
                name: file.name.clone(),
                blocks: ex.blocks,
                images,
                tables,
                unread_media,
            },
            detect_blocks,
            warnings,
        })
    }

    /// Détecteurs de l'engin plus la liste « toujours », filtrage puis fusion.
    fn detect_block(
        &self,
        block: &TextBlock,
        opts: &DetectOptions,
        list: &ListDetector,
    ) -> Vec<Detection> {
        let text = block.text.as_str();
        let extra: Option<&dyn Detector> = (!list.is_empty()).then_some(list as &dyn Detector);
        let all: Vec<Detection> = self
            .detectors
            .iter()
            .map(|d| d.as_ref() as &dyn Detector)
            .chain(extra)
            .flat_map(|d| d.detect(block, opts))
            .filter(|d| {
                opts.wants(d.kind)
                    && d.start < d.end
                    && d.end <= text.len()
                    && text.is_char_boundary(d.start)
                    && text.is_char_boundary(d.end)
            })
            .collect();
        merge(all)
    }

    /// Indices (dates, âges, lieux, fonctions, montants, matricules, événements)
    /// que le profil `opts` ne cherche pas : l'évaluation en a besoin pour voir
    /// ce qui reste lisible. Détecteurs à règles et dictionnaires seulement (l'IA
    /// de noms n'est pas relancée) ; ces valeurs entrent décochées dans l'analyse.
    fn detect_clues(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        let text = block.text.as_str();
        let clue_opts = DetectOptions {
            profile: Profile::Renforce,
            disabled: opts.disabled.clone(),
        };
        let all: Vec<Detection> = self
            .detectors
            .iter()
            .filter(|d| d.name() != NER_DETECTOR)
            .flat_map(|d| d.detect(block, &clue_opts))
            .filter(|d| {
                clue_opts.wants(d.kind)
                    && !opts.wants(d.kind)
                    && d.start < d.end
                    && d.end <= text.len()
                    && text.is_char_boundary(d.start)
                    && text.is_char_boundary(d.end)
            })
            .collect();
        merge(all)
    }

    fn analyze_with(
        &self,
        names: Vec<String>,
        load: &dyn Fn(usize) -> Result<Loaded, EngineError>,
        opts: &AnalyzeOptions,
        session: &Session,
        progress: &dyn Progress,
    ) -> Analysis {
        let total = names.len();
        let list = ListDetector::new(opts.always.clone());
        let mut never: Vec<String> = session.never.clone();
        never.extend(opts.never.iter().cloned());
        let mut agg = Aggregator::new(&never);
        let mut cache: HashMap<BlockKey, Vec<Detection>> = HashMap::new();
        let mut clue_cache: HashMap<BlockKey, Vec<Detection>> = HashMap::new();
        let clues_wanted = opts.detect.profile == Profile::Standard;
        let mut files = Vec::with_capacity(total);
        let mut store = Vec::with_capacity(total);
        let mut cancelled = false;
        for (i, name) in names.into_iter().enumerate() {
            if cancelled || progress.cancelled() {
                cancelled = true;
                files.push(FileAnalysis {
                    name: name.clone(),
                    blocks: 0,
                    images: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(EngineError::Cancelled),
                });
                store.push(FileData::empty(&name));
                continue;
            }
            progress.step(i, total, &name);
            match load(i) {
                Ok(loaded) => {
                    for block in &loaded.detect_blocks {
                        let key: BlockKey = (
                            block.text.clone(),
                            block.ctx.role,
                            block.ctx.column_header.clone(),
                        );
                        let dets = cache
                            .entry(key.clone())
                            .or_insert_with(|| self.detect_block(block, &opts.detect, &list));
                        for d in dets.iter() {
                            agg.add(
                                d.kind,
                                &block.text,
                                d.start,
                                d.end,
                                d.source,
                                d.confidence,
                                i,
                            );
                        }
                        if clues_wanted {
                            let clues = clue_cache
                                .entry(key)
                                .or_insert_with(|| self.detect_clues(block, &opts.detect));
                            for c in clues.iter() {
                                if dets.iter().any(|d| d.start < c.end && c.start < d.end) {
                                    continue;
                                }
                                agg.add(
                                    c.kind,
                                    &block.text,
                                    c.start,
                                    c.end,
                                    c.source,
                                    c.confidence,
                                    i,
                                );
                            }
                        }
                    }
                    files.push(FileAnalysis {
                        name,
                        blocks: loaded.data.blocks.len(),
                        images: loaded.data.images.iter().map(|m| m.name.clone()).collect(),
                        warnings: loaded.warnings,
                        error: None,
                    });
                    store.push(loaded.data);
                }
                Err(e) => {
                    files.push(FileAnalysis {
                        name: name.clone(),
                        blocks: 0,
                        images: Vec::new(),
                        warnings: Vec::new(),
                        error: Some(e),
                    });
                    store.push(FileData::empty(&name));
                }
            }
        }
        progress.step(total, total, "");
        Analysis::finish(
            files,
            store,
            agg,
            never,
            session,
            cancelled,
            opts.detect.profile,
        )
    }

    /// Analyse un lot de fichiers. Un fichier illisible n'interrompt pas les
    /// autres ; l'annulation est vérifiée avant chaque fichier.
    pub fn analyze(
        &self,
        files: &[InputFile],
        opts: &AnalyzeOptions,
        session: &Session,
        progress: &dyn Progress,
    ) -> Analysis {
        let names = files.iter().map(|f| f.name.clone()).collect();
        let load = |i: usize| match files.get(i) {
            Some(f) => self.load(f),
            None => Err(EngineError::Cancelled),
        };
        self.analyze_with(names, &load, opts, session, progress)
    }

    /// Analyse un texte collé (une ligne par bloc).
    pub fn analyze_text(&self, text: &str, opts: &AnalyzeOptions, session: &Session) -> Analysis {
        let load = |_: usize| {
            let blocks = text_blocks(text);
            Ok(Loaded {
                data: FileData {
                    name: "texte".into(),
                    blocks: blocks.clone(),
                    images: Vec::new(),
                    tables: Vec::new(),
                    unread_media: Vec::new(),
                },
                detect_blocks: blocks,
                warnings: Vec::new(),
            })
        };
        self.analyze_with(vec!["texte".into()], &load, opts, session, &NoProgress)
    }

    /// Enregistre les candidats cochés dans la session : personnes complètes
    /// d'abord, variantes, casse et traitement reportés, liste « jamais » ajoutée.
    /// Un candidat décoché déjà présent dans la session y est désactivé.
    ///
    /// Objectif « Anonymiser définitivement » (R7) : la session repart de zéro
    /// (valeurs et images d'origine retirées, liste « jamais » gardée) ; la
    /// numérotation des balises est propre à cet envoi.
    pub fn apply_selection(&self, analysis: &Analysis, session: &mut Session) {
        if session.goal == Goal::Anonymize {
            session.entities.clear();
            session.media.clear();
        }
        analysis.apply_to(session);
    }

    /// Anonymise un fichier avec la session : masquage des images, réécriture,
    /// contrôle de fuite bloquant. Aucun octet n'est rendu si une valeur subsiste.
    ///
    /// Le nom du fichier produit suit l'objectif de la session
    /// ([`output_file_name`]) ; en « Anonymiser définitivement », le dossier est
    /// évalué pour le destinataire de la session, approche simplifiée et
    /// `k_min` par défaut (voir [`Engine::anonymize_with`] pour d'autres réglages).
    pub fn anonymize(
        &self,
        file: &InputFile,
        analysis: &Analysis,
        session: &mut Session,
        opts: &AnonymizeOptions,
    ) -> Result<Anonymized, EngineError> {
        let settings = AssessSettings {
            goal: session.goal,
            recipient: session.recipient,
            ..AssessSettings::default()
        };
        self.anonymize_with(file, analysis, session, opts, &settings)
    }

    /// Comme [`Engine::anonymize`], avec les réglages de l'évaluation choisis
    /// par l'utilisateur. En « Anonymiser définitivement », le dossier est
    /// réévalué ici, sur l'analyse telle qu'elle est : le suffixe `_anonyme`
    /// n'est jamais tiré d'un verdict périmé (invariant 9).
    pub fn anonymize_with(
        &self,
        file: &InputFile,
        analysis: &Analysis,
        session: &mut Session,
        opts: &AnonymizeOptions,
        settings: &AssessSettings,
    ) -> Result<Anonymized, EngineError> {
        let verdict = (session.goal == Goal::Anonymize).then(|| {
            let s = AssessSettings {
                goal: session.goal,
                ..*settings
            };
            self.assess(analysis, &s).verdict
        });
        let handler = self.handler_of(file)?;
        if file.bytes.is_empty() {
            return Err(EngineError::Empty);
        }
        let replacer = Replacer::new(session);
        // Images d'origine gardées pour la restauration : seulement en objectif
        // « Masquer pour l'IA » avec une clé (R7 : aucune en « Anonymiser
        // définitivement »).
        let keep_originals = !session.irreversible && session.goal == Goal::Mask;
        let mut masks = opts.manual_masks.clone();
        if let Some(fi) = analysis.file_index(&file.name) {
            masks.extend(
                analysis
                    .masks()
                    .iter()
                    .filter(|m| m.file == fi)
                    .map(|m| ManualMask {
                        media: m.media.clone(),
                        rect: m.rect,
                    }),
            );
        }
        let masker = self
            .image_processor
            .as_deref()
            .map(|p| Masker::new(p, &replacer, analysis.ocr_by_sha(), &masks, keep_originals));
        let topts = TransformOptions {
            media: masker.as_ref().map(|m| m as &dyn MediaTransform),
            ..TransformOptions::anonymize()
        };
        let out = handler.transform(&file.bytes, &replacer, &topts)?;
        let (originals, mask_warnings, masked) = match masker {
            Some(m) => {
                if let Some(e) = m.take_error() {
                    return Err(e.into());
                }
                m.finish()
            }
            None => (Vec::new(), Vec::new(), 0),
        };

        // Contrôle de fuite : lecture exhaustive puis recherche de chaque valeur.
        let deep = handler.deep_scan(&out.bytes)?;
        let mut leaks: Vec<Leak> = text_leaks(&deep, &replacer, &out.bytes);
        let mut push_leak = |leak: Leak| {
            if !leaks.contains(&leak) {
                leaks.push(leak);
            }
        };
        // Le nom du fichier produit passe par le même remplacement que le
        // contenu, puis par le même contrôle (hors extension : celle d'un
        // format géré, jamais une donnée).
        let name = masked_name_with(&file.name, &replacer, session.goal, verdict);
        for m in replacer.matches(split_name(&name).0) {
            push_leak(Leak {
                tag: m.tag,
                part: FILE_NAME_PART.to_owned(),
            });
        }
        let mut warnings: Vec<Warning> = analysis
            .file(&file.name)
            .map(|f| f.warnings.clone())
            .unwrap_or_default();
        warnings.extend(out.warnings.iter().cloned());
        warnings.extend(mask_warnings);
        // Empreintes des images réellement présentes dans la sortie : seules
        // celles-là pourront être reconnues, donc remises, à la restauration.
        let mut output_images: BTreeSet<String> = BTreeSet::new();
        if let Some(p) = self.image_processor.as_deref() {
            for media in handler.extract(&out.bytes)?.media {
                output_images.insert(sha256_hex(&media.bytes));
                match p.ocr(&media.bytes) {
                    Ok(page) => {
                        for m in replacer.matches(&page.text_with_spans().0) {
                            push_leak(Leak {
                                tag: m.tag,
                                part: media.name.clone(),
                            });
                        }
                    }
                    Err(_) => warnings.push(Warning::new(
                        WarningCode::ImageNotProcessed,
                        media.name.clone(),
                        "Image non vérifiée après masquage : vérifiez-la à l'œil.",
                    )),
                }
            }
        }
        if !leaks.is_empty() {
            return Err(EngineError::Leak { leaks });
        }

        if keep_originals {
            // Un format qui ne conserve pas l'image octet pour octet (page d'un PDF
            // scanné) ne permet pas de la reconnaître ensuite : garder son original
            // alourdirait la table pour rien. On le dit à l'utilisateur.
            let (originals, lost): (Vec<_>, Vec<_>) = originals
                .into_iter()
                .partition(|o| output_images.contains(&o.masked_sha256));
            if !lost.is_empty() {
                warnings.push(Warning::new(
                    WarningCode::ImageNotRestorable,
                    file.extension(),
                    format!(
                        "{} image(s) masquée(s) ne pourront pas être remises à la restauration dans ce format : conservez le fichier d'origine.",
                        lost.len()
                    ),
                ));
            }
            for o in originals {
                if !session
                    .media
                    .iter()
                    .any(|m| m.masked_sha256 == o.masked_sha256)
                {
                    session.media.push(o);
                }
            }
        }
        Ok(Anonymized {
            name,
            bytes: out.bytes,
            report: FileReport {
                replacements: out.replacements.saturating_add(masked),
                warnings,
            },
        })
    }

    /// Texte anonymisé du fichier, blocs dans l'ordre (repli quand le contrôle
    /// de fuite du fichier échoue). Les métadonnées sont omises.
    pub fn export_text(&self, file: &InputFile, session: &Session) -> Result<String, EngineError> {
        let handler = self.handler_of(file)?;
        if file.bytes.is_empty() {
            return Err(EngineError::Empty);
        }
        let ex = handler.extract(&file.bytes)?;
        let replacer = Replacer::new(session);
        let mut out = Vec::with_capacity(ex.blocks.len());
        let mut leaks = Vec::new();
        for b in ex
            .blocks
            .iter()
            .filter(|b| !matches!(b.ctx.role, BlockRole::Metadata | BlockRole::Hyperlink))
        {
            let text = replacer.apply(&b.text, &b.ctx).into_owned();
            for m in replacer.matches(&text) {
                leaks.push(Leak {
                    tag: m.tag,
                    part: b.ctx.part.clone(),
                });
            }
            out.push(text);
        }
        if !leaks.is_empty() {
            return Err(EngineError::Leak { leaks });
        }
        Ok(out.join("\n"))
    }

    /// Restaure un fichier anonymisé : valeurs réelles et images d'origine.
    pub fn restore(&self, file: &InputFile, session: &Session) -> Result<Restored, EngineError> {
        let handler = self.handler_of(file)?;
        if file.bytes.is_empty() {
            return Err(EngineError::Empty);
        }
        let restorer = Restorer::new(session);
        let unmasker = Unmasker::new(session);
        let opts = TransformOptions {
            media: (!unmasker.is_empty()).then_some(&unmasker as &dyn MediaTransform),
            ..TransformOptions::restore()
        };
        let out = handler.transform(&file.bytes, &restorer, &opts)?;
        Ok(Restored {
            name: restored_file_name(&file.name, session),
            bytes: out.bytes,
            report: restorer.report(),
        })
    }

    /// Texte collé anonymisé avec la session.
    pub fn anonymize_text(&self, text: &str, session: &Session) -> String {
        Replacer::new(session)
            .apply(text, &BlockContext::new("texte", BlockRole::Body))
            .into_owned()
    }

    /// Réponse d'IA restaurée, avec le bilan (balises inconnues…).
    pub fn restore_text(&self, text: &str, session: &Session) -> (String, RestoreReport) {
        let (out, _, report) = self.restore_text_marked(text, session);
        (out, report)
    }

    /// Comme [`Engine::restore_text`], avec la place (octets, dans le texte
    /// restauré) de chaque valeur remise : l'interface les surligne.
    pub fn restore_text_marked(
        &self,
        text: &str,
        session: &Session,
    ) -> (String, Vec<std::ops::Range<usize>>, RestoreReport) {
        let r = Restorer::new(session);
        // Même parcours que `apply_replacements` : texte restauré identique.
        let reps = r.replacements(text, &BlockContext::new("texte", BlockRole::Body));
        let mut out = String::with_capacity(text.len());
        let mut marks = Vec::with_capacity(reps.len());
        let mut at = 0;
        for rep in reps {
            if rep.start < at || rep.end > text.len() || rep.start > rep.end {
                continue;
            }
            let Some(before) = text.get(at..rep.start) else {
                continue;
            };
            out.push_str(before);
            let start = out.len();
            out.push_str(&rep.with);
            marks.push(start..out.len());
            at = rep.end;
        }
        out.push_str(text.get(at..).unwrap_or_default());
        (out, marks, r.report())
    }

    /// Vrai si l'édition vérifie les visages (détecteur injecté).
    pub fn checks_faces(&self) -> bool {
        self.face_finder.is_some()
    }

    /// Entrée de l'évaluation construite à partir de l'analyse et de la
    /// sélection courante (voir le module `evaluation`).
    pub fn assess_input(&self, analysis: &Analysis) -> AssessInput {
        evaluation::build_input(analysis, self.image_processor.is_some())
    }

    /// Évalue le dossier pour un destinataire : isolement, recoupement,
    /// déduction, sujet par sujet. Moins d'une seconde sur les exemples.
    pub fn assess(&self, analysis: &Analysis, settings: &AssessSettings) -> Assessment {
        self.assess_from(&self.assess_input(analysis), settings)
    }

    /// Évalue une entrée déjà construite par [`Engine::assess_input`] (qui
    /// donne aussi, par exemple, les tableaux évalués et leurs feuilles).
    pub fn assess_from(&self, input: &AssessInput, settings: &AssessSettings) -> Assessment {
        anon_assess::assess(input, settings)
    }

    /// Applique une correction proposée par l'évaluation. `Mask` coche la
    /// valeur avec une balise ; `Generalize` et `GeneralizeMany` la cochent
    /// avec le traitement « rendre moins précis » ; `SuppressRows` masque les
    /// valeurs listées et retire les lignes de l'évaluation ; `MaskRegion`
    /// ajoute une zone masquée, appliquée par [`Engine::anonymize`]. Le
    /// résultat permet d'annuler ([`Engine::undo_fix`], Ctrl+Z).
    pub fn apply_fix(&self, analysis: &mut Analysis, fix: &Fix) -> FixUndo {
        evaluation::apply_fix(analysis, fix)
    }

    /// Annule une correction (ou « Tout corriger ») : l'analyse revient
    /// exactement à son état d'avant. Annuler dans l'ordre inverse des
    /// applications. Renvoie de quoi rétablir (Ctrl+Maj+Z) : passé de nouveau
    /// à `undo_fix`, il redonne l'état d'après, et de quoi l'annuler encore.
    pub fn undo_fix(&self, analysis: &mut Analysis, undo: FixUndo) -> FixUndo {
        evaluation::undo_fix(analysis, undo)
    }

    /// « Tout corriger » : applique la première correction de chaque échec,
    /// réévalue, et recommence tant qu'une correction change quelque chose (au
    /// plus [`FIX_ALL_MAX_ROUNDS`] passes : une généralisation peut laisser
    /// quelques lignes à supprimer). Une seule annulation défait le tout.
    pub fn fix_all(&self, analysis: &mut Analysis, settings: &AssessSettings) -> FixUndo {
        let mut undo = FixUndo::default();
        for _ in 0..FIX_ALL_MAX_ROUNDS {
            let assessment = self.assess(analysis, settings);
            let mut seen: Vec<&Fix> = Vec::new();
            let mut round = FixUndo::default();
            for fix in assessment.failures.iter().filter_map(|f| f.fixes.first()) {
                if seen.contains(&fix) {
                    continue;
                }
                seen.push(fix);
                round.extend(evaluation::apply_fix(analysis, fix));
            }
            if round.is_empty() {
                break;
            }
            undo.extend(round);
        }
        undo
    }
}

/// Valeurs validées trouvées par la lecture exhaustive du fichier produit, moins
/// les identifiants structurels du format (numéro de ligne, référence de
/// cellule, priorité de style : voir le module `structure`). Une occurrence
/// structurelle n'en compense qu'une dans la même partie : une valeur restée
/// dans le contenu est toujours signalée.
fn text_leaks(deep: &[TextBlock], replacer: &Replacer, bytes: &[u8]) -> Vec<Leak> {
    let mut found: Vec<(Leak, usize)> = Vec::new();
    for b in deep {
        for m in replacer.matches(&b.text) {
            let leak = Leak {
                tag: m.tag,
                part: b.ctx.part.clone(),
            };
            match found.iter_mut().find(|(l, _)| *l == leak) {
                Some((_, n)) => *n = n.saturating_add(1),
                None => found.push((leak, 1)),
            }
        }
    }
    if found.is_empty() {
        return Vec::new();
    }
    let structural: HashMap<(String, Tag), usize> = structural_counts(bytes, replacer);
    found
        .into_iter()
        .filter(|(leak, n)| {
            let credit = structural
                .get(&(leak.part.clone(), leak.tag))
                .copied()
                .unwrap_or(0);
            *n > credit
        })
        .map(|(leak, _)| leak)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::Treatment;

    #[test]
    fn names() {
        assert_eq!(
            output_file_name("contrat.docx", Goal::Mask, None),
            "contrat_masque.docx"
        );
        assert_eq!(
            output_file_name("contrat.docx", Goal::Mask, Some(Verdict::Anonymous)),
            "contrat_masque.docx"
        );
        assert_eq!(
            output_file_name("clients.xlsx", Goal::Anonymize, Some(Verdict::Anonymous)),
            "clients_anonyme.xlsx"
        );
        for verdict in [
            None,
            Some(Verdict::Inconclusive),
            Some(Verdict::Reidentifiable { subjects: 1 }),
        ] {
            assert_eq!(
                output_file_name("clients.xlsx", Goal::Anonymize, verdict),
                "clients_masque_sans_cle.xlsx"
            );
        }
        for produced in [
            "contrat_masque.docx",
            "contrat_anonymise.docx",
            "contrat_masque_sans_cle.docx",
            "contrat_anonyme.docx",
        ] {
            assert_eq!(
                output_name(produced, RESTORED_SUFFIX, PRODUCED_SUFFIXES),
                "contrat_restaure.docx"
            );
        }
        assert_eq!(output_name("notes", "_x", &[]), "notes_x");
        assert_eq!(output_name(".profile", "_x", &[]), ".profile_x");
    }

    fn name_session() -> Session {
        use anon_core::{EntityKind, Source};
        use anon_replace::SessionExt;
        let mut s = Session::new();
        s.intern(EntityKind::Personne, "Jeanne Dupont", Source::Manual);
        s.intern(EntityKind::Nom, "Dupont", Source::Manual);
        s.intern(EntityKind::Date, "12/03/1980", Source::Manual);
        s
    }

    #[test]
    fn produced_name_masks_validated_values() {
        let s = name_session();
        assert_eq!(
            masked_file_name("CR_Mme_Dupont.docx", &s, None),
            "CR_Mme_[NOM_1]_masque.docx"
        );
        assert_eq!(
            masked_file_name("Jeanne Dupont - bilan.pdf", &s, None),
            "[PERSONNE_1] - bilan_masque.pdf"
        );
        // Un mot qui contient la valeur n'est pas touché.
        assert_eq!(
            masked_file_name("Dupontel.txt", &s, None),
            "Dupontel_masque.txt"
        );
        assert_eq!(masked_file_name("Dupont", &s, None), "[NOM_1]_masque");
        assert_eq!(
            masked_file_name("contrat.docx", &s, None),
            "contrat_masque.docx"
        );
    }

    #[test]
    fn produced_name_never_gets_a_path_separator() {
        use anon_replace::SessionExt;
        let mut s = name_session();
        let tag = s.tag_for(anon_core::EntityKind::Nom, "Dupont").unwrap();
        assert!(s.set_treatment(tag, Treatment::Generalize("..\\a/b:c*?".into())));
        assert_eq!(
            masked_file_name("Mme Dupont.txt", &s, None),
            "Mme .._a_b_c___masque.txt"
        );
    }

    #[test]
    fn restored_name_puts_values_back() {
        let s = name_session();
        assert_eq!(
            restored_file_name("CR_Mme_[NOM_1]_masque.docx", &s),
            "CR_Mme_Dupont_restaure.docx"
        );
        // Une vraie valeur ne devient jamais un séparateur de chemin.
        assert_eq!(
            restored_file_name("naissance [DATE_1]_masque.txt", &s),
            "naissance 12_03_1980_restaure.txt"
        );
    }

    #[test]
    fn anonymize_masks_the_file_name() {
        let engine = Engine::standard();
        let file = InputFile::new("CR_Mme_Dupont.txt", b"Mme Dupont est venue.".to_vec());
        let mut session = name_session();
        let analysis = engine.analyze(
            std::slice::from_ref(&file),
            &AnalyzeOptions::default(),
            &session,
            &NoProgress,
        );
        let out = engine
            .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
            .unwrap();
        assert_eq!(out.name, "CR_Mme_[NOM_1]_masque.txt");
        let restored = engine
            .restore(&InputFile::new(out.name, out.bytes), &session)
            .unwrap();
        assert_eq!(restored.name, "CR_Mme_Dupont_restaure.txt");
        assert_eq!(restored.bytes, b"Mme Dupont est venue.");
        // Le nom n'entre pas dans le bilan de la restauration du contenu.
        assert_eq!(restored.report.restored, 1);
    }

    #[test]
    fn leak_check_covers_the_file_name() {
        use anon_core::{EntityKind, Source};
        use anon_replace::SessionExt;
        let engine = Engine::standard();
        // « Martin » rendu moins précis en une autre valeur validée : le nom
        // produit la contiendrait, le fichier n'est pas livré.
        let mut session = Session::new();
        let martin = session
            .intern(EntityKind::Nom, "Martin", Source::Manual)
            .unwrap();
        session.intern(EntityKind::Nom, "Durand", Source::Manual);
        session.set_treatment(martin, Treatment::Generalize("Durand".into()));
        let file = InputFile::new("Martin.txt", b"Bonjour.".to_vec());
        let analysis = engine.analyze(
            std::slice::from_ref(&file),
            &AnalyzeOptions::default(),
            &session,
            &NoProgress,
        );
        match engine.anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default()) {
            Err(EngineError::Leak { leaks }) => assert_eq!(
                leaks,
                [Leak {
                    tag: Tag::new(EntityKind::Nom, 2),
                    part: FILE_NAME_PART.to_owned(),
                }]
            ),
            other => unreachable!("{:?}", other.map(|o| o.name)),
        }
    }

    #[test]
    fn engine_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Engine>();
    }
}
