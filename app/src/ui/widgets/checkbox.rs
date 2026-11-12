//! Case à cocher à la charte : 18 px, rayon 4 px, bord `color-border-input`,
//! cochée en vert foncé. État intermédiaire pour l'en-tête de tableau.

use super::Icon;
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{
    Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui, Vec2, Widget, WidgetInfo, pos2,
    vec2,
};

/// Côté de la case.
const BOX: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Checked,
    /// Une partie des lignes est cochée.
    Mixed,
}

impl From<bool> for CheckState {
    fn from(b: bool) -> Self {
        if b {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    }
}

/// Case à cocher. `label` est affiché ; `accessible` le remplace pour les
/// lecteurs d'écran quand la case n'a pas de texte visible.
#[derive(Debug)]
pub struct Checkbox<'a> {
    state: CheckState,
    label: Option<&'a str>,
    accessible: &'a str,
    enabled: bool,
}

impl<'a> Checkbox<'a> {
    pub fn new(state: impl Into<CheckState>, label: &'a str) -> Self {
        Self {
            state: state.into(),
            label: Some(label),
            accessible: label,
            enabled: true,
        }
    }

    /// Case sans texte visible (ligne de tableau).
    pub fn bare(state: impl Into<CheckState>, accessible: &'a str) -> Self {
        Self {
            state: state.into(),
            label: None,
            accessible,
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let galley = self.label.map(|l| {
            egui::WidgetText::from(
                RichText::new(l)
                    .font(font(Weight::Regular, p.text_sm))
                    .color(p.text),
            )
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Wrap),
                ui.available_width() - BOX - 8.0,
                egui::TextStyle::Body,
            )
        });
        let text_size = galley.as_ref().map(|g| g.size()).unwrap_or(Vec2::ZERO);
        let gap = if galley.is_some() { 8.0 } else { 0.0 };
        let desired = vec2(BOX + gap + text_size.x, BOX.max(text_size.y).max(24.0));
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(desired, sense);
        let selected = self.state == CheckState::Checked;
        let accessible = self.accessible.to_owned();
        let enabled = self.enabled;
        response
            .widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, selected, &accessible));

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let b = Rect::from_center_size(
                pos2(rect.min.x + BOX / 2.0, rect.center().y),
                Vec2::splat(BOX),
            );
            let hovered = enabled && response.hovered();
            let alpha = if enabled { 1.0 } else { 0.5 };
            match self.state {
                CheckState::Unchecked => {
                    let edge = if hovered {
                        p.primary_dark
                    } else {
                        p.border_input
                    };
                    painter.rect(
                        b,
                        radius(p.radius_sm),
                        p.white,
                        Stroke::new(1.5, edge.gamma_multiply(alpha)),
                        StrokeKind::Inside,
                    );
                }
                CheckState::Checked | CheckState::Mixed => {
                    let fill = if hovered {
                        p.primary_darker
                    } else {
                        p.primary_dark
                    };
                    painter.rect_filled(b, radius(p.radius_sm), fill.gamma_multiply(alpha));
                    if self.state == CheckState::Checked {
                        Icon::Check.paint(ui, b.shrink(2.5), p.white);
                    } else {
                        let y = b.center().y;
                        painter.line_segment(
                            [pos2(b.min.x + 4.5, y), pos2(b.max.x - 4.5, y)],
                            Stroke::new(2.0, p.white),
                        );
                    }
                }
            }
            if response.has_focus() {
                painter.rect_stroke(
                    b.expand(2.0),
                    radius(p.radius_sm + 2.0),
                    Stroke::new(2.0, p.border_focus),
                    StrokeKind::Outside,
                );
            }
            if let Some(g) = galley {
                let pos = pos2(b.max.x + gap, rect.center().y - g.size().y / 2.0);
                painter.galley(pos, g, p.text);
            }
        }
        response
    }
}

/// Bascule un booléen avec une case à cocher ; renvoie la réponse.
pub fn toggle(ui: &mut Ui, value: &mut bool, label: &str) -> Response {
    let mut r = ui.add(Checkbox::new(*value, label));
    if r.clicked() {
        *value = !*value;
        r.mark_changed();
    }
    r
}
