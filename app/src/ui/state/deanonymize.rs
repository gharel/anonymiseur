//! État de l'onglet « Désanonymiser » : table `.anonmap` + mot de passe, puis
//! fichiers ou texte collé, puis résultat et rapport.

use super::files::FileEntry;
use crate::ui::service::RestoreOutcome;
use anon_core::Session;
use anon_engine::RestoreReport;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    File,
    Paste,
}

/// Table de correspondance.
#[derive(Clone, PartialEq)]
pub enum MapStatus {
    None,
    /// Fichier choisi, pas encore ouvert.
    Locked {
        path: PathBuf,
        name: String,
    },
    Unlocked {
        name: String,
        entries: usize,
        session: Session,
    },
}

impl std::fmt::Debug for MapStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MapStatus::None => "None",
            MapStatus::Locked { .. } => "Locked",
            MapStatus::Unlocked { .. } => "Unlocked",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockError {
    NoMap,
    EmptyPassword,
    NotAMap,
}

impl UnlockError {
    pub fn message(self) -> &'static str {
        match self {
            UnlockError::NoMap => "Choisissez d'abord une table de correspondance (.anonmap).",
            UnlockError::EmptyPassword => "Saisissez le mot de passe de la table.",
            UnlockError::NotAMap => "Ce fichier n'est pas une table de correspondance .anonmap.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    Locked,
    NoInput,
    NoOutputDir,
    Busy,
}

impl RestoreError {
    pub fn message(self) -> &'static str {
        match self {
            RestoreError::Locked => "Ouvrez la table de correspondance avant de restaurer.",
            RestoreError::NoInput => "Déposez un fichier ou collez le texte renvoyé par l'IA.",
            RestoreError::NoOutputDir => "Choisissez le dossier où enregistrer les fichiers.",
            RestoreError::Busy => "Un traitement est déjà en cours.",
        }
    }
}

/// Traitement en cours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Opening,
    Restoring,
}

/// Données confiées au fil de travail pour restaurer des fichiers.
#[derive(Debug)]
pub struct RestoreJob {
    pub paths: Vec<PathBuf>,
    pub session: Session,
    pub out_dir: PathBuf,
}

#[derive(Clone, PartialEq)]
pub struct DeanonymizeState {
    pub map: MapStatus,
    pub password: String,
    /// Message d'erreur d'ouverture (en clair, issu de `VaultError`).
    pub unlock_error: Option<String>,
    pub mode: InputMode,
    pub input_files: Vec<FileEntry>,
    pub pasted: String,
    pub out_dir: Option<PathBuf>,
    pub busy: Option<Busy>,
    pub text_result: Option<(String, RestoreReport)>,
    pub file_results: Vec<RestoreOutcome>,
    pub restore_error: Option<RestoreError>,
}

impl std::fmt::Debug for DeanonymizeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeanonymizeState")
            .field("map", &self.map)
            .field("mode", &self.mode)
            .field("files", &self.input_files.len())
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

impl Default for DeanonymizeState {
    fn default() -> Self {
        Self {
            map: MapStatus::None,
            password: String::new(),
            unlock_error: None,
            mode: InputMode::Paste,
            input_files: Vec::new(),
            pasted: String::new(),
            out_dir: None,
            busy: None,
            text_result: None,
            file_results: Vec::new(),
            restore_error: None,
        }
    }
}

/// Plages (en octets) des balises `tags` dans `text`, triées, sans chevauchement.
pub fn tag_ranges(text: &str, tags: &[String]) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    for tag in tags.iter().filter(|t| !t.is_empty()) {
        out.extend(
            text.match_indices(tag.as_str())
                .map(|(i, m)| i..i + m.len()),
        );
    }
    out.sort_by_key(|r| r.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start < last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// Vrai pour un chemin `.anonmap`.
pub fn is_map(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("anonmap"))
}

impl DeanonymizeState {
    /// Choix du fichier de table. Refuse une autre extension que `.anonmap`.
    pub fn choose_map(&mut self, path: PathBuf) -> Result<(), UnlockError> {
        let r = if is_map(&path) {
            let name = crate::ui::service::file_name(&path);
            self.map = MapStatus::Locked { path, name };
            self.text_result = None;
            self.file_results.clear();
            Ok(())
        } else {
            Err(UnlockError::NotAMap)
        };
        self.unlock_error = r.err().map(|e| e.message().to_owned());
        r
    }

    /// Prépare l'ouverture : renvoie le chemin et le mot de passe, efface ce dernier de l'écran.
    pub fn begin_unlock(&mut self) -> Result<(PathBuf, String), UnlockError> {
        let r = match &self.map {
            MapStatus::None | MapStatus::Unlocked { .. } => Err(UnlockError::NoMap),
            MapStatus::Locked { .. } if self.password.is_empty() => Err(UnlockError::EmptyPassword),
            MapStatus::Locked { path, .. } => {
                self.busy = Some(Busy::Opening);
                Ok((path.clone(), std::mem::take(&mut self.password)))
            }
        };
        if let Err(e) = r {
            self.unlock_error = Some(e.message().to_owned());
        }
        r
    }

    /// Résultat de l'ouverture.
    pub fn unlock_done(&mut self, result: Result<Session, String>) {
        self.busy = None;
        self.password.clear();
        match result {
            Ok(session) => {
                let name = match &self.map {
                    MapStatus::Locked { name, .. } => name.clone(),
                    _ => String::new(),
                };
                self.map = MapStatus::Unlocked {
                    name,
                    entries: session.entities.len(),
                    session,
                };
                self.unlock_error = None;
            }
            Err(msg) => self.unlock_error = Some(msg),
        }
    }

    pub fn session(&self) -> Option<&Session> {
        match &self.map {
            MapStatus::Unlocked { session, .. } => Some(session),
            _ => None,
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.session().is_some()
    }

    pub fn add_input_files(&mut self, files: impl IntoIterator<Item = FileEntry>) {
        for f in files {
            if !self.input_files.iter().any(|x| x.path == f.path) {
                if self.out_dir.is_none() {
                    self.out_dir = f.path.parent().map(|p| p.to_path_buf());
                }
                self.input_files.push(f);
            }
        }
        self.mode = InputMode::File;
        self.restore_error = None;
    }

    pub fn remove_input_file(&mut self, index: usize) {
        if index < self.input_files.len() {
            self.input_files.remove(index);
        }
    }

    pub fn has_input(&self) -> bool {
        match self.mode {
            InputMode::File => !self.input_files.is_empty(),
            InputMode::Paste => !self.pasted.trim().is_empty(),
        }
    }

    fn check(&self) -> Result<(), RestoreError> {
        if self.busy.is_some() {
            Err(RestoreError::Busy)
        } else if !self.is_unlocked() {
            Err(RestoreError::Locked)
        } else if !self.has_input() {
            Err(RestoreError::NoInput)
        } else {
            Ok(())
        }
    }

    /// Restauration du texte collé (rapide : sur le fil de l'interface).
    pub fn restore_text(
        &mut self,
        restore: impl FnOnce(&str, &Session) -> (String, RestoreReport),
    ) -> Result<(), RestoreError> {
        let r = self.check().and_then(|()| {
            if self.mode != InputMode::Paste {
                return Err(RestoreError::NoInput);
            }
            match self.session() {
                Some(s) => {
                    let out = restore(&self.pasted, s);
                    self.text_result = Some(out);
                    Ok(())
                }
                None => Err(RestoreError::Locked),
            }
        });
        self.restore_error = r.err();
        r
    }

    /// Restauration des fichiers : données pour le fil de travail.
    pub fn begin_restore_files(&mut self) -> Result<RestoreJob, RestoreError> {
        let r = self.check().and_then(|()| {
            let out_dir = self.out_dir.clone().ok_or(RestoreError::NoOutputDir)?;
            let session = self.session().cloned().ok_or(RestoreError::Locked)?;
            self.busy = Some(Busy::Restoring);
            Ok(RestoreJob {
                paths: self.input_files.iter().map(|f| f.path.clone()).collect(),
                session,
                out_dir,
            })
        });
        self.restore_error = r.as_ref().err().copied();
        r
    }

    pub fn restore_files_done(&mut self, results: Vec<RestoreOutcome>) {
        self.busy = None;
        self.file_results = results;
    }

    /// Ce qui manque pour restaurer, en une phrase.
    pub fn restore_blocker(&self) -> Option<&'static str> {
        if self.busy.is_some() {
            Some("Un traitement est déjà en cours.")
        } else if !self.is_unlocked() {
            Some("Ouvrez d'abord une table de correspondance.")
        } else if !self.has_input() {
            Some(match self.mode {
                InputMode::Paste => "Collez d'abord la réponse de l'IA.",
                InputMode::File => "Ajoutez d'abord les fichiers à restaurer.",
            })
        } else {
            None
        }
    }

    /// Ce qui manque pour coller et restaurer en un geste.
    pub fn paste_blocker(&self) -> Option<&'static str> {
        if self.busy.is_some() {
            Some("Un traitement est déjà en cours.")
        } else if !self.is_unlocked() {
            Some("Ouvrez d'abord une table de correspondance.")
        } else {
            None
        }
    }

    /// Ce qui manque pour ouvrir la table.
    pub fn unlock_blocker(&self) -> Option<&'static str> {
        match &self.map {
            MapStatus::None => Some("Choisissez d'abord une table de correspondance."),
            MapStatus::Locked { .. } if self.password.is_empty() => {
                Some("Saisissez le mot de passe de la table.")
            }
            _ => None,
        }
    }

    /// Referme la table et oublie tout.
    pub fn lock(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::{EntityKind, Source};
    use anon_engine::Engine;
    use anon_replace::SessionExt;

    fn session() -> Session {
        let mut s = Session::new();
        s.intern(EntityKind::Personne, "Hélène Kaloï", Source::Manual);
        s
    }

    #[test]
    fn map_must_be_anonmap() {
        let mut s = DeanonymizeState::default();
        assert_eq!(s.choose_map("x.docx".into()), Err(UnlockError::NotAMap));
        assert!(s.unlock_error.is_some());
        assert_eq!(s.choose_map("table.ANONMAP".into()), Ok(()));
        assert!(matches!(s.map, MapStatus::Locked { .. }));
    }

    #[test]
    fn unlock_flow() {
        let mut s = DeanonymizeState::default();
        assert_eq!(s.begin_unlock().err(), Some(UnlockError::NoMap));
        let _ = s.choose_map("t.anonmap".into());
        assert_eq!(s.begin_unlock().err(), Some(UnlockError::EmptyPassword));
        s.password = "secret".into();
        let job = s.begin_unlock();
        assert!(job.is_ok());
        assert!(s.password.is_empty(), "mot de passe retiré de l'écran");
        s.unlock_done(Err("Mot de passe incorrect.".into()));
        assert!(!s.is_unlocked());
        assert_eq!(s.unlock_error.as_deref(), Some("Mot de passe incorrect."));
        s.unlock_done(Ok(session()));
        assert!(s.is_unlocked());
        assert_eq!(s.unlock_error, None);
    }

    #[test]
    fn restore_text_requires_unlock_and_input() {
        let engine = Engine::standard();
        let restore = |t: &str, s: &Session| engine.restore_text(t, s);
        let mut s = DeanonymizeState::default();
        assert_eq!(s.restore_text(restore), Err(RestoreError::Locked));
        let _ = s.choose_map("t.anonmap".into());
        s.unlock_done(Ok(session()));
        assert_eq!(s.restore_text(restore), Err(RestoreError::NoInput));
        s.pasted = "Bonjour [PERSONNE_1], voir [PERSONNE_9].".into();
        assert_eq!(s.restore_text(restore), Ok(()));
        let r = s.text_result.clone();
        assert!(r.as_ref().is_some_and(|(t, _)| t.contains("Hélène Kaloï")));
        assert!(r.is_some_and(|(_, rep)| rep.unknown == vec!["[PERSONNE_9]".to_owned()]));
        s.lock();
        assert_eq!(s, DeanonymizeState::default());
    }

    #[test]
    fn unknown_tags_are_located() {
        let text = "Bonjour [PERSONNE_9], voir [PERSONNE_9] et [LIEU_4].";
        let r = tag_ranges(
            text,
            &["[PERSONNE_9]".into(), "[LIEU_4]".into(), String::new()],
        );
        assert_eq!(r.len(), 3);
        assert_eq!(&text[r[0].clone()], "[PERSONNE_9]");
        assert_eq!(&text[r[2].clone()], "[LIEU_4]");
        assert!(tag_ranges(text, &[]).is_empty());
    }

    #[test]
    fn restore_says_what_is_missing() {
        let mut s = DeanonymizeState::default();
        assert_eq!(
            s.unlock_blocker(),
            Some("Choisissez d'abord une table de correspondance.")
        );
        assert_eq!(
            s.restore_blocker(),
            Some("Ouvrez d'abord une table de correspondance.")
        );
        let _ = s.choose_map("t.anonmap".into());
        assert_eq!(
            s.unlock_blocker(),
            Some("Saisissez le mot de passe de la table.")
        );
        s.password = "x".into();
        assert_eq!(s.unlock_blocker(), None);
        s.unlock_done(Ok(session()));
        assert_eq!(
            s.restore_blocker(),
            Some("Collez d'abord la réponse de l'IA.")
        );
        s.mode = InputMode::File;
        assert_eq!(
            s.restore_blocker(),
            Some("Ajoutez d'abord les fichiers à restaurer.")
        );
        s.mode = InputMode::Paste;
        s.pasted = "[PERSONNE_1]".into();
        assert_eq!(s.restore_blocker(), None);
    }

    #[test]
    fn file_restore_job() {
        let mut s = DeanonymizeState::default();
        s.add_input_files([FileEntry::new("C:/d/reponse.docx".into(), Some(1))]);
        assert_eq!(s.mode, InputMode::File);
        assert_eq!(s.begin_restore_files().err(), Some(RestoreError::Locked));
        let _ = s.choose_map("t.anonmap".into());
        s.unlock_done(Ok(session()));
        let job = s.begin_restore_files();
        assert!(job.is_ok());
        assert_eq!(s.busy, Some(Busy::Restoring));
        assert_eq!(s.begin_restore_files().err(), Some(RestoreError::Busy));
        if let Ok(job) = job {
            assert_eq!(job.out_dir, PathBuf::from("C:/d"));
        }
        s.restore_files_done(Vec::new());
        assert!(s.busy.is_none());
    }
}
