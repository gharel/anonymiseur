//! Fichiers déposés : nom, format reconnu à l'extension, taille.

use std::path::{Path, PathBuf};

/// Format d'un fichier, d'après son extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Word,
    Excel,
    PowerPoint,
    OdfText,
    OdfSheet,
    OdfSlides,
    Pdf,
    Text,
    Csv,
    Image,
    /// Raccourci Google (`.gdoc`, `.gsheet`, `.gslides`) : pas de contenu local.
    GoogleShortcut,
    Unsupported,
}

impl FileFormat {
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "docx" | "docm" | "dotx" => FileFormat::Word,
            "xlsx" | "xlsm" => FileFormat::Excel,
            "pptx" | "pptm" => FileFormat::PowerPoint,
            "odt" => FileFormat::OdfText,
            "ods" => FileFormat::OdfSheet,
            "odp" => FileFormat::OdfSlides,
            "pdf" => FileFormat::Pdf,
            "txt" | "md" => FileFormat::Text,
            "csv" | "tsv" => FileFormat::Csv,
            "png" | "jpg" | "jpeg" | "tif" | "tiff" => FileFormat::Image,
            "gdoc" | "gsheet" | "gslides" => FileFormat::GoogleShortcut,
            _ => FileFormat::Unsupported,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileFormat::Word => "Document Word",
            FileFormat::Excel => "Classeur Excel",
            FileFormat::PowerPoint => "Présentation PowerPoint",
            FileFormat::OdfText => "Texte OpenDocument",
            FileFormat::OdfSheet => "Classeur OpenDocument",
            FileFormat::OdfSlides => "Présentation OpenDocument",
            FileFormat::Pdf => "PDF",
            FileFormat::Text => "Texte brut",
            FileFormat::Csv => "Tableau CSV",
            FileFormat::Image => "Image",
            FileFormat::GoogleShortcut => "Raccourci Google",
            FileFormat::Unsupported => "Format non pris en charge",
        }
    }

    pub fn is_supported(self) -> bool {
        !matches!(self, FileFormat::GoogleShortcut | FileFormat::Unsupported)
    }
}

/// Fichier de la liste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub format: FileFormat,
    pub size_bytes: Option<u64>,
}

impl FileEntry {
    pub fn new(path: PathBuf, size_bytes: Option<u64>) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "fichier sans nom".to_owned());
        let format = FileFormat::from_path(&path);
        Self {
            path,
            name,
            format,
            size_bytes,
        }
    }
}

/// Taille lisible : « 12 ko », « 1,4 Mo ».
pub fn human_size(bytes: u64) -> String {
    const KO: f64 = 1024.0;
    let b = bytes as f64;
    if b < KO {
        format!("{bytes} o")
    } else if b < KO * KO {
        format!("{} ko", (b / KO).round())
    } else {
        format!("{:.1} Mo", b / (KO * KO)).replace('.', ",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_from_extension() {
        let f = |s: &str| FileFormat::from_path(Path::new(s));
        assert_eq!(f("Contrat.DOCX"), FileFormat::Word);
        assert_eq!(f("a.xlsx"), FileFormat::Excel);
        assert_eq!(f("a.pdf"), FileFormat::Pdf);
        assert_eq!(f("a.gdoc"), FileFormat::GoogleShortcut);
        assert_eq!(f("a.exe"), FileFormat::Unsupported);
        assert_eq!(f("sans_extension"), FileFormat::Unsupported);
        assert!(!FileFormat::GoogleShortcut.is_supported());
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 o");
        assert_eq!(human_size(12 * 1024), "12 ko");
        assert_eq!(human_size(1_468_006), "1,4 Mo");
    }
}
