//! Fenêtre principale : en-tête (logo, étapes du dossier, palette, thème),
//! barre du dossier (objectif, annuler, destinataire), contenu, barre d'action
//! (un seul message, l'action principale), pied, notifications, voile de
//! dépôt, palette de commandes et panneau des raccourcis. Relie l'état au
//! moteur : les traitements longs passent par le fil de travail.

use super::platform::{Platform, file_entry};
use super::prefs::{self, Preferences};
use super::runner::{Msg, Runner, Shared};
use super::screens;
use super::screens::palette::Outcome as PaletteOutcome;
use super::service::{self, anonymize_batch_tracked, read_documents, restore_batch};
use super::shortcuts::{self, Command};
use super::state::anonymize::{AddReport, AnalyzeJob, AnonymizeState, Busy};
use super::state::bilan;
use super::state::deanonymize::{DeanonymizeState, MapStatus, is_map};
use super::state::drop_hint;
use super::state::files::FileEntry;
use super::state::palette::{self as pal, PaletteState};
use super::state::toasts::{ToastKind, Toasts, count_label};
use super::state::workspace::{self, FolderFacts, Primary, Screen, Step, StepStatus, Workspace};
use super::theme::{self, CONTROL_SM, ThemeChoice, Weight, font, palette, radius, with_alpha};
use super::widgets::field::field_id;
use super::widgets::nav::{command_button, pill_menu, theme_button};
use super::widgets::{Icon, PillButton, StepBar, StepItem, toast};
use crate::moteur::IaStatus;
use anon_core::{Goal, Recipient, Session};
use anon_engine::Engine;
use egui::{
    CentralPanel, Frame, Margin, Panel, RichText, ScrollArea, Stroke, StrokeKind, Ui, vec2,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Largeur maximale du contenu : vérification et réponse (tableau, deux colonnes).
const CONTENT_WIDE: f32 = 1120.0;
/// Largeur maximale du contenu : bilan et envoi.
const CONTENT_NARROW: f32 = 960.0;
/// Hauteur de l'en-tête (`header-height` de la charte : 72 px).
const HEADER_H: f32 = 72.0;
const FOOTER_H: f32 = 40.0;
/// Barre du dossier, sous l'en-tête.
const FOLDER_BAR_H: f32 = 52.0;
/// Barre d'action du dossier, au-dessus du pied.
const ACTION_BAR_H: f32 = 64.0;
/// Marge au-dessus et au-dessous du contenu.
const CONTENT_PAD_Y: f32 = 24.0;
/// Largeur du logo dans l'en-tête.
const LOGO_W: f32 = 110.0;
/// Images attendues au plus pour recevoir le texte du presse-papiers.
const PASTE_WAIT_FRAMES: u32 = 30;
/// Intervalle entre deux images d'attente du presse-papiers, en millisecondes.
const PASTE_POLL_MS: u64 = 30;
/// Identifiant du champ de recherche du tableau de vérification.
pub const SEARCH_FIELD: &str = "recherche-detections";

/// Demandes émises par les écrans, exécutées par `AnonApp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    PickDocuments,
    TryExamples,
    Analyze(AnalyzeJob),
    /// Relance l'analyse des documents du dossier.
    Reanalyze,
    /// Le texte du presse-papiers devient un document du dossier.
    PasteDocument,
    /// Choisir une clé `.anonmap` : le dossier s'ouvre à l'étape Réponse.
    PickKey,
    Unlock,
    PickExistingMap,
    OpenExistingMap,
    PickOutputDir,
    /// Masquer et enregistrer les documents (étape Envoyer).
    Anonymize,
    ExportText(String),
    /// Copier le texte masqué des documents, avec la consigne.
    CopyTextForAi,
    PickResponses,
    PickRestoreDir,
    /// Restaurer la réponse collée (à chaque modification).
    RestoreText,
    RestoreFiles,
    /// Colle le presse-papiers dans la réponse et la restaure aussitôt.
    PasteAndRestore,
    OpenFolder(PathBuf),
    /// Copie `text` et affiche la notification `toast`.
    Copy {
        text: String,
        toast: &'static str,
    },
    /// Message d'avertissement (action impossible).
    Notice(&'static str),
    Cancel,
    NewSession,
    ShowShortcuts,
    OpenPalette,
    Go(Step),
    Next,
    Back,
    Home,
    /// Revenir au dossier ouvert depuis l'accueil.
    Resume,
    SetGoal(Goal),
    SetRecipient(Recipient),
    Undo,
    Redo,
    Find,
}

/// Destination du texte demandé au presse-papiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteTarget {
    Document,
    Response,
}

pub struct AnonApp {
    pub workspace: Workspace,
    pub anonymize: AnonymizeState,
    pub deanonymize: DeanonymizeState,
    pub palette: PaletteState,
    /// Session courante, en mémoire seulement (jamais écrite en clair).
    pub session: Session,
    pub engine: Option<Arc<Engine>>,
    pub ia: Option<IaStatus>,
    /// Notifications (coin bas droit).
    pub toasts: Toasts,
    /// Panneau « Raccourcis » ouvert.
    pub show_shortcuts: bool,
    /// Préférences (thème, destinataire, fenêtre).
    pub prefs: Preferences,
    /// Fichier des préférences ; `None` : rien n'est écrit (tests).
    prefs_path: Option<PathBuf>,
    /// Préférences telles qu'enregistrées la dernière fois.
    saved_prefs: Preferences,
    /// Thème appliqué au contexte egui.
    applied_theme: Option<ThemeChoice>,
    /// Collage demandé : images écoulées depuis la demande, et destination.
    paste_wait: Option<(u32, PasteTarget)>,
    /// Analyse demandée pendant le chargement du moteur.
    queued: Option<AnalyzeJob>,
    /// Champ à activer à l'image suivante (il n'existe pas encore à celle-ci).
    pending_focus: Option<egui::Id>,
    /// Instant de l'image en cours (secondes).
    now: f64,
    runner: Runner,
    platform: Box<dyn Platform>,
}

impl std::fmt::Debug for AnonApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnonApp")
            .field("workspace", &self.workspace)
            .field("engine", &self.engine.is_some())
            .finish_non_exhaustive()
    }
}

impl AnonApp {
    /// Installe le thème, lit les préférences, lance le chargement du moteur
    /// sur le fil de travail.
    pub fn new(ctx: &egui::Context, platform: Box<dyn Platform>) -> Self {
        let path = prefs::default_path();
        let loaded = path.as_deref().map(Preferences::load).unwrap_or_default();
        theme::install(ctx, loaded.theme);
        let mut runner = Runner::new(Some(ctx.clone()));
        runner.spawn(|_| {
            let m = crate::moteur::build();
            Msg::EngineReady(Arc::new(m.engine), m.ia)
        });
        Self::assemble(runner, platform, None, None, loaded, path)
    }

    /// Moteur déjà chargé, traitements exécutés immédiatement (tests, instantanés).
    /// Préférences par défaut, jamais enregistrées.
    pub fn with_engine(
        ctx: &egui::Context,
        platform: Box<dyn Platform>,
        engine: Arc<Engine>,
        ia: IaStatus,
    ) -> Self {
        let prefs = Preferences::default();
        theme::install(ctx, prefs.theme);
        Self::assemble(
            Runner::synchronous(),
            platform,
            Some(engine),
            Some(ia),
            prefs,
            None,
        )
    }

    fn assemble(
        runner: Runner,
        platform: Box<dyn Platform>,
        engine: Option<Arc<Engine>>,
        ia: Option<IaStatus>,
        prefs: Preferences,
        prefs_path: Option<PathBuf>,
    ) -> Self {
        let workspace = Workspace {
            recipient: prefs.recipient,
            ..Workspace::default()
        };
        let session = Session {
            recipient: prefs.recipient,
            ..Session::new()
        };
        Self {
            workspace,
            anonymize: AnonymizeState::default(),
            deanonymize: DeanonymizeState::default(),
            palette: PaletteState::default(),
            session,
            engine,
            ia,
            toasts: Toasts::default(),
            show_shortcuts: false,
            prefs,
            prefs_path,
            saved_prefs: prefs,
            applied_theme: None,
            paste_wait: None,
            queued: None,
            pending_focus: None,
            now: 0.0,
            runner,
            platform,
        }
    }

    /// Avancement du traitement en cours (affichage, tests).
    pub fn progress(&self) -> Arc<Shared> {
        Arc::clone(&self.runner.shared)
    }

    /// Un traitement de l'utilisateur est en cours (analyse, écriture, clé…).
    pub fn is_busy(&self) -> bool {
        self.anonymize.busy.is_some() || self.deanonymize.busy.is_some()
    }

    /// Ce que la navigation doit savoir du dossier.
    pub fn facts(&self) -> FolderFacts {
        FolderFacts {
            documents: self.anonymize.documents(),
            analyzed: self.anonymize.analysis.is_some(),
            selected: self.anonymize.selected_count(),
            written: self.anonymize.report.is_some(),
            key: self.deanonymize.has_key(),
            busy: self.is_busy(),
        }
    }

    /// Contexte de la palette de commandes.
    fn palette_context(&self) -> pal::Context {
        pal::Context {
            screen: self.workspace.screen,
            step: self.workspace.step,
            goal: self.workspace.goal,
            recipient: self.workspace.recipient,
            theme: self.prefs.theme,
            facts: self.facts(),
            can_undo: self.anonymize.can_undo(),
            can_redo: self.anonymize.can_redo(),
        }
    }

    /// Toutes les actions de la palette, avec leur état.
    pub fn palette_entries(&self) -> Vec<pal::Entry> {
        pal::entries(&self.palette_context())
    }

    fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(kind, text, self.now);
    }

    /// Applique le thème choisi au contexte (effectif à l'image suivante).
    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied_theme != Some(self.prefs.theme) {
            ctx.set_theme(self.prefs.theme.preference());
            self.applied_theme = Some(self.prefs.theme);
            ctx.request_repaint();
        }
    }

    /// Retient le destinataire et la taille de la fenêtre ; enregistre les
    /// préférences quand un réglage choisi par l'utilisateur change.
    fn track_prefs(&mut self, ctx: &egui::Context) {
        self.prefs.recipient = self.workspace.recipient;
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            self.prefs.window = Some([r.width(), r.height()]);
        }
        let changed = Preferences {
            window: self.saved_prefs.window,
            ..self.prefs
        } != self.saved_prefs;
        if changed {
            self.save_prefs();
        }
    }

    /// Écrit les préférences (sauf en test, sans fichier).
    pub fn save_prefs(&mut self) {
        if let Some(path) = &self.prefs_path {
            let _ = self.prefs.save(path);
        }
        self.saved_prefs = self.prefs;
    }

    fn supported(&self) -> Vec<&'static str> {
        self.engine
            .as_ref()
            .map(|e| e.supported_extensions())
            .unwrap_or_default()
    }

    /// Fichiers déposés ou choisis : documents du dossier, clé, ou fichiers à
    /// restaurer à l'étape Réponse.
    pub fn add_paths(&mut self, paths: Vec<PathBuf>) {
        self.add_entries(paths.into_iter().map(file_entry).collect());
    }

    fn add_entries(&mut self, entries: Vec<FileEntry>) {
        if entries.is_empty() {
            return;
        }
        if self.is_busy() {
            self.toast(ToastKind::Warning, workspace::BUSY);
            return;
        }
        let in_response =
            self.workspace.screen == Screen::Folder && self.workspace.step == Step::Response;
        if in_response {
            let (maps, docs): (Vec<FileEntry>, Vec<FileEntry>) =
                entries.into_iter().partition(|e| is_map(&e.path));
            if let Some(m) = maps.into_iter().last() {
                self.open_key(m.path);
            }
            let before = self.deanonymize.input_files.len();
            self.deanonymize.add_input_files(docs);
            let added = self.deanonymize.input_files.len() - before;
            if added > 0 {
                self.toast(
                    ToastKind::Success,
                    format!(
                        "{} à restaurer.",
                        count_label(added, "fichier ajouté", "fichiers ajoutés")
                    ),
                );
            }
            return;
        }
        let supported = self.supported();
        let report = self.anonymize.add_files(entries, &supported);
        self.report_added(&report);
        match (&report.map, report.added) {
            (Some(map), 0) => self.open_key(map.clone()),
            (Some(map), _) => {
                let _ = self.deanonymize.choose_map(map.clone());
            }
            _ => {}
        }
        if report.added > 0 {
            self.start_analysis();
        }
    }

    /// Une seule notification pour un ajout de fichiers.
    fn report_added(&mut self, report: &AddReport) {
        let mut parts = Vec::new();
        if report.added > 0 {
            parts.push(count_label(
                report.added,
                "fichier ajouté au dossier",
                "fichiers ajoutés au dossier",
            ));
        }
        if report.outputs > 0 {
            parts.push(count_label(
                report.outputs,
                "fichier déjà masqué ignoré",
                "fichiers déjà masqués ignorés",
            ));
        }
        if report.refused > 0 {
            parts.push(format!(
                "{} : la raison est indiquée à l'écran",
                count_label(report.refused, "fichier refusé", "fichiers refusés")
            ));
        }
        if report.map.is_some() && report.added > 0 {
            parts.push("clé de restauration choisie pour l'étape Réponse".to_owned());
        }
        if parts.is_empty() {
            return;
        }
        let kind = if report.refused > 0 || report.added == 0 {
            ToastKind::Warning
        } else {
            ToastKind::Success
        };
        let mut text = parts.join(" ; ");
        text.push('.');
        self.toast(kind, text);
    }

    /// Lance l'analyse des documents du dossier et ouvre l'étape Vérifier.
    fn start_analysis(&mut self) {
        match self.anonymize.begin_analysis() {
            Ok(job) => self.spawn_analysis(job),
            Err(e) => self.toast(ToastKind::Warning, e.message()),
        }
    }

    /// Confie l'analyse au fil de travail (ou la met en attente du moteur).
    fn spawn_analysis(&mut self, job: AnalyzeJob) {
        self.workspace.enter(Step::Verify);
        let Some(engine) = self.engine.clone() else {
            self.queued = Some(job);
            return;
        };
        let opts = self.anonymize.analyze_options();
        let session = self.session.clone();
        self.runner.spawn(move |progress| {
            let (inputs, rejected) = read_documents(&job);
            let analysis = engine.analyze(&inputs, &opts, &session, progress);
            Msg::Analyzed {
                inputs,
                rejected,
                analysis,
            }
        });
    }

    /// Choisit une clé de restauration et ouvre le dossier à l'étape Réponse.
    fn open_key(&mut self, path: PathBuf) {
        match self.deanonymize.choose_map(path) {
            Ok(()) => {
                self.workspace.enter(Step::Response);
                self.pending_focus = Some(field_id(screens::response::PASSWORD_FIELD));
            }
            Err(e) => self.toast(ToastKind::Error, e.message()),
        }
    }

    fn take_dropped_files(&mut self, ui: &Ui) {
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }
    }

    /// Résultats du fil de travail.
    pub fn poll(&mut self) {
        for msg in self.runner.poll() {
            match msg {
                Msg::EngineReady(engine, ia) => {
                    self.engine = Some(engine);
                    self.ia = Some(ia);
                    if let Some(job) = self.queued.take() {
                        self.spawn_analysis(job);
                    }
                }
                Msg::Analyzed {
                    inputs,
                    rejected,
                    analysis,
                } => {
                    let ok =
                        self.anonymize
                            .analysis_done(inputs, rejected, analysis, &self.session);
                    if ok {
                        self.workspace.enter(Step::Verify);
                    } else if self.anonymize.documents() == 0 {
                        self.workspace.home();
                        self.toast(
                            ToastKind::Warning,
                            "Aucun document n'a pu être lu : la raison est indiquée à l'écran.",
                        );
                    }
                }
                Msg::Anonymized {
                    report,
                    session,
                    inputs,
                    analysis,
                } => {
                    self.session = *session;
                    let (kind, text) = written_message(&report);
                    if let Some(map) = &report.map_path {
                        self.deanonymize
                            .open_in_memory(service::file_name(map), self.session.clone());
                        self.restore_text_now();
                    }
                    self.anonymize.anonymize_done(report, inputs, analysis);
                    self.toast(kind, text);
                }
                Msg::ExistingOpened(r) => {
                    self.anonymize
                        .existing_opened(r.map_err(|e| service::vault_message(&e)));
                }
                Msg::MapOpened(r) => {
                    let ok = r.is_ok();
                    self.deanonymize
                        .unlock_done(r.map_err(|e| service::vault_message(&e)));
                    if ok {
                        self.toast(ToastKind::Success, "Clé ouverte.");
                        self.restore_text_now();
                    }
                }
                Msg::Restored(results) => self.deanonymize.restore_files_done(results),
            }
        }
    }

    /// Raccourcis clavier de cette image.
    fn run_commands(&mut self, commands: Vec<Command>, fx: &mut Vec<Effect>) {
        for c in commands {
            if self.palette.open && !matches!(c, Command::Palette | Command::Help) {
                // egui retire le focus du champ à Échap avant cette lecture :
                // Échap arrive ici, et ferme la palette.
                if c == Command::Back {
                    self.palette.close();
                }
                continue;
            }
            match c {
                Command::Palette => {
                    self.show_shortcuts = false;
                    self.palette.toggle();
                }
                Command::Help => {
                    self.palette.close();
                    self.show_shortcuts = !self.show_shortcuts;
                }
                Command::Back => {
                    if self.is_busy() {
                        fx.push(Effect::Cancel);
                    } else if self.show_shortcuts {
                        self.show_shortcuts = false;
                    } else {
                        fx.push(Effect::Back);
                    }
                }
                Command::Open if !self.is_busy() => {
                    let in_response = self.workspace.screen == Screen::Folder
                        && self.workspace.step == Step::Response;
                    fx.push(if in_response {
                        Effect::PickResponses
                    } else {
                        Effect::PickDocuments
                    });
                }
                Command::Open => {}
                Command::Next => fx.push(Effect::Next),
                Command::Undo => fx.push(Effect::Undo),
                Command::Redo => fx.push(Effect::Redo),
                Command::PasteResponse => fx.push(Effect::PasteAndRestore),
                Command::Find => fx.push(Effect::Find),
                Command::Step(n) => {
                    if let Some(s) = Step::from_index(n) {
                        fx.push(Effect::Go(s));
                    }
                }
            }
        }
    }

    /// Action principale de l'étape (bouton de la barre d'action, Ctrl+Entrée).
    fn primary_action(&mut self, fx: &mut Vec<Effect>) {
        if self.workspace.screen == Screen::Home {
            let f = self.facts();
            if f.documents > 0 || f.key {
                fx.push(Effect::Resume);
            } else {
                self.toast(
                    ToastKind::Info,
                    "Déposez des fichiers ou collez un texte pour commencer.",
                );
            }
            return;
        }
        if let Some(why) = self.primary_blocker() {
            self.toast(ToastKind::Warning, why);
            return;
        }
        let f = self.facts();
        match workspace::primary(self.workspace.step, self.workspace.goal, &f) {
            Primary::GoTo(s) => fx.push(Effect::Go(s)),
            Primary::Write => fx.push(Effect::Anonymize),
            Primary::OpenOutput => {
                if let Some(r) = &self.anonymize.report {
                    fx.push(Effect::OpenFolder(r.out_dir.clone()));
                }
            }
            Primary::CopyRestored => {
                if let Some((text, _)) = &self.deanonymize.text_result {
                    fx.push(Effect::Copy {
                        text: text.clone(),
                        toast: "Réponse restaurée copiée",
                    });
                }
            }
        }
    }

    /// Ce qui empêche l'action principale de l'étape, en une phrase.
    fn primary_blocker(&self) -> Option<&'static str> {
        let f = self.facts();
        if f.busy {
            return Some(workspace::BUSY);
        }
        match workspace::primary(self.workspace.step, self.workspace.goal, &f) {
            Primary::GoTo(s) => workspace::blocker(s, self.workspace.goal, &f),
            Primary::Write => self.anonymize.protect_blocker(),
            Primary::OpenOutput => None,
            Primary::CopyRestored => self
                .deanonymize
                .text_result
                .is_none()
                .then_some("Collez d'abord la réponse de l'IA."),
        }
    }

    /// Libellé du bouton principal de l'étape.
    fn primary_label(&self) -> &'static str {
        match workspace::primary(self.workspace.step, self.workspace.goal, &self.facts()) {
            Primary::GoTo(Step::Response) => "Passer à la réponse",
            Primary::GoTo(_) => "Continuer",
            Primary::Write => "Masquer et enregistrer",
            Primary::OpenOutput => "Ouvrir le dossier des fichiers",
            Primary::CopyRestored => "Copier la réponse restaurée",
        }
    }

    /// Message unique de la barre d'action.
    fn status_message(&self) -> String {
        let a = &self.anonymize;
        match self.workspace.step {
            Step::Verify => match a.busy {
                Some(b) => format!("{}…", b.label()),
                None if a.analysis.is_some() => a.review_summary(),
                None => "Aucun document analysé.".to_owned(),
            },
            Step::Bilan => {
                let s = bilan::summarize(a);
                format!(
                    "Bilan provisoire : {}, {} encore visibles.",
                    count_label(s.masked, "valeur masquée", "valeurs masquées"),
                    count_label(s.clue_total(), "indice", "indices")
                )
            }
            Step::Send => match (&a.report, a.busy) {
                (_, Some(b)) => format!("{}…", b.label()),
                (None, None) => match a.protect_blocker() {
                    Some(why) => why.to_owned(),
                    None => format!(
                        "Prêt : {} à masquer et enregistrer.",
                        count_label(a.documents(), "document", "documents")
                    ),
                },
                (Some(_), None) if self.deanonymize.has_key() => format!(
                    "Quand l'IA a répondu, copiez sa réponse puis revenez ici ({}).",
                    shortcuts::PASTE_RESPONSE.label()
                ),
                (Some(_), None) => {
                    "Fichiers enregistrés sans clé : aucun retour possible.".to_owned()
                }
            },
            Step::Response => {
                let key = self.deanonymize.key_name().unwrap_or_default().to_owned();
                match (&self.deanonymize.map, &self.deanonymize.text_result) {
                    (MapStatus::Locked { .. }, _) => {
                        "Saisissez le mot de passe de la clé pour l'ouvrir.".to_owned()
                    }
                    (_, Some((_, r))) => format!(
                        "{} · Clé ouverte : {key}",
                        count_label(r.restored, "valeur restaurée", "valeurs restaurées")
                    ),
                    _ => format!(
                        "Clé ouverte : {key}. Collez la réponse de l'IA ({}).",
                        shortcuts::PASTE_RESPONSE.label()
                    ),
                }
            }
        }
    }

    /// Texte du presse-papiers demandé (« Coller un texte », « Coller la réponse »).
    fn receive_paste(&mut self, ctx: &egui::Context) {
        let Some((waited, target)) = self.paste_wait else {
            return;
        };
        let pasted = ctx.input_mut(|i| {
            let pos = i
                .events
                .iter()
                .position(|e| matches!(e, egui::Event::Paste(_)));
            pos.map(|p| i.events.remove(p))
        });
        match pasted {
            Some(egui::Event::Paste(text)) if !text.trim().is_empty() => {
                self.paste_wait = None;
                self.paste_text(target, text);
            }
            Some(_) => {
                self.paste_wait = None;
                self.toast(
                    ToastKind::Warning,
                    "Le presse-papiers ne contient pas de texte.",
                );
            }
            None if waited >= PASTE_WAIT_FRAMES => {
                self.paste_wait = None;
                self.toast(
                    ToastKind::Warning,
                    "Le presse-papiers ne contient pas de texte.",
                );
            }
            None => {
                self.paste_wait = Some((waited + 1, target));
                ctx.request_repaint_after(std::time::Duration::from_millis(PASTE_POLL_MS));
            }
        }
    }

    /// Ctrl+V sans champ actif : le texte devient un document du dossier, ou la
    /// réponse de l'IA à l'étape Réponse.
    fn take_unrequested_paste(&mut self, ctx: &egui::Context) {
        if self.paste_wait.is_some()
            || self.palette.open
            || self.show_shortcuts
            || ctx.text_edit_focused()
        {
            return;
        }
        let pasted = ctx.input_mut(|i| {
            let pos = i
                .events
                .iter()
                .position(|e| matches!(e, egui::Event::Paste(_)));
            pos.map(|p| i.events.remove(p))
        });
        let Some(egui::Event::Paste(text)) = pasted else {
            return;
        };
        let target =
            if self.workspace.screen == Screen::Folder && self.workspace.step == Step::Response {
                PasteTarget::Response
            } else {
                PasteTarget::Document
            };
        self.paste_text(target, text);
    }

    fn paste_text(&mut self, target: PasteTarget, text: String) {
        match target {
            PasteTarget::Document => {
                if self.is_busy() {
                    self.toast(ToastKind::Warning, workspace::BUSY);
                    return;
                }
                match self.anonymize.add_pasted(&text) {
                    Ok(name) => {
                        self.toast(
                            ToastKind::Success,
                            format!("Texte collé ajouté au dossier : {name}."),
                        );
                        self.start_analysis();
                    }
                    Err(e) => self.toast(ToastKind::Warning, e.message()),
                }
            }
            PasteTarget::Response => {
                self.deanonymize.pasted = text;
                if !self.deanonymize.is_unlocked() {
                    self.toast(
                        ToastKind::Info,
                        "Réponse collée : ouvrez la clé pour la restaurer.",
                    );
                    return;
                }
                self.restore_text_now();
                if let Some((_, r)) = &self.deanonymize.text_result {
                    let msg = format!(
                        "Réponse collée : {}.",
                        count_label(r.restored, "balise restaurée", "balises restaurées")
                    );
                    self.toast(ToastKind::Success, msg);
                }
            }
        }
    }

    /// Demande le texte du presse-papiers (il arrive à une image suivante).
    fn request_paste(&mut self, ctx: &egui::Context, target: PasteTarget) {
        // Le texte collé ne doit pas aller dans un champ qui aurait le focus.
        ctx.memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        self.paste_wait = Some((0, target));
        ctx.request_repaint();
    }

    /// Restaure la réponse collée, si une clé est ouverte et qu'il y a un texte.
    fn restore_text_now(&mut self) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if self.deanonymize.is_unlocked() {
            let _ = self
                .deanonymize
                .restore_text(|t, s| engine.restore_text(t, s));
        }
    }

    fn go(&mut self, step: Step) {
        let f = self.facts();
        match self.workspace.go(step, &f) {
            Ok(()) => self.entered(step),
            Err(why) => self.toast(ToastKind::Warning, why),
        }
    }

    /// Arrivée sur une étape : le premier champ utile prend le focus.
    fn entered(&mut self, step: Step) {
        self.pending_focus = match step {
            Step::Send
                if self.anonymize.report.is_none()
                    && self.workspace.goal == Goal::Mask
                    && self.anonymize.protect.password.is_empty() =>
            {
                Some(field_id(screens::send::PASSWORD_FIELD))
            }
            Step::Response if matches!(self.deanonymize.map, MapStatus::Locked { .. }) => {
                Some(field_id(screens::response::PASSWORD_FIELD))
            }
            _ => None,
        };
    }

    fn apply(&mut self, ui: &Ui, effects: Vec<Effect>) {
        for e in effects {
            self.apply_one(ui, e);
        }
    }

    fn apply_one(&mut self, ui: &Ui, effect: Effect) {
        match effect {
            Effect::PickDocuments => {
                if self.is_busy() {
                    return;
                }
                let files = self.platform.pick_documents();
                self.add_entries(files);
            }
            Effect::TryExamples => self.try_examples(),
            Effect::Analyze(job) => self.spawn_analysis(job),
            Effect::Reanalyze => self.start_analysis(),
            Effect::PasteDocument => {
                if self.is_busy() {
                    self.toast(ToastKind::Warning, workspace::BUSY);
                } else {
                    self.request_paste(ui.ctx(), PasteTarget::Document);
                }
            }
            Effect::PickKey => {
                if let Some(path) = self.platform.pick_map() {
                    self.open_key(path);
                }
            }
            Effect::Unlock => {
                if let Ok((path, password)) = self.deanonymize.begin_unlock() {
                    self.runner.spawn(move |_| {
                        Msg::MapOpened(match std::fs::read(&path) {
                            Ok(bytes) => anon_vault::open(&bytes, &password),
                            Err(_) => Err(anon_vault::VaultError::NotAVault),
                        })
                    });
                }
            }
            Effect::PickExistingMap => {
                if let Some(path) = self.platform.pick_map() {
                    self.anonymize.protect.existing_path = Some(path);
                    self.anonymize.protect.existing = None;
                    self.anonymize.protect.existing_error = None;
                }
            }
            Effect::OpenExistingMap => {
                let Some(path) = self.anonymize.protect.existing_path.clone() else {
                    return;
                };
                let password = self.anonymize.protect.password.clone();
                self.anonymize.busy = Some(Busy::OpeningKey);
                self.runner.spawn(move |_| {
                    Msg::ExistingOpened(match std::fs::read(&path) {
                        Ok(bytes) => anon_vault::open(&bytes, &password),
                        Err(_) => Err(anon_vault::VaultError::NotAVault),
                    })
                });
            }
            Effect::PickOutputDir => {
                if let Some(dir) = self.platform.pick_folder(self.anonymize.out_dir.as_deref()) {
                    self.anonymize.out_dir = Some(dir);
                }
            }
            Effect::Anonymize => self.anonymize_now(),
            Effect::ExportText(name) => self.export_text(&name),
            Effect::CopyTextForAi => self.copy_text_for_ai(ui.ctx()),
            Effect::PickResponses => {
                let files = self.platform.pick_responses();
                let n = files.len();
                self.deanonymize.add_input_files(files);
                if n > 0 {
                    self.toast(
                        ToastKind::Success,
                        format!(
                            "{} à restaurer.",
                            count_label(n, "fichier ajouté", "fichiers ajoutés")
                        ),
                    );
                }
            }
            Effect::PickRestoreDir => {
                if let Some(dir) = self
                    .platform
                    .pick_folder(self.deanonymize.out_dir.as_deref())
                {
                    self.deanonymize.out_dir = Some(dir);
                }
            }
            Effect::RestoreText => self.restore_text_now(),
            Effect::RestoreFiles => self.restore_files(),
            Effect::PasteAndRestore => {
                let f = self.facts();
                if let Some(why) = workspace::blocker(Step::Response, self.workspace.goal, &f) {
                    self.toast(ToastKind::Warning, why);
                    return;
                }
                self.workspace.enter(Step::Response);
                if let Some(why) = self.deanonymize.paste_blocker() {
                    self.entered(Step::Response);
                    self.toast(ToastKind::Warning, why);
                    return;
                }
                self.request_paste(ui.ctx(), PasteTarget::Response);
            }
            Effect::OpenFolder(dir) => self.platform.open_folder(&dir),
            Effect::Copy { text, toast } => {
                ui.ctx().copy_text(text);
                self.toast(ToastKind::Success, toast);
            }
            Effect::Notice(msg) => self.toast(ToastKind::Warning, msg),
            Effect::Cancel => self.runner.shared.request_cancel(),
            Effect::NewSession => self.new_session(),
            Effect::ShowShortcuts => {
                self.palette.close();
                self.show_shortcuts = true;
            }
            Effect::OpenPalette => {
                self.show_shortcuts = false;
                self.palette.open();
            }
            Effect::Go(step) => self.go(step),
            Effect::Next => {
                let mut fx = Vec::new();
                self.primary_action(&mut fx);
                self.apply(ui, fx);
            }
            Effect::Back => {
                let f = self.facts();
                self.workspace.back(&f);
            }
            Effect::Home => self.workspace.home(),
            Effect::Resume => {
                let f = self.facts();
                let step = if f.documents == 0 && !f.analyzed && f.key {
                    Step::Response
                } else {
                    self.workspace.step
                };
                self.workspace.enter(step);
                self.entered(step);
            }
            Effect::SetGoal(g) => {
                self.workspace.goal = g;
                self.anonymize.apply_goal(g);
            }
            Effect::SetRecipient(r) => {
                self.workspace.recipient = r;
                self.session.recipient = r;
            }
            Effect::Undo => match self.anonymize.undo(&self.session) {
                Some(a) => self.toast(ToastKind::Info, format!("Annulé : {}.", a.label())),
                None => self.toast(ToastKind::Info, "Aucune décision à annuler."),
            },
            Effect::Redo => match self.anonymize.redo(&self.session) {
                Some(a) => self.toast(ToastKind::Info, format!("Rétabli : {}.", a.label())),
                None => self.toast(ToastKind::Info, "Aucune décision à rétablir."),
            },
            Effect::Find => {
                if self.anonymize.analysis.is_some() {
                    self.go(Step::Verify);
                    self.pending_focus = Some(field_id(SEARCH_FIELD));
                }
            }
        }
    }

    /// Masque et enregistre les documents (étape Envoyer).
    fn anonymize_now(&mut self) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if let Some(why) = self.anonymize.protect_blocker() {
            self.toast(ToastKind::Warning, why);
            return;
        }
        let Ok(job) = self.anonymize.begin_anonymize() else {
            return;
        };
        let mut session = job.session.unwrap_or_else(|| self.session.clone());
        session.recipient = self.workspace.recipient;
        self.runner.spawn(move |progress| {
            let mut session = session;
            let report = anonymize_batch_tracked(
                &engine,
                &job.inputs,
                &job.analysis,
                &job.never,
                &job.masks,
                &mut session,
                &job.protection,
                &job.out_dir,
                progress,
                &|i| progress.mark_failed(i),
            );
            drop(job.protection);
            Msg::Anonymized {
                report,
                session: Box::new(session),
                inputs: job.inputs,
                analysis: job.analysis,
            }
        });
    }

    fn restore_files(&mut self) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if let Ok(job) = self.deanonymize.begin_restore_files() {
            self.runner.spawn(move |progress| {
                let (inputs, unreadable) = service::read_inputs(&job.paths);
                let mut results =
                    restore_batch(&engine, &inputs, &job.session, &job.out_dir, progress);
                results.extend(unreadable.into_iter().map(|r| service::RestoreOutcome {
                    name: r.name,
                    result: Err(r.reason),
                }));
                Msg::Restored(results)
            });
        } else if let Some(why) = self.deanonymize.files_blocker() {
            self.toast(ToastKind::Warning, why);
        }
    }

    fn copy_text_for_ai(&mut self, ctx: &egui::Context) {
        let (Some(engine), Some(report)) = (self.engine.clone(), self.anonymize.report.as_ref())
        else {
            return;
        };
        let (text, n) =
            service::text_for_ai(&engine, &self.anonymize.inputs, report, &self.session);
        if n == 0 {
            self.toast(
                ToastKind::Warning,
                "Aucun texte à copier : joignez les fichiers à la conversation.",
            );
            return;
        }
        ctx.copy_text(text);
        self.toast(
            ToastKind::Success,
            format!(
                "Texte et consigne copiés : {}.",
                count_label(n, "document", "documents")
            ),
        );
    }

    /// Oublie le dossier, la clé en mémoire et les listes de la session.
    fn new_session(&mut self) {
        self.session = Session {
            recipient: self.workspace.recipient,
            ..Session::new()
        };
        self.anonymize.reset();
        self.anonymize.always.clear();
        self.deanonymize.lock();
        self.workspace = Workspace {
            recipient: self.workspace.recipient,
            ..Workspace::default()
        };
        self.palette.close();
        self.queued = None;
        self.toast(
            ToastKind::Info,
            "Nouvelle session : le dossier et la clé en mémoire ont été oubliés.",
        );
    }

    /// Charge les exemples fictifs, en un clic : toujours le même dossier
    /// (`Documents/Anonymiseur/Exemples`), réutilisé ; dialogue seulement si le
    /// dossier Documents est introuvable.
    fn try_examples(&mut self) {
        if self.is_busy() {
            return;
        }
        let dir = match self.platform.examples_dir() {
            Some(d) => d,
            None => match self.platform.pick_examples_folder() {
                Some(parent) => parent.join(service::EXAMPLES_FALLBACK_DIR),
                None => return,
            },
        };
        self.load_examples(&dir);
    }

    /// Prépare les exemples dans `dir` (fichiers manquants recréés, rien
    /// d'écrasé), les ajoute au dossier et lance l'analyse.
    pub fn load_examples(&mut self, dir: &std::path::Path) {
        match service::prepare_examples(dir) {
            Ok(ready) => {
                let supported = self.supported();
                let total = ready.documents.len();
                let report = self
                    .anonymize
                    .add_files(ready.documents.into_iter().map(file_entry), &supported);
                let msg = if report.added == 0 && total > 0 {
                    "Exemples prêts : ils sont déjà dans le dossier.".to_owned()
                } else {
                    format!(
                        "Exemples prêts : {} fictifs, dans {}.",
                        count_label(report.added, "document", "documents"),
                        examples_place(dir)
                    )
                };
                self.toast(ToastKind::Success, msg);
                if report.added > 0 {
                    self.start_analysis();
                } else if self.anonymize.analysis.is_some() {
                    self.workspace.enter(Step::Verify);
                }
            }
            Err(_) => self.toast(
                ToastKind::Error,
                "Préparation des exemples impossible : le dossier Documents n'est pas accessible en écriture.",
            ),
        }
    }

    /// Texte masqué d'un fichier bloqué par la vérification finale, en `.md`.
    fn export_text(&mut self, name: &str) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let Some(input) = self.anonymize.input(name).cloned() else {
            return;
        };
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        let suggested = format!("{stem}_anonymise.md");
        let start = self.anonymize.report.as_ref().map(|r| r.out_dir.clone());
        let Some(path) = self.platform.save_markdown(start.as_deref(), &suggested) else {
            return;
        };
        let (kind, msg) = match engine.export_text(&input, &self.session) {
            Ok(text) => match std::fs::write(&path, text) {
                Ok(()) => (
                    ToastKind::Success,
                    format!("Texte masqué enregistré : {}.", service::file_name(&path)),
                ),
                Err(_) => (
                    ToastKind::Error,
                    "Écriture impossible à cet endroit.".to_owned(),
                ),
            },
            Err(e) => (ToastKind::Error, service::sentence(&e.to_string())),
        };
        self.toast(kind, msg);
    }

    // --- Rendu ---------------------------------------------------------------

    fn header(&mut self, ui: &mut Ui, fx: &mut Vec<Effect>) {
        let p = palette();
        Panel::top("entete")
            .exact_size(HEADER_H)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(24, 0))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect();
                let logo = if p.dark_mode {
                    egui::include_image!("../../../assets/charte/logo-white-color.png")
                } else {
                    egui::include_image!("../../../assets/charte/logo-color.png")
                };
                let palette_key = shortcuts::PALETTE.label();
                // Étapes centrées, si le dossier est ouvert.
                let items = self.step_items();
                let bar = StepBar::new(&items);
                let bar_w = if self.workspace.screen == Screen::Folder {
                    bar.width(ui)
                } else {
                    0.0
                };
                let left_edge = full.min.x + LOGO_W + p.space(4);
                let right_room =
                    |cmd_w: f32| full.max.x - (CONTROL_SM + 8.0 + p.space(3) + cmd_w + p.space(4));
                let compact = bar_w > 0.0 && right_room(220.0) - left_edge < bar_w;
                ui.horizontal_centered(|ui| {
                    ui.add(
                        egui::Image::new(logo)
                            .fit_to_exact_size(vec2(LOGO_W, 36.0))
                            .alt_text("Skazy Formation"),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = p.space(3);
                        theme_button(ui, &mut self.prefs.theme);
                        if command_button(ui, &palette_key, compact).clicked() {
                            fx.push(Effect::OpenPalette);
                        }
                    });
                });
                if self.workspace.screen == Screen::Folder {
                    let cmd_w = if compact { 100.0 } else { 220.0 };
                    let right_edge = right_room(cmd_w);
                    let x = (full.center().x - bar_w / 2.0)
                        .clamp(left_edge, (right_edge - bar_w).max(left_edge));
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, full.center().y - CONTROL_SM / 2.0),
                        vec2(bar_w + 1.0, CONTROL_SM),
                    );
                    let clicked = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| bar.show(ui))
                        .inner;
                    if let Some(i) = clicked
                        && let Some(s) = Step::from_index(i)
                    {
                        fx.push(Effect::Go(s));
                    }
                }
            });
    }

    /// Étapes de l'indicateur de l'en-tête.
    fn step_items(&self) -> Vec<StepItem<'static>> {
        let f = self.facts();
        Step::ALL
            .iter()
            .map(|s| {
                let status = self.workspace.status(*s);
                let blocked = if status == StepStatus::Current {
                    None
                } else {
                    workspace::blocker(*s, self.workspace.goal, &f)
                };
                StepItem {
                    label: s.label(),
                    // Une étape sautée (clé ouverte sans document) n'est pas « faite ».
                    done: status == StepStatus::Done && blocked.is_none(),
                    current: status == StepStatus::Current,
                    blocked,
                    shortcut: None,
                }
            })
            .collect()
    }

    /// Barre du dossier : documents, objectif, annuler, destinataire.
    fn folder_bar(&mut self, ui: &mut Ui, fx: &mut Vec<Effect>) {
        let p = palette();
        Panel::top("barre-dossier")
            .exact_size(FOLDER_BAR_H)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(24, 0))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = p.space(3);
                    ui.label(
                        RichText::new("Dossier")
                            .font(font(Weight::Bold, p.text_base))
                            .color(p.heading),
                    );
                    let docs = self.anonymize.documents();
                    let detail = if docs == 0 && self.deanonymize.has_key() {
                        "clé de restauration seule".to_owned()
                    } else {
                        count_label(docs, "document", "documents")
                    };
                    ui.label(
                        RichText::new(detail)
                            .font(font(Weight::Regular, p.text_sm))
                            .color(p.text_secondary),
                    );
                    let goals: Vec<(Goal, &str)> =
                        Goal::ALL.iter().map(|g| (*g, g.label())).collect();
                    if let Some(g) = pill_menu(ui, "Objectif", self.workspace.goal, &goals) {
                        fx.push(Effect::SetGoal(g));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = p.space(3);
                        let recipients: Vec<(Recipient, &str)> =
                            Recipient::ALL.iter().map(|r| (*r, r.label())).collect();
                        if let Some(r) =
                            pill_menu(ui, "Pour", self.workspace.recipient, &recipients)
                        {
                            fx.push(Effect::SetRecipient(r));
                        }
                        // Pendant un traitement, aucune décision : le seul
                        // « Annuler » visible est celui du traitement. Sans
                        // document analysé, rien à annuler.
                        if self.is_busy() || self.anonymize.analysis.is_none() {
                            return;
                        }
                        let undo_key = shortcuts::UNDO.label();
                        let can = self.anonymize.can_undo();
                        let r = ui.add(
                            PillButton::ghost("Annuler")
                                .icon(Icon::Undo)
                                .small()
                                .kbd(&undo_key)
                                .enabled(can),
                        );
                        let r = if can {
                            let what = self
                                .anonymize
                                .history
                                .next_undo()
                                .map(|a| a.label())
                                .unwrap_or_default();
                            r.on_hover_text(format!("Annuler : {what}"))
                        } else {
                            r.on_hover_text("Aucune décision à annuler.")
                        };
                        if r.clicked() {
                            fx.push(Effect::Undo);
                        }
                    });
                });
            });
    }

    /// Barre d'action du dossier : un seul message, retour, action principale.
    fn action_bar(&mut self, ui: &mut Ui, fx: &mut Vec<Effect>) {
        let p = palette();
        let message = self.status_message();
        let blocker = self.primary_blocker();
        let label = self.primary_label();
        let next_key = shortcuts::NEXT.label();
        let busy = self.is_busy();
        Panel::bottom("barre-action")
            .exact_size(ACTION_BAR_H)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(24, 0))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    Icon::Info.show(ui, p.accent, 18.0);
                    let w = (ui.available_width() - 460.0).max(160.0);
                    screens::truncated_text(
                        ui,
                        w,
                        CONTROL_SM,
                        RichText::new(&message)
                            .font(font(Weight::Medium, p.text_sm))
                            .color(p.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = p.space(3);
                        let go = screens::guarded_button(
                            ui,
                            PillButton::primary(label).kbd(&next_key),
                            blocker,
                        );
                        if go.clicked() {
                            fx.push(Effect::Next);
                        }
                        let back = if self.workspace.step == Step::Verify {
                            "Accueil"
                        } else {
                            "Retour"
                        };
                        if ui
                            .add(PillButton::ghost(back).enabled(!busy))
                            .on_hover_text(shortcuts::with_shortcut(
                                "Étape précédente",
                                shortcuts::BACK,
                            ))
                            .clicked()
                        {
                            fx.push(Effect::Back);
                        }
                    });
                });
            });
    }

    fn footer(&mut self, ui: &mut Ui, effects: &mut Vec<Effect>) {
        let p = palette();
        Panel::bottom("pied")
            .exact_size(FOOTER_H)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(24, 0))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.set_min_height(CONTROL_SM);
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    Icon::ShieldCheck.show(ui, p.accent, 16.0);
                    let small = |t: &str| {
                        RichText::new(t)
                            .font(font(Weight::Regular, p.text_xs))
                            .color(p.text_secondary)
                    };
                    ui.label(small("Tout reste sur votre ordinateur, aucune connexion"));
                    ui.label(small("·"));
                    let ia = self
                        .ia
                        .map(IaStatus::label)
                        .unwrap_or("Chargement du moteur…");
                    ui.label(small(ia));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let busy = self.is_busy();
                        let r = ui
                            .add(
                                PillButton::ghost("Nouvelle session")
                                    .icon(Icon::RefreshCw)
                                    .small()
                                    .enabled(!busy),
                            )
                            .on_hover_text(if busy {
                                "Attendez la fin du traitement en cours."
                            } else {
                                "Oublie le dossier et la clé en mémoire, et recommence à zéro."
                            });
                        if r.clicked() {
                            effects.push(Effect::NewSession);
                        }
                        let keys = format!("Raccourcis ({})", shortcuts::HELP.label());
                        if ui
                            .add(
                                PillButton::ghost(&keys)
                                    .icon(Icon::Keyboard)
                                    .small()
                                    .accessible_label("Raccourcis clavier"),
                            )
                            .on_hover_text("Liste des raccourcis clavier")
                            .clicked()
                        {
                            effects.push(Effect::ShowShortcuts);
                        }
                    });
                });
            });
    }

    /// Voile sur toute la fenêtre quand des fichiers la survolent.
    fn drop_overlay(&self, ui: &Ui) {
        let (paths, count) = ui.input(|i| {
            (
                i.raw
                    .hovered_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect::<Vec<_>>(),
                i.raw.hovered_files.len(),
            )
        });
        if count == 0 {
            return;
        }
        let p = palette();
        let hint = drop_hint(
            self.workspace.screen,
            self.workspace.step,
            self.is_busy(),
            &paths,
            count,
        );
        let screen = ui.ctx().content_rect();
        egui::Area::new(egui::Id::unique("voile-depot"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(rect, 0.0, with_alpha(p.page, 0.95));
                painter.rect_stroke(
                    rect.shrink(16.0),
                    radius(p.radius_xl),
                    Stroke::new(2.0, p.accent),
                    StrokeKind::Inside,
                );
                let c = rect.center();
                painter.circle_filled(c - vec2(0.0, 40.0), 32.0, p.primary_subtle);
                Icon::Upload.paint(
                    ui,
                    egui::Rect::from_center_size(c - vec2(0.0, 40.0), vec2(32.0, 32.0)),
                    p.accent,
                );
                ui.put(
                    egui::Rect::from_center_size(
                        c + vec2(0.0, 24.0),
                        vec2(rect.width() - 96.0, 40.0),
                    ),
                    egui::Label::new(
                        RichText::new(&hint)
                            .font(font(Weight::Semibold, p.text_lg))
                            .color(p.heading),
                    ),
                );
            });
    }

    /// Panneau « Raccourcis clavier ».
    fn shortcuts_panel(&mut self, ui: &Ui) {
        let p = palette();
        let modal = egui::Modal::new(egui::Id::unique("raccourcis"))
            .backdrop_color(with_alpha(egui::Color32::BLACK, 0.4))
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(radius(p.radius_xl))
                    .shadow(p.shadow_xl)
                    .inner_margin(Margin::same(24)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_width(560.0);
                ui.spacing_mut().item_spacing.y = p.space(3);
                ui.horizontal(|ui| {
                    Icon::Keyboard.show(ui, p.accent, 20.0);
                    ui.label(
                        RichText::new("Raccourcis clavier")
                            .font(font(Weight::Semibold, p.text_md))
                            .color(p.heading),
                    );
                });
                egui::Grid::new("liste-raccourcis")
                    .num_columns(2)
                    .spacing(vec2(p.space(4), p.space(3)))
                    .show(ui, |ui| {
                        for (keys, action) in shortcuts::help_rows() {
                            super::widgets::keycap(ui, &keys);
                            ui.label(
                                RichText::new(action)
                                    .font(font(Weight::Regular, p.text_sm))
                                    .color(p.text),
                            );
                            ui.end_row();
                        }
                    });
                let mut close = false;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.add(PillButton::secondary("Fermer").small()).clicked();
                });
                close
            });
        if modal.inner || modal.should_close() {
            self.show_shortcuts = false;
        }
    }

    /// Palette de commandes ; renvoie l'effet de l'action choisie.
    fn command_palette(&mut self, ui: &Ui, fx: &mut Vec<Effect>) {
        let list = self.palette_entries();
        match screens::palette::show(ui, &mut self.palette, &list) {
            PaletteOutcome::None => {}
            PaletteOutcome::Close => self.palette.close(),
            PaletteOutcome::Blocked(why) => self.toast(ToastKind::Warning, why),
            PaletteOutcome::Run(action) => {
                self.palette.close();
                self.run_palette_action(action, fx);
            }
        }
    }

    fn run_palette_action(&mut self, action: pal::Action, fx: &mut Vec<Effect>) {
        use pal::Action as A;
        match action {
            A::GoTo(s) => fx.push(Effect::Go(s)),
            A::Next => fx.push(Effect::Next),
            A::Home => fx.push(Effect::Home),
            A::AddFiles => fx.push(Effect::PickDocuments),
            A::PasteText => fx.push(Effect::PasteDocument),
            A::OpenKey => fx.push(Effect::PickKey),
            A::Examples => fx.push(Effect::TryExamples),
            A::SetGoal(g) => fx.push(Effect::SetGoal(g)),
            A::SetRecipient(r) => fx.push(Effect::SetRecipient(r)),
            A::Theme(t) => self.prefs.theme = t,
            A::Undo => fx.push(Effect::Undo),
            A::Redo => fx.push(Effect::Redo),
            A::PasteResponse => fx.push(Effect::PasteAndRestore),
            A::CopyInstruction => fx.push(Effect::Copy {
                text: anon_engine::ai_instruction().to_owned(),
                toast: "Consigne copiée",
            }),
            A::Find => fx.push(Effect::Find),
            A::Shortcuts => fx.push(Effect::ShowShortcuts),
            A::NewSession => fx.push(Effect::NewSession),
        }
    }

    /// Contenu de l'écran courant.
    fn content(&mut self, ui: &mut Ui, view_h: f32, fx: &mut Vec<Effect>) {
        let p = palette();
        let shared = Arc::clone(&self.runner.shared);
        if self.workspace.screen == Screen::Home {
            let f = self.facts();
            let info = screens::home::HomeInfo {
                goal: self.workspace.goal,
                documents: f.documents,
                resumable: f.documents > 0 || f.key,
                busy: f.busy,
            };
            screens::home::show(ui, &mut self.anonymize, info, view_h, fx);
            return;
        }
        let max = match self.workspace.step {
            Step::Verify | Step::Response => CONTENT_WIDE,
            Step::Bilan | Step::Send => CONTENT_NARROW,
        };
        let side = p.space(6);
        let avail = ui.available_width();
        let width = (avail - 2.0 * side).min(max);
        let left = ((avail - width) / 2.0).max(side);
        ui.horizontal(|ui| {
            ui.add_space(left);
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = p.space(4);
                match self.workspace.step {
                    Step::Verify => screens::verify::show(
                        ui,
                        &mut self.anonymize,
                        &self.session,
                        &shared,
                        view_h,
                        fx,
                    ),
                    Step::Bilan => {
                        let s = bilan::summarize(&self.anonymize);
                        screens::bilan::show(ui, &s);
                    }
                    Step::Send => screens::send::show(
                        ui,
                        &mut self.anonymize,
                        self.workspace.goal,
                        &shared,
                        fx,
                    ),
                    Step::Response => {
                        screens::response::show(ui, &mut self.deanonymize, &shared, view_h, fx)
                    }
                }
            });
        });
    }

    /// Construit toute l'interface dans `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        self.now = ui.input(|i| i.time);
        self.apply_theme(ui.ctx());
        theme::sync(ui);
        let p = palette();
        self.poll();
        self.receive_paste(ui.ctx());
        self.take_dropped_files(ui);
        self.take_unrequested_paste(ui.ctx());
        if let Some(id) = self.pending_focus.take() {
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        let mut effects = Vec::new();
        let commands = shortcuts::read(ui.ctx());
        self.run_commands(commands, &mut effects);
        if self.runner.is_busy() {
            // L'avancement est lu dans l'état partagé : on redessine régulièrement.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.header(ui, &mut effects);
        self.footer(ui, &mut effects);
        let folder = self.workspace.screen == Screen::Folder;
        if folder {
            self.folder_bar(ui, &mut effects);
            self.action_bar(ui, &mut effects);
        }
        CentralPanel::default()
            .frame(Frame::new().fill(p.page))
            .show(ui, |ui| {
                let view_h = ui.available_height() - 2.0 * CONTENT_PAD_Y;
                ScrollArea::vertical()
                    .id_salt(("contenu", folder, self.workspace.step.index()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(CONTENT_PAD_Y);
                        self.content(ui, view_h, &mut effects);
                        ui.add_space(CONTENT_PAD_Y);
                    });
            });
        if self.palette.open {
            self.command_palette(ui, &mut effects);
        }
        if self.show_shortcuts {
            self.shortcuts_panel(ui);
        }
        self.apply(ui, effects);
        // Traitements synchrones (tests) : résultat visible dès cette image.
        self.poll();
        if self.pending_focus.is_some() {
            ui.ctx().request_repaint();
        }
        let bottom = if folder {
            FOOTER_H + ACTION_BAR_H
        } else {
            FOOTER_H
        };
        toast::show(ui, &mut self.toasts, self.now, bottom);
        self.drop_overlay(ui);
        self.track_prefs(ui.ctx());
    }
}

/// Notification après l'écriture des fichiers.
fn written_message(report: &service::BatchReport) -> (ToastKind, String) {
    if report.fatal.is_some() {
        return (
            ToastKind::Error,
            "Aucun fichier écrit : la raison est indiquée à l'écran.".to_owned(),
        );
    }
    let delivered = report.delivered();
    let total = report.files.len();
    if delivered == total {
        (
            ToastKind::Success,
            format!(
                "{} : vérification finale réussie.",
                count_label(delivered, "fichier prêt", "fichiers prêts")
            ),
        )
    } else {
        (
            ToastKind::Warning,
            format!(
                "{} sur {total} : voyez le détail à l'écran.",
                count_label(delivered, "fichier prêt", "fichiers prêts")
            ),
        )
    }
}

/// Emplacement des exemples, sans chemin de la machine : « Documents ›
/// Anonymiseur › Exemples », ou le nom du dossier choisi.
fn examples_place(dir: &std::path::Path) -> String {
    let parts: Vec<String> = dir
        .components()
        .rev()
        .take(service::EXAMPLES_SUBDIR.len())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let fixed = parts
        .iter()
        .rev()
        .map(String::as_str)
        .eq(service::EXAMPLES_SUBDIR.iter().copied());
    if fixed {
        "Documents › Anonymiseur › Exemples".to_owned()
    } else {
        format!("le dossier « {} »", service::file_name(dir))
    }
}

impl eframe::App for AnonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_prefs();
    }
}
