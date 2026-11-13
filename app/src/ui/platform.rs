//! Effets de bord du système (dialogues natifs, explorateur de fichiers),
//! isolés pour que le rendu reste testable sans fenêtre.

use super::state::files::FileEntry;
use std::path::{Path, PathBuf};

pub trait Platform {
    /// Dialogue de choix de documents à anonymiser.
    fn pick_documents(&mut self) -> Vec<FileEntry>;
    /// Dialogue de choix d'une table `.anonmap`.
    fn pick_map(&mut self) -> Option<PathBuf>;
    /// Dialogue de choix de fichiers à restaurer.
    fn pick_responses(&mut self) -> Vec<FileEntry>;
    /// Dialogue de choix d'un dossier.
    fn pick_folder(&mut self, start: Option<&Path>) -> Option<PathBuf>;
    /// Emplacement fixe des exemples (`Documents/Anonymiseur/Exemples`), s'il
    /// est connu.
    fn examples_dir(&mut self) -> Option<PathBuf>;
    /// Dialogue de choix du dossier où copier les fichiers d'exemple (repli
    /// quand le dossier Documents est introuvable).
    fn pick_examples_folder(&mut self) -> Option<PathBuf>;
    /// Dialogue « Enregistrer sous » pour un texte Markdown.
    fn save_markdown(&mut self, start: Option<&Path>, name: &str) -> Option<PathBuf>;
    /// Ouvre un dossier dans l'explorateur.
    fn open_folder(&mut self, path: &Path);
}

/// Entrée de liste pour un chemin (taille lue sans échouer).
pub fn file_entry(path: PathBuf) -> FileEntry {
    let size = std::fs::metadata(&path).ok().map(|m| m.len());
    FileEntry::new(path, size)
}

/// Implémentation native : `rfd` pour les dialogues.
#[derive(Debug, Default)]
pub struct NativePlatform;

impl Platform for NativePlatform {
    fn pick_documents(&mut self) -> Vec<FileEntry> {
        rfd::FileDialog::new()
            .set_title("Choisir les documents à anonymiser")
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(file_entry)
            .collect()
    }

    fn pick_map(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Choisir la table de correspondance")
            .add_filter("Table de correspondance", &["anonmap"])
            .pick_file()
    }

    fn pick_responses(&mut self) -> Vec<FileEntry> {
        rfd::FileDialog::new()
            .set_title("Choisir les fichiers à restaurer")
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(file_entry)
            .collect()
    }

    fn pick_folder(&mut self, start: Option<&Path>) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new().set_title("Choisir le dossier de sortie");
        if let Some(s) = start {
            d = d.set_directory(s);
        }
        d.pick_folder()
    }

    fn examples_dir(&mut self) -> Option<PathBuf> {
        dirs::document_dir().map(|d| super::service::examples_dir_in(&d))
    }

    fn pick_examples_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Choisir où copier les fichiers d'exemple")
            .pick_folder()
    }

    fn save_markdown(&mut self, start: Option<&Path>, name: &str) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title("Enregistrer le texte anonymisé")
            .add_filter("Markdown", &["md"])
            .set_file_name(name);
        if let Some(s) = start {
            d = d.set_directory(s);
        }
        d.save_file()
    }

    fn open_folder(&mut self, path: &Path) {
        let program = if cfg!(target_os = "windows") {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        // Échec silencieux : le chemin est affiché dans le rapport.
        let _ = std::process::Command::new(program).arg(path).spawn();
    }
}

/// Sans effet : tests. Garde la trace des demandes.
#[derive(Debug, Default)]
pub struct NullPlatform {
    pub requests: Vec<&'static str>,
    /// Emplacement des exemples rendu par [`Platform::examples_dir`].
    pub examples: Option<PathBuf>,
}

impl Platform for NullPlatform {
    fn pick_documents(&mut self) -> Vec<FileEntry> {
        self.requests.push("documents");
        Vec::new()
    }

    fn pick_map(&mut self) -> Option<PathBuf> {
        self.requests.push("table");
        None
    }

    fn pick_responses(&mut self) -> Vec<FileEntry> {
        self.requests.push("réponses");
        Vec::new()
    }

    fn pick_folder(&mut self, _start: Option<&Path>) -> Option<PathBuf> {
        self.requests.push("dossier de sortie");
        None
    }

    fn examples_dir(&mut self) -> Option<PathBuf> {
        self.examples.clone()
    }

    fn pick_examples_folder(&mut self) -> Option<PathBuf> {
        self.requests.push("dossier des exemples");
        None
    }

    fn save_markdown(&mut self, _start: Option<&Path>, _name: &str) -> Option<PathBuf> {
        self.requests.push("enregistrer");
        None
    }

    fn open_folder(&mut self, _path: &Path) {
        self.requests.push("ouvrir le dossier");
    }
}
