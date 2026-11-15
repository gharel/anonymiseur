//! Fenêtres du presse-papiers express (maquette F).
//!
//! - Notification : petite fenêtre sans bordure, au premier plan, en bas à
//!   droite de l'écran, qui ne prend pas le focus : nombres et balises
//!   seulement, jamais le texte copié.
//! - Vérification : texte copié surligné (« masqué » en gris, « à décider » en
//!   jaune) et file des valeurs à décider ; M masque, G garde, J et K passent
//!   à la suivante ou à la précédente, Entrée remplace le presse-papiers,
//!   Échap annule.
//!
//! Le contenu est dessiné ici ; `AnonApp` le place dans ses fenêtres (viewports
//! egui secondaires).

use crate::ui::state::anonymize::needs_confirmation;
pub use crate::ui::state::express::ReviewOutcome;
use crate::ui::state::express::{ExpressReview, Notice};
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{Weight, font, palette, radius};
use crate::ui::widgets::field::{Mark, marked_label};
use crate::ui::widgets::toast;
use crate::ui::widgets::{Badge, Icon, PillButton, caption, overline};
use anon_engine::SELECT_MIN_CONFIDENCE;
use egui::text::TextFormat;
use egui::{Align, Frame, Key, Layout, Margin, Modifiers, RichText, Stroke, Ui, vec2};

/// Taille de la notification, en points.
pub const NOTICE_SIZE: [f32; 2] = [400.0, 72.0];
/// Écart entre la notification et le bord droit, puis le bas de l'écran
/// (place de la barre des tâches ou du Dock).
pub const NOTICE_MARGIN: [f32; 2] = [16.0, 72.0];
/// Taille de départ et taille minimale de la fenêtre de vérification.
pub const REVIEW_SIZE: [f32; 2] = [760.0, 560.0];
pub const REVIEW_MIN: [f32; 2] = [600.0, 440.0];
/// Largeur de la colonne « À décider ».
const QUEUE_W: f32 = 272.0;
/// Taille et interligne du texte copié.
const TEXT_SIZE: f32 = 15.0;
const LINE_HEIGHT: f32 = 26.0;
/// Hauteur de la barre d'actions, en bas.
const ACTIONS_H: f32 = 64.0;

/// Titre de la fenêtre de notification (sans barre de titre ni bouton dans la
/// barre des tâches, mais lu par les lecteurs d'écran).
pub const NOTICE_TITLE: &str = "Anonymiseur : presse-papiers";
/// Titre de la fenêtre de vérification.
pub const REVIEW_TITLE: &str = "Vérifier avant de remplacer le presse-papiers";
/// Libellé de l'action principale.
pub const REPLACE: &str = "Remplacer le presse-papiers";

/// Position de la notification : en bas à droite d'un écran de `monitor`
/// points ; `None` si la taille de l'écran est inconnue.
pub fn notice_position(monitor: Option<egui::Vec2>) -> Option<egui::Pos2> {
    let m = monitor?;
    let x = m.x - NOTICE_SIZE[0] - NOTICE_MARGIN[0];
    let y = m.y - NOTICE_SIZE[1] - NOTICE_MARGIN[1];
    (x >= 0.0 && y >= 0.0).then(|| egui::pos2(x, y))
}

/// Contenu de la fenêtre de notification.
pub fn notification(ui: &mut Ui, notice: &Notice) {
    let p = palette();
    let (bar, ink, icon) = toast::style(notice.kind);
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p.surface);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, vec2(4.0, rect.height())),
        0.0,
        bar,
    );
    let inner = rect.shrink2(vec2(16.0, 8.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner.with_min_x(inner.min.x + 4.0))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            icon.show(ui, ink, 20.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new("Anonymiseur · presse-papiers")
                        .font(font(Weight::Semibold, p.text_xs))
                        .color(p.text_secondary),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(&notice.text)
                            .font(font(Weight::Medium, p.text_sm))
                            .color(p.text),
                    )
                    .wrap(),
                );
            });
        },
    );
    let role = if notice.kind == crate::ui::state::toasts::ToastKind::Error {
        egui::Role::Alert
    } else {
        egui::Role::Status
    };
    let live = ui.interact(
        rect,
        egui::Id::unique("notification-presse-papiers"),
        egui::Sense::hover(),
    );
    live.widget_info(|| egui::WidgetInfo::labeled(role, true, &notice.text));
}

/// Raccourcis de la vérification : M, G, J, K, Entrée, Échap.
fn handle_keys(ui: &Ui, r: &mut ExpressReview) -> ReviewOutcome {
    let (m, g, j, k, enter, escape) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::M),
            i.consume_key(Modifiers::NONE, Key::G),
            i.consume_key(Modifiers::NONE, Key::J),
            i.consume_key(Modifiers::NONE, Key::K),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if m {
        r.decide(true);
    }
    if g {
        r.decide(false);
    }
    if j {
        r.step(true);
    }
    if k {
        r.step(false);
    }
    if m || g || j || k {
        ui.ctx().request_repaint();
    }
    if escape {
        ReviewOutcome::Cancel
    } else if enter {
        ReviewOutcome::Replace
    } else {
        ReviewOutcome::None
    }
}

/// Contenu de la fenêtre de vérification.
pub fn review(ui: &mut Ui, r: &mut ExpressReview) -> ReviewOutcome {
    let p = palette();
    let mut outcome = handle_keys(ui, r);
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, p.page);
    // Barre d'actions en bas.
    let actions =
        egui::Rect::from_min_max(egui::pos2(full.min.x, full.max.y - ACTIONS_H), full.max);
    let body = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, actions.min.y));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(body.shrink(p.space(5)))
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing.y = p.space(3);
            header(ui, r);
            let rest = ui.available_rect_before_wrap();
            let text_rect = egui::Rect::from_min_max(
                rest.min,
                egui::pos2(rest.max.x - QUEUE_W - p.space(4), rest.max.y),
            );
            let queue_rect =
                egui::Rect::from_min_max(egui::pos2(rest.max.x - QUEUE_W, rest.min.y), rest.max);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(text_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| text_panel(ui, r),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(queue_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| queue_panel(ui, r),
            );
            ui.advance_cursor_after_rect(rest);
        },
    );
    let bar = action_bar(ui, actions, r);
    if outcome == ReviewOutcome::None {
        outcome = bar;
    }
    outcome
}

/// Titre et résumé : valeurs masquées, valeurs à décider.
fn header(ui: &mut Ui, r: &ExpressReview) {
    let p = palette();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = p.space(2);
        Icon::ClipboardPaste.show(ui, p.accent, 20.0);
        ui.label(
            RichText::new(REVIEW_TITLE)
                .font(font(Weight::Semibold, p.text_md))
                .color(p.heading),
        );
    });
    let pending = r.prepared.pending_count();
    let mut line = count_label(
        r.prepared.masked_count(),
        "valeur masquée",
        "valeurs masquées",
    );
    if pending > 0 {
        line.push_str(&format!(" · {pending} à décider"));
    }
    line.push_str(" · le texte copié n'est remplacé qu'après votre accord");
    caption(ui, &line);
}

/// Bloc du texte copié : texte, plages surlignées, valeur courante présente.
type MarkedBlock = (String, Vec<(std::ops::Range<usize>, Mark)>, bool);

/// Texte copié surligné.
fn text_panel(ui: &mut Ui, r: &mut ExpressReview) {
    let p = palette();
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(radius(p.radius_lg))
        .inner_margin(Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let base = TextFormat {
                font_id: font(Weight::Regular, TEXT_SIZE),
                color: p.text,
                line_height: Some(LINE_HEIGHT),
                ..Default::default()
            };
            let current = r.current;
            let reveal = std::mem::take(&mut r.reveal);
            let Some(view) = r.view() else {
                return;
            };
            let blocks: Vec<MarkedBlock> = view
                .blocks
                .iter()
                .map(|b| {
                    let mut has_current = false;
                    let marks = b
                        .spans
                        .iter()
                        .filter_map(|s| {
                            let pending = r.prepared.is_pending(s.candidate);
                            let mark = if Some(s.candidate) == current {
                                has_current = true;
                                Mark::Current
                            } else if pending {
                                Mark::Pending
                            } else if s.masked {
                                Mark::Masked
                            } else {
                                return None;
                            };
                            Some((s.start..s.end, mark))
                        })
                        .collect();
                    (b.text.clone(), marks, has_current)
                })
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("texte-presse-papiers")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = p.space(2);
                    for (text, marks, has_current) in &blocks {
                        let resp = marked_label(ui, text, marks, &base);
                        if reveal && *has_current {
                            resp.scroll_to_me(Some(Align::Center));
                        }
                    }
                });
        });
}

/// Pourquoi la valeur demande une décision.
fn reason(r: &ExpressReview, id: usize) -> &'static str {
    let Some(c) = r.prepared.candidate(id) else {
        return "";
    };
    match (c.ambiguous, c.confidence < SELECT_MIN_CONFIDENCE) {
        (true, true) => "Mot courant ou valeur courte, détecté avec peu de certitude.",
        (true, false) => {
            "Mot courant ou valeur courte : il peut désigner autre chose qu'une donnée à masquer."
        }
        _ => "Détection incertaine : vérifiez-la dans le texte.",
    }
}

/// File « À décider » : valeur courante et ses décisions.
fn queue_panel(ui: &mut Ui, r: &mut ExpressReview) {
    let p = palette();
    ui.spacing_mut().item_spacing.y = p.space(3);
    let (done, total) = r.progress();
    ui.horizontal(|ui| {
        overline(ui, "À décider");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{done} / {total}"))
                    .font(font(Weight::Bold, p.text_sm))
                    .color(p.heading),
            );
        });
    });
    let Some(id) = r.current else {
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(radius(p.radius_xl))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = p.space(2);
                ui.horizontal(|ui| {
                    Icon::CircleCheck.show(ui, p.accent, 18.0);
                    ui.label(
                        RichText::new("Tout est décidé")
                            .font(font(Weight::Semibold, p.text_base))
                            .color(p.heading),
                    );
                });
                caption(
                    ui,
                    "Entrée remplace le presse-papiers par le texte masqué. J et K reviennent sur une valeur.",
                );
            });
        return;
    };
    let Some(c) = r.prepared.candidate(id).cloned() else {
        return;
    };
    let decided = !r.prepared.is_pending(id);
    let mut clicked = [false; 4];
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.5, p.pending.edge))
        .corner_radius(radius(p.radius_xl))
        .shadow(p.shadow_sm)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = p.space(2);
            ui.add(Badge::warning(c.kind.label()).small());
            ui.add(
                egui::Label::new(
                    RichText::new(format!("« {} »", c.value))
                        .font(font(Weight::Extrabold, p.text_lg))
                        .color(p.heading),
                )
                .wrap(),
            );
            let state = match (decided, c.selected) {
                (true, true) => "décidé : masquée",
                (true, false) => "décidé : gardée visible",
                (false, true) => "masquée pour l'instant",
                (false, false) => "visible pour l'instant",
            };
            caption(
                ui,
                &format!(
                    "{} dans le texte · {state}",
                    count_label(c.occurrences, "fois", "fois")
                ),
            );
            if needs_confirmation(&c) {
                ui.add(
                    egui::Label::new(
                        RichText::new(reason(r, id))
                            .font(font(Weight::Regular, p.text_xs))
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
            }
            ui.add_space(p.space(1));
            clicked = [
                ui.add(
                    PillButton::primary("Masquer")
                        .icon(Icon::EyeOff)
                        .kbd("M")
                        .small()
                        .full_width(true),
                )
                .on_hover_text("Masquer cette valeur (M)")
                .clicked(),
                ui.add(
                    PillButton::secondary("Garder")
                        .icon(Icon::Eye)
                        .kbd("G")
                        .small()
                        .full_width(true),
                )
                .on_hover_text("Garder cette valeur visible (G)")
                .clicked(),
                ui.add(
                    PillButton::ghost("Suivante")
                        .icon(Icon::ChevronRight)
                        .kbd("J")
                        .small(),
                )
                .on_hover_text("Valeur à décider suivante (J)")
                .clicked(),
                ui.add(
                    PillButton::ghost("Précédente")
                        .icon(Icon::ChevronUp)
                        .kbd("K")
                        .small(),
                )
                .on_hover_text("Valeur à décider précédente (K)")
                .clicked(),
            ];
        });
    if clicked[0] {
        r.decide(true);
    }
    if clicked[1] {
        r.decide(false);
    }
    if clicked[2] {
        r.step(true);
    }
    if clicked[3] {
        r.step(false);
    }
}

/// « Annuler » et « Remplacer le presse-papiers », et une ligne d'aide.
fn action_bar(ui: &mut Ui, rect: egui::Rect, r: &ExpressReview) -> ReviewOutcome {
    let p = palette();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p.surface);
    painter.hline(rect.x_range(), rect.min.y, Stroke::new(1.0, p.border));
    let pending = r.prepared.pending_count();
    let hint = if pending == 0 {
        "Tout est décidé : Entrée remplace le presse-papiers.".to_owned()
    } else {
        format!("{pending} à décider : M masque, G garde.")
    };
    let mut outcome = ReviewOutcome::None;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(p.space(5), 0.0)))
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            if ui
                .add(
                    PillButton::primary(REPLACE)
                        .icon(Icon::Check)
                        .kbd("Entrée")
                        .small(),
                )
                .on_hover_text("Remplacer le presse-papiers par le texte masqué (Entrée)")
                .clicked()
            {
                outcome = ReviewOutcome::Replace;
            }
            if ui
                .add(PillButton::ghost("Annuler").kbd("Échap").small())
                .on_hover_text("Laisser le presse-papiers tel quel (Échap)")
                .clicked()
            {
                outcome = ReviewOutcome::Cancel;
            }
            // Le reste de la ligne, à gauche : l'aide.
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = p.space(2);
                Icon::Info.show(ui, p.accent, 18.0);
                let w = ui.available_width().max(0.0);
                crate::ui::screens::truncated_text(
                    ui,
                    w,
                    crate::ui::theme::CONTROL_SM,
                    RichText::new(hint)
                        .font(font(Weight::Regular, p.text_sm))
                        .color(p.text_secondary),
                );
            });
        },
    );
    outcome
}
