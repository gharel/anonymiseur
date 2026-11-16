//! Accueil : une seule action principale (déposer des fichiers ou coller un
//! texte), le choix de l'objectif avec ses conséquences, et deux liens :
//! restaurer une réponse avec une clé, essayer avec des exemples.

use super::control_row;
use super::verify::rejected_banner;
use crate::ui::app::Effect;
use crate::ui::shortcuts::{self, with_shortcut};
use crate::ui::state::anonymize::AnonymizeState;
use crate::ui::state::toasts::count_label;
use crate::ui::state::workspace::{goal_key_note, goal_summary};
use crate::ui::theme::{CONTROL_SM, Weight, font, palette};
use crate::ui::widgets::{ChoiceCard, DropZone, Icon, PillButton, overline};
use anon_core::Goal;
use egui::{RichText, Ui};

/// Largeur maximale de l'accueil.
const HOME_MAX_W: f32 = 960.0;
/// Hauteur approximative du contenu, pour le centrer verticalement.
const HOME_H: f32 = 508.0;
/// Taille du titre de l'accueil.
const TITLE_SIZE: f32 = 32.0;
/// Hauteur de la zone de dépôt et de la bande de ses boutons.
const DROP_H: f32 = 208.0;
const DROP_FOOTER_H: f32 = 64.0;
/// Marge sous la bande des boutons : avec des boutons de 32 points centrés
/// dans le reste de la bande, ils restent à 24 points du bord en pointillés.
const DROP_FOOTER_PAD: f32 = 16.0;
/// Largeur à partir de laquelle les deux objectifs sont côte à côte.
const TWO_COLUMNS: f32 = 640.0;

/// Ce que l'accueil affiche du dossier en cours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeInfo {
    pub goal: Goal,
    /// Documents du dossier déjà ouvert (0 : aucun).
    pub documents: usize,
    /// Un dossier (documents ou clé) est ouvert : on peut le reprendre.
    pub resumable: bool,
    pub busy: bool,
}

pub fn show(
    ui: &mut Ui,
    st: &mut AnonymizeState,
    info: HomeInfo,
    view_h: f32,
    fx: &mut Vec<Effect>,
) {
    let p = palette();
    let avail = ui.available_width();
    let side = p.space(6);
    let width = (avail - 2.0 * side).min(HOME_MAX_W);
    let left = ((avail - width) / 2.0).max(side);
    ui.add_space(((view_h - HOME_H) / 2.0).max(0.0));
    ui.horizontal(|ui| {
        ui.add_space(left);
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = p.space(2);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Préparer des documents pour une IA")
                        .font(font(Weight::Bold, TITLE_SIZE))
                        .color(p.heading),
                );
                ui.horizontal(|ui| {
                    let text = "Tout reste sur votre ordinateur.";
                    let g = ui.painter().layout_no_wrap(
                        text.to_owned(),
                        font(Weight::Regular, p.text_md),
                        p.text_secondary,
                    );
                    let w = 20.0 + p.space(2) + g.size().x;
                    ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    Icon::ShieldCheck.show(ui, p.accent, 20.0);
                    ui.label(
                        RichText::new(text)
                            .font(font(Weight::Regular, p.text_md))
                            .color(p.text_secondary),
                    );
                });
            });
            ui.add_space(p.space(4));
            drop_zone(ui, info, fx);
            if rejected_banner(ui, &st.rejected) {
                st.dismiss_rejected();
            }
            ui.add_space(p.space(4));
            overline(ui, "Objectif");
            goal_choice(ui, info.goal, fx);
            ui.add_space(p.space(3));
            links(ui, info, fx);
        });
    });
}

fn drop_zone(ui: &mut Ui, info: HomeInfo, fx: &mut Vec<Effect>) {
    let p = palette();
    let paste = shortcuts::PASTE_TEXT.label();
    let title = format!("Déposez des fichiers, ou collez un texte ({paste})");
    let zone = ui
        .add(
            DropZone::new(
                &title,
                "Word, Excel, PowerPoint, PDF, OpenDocument, images, texte",
            )
            .height(DROP_H)
            .footer(DROP_FOOTER_H),
        )
        .on_hover_text(with_shortcut("Choisir des fichiers", shortcuts::OPEN));
    let open = shortcuts::OPEN.label();
    let band = DropZone::footer_rect(zone.rect, DROP_FOOTER_H);
    let band = band.with_max_y(band.max.y - DROP_FOOTER_PAD);
    let pick = PillButton::primary("Choisir des fichiers")
        .icon(Icon::FolderOpen)
        .small()
        .kbd(&open)
        .enabled(!info.busy);
    let paste_btn = PillButton::secondary("Coller un texte")
        .icon(Icon::ClipboardPaste)
        .small()
        .kbd(&paste)
        .enabled(!info.busy);
    // Les deux boutons, centrés dans la bande du bas de la zone. Un Ui enfant
    // qui ne déplace pas le curseur du parent : la suite reste sous la zone
    // entière, pas sous la bande (plus haute que le bas de la zone).
    let gap = p.space(3);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(band)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = gap;
    let w = pick.natural_width(&row) + gap + paste_btn.natural_width(&row);
    row.add_space(((band.width() - w) / 2.0).max(0.0));
    let pick_r = row.add(pick);
    let paste_r = row.add(paste_btn);
    if (zone.clicked() || pick_r.clicked()) && !info.busy {
        fx.push(Effect::PickDocuments);
    }
    if paste_r
        .on_hover_text("Le texte du presse-papiers devient un document du dossier")
        .clicked()
    {
        fx.push(Effect::PasteDocument);
    }
}

fn goal_choice(ui: &mut Ui, goal: Goal, fx: &mut Vec<Effect>) {
    let p = palette();
    let two = ui.available_width() >= TWO_COLUMNS;
    let card = |g: Goal| {
        let icon = match g {
            Goal::Mask => Icon::KeyRound,
            Goal::Anonymize => Icon::Lock,
        };
        ChoiceCard::new(g.label(), goal_summary(g), goal == g).note(icon, goal_key_note(g))
    };
    let mut chosen = None;
    if two {
        let gap = p.space(4);
        let w = (ui.available_width() - gap) / 2.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for g in Goal::ALL {
                if ui.add(card(*g).width(w)).clicked() {
                    chosen = Some(*g);
                }
            }
        });
    } else {
        for g in Goal::ALL {
            if ui.add(card(*g)).clicked() {
                chosen = Some(*g);
            }
        }
    }
    if let Some(g) = chosen
        && g != goal
    {
        fx.push(Effect::SetGoal(g));
    }
}

fn links(ui: &mut Ui, info: HomeInfo, fx: &mut Vec<Effect>) {
    let p = palette();
    control_row(ui, CONTROL_SM, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(6);
        if ui
            .add(
                PillButton::ghost("Restaurer une réponse avec une clé")
                    .icon(Icon::KeyRound)
                    .small()
                    .enabled(!info.busy),
            )
            .on_hover_text(
                "Ouvrez une clé de restauration (.anonmap) pour restaurer la réponse d'une IA",
            )
            .clicked()
        {
            fx.push(Effect::PickKey);
        }
        if ui
            .add(
                PillButton::ghost("Essayer avec des exemples")
                    .icon(Icon::Sparkles)
                    .small()
                    .enabled(!info.busy),
            )
            .on_hover_text(
                "Charge des documents fictifs, placés dans Documents › Anonymiseur › Exemples.",
            )
            .clicked()
        {
            fx.push(Effect::TryExamples);
        }
        if info.resumable {
            let label = if info.documents > 0 {
                format!(
                    "Reprendre le dossier ({})",
                    count_label(info.documents, "document", "documents")
                )
            } else {
                "Reprendre le dossier".to_owned()
            };
            if ui
                .add(PillButton::ghost(&label).icon(Icon::ChevronRight).small())
                .clicked()
            {
                fx.push(Effect::Resume);
            }
        }
    });
}
