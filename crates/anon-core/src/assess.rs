//! Évaluation de l'anonymat selon le projet de lignes directrices 02/2026 du CEPD :
//! isolement, recoupement, déduction. CONTRAT partagé.
//!
//! - Entrées (`AssessInput`) : construites par `anon-engine` à partir de l'analyse.
//! - Calcul : crate `anon-assess`.
//! - Résultat (`Assessment`) : affiché par l'interface, résumé dans la fiche
//!   d'évaluation, corrections (`Fix`) appliquées par le moteur.
//!
//! Les champs `value` sont du contenu utilisateur : aucun `Debug` ne les affiche.

use crate::entities::{EntityKind, Tag};
use crate::format::PixelRect;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Taille minimale, par défaut, d'un groupe de personnes indiscernables (k-anonymat).
pub const K_MIN_DEFAULT: u32 = 5;
/// Plus petite valeur de `k_min` acceptée par le réglage.
pub const K_MIN_LOWEST: u32 = 2;
/// Plus grande valeur de `k_min` acceptée par le réglage.
pub const K_MIN_HIGHEST: u32 = 20;
/// Part maximale des lignes d'un tableau que l'outil peut supprimer pour atteindre `k_min`.
pub const MAX_SUPPRESSION_RATE: f64 = 0.05;
/// Délai de réévaluation conseillé, en mois (§ 35 des lignes directrices).
pub const REASSESS_MONTHS: u32 = 12;

/// Objectif choisi pour un dossier. Sérialisé dans `.anonmap` (`Session::goal`) :
/// ne jamais renommer une variante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Goal {
    /// « Masquer pour l'IA » : pseudonymisation, réversible avec la clé de restauration.
    #[default]
    Mask,
    /// « Anonymiser définitivement » : aucune clé, évaluation des trois critères requise.
    Anonymize,
}

impl Goal {
    pub const ALL: &'static [Goal] = &[Goal::Mask, Goal::Anonymize];

    pub fn label(self) -> &'static str {
        match self {
            Goal::Mask => "Masquer pour l'IA",
            Goal::Anonymize => "Anonymiser définitivement",
        }
    }

    /// Suffixe des fichiers produits. Le mot « anonyme » n'apparaît que si
    /// l'évaluation conclut à l'anonymat (exigence R1).
    pub fn output_suffix(self, anonymous: bool) -> &'static str {
        match (self, anonymous) {
            (Goal::Mask, _) => "_masque",
            (Goal::Anonymize, true) => "_anonyme",
            (Goal::Anonymize, false) => "_masque_sans_cle",
        }
    }
}

/// Destinataire du document : l'évaluation se fait pour lui (CJUE, C-413/23 P).
/// Sérialisé dans `.anonmap` (`Session::recipient`) : ne jamais renommer une variante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Recipient {
    /// Assistant d'IA grand public (traité comme un attaquant capable de déduction).
    #[default]
    PublicAi,
    /// IA de votre organisation, encadrée par un contrat.
    OrganizationAi,
    /// Partage avec un tiers identifié.
    ExternalShare,
    /// Publication ouverte.
    Publication,
}

impl Recipient {
    pub const ALL: &'static [Recipient] = &[
        Recipient::PublicAi,
        Recipient::OrganizationAi,
        Recipient::ExternalShare,
        Recipient::Publication,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Recipient::PublicAi => "Assistant IA grand public",
            Recipient::OrganizationAi => "IA de votre organisation, sous contrat",
            Recipient::ExternalShare => "Partage externe",
            Recipient::Publication => "Publication",
        }
    }
}

/// Approche d'évaluation (§ 2 des lignes directrices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Approach {
    /// Toute voie de réidentification compte, quel que soit l'attaquant.
    Simplified,
    /// Le contexte du destinataire, consigné dans la fiche, écarte certains risques.
    Contextual,
}

impl Approach {
    pub fn label(self) -> &'static str {
        match self {
            Approach::Simplified => "Approche simplifiée",
            Approach::Contextual => "Approche contextuelle",
        }
    }
}

/// Réponses aux questions de l'approche contextuelle. `None` : pas de réponse,
/// l'approche simplifiée s'applique sur ce point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ContextAnswers {
    /// Le destinataire connaît-il les personnes ?
    pub recipient_knows_people: Option<bool>,
    /// Le document sera-t-il publié ?
    pub published: Option<bool>,
    /// D'autres documents du même dossier lui seront-ils envoyés ?
    pub other_documents: Option<bool>,
}

impl ContextAnswers {
    pub fn approach(&self) -> Approach {
        if self.recipient_knows_people.is_none()
            && self.published.is_none()
            && self.other_documents.is_none()
        {
            Approach::Simplified
        } else {
            Approach::Contextual
        }
    }
}

/// Réglages d'une évaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssessSettings {
    pub goal: Goal,
    pub recipient: Recipient,
    pub answers: ContextAnswers,
    /// Taille minimale d'un groupe, bornée par [`K_MIN_LOWEST`] et [`K_MIN_HIGHEST`].
    pub k_min: u32,
}

impl Default for AssessSettings {
    fn default() -> Self {
        Self {
            goal: Goal::default(),
            recipient: Recipient::default(),
            answers: ContextAnswers::default(),
            k_min: K_MIN_DEFAULT,
        }
    }
}

/// Les trois critères de l'anonymat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Criterion {
    /// Une personne (un enregistrement) peut-elle être isolée ?
    Isolation,
    /// Peut-on relier ce document à d'autres données sur la même personne ?
    Linkage,
    /// Peut-on déduire une information précise et significative sur une personne ?
    Inference,
}

impl Criterion {
    pub const ALL: [Criterion; 3] = [
        Criterion::Isolation,
        Criterion::Linkage,
        Criterion::Inference,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Criterion::Isolation => "Isolement",
            Criterion::Linkage => "Recoupement",
            Criterion::Inference => "Déduction",
        }
    }

    /// Position dans [`Criterion::ALL`] et dans `SubjectReport::passed`.
    pub fn index(self) -> usize {
        match self {
            Criterion::Isolation => 0,
            Criterion::Linkage => 1,
            Criterion::Inference => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Entrées
// ---------------------------------------------------------------------------

/// Ce que montrera le document produit à la place d'une valeur.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Shown {
    /// Remplacée par une balise.
    Masked(Tag),
    /// Remplacée par une forme moins précise (« 60-69 ans », « province Sud »).
    Generalized(String),
    /// Visible telle quelle.
    Visible,
}

impl fmt::Debug for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shown::Masked(tag) => write!(f, "Masked({tag})"),
            Shown::Generalized(_) => f.write_str("Generalized(..)"),
            Shown::Visible => f.write_str("Visible"),
        }
    }
}

/// Une valeur mentionnée dans un bloc de texte.
#[derive(Clone, PartialEq)]
pub struct Mention {
    /// Identifiant de la valeur dans l'analyse du moteur (`Candidate::id`). Les
    /// corrections (`Fix`) désignent les valeurs par cet identifiant.
    pub value_id: usize,
    pub kind: EntityKind,
    /// Forme canonique (contenu utilisateur).
    pub value: String,
    pub shown: Shown,
}

impl fmt::Debug for Mention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mention")
            .field("value_id", &self.value_id)
            .field("kind", &self.kind)
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

/// Un paragraphe (ou une cellule de texte libre) et les valeurs qu'il mentionne,
/// dans l'ordre du texte. Les indices d'une personne sont ceux du même paragraphe.
#[derive(Debug, Clone, PartialEq)]
pub struct TextUnit {
    /// Position du fichier dans le dossier.
    pub file: usize,
    pub mentions: Vec<Mention>,
}

/// Une cellule de tableau.
#[derive(Clone, PartialEq)]
pub struct TableCell {
    /// Contenu utilisateur.
    pub value: String,
    /// Identifiant de la valeur dans l'analyse, si elle y figure.
    pub value_id: Option<usize>,
    pub shown: Shown,
}

impl fmt::Debug for TableCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableCell")
            .field("value_id", &self.value_id)
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

/// Rôle d'une colonne dans l'évaluation d'un tableau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnRole {
    /// Identifiant direct (nom, email, téléphone…).
    Direct(EntityKind),
    /// Identifiant stable (numéro client, matricule) : risque de recoupement.
    Stable(EntityKind),
    /// Indice (âge, date, commune, fonction, montant).
    Clue(EntityKind),
    /// Attribut sensible (art. 9 et 10).
    Sensitive(EntityKind),
    /// Autre colonne, sans effet sur l'évaluation.
    Other,
}

/// Un tableau (feuille de calcul, CSV) : une ligne est un enregistrement.
#[derive(Clone, PartialEq)]
pub struct TableInput {
    pub file: usize,
    /// Emplacement (nom de feuille), jamais affiché dans les journaux.
    pub sheet: String,
    pub headers: Vec<String>,
    /// Rôle de chaque colonne, même longueur que `headers`.
    pub roles: Vec<ColumnRole>,
    pub rows: Vec<Vec<TableCell>>,
}

impl fmt::Debug for TableInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableInput")
            .field("file", &self.file)
            .field("roles", &self.roles)
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

/// Une image du dossier (image seule, image d'un document, page scannée).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInput {
    pub file: usize,
    /// Nom de l'image dans le fichier (`ppt/media/image1.png`, `page 2`).
    pub media: String,
    /// Visages trouvés et non masqués. `None` : visages non vérifiés (édition Lite).
    pub faces: Option<Vec<PixelRect>>,
}

/// Tout ce que l'évaluation examine.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AssessInput {
    pub texts: Vec<TextUnit>,
    pub tables: Vec<TableInput>,
    pub images: Vec<ImageInput>,
}

// ---------------------------------------------------------------------------
// Résultat
// ---------------------------------------------------------------------------

/// Ce qui est évalué : une personne d'un texte, un groupe de lignes, une image.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubjectRef {
    /// Personne désignée par l'identifiant de sa valeur `Personne` (ou `Nom`, `Prenom`).
    Person { value_id: usize },
    /// Groupe de lignes d'un tableau qui partagent les mêmes indices.
    TableGroup { table: usize, group: usize },
    /// Image qui montre un visage.
    Image { file: usize, media: String },
}

/// Raison d'un échec.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FailureReason {
    /// Isolement : un identifiant direct reste visible.
    DirectIdentifier { kind: EntityKind },
    /// Isolement : le groupe estimé compte moins de `k_min` personnes.
    SmallGroup { estimate: u64 },
    /// Recoupement : un identifiant stable reste visible.
    StableIdentifier { kind: EntityKind },
    /// Déduction : une donnée sensible ou un événement personnel reste lié à une personne isolable.
    SensitiveLinked { kind: EntityKind },
    /// Déduction : tous les membres d'un groupe partagent la même valeur sensible (l-diversité).
    HomogeneousGroup { column: usize },
    /// Isolement : un visage reste visible.
    FaceVisible,
    /// Non concluant : visages non vérifiés (édition Lite).
    FacesNotChecked,
}

/// Correction proposée, appliquée par le moteur et annulable par l'interface.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Fix {
    /// Remplacer la valeur par une balise.
    Mask { value_id: usize },
    /// Remplacer la valeur par une forme moins précise.
    Generalize { value_id: usize, to: String },
    /// Plusieurs valeurs rendues moins précises (colonne d'un tableau).
    GeneralizeMany { items: Vec<(usize, String)> },
    /// Masquer les indices des lignes hors norme d'un tableau (suppression bornée
    /// par [`MAX_SUPPRESSION_RATE`]).
    SuppressRows {
        table: usize,
        rows: Vec<usize>,
        value_ids: Vec<usize>,
    },
    /// Masquer une zone d'image (visage).
    MaskRegion {
        file: usize,
        media: String,
        rect: PixelRect,
    },
}

impl fmt::Debug for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Mask { value_id } => write!(f, "Mask({value_id})"),
            Fix::Generalize { value_id, .. } => write!(f, "Generalize({value_id})"),
            Fix::GeneralizeMany { items } => write!(f, "GeneralizeMany({})", items.len()),
            Fix::SuppressRows { table, rows, .. } => {
                write!(f, "SuppressRows(table {table}, {} lignes)", rows.len())
            }
            Fix::MaskRegion { file, media, rect } => {
                write!(f, "MaskRegion({file}, {media}, {rect:?})")
            }
        }
    }
}

/// Un échec d'un critère pour un sujet, avec ses corrections (la première est conseillée).
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub criterion: Criterion,
    pub subject: SubjectRef,
    pub reason: FailureReason,
    pub fixes: Vec<Fix>,
}

/// Bilan d'un sujet.
#[derive(Debug, Clone, PartialEq)]
pub struct SubjectReport {
    pub subject: SubjectRef,
    /// Indices encore visibles rattachés au sujet, ce qu'une IA pourrait croiser :
    /// (type, identifiant de la valeur dans l'analyse, comme `Mention::value_id`).
    pub clues: Vec<(EntityKind, usize)>,
    /// Taille estimée du groupe auquel le sujet appartient, si elle a été estimée.
    pub group_estimate: Option<u64>,
    /// Résultat par critère, dans l'ordre de [`Criterion::ALL`].
    pub passed: [bool; 3],
}

impl SubjectReport {
    pub fn passes(&self) -> bool {
        self.passed.iter().all(|p| *p)
    }
}

/// Conclusion de l'évaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Chaque sujet passe les trois critères (§ 36 : un seul échec suffit à conclure
    /// que le document reste personnel).
    Anonymous,
    /// Des sujets restent reconnaissables.
    Reidentifiable { subjects: usize },
    /// L'évaluation ne peut pas conclure (visages non vérifiés).
    Inconclusive,
}

/// Résultat complet d'une évaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub settings: AssessSettings,
    pub approach: Approach,
    pub subjects: Vec<SubjectReport>,
    pub failures: Vec<Failure>,
    pub verdict: Verdict,
}

impl Assessment {
    pub fn is_anonymous(&self) -> bool {
        self.verdict == Verdict::Anonymous
    }

    /// Nombre de sujets qui passent et qui échouent un critère.
    pub fn summary(&self, criterion: Criterion) -> (usize, usize) {
        let i = criterion.index();
        let failing = self
            .subjects
            .iter()
            .filter(|s| s.passed.get(i).is_some_and(|p| !p))
            .count();
        (self.subjects.len().saturating_sub(failing), failing)
    }

    /// Échecs d'un critère.
    pub fn failures_of(&self, criterion: Criterion) -> impl Iterator<Item = &Failure> {
        self.failures
            .iter()
            .filter(move |f| f.criterion == criterion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixes_follow_the_verdict() {
        assert_eq!(Goal::Mask.output_suffix(true), "_masque");
        assert_eq!(Goal::Anonymize.output_suffix(true), "_anonyme");
        assert_eq!(Goal::Anonymize.output_suffix(false), "_masque_sans_cle");
    }

    #[test]
    fn approach_from_answers() {
        assert_eq!(ContextAnswers::default().approach(), Approach::Simplified);
        let a = ContextAnswers {
            published: Some(false),
            ..ContextAnswers::default()
        };
        assert_eq!(a.approach(), Approach::Contextual);
    }

    #[test]
    fn debug_hides_values() {
        let m = Mention {
            value_id: 3,
            kind: EntityKind::Lieu,
            value: "Farino".into(),
            shown: Shown::Generalized("province Sud".into()),
        };
        let s = format!("{m:?}");
        assert!(!s.contains("Farino") && !s.contains("province"), "{s}");
        let fix = Fix::Generalize {
            value_id: 3,
            to: "province Sud".into(),
        };
        assert!(!format!("{fix:?}").contains("province"));
    }

    #[test]
    fn summary_counts_subjects() {
        let ok = SubjectReport {
            subject: SubjectRef::Person { value_id: 1 },
            clues: Vec::new(),
            group_estimate: None,
            passed: [true; 3],
        };
        let ko = SubjectReport {
            subject: SubjectRef::Person { value_id: 2 },
            passed: [false, true, true],
            ..ok.clone()
        };
        let a = Assessment {
            settings: AssessSettings::default(),
            approach: Approach::Simplified,
            subjects: vec![ok, ko],
            failures: Vec::new(),
            verdict: Verdict::Reidentifiable { subjects: 1 },
        };
        assert_eq!(a.summary(Criterion::Isolation), (1, 1));
        assert_eq!(a.summary(Criterion::Linkage), (2, 0));
        assert!(!a.is_anonymous());
    }
}
