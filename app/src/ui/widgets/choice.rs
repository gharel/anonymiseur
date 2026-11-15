//! Carte de choix exclusif (bouton radio en carte) : titre, conséquence en
//! une phrase, note en vert. Sert au choix de l'objectif sur l'accueil.

use super::Icon;
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{
    Align2, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, Vec2, Widget, WidgetInfo, pos2,
    vec2,
};

/// Marges intérieures de la carte.
const PAD: Vec2 = vec2(18.0, 16.0);
/// Diamètre du bouton radio.
const RADIO: f32 = 20.0;
/// Écart entre le bouton radio et le texte.
const GAP: f32 = 14.0;

#[derive(Debug, Clone)]
pub struct ChoiceCard<'a> {
    title: &'a str,
    description: &'a str,
    note: &'a str,
    note_icon: Icon,
    selected: bool,
    width: Option<f32>,
}

impl<'a> ChoiceCard<'a> {
    pub fn new(title: &'a str, description: &'a str, selected: bool) -> Self {
        Self {
            title,
            description,
            note: "",
            note_icon: Icon::KeyRound,
            selected,
            width: None,
        }
    }

    pub fn note(mut self, icon: Icon, note: &'a str) -> Self {
        self.note_icon = icon;
        self.note = note;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }
}

impl Widget for ChoiceCard<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let text_w = (width - 2.0 * PAD.x - RADIO - GAP).max(40.0);
        let title = ui.painter().layout(
            self.title.to_owned(),
            font(Weight::Bold, p.text_base),
            p.heading,
            text_w,
        );
        let desc = ui.painter().layout(
            self.description.to_owned(),
            font(Weight::Regular, p.text_sm),
            p.text,
            text_w,
        );
        let note = (!self.note.is_empty()).then(|| {
            ui.painter().layout_no_wrap(
                self.note.to_owned(),
                font(Weight::Semibold, p.text_xs),
                p.accent,
            )
        });
        let note_h = note.as_ref().map_or(0.0, |g| 8.0 + g.size().y.max(16.0));
        let height = PAD.y * 2.0 + title.size().y + 2.0 + desc.size().y + note_h;
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        let selected = self.selected;
        let name = format!("{}. {}", self.title, self.description);
        response.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, &name));
        if ui.is_rect_visible(rect) {
            let hovered = response.hovered();
            // Vert de texte (et non le vert des boutons) : assez contrasté sur
            // le fond de marque dans les deux thèmes.
            let (fill, edge) = if selected {
                (p.primary_subtle, p.accent)
            } else if hovered {
                (p.surface, p.accent)
            } else {
                (p.surface, p.border)
            };
            let painter = ui.painter();
            painter.rect(
                rect,
                radius(p.radius_xl),
                fill,
                Stroke::new(1.5, edge),
                StrokeKind::Inside,
            );
            let c = pos2(
                rect.min.x + PAD.x + RADIO / 2.0,
                rect.min.y + PAD.y + RADIO / 2.0,
            );
            let ring = if selected { p.accent } else { p.border_input };
            painter.circle_stroke(c, RADIO / 2.0 - 1.0, Stroke::new(2.0, ring));
            if selected {
                painter.circle_filled(c, 5.0, p.accent);
            }
            let x = rect.min.x + PAD.x + RADIO + GAP;
            let mut y = rect.min.y + PAD.y;
            let title_h = title.size().y;
            painter.galley(pos2(x, y), title, p.heading);
            y += title_h + 2.0;
            let desc_h = desc.size().y;
            painter.galley(pos2(x, y), desc, p.text);
            y += desc_h + 8.0;
            if let Some(g) = note {
                let line = g.size().y.max(16.0);
                self.note_icon.paint(
                    ui,
                    Rect::from_center_size(pos2(x + 8.0, y + line / 2.0), Vec2::splat(16.0)),
                    p.accent,
                );
                ui.painter().galley(
                    Align2::LEFT_CENTER
                        .anchor_size(pos2(x + 22.0, y + line / 2.0), g.size())
                        .min,
                    g,
                    p.accent,
                );
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect.expand(2.0),
                    radius(p.radius_xl + 2.0),
                    Stroke::new(2.0, p.border_focus),
                    StrokeKind::Outside,
                );
            }
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}
