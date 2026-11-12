//! Barre de progression et jauge de robustesse du mot de passe.

use crate::ui::state::password::{Strength, StrengthLevel};
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{Rect, Response, RichText, Role, Sense, Ui, Widget, WidgetInfo, pos2, vec2};

/// Hauteur de la piste.
const TRACK: f32 = 8.0;

/// Barre de progression avec libellé et pourcentage.
#[derive(Debug, Clone)]
pub struct ProgressBar<'a> {
    fraction: f32,
    label: &'a str,
}

impl<'a> ProgressBar<'a> {
    pub fn new(fraction: f32, label: &'a str) -> Self {
        Self {
            fraction: fraction.clamp(0.0, 1.0),
            label,
        }
    }
}

impl Widget for ProgressBar<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let percent = (self.fraction * 100.0).round() as u32;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.label)
                        .font(font(Weight::Medium, p.text_sm))
                        .color(p.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{percent} %"))
                            .font(font(Weight::Semibold, p.text_sm))
                            .color(p.text_secondary),
                    );
                });
            });
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), TRACK), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, radius(TRACK / 2.0), p.gray_200);
            if self.fraction > 0.0 {
                let w = (rect.width() * self.fraction).max(TRACK);
                let fill = Rect::from_min_size(rect.min, vec2(w, TRACK));
                painter.rect_filled(fill, radius(TRACK / 2.0), p.primary);
            }
            let label = format!("{} : {percent} %", self.label);
            response.widget_info(|| {
                let mut info = WidgetInfo::labeled(Role::ProgressIndicator, true, &label);
                info.value = Some(f64::from(self.fraction));
                info
            });
            response
        })
        .inner
    }
}

/// Jauge de robustesse : quatre segments et un libellé.
#[derive(Debug, Clone, Copy)]
pub struct StrengthMeter {
    strength: Strength,
}

impl StrengthMeter {
    pub fn new(strength: Strength) -> Self {
        Self { strength }
    }
}

impl Widget for StrengthMeter {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let level = self.strength.level;
        let filled = level.segments();
        let color = match level {
            StrengthLevel::Empty => p.gray_300,
            StrengthLevel::TooWeak => p.error,
            StrengthLevel::Weak => p.warning,
            StrengthLevel::Good => p.primary,
            StrengthLevel::Strong => p.primary_dark,
        };
        // Texte toujours AA : rouge d'erreur pour « trop faible », sinon anthracite.
        let text_color = match level {
            StrengthLevel::TooWeak => p.error,
            _ => p.text,
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let width = ui.available_width().min(360.0);
            let (rect, response) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
            let gap = 6.0;
            let seg_w = (rect.width() - 3.0 * gap) / 4.0;
            for i in 0..4u8 {
                let x = rect.min.x + f32::from(i) * (seg_w + gap);
                let seg = Rect::from_min_size(pos2(x, rect.min.y), vec2(seg_w, rect.height()));
                let c = if i < filled { color } else { p.gray_200 };
                ui.painter().rect_filled(seg, radius(3.0), c);
            }
            let text = format!("Robustesse : {}", level.label());
            ui.label(
                RichText::new(&text)
                    .font(font(Weight::Medium, p.text_xs))
                    .color(text_color),
            );
            response.widget_info(|| WidgetInfo::labeled(Role::ProgressIndicator, true, &text));
            response
        })
        .inner
    }
}
