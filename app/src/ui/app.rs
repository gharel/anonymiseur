//! Fenêtre principale : en-tête (logo, onglets), contenu, pied. Relie l'état
//! des onglets au moteur : les traitements longs passent par le fil de travail.

use super::platform::{Platform, file_entry};
use super::prefs::{self, Preferences};
use super::runner::{Msg, Runner};
use super::screens;
use super::service::{self, anonymize_batch, read_inputs, restore_batch};
use super::state::Tab;
use super::state::anonymize::{AddReport, AnonymizeState, Step};
use super::state::deanonymize::{DeanonymizeState, is_map};
use super::state::freetext::{FreeTextNotice, FreeTextState};
use super::theme::{self, CONTROL_SM, ThemeChoice, Weight, font, palette};
use super::widgets::nav::theme_button;
use super::widgets::{Banner, BannerKind, Icon, PillButton, TabBar};
use crate::moteur::IaStatus;
use anon_core::Session;
use anon_engine::Engine;
use egui::{CentralPanel, Frame, Margin, Panel, RichText, ScrollArea, Stroke, Ui, vec2};
use std::path::PathBuf;
use std::sync::Arc;

/// Largeur maximale du contenu.
const CONTENT_MAX: f32 = 1040.0;
/// Hauteur de l'en-tête (`header-height` de la charte : 72 px).
const HEADER_H: f32 = 72.0;
const FOOTER_H: f32 = 40.0;

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
    FreeAnonymize,
    FreeRestore,
    OpenFolder(PathBuf),
    Copy(String),
    Cancel,
    NewSession,
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
    /// Message ponctuel affiché en haut du contenu.
    pub notice: Option<(BannerKind, String)>,
    /// Préférences (thème, niveau de protection, fenêtre).
    pub prefs: Preferences,
    /// Fichier des préférences ; `None` : rien n'est écrit (tests).
    prefs_path: Option<PathBuf>,
    /// Préférences telles qu'enregistrées la dernière fois.
    saved_prefs: Preferences,
    /// Thème appliqué au contexte egui.
    applied_theme: Option<ThemeChoice>,
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
            notice: None,
            prefs,
            prefs_path,
            saved_prefs: prefs,
            applied_theme: None,
            runner,
            platform,
        }
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
                }
            }
            Tab::Deanonymize => {
                for path in paths {
                    if is_map(&path) {
                        let _ = self.deanonymize.choose_map(path);
                    } else {
                        self.deanonymize.add_input_files([file_entry(path)]);
                    }
                }
            }
            Tab::FreeText => {}
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
                Msg::MapOpened(r) => self.deanonymize.unlock_done(r.map_err(|e| e.to_string())),
                Msg::Restored(results) => self.deanonymize.restore_files_done(results),
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
                    self.notice = Some((
                        BannerKind::Info,
                        "Le moteur termine son chargement : relancez l'analyse dans un instant."
                            .to_owned(),
                    ));
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
                    let report = anonymize_batch(
                        &engine,
                        &job.inputs,
                        &job.analysis,
                        &job.never,
                        &job.masks,
                        &mut session,
                        &job.protection,
                        &job.out_dir,
                        progress,
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
                self.deanonymize.add_input_files(files);
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
            Effect::Copy(text) => {
                ui.ctx().copy_text(text);
                self.notice = Some((
                    BannerKind::Success,
                    "Copié dans le presse-papiers.".to_owned(),
                ));
            }
            Effect::Cancel => self.runner.shared.request_cancel(),
            Effect::NewSession => {
                self.session = Session::new();
                self.anonymize.reset();
                self.anonymize.always.clear();
                self.deanonymize.lock();
                self.free_text = FreeTextState::default();
                self.notice = Some((
                    BannerKind::Info,
                    "Nouvelle session : les correspondances en mémoire ont été oubliées."
                        .to_owned(),
                ));
            }
        }
    }

    fn restore(&mut self) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        match self.deanonymize.mode {
            super::state::deanonymize::InputMode::Paste => {
                let _ = self
                    .deanonymize
                    .restore_text(|t, s| engine.restore_text(t, s));
            }
            super::state::deanonymize::InputMode::File => {
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

    /// Message pour les fichiers écartés d'un ajout.
    fn report_added(&mut self, report: &AddReport) {
        if let Some(msg) = report.outputs_message() {
            self.notice = Some((BannerKind::Info, msg));
        }
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
                let report = self
                    .anonymize
                    .add_files(ready.documents.into_iter().map(file_entry), &supported);
                self.notice = Some((
                    BannerKind::Success,
                    format!(
                        "{} documents d'exemple, entièrement fictifs, chargés depuis {}. LISEZMOI.md y propose un scénario guidé ; reponse-ia-exemple.md sert à l'onglet Désanonymiser.",
                        report.added,
                        examples_place(dir)
                    ),
                ));
            }
            Err(_) => {
                self.notice = Some((
                    BannerKind::Error,
                    "Préparation des exemples impossible : le dossier Documents n'est pas accessible en écriture."
                        .to_owned(),
                ));
            }
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
        self.notice = Some(match engine.export_text(&input, &self.session) {
            Ok(text) => match std::fs::write(&path, text) {
                Ok(()) => (
                    BannerKind::Success,
                    format!(
                        "Texte anonymisé enregistré : {}.",
                        service::file_name(&path)
                    ),
                ),
                Err(_) => (
                    BannerKind::Error,
                    "Écriture impossible à cet endroit.".to_owned(),
                ),
            },
            Err(e) => (BannerKind::Error, service::sentence(&e.to_string())),
        });
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
                    if let Some(i) = TabBar::new(&tabs).show(ui, self.tab.index())
                        && let Some(tab) = Tab::from_index(i)
                    {
                        self.tab = tab;
                        self.notice = None;
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
                    ui.spacing_mut().item_spacing.x = 6.0;
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
                        if ui
                            .add(
                                PillButton::ghost("Nouvelle session")
                                    .icon(Icon::RefreshCw)
                                    .small()
                                    .enabled(!busy),
                            )
                            .on_hover_text(
                                "Oublie les correspondances en mémoire et recommence à zéro.",
                            )
                            .clicked()
                        {
                            effects.push(Effect::NewSession);
                        }
                    });
                });
            });
    }

    /// Construit toute l'interface dans `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        self.apply_theme(ui.ctx());
        theme::sync(ui);
        let p = palette();
        self.poll();
        self.take_dropped_files(ui);
        if self.runner.is_busy() {
            // L'avancement est lu dans l'état partagé : on redessine régulièrement.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut effects = Vec::new();
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
                                if let Some((kind, text)) = &self.notice {
                                    Banner::new(*kind, text).show(ui);
                                }
                                let shared = Arc::clone(&self.runner.shared);
                                match self.tab {
                                    Tab::Anonymize => screens::anonymize::show(
                                        ui,
                                        &mut self.anonymize,
                                        &self.session,
                                        &shared,
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
        self.apply(ui, effects);
        // Traitements synchrones (tests) : résultat visible dès cette image.
        self.poll();
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
