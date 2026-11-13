//! Sélecteur (`Select` de la charte) : même cadre qu'un champ (bord
//! `color-border-input`, rayon 8 px, anneau de focus), chevron Lucide à droite,
//! même hauteur que les champs et les boutons posés sur la même ligne.
//!
//! Clavier : Entrée ou Espace ouvre la liste ; ↑ et ↓ changent la valeur
//! sans l'ouvrir.

use super::Icon;
use crate::ui::theme::{
    CONTROL_MD, FIELD_BORDER, FIELD_PAD_X, FIELD_RADIUS, Weight, font, palette, radius,
};
use egui::{
    Align2, Color32, Id, Key, Popup, PopupCloseBehavior, Rect, Response, Role, Sense, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, pos2, vec2,
};

/// Hauteur d'une option de la liste.
const OPTION_H: f32 = 36.0;
/// Hauteur maximale de la liste avant défilement.
const LIST_MAX_H: f32 = 320.0;

#[derive(Debug)]
pub struct Select<'a, T> {
    id_salt: &'a str,
    label: &'a str,
    value: &'a mut T,
    options: &'a [(T, &'a str)],
    width: f32,
    height: f32,
}

impl<'a, T: PartialEq + Copy> Select<'a, T> {
    /// `label` : nom lu par les lecteurs d'écran et affiché en infobulle.
    pub fn new(
        id_salt: &'a str,
        label: &'a str,
        value: &'a mut T,
        options: &'a [(T, &'a str)],
    ) -> Self {
        Self {
            id_salt,
            label,
            value,
            options,
            width: 200.0,
            height: CONTROL_MD,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn control_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    fn index(&self) -> Option<usize> {
        self.options.iter().position(|(v, _)| *v == *self.value)
    }

    /// Affiche le sélecteur ; la réponse est marquée « modifiée » quand la
    /// valeur change.
    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette();
        let id = Id::unique(("select", self.id_salt));
        let (_, rect) = ui.allocate_space(vec2(self.width, self.height));
        let mut response = ui.interact(rect, id, Sense::click());
        let current = self.index();
        let current_label = current
            .and_then(|i| self.options.get(i))
            .map(|(_, l)| *l)
            .unwrap_or_default()
            .to_owned();
        let label = self.label.to_owned();
        let shown = current_label.clone();
        response.widget_info(|| {
            let mut info = WidgetInfo::labeled(Role::ComboBox, true, &label);
            info.current_text_value = Some(shown.clone());
            info
        });

        // Flèches : valeur précédente ou suivante, liste fermée.
        let mut changed = false;
        let popup_id = Popup::default_response_id(&response);
        let open = Popup::is_id_open(ui.ctx(), popup_id);
        if response.has_focus() && !open {
            let (up, down) =
                ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
            let n = self.options.len();
            if n > 0 && (up || down) {
                let i = current.unwrap_or(0);
                let next = if down {
                    (i + 1).min(n - 1)
                } else {
                    i.saturating_sub(1)
                };
                if let Some((v, _)) = self.options.get(next)
                    && *v != *self.value
                {
                    *self.value = *v;
                    changed = true;
                }
            }
        }

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let focused = response.has_focus() || open;
            let edge = if focused {
                p.border_focus
            } else if response.hovered() {
                p.accent
            } else {
                p.border_input
            };
            painter.rect(
                rect,
                radius(FIELD_RADIUS),
                p.surface,
                Stroke::new(FIELD_BORDER, edge),
                StrokeKind::Inside,
            );
            let chevron = 16.0;
            let text_rect = Rect::from_min_max(
                pos2(rect.min.x + FIELD_PAD_X, rect.min.y),
                pos2(rect.max.x - FIELD_PAD_X - chevron - 6.0, rect.max.y),
            );
            let galley = ui.painter().layout(
                current_label.clone(),
                font(Weight::Regular, p.text_sm),
                p.text,
                f32::INFINITY,
            );
            ui.painter().with_clip_rect(text_rect).galley(
                pos2(text_rect.min.x, rect.center().y - galley.size().y / 2.0),
                galley,
                p.text,
            );
            let icon = if open {
                Icon::ChevronUp
            } else {
                Icon::ChevronDown
            };
            icon.paint(
                ui,
                Rect::from_center_size(
                    pos2(rect.max.x - FIELD_PAD_X - chevron / 2.0, rect.center().y),
                    Vec2::splat(chevron),
                ),
                if focused { p.accent } else { p.text_secondary },
            );
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect.expand(2.0),
                    radius(FIELD_RADIUS + 2.0),
                    Stroke::new(2.0, p.border_focus),
                    StrokeKind::Outside,
                );
            }
        }

        let options = self.options;
        let value = &mut *self.value;
        let width = rect.width();
        Popup::menu(&response)
            .width(width)
            .gap(4.0)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.set_min_width(width);
                egui::ScrollArea::vertical()
                    .max_height(LIST_MAX_H)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (v, l) in options {
                            if option_row(ui, l, *v == *value).clicked() && *v != *value {
                                *value = *v;
                                changed = true;
                            }
                        }
                    });
            });
        if changed {
            response.mark_changed();
        }
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(self.label)
    }
}

/// Option de la liste : survol teinté, coche verte sur la valeur choisie.
fn option_row(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let p = palette();
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), OPTION_H), Sense::click());
    let label_owned = label.to_owned();
    response.widget_info(|| WidgetInfo::selected(Role::MenuItem, true, selected, &label_owned));
    let painter = ui.painter();
    let fill = if response.is_pointer_button_down_on() {
        p.hover_strong
    } else if response.hovered() || response.has_focus() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, radius(p.radius_sm), fill);
    let check = 16.0;
    let x = rect.min.x + 10.0;
    if selected {
        Icon::Check.paint(
            ui,
            Rect::from_center_size(pos2(x + check / 2.0, rect.center().y), Vec2::splat(check)),
            p.accent,
        );
    }
    let weight = if selected {
        Weight::Semibold
    } else {
        Weight::Regular
    };
    painter.text(
        pos2(x + check + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font(weight, p.text_sm),
        if selected { p.accent } else { p.text },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
