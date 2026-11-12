//! Table de correspondance d'une session : balises ↔ valeurs réelles.
//!
//! Les **structures** ci-dessous sont un CONTRAT : elles sont sérialisées telles
//! quelles dans le fichier `.anonmap` (voir `mapping`). Toute évolution passe par
//! `FORMAT_VERSION` et un test de relecture de l'ancienne version.

use crate::entities::{Source, Tag};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Version du format sérialisé.
pub const FORMAT_VERSION: u32 = 1;

/// Ce qui remplace la valeur dans le document anonymisé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Treatment {
    /// Balise `[TYPE_N]`, réversible.
    Tag,
    /// Valeur généralisée lisible (« 1984 », « 30-39 ans », « province Sud »).
    /// Irréversible : la restauration ne la touche pas.
    Generalize(String),
}

/// Une donnée sensible validée par l'utilisateur.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub tag: Tag,
    /// Forme canonique, remise à la place de la balise lors de la restauration.
    pub value: String,
    /// Autres formes de surface remplacées par la même balise (`DUPONT Jean`,
    /// `J. Dupont`). Ne contient pas `value`.
    pub variants: Vec<String>,
    /// Vrai pour les valeurs courtes ou les mots courants (« Pierre », « Martin ») :
    /// la casse doit alors correspondre exactement.
    pub case_sensitive: bool,
    /// Faux si l'utilisateur a décoché la ligne : la valeur n'est pas remplacée.
    pub enabled: bool,
    pub treatment: Treatment,
    pub source: Source,
}

impl fmt::Debug for Entity {
    /// N'affiche jamais la valeur réelle (pas de contenu utilisateur dans les journaux).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entity")
            .field("tag", &self.tag.to_string())
            .field("variants", &self.variants.len())
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// Image d'origine conservée pour remettre l'image non masquée à la restauration.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaOriginal {
    /// SHA-256 (hexadécimal minuscule) de l'image **masquée** écrite dans le fichier anonymisé.
    pub masked_sha256: String,
    pub original: Vec<u8>,
}

impl fmt::Debug for MediaOriginal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaOriginal")
            .field("masked_sha256", &self.masked_sha256)
            .field("len", &self.original.len())
            .finish()
    }
}

/// Table d'une session. Une session couvre plusieurs fichiers et le texte libre :
/// une même valeur y reçoit toujours la même balise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub format_version: u32,
    pub entities: Vec<Entity>,
    /// Liste « ne jamais anonymiser » (formes telles que saisies).
    pub never: Vec<String>,
    pub media: Vec<MediaOriginal>,
    /// Mode irréversible : la table n'est pas enregistrée sur disque.
    pub irreversible: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            entities: Vec::new(),
            never: Vec::new(),
            media: Vec::new(),
            irreversible: false,
        }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entity(&self, tag: Tag) -> Option<&Entity> {
        self.entities.iter().find(|e| e.tag == tag)
    }

    pub fn entity_mut(&mut self, tag: Tag) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.tag == tag)
    }
}

// Les méthodes d'enregistrement des valeurs (`intern`, liaison PERSONNE/NOM/PRENOM,
// variantes, liste « jamais ») sont implémentées dans ce fichier par le lot
// « moteur de remplacement ». Voir AGENTS.md.
