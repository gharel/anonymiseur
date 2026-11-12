//! État de l'onglet « Anonymiser » : Déposer → Vérifier → Protéger → Terminé.
//! Aucune dépendance à egui : les transitions se testent seules.

use super::files::FileEntry;
use super::password::{self, Strength};
use anon_core::{DetectOptions, EntityKind, Family, Profile, Tag};
use std::path::PathBuf;

/// Étapes, dans l'ordre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Drop,
    Review,
    Protect,
    Done,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::Drop, Step::Review, Step::Protect, Step::Done];

    pub fn index(self) -> usize {
        match self {
            Step::Drop => 0,
            Step::Review => 1,
            Step::Protect => 2,
            Step::Done => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Step::Drop => "Déposer",
            Step::Review => "Vérifier",
            Step::Protect => "Protéger",
            Step::Done => "Terminé",
        }
    }
}

/// Une valeur détectée, proposée à l'utilisateur.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub kind: EntityKind,
    pub value: String,
    /// Extrait autour de la valeur (déjà raccourci).
    pub context: String,
    pub occurrences: u32,
    pub tag: Tag,
    pub selected: bool,
    /// Détection incertaine : l'utilisateur doit trancher.
    pub to_confirm: bool,
}

/// Erreurs de saisie d'un terme manuel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualTermError {
    Empty,
    AlreadyListed,
}

impl ManualTermError {
    pub fn message(self) -> &'static str {
        match self {
            ManualTermError::Empty => "Saisissez un terme à anonymiser.",
            ManualTermError::AlreadyListed => "Ce terme est déjà dans la liste.",
        }
    }
}

/// Écran « Vérifier ».
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewState {
    pub profile: Profile,
    pub candidates: Vec<Candidate>,
    /// Familles affichées ; vide = toutes.
    pub family_filter: Vec<Family>,
    pub manual_term: String,
    pub manual_kind: EntityKind,
    pub manual_error: Option<ManualTermError>,
}

impl Default for ReviewState {
    fn default() -> Self {
        Self {
            profile: Profile::Standard,
            candidates: Vec::new(),
            family_filter: Vec::new(),
            manual_term: String::new(),
            manual_kind: EntityKind::Autre,
            manual_error: None,
        }
    }
}

impl ReviewState {
    pub fn new(candidates: Vec<Candidate>) -> Self {
        Self {
            candidates,
            ..Self::default()
        }
    }

    /// Le type est-il proposé avec le profil courant ?
    fn profile_wants(&self, kind: EntityKind) -> bool {
        DetectOptions {
            profile: self.profile,
            ..DetectOptions::default()
        }
        .wants(kind)
    }

    /// Une ligne est retenue par le profil (les ajouts manuels le sont toujours).
    fn in_profile(&self, c: &Candidate) -> bool {
        self.profile_wants(c.kind) || c.kind == EntityKind::Autre
    }

    /// Indices des lignes visibles, dans l'ordre : profil puis filtre de famille.
    pub fn visible(&self) -> Vec<usize> {
        self.candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| self.in_profile(c))
            .filter(|(_, c)| {
                self.family_filter.is_empty() || self.family_filter.contains(&c.kind.family())
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Nombre de lignes par famille, dans le profil courant (pour les filtres).
    pub fn count_by_family(&self, family: Family) -> usize {
        self.candidates
            .iter()
            .filter(|c| self.in_profile(c) && c.kind.family() == family)
            .count()
    }

    pub fn toggle_family(&mut self, family: Family) {
        if let Some(pos) = self.family_filter.iter().position(|f| *f == family) {
            self.family_filter.remove(pos);
        } else {
            self.family_filter.push(family);
        }
    }

    pub fn clear_filter(&mut self) {
        self.family_filter.clear();
    }

    pub fn toggle(&mut self, index: usize) {
        if let Some(c) = self.candidates.get_mut(index) {
            c.selected = !c.selected;
            c.to_confirm = false;
        }
    }

    /// État de la case d'en-tête pour les lignes visibles.
    pub fn header_state(&self) -> (usize, usize) {
        let vis = self.visible();
        let checked = vis
            .iter()
            .filter(|i| self.candidates.get(**i).is_some_and(|c| c.selected))
            .count();
        (checked, vis.len())
    }

    /// Coche ou décoche toutes les lignes visibles.
    pub fn set_all_visible(&mut self, selected: bool) {
        for i in self.visible() {
            if let Some(c) = self.candidates.get_mut(i) {
                c.selected = selected;
            }
        }
    }

    /// Lignes qui seront anonymisées (cochées et dans le profil).
    pub fn selected_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|c| c.selected && self.in_profile(c))
            .count()
    }

    pub fn pending_confirmations(&self) -> usize {
        self.candidates
            .iter()
            .filter(|c| c.to_confirm && self.in_profile(c))
            .count()
    }

    /// Prochain index libre pour un type (les parties de personne partagent leurs indices).
    fn next_index(&self, kind: EntityKind) -> u32 {
        self.candidates
            .iter()
            .filter(|c| {
                c.tag.kind == kind || (kind.is_person_part() && c.tag.kind.is_person_part())
            })
            .map(|c| c.tag.index)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Ajoute le terme saisi à la main ; il est coché d'office.
    pub fn add_manual(&mut self) -> Result<usize, ManualTermError> {
        let term = self.manual_term.trim().to_owned();
        let result = if term.is_empty() {
            Err(ManualTermError::Empty)
        } else if self
            .candidates
            .iter()
            .any(|c| c.value.to_lowercase() == term.to_lowercase())
        {
            Err(ManualTermError::AlreadyListed)
        } else {
            let kind = self.manual_kind;
            let tag = Tag::new(kind, self.next_index(kind));
            self.candidates.push(Candidate {
                kind,
                value: term,
                context: "Ajout manuel".to_owned(),
                occurrences: 0,
                tag,
                selected: true,
                to_confirm: false,
            });
            self.manual_term.clear();
            // Le nouveau terme doit rester visible malgré un éventuel filtre.
            let fam = kind.family();
            if !self.family_filter.is_empty() && !self.family_filter.contains(&fam) {
                self.family_filter.push(fam);
            }
            Ok(self.candidates.len() - 1)
        };
        self.manual_error = result.err();
        result
    }
}

/// Erreurs de mot de passe à l'étape « Protéger ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordIssue {
    TooWeak,
    Mismatch,
}

impl PasswordIssue {
    pub fn message(self) -> &'static str {
        match self {
            PasswordIssue::TooWeak => "Mot de passe trop faible : au moins 10 caractères, variés.",
            PasswordIssue::Mismatch => "Les deux mots de passe ne correspondent pas.",
        }
    }
}

/// Écran « Protéger ».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtectState {
    pub password: String,
    pub confirm: String,
    /// Aucune table de correspondance : la restauration sera impossible.
    pub irreversible: bool,
    pub output_dir: Option<PathBuf>,
}

impl ProtectState {
    pub fn strength(&self) -> Strength {
        password::evaluate(&self.password)
    }

    /// Problème à signaler sous le champ concerné (aucun tant que rien n'est saisi).
    pub fn password_issue(&self) -> Option<PasswordIssue> {
        if self.irreversible || self.password.is_empty() {
            return None;
        }
        if !self.strength().level.is_acceptable() {
            return Some(PasswordIssue::TooWeak);
        }
        None
    }

    pub fn confirm_issue(&self) -> Option<PasswordIssue> {
        if self.irreversible || self.confirm.is_empty() {
            return None;
        }
        (self.confirm != self.password).then_some(PasswordIssue::Mismatch)
    }

    pub fn can_proceed(&self) -> bool {
        self.irreversible
            || (self.strength().level.is_acceptable() && self.password == self.confirm)
    }

    /// Efface les mots de passe de la mémoire de l'écran.
    pub fn clear_secrets(&mut self) {
        self.password.clear();
        self.confirm.clear();
    }
}

/// Résultat du contrôle de fuite d'un fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakCheck {
    Passed,
    /// Le fichier n'est pas livré : `n` valeurs subsistent.
    Blocked(u32),
}

/// Risque de ré-identification estimé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Risk::Low => "Faible",
            Risk::Medium => "Moyen",
            Risk::High => "Élevé",
        }
    }
}

/// Rapport d'un fichier traité.
#[derive(Debug, Clone, PartialEq)]
pub struct FileReport {
    pub name: String,
    pub output_name: String,
    pub replacements: u32,
    pub warnings: Vec<String>,
    pub leak_check: LeakCheck,
    pub risk: Risk,
    pub risk_reason: Option<String>,
}

/// Erreur de transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError {
    NoFiles,
    NothingSelected,
    PasswordNotReady,
    WrongStep,
}

impl StepError {
    pub fn message(self) -> &'static str {
        match self {
            StepError::NoFiles => "Ajoutez au moins un fichier pris en charge.",
            StepError::NothingSelected => "Cochez au moins une valeur à anonymiser.",
            StepError::PasswordNotReady => {
                "Choisissez un mot de passe robuste et confirmez-le, ou activez le mode irréversible."
            }
            StepError::WrongStep => "Action impossible à cette étape.",
        }
    }
}

/// État complet de l'onglet.
#[derive(Debug, Clone, PartialEq)]
pub struct AnonymizeState {
    pub step: Step,
    pub files: Vec<FileEntry>,
    /// Fichiers refusés au dépôt (raccourcis Google, formats inconnus).
    pub rejected: Vec<FileEntry>,
    pub review: ReviewState,
    pub protect: ProtectState,
    pub reports: Vec<FileReport>,
    pub error: Option<StepError>,
}

impl Default for AnonymizeState {
    fn default() -> Self {
        Self {
            step: Step::Drop,
            files: Vec::new(),
            rejected: Vec::new(),
            review: ReviewState::default(),
            protect: ProtectState::default(),
            reports: Vec::new(),
            error: None,
        }
    }
}

impl AnonymizeState {
    /// Ajoute des fichiers ; ignore les doublons, met de côté les formats refusés.
    pub fn add_files(&mut self, entries: impl IntoIterator<Item = FileEntry>) {
        for e in entries {
            if self.files.iter().any(|f| f.path == e.path)
                || self.rejected.iter().any(|f| f.path == e.path)
            {
                continue;
            }
            if e.format.is_supported() {
                self.files.push(e);
            } else {
                self.rejected.push(e);
            }
        }
        self.error = None;
    }

    pub fn remove_file(&mut self, index: usize) {
        if index < self.files.len() {
            self.files.remove(index);
        }
    }

    pub fn dismiss_rejected(&mut self) {
        self.rejected.clear();
    }

    /// Déposer → Vérifier, avec les détections fournies par l'analyse.
    pub fn start_review(&mut self, candidates: Vec<Candidate>) -> Result<(), StepError> {
        let r = if self.step != Step::Drop {
            Err(StepError::WrongStep)
        } else if self.files.is_empty() {
            Err(StepError::NoFiles)
        } else {
            let profile = self.review.profile;
            self.review = ReviewState::new(candidates);
            self.review.profile = profile;
            self.step = Step::Review;
            Ok(())
        };
        self.error = r.err();
        r
    }

    /// Vérifier → Protéger.
    pub fn confirm_review(&mut self) -> Result<(), StepError> {
        let r = if self.step != Step::Review {
            Err(StepError::WrongStep)
        } else if self.review.selected_count() == 0 {
            Err(StepError::NothingSelected)
        } else {
            self.step = Step::Protect;
            Ok(())
        };
        self.error = r.err();
        r
    }

    /// Protéger → Terminé, avec les rapports produits par le traitement.
    pub fn finish(&mut self, reports: Vec<FileReport>) -> Result<(), StepError> {
        let r = if self.step != Step::Protect {
            Err(StepError::WrongStep)
        } else if !self.protect.can_proceed() {
            Err(StepError::PasswordNotReady)
        } else {
            self.protect.clear_secrets();
            self.reports = reports;
            self.step = Step::Done;
            Ok(())
        };
        self.error = r.err();
        r
    }

    /// Étape précédente (sans effet sur Déposer et Terminé).
    pub fn back(&mut self) {
        self.step = match self.step {
            Step::Review => Step::Drop,
            Step::Protect => Step::Review,
            other => other,
        };
        self.error = None;
    }

    /// Recommence depuis le début.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Consigne à coller avant le texte anonymisé dans l'IA.
    pub fn ai_instruction(&self) -> String {
        "Ce document a été anonymisé : les données personnelles sont remplacées par des \
balises entre crochets, comme [PERSONNE_1] ou [EMAIL_2]. Recopiez ces balises exactement, \
sans les modifier, les traduire ni les compléter. N'essayez pas de deviner les valeurs \
d'origine."
            .to_owned()
    }

    /// Fichiers réellement livrés (contrôle de fuite réussi).
    pub fn delivered_count(&self) -> usize {
        self.reports
            .iter()
            .filter(|r| r.leak_check == LeakCheck::Passed)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mock;

    fn with_files() -> AnonymizeState {
        let mut s = AnonymizeState::default();
        s.add_files(mock::files());
        s
    }

    #[test]
    fn cannot_review_without_files() {
        let mut s = AnonymizeState::default();
        assert_eq!(s.start_review(mock::candidates()), Err(StepError::NoFiles));
        assert_eq!(s.step, Step::Drop);
        assert_eq!(s.error, Some(StepError::NoFiles));
    }

    #[test]
    fn google_shortcuts_are_rejected_and_duplicates_ignored() {
        let mut s = AnonymizeState::default();
        s.add_files(vec![
            FileEntry::new("a/Contrat.docx".into(), Some(10)),
            FileEntry::new("a/Contrat.docx".into(), Some(10)),
            FileEntry::new("a/Budget.gsheet".into(), Some(1)),
        ]);
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.rejected.len(), 1);
        s.remove_file(0);
        s.remove_file(5);
        assert!(s.files.is_empty());
    }

    #[test]
    fn full_happy_path() {
        let mut s = with_files();
        assert_eq!(s.start_review(mock::candidates()), Ok(()));
        assert_eq!(s.step, Step::Review);
        assert_eq!(s.confirm_review(), Ok(()));
        assert_eq!(s.step, Step::Protect);
        assert_eq!(s.finish(mock::reports()), Err(StepError::PasswordNotReady));
        s.protect.password = "Cheval-Agrafe-Batterie-47".into();
        s.protect.confirm = "Cheval-Agrafe-Batterie-47".into();
        assert_eq!(s.finish(mock::reports()), Ok(()));
        assert_eq!(s.step, Step::Done);
        assert!(s.protect.password.is_empty(), "mot de passe effacé");
        assert!(!s.reports.is_empty());
        s.reset();
        assert_eq!(s, AnonymizeState::default());
    }

    #[test]
    fn back_goes_one_step() {
        let mut s = with_files();
        let _ = s.start_review(mock::candidates());
        let _ = s.confirm_review();
        s.back();
        assert_eq!(s.step, Step::Review);
        s.back();
        assert_eq!(s.step, Step::Drop);
        s.back();
        assert_eq!(s.step, Step::Drop);
    }

    #[test]
    fn transitions_refuse_the_wrong_step() {
        let mut s = with_files();
        assert_eq!(s.confirm_review(), Err(StepError::WrongStep));
        assert_eq!(s.finish(vec![]), Err(StepError::WrongStep));
    }

    #[test]
    fn nothing_selected_blocks_review() {
        let mut s = with_files();
        let _ = s.start_review(mock::candidates());
        s.review.set_all_visible(false);
        assert_eq!(s.confirm_review(), Err(StepError::NothingSelected));
    }

    #[test]
    fn irreversible_mode_skips_password() {
        let mut s = with_files();
        let _ = s.start_review(mock::candidates());
        let _ = s.confirm_review();
        s.protect.irreversible = true;
        assert!(s.protect.can_proceed());
        assert_eq!(s.finish(mock::reports()), Ok(()));
    }

    #[test]
    fn password_issues() {
        let mut p = ProtectState::default();
        assert_eq!(p.password_issue(), None);
        p.password = "court".into();
        assert_eq!(p.password_issue(), Some(PasswordIssue::TooWeak));
        p.password = "Qmzrtx-Wpvk7".into();
        assert_eq!(p.password_issue(), None);
        p.confirm = "Qmzrtx-Wpvk".into();
        assert_eq!(p.confirm_issue(), Some(PasswordIssue::Mismatch));
        assert!(!p.can_proceed());
        p.confirm.push('7');
        assert_eq!(p.confirm_issue(), None);
        assert!(p.can_proceed());
    }

    #[test]
    fn profile_hides_quasi_identifiers() {
        let mut r = ReviewState::new(mock::candidates());
        let standard = r.visible().len();
        assert!(
            r.visible()
                .iter()
                .all(|i| !r.candidates[*i].kind.is_quasi_identifier())
        );
        r.profile = Profile::Renforce;
        assert!(r.visible().len() > standard);
    }

    #[test]
    fn family_filter() {
        let mut r = ReviewState::new(mock::candidates());
        r.profile = Profile::Renforce;
        let all = r.visible().len();
        r.toggle_family(Family::Personnes);
        let people = r.visible();
        assert!(!people.is_empty() && people.len() < all);
        assert!(
            people
                .iter()
                .all(|i| r.candidates[*i].kind.family() == Family::Personnes)
        );
        assert_eq!(people.len(), r.count_by_family(Family::Personnes));
        r.toggle_family(Family::Contacts);
        assert_eq!(
            r.visible().len(),
            r.count_by_family(Family::Personnes) + r.count_by_family(Family::Contacts)
        );
        r.toggle_family(Family::Personnes);
        r.toggle_family(Family::Contacts);
        assert_eq!(r.visible().len(), all);
        r.toggle_family(Family::Lieux);
        r.clear_filter();
        assert_eq!(r.visible().len(), all);
    }

    #[test]
    fn header_checkbox_acts_on_visible_rows_only() {
        let mut r = ReviewState::new(mock::candidates());
        r.toggle_family(Family::Contacts);
        r.set_all_visible(false);
        let (checked, total) = r.header_state();
        assert_eq!(checked, 0);
        assert!(total > 0);
        r.clear_filter();
        assert!(
            r.header_state().0 > 0,
            "les autres familles restent cochées"
        );
    }

    #[test]
    fn toggling_a_row_resolves_its_confirmation() {
        let mut r = ReviewState::new(mock::candidates());
        let idx = r.candidates.iter().position(|c| c.to_confirm);
        assert!(idx.is_some());
        if let Some(i) = idx {
            let before = r.pending_confirmations();
            r.toggle(i);
            assert_eq!(r.pending_confirmations(), before - 1);
        }
    }

    #[test]
    fn manual_terms() {
        let mut r = ReviewState::new(mock::candidates());
        assert_eq!(r.add_manual(), Err(ManualTermError::Empty));
        r.manual_term = "  claire martin ".into();
        assert_eq!(r.add_manual(), Err(ManualTermError::AlreadyListed));
        r.manual_term = "Projet Hirondelle".into();
        r.toggle_family(Family::Personnes);
        let added = r.add_manual();
        assert!(added.is_ok());
        assert_eq!(r.manual_error, None);
        assert!(r.manual_term.is_empty());
        if let Ok(i) = added {
            assert!(r.visible().contains(&i), "visible malgré le filtre");
            assert_eq!(r.candidates[i].tag.to_string(), "[AUTRE_1]");
        }
    }

    #[test]
    fn person_parts_share_indices() {
        let mut r = ReviewState::new(mock::candidates());
        let max_person = r
            .candidates
            .iter()
            .filter(|c| c.kind.is_person_part())
            .map(|c| c.tag.index)
            .max()
            .unwrap_or(0);
        r.manual_term = "Bastien".into();
        r.manual_kind = EntityKind::Prenom;
        if let Ok(i) = r.add_manual() {
            assert_eq!(r.candidates[i].tag.index, max_person + 1);
        }
    }
}
