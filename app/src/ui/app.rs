//! Fenêtre principale : en-tête (logo, onglets), contenu, pied.

use super::platform::{Platform, file_entry};
use super::screens;
use super::state::Tab;
use super::state::anonymize::{AnonymizeState, Step};
use super::state::deanonymize::DeanonymizeState;
use super::state::freetext::FreeTextState;
use super::theme::{self, Weight, font, palette};
use super::widgets::{Icon, TabBar};
use egui::{CentralPanel, Frame, Margin, Panel, RichText, ScrollArea, Stroke, Ui, vec2};

/// Largeur maximale du contenu.
const CONTENT_MAX: f32 = 1040.0;
/// Hauteur de l'en-tête (`header-height` de la charte : 72 px).
const HEADER_H: f32 = 72.0;
const FOOTER_H: f32 = 36.0;

/// Demandes d'effets de bord émises par les écrans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    PickDocuments,
    PickMap,
    PickResponse,
    OpenOutputFolder,
    Copy(String),
}

pub struct AnonApp {
    pub tab: Tab,
    pub anonymize: AnonymizeState,
    pub deanonymize: DeanonymizeState,
    pub free_text: FreeTextState,
    platform: Box<dyn Platform>,
}

impl std::fmt::Debug for AnonApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnonApp")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl AnonApp {
    /// Installe le thème et crée l'application.
    pub fn new(ctx: &egui::Context, platform: Box<dyn Platform>) -> Self {
        theme::install(ctx);
        Self {
            tab: Tab::default(),
            anonymize: AnonymizeState::default(),
            deanonymize: DeanonymizeState::default(),
            free_text: FreeTextState::default(),
            platform,
        }
    }

    /// Fichiers lâchés sur la fenêtre : envoyés à l'onglet courant.
    fn take_dropped_files(&mut self, ui: &Ui) {
        let dropped: Vec<_> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        match self.tab {
            Tab::Anonymize => {
                if self.anonymize.step == Step::Drop {
                    self.anonymize
                        .add_files(dropped.into_iter().map(file_entry));
                }
            }
            Tab::Deanonymize => {
                for path in dropped {
                    let is_map = path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("anonmap"));
                    if is_map {
                        let _ = self.deanonymize.choose_map(path);
                    } else {
                        self.deanonymize.set_input_file(file_entry(path));
                    }
                }
            }
            Tab::FreeText => {}
        }
    }

    fn apply(&mut self, ui: &Ui, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::PickDocuments => {
                    let files = self.platform.pick_documents();
                    self.anonymize.add_files(files);
                }
                Effect::PickMap => {
                    if let Some(path) = self.platform.pick_map() {
                        let _ = self.deanonymize.choose_map(path);
                    }
                }
                Effect::PickResponse => {
                    if let Some(file) = self.platform.pick_response() {
                        self.deanonymize.set_input_file(file);
                    }
                }
                Effect::OpenOutputFolder => {
                    let dir = self
                        .anonymize
                        .files
                        .first()
                        .and_then(|f| f.path.parent().map(|p| p.to_path_buf()));
                    if let Some(dir) = dir {
                        self.platform.open_folder(&dir);
                    }
                }
                Effect::Copy(text) => ui.ctx().copy_text(text),
            }
        }
    }

    /// Construit toute l'interface dans `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        let p = palette();
        self.take_dropped_files(ui);

        Panel::top("entete")
            .exact_size(HEADER_H)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.white)
                    .inner_margin(Margin::symmetric(24, 0))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!(
                            "../../../assets/charte/logo-color.png"
                        ))
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
                    }
                });
            });

        Panel::bottom("pied")
            .exact_size(FOOTER_H)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.white)
                    .inner_margin(Margin::symmetric(24, 0))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    Icon::ShieldCheck.show(ui, p.primary_dark, 16.0);
                    ui.label(
                        RichText::new("Tout reste sur votre ordinateur — aucune connexion")
                            .font(font(Weight::Regular, p.text_xs))
                            .color(p.text_secondary),
                    );
                });
            });

        let mut effects = Vec::new();
        CentralPanel::default()
            .frame(Frame::new().fill(p.bg_subtle))
            .show(ui, |ui| {
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
                                match self.tab {
                                    Tab::Anonymize => screens::anonymize::show(
                                        ui,
                                        &mut self.anonymize,
                                        &mut effects,
                                    ),
                                    Tab::Deanonymize => screens::deanonymize::show(
                                        ui,
                                        &mut self.deanonymize,
                                        &mut effects,
                                    ),
                                    Tab::FreeText => screens::freetext::show(
                                        ui,
                                        &mut self.free_text,
                                        &mut effects,
                                    ),
                                }
                            });
                        });
                        ui.add_space(p.space(10));
                    });
            });
        self.apply(ui, effects);
    }
}

impl eframe::App for AnonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
