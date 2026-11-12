//! Erreurs du moteur. Les messages sont en français et ne contiennent jamais
//! de contenu utilisateur : seulement des extensions, des emplacements et des
//! balises.

use anon_core::{FormatError, Tag};

/// Valeur validée retrouvée dans le fichier produit. On ne garde que la balise
/// et l'emplacement, jamais la valeur elle-même.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    pub tag: Tag,
    /// Emplacement dans le fichier (`word/document.xml`, `ppt/media/image1.png`…).
    pub part: String,
}

/// Erreur d'un traitement de fichier.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{}", unsupported_message(.extension))]
    Unsupported { extension: String },
    #[error("le fichier est vide")]
    Empty,
    #[error("{0}")]
    Format(#[from] FormatError),
    #[error(
        "contrôle de fuite : {} valeur(s) validée(s) encore présente(s) dans le fichier produit ; aucun fichier n'a été créé",
        .leaks.len()
    )]
    Leak { leaks: Vec<Leak> },
    #[error("opération annulée")]
    Cancelled,
}

/// Anciens formats binaires Office.
const LEGACY_OFFICE: &[&str] = &["doc", "xls", "ppt", "dot", "xlt", "pot"];
/// Raccourcis Google Drive (pas de contenu local).
const GOOGLE_FORMATS: &[&str] = &["gdoc", "gsheet", "gslides", "gdraw", "gform"];

fn unsupported_message(extension: &str) -> String {
    if extension.is_empty() {
        return "format non reconnu : le fichier n'a pas d'extension".into();
    }
    if LEGACY_OFFICE.contains(&extension) {
        return format!(
            "ancien format Office (.{extension}) non pris en charge : ouvrez le fichier dans \
             Word, Excel ou PowerPoint et réenregistrez-le au format moderne (.{extension}x)"
        );
    }
    if GOOGLE_FORMATS.contains(&extension) {
        return format!(
            "raccourci Google (.{extension}) : ce fichier ne contient pas le document. \
             Dans Google Docs, Sheets ou Slides, utilisez Fichier > Télécharger et choisissez \
             le format Microsoft Office (.docx, .xlsx ou .pptx)"
        );
    }
    format!("format non pris en charge (.{extension})")
}

/// Longueur maximale d'une extension reprise dans un message.
const MAX_EXTENSION_CHARS: usize = 10;

/// Extension d'un nom de fichier, en minuscules ASCII, sans point. Vide si
/// absente ou suspecte (elle est reprise dans les messages d'erreur).
pub(crate) fn extension_of(name: &str) -> String {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return String::new();
    };
    if ext.is_empty()
        || ext.chars().count() > MAX_EXTENSION_CHARS
        || !ext.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return String::new();
    }
    ext.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let doc = EngineError::Unsupported {
            extension: "doc".into(),
        }
        .to_string();
        assert!(doc.contains("réenregistrez"), "{doc}");
        assert!(doc.contains(".docx"));
        let g = EngineError::Unsupported {
            extension: "gsheet".into(),
        }
        .to_string();
        assert!(g.contains("Télécharger"), "{g}");
        let other = EngineError::Unsupported {
            extension: "zzz".into(),
        }
        .to_string();
        assert_eq!(other, "format non pris en charge (.zzz)");
    }

    #[test]
    fn extensions() {
        assert_eq!(extension_of("Rapport.DOCX"), "docx");
        assert_eq!(extension_of("sans"), "");
        assert_eq!(extension_of("a.b c"), "");
        assert_eq!(extension_of("x.abcdefghijklmnop"), "");
        assert_eq!(extension_of("archive.tar.gz"), "gz");
    }
}
