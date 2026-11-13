//! Fenêtre principale : en-tête (logo, onglets, thème), contenu, pied,
//! notifications, voile de dépôt et panneau des raccourcis. Relie l'état des
//! onglets au moteur : les traitements longs passent par le fil de travail.

use super::platform::{Platform, file_entry};
use super::prefs::{self, Preferences};
use super::runner::{Msg, Runner, Shared};
use super::screens;
use super::service::{self, anonymize_batch_tracked, read_inputs, restore_batch};
use super::shortcuts::{self, Command};
use super::state::anonymize::{AddReport, AnonymizeState, Step};
use super::state::deanonymize::{DeanonymizeState, InputMode, MapStatus, is_map};
use super::state::freetext::{FreeTextNotice, FreeTextState};
use super::state::toasts::{ToastKind, Toasts, count_label};
use super::state::{Tab, drop_hint};
use super::theme::{self, CONTROL_SM, ThemeChoice, Weight, font, palette, radius, with_alpha};
use super::widgets::field::field_id;
use super::widgets::nav::theme_button;
use super::widgets::{Icon, PillButton, TabBar, toast};
use crate::moteur::IaStatus;
use anon_core::Session;
use anon_engine::Engine;
use egui::{
    CentralPanel, Frame, Margin, Panel, RichText, ScrollArea, Stroke, StrokeKind, Ui, vec2,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Largeur maximale du contenu.
const CONTENT_MAX: f32 = 1040.0;
/// Hauteur de l'en-tête (`header-height` de la charte : 72 px).
const HEADER_H: f32 = 72.0;
const FOOTER_H: f32 = 40.0;
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
    Analyze(Vec<PathBuf>),
    PickExistingMap,
    OpenExistingMap,
    PickOutputDir,
    Anonymize,
    ExportText(String),
    PickMap,
    Unlock,
    PickResponses,
    PickRestoreDir,
    Restore,
    /// Colle le presse-papiers dans la zone de réponse et restaure aussitôt.
    PasteAndRestore,
    FreeAnonymize,
    FreeRestore,
    OpenFolder(PathBuf),
    /// Copie `text` et affiche la notification `toast`.
    Copy {
        text: String,
        toast: &'static str,
    },
    Cancel,
    NewSession,
    /// Table déposée à l'étape Déposer : la reprendre à l'étape Protéger.
    AcceptOfferedMap,
    DismissWelcome,
    ShowShortcuts,
}

pub struct AnonApp {
    pub tab: Tab,
    pub anonymize: AnonymizeState,
    pub deanonymize: DeanonymizeState,
    pub free_text: FreeTextState,
    /// Session courante, en mémoire seulement (jamais écrite en clair).
    pub session: Session,
    pub engine: Option<Arc<Engine>>,
    pub ia: Option<IaStatus>,
    /// Notifications (coin bas droit).
    pub toasts: Toasts,
    /// Panneau « Raccourcis » ouvert.
    pub show_shortcuts: bool,
    /// Préférences (thème, niveau de protection, fenêtre, accueil).
    pub prefs: Preferences,
    /// Fichier des préférences ; `None` : rien n'est écrit (tests).
    prefs_path: Option<PathBuf>,
    /// Préférences telles qu'enregistrées la dernière fois.
    saved_prefs: Preferences,
    /// Thème appliqué au contexte egui.
    applied_theme: Option<ThemeChoice>,
    /// Collage demandé : images écoulées depuis la demande.
    paste_wait: Option<u32>,
    /// Instant de l'image en cours (secondes).
    now: f64,
    runner: Runner,
    platform: Box<dyn Platform>,
}

impl std::fmt::Debug for AnonApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnonApp")
            .field("tab", &self.tab)
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
        let anonymize = AnonymizeState {
            profile: prefs.profile.into(),
            ..AnonymizeState::default()
        };
        Self {
            tab: Tab::default(),
            anonymize,
            deanonymize: DeanonymizeState::default(),
            free_text: FreeTextState::default(),
            session: Session::new(),
            engine,
            ia,
            toasts: Toasts::default(),
            show_shortcuts: false,
            prefs,
            prefs_path,
            saved_prefs: prefs,
            applied_theme: None,
            paste_wait: None,
            now: 0.0,
            runner,
            platform,
        }
    }

    /// Avancement du traitement en cours (affichage, tests).
    pub fn progress(&self) -> Arc<Shared> {
        Arc::clone(&self.runner.shared)
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

    /// Retient le profil et la taille de la fenêtre ; enregistre les
    /// préférences quand un réglage choisi par l'utilisateur change.
    fn track_prefs(&mut self, ctx: &egui::Context) {
        self.prefs.profile = self.anonymize.profile.into();
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

    /// Ajoute des fichiers à l'onglet courant (dépôt ou dialogue).
    pub fn add_paths(&mut self, paths: Vec<PathBuf>) {
        match self.tab {
            Tab::Anonymize => {
                if self.anonymize.step == Step::Drop && self.anonymize.busy.is_none() {
                    let supported = self.supported();
                    let report = self
                        .anonymize
                        .add_files(paths.into_iter().map(file_entry), &supported);
                    self.report_added(&report);
                } else {
                    self.toast(
                        ToastKind::Warning,
                        "Revenez à l'étape Déposer pour ajouter des fichiers.",
                    );
                }
            }
            Tab::Deanonymize => {
                let mut added = 0;
                for path in paths {
                    if is_map(&path) {
                        let name = service::file_name(&path);
                        if self.deanonymize.choose_map(path).is_ok() {
                            self.toast(
                                ToastKind::Success,
                                format!("Table « {name} » choisie : saisissez son mot de passe."),
                            );
                        }
                    } else {
                        let before = self.deanonymize.input_files.len();
                        self.deanonymize.add_input_files([file_entry(path)]);
                        added += self.deanonymize.input_files.len() - before;
                    }
                }
                if added > 0 {
                    self.toast(
                        ToastKind::Success,
                        count_label(added, "fichier ajouté", "fichiers ajoutés"),
                    );
                }
            }
            Tab::FreeText => self.toast(
                ToastKind::Info,
                "Déposez les fichiers dans l'onglet Anonymiser ou Désanonymiser.",
            ),
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
                }
                Msg::Analyzed {
                    inputs,
                    rejected,
                    analysis,
                } => self
                    .anonymize
                    .analysis_done(inputs, rejected, analysis, &self.session),
                Msg::Anonymized {
                    report,
                    session,
                    inputs,
                    analysis,
                } => {
                    self.session = session;
                    self.anonymize.anonymize_done(report, inputs, analysis);
                }
                Msg::ExistingOpened(r) => {
                    self.anonymize.existing_opened(r.map_err(|e| e.to_string()));
                }
                Msg::MapOpened(r) => {
                    let ok = r.is_ok();
                    self.deanonymize.unlock_done(r.map_err(|e| e.to_string()));
                    if ok {
                        self.toast(ToastKind::Success, "Table ouverte.");
                    }
                }
                Msg::Restored(results) => self.deanonymize.restore_files_done(results),
            }
        }
    }

    /// Raccourcis clavier de cette image.
    fn run_commands(&mut self, ctx: &egui::Context, commands: Vec<Command>, fx: &mut Vec<Effect>) {
        let busy = self.runner.is_busy();
        for c in commands {
            match c {
                Command::Tab(n) => {
                    if let Some(t) = Tab::from_index(n) {
                        self.tab = t;
                    }
                }
                Command::Help => self.show_shortcuts = !self.show_shortcuts,
                Command::Back => {
                    if busy {
                        fx.push(Effect::Cancel);
                    } else if self.show_shortcuts {
                        self.show_shortcuts = false;
                    } else if self.tab == Tab::Anonymize {
                        self.anonymize.back();
                    }
                }
                Command::Open if !busy => match self.tab {
                    Tab::Anonymize if self.anonymize.step == Step::Drop => {
                        fx.push(Effect::PickDocuments);
                    }
                    Tab::Deanonymize => fx.push(Effect::PickMap),
                    _ => {}
                },
                Command::Open => {}
                Command::Primary if !busy => self.primary_action(fx),
                Command::Primary => {}
                Command::Find => {
                    if self.tab == Tab::Anonymize && self.anonymize.step == Step::Review {
                        ctx.memory_mut(|m| m.request_focus(field_id(SEARCH_FIELD)));
                    }
                }
            }
        }
    }

    /// Action principale de l'étape (Ctrl+Entrée), si elle est possible.
    fn primary_action(&mut self, fx: &mut Vec<Effect>) {
        match self.tab {
            Tab::Anonymize => match self.anonymize.step {
                Step::Drop => {
                    if self.anonymize.analyze_blocker().is_none()
                        && let Ok(paths) = self.anonymize.begin_analysis()
                    {
                        fx.push(Effect::Analyze(paths));
                    }
                }
                Step::Review => {
                    let _ = self.anonymize.confirm_review();
                }
                Step::Protect => {
                    if self.anonymize.protect_blocker().is_none() {
                        fx.push(Effect::Anonymize);
                    }
                }
                Step::Done => {}
            },
            Tab::Deanonymize => match self.deanonymize.map {
                MapStatus::Locked { .. } if self.deanonymize.unlock_blocker().is_none() => {
                    fx.push(Effect::Unlock);
                }
                MapStatus::Unlocked { .. } if self.deanonymize.restore_blocker().is_none() => {
                    fx.push(Effect::Restore);
                }
                _ => {}
            },
            Tab::FreeText => fx.push(Effect::FreeAnonymize),
        }
    }

    /// Texte du presse-papiers demandé par « Coller depuis le presse-papiers ».
    fn receive_paste(&mut self, ctx: &egui::Context) {
        let Some(waited) = self.paste_wait else {
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
                self.deanonymize.mode = InputMode::Paste;
                self.deanonymize.pasted = text;
                self.restore();
                if let Some((_, r)) = &self.deanonymize.text_result {
                    let msg = format!(
                        "Réponse collée : {}.",
                        count_label(r.restored, "balise restaurée", "balises restaurées")
                    );
                    self.toast(ToastKind::Success, msg);
                }
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
                self.paste_wait = Some(waited + 1);
                ctx.request_repaint_after(std::time::Duration::from_millis(PASTE_POLL_MS));
            }
        }
    }

    fn apply(&mut self, ui: &Ui, effects: Vec<Effect>) {
        for e in effects {
            self.apply_one(ui, e);
        }
    }

    fn apply_one(&mut self, ui: &Ui, effect: Effect) {
        match effect {
            Effect::PickDocuments => {
                let files = self.platform.pick_documents();
                let supported = self.supported();
                let report = self.anonymize.add_files(files, &supported);
                self.report_added(&report);
            }
            Effect::TryExamples => self.try_examples(),
            Effect::Analyze(paths) => {
                let Some(engine) = self.engine.clone() else {
                    self.anonymize.busy = None;
                    self.toast(
                        ToastKind::Info,
                        "Le moteur termine son chargement : relancez l'analyse dans un instant.",
                    );
                    return;
                };
                let opts = self.anonymize.analyze_options();
                let session = self.session.clone();
                self.runner.spawn(move |progress| {
                    let (inputs, rejected) = read_inputs(&paths);
                    let analysis = engine.analyze(&inputs, &opts, &session, progress);
                    Msg::Analyzed {
                        inputs,
                        rejected,
                        analysis,
                    }
                });
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
                self.anonymize.busy = Some(super::state::anonymize::Busy::OpeningTable);
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
            Effect::Anonymize => {
                let Some(engine) = self.engine.clone() else {
                    return;
                };
                let Ok(job) = self.anonymize.begin_anonymize() else {
                    return;
                };
                let session = job.session.unwrap_or_else(|| self.session.clone());
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
                        session,
                        inputs: job.inputs,
                        analysis: job.analysis,
                    }
                });
            }
            Effect::ExportText(name) => self.export_text(&name),
            Effect::PickMap => {
                if let Some(path) = self.platform.pick_map() {
                    let _ = self.deanonymize.choose_map(path);
                    ui.ctx().memory_mut(|m| {
                        m.request_focus(field_id(screens::deanonymize::PASSWORD_FIELD))
                    });
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
            Effect::PickResponses => {
                let files = self.platform.pick_responses();
                let n = files.len();
                self.deanonymize.add_input_files(files);
                if n > 0 {
                    self.toast(
                        ToastKind::Success,
                        count_label(n, "fichier ajouté", "fichiers ajoutés"),
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
            Effect::Restore => self.restore(),
            Effect::PasteAndRestore => {
                if let Some(reason) = self.deanonymize.paste_blocker() {
                    self.toast(ToastKind::Warning, reason);
                    return;
                }
                let ctx = ui.ctx();
                // Le texte collé ne doit pas aller dans un champ qui aurait le focus.
                ctx.memory_mut(|m| {
                    if let Some(f) = m.focused() {
                        m.surrender_focus(f);
                    }
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                self.paste_wait = Some(0);
                ctx.request_repaint();
            }
            Effect::FreeAnonymize => {
                let Some(engine) = self.engine.clone() else {
                    self.free_text.notice = Some(FreeTextNotice::EngineLoading);
                    return;
                };
                let opts = self.anonymize.analyze_options();
                let session = &mut self.session;
                self.free_text.anonymize(|t| {
                    let a = engine.analyze_text(t, &opts, session);
                    engine.apply_selection(&a, session);
                    (engine.anonymize_text(t, session), a.selected().count())
                });
            }
            Effect::FreeRestore => {
                let Some(engine) = self.engine.clone() else {
                    self.free_text.notice = Some(FreeTextNotice::EngineLoading);
                    return;
                };
                let session = &self.session;
                self.free_text.restore(|t| {
                    let (out, r) = engine.restore_text(t, session);
                    (out, r.restored, r.unknown.len())
                });
            }
            Effect::OpenFolder(dir) => self.platform.open_folder(&dir),
            Effect::Copy { text, toast } => {
                ui.ctx().copy_text(text);
                self.toast(ToastKind::Success, toast);
            }
            Effect::Cancel => self.runner.shared.request_cancel(),
            Effect::NewSession => {
                self.session = Session::new();
                self.anonymize.reset();
                self.anonymize.always.clear();
                self.deanonymize.lock();
                self.free_text = FreeTextState::default();
                self.toast(
                    ToastKind::Info,
                    "Nouvelle session : les correspondances en mémoire ont été oubliées.",
                );
            }
            Effect::AcceptOfferedMap => {
                if self.anonymize.accept_offered_map() {
                    self.toast(
                        ToastKind::Success,
                        "Table retenue : elle sera reprise à l'étape Protéger, avec son mot de passe.",
                    );
                }
            }
            Effect::DismissWelcome => self.prefs.welcome_seen = true,
            Effect::ShowShortcuts => self.show_shortcuts = true,
        }
    }

    fn restore(&mut self) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        match self.deanonymize.mode {
            InputMode::Paste => {
                let _ = self
                    .deanonymize
                    .restore_text(|t, s| engine.restore_text(t, s));
            }
            InputMode::File => {
                if let Ok(job) = self.deanonymize.begin_restore_files() {
                    self.runner.spawn(move |progress| {
                        let (inputs, unreadable) = read_inputs(&job.paths);
                        let mut results =
                            restore_batch(&engine, &inputs, &job.session, &job.out_dir, progress);
                        results.extend(unreadable.into_iter().map(|r| service::RestoreOutcome {
                            name: r.name,
                            result: Err(r.reason),
                        }));
                        Msg::Restored(results)
                    });
                }
            }
        }
    }

    /// Notifications d'un ajout de fichiers ; une table déposée est proposée.
    fn report_added(&mut self, report: &AddReport) {
        if report.added > 0 {
            self.toast(
                ToastKind::Success,
                count_label(report.added, "fichier ajouté", "fichiers ajoutés"),
            );
        }
        if let Some(msg) = report.outputs_message() {
            self.toast(ToastKind::Info, msg);
        }
        if report.refused > 0 {
            self.toast(
                ToastKind::Warning,
                format!(
                    "{} : la raison est indiquée sous la liste.",
                    count_label(report.refused, "fichier refusé", "fichiers refusés")
                ),
            );
        }
        if let Some(map) = &report.map {
            self.anonymize.offered_map = Some(map.clone());
        }
    }

    /// Notification pour le texte libre (le bandeau a laissé place aux notifications).
    fn free_text_notice(&mut self) {
        let Some(n) = self.free_text.notice.take() else {
            return;
        };
        // La copie a déjà sa notification (« Texte copié »).
        if n == FreeTextNotice::Copied {
            return;
        }
        let kind = if n.is_problem() {
            ToastKind::Warning
        } else {
            ToastKind::Success
        };
        self.toast(kind, n.message());
    }

    /// Charge les exemples fictifs, en un clic : toujours le même dossier
    /// (`Documents/Anonymiseur/Exemples`), réutilisé ; dialogue seulement si le
    /// dossier Documents est introuvable.
    fn try_examples(&mut self) {
        if self.anonymize.step != Step::Drop || self.anonymize.busy.is_some() {
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
    /// d'écrasé) et charge les documents d'exemple dans la liste.
    pub fn load_examples(&mut self, dir: &std::path::Path) {
        match service::prepare_examples(dir) {
            Ok(ready) => {
                let supported = self.supported();
                let total = ready.documents.len();
                let report = self
                    .anonymize
                    .add_files(ready.documents.into_iter().map(file_entry), &supported);
                let msg = if report.added == 0 && total > 0 {
                    "Exemples prêts : ils sont déjà dans la liste.".to_owned()
                } else {
                    format!(
                        "Exemples prêts : {} fictifs, dans {}.",
                        count_label(report.added, "document", "documents"),
                        examples_place(dir)
                    )
                };
                self.toast(ToastKind::Success, msg);
            }
            Err(_) => self.toast(
                ToastKind::Error,
                "Préparation des exemples impossible : le dossier Documents n'est pas accessible en écriture.",
            ),
        }
    }

    /// Texte anonymisé d'un fichier bloqué par le contrôle de fuite, en `.md`.
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
                    format!(
                        "Texte anonymisé enregistré : {}.",
                        service::file_name(&path)
                    ),
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

    fn header(&mut self, ui: &mut Ui) {
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
                ui.horizontal_centered(|ui| {
                    let logo = if p.dark_mode {
                        egui::include_image!("../../../assets/charte/logo-white-color.png")
                    } else {
                        egui::include_image!("../../../assets/charte/logo-color.png")
                    };
                    ui.add(
                        egui::Image::new(logo)
                            .fit_to_exact_size(vec2(110.0, 36.0))
                            .alt_text("Skazy Formation"),
                    );
                    ui.add_space(p.space(8));
                    let tabs = [
                        (Icon::Lock, Tab::Anonymize.label()),
                        (Icon::LockOpen, Tab::Deanonymize.label()),
                        (Icon::FileText, Tab::FreeText.label()),
                    ];
                    let hints: Vec<String> = shortcuts::TABS.iter().map(|s| s.label()).collect();
                    if let Some(i) = TabBar::new(&tabs).hints(&hints).show(ui, self.tab.index())
                        && let Some(tab) = Tab::from_index(i)
                    {
                        self.tab = tab;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        theme_button(ui, &mut self.prefs.theme);
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
                    ui.label(small("Tout reste sur votre ordinateur — aucune connexion"));
                    ui.label(small("·"));
                    let ia = self
                        .ia
                        .map(IaStatus::label)
                        .unwrap_or("Chargement du moteur…");
                    ui.label(small(ia));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let busy = self.runner.is_busy();
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
                                "Oublie les correspondances en mémoire et recommence à zéro."
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
        let busy = self.runner.is_busy()
            || self.anonymize.busy.is_some()
            || self.deanonymize.busy.is_some();
        let hint = drop_hint(self.tab, self.anonymize.step, busy, &paths, count);
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
                let r = ui.put(
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
                let _ = r;
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
                ui.set_width(520.0);
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

    /// Construit toute l'interface dans `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        self.now = ui.input(|i| i.time);
        self.apply_theme(ui.ctx());
        theme::sync(ui);
        let p = palette();
        self.poll();
        self.receive_paste(ui.ctx());
        self.take_dropped_files(ui);
        let mut effects = Vec::new();
        let commands = shortcuts::read(ui.ctx());
        self.run_commands(ui.ctx(), commands, &mut effects);
        if self.runner.is_busy() {
            // L'avancement est lu dans l'état partagé : on redessine régulièrement.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.header(ui);
        self.footer(ui, &mut effects);

        CentralPanel::default()
            .frame(Frame::new().fill(p.page))
            .show(ui, |ui| {
                let view_h = ui.available_height();
                ScrollArea::vertical()
                    .id_salt(("contenu", self.tab.index()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let side = p.space(6);
                        let avail = ui.available_width();
                        let width = (avail - 2.0 * side).min(CONTENT_MAX);
                        let left = ((avail - width) / 2.0).max(side);
                        ui.add_space(p.space(8));
                        ui.horizontal(|ui| {
                            ui.add_space(left);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                ui.spacing_mut().item_spacing.y = p.space(4);
                                let shared = Arc::clone(&self.runner.shared);
                                match self.tab {
                                    Tab::Anonymize => screens::anonymize::show(
                                        ui,
                                        &mut self.anonymize,
                                        &self.session,
                                        &shared,
                                        !self.prefs.welcome_seen,
                                        &mut effects,
                                    ),
                                    Tab::Deanonymize => screens::deanonymize::show(
                                        ui,
                                        &mut self.deanonymize,
                                        &shared,
                                        &mut effects,
                                    ),
                                    Tab::FreeText => screens::freetext::show(
                                        ui,
                                        &mut self.free_text,
                                        view_h,
                                        &mut effects,
                                    ),
                                }
                            });
                        });
                        ui.add_space(p.space(10));
                    });
            });
        if self.show_shortcuts {
            self.shortcuts_panel(ui);
        }
        self.apply(ui, effects);
        // Traitements synchrones (tests) : résultat visible dès cette image.
        self.poll();
        self.free_text_notice();
        toast::show(ui, &mut self.toasts, self.now, FOOTER_H);
        self.drop_overlay(ui);
        self.track_prefs(ui.ctx());
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
