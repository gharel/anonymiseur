//! Palette de commandes (Ctrl+K) : champ de recherche, liste filtrée avec le
//! raccourci de chaque action, flèches pour choisir, Entrée pour lancer,
//! Échap pour fermer. Une action impossible reste listée, grisée, avec sa raison.

use crate::ui::state::palette::{Action, Entry, PaletteState, filter};
use crate::ui::theme::{Weight, font, palette, radius, with_alpha};
use crate::ui::widgets::{Icon, TextField, keycap_size, paint_keycap};
use egui::{
    Align2, Frame, Id, Key, Margin, Modifiers, Rect, Role, Sense, Stroke, Ui, Vec2, WidgetInfo,
    pos2, vec2,
};

/// Largeur de la palette.
const WIDTH: f32 = 600.0;
/// Distance au haut de la fenêtre.
const TOP: f32 = 96.0;
/// Hauteur d'une ligne, et d'une ligne avec sa raison.
const ROW_H: f32 = 40.0;
const ROW_BLOCKED_H: f32 = 56.0;
/// Hauteur maximale de la liste avant défilement.
const LIST_MAX_H: f32 = 400.0;
/// Identifiant du champ de recherche.
pub const SEARCH: &str = "recherche-commandes";

/// Ce que l'utilisateur a fait dans la palette à cette image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Rien de décisif.
    None,
    Close,
    Run(Action),
    /// Ligne impossible choisie : la raison.
    Blocked(&'static str),
}

/// Affiche la palette. `list` : toutes les actions avec leur état.
pub fn show(ui: &Ui, st: &mut PaletteState, list: &[Entry]) -> Outcome {
    let p = palette();
    let ctx = ui.ctx().clone();
    // Touches de navigation, lues avant le champ (qui garde le focus).
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let shown = filter(list, &st.query);
    if up {
        st.move_by(-1, shown.len());
    }
    if down {
        st.move_by(1, shown.len());
    }
    let mut outcome = Outcome::None;
    if escape {
        return Outcome::Close;
    }
    if enter {
        outcome = match st.chosen(list) {
            Some(Ok(a)) => Outcome::Run(a),
            Some(Err(why)) => Outcome::Blocked(why),
            None => Outcome::None,
        };
    }
    let area = egui::Modal::default_area(Id::unique("palette-commandes"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, TOP));
    let modal = egui::Modal::new(Id::unique("palette-commandes"))
        .area(area)
        .backdrop_color(with_alpha(egui::Color32::BLACK, 0.4))
        .frame(
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(radius(p.radius_xl))
                .shadow(p.shadow_xl)
                .inner_margin(Margin::same(12)),
        )
        .show(&ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = p.space(2);
            let before = st.query.clone();
            let r = TextField::new("Rechercher une action", &mut st.query)
                .placeholder("Rechercher une action : étape, clé, exemples, thème…")
                .id_salt(SEARCH)
                .hide_label()
                .show_control(ui);
            if !r.has_focus() {
                r.request_focus();
            }
            if st.query != before {
                st.query_changed();
            }
            let shown = filter(list, &st.query);
            let mut clicked = None;
            egui::ScrollArea::vertical()
                .id_salt("liste-commandes")
                .max_height(LIST_MAX_H)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if shown.is_empty() {
                        ui.add_space(p.space(2));
                        crate::ui::widgets::muted(ui, "Aucune action ne correspond.");
                        return;
                    }
                    for (pos, i) in shown.iter().enumerate() {
                        let Some(e) = list.get(*i) else { continue };
                        let selected = pos == st.selected.min(shown.len() - 1);
                        let r = row(ui, e, selected);
                        if selected && (up || down) {
                            r.scroll_to_me(None);
                        }
                        if r.clicked() {
                            clicked = Some(pos);
                        }
                    }
                });
            ui.add_space(p.space(1));
            crate::ui::widgets::caption(
                ui,
                "↑ ↓ pour choisir · Entrée pour lancer · Échap pour fermer",
            );
            clicked
        });
    if let Some(pos) = modal.inner {
        st.selected = pos;
        outcome = match st.chosen(list) {
            Some(Ok(a)) => Outcome::Run(a),
            Some(Err(why)) => Outcome::Blocked(why),
            None => Outcome::None,
        };
    }
    if outcome == Outcome::None && modal.backdrop_response.clicked() {
        outcome = Outcome::Close;
    }
    outcome
}

/// Une ligne : libellé (et raison si l'action est impossible), raccourci à droite.
fn row(ui: &mut Ui, e: &Entry, selected: bool) -> egui::Response {
    let p = palette();
    let h = if e.blocker.is_some() {
        ROW_BLOCKED_H
    } else {
        ROW_H
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let enabled = e.enabled();
    let label = match e.blocker {
        Some(why) => format!("{}. {why}", e.label),
        None => e.label.clone(),
    };
    response.widget_info(|| {
        let mut info = WidgetInfo::selected(Role::ListItem, enabled, selected, &label);
        info.label = Some(label.clone());
        info
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, radius(p.radius_md), p.primary_subtle);
    } else if response.hovered() {
        painter.rect_filled(rect, radius(p.radius_md), p.hover);
    }
    let ink = if !enabled {
        p.text_secondary
    } else if selected {
        p.accent
    } else {
        p.text
    };
    let x = rect.min.x + 12.0;
    let title_y = if e.blocker.is_some() {
        rect.min.y + 18.0
    } else {
        rect.center().y
    };
    if selected {
        Icon::ChevronRight.paint(
            ui,
            Rect::from_center_size(pos2(rect.max.x - 12.0, title_y), Vec2::splat(14.0)),
            p.accent,
        );
    }
    let weight = if selected {
        Weight::Semibold
    } else {
        Weight::Regular
    };
    ui.painter().text(
        pos2(x, title_y),
        Align2::LEFT_CENTER,
        &e.label,
        font(weight, p.text_sm),
        ink,
    );
    if let Some(why) = e.blocker {
        ui.painter().text(
            pos2(x, rect.max.y - 16.0),
            Align2::LEFT_CENTER,
            why,
            font(Weight::Regular, p.text_xs),
            p.text_secondary,
        );
    }
    if let Some(k) = &e.shortcut {
        let size = keycap_size(ui, k);
        let r = Rect::from_center_size(pos2(rect.max.x - 32.0 - size.x / 2.0, title_y), size);
        paint_keycap(ui, r, k, false, enabled);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
