//! Opérations sur les fichiers, appelées depuis le fil de travail : lecture,
//! évaluation du dossier et « Tout corriger », masquage par lot, écriture sans
//! écrasement, scellement de la clé de restauration (dossier des clés,
//! conservation, trousseau), fiche d'évaluation, réponse restaurée,
//! destruction d'une clé, restauration. Sans egui : testable de bout en bout
//! sur `exemples/`.
//!
//! Aucun contenu utilisateur n'est journalisé : les messages ne citent que des
//! noms de fichiers choisis par l'utilisateur et des balises.

use crate::ui::state::anonymize::AnalyzeJob;
use crate::ui::state::bilan::BilanState;
use crate::ui::state::images::FileMask;
use anon_core::{AssessSettings, Assessment, EntityKind, Goal, Session, Warning};
use anon_engine::{
    Analysis, AnonymizeOptions, Anonymized, Engine, EngineError, FicheMeta, FixUndo, InputFile,
    Progress, RestoreReport, render_markdown,
};
use anon_replace::SessionExt;
use anon_vault::keychain::{SecretStore, key_id};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Nombre maximal de suffixes essayés pour trouver un nom libre.
const MAX_NAME_ATTEMPTS: u32 = 9999;

/// Fichier d'exemple embarqué dans l'exécutable.
#[derive(Debug, Clone, Copy)]
pub struct ExampleFile {
    pub name: &'static str,
    pub bytes: &'static [u8],
    /// Document à anonymiser (chargé dans la liste) ; sinon notice ou réponse
    /// d'IA simulée, seulement copiée.
    pub document: bool,
}

macro_rules! example {
    ($name:literal, $document:literal) => {
        ExampleFile {
            name: $name,
            bytes: include_bytes!(concat!("../../../exemples/", $name)),
            document: $document,
        }
    };
}

/// Fichiers fictifs de `exemples/` (sans `attendu.json` ni `evaluation.json`,
/// réservés aux tests).
pub const EXAMPLES: &[ExampleFile] = &[
    example!("LISEZMOI.md", false),
    example!("brief.md", true),
    example!("carte-visite.png", true),
    example!("compte-rendu-commission.docx", true),
    example!("compte-rendu.odt", true),
    example!("contrat-prestation.docx", true),
    example!("courrier-arret-maladie.txt", true),
    example!("courrier-scanne.pdf", true),
    example!("export.csv", true),
    example!("facture.pdf", true),
    example!("fichier-clients.xlsx", true),
    example!("informe-es.txt", true),
    example!("notes.txt", true),
    example!("patients.xlsx", true),
    example!("planning.ods", true),
    example!("presentation-projet.pptx", true),
    example!("reponse-ia-exemple.md", false),
    example!("report-en.docx", true),
    example!("support.odp", true),
];

/// Fichiers de `exemples/` réservés aux tests : jamais copiés chez l'utilisateur.
pub const TEST_ONLY_EXAMPLES: [&str; 2] = ["attendu.json", "evaluation.json"];

/// Emplacement fixe des exemples, sous le dossier Documents : `Anonymiseur/Exemples`.
pub const EXAMPLES_SUBDIR: [&str; 2] = ["Anonymiseur", "Exemples"];

/// Nom du dossier d'exemples créé dans un dossier choisi (repli quand le
/// dossier Documents est introuvable).
pub const EXAMPLES_FALLBACK_DIR: &str = "Exemples Anonymiseur";

/// Dossier des exemples sous un dossier Documents.
pub fn examples_dir_in(documents: &Path) -> PathBuf {
    EXAMPLES_SUBDIR
        .iter()
        .fold(documents.to_path_buf(), |d, part| d.join(part))
}

/// Crée `parent/name`, ou `name_2`, `name_3`… si le nom est pris.
pub fn create_new_dir(parent: &Path, name: &str) -> std::io::Result<PathBuf> {
    for n in 1..=MAX_NAME_ATTEMPTS {
        let candidate = if n == 1 {
            name.to_owned()
        } else {
            format!("{name}_{n}")
        };
        let path = parent.join(candidate);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "aucun nom libre",
    ))
}

/// Exemples prêts : dossier, documents à charger, fichiers recréés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedExamples {
    pub dir: PathBuf,
    /// Documents d'exemple à anonymiser (ni la notice, ni la réponse d'IA simulée).
    pub documents: Vec<PathBuf>,
    /// Fichiers d'exemple absents, recréés à cet appel.
    pub created: usize,
}

/// Prépare les exemples dans `dir`, toujours le même : le dossier est créé
/// s'il manque, puis seuls les fichiers d'exemple absents sont écrits. Un
/// fichier existant n'est jamais remplacé (création exclusive), et seuls les
/// documents d'exemple sont renvoyés : jamais un fichier produit par l'outil.
pub fn prepare_examples(dir: &Path) -> std::io::Result<PreparedExamples> {
    std::fs::create_dir_all(dir)?;
    let mut documents = Vec::new();
    let mut created = 0;
    for e in EXAMPLES {
        let path = dir.join(e.name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(e.bytes)?;
                f.sync_all()?;
                created += 1;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
        if e.document && path.is_file() {
            documents.push(path);
        }
    }
    Ok(PreparedExamples {
        dir: dir.to_path_buf(),
        documents,
        created,
    })
}

/// Fichier refusé ou illisible, avec la raison en clair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub name: String,
    pub reason: String,
}

/// Première lettre en majuscule (les messages du moteur commencent en minuscule).
pub fn sentence(msg: &str) -> String {
    let mut chars = msg.chars();
    match chars.next() {
        Some(c) => {
            let mut s: String = c.to_uppercase().collect();
            s.push_str(chars.as_str());
            if !s.ends_with('.') {
                s.push('.');
            }
            s
        }
        None => String::new(),
    }
}

/// Nom de fichier affichable d'un chemin.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier sans nom".to_owned())
}

/// Extension en minuscules, sans point.
pub fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// Message de refus d'une extension non gérée (anciens formats Office,
/// raccourcis Google… : message du moteur).
pub fn unsupported_reason(path: &Path) -> String {
    sentence(
        &EngineError::Unsupported {
            extension: extension(path),
        }
        .to_string(),
    )
}

/// Lit les documents d'une analyse : fichiers du disque, puis textes collés
/// (déjà en mémoire).
pub fn read_documents(job: &AnalyzeJob) -> (Vec<InputFile>, Vec<Rejected>) {
    let (mut inputs, rejected) = read_inputs(&job.paths);
    inputs.extend(job.texts.iter().cloned());
    (inputs, rejected)
}

/// Message d'erreur d'ouverture d'une clé, dans le vocabulaire de l'interface.
pub fn vault_message(e: &anon_vault::VaultError) -> String {
    match e {
        anon_vault::VaultError::NotAVault => {
            "Ce fichier n'est pas une clé de restauration Anonymiseur.".to_owned()
        }
        other => other.to_string(),
    }
}

/// Lit les fichiers ; un fichier illisible est refusé sans bloquer les autres.
pub fn read_inputs(paths: &[PathBuf]) -> (Vec<InputFile>, Vec<Rejected>) {
    let mut ok = Vec::new();
    let mut rejected = Vec::new();
    for p in paths {
        let name = file_name(p);
        match std::fs::read(p) {
            Ok(bytes) => ok.push(InputFile::new(name, bytes)),
            Err(_) => rejected.push(Rejected {
                name,
                reason: "Fichier illisible : il a peut-être été déplacé, ou il est ouvert dans \
                         une autre application."
                    .to_owned(),
            }),
        }
    }
    (ok, rejected)
}

/// Écrit `bytes` dans `dir` sous `name`, ou `nom_2.ext`, `nom_3.ext`… si le
/// nom est pris. Ne remplace jamais un fichier existant (création exclusive).
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), Some(e.to_owned())),
        _ => (name.to_owned(), None),
    };
    for n in 1..=MAX_NAME_ATTEMPTS {
        let candidate = match (n, &ext) {
            (1, _) => name.to_owned(),
            (_, Some(e)) => format!("{stem}_{n}.{e}"),
            (_, None) => format!("{stem}_{n}"),
        };
        let path = dir.join(candidate);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.sync_all()?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "aucun nom libre",
    ))
}

/// Protection de la clé de restauration.
#[derive(Clone, PartialEq, Eq)]
pub enum Protection {
    /// Nouvelle clé chiffrée par ce mot de passe.
    Password(String),
    /// Aucune clé : restauration impossible (objectif « Anonymiser définitivement »).
    Irreversible,
}

impl std::fmt::Debug for Protection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protection::Password(_) => f.write_str("Password(…)"),
            Protection::Irreversible => f.write_str("Irreversible"),
        }
    }
}

/// Durées de conservation proposées pour une clé, en mois.
pub const RETENTION_CHOICES: [u32; 3] = [6, 12, 24];

/// Date et heure locales, pour nommer une clé ou une fiche.
pub fn local_now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// `JJ-MM-AAAA-HHhMM`.
fn stamp(at: &chrono::NaiveDateTime) -> String {
    at.format("%d-%m-%Y-%Hh%M").to_string()
}

/// Nom d'une clé de restauration : `dossier-JJ-MM-AAAA-HHhMM.anonmap`.
pub fn key_file_name(at: &chrono::NaiveDateTime) -> String {
    format!("dossier-{}.anonmap", stamp(at))
}

/// Nom proposé pour la fiche d'évaluation : `fiche-evaluation-JJ-MM-AAAA-HHhMM.md`.
/// Il ne dit rien du verdict : le mot « anonyme » n'y figure jamais (invariant 9).
pub fn fiche_file_name(at: &chrono::NaiveDateTime) -> String {
    format!("fiche-evaluation-{}.md", stamp(at))
}

/// Où et comment écrire la clé de restauration (objectif « Masquer pour l'IA »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPlan {
    /// Dossier des clés (par défaut `Documents/Anonymiseur/Clés`).
    pub dir: PathBuf,
    /// Nom du fichier (`dossier-JJ-MM-AAAA-HHhMM.anonmap`).
    pub name: String,
    /// Durée de conservation, en mois.
    pub keep_months: u32,
    /// Retenir le mot de passe dans le trousseau du système après le scellement.
    pub remember: bool,
}

impl KeyPlan {
    /// Clé dans `dir`, nommée à l'heure locale, conservée 12 mois, mot de
    /// passe non retenu.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            name: key_file_name(&local_now()),
            keep_months: anon_vault::retention::DEFAULT_RETENTION_MONTHS,
            remember: false,
        }
    }
}

/// Ce que l'envoi produit et où : protection, dossier des fichiers, clé,
/// réglages de l'évaluation (suffixe `_anonyme` ou `_masque_sans_cle`).
#[derive(Debug, Clone, PartialEq)]
pub struct SendPlan {
    pub protection: Protection,
    pub out_dir: PathBuf,
    /// Clé de restauration ; ignorée sans mot de passe.
    pub key: KeyPlan,
    pub settings: AssessSettings,
}

impl SendPlan {
    /// Plan d'envoi : objectif déduit de la protection, réglages par défaut.
    pub fn new(protection: Protection, out_dir: impl Into<PathBuf>, key: KeyPlan) -> Self {
        let goal = match protection {
            Protection::Password(_) => Goal::Mask,
            Protection::Irreversible => Goal::Anonymize,
        };
        Self {
            protection,
            out_dir: out_dir.into(),
            key,
            settings: AssessSettings {
                goal,
                ..AssessSettings::default()
            },
        }
    }
}

/// Résultat d'un fichier du lot.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Written {
        path: PathBuf,
        replacements: usize,
        warnings: Vec<Warning>,
    },
    /// Vérification finale : fichier non livré. Balises concernées et emplacements.
    Leak {
        tags: Vec<String>,
        parts: Vec<String>,
    },
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileOutcome {
    /// Nom du fichier d'origine.
    pub name: String,
    pub outcome: Outcome,
}

/// Bilan d'un lot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BatchReport {
    pub files: Vec<FileOutcome>,
    pub out_dir: PathBuf,
    /// Clé écrite (absente sans clé ou si rien n'a été livré).
    pub map_path: Option<PathBuf>,
    /// Identifiant de la clé écrite, pour le trousseau (il change à chaque scellement).
    pub key_id: Option<String>,
    /// Date jusqu'à laquelle conserver la clé (`AAAA-MM-JJ`).
    pub keep_until: Option<String>,
    /// Mot de passe retenu dans le trousseau : `Some(Ok)` s'il l'est,
    /// `Some(Err(raison))` en cas d'échec, `None` s'il n'était pas demandé.
    pub remembered: Option<Result<(), String>>,
    /// Erreur bloquante : rien n'a été écrit.
    pub fatal: Option<String>,
    pub cancelled: bool,
}

impl BatchReport {
    pub fn delivered(&self) -> usize {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, Outcome::Written { .. }))
            .count()
    }
}

/// Ajoute à l'analyse les zones tracées à la main : l'évaluation (visages
/// couverts) et l'anonymisation les voient comme les zones des corrections.
/// Renvoie leurs identifiants, pour les retirer ([`remove_user_masks`]).
pub fn add_user_masks(analysis: &mut Analysis, masks: &[FileMask]) -> Vec<usize> {
    masks
        .iter()
        .filter_map(|m| {
            let file = analysis.file_index(&m.file)?;
            analysis.add_mask(file, &m.mask.media, m.mask.rect)
        })
        .collect()
}

/// Retire les zones ajoutées par [`add_user_masks`].
pub fn remove_user_masks(analysis: &mut Analysis, ids: &[usize]) {
    for id in ids {
        analysis.remove_mask(*id);
    }
}

/// Nom d'un tableau évalué : fichier et feuille.
#[derive(Clone, PartialEq, Eq)]
pub struct TableName {
    pub file: String,
    pub sheet: String,
    /// En-têtes des colonnes (affichés à l'écran seulement).
    pub headers: Vec<String>,
}

impl std::fmt::Debug for TableName {
    /// Ni la feuille ni les en-têtes (contenu du document).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableName")
            .field("headers", &self.headers.len())
            .finish_non_exhaustive()
    }
}

/// Évaluation du dossier et noms des tableaux évalués, dans l'ordre de
/// l'évaluation (`SubjectRef::TableGroup::table`).
#[derive(Debug, Clone, PartialEq)]
pub struct FolderAssessment {
    pub assessment: Assessment,
    pub tables: Vec<TableName>,
}

/// Évalue le dossier : `analysis` (une copie, le plus souvent) reçoit les
/// zones tracées à la main avant l'évaluation. Long sur un gros dossier : à
/// appeler sur le fil de travail.
pub fn assess_folder(
    engine: &Engine,
    analysis: &mut Analysis,
    masks: &[FileMask],
    settings: &AssessSettings,
) -> FolderAssessment {
    let ids = add_user_masks(analysis, masks);
    let input = engine.assess_input(analysis);
    let tables = input
        .tables
        .iter()
        .map(|t| TableName {
            file: analysis
                .files
                .get(t.file)
                .map(|f| f.name.clone())
                .unwrap_or_default(),
            sheet: t.sheet.clone(),
            headers: t.headers.clone(),
        })
        .collect();
    let assessment = engine.assess_from(&input, settings);
    remove_user_masks(analysis, &ids);
    FolderAssessment { assessment, tables }
}

/// « Tout corriger » sur `analysis`, zones tracées à la main comprises, puis
/// l'évaluation du résultat. Renvoie de quoi annuler (une seule annulation).
pub fn fix_all_folder(
    engine: &Engine,
    analysis: &mut Analysis,
    masks: &[FileMask],
    settings: &AssessSettings,
) -> (FixUndo, FolderAssessment) {
    let ids = add_user_masks(analysis, masks);
    let undo = engine.fix_all(analysis, settings);
    remove_user_masks(analysis, &ids);
    let assessed = assess_folder(engine, analysis, masks, settings);
    (undo, assessed)
}

/// Masque un lot : sélection enregistrée dans la session, chaque fichier
/// traité en mémoire (vérification finale bloquante), puis la clé est scellée et
/// écrite **avant** les fichiers : un fichier masqué n'existe jamais sans sa
/// clé. En cas d'annulation, ce qui est déjà traité est livré avec sa clé.
///
/// L'objectif de la session suit la protection : sans clé, « Anonymiser
/// définitivement » (`Session::goal`, `Session::irreversible`), et le nom des
/// fichiers suit l'évaluation faite avec `plan.settings` (`_anonyme` ou
/// `_masque_sans_cle`). Avec une clé : dates de conservation dans la session,
/// clé écrite dans `plan.key.dir`, mot de passe retenu dans `keychain` si
/// `plan.key.remember`.
///
/// `masks` : zones tracées à la main, ajoutées à une copie de l'analyse.
#[allow(clippy::too_many_arguments)]
pub fn anonymize_batch(
    engine: &Engine,
    inputs: &[InputFile],
    analysis: &Analysis,
    never: &[String],
    masks: &[FileMask],
    session: &mut Session,
    plan: &SendPlan,
    keychain: Option<&dyn SecretStore>,
    progress: &dyn Progress,
) -> BatchReport {
    anonymize_batch_tracked(
        engine,
        inputs,
        analysis,
        never,
        masks,
        session,
        plan,
        keychain,
        progress,
        &|_| {},
    )
}

/// Comme [`anonymize_batch`], en signalant chaque fichier en échec
/// (`failed(indice)`) dès qu'il est connu : la liste d'avancement l'affiche.
#[allow(clippy::too_many_arguments)]
pub fn anonymize_batch_tracked(
    engine: &Engine,
    inputs: &[InputFile],
    analysis: &Analysis,
    never: &[String],
    masks: &[FileMask],
    session: &mut Session,
    plan: &SendPlan,
    keychain: Option<&dyn SecretStore>,
    progress: &dyn Progress,
    failed: &dyn Fn(usize),
) -> BatchReport {
    session.irreversible = matches!(plan.protection, Protection::Irreversible);
    session.goal = if session.irreversible {
        Goal::Anonymize
    } else {
        Goal::Mask
    };
    let mut analysis = analysis.clone();
    add_user_masks(&mut analysis, masks);
    engine.apply_selection(&analysis, session);
    for n in never {
        session.add_never(n);
    }
    let mut report = BatchReport {
        out_dir: plan.out_dir.clone(),
        ..BatchReport::default()
    };
    let total = inputs.len();
    let mut ready: Vec<(usize, Anonymized)> = Vec::new();
    // Le dossier de sortie (`Anonymisés` par défaut) n'est créé qu'à l'écriture.
    for (i, file) in inputs.iter().enumerate() {
        if report.cancelled || progress.cancelled() {
            report.cancelled = true;
            report.files.push(FileOutcome {
                name: file.name.clone(),
                outcome: Outcome::Cancelled,
            });
            continue;
        }
        progress.step(i, total, &file.name);
        let analysis_error = analysis
            .files
            .iter()
            .find(|f| f.name == file.name)
            .and_then(|f| f.error.as_ref())
            .map(|e| sentence(&e.to_string()));
        let outcome = match analysis_error {
            Some(msg) => Outcome::Failed(msg),
            None => {
                match engine.anonymize_with(
                    file,
                    &analysis,
                    session,
                    &AnonymizeOptions::default(),
                    &plan.settings,
                ) {
                    Ok(out) => {
                        ready.push((report.files.len(), out));
                        // Remplacé après écriture.
                        Outcome::Cancelled
                    }
                    Err(EngineError::Leak { leaks }) => {
                        let mut tags: Vec<String> =
                            leaks.iter().map(|l| l.tag.to_string()).collect();
                        tags.dedup();
                        let mut parts: Vec<String> = leaks.iter().map(|l| l.part.clone()).collect();
                        parts.dedup();
                        Outcome::Leak { tags, parts }
                    }
                    Err(e) => Outcome::Failed(sentence(&e.to_string())),
                }
            }
        };
        if matches!(outcome, Outcome::Leak { .. } | Outcome::Failed(_)) {
            failed(i);
        }
        report.files.push(FileOutcome {
            name: file.name.clone(),
            outcome,
        });
    }
    progress.step(total, total, "");
    if ready.is_empty() {
        return report;
    }
    let out_dir = plan.out_dir.as_path();
    if std::fs::create_dir_all(out_dir).is_err() {
        report.fatal = Some(
            "Impossible de créer le dossier de sortie : aucun fichier n'a été écrit. Choisissez \
             un autre dossier de sortie."
                .to_owned(),
        );
        mark_unwritten(&mut report, &ready);
        return report;
    }

    // Clé d'abord.
    if let Protection::Password(pw) = &plan.protection {
        let today = anon_vault::retention::today();
        session.keep_until = anon_vault::retention::add_months(&today, plan.key.keep_months);
        session.created_on = Some(today);
        let sealed = match anon_vault::seal(session, pw) {
            Ok(b) => b,
            Err(e) => {
                report.fatal = Some(format!(
                    "Clé de restauration non chiffrée ({e}) : aucun fichier n'a été écrit."
                ));
                mark_unwritten(&mut report, &ready);
                return report;
            }
        };
        let written = std::fs::create_dir_all(&plan.key.dir)
            .and_then(|()| write_new(&plan.key.dir, &plan.key.name, &sealed));
        match written {
            Ok(p) => report.map_path = Some(p),
            Err(_) => {
                report.fatal = Some(
                    "Impossible d'écrire la clé de restauration dans son dossier : aucun \
                     fichier n'a été écrit. Choisissez un autre dossier pour la clé."
                        .to_owned(),
                );
                mark_unwritten(&mut report, &ready);
                return report;
            }
        }
        report.key_id = key_id(&sealed);
        report.keep_until = session.keep_until.clone();
        if plan.key.remember {
            // L'identifiant change à chaque scellement : retenir celui-ci.
            report.remembered = Some(match (keychain, &report.key_id) {
                (Some(store), Some(id)) => store.remember(id, pw).map_err(|e| e.to_string()),
                _ => Err(anon_vault::VaultError::KeychainUnavailable.to_string()),
            });
        }
    }
    for (slot, out) in ready {
        let outcome = match write_new(out_dir, &out.name, &out.bytes) {
            Ok(path) => Outcome::Written {
                path,
                replacements: out.report.replacements,
                warnings: out.report.warnings,
            },
            Err(_) => Outcome::Failed("Écriture impossible dans le dossier de sortie.".to_owned()),
        };
        if let Some(f) = report.files.get_mut(slot) {
            f.outcome = outcome;
        }
    }
    report
}

fn mark_unwritten(report: &mut BatchReport, ready: &[(usize, Anonymized)]) {
    for (slot, _) in ready {
        if let Some(f) = report.files.get_mut(*slot) {
            f.outcome = Outcome::Failed("Non écrit : la clé n'a pas pu être créée.".to_owned());
        }
    }
}

/// Fiche d'évaluation dans le format du chemin choisi : PDF pour `.pdf`,
/// Markdown sinon.
pub fn fiche_bytes(markdown: &str, path: &Path) -> Result<Vec<u8>, String> {
    if extension(path) == "pdf" {
        anon_pdf::render_fiche_pdf(markdown).map_err(|e| sentence(&e.to_string()))
    } else {
        Ok(markdown.as_bytes().to_vec())
    }
}

/// Réponse restaurée dans le format du chemin choisi : document Word pour
/// `.docx`, texte (Markdown) sinon.
pub fn restored_bytes(text: &str, path: &Path) -> Result<Vec<u8>, String> {
    if extension(path) == "docx" {
        anon_office::text_to_docx(text).map_err(|e| sentence(&e.to_string()))
    } else {
        Ok(text.as_bytes().to_vec())
    }
}

/// Écrit un fichier choisi par l'utilisateur (dialogue « Enregistrer sous »,
/// qui a déjà demandé s'il fallait remplacer un fichier existant).
pub fn save_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|_| "Écriture impossible à cet endroit.".to_owned())
}

/// Bilan de la destruction d'une clé.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Destroyed {
    /// Le fichier `.anonmap` a été supprimé (ou n'existait plus).
    pub file: bool,
    /// Le mot de passe a été oublié par le trousseau (ou n'y était pas).
    pub keychain: bool,
}

/// Détruit une clé : supprime le fichier puis oublie son mot de passe dans le
/// trousseau. Un fichier déjà absent compte comme supprimé ; un fichier qui
/// ne peut pas l'être laisse tout en place.
pub fn destroy_key(
    path: Option<&Path>,
    key_id: Option<&str>,
    keychain: Option<&dyn SecretStore>,
) -> Destroyed {
    let file = match path {
        Some(p) => match std::fs::remove_file(p) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        },
        None => true,
    };
    if !file {
        return Destroyed::default();
    }
    let keychain = match (keychain, key_id) {
        (Some(store), Some(id)) => store.forget(id).is_ok(),
        _ => true,
    };
    Destroyed { file, keychain }
}

/// Fiche d'évaluation en Markdown (`anon_engine::fiche`) : techniques
/// appliquées (zones tracées comprises), résultat de l'évaluation, contexte,
/// champs saisis. Seulement des balises, des catégories et des nombres.
pub fn fiche_markdown(
    analysis: &Analysis,
    masks: &[FileMask],
    assessment: &Assessment,
    bilan: &BilanState,
    date: &str,
) -> String {
    let mut a = analysis.clone();
    add_user_masks(&mut a, masks);
    let fields = &bilan.fiche;
    let mut meta = FicheMeta::new(&a, assessment, date, fields.with_file_names);
    meta.answers_justification = bilan.answers_justification.clone();
    meta.legal_basis = fields.legal_basis.clone();
    meta.purpose = fields.purpose.clone();
    meta.accepted_risks = fields.accepted_risks.clone();
    meta.risks_justification = fields.risks_justification.clone();
    render_markdown(assessment, &meta)
}

/// Résultat de la restauration d'un fichier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub name: String,
    pub result: Result<(PathBuf, RestoreReport), String>,
}

/// Restaure des fichiers et les écrit en `<nom>_restaure.<ext>`, sans écraser.
pub fn restore_batch(
    engine: &Engine,
    inputs: &[InputFile],
    session: &Session,
    out_dir: &Path,
    progress: &dyn Progress,
) -> Vec<RestoreOutcome> {
    let total = inputs.len();
    let mut out = Vec::with_capacity(total);
    for (i, file) in inputs.iter().enumerate() {
        if progress.cancelled() {
            out.push(RestoreOutcome {
                name: file.name.clone(),
                result: Err("Annulé.".to_owned()),
            });
            continue;
        }
        progress.step(i, total, &file.name);
        let result = match engine.restore(file, session) {
            Ok(r) => match write_new(out_dir, &r.name, &r.bytes) {
                Ok(p) => Ok((p, r.report)),
                Err(_) => Err("Écriture impossible dans le dossier de sortie.".to_owned()),
            },
            Err(e) => Err(sentence(&e.to_string())),
        };
        out.push(RestoreOutcome {
            name: file.name.clone(),
            result,
        });
    }
    progress.step(total, total, "");
    out
}

/// Balises non retrouvées d'un bilan, en texte.
pub fn unused_tags(report: &RestoreReport) -> Vec<String> {
    report.unused.iter().map(|t| t.to_string()).collect()
}

/// Texte masqué des documents livrés, précédé de la consigne pour l'IA :
/// prêt à coller dans une conversation. Les documents sont numérotés, sans
/// leur nom (il peut désigner une personne). Chaque texte passe la
/// vérification finale (`Engine::export_text`) ; un document sans texte ou
/// bloqué est omis. Renvoie le texte et le nombre de documents inclus.
pub fn text_for_ai(
    engine: &Engine,
    inputs: &[InputFile],
    report: &BatchReport,
    session: &Session,
) -> (String, usize) {
    let mut out = anon_engine::ai_instruction().trim_end().to_owned();
    let mut included = 0;
    for f in &report.files {
        if !matches!(f.outcome, Outcome::Written { .. }) {
            continue;
        }
        let Some(input) = inputs.iter().find(|i| i.name == f.name) else {
            continue;
        };
        let Ok(text) = engine.export_text(input, session) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        included += 1;
        out.push_str(&format!("\n\n--- Document {included} ---\n\n"));
        out.push_str(text.trim_end());
    }
    out.push('\n');
    (out, included)
}

/// Nombre de types distincts (pour le résumé d'une clé).
pub fn kinds_in(session: &Session) -> usize {
    let mut kinds: Vec<EntityKind> = session.entities.iter().map(|e| e.tag.kind).collect();
    kinds.sort();
    kinds.dedup();
    kinds.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("anonymiseur-service-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    const LETTRE: &str = "Madame Hélène Kaloï a écrit à helene.kaloi@example.com.";

    /// Analyse d'une lettre, toutes valeurs cochées.
    fn lettre(engine: &Engine) -> (Vec<InputFile>, Analysis) {
        let inputs = vec![InputFile::new("lettre.txt", LETTRE.as_bytes().to_vec())];
        let analysis = engine.analyze(
            &inputs,
            &anon_engine::AnalyzeOptions::default(),
            &Session::new(),
            &anon_engine::NoProgress,
        );
        (inputs, analysis)
    }

    #[test]
    fn the_session_goal_follows_the_protection() {
        let engine = Engine::standard();
        let (inputs, analysis) = lettre(&engine);
        for (protection, goal, map) in [
            (Protection::Irreversible, Goal::Anonymize, false),
            (
                Protection::Password("Cheval-Agrafe-Batterie-47".into()),
                Goal::Mask,
                true,
            ),
        ] {
            let d = temp_dir(if map {
                "objectif-cle"
            } else {
                "objectif-sans-cle"
            });
            let mut session = Session::new();
            session.goal = Goal::Anonymize;
            let plan = SendPlan::new(protection.clone(), &d, KeyPlan::new(d.join("Clés")));
            let report = anonymize_batch(
                &engine,
                &inputs,
                &analysis,
                &[],
                &[],
                &mut session,
                &plan,
                None,
                &anon_engine::NoProgress,
            );
            assert_eq!(report.delivered(), 1);
            assert_eq!(session.goal, goal);
            assert_eq!(session.irreversible, goal == Goal::Anonymize);
            assert_eq!(report.map_path.is_some(), map, "aucune clé sans retour");
            if let (Some(path), Protection::Password(pw)) = (&report.map_path, &protection) {
                let bytes = std::fs::read(path).unwrap_or_default();
                let reopened = anon_vault::open(&bytes, pw);
                assert_eq!(reopened.map(|s| s.goal), Ok(Goal::Mask));
            }
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn the_key_goes_to_its_own_folder_with_its_dates_and_name() {
        let engine = Engine::standard();
        let (inputs, analysis) = lettre(&engine);
        let d = temp_dir("dossier-des-cles");
        let keys = d.join("Anonymiseur").join("Clés");
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 16)
            .and_then(|d| d.and_hms_opt(14, 5, 0))
            .unwrap_or_default();
        let key = KeyPlan {
            dir: keys.clone(),
            name: key_file_name(&at),
            keep_months: 6,
            remember: true,
        };
        let pw = "Cheval-Agrafe-Batterie-47";
        let plan = SendPlan::new(Protection::Password(pw.into()), d.join("Anonymisés"), key);
        let store = anon_vault::keychain::MemoryStore::new();
        let mut session = Session::new();
        let report = anonymize_batch(
            &engine,
            &inputs,
            &analysis,
            &[],
            &[],
            &mut session,
            &plan,
            Some(&store),
            &anon_engine::NoProgress,
        );
        // Dossier des clés créé au besoin, distinct de celui des fichiers.
        assert_eq!(
            report.map_path,
            Some(keys.join("dossier-16-11-2026-14h05.anonmap"))
        );
        assert!(
            d.join("Anonymisés").join("lettre_masque.txt").is_file(),
            "fichier produit à part"
        );
        // Dates de conservation : aujourd'hui, plus 6 mois.
        let today = anon_vault::retention::today();
        assert_eq!(session.created_on.as_deref(), Some(today.as_str()));
        assert_eq!(
            session.keep_until,
            anon_vault::retention::add_months(&today, 6)
        );
        assert_eq!(report.keep_until, session.keep_until);
        // Mot de passe retenu sous l'identifiant de cette clé.
        assert_eq!(report.remembered, Some(Ok(())));
        let map = report.map_path.clone().unwrap_or_default();
        let bytes = std::fs::read(&map).unwrap_or_default();
        let id = key_id(&bytes).unwrap_or_default();
        assert_eq!(report.key_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            store.recall(&id).ok().flatten().map(|p| p.to_string()),
            Some(pw.to_owned())
        );
        let reopened = anon_vault::open(&bytes, pw).unwrap_or_default();
        assert_eq!(reopened.keep_until, session.keep_until);
        // Détruire : fichier supprimé, mot de passe oublié.
        let destroyed = destroy_key(Some(&map), Some(&id), Some(&store));
        assert_eq!(
            destroyed,
            Destroyed {
                file: true,
                keychain: true
            }
        );
        assert!(!map.exists());
        assert!(store.recall(&id).ok().flatten().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn without_keychain_the_password_is_not_remembered() {
        let engine = Engine::standard();
        let (inputs, analysis) = lettre(&engine);
        let d = temp_dir("sans-trousseau");
        let mut key = KeyPlan::new(d.join("Clés"));
        key.remember = true;
        let plan = SendPlan::new(
            Protection::Password("Cheval-Agrafe-Batterie-47".into()),
            &d,
            key,
        );
        let report = anonymize_batch(
            &engine,
            &inputs,
            &analysis,
            &[],
            &[],
            &mut Session::new(),
            &plan,
            None,
            &anon_engine::NoProgress,
        );
        assert_eq!(report.delivered(), 1, "l'envoi n'est pas bloqué");
        assert!(matches!(&report.remembered, Some(Err(why)) if why.contains("rousseau")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_of_keys_and_fiches() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 1, 5)
            .and_then(|d| d.and_hms_opt(9, 7, 0))
            .unwrap_or_default();
        assert_eq!(key_file_name(&at), "dossier-05-01-2026-09h07.anonmap");
        assert_eq!(fiche_file_name(&at), "fiche-evaluation-05-01-2026-09h07.md");
        assert!(!fiche_file_name(&at).contains("anonyme"));
        assert!(crate::ui::state::files::is_tool_output(Path::new(
            &key_file_name(&at)
        )));
        assert_eq!(RETENTION_CHOICES, [6, 12, 24]);
    }

    #[test]
    fn fiche_and_response_in_the_chosen_format() {
        let md = "# Fiche d'évaluation\n\nAucun échec.\n";
        assert_eq!(
            fiche_bytes(md, Path::new("fiche.md")),
            Ok(md.as_bytes().to_vec())
        );
        let pdf = fiche_bytes(md, Path::new("fiche.PDF")).unwrap_or_default();
        assert!(pdf.starts_with(b"%PDF"));
        let docx = restored_bytes("Bonjour Hélène.", Path::new("reponse.docx")).unwrap_or_default();
        assert!(docx.starts_with(b"PK"), "paquet zip");
        assert_eq!(
            restored_bytes("Bonjour.", Path::new("reponse.md")),
            Ok(b"Bonjour.".to_vec())
        );
    }

    #[test]
    fn destroying_a_missing_key_is_not_an_error() {
        let d = temp_dir("cle-absente");
        let gone = destroy_key(Some(&d.join("absente.anonmap")), None, None);
        assert!(gone.file && gone.keychain);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn text_for_ai_starts_with_the_instruction_and_hides_values() {
        let engine = Engine::standard();
        let (inputs, analysis) = lettre(&engine);
        let d = temp_dir("texte-pour-ia");
        let mut session = Session::new();
        let plan = SendPlan::new(Protection::Irreversible, &d, KeyPlan::new(d.join("Clés")));
        let report = anonymize_batch(
            &engine,
            &inputs,
            &analysis,
            &[],
            &[],
            &mut session,
            &plan,
            None,
            &anon_engine::NoProgress,
        );
        let (text, n) = text_for_ai(&engine, &inputs, &report, &session);
        assert_eq!(n, 1);
        assert!(text.starts_with(anon_engine::ai_instruction().trim_end()));
        assert!(text.contains("--- Document 1 ---"));
        assert!(text.contains("[EMAIL_1]"), "{text}");
        assert!(!text.contains("kaloi@example.com"));
        assert!(!text.contains("lettre.txt"), "pas de nom de fichier");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn documents_include_pasted_texts() {
        let d = temp_dir("documents");
        let file = d.join("a.txt");
        let _ = std::fs::write(&file, "Bonjour");
        let job = AnalyzeJob {
            paths: vec![file, d.join("absent.txt")],
            texts: vec![InputFile::new("texte-colle-1.txt", b"Salut".to_vec())],
        };
        let (inputs, rejected) = read_documents(&job);
        let names: Vec<&str> = inputs.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "texte-colle-1.txt"]);
        assert_eq!(rejected.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vault_messages_use_the_key_vocabulary() {
        let m = vault_message(&anon_vault::VaultError::NotAVault);
        assert!(m.contains("clé de restauration"), "{m}");
        assert!(vault_message(&anon_vault::VaultError::WrongPassword).starts_with("Mot de passe"));
    }

    #[test]
    fn write_new_never_overwrites() {
        let d = temp_dir("ecrasement");
        let a = write_new(&d, "note_anonymise.txt", b"un");
        let b = write_new(&d, "note_anonymise.txt", b"deux");
        let c = write_new(&d, "sans", b"trois");
        let c2 = write_new(&d, "sans", b"quatre");
        assert_eq!(a.ok(), Some(d.join("note_anonymise.txt")));
        assert_eq!(b.ok(), Some(d.join("note_anonymise_2.txt")));
        assert_eq!(c.ok(), Some(d.join("sans")));
        assert_eq!(c2.ok(), Some(d.join("sans_2")));
        assert_eq!(
            std::fs::read(d.join("note_anonymise.txt")).ok(),
            Some(b"un".to_vec())
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn new_dirs_are_never_reused() {
        let d = temp_dir("dossiers");
        let a = create_new_dir(&d, "Exemples");
        let b = create_new_dir(&d, "Exemples");
        assert_eq!(a.ok(), Some(d.join("Exemples")));
        assert_eq!(b.ok(), Some(d.join("Exemples_2")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn examples_are_reused_without_overwriting() {
        let d = temp_dir("exemples").join("Anonymiseur").join("Exemples");
        let first = prepare_examples(&d);
        assert!(first.is_ok());
        let Ok(first) = first else { return };
        assert_eq!(first.created, EXAMPLES.len());
        // Un fichier modifié par l'utilisateur et un fichier supprimé.
        let _ = std::fs::write(d.join("notes.txt"), "à garder");
        let _ = std::fs::remove_file(d.join("brief.md"));
        let _ = std::fs::write(d.join("notes_anonymise.txt"), "sortie");
        let again = prepare_examples(&d);
        assert!(again.is_ok());
        let Ok(again) = again else { return };
        assert_eq!(again.dir, d, "même dossier");
        assert_eq!(again.created, 1, "seul le fichier supprimé est recréé");
        assert_eq!(
            std::fs::read_to_string(d.join("notes.txt")).ok().as_deref(),
            Some("à garder")
        );
        assert_eq!(again.documents, first.documents);
        assert!(
            again
                .documents
                .iter()
                .all(|p| !crate::ui::state::files::is_tool_output(p))
        );
        let _ = std::fs::remove_dir_all(temp_dir("exemples"));
    }

    #[test]
    fn examples_dir_is_under_documents() {
        assert_eq!(
            examples_dir_in(Path::new("C:/Users/x/Documents")),
            PathBuf::from("C:/Users/x/Documents/Anonymiseur/Exemples")
        );
    }

    #[test]
    fn masks_go_to_their_file() {
        let engine = Engine::standard();
        let inputs = vec![
            InputFile::new("a.txt", b"Bonjour".to_vec()),
            InputFile::new("b.txt", b"Salut".to_vec()),
        ];
        let mut analysis = engine.analyze(
            &inputs,
            &anon_engine::AnalyzeOptions::default(),
            &Session::new(),
            &anon_engine::NoProgress,
        );
        let mut m = crate::ui::state::images::ManualMasks::default();
        let r = anon_core::PixelRect {
            x: 0,
            y: 0,
            w: 5,
            h: 5,
        };
        m.add("a.txt", "image", r);
        m.add("b.txt", "image", r);
        m.add("c.png", "image", r);
        let ids = add_user_masks(&mut analysis, &m.masks);
        assert_eq!(ids.len(), 2, "zone d'un fichier absent ignorée");
        let files: Vec<usize> = analysis.masks().iter().map(|x| x.file).collect();
        assert_eq!(files, [0, 1]);
        remove_user_masks(&mut analysis, &ids);
        assert!(analysis.masks().is_empty());
    }

    #[test]
    fn sentences() {
        assert_eq!(sentence("le fichier est vide"), "Le fichier est vide.");
        assert_eq!(sentence("éditer."), "Éditer.");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn legacy_and_google_reasons() {
        let doc = unsupported_reason(Path::new("C:/x/Ancien.DOC"));
        assert!(doc.starts_with("Ancien format Office"), "{doc}");
        let g = unsupported_reason(Path::new("Budget.gsheet"));
        assert!(g.contains("Télécharger"), "{g}");
    }

    #[test]
    fn unreadable_file_is_rejected_alone() {
        let d = temp_dir("lecture");
        let ok = d.join("a.txt");
        let _ = std::fs::write(&ok, "Bonjour");
        let (inputs, rejected) = read_inputs(&[ok, d.join("absent.txt")]);
        assert_eq!(inputs.len(), 1);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].name, "absent.txt");
        let _ = std::fs::remove_dir_all(&d);
    }
}
