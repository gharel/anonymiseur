//! Orchestration : gestionnaires de formats, détecteurs, table de correspondance.

use crate::analysis::{Aggregator, Analysis, AnalyzeOptions, FileAnalysis, FileData, ImageData};
use crate::error::{EngineError, Leak, extension_of};
use crate::media::{LOW_OCR_CONFIDENCE, ManualMask, Masker, Unmasker, sha256_hex};
use crate::risk::{RiskReport, assess};
use anon_core::{
    BlockContext, BlockRole, DetectOptions, Detection, Detector, EntityKind, Handler,
    ImageProcessor, MediaTransform, Profile, Session, TextBlock, TextTransform, TransformOptions,
    Warning, WarningCode,
};
use anon_detect::{ColumnDetector, ListDetector, QuasiDetector, RuleDetector, merge};
use anon_dict::DictionaryDetector;
use anon_replace::{Replacer, RestoreReport, Restorer};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// Réglages d'une anonymisation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnonymizeOptions {
    /// Zones d'image à masquer en plus de celles trouvées par l'OCR.
    pub manual_masks: Vec<ManualMask>,
}

/// Bilan d'un fichier anonymisé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    /// Remplacements de texte et zones d'image masquées.
    pub replacements: usize,
    pub warnings: Vec<Warning>,
    pub risk: RiskReport,
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

/// Extensions dont les lignes sont découpées en cellules pour la détection.
const CSV_EXTENSIONS: &[&str] = &["csv", "tsv"];

/// Suffixes des noms de fichiers produits.
const ANONYMIZED_SUFFIX: &str = "_anonymise";
const RESTORED_SUFFIX: &str = "_restaure";

/// Construction d'un [`Engine`] par injection.
#[derive(Default)]
pub struct EngineBuilder {
    handlers: Vec<(String, &'static dyn Handler)>,
    lookups: Vec<HandlerLookup>,
    detectors: Vec<Box<dyn Detector + Send>>,
    image_processor: Option<Box<dyn ImageProcessor + Send>>,
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

    pub fn build(self) -> Engine {
        Engine {
            handlers: self.handlers,
            lookups: self.lookups,
            detectors: self.detectors,
            image_processor: self.image_processor,
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

fn output_name(name: &str, suffix: &str, strip: Option<&str>) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (name, None),
    };
    let stem = strip.and_then(|s| stem.strip_suffix(s)).unwrap_or(stem);
    match ext {
        Some(e) => format!("{stem}{suffix}.{e}"),
        None => format!("{stem}{suffix}"),
    }
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
                            });
                        }
                        Err(_) => warnings.push(Warning::new(
                            WarningCode::ImageNotProcessed,
                            m.name.clone(),
                            "Image illisible par l'OCR, non analysée.",
                        )),
                    }
                }
            }
            None if !ex.media.is_empty() => warnings.push(Warning::new(
                WarningCode::ImageNotProcessed,
                "images",
                format!(
                    "{} image(s) non analysée(s) : lecture des images indisponible.",
                    ex.media.len()
                ),
            )),
            None => {}
        }
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
                            .entry(key)
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
        Analysis::finish(files, store, agg, never, session, cancelled)
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
    pub fn apply_selection(&self, analysis: &Analysis, session: &mut Session) {
        analysis.apply_to(session);
    }

    /// Quasi-identifiants encore visibles dans des blocs.
    fn risk(&self, blocks: &[TextBlock]) -> RiskReport {
        let opts = DetectOptions {
            profile: Profile::Renforce,
            disabled: BTreeSet::new(),
        };
        let list = ListDetector::default();
        let mut counts: BTreeMap<EntityKind, usize> = BTreeMap::new();
        for b in blocks {
            for d in self.detect_block(b, &opts, &list) {
                let inside_tag = b
                    .text
                    .get(d.start..d.end)
                    .is_none_or(|s| s.contains('[') || s.contains(']'));
                if d.kind.is_quasi_identifier() && !inside_tag {
                    let n = counts.entry(d.kind).or_insert(0);
                    *n = n.saturating_add(1);
                }
            }
        }
        assess(&counts)
    }

    /// Anonymise un fichier avec la session : masquage des images, réécriture,
    /// contrôle de fuite bloquant. Aucun octet n'est rendu si une valeur subsiste.
    pub fn anonymize(
        &self,
        file: &InputFile,
        analysis: &Analysis,
        session: &mut Session,
        opts: &AnonymizeOptions,
    ) -> Result<Anonymized, EngineError> {
        let handler = self.handler_of(file)?;
        if file.bytes.is_empty() {
            return Err(EngineError::Empty);
        }
        let replacer = Replacer::new(session);
        let masker = self.image_processor.as_deref().map(|p| {
            Masker::new(
                p,
                &replacer,
                analysis.ocr_by_sha(),
                &opts.manual_masks,
                !session.irreversible,
            )
        });
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
        let mut leaks: Vec<Leak> = Vec::new();
        let mut push_leak = |leak: Leak| {
            if !leaks.contains(&leak) {
                leaks.push(leak);
            }
        };
        for b in &deep {
            for m in replacer.matches(&b.text) {
                push_leak(Leak {
                    tag: m.tag,
                    part: b.ctx.part.clone(),
                });
            }
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

        let risk = self.risk(&deep);
        if !session.irreversible {
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
            name: output_name(&file.name, ANONYMIZED_SUFFIX, None),
            bytes: out.bytes,
            report: FileReport {
                replacements: out.replacements.saturating_add(masked),
                warnings,
                risk,
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
            name: output_name(&file.name, RESTORED_SUFFIX, Some(ANONYMIZED_SUFFIX)),
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
        let r = Restorer::new(session);
        let out = r
            .apply(text, &BlockContext::new("texte", BlockRole::Body))
            .into_owned();
        (out, r.report())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            output_name("contrat.docx", ANONYMIZED_SUFFIX, None),
            "contrat_anonymise.docx"
        );
        assert_eq!(
            output_name(
                "contrat_anonymise.docx",
                RESTORED_SUFFIX,
                Some(ANONYMIZED_SUFFIX)
            ),
            "contrat_restaure.docx"
        );
        assert_eq!(output_name("notes", "_x", None), "notes_x");
        assert_eq!(output_name(".profile", "_x", None), ".profile_x");
    }

    #[test]
    fn engine_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Engine>();
    }
}
