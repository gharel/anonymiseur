//! Types de données sensibles, balises et détections. CONTRAT partagé.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Type d'une donnée sensible. Le préfixe de balise est stable : il fait partie
/// du format `.anonmap` et de ce que voit l'IA. Ne jamais renommer une variante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntityKind {
    Personne,
    Nom,
    Prenom,
    Societe,
    Lieu,
    Adresse,
    Email,
    Tel,
    Iban,
    Nir,
    Siret,
    Ridet,
    Cb,
    Ip,
    Url,
    DateNaiss,
    Date,
    Age,
    Fonction,
    Montant,
    Matricule,
    Plaque,
    Identifiant,
    Autre,
}

/// Regroupement pour l'affichage (couleur des badges) et les profils.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Family {
    Personnes,
    Organisations,
    Lieux,
    Contacts,
    Identifiants,
    Finances,
    Contexte,
}

impl EntityKind {
    pub const ALL: &'static [EntityKind] = &[
        EntityKind::Personne,
        EntityKind::Nom,
        EntityKind::Prenom,
        EntityKind::Societe,
        EntityKind::Lieu,
        EntityKind::Adresse,
        EntityKind::Email,
        EntityKind::Tel,
        EntityKind::Iban,
        EntityKind::Nir,
        EntityKind::Siret,
        EntityKind::Ridet,
        EntityKind::Cb,
        EntityKind::Ip,
        EntityKind::Url,
        EntityKind::DateNaiss,
        EntityKind::Date,
        EntityKind::Age,
        EntityKind::Fonction,
        EntityKind::Montant,
        EntityKind::Matricule,
        EntityKind::Plaque,
        EntityKind::Identifiant,
        EntityKind::Autre,
    ];

    /// Préfixe de balise, en majuscules ASCII (`PERSONNE`, `DATE_NAISS`…).
    pub fn prefix(self) -> &'static str {
        match self {
            EntityKind::Personne => "PERSONNE",
            EntityKind::Nom => "NOM",
            EntityKind::Prenom => "PRENOM",
            EntityKind::Societe => "SOCIETE",
            EntityKind::Lieu => "LIEU",
            EntityKind::Adresse => "ADRESSE",
            EntityKind::Email => "EMAIL",
            EntityKind::Tel => "TEL",
            EntityKind::Iban => "IBAN",
            EntityKind::Nir => "NIR",
            EntityKind::Siret => "SIRET",
            EntityKind::Ridet => "RIDET",
            EntityKind::Cb => "CB",
            EntityKind::Ip => "IP",
            EntityKind::Url => "URL",
            EntityKind::DateNaiss => "DATE_NAISS",
            EntityKind::Date => "DATE",
            EntityKind::Age => "AGE",
            EntityKind::Fonction => "FONCTION",
            EntityKind::Montant => "MONTANT",
            EntityKind::Matricule => "MATRICULE",
            EntityKind::Plaque => "PLAQUE",
            EntityKind::Identifiant => "IDENTIFIANT",
            EntityKind::Autre => "AUTRE",
        }
    }

    /// Inverse strict de [`prefix`](Self::prefix) (majuscules ASCII exactes).
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.prefix() == prefix)
    }

    /// Libellé français pour l'interface.
    pub fn label(self) -> &'static str {
        match self {
            EntityKind::Personne => "Personne",
            EntityKind::Nom => "Nom de famille",
            EntityKind::Prenom => "Prénom",
            EntityKind::Societe => "Société",
            EntityKind::Lieu => "Lieu",
            EntityKind::Adresse => "Adresse",
            EntityKind::Email => "Email",
            EntityKind::Tel => "Téléphone",
            EntityKind::Iban => "IBAN",
            EntityKind::Nir => "N° de sécurité sociale",
            EntityKind::Siret => "SIRET / SIREN",
            EntityKind::Ridet => "RIDET",
            EntityKind::Cb => "Carte bancaire",
            EntityKind::Ip => "Adresse IP",
            EntityKind::Url => "Lien",
            EntityKind::DateNaiss => "Date de naissance",
            EntityKind::Date => "Date",
            EntityKind::Age => "Âge",
            EntityKind::Fonction => "Fonction",
            EntityKind::Montant => "Montant",
            EntityKind::Matricule => "Matricule",
            EntityKind::Plaque => "Plaque d'immatriculation",
            EntityKind::Identifiant => "Identifiant",
            EntityKind::Autre => "Autre",
        }
    }

    pub fn family(self) -> Family {
        match self {
            EntityKind::Personne | EntityKind::Nom | EntityKind::Prenom => Family::Personnes,
            EntityKind::Societe => Family::Organisations,
            EntityKind::Lieu | EntityKind::Adresse => Family::Lieux,
            EntityKind::Email | EntityKind::Tel | EntityKind::Url | EntityKind::Ip => {
                Family::Contacts
            }
            EntityKind::Nir
            | EntityKind::Siret
            | EntityKind::Ridet
            | EntityKind::Matricule
            | EntityKind::Plaque
            | EntityKind::Identifiant => Family::Identifiants,
            EntityKind::Iban | EntityKind::Cb | EntityKind::Montant => Family::Finances,
            EntityKind::DateNaiss
            | EntityKind::Date
            | EntityKind::Age
            | EntityKind::Fonction
            | EntityKind::Autre => Family::Contexte,
        }
    }

    /// Vrai pour les quasi-identifiants : ils n'identifient pas seuls, mais leur
    /// combinaison le peut. Détectés seulement en profil « Renforcé ».
    pub fn is_quasi_identifier(self) -> bool {
        matches!(
            self,
            EntityKind::Date
                | EntityKind::Age
                | EntityKind::Fonction
                | EntityKind::Montant
                | EntityKind::Lieu
                | EntityKind::Matricule
        )
    }

    /// `Personne`, `Nom` et `Prenom` partagent leurs indices : `[PERSONNE_1]`,
    /// `[NOM_1]` et `[PRENOM_1]` désignent la même personne.
    pub fn is_person_part(self) -> bool {
        matches!(
            self,
            EntityKind::Personne | EntityKind::Nom | EntityKind::Prenom
        )
    }
}

/// Balise `[PREFIXE_N]`, N ≥ 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tag {
    pub kind: EntityKind,
    pub index: u32,
}

impl Tag {
    pub fn new(kind: EntityKind, index: u32) -> Self {
        Self { kind, index }
    }

    /// Forme sans crochets (`PERSONNE_1`), pour les contextes où ils sont interdits.
    pub fn bare(self) -> String {
        format!("{}_{}", self.kind.prefix(), self.index)
    }

    /// Forme valide dans une URI : `EMAIL_1@anon.invalid` pour un email,
    /// `https://anon.invalid/URL_1` pour un lien, forme nue sinon.
    pub fn uri_token(self) -> String {
        match self.kind {
            EntityKind::Email => format!("{}@anon.invalid", self.bare()),
            EntityKind::Url => format!("https://anon.invalid/{}", self.bare()),
            _ => self.bare(),
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}_{}]", self.kind.prefix(), self.index)
    }
}

/// Origine d'une détection, pour l'écran de validation et la fusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    /// Expression régulière + contrôle de clé.
    Rule,
    /// Dictionnaire de prénoms, noms, communes.
    Dictionary,
    /// Règle de colonne de tableur (en-tête « Nom », « Tél »…).
    Column,
    /// Modèle d'IA embarqué.
    Ner,
    /// Liste « toujours anonymiser » de l'utilisateur.
    List,
    /// Ajout manuel dans l'écran de validation.
    Manual,
}

/// Une occurrence détectée dans un bloc de texte. `start..end` sont des offsets
/// en **octets** dans `TextBlock::text`, toujours sur des frontières de caractères.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub kind: EntityKind,
    pub start: usize,
    pub end: usize,
    pub source: Source,
    /// 0.0 à 1.0. Les règles à clé validée valent 1.0.
    pub confidence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_round_trip() {
        for kind in EntityKind::ALL {
            assert_eq!(EntityKind::from_prefix(kind.prefix()), Some(*kind));
        }
    }

    #[test]
    fn prefixes_are_unique_ascii_uppercase() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in EntityKind::ALL {
            let p = kind.prefix();
            assert!(p.chars().all(|c| c.is_ascii_uppercase() || c == '_'), "{p}");
            assert!(seen.insert(p), "préfixe en double : {p}");
        }
    }

    #[test]
    fn tag_forms() {
        let t = Tag::new(EntityKind::Email, 2);
        assert_eq!(t.to_string(), "[EMAIL_2]");
        assert_eq!(t.bare(), "EMAIL_2");
        assert_eq!(t.uri_token(), "EMAIL_2@anon.invalid");
        assert_eq!(
            Tag::new(EntityKind::Url, 1).uri_token(),
            "https://anon.invalid/URL_1"
        );
        assert_eq!(
            Tag::new(EntityKind::DateNaiss, 3).to_string(),
            "[DATE_NAISS_3]"
        );
    }
}
