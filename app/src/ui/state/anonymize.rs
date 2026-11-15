//! Documents du dossier de travail et décisions de vérification : fichiers et
//! textes collés, analyse (toujours en profil Renforcé), valeurs cochées et
//! leur historique d'annulation, protection par la clé de restauration, bilan
//! d'écriture. Aucune dépendance à egui : les transitions se testent seules.
//! Les traitements longs sont confiés au fil de travail par `AnonApp` ; l'état
//! reçoit leurs résultats par les méthodes `*_done`.

use super::files::{FileEntry, default_output_dir, is_tool_output};
use super::generalize::{generalize, is_generalizable};
use super::history::{Action, History};
use super::images::{FileMask, ManualMasks};
use crate::ui::service::{BatchReport, Protection, Rejected, unsupported_reason};
use anon_core::{
    DetectOptions, EntityKind, Family, Goal, PixelRect, Profile, Session, Tag, Treatment,
};
use anon_engine::{Analysis, AnalyzeOptions, Candidate, InputFile, SELECT_MIN_CONFIDENCE};
use anon_vault::{PasswordStrength, password_strength};
use std::collections::HashMap;
use std::path::PathBuf;

/// Profil de détection : la V2 cherche toujours les indices (dates, âges,
/// fonctions, lieux, montants) en plus des identifiants.
pub const ANALYSIS_PROFILE: Profile = Profile::Renforce;

/// Traitement en cours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Analyzing,
    OpeningKey,
    Anonymizing,
}

impl Busy {
    pub fn label(self) -> &'static str {
        match self {
            Busy::Analyzing => "Analyse des documents",
            Busy::OpeningKey => "Ouverture de la clé",
            Busy::Anonymizing => "Masquage et vérification finale",
        }
    }
}

/// Bilan d'un ajout de fichiers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddReport {
    /// Fichiers ajoutés au dossier.
    pub added: usize,
    /// Fichiers produits par l'outil (hors clés), écartés.
    pub outputs: usize,
    /// Fichiers refusés (format non pris en charge).
    pub refused: usize,
    /// Clé `.anonmap` déposée (la dernière) : à ouvrir à l'étape Réponse.
    pub map: Option<PathBuf>,
}

impl AddReport {
    /// Message pour les fichiers produits par l'outil, s'il y en a.
    pub fn outputs_message(&self) -> Option<String> {
        match self.outputs {
            0 => None,
            1 => Some("1 fichier déjà masqué ignoré : un fichier produit par l'outil n'est pas traité une seconde fois.".to_owned()),
            n => Some(format!("{n} fichiers déjà masqués ignorés : les fichiers produits par l'outil ne sont pas traités une seconde fois.")),
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
            ManualTermError::Empty => "Saisissez un terme à masquer.",
            ManualTermError::AlreadyListed => "Ce terme est déjà dans la liste.",
        }
    }
}

/// Image analysée, telle que listée à l'écran de vérification.
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

/// Texte collé : un document du dossier, gardé en mémoire seulement
/// (jamais écrit en clair sur le disque).
#[derive(Clone, PartialEq, Eq)]
pub struct PastedText {
    /// Nom du document (`texte-colle-1.txt`), aussi nom du fichier produit.
    pub name: String,
    pub text: String,
}

impl std::fmt::Debug for PastedText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastedText")
            .field("len", &self.text.len())
            .finish_non_exhaustive()
    }
}

/// Préfixe du nom d'un texte collé.
pub const PASTED_PREFIX: &str = "texte-colle";

/// Texte collé refusé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteError {
    Empty,
}

impl PasteError {
    pub fn message(self) -> &'static str {
        match self {
            PasteError::Empty => "Le presse-papiers ne contient pas de texte.",
        }
    }
}

/// Mode de protection choisi à l'étape « Envoyer ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtectMode {
    #[default]
    NewPassword,
    /// Reprendre une clé existante : mêmes balises, même mot de passe.
    ExistingTable,
    /// Objectif « Anonymiser définitivement » : aucune clé.
    Irreversible,
}

/// Clé existante ouverte (session gardée en mémoire seulement).
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

/// Niveau minimal accepté pour chiffrer la clé.
pub fn is_acceptable(s: PasswordStrength) -> bool {
    s >= PasswordStrength::Correct
}

/// Protection de la clé de restauration (étape « Envoyer »).
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
                    Some("Choisissez un mot de passe pour la clé de restauration.")
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
                    Some("Choisissez la clé à reprendre.")
                } else if self.existing.is_none() {
                    Some("Ouvrez d'abord la clé à reprendre.")
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
    NotAnalyzed,
    NothingSelected,
    PasswordNotReady,
    NoOutputDir,
    Busy,
}

impl StepError {
    pub fn message(self) -> &'static str {
        match self {
            StepError::NoFiles => "Ajoutez au moins un document pris en charge.",
            StepError::NotAnalyzed => "Les documents ne sont pas encore analysés.",
            StepError::NothingSelected => "Cochez au moins une valeur à masquer.",
            StepError::PasswordNotReady => {
                "Choisissez un mot de passe robuste et confirmez-le, ou reprenez une clé existante."
            }
            StepError::NoOutputDir => "Choisissez le dossier où enregistrer les fichiers.",
            StepError::Busy => "Un traitement est déjà en cours.",
        }
    }
}

/// Documents à lire et à analyser : fichiers sur le disque et textes collés.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnalyzeJob {
    pub paths: Vec<PathBuf>,
    pub texts: Vec<InputFile>,
}

/// Données confiées au fil de travail pour le masquage.
#[derive(Debug)]
pub struct AnonymizeJob {
    pub inputs: Vec<InputFile>,
    pub analysis: Analysis,
    pub never: Vec<String>,
    /// Zones tracées à la main sur les images.
    pub masks: Vec<FileMask>,
    pub protection: Protection,
    pub out_dir: PathBuf,
    /// Session de départ : clé reprise, ou session courante.
    pub session: Option<Session>,
}

/// Décisions de vérification : ce que l'historique d'annulation conserve.
#[derive(Clone, PartialEq)]
pub struct Decisions {
    candidates: Vec<Candidate>,
    never: Vec<String>,
    masks: ManualMasks,
}

impl std::fmt::Debug for Decisions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decisions")
            .field("candidates", &self.candidates.len())
            .field("never", &self.never.len())
            .field("masks", &self.masks.len())
            .finish()
    }
}

/// État des documents du dossier.
#[derive(Debug)]
pub struct AnonymizeState {
    pub files: Vec<FileEntry>,
    /// Textes collés, documents du dossier au même titre que les fichiers.
    pub pasted: Vec<PastedText>,
    /// Numéro du prochain texte collé.
    next_paste: usize,
    /// Fichiers refusés (au dépôt ou à la lecture), avec la raison.
    pub rejected: Vec<Rejected>,
    pub inputs: Vec<InputFile>,
    pub analysis: Option<Analysis>,
    /// Familles affichées ; vide = toutes.
    pub family_filter: Vec<Family>,
    pub manual_term: String,
    pub manual_kind: EntityKind,
    pub manual_error: Option<ManualTermError>,
    /// Liste « ne jamais masquer » de cette session.
    pub never: Vec<String>,
    /// Liste « toujours masquer » : terme et type, cherchés à chaque analyse.
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
    /// Annuler, rétablir les décisions de vérification.
    pub history: History<Decisions>,
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
            files: Vec::new(),
            pasted: Vec::new(),
            next_paste: 1,
            rejected: Vec::new(),
            inputs: Vec::new(),
            analysis: None,
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
            history: History::default(),
        }
    }
}

impl AnonymizeState {
    /// Ajoute des fichiers : doublons ignorés, fichiers produits par l'outil
    /// écartés, extensions non gérées refusées avec leur raison (`supported`
    /// vide = tout accepter, le moteur tranchera). Une clé `.anonmap` n'est
    /// pas un document : elle est signalée dans le bilan. Le dossier de sortie
    /// par défaut est le sous-dossier `Anonymisés` du premier fichier.
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

    /// Ajoute un texte collé comme document ; renvoie son nom.
    pub fn add_pasted(&mut self, text: &str) -> Result<String, PasteError> {
        if text.trim().is_empty() {
            return Err(PasteError::Empty);
        }
        let name = format!("{PASTED_PREFIX}-{}.txt", self.next_paste);
        self.next_paste += 1;
        self.pasted.push(PastedText {
            name: name.clone(),
            text: text.to_owned(),
        });
        self.error = None;
        Ok(name)
    }

    /// Documents du dossier : fichiers et textes collés.
    pub fn documents(&self) -> usize {
        self.files.len() + self.pasted.len()
    }

    /// Noms des documents, dans l'ordre de l'analyse.
    pub fn document_names(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|f| f.name.clone())
            .chain(self.pasted.iter().map(|p| p.name.clone()))
            .collect()
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
                profile: ANALYSIS_PROFILE,
                ..DetectOptions::default()
            },
            always: self.always.clone(),
            never: self.never.clone(),
        }
    }

    /// Ajoute le terme saisi à la liste « toujours masquer ». Si une analyse
    /// est affichée, elle est relancée : renvoie les documents à relire.
    pub fn add_always(&mut self) -> Result<Option<AnalyzeJob>, ManualTermError> {
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

    /// Retire un terme de la liste « toujours masquer » ; relance l'analyse si
    /// elle est affichée.
    pub fn remove_always(&mut self, index: usize) -> Option<AnalyzeJob> {
        if index >= self.always.len() {
            return None;
        }
        self.always.remove(index);
        self.reanalyze()
    }

    fn reanalyze(&mut self) -> Option<AnalyzeJob> {
        if self.analysis.is_some() {
            self.begin_analysis().ok()
        } else {
            None
        }
    }

    /// Lance l'analyse : renvoie les documents à lire.
    pub fn begin_analysis(&mut self) -> Result<AnalyzeJob, StepError> {
        let r = if self.busy.is_some() {
            Err(StepError::Busy)
        } else if self.documents() == 0 {
            Err(StepError::NoFiles)
        } else {
            self.busy = Some(Busy::Analyzing);
            Ok(AnalyzeJob {
                paths: self.files.iter().map(|f| f.path.clone()).collect(),
                texts: self
                    .pasted
                    .iter()
                    .map(|p| InputFile::new(p.name.clone(), p.text.as_bytes().to_vec()))
                    .collect(),
            })
        };
        self.error = r.as_ref().err().copied();
        r
    }

    /// Résultat de l'analyse. Les fichiers illisibles passent dans `rejected`.
    /// Renvoie vrai si une analyse est disponible (sinon rien à vérifier).
    /// Les décisions repartent de zéro : l'historique et le bilan d'écriture
    /// sont effacés.
    pub fn analysis_done(
        &mut self,
        inputs: Vec<InputFile>,
        unreadable: Vec<Rejected>,
        analysis: Analysis,
        session: &Session,
    ) -> bool {
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
        self.pasted.retain(|p| ok.contains(&p.name));
        self.inputs = inputs
            .into_iter()
            .filter(|i| ok.contains(&i.name))
            .collect();
        self.history.clear();
        if analysis.cancelled || self.inputs.is_empty() {
            self.analysis = None;
            return false;
        }
        self.analysis = Some(analysis);
        self.report = None;
        let images = self.images();
        self.masks
            .retain_images(|f, m| images.iter().any(|i| i.file == f && i.name == m));
        if self.image_index >= images.len() {
            self.image_index = 0;
        }
        self.refresh_preview(session);
        true
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

    /// Forme moins précise proposée pour cette ligne (type daté ou chiffré,
    /// valeur lisible, pas encore dans la clé).
    pub fn generalization_for(&self, id: usize) -> Option<String> {
        let c = self.candidate(id)?;
        if !is_generalizable(c.kind) || c.existing_tag.is_some() {
            return None;
        }
        generalize(c.kind, &c.value)
    }

    /// Forme moins précise retenue pour cette ligne.
    pub fn generalized(&self, id: usize) -> Option<&str> {
        match &self.candidate(id)?.treatment {
            Treatment::Generalize(g) => Some(g.as_str()),
            Treatment::Tag => None,
        }
    }

    /// Nombre de lignes cochées dont la valeur sera rendue moins précise.
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

    /// Résumé du tableau : « 42 valeurs masquées sur 57 · 3 à confirmer ».
    pub fn review_summary(&self) -> String {
        let total = self.candidates().len();
        let checked = self.selected_count();
        let mut s = if total == 1 {
            format!("{checked} valeur masquée sur 1")
        } else if checked <= 1 {
            format!("{checked} valeur masquée sur {total}")
        } else {
            format!("{checked} valeurs masquées sur {total}")
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
        (self.documents() == 0).then_some("Ajoutez au moins un document.")
    }

    /// Raison pour laquelle la vérification ne peut pas être validée.
    pub fn review_blocker(&self) -> Option<&'static str> {
        if self.analysis.is_none() {
            Some(StepError::NotAnalyzed.message())
        } else if self.selected_count() == 0 {
            Some(StepError::NothingSelected.message())
        } else {
            None
        }
    }

    /// Raison pour laquelle le masquage ne peut pas démarrer.
    pub fn protect_blocker(&self) -> Option<&'static str> {
        self.review_blocker()
            .or_else(|| self.protect.blocker())
            .or_else(|| {
                self.out_dir
                    .is_none()
                    .then_some("Choisissez le dossier où enregistrer les fichiers.")
            })
    }

    /// Aligne la protection sur l'objectif : aucune clé pour « Anonymiser
    /// définitivement ». Le bilan d'écriture d'un autre objectif est effacé.
    pub fn apply_goal(&mut self, goal: Goal) {
        let mode = match (goal, self.protect.mode) {
            (Goal::Anonymize, _) => ProtectMode::Irreversible,
            (Goal::Mask, ProtectMode::Irreversible) => ProtectMode::NewPassword,
            (Goal::Mask, m) => m,
        };
        if mode != self.protect.mode {
            self.protect.mode = mode;
            self.protect.password.clear();
            self.protect.confirm.clear();
            self.report = None;
        }
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

    // --- Décisions de vérification, annulables ------------------------------

    fn decisions(&self) -> Option<Decisions> {
        self.analysis.as_ref().map(|a| Decisions {
            candidates: a.candidates.clone(),
            never: self.never.clone(),
            masks: self.masks.clone(),
        })
    }

    fn restore_decisions(&mut self, d: Decisions, session: &Session) {
        if let Some(a) = self.analysis.as_mut() {
            a.candidates = d.candidates;
        }
        self.never = d.never;
        self.masks = d.masks;
        self.report = None;
        self.focus_row = None;
        self.refresh_preview(session);
    }

    /// Retient l'état avant une décision. Une décision rend caduc ce qui a
    /// déjà été enregistré à l'étape Envoyer.
    fn checkpoint(&mut self, action: Action) {
        if let Some(d) = self.decisions() {
            self.history.record(action, d);
            self.report = None;
        }
    }

    /// Annule la dernière décision ; renvoie sa nature.
    pub fn undo(&mut self, session: &Session) -> Option<Action> {
        if self.busy.is_some() {
            return None;
        }
        let current = self.decisions()?;
        let (action, before) = self.history.undo(current)?;
        self.restore_decisions(before, session);
        Some(action)
    }

    /// Rétablit la dernière décision annulée ; renvoie sa nature.
    pub fn redo(&mut self, session: &Session) -> Option<Action> {
        if self.busy.is_some() {
            return None;
        }
        let current = self.decisions()?;
        let (action, after) = self.history.redo(current)?;
        self.restore_decisions(after, session);
        Some(action)
    }

    pub fn can_undo(&self) -> bool {
        self.busy.is_none() && self.analysis.is_some() && self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.busy.is_none() && self.analysis.is_some() && self.history.can_redo()
    }

    fn with_analysis(&mut self, session: &Session, f: impl FnOnce(&mut Analysis)) {
        if let Some(a) = self.analysis.as_mut() {
            f(a);
        }
        self.refresh_preview(session);
    }

    /// Coche ou décoche une valeur.
    pub fn toggle(&mut self, id: usize, session: &Session) {
        let Some(selected) = self.candidate(id).map(|c| c.selected) else {
            return;
        };
        self.set_selected(id, !selected, session);
    }

    /// Masque (`true`) ou garde visible une valeur.
    pub fn set_selected(&mut self, id: usize, selected: bool, session: &Session) {
        if self.candidate(id).is_none_or(|c| c.selected == selected) {
            return;
        }
        self.checkpoint(if selected {
            Action::Check
        } else {
            Action::Uncheck
        });
        self.with_analysis(session, |a| {
            a.set_selected(id, selected);
        });
    }

    pub fn set_kind(&mut self, id: usize, kind: EntityKind, session: &Session) {
        if self.candidate(id).is_none_or(|c| c.kind == kind) {
            return;
        }
        self.checkpoint(Action::Kind);
        let generalized = self.generalized(id).is_some();
        self.with_analysis(session, |a| {
            a.set_kind(id, kind);
        });
        // La forme moins précise dépend du type : recalculée, ou abandonnée.
        if generalized && !self.apply_generalized(id, true, session) {
            self.apply_generalized(id, false, session);
        }
    }

    pub fn set_case_sensitive(&mut self, id: usize, on: bool, session: &Session) {
        if self.candidate(id).is_none_or(|c| c.case_sensitive == on) {
            return;
        }
        self.checkpoint(Action::Case);
        self.with_analysis(session, |a| {
            a.set_case_sensitive(id, on);
        });
    }

    /// « Ne jamais masquer » : la ligne disparaît, la valeur rejoint la liste.
    pub fn never_anonymize(&mut self, id: usize, session: &Session) {
        let Some(value) = self.candidate(id).map(|c| c.value.clone()) else {
            return;
        };
        self.checkpoint(Action::RemoveValue);
        if !self.never.contains(&value) {
            self.never.push(value);
        }
        self.with_analysis(session, |a| {
            a.remove(id);
        });
    }

    /// Choisit la balise (`false`) ou la forme moins précise (`true`). Renvoie
    /// faux si la forme moins précise n'est pas possible pour cette ligne.
    pub fn set_generalized(&mut self, id: usize, on: bool, session: &Session) -> bool {
        if on && self.generalization_for(id).is_none() {
            return false;
        }
        if self.candidate(id).is_none() || self.generalized(id).is_some() == on {
            return self.candidate(id).is_some();
        }
        self.checkpoint(Action::Treatment);
        self.apply_generalized(id, on, session)
    }

    fn apply_generalized(&mut self, id: usize, on: bool, session: &Session) -> bool {
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

    /// État de la case d'en-tête : (cochées, visibles).
    pub fn header_state(&self) -> (usize, usize) {
        let vis = self.visible();
        let checked = vis
            .iter()
            .filter(|id| self.candidate(**id).is_some_and(|c| c.selected))
            .count();
        (checked, vis.len())
    }

    /// Coche ou décoche toutes les lignes affichées.
    pub fn set_all_visible(&mut self, selected: bool, session: &Session) {
        let vis = self.visible();
        let changes = vis
            .iter()
            .any(|id| self.candidate(*id).is_some_and(|c| c.selected != selected));
        if !changes {
            return;
        }
        self.checkpoint(if selected {
            Action::CheckAll
        } else {
            Action::UncheckAll
        });
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
        if !term.is_empty() && self.analysis.is_some() {
            self.checkpoint(Action::AddValue);
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

    /// Zone tracée à la main sur une image.
    pub fn add_mask(&mut self, file: &str, media: &str, rect: PixelRect) {
        if self.masks.of_image(file, media).contains(&rect) {
            return;
        }
        self.checkpoint(Action::AddZone);
        self.masks.add(file, media, rect);
    }

    /// Retire la `index`-ième zone d'une image.
    pub fn remove_mask(&mut self, file: &str, media: &str, index: usize) {
        if index >= self.masks.of_image(file, media).len() {
            return;
        }
        self.checkpoint(Action::RemoveZone);
        self.masks.remove(file, media, index);
    }

    /// Retire toutes les zones d'une image.
    pub fn clear_image_masks(&mut self, file: &str, media: &str) {
        if self.masks.of_image(file, media).is_empty() {
            return;
        }
        self.checkpoint(Action::RemoveZone);
        self.masks.clear_image(file, media);
    }

    // --- Envoi ---------------------------------------------------------------

    /// Clé existante ouverte (ou erreur d'ouverture).
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

    /// Envoyer → traitement : prend l'analyse et les documents pour le fil de travail.
    pub fn begin_anonymize(&mut self) -> Result<AnonymizeJob, StepError> {
        let r = if self.busy.is_some() {
            Err(StepError::Busy)
        } else if self.analysis.is_none() {
            Err(StepError::NotAnalyzed)
        } else if self.selected_count() == 0 {
            Err(StepError::NothingSelected)
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
                _ => Err(StepError::NotAnalyzed),
            }
        };
        self.error = r.as_ref().err().copied();
        r
    }

    /// Résultat du masquage : l'analyse et les documents reviennent (texte
    /// d'un fichier bloqué, nouvel envoi après une autre décision).
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
    }

    /// Recommence (les documents sont oubliés). La liste « toujours masquer »
    /// de la session est gardée.
    pub fn reset(&mut self) {
        let always = std::mem::take(&mut self.always);
        *self = Self::default();
        self.always = always;
    }

    /// Document d'entrée par nom (export du texte d'un fichier bloqué).
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

    /// État en vérification sur un vrai texte analysé par le moteur.
    fn reviewed() -> (AnonymizeState, Session) {
        let engine = Engine::standard();
        let mut s = AnonymizeState::default();
        s.add_files([entry("lettre.txt")], &["txt"]);
        let job = s.begin_analysis();
        assert!(job.is_ok());
        let input = InputFile::new("lettre.txt", LETTRE.as_bytes().to_vec());
        let session = Session::new();
        let analysis = engine.analyze(
            std::slice::from_ref(&input),
            &s.analyze_options(),
            &session,
            &NoProgress,
        );
        assert!(s.analysis_done(vec![input], Vec::new(), analysis, &session));
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
        assert_eq!(r.outputs, 3, "la clé est comptée à part");
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
                .is_some_and(|m| m.starts_with("3 fichiers déjà masqués ignorés"))
        );
        let one = AddReport {
            outputs: 1,
            ..Default::default()
        };
        assert!(
            one.outputs_message()
                .is_some_and(|m| m.starts_with("1 fichier déjà masqué ignoré"))
        );
        assert_eq!(AddReport::default().outputs_message(), None);
    }

    #[test]
    fn analysis_is_always_renforce() {
        let s = AnonymizeState::default();
        assert_eq!(s.analyze_options().detect.profile, Profile::Renforce);
        let (s, _) = reviewed();
        assert!(
            s.visible()
                .iter()
                .any(|id| s.candidate(*id).is_some_and(|c| c.kind == EntityKind::Age)),
            "les indices sont cherchés"
        );
    }

    #[test]
    fn analysis_gives_previews() {
        let (s, _) = reviewed();
        assert!(s.analysis.is_some());
        assert!(s.busy.is_none());
        assert!(!s.visible().is_empty());
        assert!(!s.tag_preview.is_empty());
        assert!(s.selected_count() > 0);
    }

    #[test]
    fn failed_file_leaves_nothing_to_review() {
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
        assert!(!s.analysis_done(vec![input], Vec::new(), a, &Session::new()));
        assert!(s.analysis.is_none());
        assert!(s.files.is_empty());
        assert_eq!(s.rejected.len(), 1);
        assert!(s.rejected[0].reason.starts_with("Le fichier est vide"));
    }

    #[test]
    fn pasted_text_is_a_document_of_the_folder() {
        let engine = Engine::standard();
        let mut s = AnonymizeState::default();
        assert_eq!(s.add_pasted("  \n "), Err(PasteError::Empty));
        assert_eq!(s.add_pasted(LETTRE).as_deref(), Ok("texte-colle-1.txt"));
        assert_eq!(s.documents(), 1);
        assert_eq!(s.out_dir, None, "aucun dossier deviné pour un texte collé");
        assert!(!format!("{:?}", s.pasted).contains("Kaloï"));
        let job = s.begin_analysis();
        assert!(job.as_ref().is_ok_and(|j| j.paths.is_empty()));
        let Ok(job) = job else { return };
        assert_eq!(job.texts[0].name, "texte-colle-1.txt");
        let session = Session::new();
        let a = engine.analyze(&job.texts, &s.analyze_options(), &session, &NoProgress);
        assert!(s.analysis_done(job.texts, Vec::new(), a, &session));
        assert!(s.selected_count() > 0);
        assert_eq!(s.document_names(), ["texte-colle-1.txt"]);
        assert_eq!(
            s.add_pasted("Autre texte").as_deref(),
            Ok("texte-colle-2.txt")
        );
        assert_eq!(
            s.protect_blocker(),
            Some("Choisissez un mot de passe pour la clé de restauration.")
        );
    }

    #[test]
    fn row_operations() {
        let (mut s, session) = reviewed();
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
    fn every_decision_can_be_undone_and_redone() {
        let (mut s, session) = reviewed();
        let original = s.decisions();
        let first = s.visible()[0];
        let value = s.candidate(first).map(|c| c.value.clone()).unwrap();
        let toggled = if s.candidate(first).unwrap().selected {
            Action::Uncheck
        } else {
            Action::Check
        };
        assert!(!s.can_undo() && !s.can_redo());

        let case = !s.candidate(first).unwrap().case_sensitive;
        s.toggle(first, &session);
        s.set_kind(first, EntityKind::Matricule, &session);
        s.set_case_sensitive(first, case, &session);
        s.manual_term = "Projet Corail".into();
        s.manual_kind = EntityKind::Autre;
        let added = s.add_manual(&session).unwrap();
        s.never_anonymize(first, &session);
        s.add_mask(
            "a.png",
            "image",
            PixelRect {
                x: 0,
                y: 0,
                w: 9,
                h: 9,
            },
        );
        assert_eq!(s.history.len(), 6);
        assert!(s.never.contains(&value));

        // Annuler pas à pas, dans l'ordre inverse.
        let mut undone = Vec::new();
        while let Some(a) = s.undo(&session) {
            undone.push(a);
        }
        assert_eq!(
            undone,
            [
                Action::AddZone,
                Action::RemoveValue,
                Action::AddValue,
                Action::Case,
                Action::Kind,
                toggled,
            ]
        );
        assert_eq!(s.decisions(), original, "état d'origine retrouvé");
        assert!(s.candidate(added).is_none());
        assert!(!s.never.contains(&value));
        assert!(s.masks.is_empty());

        // Tout rétablir.
        while s.redo(&session).is_some() {}
        assert!(s.candidate(first).is_none(), "retirée de nouveau");
        assert!(s.candidate(added).is_some_and(|c| c.selected));
        assert_eq!(s.masks.len(), 1);
        assert!(!s.can_redo());
    }

    #[test]
    fn treatment_and_bulk_decisions_are_undoable() {
        let (mut s, session) = reviewed();
        let age = s
            .visible()
            .into_iter()
            .find(|id| s.candidate(*id).is_some_and(|c| c.kind == EntityKind::Age))
            .unwrap();
        assert!(s.set_generalized(age, true, &session));
        assert_eq!(s.generalized(age), Some("40-49 ans"));
        assert_eq!(s.undo(&session), Some(Action::Treatment));
        assert_eq!(s.generalized(age), None);
        let checked = s.selected_count();
        s.set_all_visible(false, &session);
        assert_eq!(s.selected_count(), 0);
        // Sans changement, rien n'est retenu.
        s.set_all_visible(false, &session);
        assert_eq!(s.history.next_undo(), Some(Action::UncheckAll));
        assert_eq!(s.undo(&session), Some(Action::UncheckAll));
        assert_eq!(s.selected_count(), checked);
        assert_eq!(s.redo(&session), Some(Action::UncheckAll));
        assert_eq!(s.selected_count(), 0);
    }

    #[test]
    fn a_decision_after_writing_invalidates_the_report() {
        let (mut s, session) = reviewed();
        s.report = Some(BatchReport::default());
        let first = s.visible()[0];
        s.toggle(first, &session);
        assert!(s.report.is_none(), "l'envoi est à refaire");
    }

    #[test]
    fn a_new_analysis_clears_the_history() {
        let (mut s, session) = reviewed();
        let first = s.visible()[0];
        s.toggle(first, &session);
        assert!(s.can_undo());
        let job = s.begin_analysis().unwrap();
        assert!(!s.can_undo(), "pas d'annulation pendant l'analyse");
        let input = InputFile::new("lettre.txt", LETTRE.as_bytes().to_vec());
        let a = Engine::standard().analyze(
            std::slice::from_ref(&input),
            &s.analyze_options(),
            &session,
            &NoProgress,
        );
        assert_eq!(job.paths.len(), 1);
        s.analysis_done(vec![input], Vec::new(), a, &session);
        assert!(!s.can_undo());
    }

    #[test]
    fn family_filter_and_header() {
        let (mut s, session) = reviewed();
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
        let (mut s, session) = reviewed();
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
        assert!(!p.can_proceed(), "clé non ouverte");
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
    fn goal_sets_the_protection() {
        let mut s = AnonymizeState::default();
        s.protect.password = "x".into();
        s.apply_goal(Goal::Anonymize);
        assert_eq!(s.protect.mode, ProtectMode::Irreversible);
        assert!(s.protect.password.is_empty());
        assert_eq!(s.protect.blocker(), None, "aucun mot de passe demandé");
        s.apply_goal(Goal::Mask);
        assert_eq!(s.protect.mode, ProtectMode::NewPassword);
        s.protect.mode = ProtectMode::ExistingTable;
        s.apply_goal(Goal::Mask);
        assert_eq!(s.protect.mode, ProtectMode::ExistingTable, "choix gardé");
    }

    #[test]
    fn anonymize_job_round_trip() {
        let (mut s, _) = reviewed();
        assert_eq!(s.begin_anonymize().err(), Some(StepError::PasswordNotReady));
        s.apply_goal(Goal::Anonymize);
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
            assert_eq!(job.protection, Protection::Irreversible);
            s.anonymize_done(BatchReport::default(), job.inputs, job.analysis);
        }
        assert!(s.report.is_some());
        assert!(s.analysis.is_some());
        s.reset();
        assert!(s.analysis.is_none() && s.files.is_empty());
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
        // Gardée par « Nouvelle session ».
        s.reset();
        assert_eq!(s.always.len(), 1);
    }

    #[test]
    fn always_list_relaunches_the_analysis() {
        let (mut s, _) = reviewed();
        s.always_term = "Pacifique".into();
        s.always_kind = EntityKind::Societe;
        let job = s.add_always();
        assert_eq!(job.map(|j| j.map(|j| j.paths.len())), Ok(Some(1)));
        assert_eq!(s.busy, Some(Busy::Analyzing));
        s.busy = None;
        assert_eq!(s.remove_always(5), None);
        assert_eq!(s.remove_always(0).map(|j| j.paths.len()), Some(1));
        assert!(s.always.is_empty());
    }

    #[test]
    fn less_precise_values() {
        let (mut s, session) = reviewed();
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
        // Un type non chiffré abandonne la forme moins précise.
        s.set_kind(age, EntityKind::Matricule, &session);
        assert_eq!(s.generalized(age), None);
        assert!(!s.set_generalized(age, true, &session));
        // Une personne ne se rend pas moins précise.
        let person = s.visible().into_iter().find(|id| {
            s.candidate(*id)
                .is_some_and(|c| c.kind == EntityKind::Personne)
        });
        assert_eq!(person.and_then(|p| s.generalization_for(p)), None);
    }

    #[test]
    fn text_files_have_no_images() {
        let (s, _) = reviewed();
        assert!(s.images().is_empty());
        assert!(s.auto_zones(0).is_empty());
    }

    #[test]
    fn search_sort_and_summary() {
        let (mut s, session) = reviewed();
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
            summary.starts_with(&format!(
                "{} valeurs masquées sur {all}",
                s.selected_count()
            )),
            "{summary}"
        );
    }

    #[test]
    fn next_pending_cycles() {
        let (mut s, _) = reviewed();
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
            Some("Choisissez un mot de passe pour la clé de restauration.")
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
        assert_eq!(p.blocker(), Some("Choisissez la clé à reprendre."));
        p.existing_path = Some("t.anonmap".into());
        assert_eq!(p.blocker(), Some("Ouvrez d'abord la clé à reprendre."));
        p.mode = ProtectMode::Irreversible;
        assert_eq!(p.blocker(), None);
        let (mut r, session) = reviewed();
        r.protect = p;
        r.out_dir = None;
        assert_eq!(
            r.protect_blocker(),
            Some("Choisissez le dossier où enregistrer les fichiers.")
        );
        assert_eq!(r.review_blocker(), None);
        r.set_all_visible(false, &session);
        assert_eq!(
            r.review_blocker(),
            Some("Cochez au moins une valeur à masquer.")
        );
    }

    #[test]
    fn dropped_key_is_reported_not_added() {
        let mut s = AnonymizeState::default();
        let r = s.add_files([entry("table_anonymiseur.anonmap")], &["txt"]);
        assert_eq!(r.outputs, 0);
        assert_eq!(r.added, 0);
        assert!(r.map.is_some());
        assert!(s.files.is_empty());
    }
}
