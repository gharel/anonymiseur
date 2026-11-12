//! Étiquettes des modèles (`config.json`, champ `id2label`) et correspondance avec
//! les types de l'application.

use crate::NerError;
use anon_core::entities::EntityKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bio {
    /// Début d'entité (`B-`).
    B,
    /// Intérieur d'entité (`I-`), ou schéma IO sans `B-`.
    I,
    O,
}

/// Une étiquette du modèle. `kind == None` : hors entité ou type ignoré.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label {
    pub bio: Bio,
    pub kind: Option<EntityKind>,
}

/// Type d'étiquette du modèle -> type de l'application. Les types déjà couverts par
/// les règles à clé (email, téléphone, IBAN, URL…) ou trop vagues (`MISC`, `DATE`,
/// `ID_NUMBER`, `SECRET`…) sont ignorés.
pub fn map_label_type(t: &str) -> Option<EntityKind> {
    match t {
        "PER" | "PERSON" => Some(EntityKind::Personne),
        "ORG" => Some(EntityKind::Societe),
        "LOC" => Some(EntityKind::Lieu),
        "ADDRESS" => Some(EntityKind::Adresse),
        "DATE_OF_BIRTH" => Some(EntityKind::DateNaiss),
        _ => None,
    }
}

pub fn parse_label(raw: &str) -> Label {
    match raw.split_once('-') {
        Some(("B", t)) => Label {
            bio: Bio::B,
            kind: map_label_type(t),
        },
        Some(("I", t)) => Label {
            bio: Bio::I,
            kind: map_label_type(t),
        },
        _ => Label {
            bio: Bio::O,
            kind: None,
        },
    }
}

#[cfg(test)]
pub fn parse_labels(raw: &[&str]) -> Vec<Label> {
    raw.iter().map(|r| parse_label(r)).collect()
}

/// Lit `id2label` dans un `config.json` de modèle Hugging Face.
pub fn labels_from_config(json: &[u8]) -> Result<Vec<Label>, NerError> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| NerError::Config("JSON illisible"))?;
    let map = v
        .get("id2label")
        .and_then(|m| m.as_object())
        .ok_or(NerError::Config("champ id2label absent"))?;
    let n = map.len();
    let mut out = vec![
        Label {
            bio: Bio::O,
            kind: None
        };
        n
    ];
    for (k, val) in map {
        let i: usize = k
            .parse()
            .map_err(|_| NerError::Config("clé id2label non numérique"))?;
        let s = val
            .as_str()
            .ok_or(NerError::Config("étiquette non textuelle"))?;
        let slot = out
            .get_mut(i)
            .ok_or(NerError::Config("indices id2label non contigus"))?;
        *slot = parse_label(s);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bio_and_io_schemes() {
        assert_eq!(
            parse_label("B-PERSON"),
            Label {
                bio: Bio::B,
                kind: Some(EntityKind::Personne)
            }
        );
        assert_eq!(parse_label("I-LOC").kind, Some(EntityKind::Lieu));
        assert_eq!(parse_label("I-MISC").kind, None);
        assert_eq!(parse_label("B-EMAIL").kind, None);
        assert_eq!(parse_label("O").bio, Bio::O);
    }

    #[test]
    fn reads_id2label() {
        let cfg = br#"{"id2label":{"0":"O","2":"I-PER","1":"I-ORG"}}"#;
        let l = labels_from_config(cfg).unwrap();
        assert_eq!(l[1].kind, Some(EntityKind::Societe));
        assert_eq!(l[2].kind, Some(EntityKind::Personne));
        assert!(labels_from_config(br#"{"id2label":{"0":"O","5":"I-PER"}}"#).is_err());
        assert!(labels_from_config(b"{").is_err());
    }
}
