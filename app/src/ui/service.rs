//! Opérations sur les fichiers, appelées depuis le fil de travail : lecture,
//! anonymisation par lot, écriture sans écrasement, scellement de la table,
//! restauration. Sans egui : testable de bout en bout sur `exemples/`.
//!
//! Aucun contenu utilisateur n'est journalisé : les messages ne citent que des
//! noms de fichiers choisis par l'utilisateur et des balises.

use crate::ui::state::images::FileMask;
use anon_core::{EntityKind, Session, Warning};
use anon_engine::{
    Analysis, AnonymizeOptions, Anonymized, Engine, EngineError, InputFile, Progress,
    RestoreReport, RiskReport,
};
use anon_replace::SessionExt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Nombre maximal de suffixes essayés pour trouver un nom libre.
const MAX_NAME_ATTEMPTS: u32 = 9999;
/// Nom de base de la table de correspondance écrite à côté des fichiers.
pub const MAP_BASENAME: &str = "table_anonymiseur.anonmap";

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

/// Fichiers fictifs de `exemples/` (sans `attendu.json`, réservé aux tests).
pub const EXAMPLES: &[ExampleFile] = &[
    example!("LISEZMOI.md", false),
    example!("brief.md", true),
    example!("carte-visite.png", true),
    example!("compte-rendu.odt", true),
    example!("contrat-prestation.docx", true),
    example!("courrier-scanne.pdf", true),
    example!("export.csv", true),
    example!("facture.pdf", true),
    example!("fichier-clients.xlsx", true),
    example!("informe-es.txt", true),
    example!("notes.txt", true),
    example!("planning.ods", true),
    example!("presentation-projet.pptx", true),
    example!("reponse-ia-exemple.md", false),
    example!("report-en.docx", true),
    example!("support.odp", true),
];

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

/// Protection de la table de correspondance.
#[derive(Clone, PartialEq, Eq)]
pub enum Protection {
    /// Nouvelle table chiffrée par ce mot de passe.
    Password(String),
    /// Aucune table : restauration impossible.
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

/// Résultat d'un fichier du lot.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Written {
        path: PathBuf,
        replacements: usize,
        warnings: Vec<Warning>,
        risk: RiskReport,
    },
    /// Contrôle de fuite : fichier non livré. Balises concernées et emplacements.
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
    /// Table écrite (absente en mode irréversible ou si rien n'a été livré).
    pub map_path: Option<PathBuf>,
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

/// Anonymise un lot : sélection enregistrée dans la session, chaque fichier
/// traité en mémoire (contrôle de fuite bloquant), puis la table est scellée et
/// écrite **avant** les fichiers : un fichier anonymisé n'existe jamais sans sa
/// table. En cas d'annulation, ce qui est déjà traité est livré avec sa table.
///
/// `masks` : zones tracées à la main, transmises au fichier qu'elles concernent.
#[allow(clippy::too_many_arguments)]
pub fn anonymize_batch(
    engine: &Engine,
    inputs: &[InputFile],
    analysis: &Analysis,
    never: &[String],
    masks: &[FileMask],
    session: &mut Session,
    protection: &Protection,
    out_dir: &Path,
    progress: &dyn Progress,
) -> BatchReport {
    anonymize_batch_tracked(
        engine,
        inputs,
        analysis,
        never,
        masks,
        session,
        protection,
        out_dir,
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
    protection: &Protection,
    out_dir: &Path,
    progress: &dyn Progress,
    failed: &dyn Fn(usize),
) -> BatchReport {
    session.irreversible = matches!(protection, Protection::Irreversible);
    engine.apply_selection(analysis, session);
    for n in never {
        session.add_never(n);
    }
    let mut report = BatchReport {
        out_dir: out_dir.to_path_buf(),
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
                match engine.anonymize(file, analysis, session, &options_for(masks, &file.name)) {
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
    if std::fs::create_dir_all(out_dir).is_err() {
        report.fatal = Some(
            "Impossible de créer le dossier de sortie : aucun fichier n'a été écrit. Choisissez \
             un autre dossier de sortie."
                .to_owned(),
        );
        mark_unwritten(&mut report, &ready);
        return report;
    }

    // Table d'abord.
    if let Protection::Password(pw) = protection {
        let sealed = match anon_vault::seal(session, pw) {
            Ok(b) => b,
            Err(e) => {
                report.fatal = Some(format!(
                    "Table non chiffrée ({e}) : aucun fichier n'a été écrit."
                ));
                mark_unwritten(&mut report, &ready);
                return report;
            }
        };
        match write_new(out_dir, MAP_BASENAME, &sealed) {
            Ok(p) => report.map_path = Some(p),
            Err(_) => {
                report.fatal = Some(
                    "Impossible d'écrire la table de correspondance dans ce dossier : aucun \
                     fichier n'a été écrit. Choisissez un autre dossier de sortie."
                        .to_owned(),
                );
                mark_unwritten(&mut report, &ready);
                return report;
            }
        }
    }
    for (slot, out) in ready {
        let outcome = match write_new(out_dir, &out.name, &out.bytes) {
            Ok(path) => Outcome::Written {
                path,
                replacements: out.report.replacements,
                warnings: out.report.warnings,
                risk: out.report.risk,
            },
            Err(_) => Outcome::Failed("Écriture impossible dans le dossier de sortie.".to_owned()),
        };
        if let Some(f) = report.files.get_mut(slot) {
            f.outcome = outcome;
        }
    }
    report
}

fn options_for(masks: &[FileMask], file: &str) -> AnonymizeOptions {
    AnonymizeOptions {
        manual_masks: masks
            .iter()
            .filter(|m| m.file == file)
            .map(|m| m.mask.clone())
            .collect(),
    }
}

fn mark_unwritten(report: &mut BatchReport, ready: &[(usize, Anonymized)]) {
    for (slot, _) in ready {
        if let Some(f) = report.files.get_mut(*slot) {
            f.outcome = Outcome::Failed("Non écrit : la table n'a pas pu être créée.".to_owned());
        }
    }
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

/// Nombre de types distincts (pour le résumé d'une table).
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
        let mut m = crate::ui::state::images::ManualMasks::default();
        let r = anon_core::PixelRect {
            x: 0,
            y: 0,
            w: 5,
            h: 5,
        };
        m.add("a.png", "image", r);
        m.add("b.png", "image", r);
        assert_eq!(options_for(&m.masks, "a.png").manual_masks.len(), 1);
        assert!(options_for(&m.masks, "c.png").manual_masks.is_empty());
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
