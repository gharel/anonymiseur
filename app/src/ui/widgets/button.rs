//! Bouton pilule (`Button` de la charte) : primaire, secondaire, fantôme, sombre.

use super::{Icon, TRACKING_WIDE, spaced_job};
use crate::ui::theme::{CONTROL_LG, CONTROL_MD, CONTROL_SM, Weight, font, palette, radius};
use egui::{
    Color32, Response, Role, Sense, Stroke, StrokeKind, Ui, Vec2, Widget, WidgetInfo, vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Fond vert foncé, texte blanc.
    Primary,
    /// Contour vert foncé, texte vert foncé.
    Secondary,
    /// Texte seul.
    Ghost,
    /// Fond anthracite, texte blanc.
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    Sm,
    Md,
    Lg,
}

impl ButtonSize {
    /// Taille de texte, marge horizontale, taille d'icône et hauteur totale (px).
    fn metrics(self) -> (f32, f32, f32, f32) {
        let p = palette();
        match self {
            ButtonSize::Sm => (p.text_sm, 16.0, 16.0, CONTROL_SM),
            ButtonSize::Md => (p.text_base, 24.0, 20.0, CONTROL_MD),
            ButtonSize::Lg => (p.text_md, 32.0, 20.0, CONTROL_LG),
        }
    }
}

/// Bouton pilule.
#[derive(Debug, Clone)]
pub struct PillButton<'a> {
    text: &'a str,
    variant: ButtonVariant,
    size: ButtonSize,
    icon: Option<Icon>,
    enabled: bool,
    full_width: bool,
    /// Libellé lu par les lecteurs d'écran quand il diffère du texte visible.
    accessible: Option<&'a str>,
    preview_hover: bool,
}

impl<'a> PillButton<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            variant: ButtonVariant::Primary,
            size: ButtonSize::Md,
            icon: None,
            enabled: true,
            full_width: false,
            accessible: None,
            preview_hover: false,
        }
    }

    pub fn primary(text: &'a str) -> Self {
        Self::new(text)
    }

    pub fn secondary(text: &'a str) -> Self {
        Self::new(text).variant(ButtonVariant::Secondary)
    }

    pub fn ghost(text: &'a str) -> Self {
        Self::new(text).variant(ButtonVariant::Ghost)
    }

    /// Remplace le texte du bouton (« Copié » après une copie).
    pub fn label(mut self, text: &'a str) -> Self {
        self.text = text;
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Sm)
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn full_width(mut self, full: bool) -> Self {
        self.full_width = full;
        self
    }

    /// Affiche l'état survolé sans pointeur (galerie des composants).
    pub fn preview_hover(mut self, on: bool) -> Self {
        self.preview_hover = on;
        self
    }

    pub fn accessible_label(mut self, label: &'a str) -> Self {
        self.accessible = Some(label);
        self
    }

    /// Couleurs fond, contour, texte selon l'état.
    fn colors(&self, hovered: bool) -> (Color32, Color32, Color32) {
        let p = palette();
        match self.variant {
            ButtonVariant::Primary => {
                let fill = if hovered {
                    p.primary_darker
                } else {
                    p.primary_dark
                };
                (fill, fill, p.on_primary)
            }
            ButtonVariant::Secondary => {
                let fill = if hovered {
                    p.primary_subtle
                } else {
                    Color32::TRANSPARENT
                };
                let ink = if hovered { p.accent_hover } else { p.accent };
                (fill, ink, ink)
            }
            ButtonVariant::Ghost => {
                let fill = if hovered {
                    p.primary_subtle
                } else {
                    Color32::TRANSPARENT
                };
                let ink = if hovered { p.accent_hover } else { p.accent };
                (fill, Color32::TRANSPARENT, ink)
            }
            ButtonVariant::Dark => {
                let fill = if hovered {
                    p.dark_button_hover
                } else {
                    p.dark_button
                };
                (fill, fill, p.on_primary)
            }
        }
    }
}

/// Épaisseur du contour des boutons (2 px dans la charte).
const BORDER: f32 = 2.0;
/// Écart entre l'icône et le texte.
const GAP: f32 = 8.0;

impl Widget for PillButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let (text_size, pad_x, icon_size, height) = self.metrics();
        let font_id = font(Weight::Semibold, text_size);
        let galley = ui.painter().layout_job(spaced_job(
            self.text,
            font_id,
            Color32::PLACEHOLDER,
            TRACKING_WIDE,
        ));
        let icon_w = if self.icon.is_some() {
            icon_size + GAP
        } else {
            0.0
        };
        let content_w = icon_w + galley.size().x;
        let mut width = content_w + 2.0 * (pad_x + BORDER);
        if self.full_width {
            width = width.max(ui.available_width());
        }
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
        let label = self.accessible.unwrap_or(self.text).to_owned();
        let enabled = self.enabled;
        response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, &label));
        if ui.is_rect_visible(rect) {
            let pressed = enabled && response.is_pointer_button_down_on();
            let hot = self.preview_hover || pressed || (enabled && response.hovered());
            let (mut fill, mut edge, mut ink) = self.colors(hot);
            if !enabled {
                // Opacité 0,5 de la charte pour l'état désactivé.
                fill = fill.gamma_multiply(0.5);
                edge = edge.gamma_multiply(0.5);
                ink = ink.gamma_multiply(0.5);
            }
            let painter = ui.painter();
            let r = radius(p.radius_full);
            // Appui : le bouton s'enfonce d'un pixel.
            let rect = if pressed {
                rect.translate(vec2(0.0, 1.0))
            } else {
                rect
            };
            painter.rect(rect, r, fill, Stroke::new(BORDER, edge), StrokeKind::Inside);
            let start_x = rect.center().x - content_w / 2.0;
            if let Some(icon) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(start_x + icon_size / 2.0, rect.center().y),
                    Vec2::splat(icon_size),
                );
                icon.paint(ui, icon_rect, ink);
            }
            let text_pos = egui::pos2(start_x + icon_w, rect.center().y - galley.size().y / 2.0);
            painter.galley(text_pos, galley, ink);
            if response.has_focus() {
                // Anneau de focus de 2 px, décalé de 2 px.
                painter.rect_stroke(
                    rect.expand(2.0),
                    r,
                    Stroke::new(2.0, p.border_focus),
                    StrokeKind::Outside,
                );
            }
        }
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }
}

impl PillButton<'_> {
    fn metrics(&self) -> (f32, f32, f32, f32) {
        self.size.metrics()
    }
}

/// Bouton icône seul (fantôme, rond, 32 px), avec nom accessible obligatoire.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, color: Color32) -> Response {
    icon_button_sized(ui, icon, label, color, CONTROL_SM)
}

/// Bouton icône seul de côté `size`.
pub fn icon_button_sized(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    color: Color32,
    size: f32,
) -> Response {
    let p = palette();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let label_owned = label.to_owned();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label_owned));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.is_pointer_button_down_on() {
            painter.rect_filled(rect, radius(p.radius_full), p.hover_strong);
        } else if response.hovered() {
            painter.rect_filled(rect, radius(p.radius_full), p.muted);
        }
        let ink = if response.hovered() { p.accent } else { color };
        icon.paint(
            ui,
            egui::Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
            ink,
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(1.0),
                radius(p.radius_full),
                Stroke::new(2.0, p.border_focus),
                StrokeKind::Outside,
            );
        }
    }
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
