//! Carte : fond blanc, rayon 16 px, ombre légère, marge intérieure 20 px,
//! barre de couleur de 6 px en haut (facultative).

use crate::ui::theme::{palette, radius};
use egui::{Color32, Frame, InnerResponse, Margin, Rect, Stroke, Ui, vec2};

/// Hauteur de la barre de couleur.
const ACCENT_HEIGHT: f32 = 6.0;

#[derive(Debug, Clone, Copy)]
pub struct Card {
    accent: Option<Color32>,
    padding: f32,
    fill: Color32,
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Card {
    pub fn new() -> Self {
        let p = palette();
        Self {
            accent: None,
            padding: p.space(5),
            fill: p.surface,
        }
    }

    pub fn accent(mut self, color: Color32) -> Self {
        self.accent = Some(color);
        self
    }

    pub fn padding(mut self, px: f32) -> Self {
        self.padding = px;
        self
    }

    pub fn fill(mut self, fill: Color32) -> Self {
        self.fill = fill;
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let p = palette();
        let pad = self.padding.round() as i8;
        let top = if self.accent.is_some() {
            (self.padding + ACCENT_HEIGHT).round() as i8
        } else {
            pad
        };
        let r = radius(p.radius_xl);
        let frame = Frame::new()
            .fill(self.fill)
            .corner_radius(r)
            .shadow(p.shadow_md)
            .stroke(Stroke::new(1.0, p.border))
            .inner_margin(Margin {
                left: pad,
                right: pad,
                top,
                bottom: pad,
            });
        let inner = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
        if let Some(color) = self.accent {
            let rect = inner.response.rect;
            let bar = Rect::from_min_size(rect.min, vec2(rect.width(), ACCENT_HEIGHT));
            // La forme entière de la carte, découpée à la hauteur de la barre : les
            // coins supérieurs suivent l'arrondi.
            ui.painter().with_clip_rect(bar).rect_filled(rect, r, color);
        }
        inner
    }
}
