//! Blocs de texte logiques et transformation de texte. CONTRAT partagé.

use std::borrow::Cow;

/// Rôle d'un bloc dans le document, utile à la détection et aux rapports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockRole {
    Body,
    /// Cellule de tableur ; `BlockContext::column_header` porte l'en-tête de colonne.
    Cell,
    Header,
    Footer,
    Comment,
    Note,
    /// Auteur, société, titre… dans les propriétés du fichier.
    Metadata,
    /// Littéral de chaîne dans une formule.
    Formula,
    /// Cible d'un lien (URI).
    Hyperlink,
    /// Texte lu par OCR dans une image ou une page scannée.
    Ocr,
    Other,
}

/// Où se trouve un bloc. `part` ne contient jamais de contenu utilisateur :
/// seulement un emplacement (`word/document.xml`, `page 3`, `Feuil1!B`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockContext {
    pub part: String,
    pub role: BlockRole,
    pub column_header: Option<String>,
}

impl BlockContext {
    pub fn new(part: impl Into<String>, role: BlockRole) -> Self {
        Self {
            part: part.into(),
            role,
            column_header: None,
        }
    }
}

/// Unité de texte logique : un paragraphe entier (runs recollés), une cellule,
/// une propriété… C'est sur ces blocs que tourne la détection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub text: String,
    pub ctx: BlockContext,
}

/// Remplacement de `text[start..end]` par `with`. Offsets en octets, sur des
/// frontières de caractères.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub start: usize,
    pub end: usize,
    pub with: String,
}

/// Transformation appliquée à chaque bloc par un `Handler`. Deux implémentations :
/// `replace::Replacer` (anonymiser) et `restore::Restorer` (désanonymiser).
pub trait TextTransform: Sync {
    /// Remplacements à faire, triés par `start`, sans chevauchement.
    fn replacements(&self, text: &str, ctx: &BlockContext) -> Vec<Replacement>;

    /// Texte transformé. `Cow::Borrowed` si rien ne change.
    fn apply<'a>(&self, text: &'a str, ctx: &BlockContext) -> Cow<'a, str> {
        apply_replacements(text, &self.replacements(text, ctx))
    }

    /// Variante pour une cible d'URI (`mailto:`, `https:`) : les crochets y sont
    /// interdits, on utilise `Tag::uri_token`.
    fn apply_uri<'a>(&self, uri: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(uri)
    }
}

/// Applique des remplacements triés et sans chevauchement.
pub fn apply_replacements<'a>(text: &'a str, reps: &[Replacement]) -> Cow<'a, str> {
    if reps.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for r in reps {
        if r.start < cursor || r.end > text.len() || r.start > r.end {
            continue;
        }
        out.push_str(&text[cursor..r.start]);
        out.push_str(&r.with);
        cursor = r.end;
    }
    out.push_str(&text[cursor..]);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_replacement_borrows() {
        assert!(matches!(
            apply_replacements("abc", &[]),
            Cow::Borrowed("abc")
        ));
    }

    #[test]
    fn replaces_in_order() {
        let reps = vec![
            Replacement {
                start: 0,
                end: 4,
                with: "[PRENOM_1]".into(),
            },
            Replacement {
                start: 5,
                end: 11,
                with: "[NOM_1]".into(),
            },
        ];
        assert_eq!(
            apply_replacements("Jean Dupont !", &reps),
            "[PRENOM_1] [NOM_1] !"
        );
    }

    #[test]
    fn ignores_overlapping_or_out_of_range() {
        let reps = vec![
            Replacement {
                start: 0,
                end: 4,
                with: "A".into(),
            },
            Replacement {
                start: 2,
                end: 6,
                with: "B".into(),
            },
            Replacement {
                start: 9,
                end: 99,
                with: "C".into(),
            },
        ];
        assert_eq!(apply_replacements("Jean Dupont", &reps), "A Dupont");
    }
}
