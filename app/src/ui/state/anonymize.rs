//! État de l'onglet « Anonymiser » : Déposer → Vérifier → Protéger → Terminé.
//! Aucune dépendance à egui : les transitions se testent seules. Les
//! traitements longs sont confiés au fil de travail par `AnonApp` ; l'état
//! reçoit leurs résultats par les méthodes `*_done`.

use super::files::{FileEntry, default_output_dir, is_tool_output};
use super::generalize::{generalize, is_generalizable};
use super::images::{FileMask, ManualMasks};
use crate::ui::service::{BatchReport, Protection, Rejected, unsupported_reason};
use anon_core::{DetectOptions, EntityKind, Family, PixelRect, Profile, Session, Tag, Treatment};
use anon_engine::{Analysis, AnalyzeOptions, Candidate, InputFile, SELECT_MIN_CONFIDENCE};
use anon_vault::{PasswordStrength, password_strength};
use std::collections::HashMap;
use std::path::PathBuf;

/// Étapes, dans l'ordre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
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

/// Traitement en cours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Analyzing,
    OpeningTable,
    Anonymizing,
}

impl Busy {
    pub fn label(self) -> &'static str {
        match self {
            Busy::Analyzing => "Analyse des fichiers",
            Busy::OpeningTable => "Ouverture de la table",
            Busy::Anonymizing => "Anonymisation et contrôle de fuite",
        }
    }
}

/// Bilan d'un ajout de fichiers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddReport {
    /// Fichiers ajoutés à la liste.
    pub added: usize,
    /// Fichiers produits par l'outil (hors tables), écartés.
    pub outputs: usize,
    /// Fichiers refusés (format non pris en charge).
    pub refused: usize,
    /// Table `.anonmap` déposée (la dernière) : proposée à la reprise.
    pub map: Option<PathBuf>,
}

impl AddReport {
    /// Message pour les fichiers produits par l'outil, s'il y en a.
    pub fn outputs_message(&self) -> Option<String> {
        match self.outputs {
            0 => None,
            1 => Some("1 fichier déjà anonymisé ignoré : un fichier produit par l'outil n'est pas anonymisé une seconde fois.".to_owned()),
            n => Some(format!("{n} fichiers déjà anonymisés ignorés : les fichiers produits par l'outil ne sont pas anonymisés une seconde fois.")),
        }
    }
}

/// Forme de comparaison : minuscules, sans accents (recherche et tri).
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'û' | 'ü' | 'ú' | 'ù' => 'u',
            'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// Une ligne est « à confirmer » : valeur ambiguë ou confiance faible.
pub fn needs_confirmation(c: &Candidate) -> bool {
    c.ambiguous || c.confidence < SELECT_MIN_CONFIDENCE
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

/// Image analysée, telle que listée à l'écran de validation.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub file: String,
    pub name: String,
    /// Dimensions lues par l'OCR (0 si inconnues).
    pub width: u32,
    pub height: u32,
    pub low_confidence: bool,
}

impl std::fmt::Debug for ImageInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageInfo")
            .field("name", &self.name)
            .field("low_confidence", &self.low_confidence)
            .finish_non_exhaustive()
    }
}

/// Mode de protection choisi à l'étape « Protéger ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtectMode {
    #[default]
    NewPassword,
    /// Reprendre une table existante : mêmes balises, même mot de passe.
    ExistingTable,
    Irreversible,
}

/// Table existante ouverte (session gardée en mémoire seulement).
#[derive(Clone, PartialEq)]
pub struct ExistingTable {
    pub name: String,
    pub session: Session,
    pub entries: usize,
}

impl std::fmt::Debug for ExistingTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExistingTable")
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

/// Erreurs de mot de passe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordIssue {
    TooWeak,
    Mismatch,
}

impl PasswordIssue {
    pub fn message(self) -> &'static str {
        match self {
            PasswordIssue::TooWeak => {
                "Mot de passe trop faible : au moins 12 caractères, en mêlant minuscules, majuscules, chiffres et symboles, ou une longue phrase."
            }
            PasswordIssue::Mismatch => "Les deux mots de passe ne correspondent pas.",
        }
    }
}

/// Niveau minimal accepté pour chiffrer la table.
pub fn is_acceptable(s: PasswordStrength) -> bool {
    s >= PasswordStrength::Correct
}

/// Écran « Protéger ».
#[derive(Clone, Default, PartialEq)]
pub struct ProtectState {
    pub mode: ProtectMode,
    pub password: String,
    pub confirm: String,
    /// Fichier `.anonmap` choisi pour la reprise, avant ouverture.
    pub existing_path: Option<PathBuf>,
    pub existing: Option<ExistingTable>,
    pub existing_error: Option<String>,
}

impl std::fmt::Debug for ProtectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtectState")
            .field("mode", &self.mode)
            .field("existing", &self.existing)
            .finish_non_exhaustive()
    }
}

impl ProtectState {
    pub fn strength(&self) -> PasswordStrength {
        password_strength(&self.password)
    }

    pub fn password_issue(&self) -> Option<PasswordIssue> {
        if self.mode != ProtectMode::NewPassword || self.password.is_empty() {
            return None;
        }
        (!is_acceptable(self.strength())).then_some(PasswordIssue::TooWeak)
    }

    pub fn confirm_issue(&self) -> Option<PasswordIssue> {
        if self.mode != ProtectMode::NewPassword || self.confirm.is_empty() {
            return None;
        }
        (self.confirm != self.password).then_some(PasswordIssue::Mismatch)
    }

    /// Ce qui manque pour continuer, en une phrase.
    pub fn blocker(&self) -> Option<&'static str> {
        match self.mode {
            ProtectMode::NewPassword => {
                if self.password.is_empty() {
                    Some("Choisissez un mot de passe pour la table.")
                } else if !is_acceptable(self.strength()) {
                    Some("Le mot de passe est trop faible.")
                } else if self.confirm.is_empty() {
                    Some("Confirmez le mot de passe.")
                } else if self.confirm != self.password {
                    Some("Les deux mots de passe diffèrent.")
                } else {
                    None
                }
            }
            ProtectMode::ExistingTable => {
                if self.existing_path.is_none() {
                    Some("Choisissez la table à reprendre.")
                } else if self.existing.is_none() {
                    Some("Ouvrez d'abord la table à reprendre.")
                } else {
                    None
                }
            }
            ProtectMode::Irreversible => None,
        }
    }

    pub fn can_proceed(&self) -> bool {
        match self.mode {
            ProtectMode::NewPassword => {
                is_acceptable(self.strength()) && self.password == self.confirm
            }
            ProtectMode::ExistingTable => self.existing.is_some() && !self.password.is_empty(),
            ProtectMode::Irreversible => true,
        }
    }

    pub fn protection(&self) -> Protection {
        match self.mode {
            ProtectMode::Irreversible => Protection::Irreversible,
            _ => Protection::Password(self.password.clone()),
        }
    }

    pub fn clear_secrets(&mut self) {
        self.password.clear();
        self.confirm.clear();
        self.existing = None;
    }
}

/// Erreur de transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError {
    NoFiles,
    NothingSelected,
    PasswordNotReady,
    NoOutputDir,
    Busy,
    WrongStep,
}

impl StepError {
    pub fn message(self) -> &'static str {
        match self {
            StepError::NoFiles => "Ajoutez au moins un fichier pris en charge.",
            StepError::NothingSelected => "Cochez au moins une valeur à anonymiser.",
            StepError::PasswordNotReady => {
                "Choisissez un mot de passe robuste et confirmez-le, reprenez une table existante, ou activez le mode irréversible."
            }
            StepError::NoOutputDir => "Choisissez le dossier où enregistrer les fichiers.",
            StepError::Busy => "Un traitement est déjà en cours.",
            StepError::WrongStep => "Action impossible à cette étape.",
        }
    }
}

/// Données confiées au fil de travail pour l'anonymisation.
#[derive(Debug)]
pub struct AnonymizeJob {
    pub inputs: Vec<InputFile>,
    pub analysis: Analysis,
    pub never: Vec<String>,
    /// Zones tracées à la main sur les images.
    pub masks: Vec<FileMask>,
    pub protection: Protection,
    pub out_dir: PathBuf,
    /// Session de départ : table reprise, ou session courante.
    pub session: Option<Session>,
}

/// État complet de l'onglet.
#[derive(Debug)]
pub struct AnonymizeState {
    pub step: Step,
    pub files: Vec<FileEntry>,
    /// Fichiers refusés (au dépôt ou à la lecture), avec la raison.
    pub rejected: Vec<Rejected>,
    pub inputs: Vec<InputFile>,
    pub analysis: Option<Analysis>,
    pub profile: Profile,
    /// Familles affichées ; vide = toutes.
    pub family_filter: Vec<Family>,
    pub manual_term: String,
    pub manual_kind: EntityKind,
    pub manual_error: Option<ManualTermError>,
    /// Liste « ne jamais anonymiser » de cette session.
    pub never: Vec<String>,
    /// Liste « toujours anonymiser » : terme et type, cherchés à chaque analyse.
    pub always: Vec<(String, EntityKind)>,
    pub always_term: String,
    pub always_kind: EntityKind,
    pub always_error: Option<ManualTermError>,
    /// Balise prévue par candidat coché.
    pub tag_preview: HashMap<usize, Tag>,
    /// Zones tracées à la main sur les images.
    pub masks: ManualMasks,
    /// Image affichée dans l'aperçu (indice dans [`AnonymizeState::images`]).
    pub image_index: usize,
    /// Zones masquées automatiquement par image, pour la sélection courante.
    pub mask_preview: Vec<Vec<PixelRect>>,
    pub protect: ProtectState,
    pub out_dir: Option<PathBuf>,
    pub report: Option<BatchReport>,
    pub busy: Option<Busy>,
    pub error: Option<StepError>,
    /// Recherche dans le tableau (valeur et contexte).
    pub search: String,
    /// Tri du tableau ; `None` : ordre de l'analyse.
    pub sort: Option<Sort>,
    /// Ligne mise en avant (« Suivante à confirmer »), indice dans [`AnonymizeState::visible`].
    pub focus_row: Option<usize>,
    /// Faire défiler le tableau jusqu'à `focus_row` à la prochaine image.
    pub jump: bool,
    /// Table `.anonmap` déposée à l'étape Déposer, proposée à la reprise.
    pub offered_map: Option<PathBuf>,
}

/// Colonnes triables du tableau de vérification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Kind,
    Value,
    Occurrences,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub ascending: bool,
}

impl Default for AnonymizeState {
    fn default() -> Self {
        Self {
            step: Step::Drop,
            files: Vec::new(),
            rejected: Vec::new(),
            inputs: Vec::new(),
            analysis: None,
            profile: Profile::Standard,
            family_filter: Vec::new(),
            manual_term: String::new(),
            manual_kind: EntityKind::Autre,
            manual_error: None,
            never: Vec::new(),
            always: Vec::new(),
            always_term: String::new(),
            always_kind: EntityKind::Personne,
            always_error: None,
            tag_preview: HashMap::new(),
            masks: ManualMasks::default(),
            image_index: 0,
            mask_preview: Vec::new(),
            protect: ProtectState::default(),
            out_dir: None,
            report: None,
            busy: None,
            error: None,
            search: String::new(),
            sort: None,
            focus_row: None,
            jump: false,
            offered_map: None,
        }
    }
}

impl AnonymizeState {
    /// Ajoute des fichiers : doublons ignorés, fichiers produits par l'outil
    /// écartés, extensions non gérées refusées avec leur raison (`supported`
    /// vide = tout accepter, le moteur tranchera). Le dossier de sortie par
    /// défaut est le sous-dossier `Anonymisés` du premier fichier.
    pub fn add_files(
        &mut self,
        entries: impl IntoIterator<Item = FileEntry>,
        supported: &[&str],
    ) -> AddReport {
        let mut report = AddReport::default();
        for e in entries {
            if self.files.iter().any(|f| f.path == e.path) {
                continue;
            }
            if crate::ui::state::deanonymize::is_map(&e.path) {
                report.map = Some(e.path.clone());
                continue;
            }
            if is_tool_output(&e.path) {
                report.outputs += 1;
                continue;
            }
            let ext = crate::ui::service::extension(&e.path);
            if !supported.is_empty() && !supported.contains(&ext.as_str()) {
                self.rejected.retain(|r| r.name != e.name);
                self.rejected.push(Rejected {
                    name: e.name.clone(),
                    reason: unsupported_reason(&e.path),
                });
                report.refused += 1;
                continue;
            }
            if self.out_dir.is_none() {
                self.out_dir = default_output_dir(&e.path);
            }
            self.files.push(e);
            report.added += 1;
        }
        self.error = None;
        report
    }

    pub fn remove_file(&mut self, index: usize) {
        if index < self.files.len() {
            self.files.remove(index);
        }
    }

    pub fn dismiss_rejected(&mut self) {
        self.rejected.clear();
    }

    pub fn analyze_options(&self) -> AnalyzeOptions {
        AnalyzeOptions {
            detect: DetectOptions {
                profile: self.profile,
                ..DetectOptions::default()
            },
            always: self.always.clone(),
            never: self.never.clone(),
        }
    }

    /// Ajoute le terme saisi à la liste « toujours anonymiser ». Si une analyse
    /// est affichée, elle est relancée : renvoie les chemins à relire.
    pub fn add_always(&mut self) -> Result<Option<Vec<PathBuf>>, ManualTermError> {
        let term = self.always_term.trim().to_owned();
        let r = if term.is_empty() {
            Err(ManualTermError::Empty)
        } else if self
            .always
            .iter()
            .any(|(t, k)| *k == self.always_kind && t.to_lowercase() == term.to_lowercase())
        {
            Err(ManualTermError::AlreadyListed)
        } else {
            self.always.push((term, self.always_kind));
            self.always_term.clear();
            Ok(self.reanalyze())
        };
        self.always_error = r.as_ref().err().copied();
        r
    }

    /// Retire un terme de la liste « toujours anonymiser » ; relance l'analyse
    /// si elle est affichée.
    pub fn remove_always(&mut self, index: usize) -> Option<Vec<PathBuf>> {
        if index >= self.always.len() {
            return None;
        }
        self.always.remove(index);
        self.reanalyze()
    }

    /// Le type choisi n'est cherché qu'en profil Renforcé.
    pub fn always_kind_needs_renforce(&self) -> bool {
        self.profile == Profile::Standard && self.always_kind.is_quasi_identifier()
    }

    fn reanalyze(&mut self) -> Option<Vec<PathBuf>> {
        if self.analysis.is_some() && self.step == Step::Review {
            self.begin_analysis().ok()
        } else {
            None
        }
    }

    /// Lance l'analyse : renvoie les chemins à lire.
    pub fn begin_analysis(&mut self) -> Result<Vec<PathBuf>, StepError> {
        let r = if self.busy.is_some() {
            Err(StepError::Busy)
        } else if self.files.is_empty() {
            Err(StepError::NoFiles)
        } else if !matches!(self.step, Step::Drop | Step::Review) {
            Err(StepError::WrongStep)
        } else {
            self.busy = Some(Busy::Analyzing);
            Ok(self.files.iter().map(|f| f.path.clone()).collect())
        };
        self.error = r.as_ref().err().copied();
        r
    }

    /// Résultat de l'analyse. Les fichiers illisibles passent dans `rejected` ;
    /// s'il ne reste rien, on reste sur « Déposer ».
    pub fn analysis_done(
        &mut self,
        inputs: Vec<InputFile>,
        unreadable: Vec<Rejected>,
        analysis: Analysis,
        session: &Session,
    ) {
        self.busy = None;
        let mut failed = Vec::new();
        for f in &analysis.files {
            if let Some(e) = &f.error {
                failed.push(Rejected {
                    name: f.name.clone(),
                    reason: crate::ui::service::sentence(&e.to_string()),
                });
            }
        }
        for r in unreadable.into_iter().chain(failed) {
            self.rejected.retain(|x| x.name != r.name);
            self.rejected.push(r);
        }
        let ok: Vec<String> = analysis
            .files
            .iter()
            .filter(|f| f.error.is_none())
            .map(|f| f.name.clone())
            .collect();
        self.files.retain(|f| ok.contains(&f.name));
        self.inputs = inputs
            .into_iter()
            .filter(|i| ok.contains(&i.name))
            .collect();
        if analysis.cancelled || self.inputs.is_empty() {
            self.analysis = None;
            self.step = Step::Drop;
            return;
        }
        self.analysis = Some(analysis);
        self.step = Step::Review;
        let images = self.images();
        self.masks
            .retain_images(|f, m| images.iter().any(|i| i.file == f && i.name == m));
        if self.image_index >= images.len() {
            self.image_index = 0;
        }
        self.refresh_preview(session);
    }

    /// Changer de profil relance l'analyse : renvoie les chemins à relire.
    pub fn set_profile(&mut self, profile: Profile) -> Option<Vec<PathBuf>> {
        if self.profile == profile {
            return None;
        }
        self.profile = profile;
        if self.analysis.is_some() {
            self.begin_analysis().ok()
        } else {
            None
        }
    }

    pub fn refresh_preview(&mut self, session: &Session) {
        self.tag_preview = self
            .analysis
            .as_ref()
            .map(|a| a.preview_tags(session).into_iter().collect())
            .unwrap_or_default();
        self.mask_preview = self
            .analysis
            .as_ref()
            .filter(|a| a.images().next().is_some())
            .map(|a| a.mask_previews(session))
            .unwrap_or_default();
    }

    /// Images analysées (fichiers image, pages scannées, images des documents).
    pub fn images(&self) -> Vec<ImageInfo> {
        self.analysis
            .as_ref()
            .map(|a| {
                a.images()
                    .map(|i| ImageInfo {
                        file: i.file.to_owned(),
                        name: i.name.to_owned(),
                        width: i.ocr.width,
                        height: i.ocr.height,
                        low_confidence: i.low_confidence(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Zones automatiques prévues pour l'image `index`.
    pub fn auto_zones(&self, index: usize) -> &[PixelRect] {
        self.mask_preview
            .get(index)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Généralisation proposée pour cette ligne (profil Renforcé, type daté
    /// ou chiffré, valeur lisible, pas encore dans la table).
    pub fn generalization_for(&self, id: usize) -> Option<String> {
        if self.profile != Profile::Renforce {
            return None;
        }
        let c = self.candidate(id)?;
        if !is_generalizable(c.kind) || c.existing_tag.is_some() {
            return None;
        }
        generalize(c.kind, &c.value)
    }

    /// Valeur généralisée retenue pour cette ligne.
    pub fn generalized(&self, id: usize) -> Option<&str> {
        match &self.candidate(id)?.treatment {
            Treatment::Generalize(g) => Some(g.as_str()),
            Treatment::Tag => None,
        }
    }

    /// Choisit balise (`false`) ou valeur généralisée (`true`). Renvoie faux si
    /// la généralisation n'est pas possible pour cette ligne.
    pub fn set_generalized(&mut self, id: usize, on: bool, session: &Session) -> bool {
        let treatment = if on {
            match self.generalization_for(id) {
                Some(g) => Treatment::Generalize(g),
                None => return false,
            }
        } else {
            Treatment::Tag
        };
        let mut ok = false;
        self.with_analysis(session, |a| ok = a.set_treatment(id, treatment));
        ok
    }

    /// Nombre de lignes cochées dont la valeur sera généralisée.
    pub fn generalized_count(&self) -> usize {
        self.candidates()
            .iter()
            .filter(|c| c.selected && matches!(c.treatment, Treatment::Generalize(_)))
            .count()
    }

    fn candidates(&self) -> &[Candidate] {
        self.analysis
            .as_ref()
            .map(|a| a.candidates.as_slice())
            .unwrap_or(&[])
    }

    /// Identifiants des lignes visibles (filtre par famille, recherche dans
    /// la valeur et le contexte), triées si un tri est choisi.
    pub fn visible(&self) -> Vec<usize> {
        let needle = fold(self.search.trim());
        let mut rows: Vec<&Candidate> = self
            .candidates()
            .iter()
            .filter(|c| {
                self.family_filter.is_empty() || self.family_filter.contains(&c.kind.family())
            })
            .filter(|c| {
                needle.is_empty()
                    || fold(&c.value).contains(&needle)
                    || fold(&c.context).contains(&needle)
            })
            .collect();
        if let Some(sort) = self.sort {
            rows.sort_by(|a, b| {
                let o = match sort.key {
                    SortKey::Kind => a.kind.label().cmp(b.kind.label()),
                    SortKey::Value => fold(&a.value).cmp(&fold(&b.value)),
                    SortKey::Occurrences => a.occurrences.cmp(&b.occurrences),
                };
                if sort.ascending { o } else { o.reverse() }
            });
        }
        rows.into_iter().map(|c| c.id).collect()
    }

    /// Clic sur un en-tête : tri croissant, puis décroissant.
    pub fn toggle_sort(&mut self, key: SortKey) {
        self.sort = Some(match self.sort {
            Some(s) if s.key == key => Sort {
                key,
                ascending: !s.ascending,
            },
            _ => Sort {
                key,
                ascending: key != SortKey::Occurrences,
            },
        });
        self.focus_row = None;
    }

    /// Résumé du tableau : « 42 valeurs cochées sur 57 · 3 à confirmer ».
    pub fn review_summary(&self) -> String {
        let total = self.candidates().len();
        let checked = self.selected_count();
        let mut s = if total == 1 {
            format!("{checked} valeur cochée sur 1")
        } else if checked <= 1 {
            format!("{checked} valeur cochée sur {total}")
        } else {
            format!("{checked} valeurs cochées sur {total}")
        };
        let pending = self.pending_confirmations();
        if pending > 0 {
            s.push_str(&format!(" · {pending} à confirmer"));
        }
        s
    }

    /// Indice (dans [`AnonymizeState::visible`]) de la ligne à confirmer qui
    /// suit `after`, en reprenant au début.
    pub fn next_pending(&self, after: Option<usize>) -> Option<usize> {
        let vis = self.visible();
        let pending: Vec<usize> = vis
            .iter()
            .enumerate()
            .filter(|(_, id)| self.candidate(**id).is_some_and(needs_confirmation))
            .map(|(i, _)| i)
            .collect();
        let start = after.map_or(0, |a| a + 1);
        pending
            .iter()
            .copied()
            .find(|i| *i >= start)
            .or_else(|| pending.first().copied())
    }

    /// Met en avant la ligne à confirmer suivante (le tableau y défile).
    pub fn focus_next_pending(&mut self) -> Option<usize> {
        self.focus_row = self.next_pending(self.focus_row);
        self.jump = self.focus_row.is_some();
        self.focus_row
    }

    /// Raison pour laquelle l'analyse ne peut pas démarrer.
    pub fn analyze_blocker(&self) -> Option<&'static str> {
        self.files
            .is_empty()
            .then_some("Ajoutez au moins un fichier à anonymiser.")
    }

    /// Raison pour laquelle l'étape Vérifier ne peut pas être validée.
    pub fn review_blocker(&self) -> Option<&'static str> {
        (self.selected_count() == 0).then_some("Cochez au moins une valeur à anonymiser.")
    }

    /// Raison pour laquelle l'anonymisation ne peut pas démarrer.
    pub fn protect_blocker(&self) -> Option<&'static str> {
        self.protect.blocker().or_else(|| {
            self.out_dir
                .is_none()
                .then_some("Choisissez le dossier de sortie.")
        })
    }

    /// Une table déposée à l'étape Déposer : elle sera reprise à l'étape Protéger.
    pub fn accept_offered_map(&mut self) -> bool {
        let Some(path) = self.offered_map.take() else {
            return false;
        };
        self.protect.mode = ProtectMode::ExistingTable;
        self.protect.existing_path = Some(path);
        self.protect.existing = None;
        self.protect.existing_error = None;
        true
    }

    pub fn candidate(&self, id: usize) -> Option<&Candidate> {
        self.analysis.as_ref().and_then(|a| a.candidate(id))
    }

    pub fn count_by_family(&self, family: Family) -> usize {
        self.candidates()
            .iter()
            .filter(|c| c.kind.family() == family)
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

    fn with_analysis(&mut self, session: &Session, f: impl FnOnce(&mut Analysis)) {
        if let Some(a) = self.analysis.as_mut() {
            f(a);
        }
        self.refresh_preview(session);
    }

    pub fn toggle(&mut self, id: usize, session: &Session) {
        let selected = self.candidate(id).is_some_and(|c| c.selected);
        self.with_analysis(session, |a| {
            a.set_selected(id, !selected);
        });
    }

    pub fn set_kind(&mut self, id: usize, kind: EntityKind, session: &Session) {
        let generalized = self.generalized(id).is_some();
        self.with_analysis(session, |a| {
            a.set_kind(id, kind);
        });
        // La généralisation dépend du type : recalculée, ou abandonnée.
        if generalized && !self.set_generalized(id, true, session) {
            self.set_generalized(id, false, session);
        }
    }

    pub fn set_case_sensitive(&mut self, id: usize, on: bool, session: &Session) {
        self.with_analysis(session, |a| {
            a.set_case_sensitive(id, on);
        });
    }

    /// « Ne jamais anonymiser » : la ligne disparaît, la valeur rejoint la liste.
    pub fn never_anonymize(&mut self, id: usize, session: &Session) {
        let Some(value) = self.candidate(id).map(|c| c.value.clone()) else {
            return;
        };
        if !self.never.contains(&value) {
            self.never.push(value);
        }
        self.with_analysis(session, |a| {
            a.remove(id);
        });
    }

    /// État de la case d'en-tête : (cochées, visibles).
    pub fn header_state(&self) -> (usize, usize) {
        let vis = self.visible();
        let checked = vis
            .iter()
            .filter(|id| self.candidate(**id).is_some_and(|c| c.selected))
            .count();
        (checked, vis.len())
    }

    pub fn set_all_visible(&mut self, selected: bool, session: &Session) {
        let vis = self.visible();
        self.with_analysis(session, |a| {
            for id in vis {
                a.set_selected(id, selected);
            }
        });
    }

    pub fn selected_count(&self) -> usize {
        self.candidates().iter().filter(|c| c.selected).count()
    }

    pub fn pending_confirmations(&self) -> usize {
        self.candidates()
            .iter()
            .filter(|c| needs_confirmation(c))
            .count()
    }

    /// Ajoute le terme saisi ; renvoie l'identifiant de la ligne.
    pub fn add_manual(&mut self, session: &Session) -> Result<usize, ManualTermError> {
        let term = self.manual_term.trim().to_owned();
        let kind = self.manual_kind;
        let mut id = None;
        if !term.is_empty() {
            self.with_analysis(session, |a| id = a.add_manual(kind, &term));
        }
        let r = id.ok_or(ManualTermError::Empty);
        if r.is_ok() {
            self.manual_term.clear();
            let fam = kind.family();
            if !self.family_filter.is_empty() && !self.family_filter.contains(&fam) {
                self.family_filter.push(fam);
            }
        }
        self.manual_error = r.err();
        r
    }

    /// Vérifier → Protéger.
    pub fn confirm_review(&mut self) -> Result<(), StepError> {
        let r = if self.step != Step::Review {
            Err(StepError::WrongStep)
        } else if self.selected_count() == 0 {
            Err(StepError::NothingSelected)
        } else {
            self.step = Step::Protect;
            Ok(())
        };
        self.error = r.err();
        r
    }

    /// Table existante ouverte (ou erreur d'ouverture).
    pub fn existing_opened(&mut self, result: Result<Session, String>) {
        self.busy = None;
        match result {
            Ok(session) => {
                let name = self
                    .protect
                    .existing_path
                    .as_deref()
                    .map(crate::ui::service::file_name)
                    .unwrap_or_default();
                self.protect.existing = Some(ExistingTable {
                    name,
                    entries: session.entities.len(),
                    session,
                });
                self.protect.existing_error = None;
            }
            Err(msg) => {
                self.protect.existing = None;
                self.protect.existing_error = Some(msg);
            }
        }
    }

    /// Protéger → traitement : prend l'analyse et les fichiers pour le fil de travail.
    pub fn begin_anonymize(&mut self) -> Result<AnonymizeJob, StepError> {
        let r = if self.busy.is_some() {
            Err(StepError::Busy)
        } else if self.step != Step::Protect {
            Err(StepError::WrongStep)
        } else if !self.protect.can_proceed() {
            Err(StepError::PasswordNotReady)
        } else if self.out_dir.is_none() {
            Err(StepError::NoOutputDir)
        } else {
            match (self.analysis.take(), self.out_dir.clone()) {
                (Some(analysis), Some(out_dir)) => {
                    self.busy = Some(Busy::Anonymizing);
                    Ok(AnonymizeJob {
                        inputs: std::mem::take(&mut self.inputs),
                        analysis,
                        never: self.never.clone(),
                        masks: self.masks.masks.clone(),
                        protection: self.protect.protection(),
                        out_dir,
                        session: self
                            .protect
                            .existing
                            .as_ref()
                            .filter(|_| self.protect.mode == ProtectMode::ExistingTable)
                            .map(|e| e.session.clone()),
                    })
                }
                _ => Err(StepError::WrongStep),
            }
        };
        self.error = r.as_ref().err().copied();
        r
    }

    /// Résultat de l'anonymisation : l'analyse et les fichiers reviennent (pour
    /// l'export en texte d'un fichier bloqué).
    pub fn anonymize_done(
        &mut self,
        report: BatchReport,
        inputs: Vec<InputFile>,
        analysis: Analysis,
    ) {
        self.busy = None;
        self.inputs = inputs;
        self.analysis = Some(analysis);
        self.protect.clear_secrets();
        self.report = Some(report);
        self.step = Step::Done;
    }

    /// Étape précédente.
    pub fn back(&mut self) {
        if self.busy.is_some() {
            return;
        }
        self.step = match self.step {
            Step::Review => Step::Drop,
            Step::Protect => Step::Review,
            other => other,
        };
        self.error = None;
    }

    /// Recommence (les fichiers déposés sont oubliés). Le profil et la liste
    /// « toujours anonymiser » de la session sont gardés.
    pub fn reset(&mut self) {
        let profile = self.profile;
        let always = std::mem::take(&mut self.always);
        *self = Self::default();
        self.profile = profile;
        self.always = always;
    }

    /// Fichier d'entrée par nom (export du texte d'un fichier bloqué).
    pub fn input(&self, name: &str) -> Option<&InputFile> {
        self.inputs.iter().find(|i| i.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_engine::{Engine, NoProgress};

    const LETTRE: &str = "Madame Hélène Kaloï, 44 ans, a rencontré Pierre Garnier.\n\
        Contact : helene.kaloi@example.com.\nNautilus Pacifique SARL";

    fn entry(name: &str) -> FileEntry {
        FileEntry::new(PathBuf::from("C:/dossier").join(name), Some(10))
    }

    /// État en revue sur un vrai texte analysé par le moteur.
    fn reviewed(profile: Profile) -> (AnonymizeState, Session) {
        let engine = Engine::standard();
        let mut s = AnonymizeState {
            profile,
            ..Default::default()
        };
        s.add_files([entry("lettre.txt")], &["txt"]);
        let paths = s.begin_analysis();
        assert!(paths.is_ok());
        let input = InputFile::new("lettre.txt", LETTRE.as_bytes().to_vec());
        let session = Session::new();
        let analysis = engine.analyze(
            std::slice::from_ref(&input),
            &s.analyze_options(),
            &session,
            &NoProgress,
        );
        s.analysis_done(vec![input], Vec::new(), analysis, &session);
        (s, session)
    }

    #[test]
    fn unsupported_files_are_rejected_with_reason() {
        let mut s = AnonymizeState::default();
        s.add_files(
            [
                entry("a.docx"),
                entry("a.docx"),
                entry("vieux.doc"),
                entry("b.gsheet"),
            ],
            &["docx", "txt"],
        );
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.rejected.len(), 2);
        assert!(s.rejected[0].reason.contains("réenregistrez"));
        assert_eq!(
            s.out_dir,
            Some(PathBuf::from("C:/dossier").join(super::super::files::OUTPUT_DIR_NAME))
        );
        s.remove_file(0);
        assert_eq!(s.begin_analysis(), Err(StepError::NoFiles));
    }

    #[test]
    fn tool_outputs_are_never_added() {
        let mut s = AnonymizeState::default();
        let r = s.add_files(
            [
                entry("contrat_anonymise.docx"),
                entry("notes_anonymise_2.txt"),
                entry("table_anonymiseur.anonmap"),
                entry("notes_restaure.txt"),
                entry("contrat.docx"),
            ],
            &["docx", "txt"],
        );
        assert_eq!(r.added, 1);
        assert_eq!(r.outputs, 3, "la table est comptée à part");
        assert_eq!(r.refused, 0, "une sortie n'est pas un refus");
        assert!(s.rejected.is_empty());
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].name, "contrat.docx");
        assert_eq!(
            r.map,
            Some(PathBuf::from("C:/dossier/table_anonymiseur.anonmap"))
        );
        assert!(
            r.outputs_message()
                .is_some_and(|m| m.starts_with("3 fichiers déjà anonymisés ignorés"))
        );
        let one = AddReport {
            outputs: 1,
            ..Default::default()
        };
        assert!(
            one.outputs_message()
                .is_some_and(|m| m.starts_with("1 fichier déjà anonymisé ignoré"))
        );
        assert_eq!(AddReport::default().outputs_message(), None);
    }

    #[test]
    fn analysis_moves_to_review_with_previews() {
        let (s, _) = reviewed(Profile::Standard);
        assert_eq!(s.step, Step::Review);
        assert!(s.busy.is_none());
        assert!(!s.visible().is_empty());
        assert!(!s.tag_preview.is_empty());
        assert!(s.selected_count() > 0);
    }

    #[test]
    fn failed_file_stays_on_drop() {
        let engine = Engine::standard();
        let mut s = AnonymizeState::default();
        s.add_files([entry("vide.txt")], &[]);
        let _ = s.begin_analysis();
        let input = InputFile::new("vide.txt", Vec::new());
        let a = engine.analyze(
            std::slice::from_ref(&input),
            &s.analyze_options(),
            &Session::new(),
            &NoProgress,
        );
        s.analysis_done(vec![input], Vec::new(), a, &Session::new());
        assert_eq!(s.step, Step::Drop);
        assert!(s.files.is_empty());
        assert_eq!(s.rejected.len(), 1);
        assert!(s.rejected[0].reason.starts_with("Le fichier est vide"));
    }

    #[test]
    fn profile_change_requests_a_new_analysis() {
        let (mut s, _) = reviewed(Profile::Standard);
        assert_eq!(s.set_profile(Profile::Standard), None);
        let again = s.set_profile(Profile::Renforce);
        assert_eq!(again.map(|p| p.len()), Some(1));
        assert_eq!(s.busy, Some(Busy::Analyzing));
    }

    #[test]
    fn row_operations() {
        let (mut s, session) = reviewed(Profile::Renforce);
        let ids = s.visible();
        let first = ids[0];
        let before = s.candidate(first).map(|c| c.selected);
        s.toggle(first, &session);
        assert_eq!(s.candidate(first).map(|c| c.selected), before.map(|b| !b));
        s.set_kind(first, EntityKind::Matricule, &session);
        assert_eq!(
            s.candidate(first).map(|c| c.kind),
            Some(EntityKind::Matricule)
        );
        s.set_case_sensitive(first, true, &session);
        assert_eq!(s.candidate(first).map(|c| c.case_sensitive), Some(true));
        let value = s
            .candidate(first)
            .map(|c| c.value.clone())
            .unwrap_or_default();
        s.never_anonymize(first, &session);
        assert!(s.candidate(first).is_none());
        assert!(s.never.contains(&value));
        assert!(s.analyze_options().never.contains(&value));
    }

    #[test]
    fn family_filter_and_header() {
        let (mut s, session) = reviewed(Profile::Renforce);
        let all = s.visible().len();
        s.toggle_family(Family::Contacts);
        let contacts = s.visible();
        assert!(!contacts.is_empty() && contacts.len() < all);
        assert_eq!(contacts.len(), s.count_by_family(Family::Contacts));
        s.set_all_visible(false, &session);
        assert_eq!(s.header_state().0, 0);
        s.clear_filter();
        assert!(
            s.header_state().0 > 0,
            "les autres familles restent cochées"
        );
        assert_eq!(s.visible().len(), all);
    }

    #[test]
    fn manual_terms() {
        let (mut s, session) = reviewed(Profile::Standard);
        assert_eq!(s.add_manual(&session), Err(ManualTermError::Empty));
        s.manual_term = "Nautilus".into();
        s.manual_kind = EntityKind::Societe;
        s.toggle_family(Family::Personnes);
        let id = s.add_manual(&session);
        assert!(id.is_ok());
        if let Ok(id) = id {
            assert!(s.visible().contains(&id), "visible malgré le filtre");
            assert!(s.tag_preview.contains_key(&id));
        }
        assert!(s.manual_term.is_empty());
    }

    #[test]
    fn protect_rules() {
        let mut p = ProtectState::default();
        assert!(!p.can_proceed());
        p.password = "court".into();
        assert_eq!(p.password_issue(), Some(PasswordIssue::TooWeak));
        p.password = "Cheval-Agrafe-Batterie-47".into();
        assert_eq!(p.password_issue(), None);
        p.confirm = "Cheval".into();
        assert_eq!(p.confirm_issue(), Some(PasswordIssue::Mismatch));
        assert!(!p.can_proceed());
        p.confirm = p.password.clone();
        assert!(p.can_proceed());
        p.mode = ProtectMode::ExistingTable;
        assert!(!p.can_proceed(), "table non ouverte");
        p.existing = Some(ExistingTable {
            name: "t.anonmap".into(),
            session: Session::new(),
            entries: 0,
        });
        assert!(p.can_proceed());
        p.mode = ProtectMode::Irreversible;
        assert_eq!(p.protection(), Protection::Irreversible);
        p.clear_secrets();
        assert!(p.password.is_empty() && p.existing.is_none());
    }

    #[test]
    fn anonymize_job_round_trip() {
        let (mut s, _) = reviewed(Profile::Standard);
        assert_eq!(s.begin_anonymize().err(), Some(StepError::WrongStep));
        assert_eq!(s.confirm_review(), Ok(()));
        assert_eq!(s.begin_anonymize().err(), Some(StepError::PasswordNotReady));
        s.protect.mode = ProtectMode::Irreversible;
        let job = s.begin_anonymize();
        assert!(job.is_ok());
        assert_eq!(s.busy, Some(Busy::Anonymizing));
        assert!(s.analysis.is_none());
        assert_eq!(s.begin_anonymize().err(), Some(StepError::Busy));
        if let Ok(job) = job {
            assert_eq!(
                job.out_dir,
                PathBuf::from("C:/dossier").join(super::super::files::OUTPUT_DIR_NAME)
            );
            s.anonymize_done(BatchReport::default(), job.inputs, job.analysis);
        }
        assert_eq!(s.step, Step::Done);
        assert!(s.analysis.is_some());
        s.reset();
        assert_eq!(s.step, Step::Drop);
    }

    #[test]
    fn always_list() {
        let mut s = AnonymizeState::default();
        s.add_files([entry("lettre.txt")], &["txt"]);
        assert_eq!(s.add_always(), Err(ManualTermError::Empty));
        s.always_term = "  Nautilus ".into();
        s.always_kind = EntityKind::Societe;
        assert_eq!(s.add_always(), Ok(None), "pas d'analyse à relancer");
        assert!(s.always_term.is_empty());
        s.always_term = "nautilus".into();
        assert_eq!(s.add_always(), Err(ManualTermError::AlreadyListed));
        assert_eq!(s.always_error, Some(ManualTermError::AlreadyListed));
        assert_eq!(
            s.analyze_options().always,
            vec![("Nautilus".to_owned(), EntityKind::Societe)]
        );
        s.always_kind = EntityKind::Lieu;
        assert!(s.always_kind_needs_renforce());
        s.profile = Profile::Renforce;
        assert!(!s.always_kind_needs_renforce());
        // Gardée par « Anonymiser d'autres fichiers ».
        s.reset();
        assert_eq!(s.always.len(), 1);
    }

    #[test]
    fn always_list_relaunches_the_analysis() {
        let (mut s, _) = reviewed(Profile::Standard);
        s.always_term = "Pacifique".into();
        s.always_kind = EntityKind::Societe;
        let paths = s.add_always();
        assert_eq!(paths.map(|p| p.map(|v| v.len())), Ok(Some(1)));
        assert_eq!(s.busy, Some(Busy::Analyzing));
        s.busy = None;
        assert_eq!(s.remove_always(5), None);
        assert_eq!(s.remove_always(0).map(|v| v.len()), Some(1));
        assert!(s.always.is_empty());
    }

    #[test]
    fn generalization_in_renforce() {
        let (mut s, session) = reviewed(Profile::Renforce);
        let age = s
            .visible()
            .into_iter()
            .find(|id| s.candidate(*id).is_some_and(|c| c.kind == EntityKind::Age));
        let Some(age) = age else {
            panic!("âge non détecté");
        };
        assert_eq!(s.generalization_for(age).as_deref(), Some("40-49 ans"));
        assert_eq!(s.generalized(age), None);
        assert!(s.set_generalized(age, true, &session));
        assert_eq!(s.generalized(age), Some("40-49 ans"));
        assert_eq!(s.generalized_count(), 1);
        // Un type non chiffré abandonne la généralisation.
        s.set_kind(age, EntityKind::Matricule, &session);
        assert_eq!(s.generalized(age), None);
        assert!(!s.set_generalized(age, true, &session));
        // Une personne ne se généralise pas.
        let person = s.visible().into_iter().find(|id| {
            s.candidate(*id)
                .is_some_and(|c| c.kind == EntityKind::Personne)
        });
        assert_eq!(person.and_then(|p| s.generalization_for(p)), None);
    }

    #[test]
    fn no_generalization_in_standard() {
        let (s, _) = reviewed(Profile::Standard);
        assert!(
            s.visible()
                .iter()
                .all(|id| s.generalization_for(*id).is_none())
        );
    }

    #[test]
    fn text_files_have_no_images() {
        let (s, _) = reviewed(Profile::Standard);
        assert!(s.images().is_empty());
        assert!(s.auto_zones(0).is_empty());
    }

    #[test]
    fn search_sort_and_summary() {
        let (mut s, session) = reviewed(Profile::Standard);
        let all = s.visible().len();
        s.search = "KALOÏ".into();
        let found = s.visible();
        assert!(!found.is_empty() && found.len() < all);
        // Sans accents : « KALOÏ » trouve aussi l'email « helene.kaloi@… ».
        assert!(found.iter().all(|id| s.candidate(*id).is_some_and(|c| {
            fold(&c.value).contains("kaloi") || fold(&c.context).contains("kaloi")
        })));
        assert!(
            found
                .iter()
                .any(|id| s.candidate(*id).is_some_and(|c| c.value.contains("kaloi@")))
        );
        // Cocher ou décocher : seulement les lignes affichées.
        s.set_all_visible(false, &session);
        s.search.clear();
        assert_eq!(s.selected_count(), all - found.len());
        s.set_all_visible(true, &session);

        s.toggle_sort(SortKey::Value);
        let values: Vec<String> = s
            .visible()
            .iter()
            .filter_map(|id| s.candidate(*id).map(|c| fold(&c.value)))
            .collect();
        let mut sorted = values.clone();
        sorted.sort();
        assert_eq!(values, sorted);
        s.toggle_sort(SortKey::Value);
        assert_eq!(s.sort.map(|x| x.ascending), Some(false));
        s.toggle_sort(SortKey::Occurrences);
        let occ: Vec<usize> = s
            .visible()
            .iter()
            .filter_map(|id| s.candidate(*id).map(|c| c.occurrences))
            .collect();
        assert!(
            occ.windows(2).all(|w| w[0] >= w[1]),
            "plus fréquentes d'abord"
        );

        let summary = s.review_summary();
        assert!(
            summary.starts_with(&format!("{} valeurs cochées sur {all}", s.selected_count())),
            "{summary}"
        );
    }

    #[test]
    fn next_pending_cycles() {
        let (mut s, _) = reviewed(Profile::Renforce);
        let pending = s.pending_confirmations();
        if pending == 0 {
            assert_eq!(s.next_pending(None), None);
            return;
        }
        let first = s.focus_next_pending();
        assert!(first.is_some());
        let mut seen = vec![first];
        for _ in 0..pending {
            seen.push(s.focus_next_pending());
        }
        assert_eq!(seen.first(), seen.last(), "on revient au début");
        assert!(
            s.review_summary()
                .ends_with(&format!("{pending} à confirmer"))
        );
    }

    #[test]
    fn disabled_actions_say_why() {
        let mut s = AnonymizeState::default();
        assert!(s.analyze_blocker().is_some());
        s.add_files([entry("a.txt")], &["txt"]);
        assert_eq!(s.analyze_blocker(), None);
        let mut p = ProtectState::default();
        assert_eq!(
            p.blocker(),
            Some("Choisissez un mot de passe pour la table.")
        );
        p.password = "court".into();
        assert_eq!(p.blocker(), Some("Le mot de passe est trop faible."));
        p.password = "Cheval-Agrafe-Batterie-47".into();
        assert_eq!(p.blocker(), Some("Confirmez le mot de passe."));
        p.confirm = "Cheval".into();
        assert_eq!(p.blocker(), Some("Les deux mots de passe diffèrent."));
        p.confirm = p.password.clone();
        assert_eq!(p.blocker(), None);
        p.mode = ProtectMode::ExistingTable;
        assert_eq!(p.blocker(), Some("Choisissez la table à reprendre."));
        p.existing_path = Some("t.anonmap".into());
        assert_eq!(p.blocker(), Some("Ouvrez d'abord la table à reprendre."));
        p.mode = ProtectMode::Irreversible;
        assert_eq!(p.blocker(), None);
        s.protect = p;
        s.out_dir = None;
        assert_eq!(
            s.protect_blocker(),
            Some("Choisissez le dossier de sortie.")
        );
        let (mut r, session) = reviewed(Profile::Standard);
        assert_eq!(r.review_blocker(), None);
        r.set_all_visible(false, &session);
        assert_eq!(
            r.review_blocker(),
            Some("Cochez au moins une valeur à anonymiser.")
        );
    }

    #[test]
    fn dropped_map_is_offered_for_reuse() {
        let mut s = AnonymizeState::default();
        let r = s.add_files([entry("table_anonymiseur.anonmap")], &["txt"]);
        assert_eq!(r.outputs, 0);
        assert!(r.map.is_some());
        assert!(!s.accept_offered_map(), "rien de proposé");
        s.offered_map = r.map;
        assert!(s.accept_offered_map());
        assert_eq!(s.protect.mode, ProtectMode::ExistingTable);
        assert!(s.protect.existing_path.is_some());
        assert!(s.offered_map.is_none());
    }

    #[test]
    fn back_goes_one_step() {
        let (mut s, _) = reviewed(Profile::Standard);
        let _ = s.confirm_review();
        s.back();
        assert_eq!(s.step, Step::Review);
        s.back();
        assert_eq!(s.step, Step::Drop);
    }
}
