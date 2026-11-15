//! État de l'étape « Réponse » : la clé de restauration (en mémoire après
//! l'envoi, ou ouverte depuis un fichier `.anonmap` avec son mot de passe), la
//! réponse collée de l'IA et sa version restaurée, les fichiers renvoyés par
//! l'IA et leur restauration.

use super::files::FileEntry;
use crate::ui::service::RestoreOutcome;
use anon_core::Session;
use anon_engine::RestoreReport;
use std::ops::Range;
use std::path::PathBuf;

/// Clé de restauration.
#[derive(Clone, PartialEq)]
pub enum MapStatus {
    None,
    /// Fichier choisi, pas encore ouvert.
    Locked {
        path: PathBuf,
        name: String,
    },
    /// Clé ouverte, gardée en mémoire pour toute la session.
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
            UnlockError::NoMap => "Choisissez d'abord une clé de restauration (.anonmap).",
            UnlockError::EmptyPassword => "Saisissez le mot de passe de la clé.",
            UnlockError::NotAMap => "Ce fichier n'est pas une clé de restauration .anonmap.",
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
            RestoreError::Locked => "Ouvrez la clé de restauration avant de restaurer.",
            RestoreError::NoInput => {
                "Collez la réponse de l'IA, ou ajoutez les fichiers qu'elle a renvoyés."
            }
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
    /// Fichier de la clé ouverte ou à ouvrir.
    pub key_path: Option<PathBuf>,
    /// Identifiant de la clé pour le trousseau (`anon_vault::keychain::key_id`).
    pub key_id: Option<String>,
    /// « Retenir sur cet ordinateur » : le mot de passe saisi entre dans le
    /// trousseau du système.
    pub remember: bool,
    /// Destruction de la clé demandée, en attente de confirmation.
    pub destroy_confirm: bool,
    /// La clé vient d'être détruite : l'étape Réponse le dit.
    pub destroyed: bool,
    /// Place des valeurs remises dans la réponse restaurée (octets).
    pub restored_marks: Vec<Range<usize>>,
    /// Message d'erreur d'ouverture (sans contenu de l'utilisateur).
    pub unlock_error: Option<String>,
    /// Fichiers renvoyés par l'IA, à restaurer.
    pub input_files: Vec<FileEntry>,
    /// Réponse de l'IA collée.
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
            key_path: None,
            key_id: None,
            remember: false,
            destroy_confirm: false,
            destroyed: false,
            restored_marks: Vec::new(),
            unlock_error: None,
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
    /// Choix du fichier de clé. Refuse une autre extension que `.anonmap`.
    pub fn choose_map(&mut self, path: PathBuf) -> Result<(), UnlockError> {
        let r = if is_map(&path) {
            let name = crate::ui::service::file_name(&path);
            self.key_path = Some(path.clone());
            self.key_id = None;
            self.destroyed = false;
            self.destroy_confirm = false;
            self.map = MapStatus::Locked { path, name };
            self.text_result = None;
            self.restored_marks.clear();
            self.file_results.clear();
            Ok(())
        } else {
            Err(UnlockError::NotAMap)
        };
        self.unlock_error = r.err().map(|e| e.message().to_owned());
        r
    }

    /// Clé déjà en mémoire (créée à l'étape Envoyer) : aucun mot de passe à
    /// redemander. Une réponse déjà collée est restaurée de nouveau par l'appelant.
    pub fn open_in_memory(&mut self, name: String, session: Session) {
        self.map = MapStatus::Unlocked {
            name,
            entries: session.entities.len(),
            session,
        };
        self.password.clear();
        self.unlock_error = None;
        self.text_result = None;
        self.restored_marks.clear();
        self.restore_error = None;
        self.destroyed = false;
        self.destroy_confirm = false;
    }

    /// Remplace la session de la clé ouverte par une session qui la prolonge
    /// (presse-papiers express : valeurs du texte copié ajoutées). Le nom, le
    /// fichier et la réponse collée restent ; la réponse est à restaurer de
    /// nouveau par l'appelant. Faux si aucune clé n'est ouverte.
    pub fn replace_session(&mut self, session: Session) -> bool {
        match &mut self.map {
            MapStatus::Unlocked {
                entries,
                session: s,
                ..
            } => {
                *entries = session.entities.len();
                *s = session;
                true
            }
            _ => false,
        }
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

    /// Une clé est ouverte ou prête à ouvrir.
    pub fn has_key(&self) -> bool {
        !matches!(self.map, MapStatus::None)
    }

    /// Nom de la clé, si une clé est choisie.
    pub fn key_name(&self) -> Option<&str> {
        match &self.map {
            MapStatus::None => None,
            MapStatus::Locked { name, .. } | MapStatus::Unlocked { name, .. } => Some(name),
        }
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
        self.restore_error = None;
    }

    pub fn remove_input_file(&mut self, index: usize) {
        if index < self.input_files.len() {
            self.input_files.remove(index);
        }
    }

    fn check(&self, has_input: bool) -> Result<(), RestoreError> {
        if self.busy.is_some() {
            Err(RestoreError::Busy)
        } else if !self.is_unlocked() {
            Err(RestoreError::Locked)
        } else if !has_input {
            Err(RestoreError::NoInput)
        } else {
            Ok(())
        }
    }

    /// Restauration de la réponse collée (rapide : sur le fil de l'interface).
    pub fn restore_text(
        &mut self,
        restore: impl FnOnce(&str, &Session) -> (String, RestoreReport),
    ) -> Result<(), RestoreError> {
        self.restored_marks.clear();
        let r = self
            .check(!self.pasted.trim().is_empty())
            .and_then(|()| match self.session() {
                Some(s) => {
                    let out = restore(&self.pasted, s);
                    self.text_result = Some(out);
                    Ok(())
                }
                None => Err(RestoreError::Locked),
            });
        if r.is_err() {
            self.text_result = None;
        }
        self.restore_error = r.err();
        r
    }

    /// Comme [`DeanonymizeState::restore_text`], avec la place des valeurs
    /// remises (`Engine::restore_text_marked`) : l'écran les surligne.
    pub fn restore_text_marked(
        &mut self,
        restore: impl FnOnce(&str, &Session) -> (String, Vec<Range<usize>>, RestoreReport),
    ) -> Result<(), RestoreError> {
        let mut marks = Vec::new();
        let r = self.restore_text(|t, s| {
            let (text, m, report) = restore(t, s);
            marks = m;
            (text, report)
        });
        self.restored_marks = if r.is_ok() { marks } else { Vec::new() };
        r
    }

    /// Date de fin de conservation de la clé ouverte, si elle est dépassée
    /// à la date `today` (`AAAA-MM-JJ`).
    pub fn expired_since(&self, today: &str) -> Option<&str> {
        let until = self.session()?.keep_until.as_deref()?;
        anon_vault::retention::is_past(until, today).then_some(until)
    }

    /// Date de fin de conservation de la clé ouverte.
    pub fn keep_until(&self) -> Option<&str> {
        self.session()?.keep_until.as_deref()
    }

    /// Une clé ouverte ou choisie peut être détruite.
    pub fn can_destroy(&self) -> bool {
        self.busy.is_none() && self.has_key() && self.key_path.is_some()
    }

    /// Clé détruite : tout est oublié ; l'étape Réponse rappelle que les
    /// documents d'origine restent des données personnelles.
    pub fn destroy_done(&mut self) {
        *self = Self::default();
        self.destroyed = true;
    }

    /// Restauration des fichiers : données pour le fil de travail.
    pub fn begin_restore_files(&mut self) -> Result<RestoreJob, RestoreError> {
        let r = self.check(!self.input_files.is_empty()).and_then(|()| {
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

    /// Ce qui manque pour restaurer la réponse collée, en une phrase.
    pub fn restore_blocker(&self) -> Option<&'static str> {
        if self.busy.is_some() {
            Some("Un traitement est déjà en cours.")
        } else if !self.is_unlocked() {
            Some("Ouvrez d'abord la clé de restauration.")
        } else if self.pasted.trim().is_empty() {
            Some("Collez d'abord la réponse de l'IA.")
        } else {
            None
        }
    }

    /// Ce qui manque pour restaurer les fichiers renvoyés par l'IA.
    pub fn files_blocker(&self) -> Option<&'static str> {
        if self.busy.is_some() {
            Some("Un traitement est déjà en cours.")
        } else if !self.is_unlocked() {
            Some("Ouvrez d'abord la clé de restauration.")
        } else if self.input_files.is_empty() {
            Some("Ajoutez d'abord les fichiers à restaurer.")
        } else {
            None
        }
    }

    /// Ce qui manque pour coller et restaurer en un geste.
    pub fn paste_blocker(&self) -> Option<&'static str> {
        if self.busy.is_some() {
            Some("Un traitement est déjà en cours.")
        } else if !self.is_unlocked() {
            Some("Ouvrez d'abord la clé de restauration.")
        } else {
            None
        }
    }

    /// Ce qui manque pour ouvrir la clé.
    pub fn unlock_blocker(&self) -> Option<&'static str> {
        match &self.map {
            MapStatus::None => Some("Choisissez d'abord une clé de restauration."),
            MapStatus::Locked { .. } if self.password.is_empty() => {
                Some("Saisissez le mot de passe de la clé.")
            }
            _ => None,
        }
    }

    /// Referme la clé et oublie tout.
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
        assert!(s.has_key() && !s.is_unlocked());
        assert_eq!(s.key_name(), Some("table.ANONMAP"));
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
    fn key_in_memory_needs_no_password() {
        let mut s = DeanonymizeState {
            pasted: "Bonjour [PERSONNE_1].".into(),
            ..Default::default()
        };
        assert_eq!(
            s.paste_blocker(),
            Some("Ouvrez d'abord la clé de restauration.")
        );
        s.open_in_memory("table_anonymiseur.anonmap".into(), session());
        assert!(s.is_unlocked());
        assert_eq!(s.paste_blocker(), None);
        assert_eq!(s.restore_blocker(), None);
        let engine = Engine::standard();
        assert_eq!(s.restore_text(|t, k| engine.restore_text(t, k)), Ok(()));
        assert!(
            s.text_result
                .as_ref()
                .is_some_and(|(t, _)| t == "Bonjour Hélène Kaloï.")
        );
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
        // Réponse effacée : plus de résultat.
        s.pasted.clear();
        assert_eq!(s.restore_text(restore), Err(RestoreError::NoInput));
        assert!(s.text_result.is_none());
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
            Some("Choisissez d'abord une clé de restauration.")
        );
        assert_eq!(
            s.restore_blocker(),
            Some("Ouvrez d'abord la clé de restauration.")
        );
        let _ = s.choose_map("t.anonmap".into());
        assert_eq!(
            s.unlock_blocker(),
            Some("Saisissez le mot de passe de la clé.")
        );
        s.password = "x".into();
        assert_eq!(s.unlock_blocker(), None);
        s.unlock_done(Ok(session()));
        assert_eq!(
            s.restore_blocker(),
            Some("Collez d'abord la réponse de l'IA.")
        );
        assert_eq!(
            s.files_blocker(),
            Some("Ajoutez d'abord les fichiers à restaurer.")
        );
        s.pasted = "[PERSONNE_1]".into();
        assert_eq!(s.restore_blocker(), None);
    }

    #[test]
    fn file_restore_job() {
        let mut s = DeanonymizeState::default();
        s.add_input_files([FileEntry::new("C:/d/reponse.docx".into(), Some(1))]);
        assert_eq!(s.begin_restore_files().err(), Some(RestoreError::Locked));
        let _ = s.choose_map("t.anonmap".into());
        s.unlock_done(Ok(session()));
        assert_eq!(s.files_blocker(), None);
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

    #[test]
    fn restored_values_are_located() {
        let engine = Engine::standard();
        let mut s = DeanonymizeState {
            pasted: "Bonjour [PERSONNE_1], voir [PERSONNE_9].".into(),
            ..Default::default()
        };
        s.open_in_memory("dossier.anonmap".into(), session());
        assert_eq!(
            s.restore_text_marked(|t, k| engine.restore_text_marked(t, k)),
            Ok(())
        );
        let (text, report) = s.text_result.clone().unwrap_or_default();
        let shown: Vec<&str> = s.restored_marks.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(shown, ["Hélène Kaloï"]);
        assert_eq!(report.unknown, ["[PERSONNE_9]"]);
        // Une restauration sans repères les efface.
        assert_eq!(s.restore_text(|t, k| engine.restore_text(t, k)), Ok(()));
        assert!(s.restored_marks.is_empty());
        // Réponse effacée : plus de résultat, plus de repères.
        s.pasted.clear();
        assert!(
            s.restore_text_marked(|t, k| engine.restore_text_marked(t, k))
                .is_err()
        );
        assert!(s.restored_marks.is_empty());
    }

    #[test]
    fn an_expired_key_is_reminded_then_destroyed() {
        let mut key = session();
        key.keep_until = Some("2026-01-31".into());
        let mut s = DeanonymizeState::default();
        let _ = s.choose_map("C:/cles/dossier.anonmap".into());
        assert_eq!(s.key_path, Some(PathBuf::from("C:/cles/dossier.anonmap")));
        s.unlock_done(Ok(key));
        assert_eq!(s.keep_until(), Some("2026-01-31"));
        assert_eq!(s.expired_since("2026-11-16"), Some("2026-01-31"));
        assert_eq!(
            s.expired_since("2026-01-31"),
            None,
            "le dernier jour compte encore"
        );
        assert!(s.can_destroy());
        s.destroy_confirm = true;
        s.destroy_done();
        assert!(s.destroyed);
        assert!(!s.has_key() && s.key_path.is_none() && s.key_id.is_none());
        assert!(!s.can_destroy());
        // Une autre clé choisie : le message de destruction disparaît.
        let _ = s.choose_map("C:/cles/autre.anonmap".into());
        assert!(!s.destroyed);
        // Une clé sans date (V1) n'est jamais signalée.
        s.unlock_done(Ok(session()));
        assert_eq!(s.expired_since("2099-01-01"), None);
    }
}
