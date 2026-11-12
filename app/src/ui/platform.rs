//! Effets de bord du système (dialogues natifs, explorateur de fichiers),
//! isolés pour que le rendu reste testable sans fenêtre.

use super::state::files::FileEntry;
use std::path::{Path, PathBuf};

pub trait Platform {
    /// Dialogue de choix de documents à anonymiser.
    fn pick_documents(&mut self) -> Vec<FileEntry>;
    /// Dialogue de choix d'une table `.anonmap`.
    fn pick_map(&mut self) -> Option<PathBuf>;
    /// Dialogue de choix d'un fichier à restaurer.
    fn pick_response(&mut self) -> Option<FileEntry>;
    /// Ouvre un dossier dans l'explorateur.
    fn open_folder(&mut self, path: &Path);
}

/// Taille d'un fichier, sans échouer.
pub fn file_entry(path: PathBuf) -> FileEntry {
    let size = std::fs::metadata(&path).ok().map(|m| m.len());
    FileEntry::new(path, size)
}

/// Implémentation native : `rfd` pour les dialogues.
#[derive(Debug, Default)]
pub struct NativePlatform;

const DOCUMENT_EXTENSIONS: &[&str] = &[
    "docx", "xlsx", "pptx", "odt", "ods", "odp", "pdf", "txt", "md", "csv", "png", "jpg", "jpeg",
];

impl Platform for NativePlatform {
    fn pick_documents(&mut self) -> Vec<FileEntry> {
        rfd::FileDialog::new()
            .set_title("Choisir les documents à anonymiser")
            .add_filter("Documents pris en charge", DOCUMENT_EXTENSIONS)
            .add_filter("Tous les fichiers", &["*"])
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

    fn pick_response(&mut self) -> Option<FileEntry> {
        rfd::FileDialog::new()
            .set_title("Choisir le fichier à restaurer")
            .add_filter("Documents pris en charge", DOCUMENT_EXTENSIONS)
            .pick_file()
            .map(file_entry)
    }

    fn open_folder(&mut self, path: &Path) {
        let program = if cfg!(target_os = "windows") {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        // Échec silencieux : l'utilisateur voit le chemin dans le rapport.
        let _ = std::process::Command::new(program).arg(path).spawn();
    }
}

/// Sans effet : maquette et tests. Garde la trace des demandes.
#[derive(Debug, Default)]
pub struct NullPlatform {
    pub requests: Vec<&'static str>,
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

    fn pick_response(&mut self) -> Option<FileEntry> {
        self.requests.push("réponse");
        None
    }

    fn open_folder(&mut self, _path: &Path) {
        self.requests.push("dossier");
    }
}
