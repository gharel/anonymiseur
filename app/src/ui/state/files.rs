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

/// Sous-dossier de sortie par défaut, à côté du premier fichier : les résultats
/// ne se mélangent pas aux sources.
pub const OUTPUT_DIR_NAME: &str = "Anonymisés";

/// Suffixes des fichiers produits par l'outil (voir `anon-engine`).
const OUTPUT_SUFFIXES: [&str; 2] = ["_anonymise", "_restaure"];

/// Vrai pour un fichier produit par l'outil : `nom_anonymise.ext`,
/// `nom_anonymise_2.ext`, `nom_restaure.ext`, `nom_restaure_3.ext` ou une
/// table `.anonmap`. Ces fichiers ne sont jamais anonymisés une seconde fois.
pub fn is_tool_output(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext == "anonmap" {
        return true;
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let stem = stem.to_lowercase();
    // `_2`, `_3`… ajouté quand le nom était pris.
    let base = match stem.rsplit_once('_') {
        Some((head, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => head,
        _ => stem.as_str(),
    };
    OUTPUT_SUFFIXES.iter().any(|s| base.ends_with(s))
}

/// Dossier de sortie par défaut pour un premier fichier : `Anonymisés` à côté
/// de lui (ou son dossier s'il s'appelle déjà ainsi).
pub fn default_output_dir(first_file: &Path) -> Option<PathBuf> {
    let parent = first_file.parent()?;
    let already = parent
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == OUTPUT_DIR_NAME);
    Some(if already {
        parent.to_path_buf()
    } else {
        parent.join(OUTPUT_DIR_NAME)
    })
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
    fn tool_outputs_are_recognised() {
        let out = |s: &str| is_tool_output(Path::new(s));
        assert!(out("C:/d/contrat_anonymise.docx"));
        assert!(out("contrat_ANONYMISE_2.docx"));
        assert!(out("notes_anonymise_12.txt"));
        assert!(out("notes_restaure.txt"));
        assert!(out("notes_anonymise_2_restaure_3.txt"));
        assert!(out("table_anonymiseur.anonmap"));
        assert!(out("table_anonymiseur_2.ANONMAP"));
        assert!(out("brief_anonymise.md"));
        assert!(out("sans_extension_anonymise"));
        assert!(!out("contrat.docx"));
        assert!(!out("anonymise.txt"), "nom sans préfixe : pas une sortie");
        assert!(!out("notes_anonymiseur.txt"));
        assert!(!out("restaurer_2.txt"));
        assert!(!out("rapport_2.docx"));
        assert!(!out("reponse-ia-exemple.md"));
    }

    #[test]
    fn output_dir_is_a_subfolder() {
        assert_eq!(
            default_output_dir(Path::new("C:/d/a.docx")),
            Some(PathBuf::from("C:/d").join(OUTPUT_DIR_NAME))
        );
        let inside = PathBuf::from("C:/d").join(OUTPUT_DIR_NAME).join("a.docx");
        assert_eq!(
            default_output_dir(&inside),
            Some(PathBuf::from("C:/d").join(OUTPUT_DIR_NAME))
        );
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 o");
        assert_eq!(human_size(12 * 1024), "12 ko");
        assert_eq!(human_size(1_468_006), "1,4 Mo");
    }
}
