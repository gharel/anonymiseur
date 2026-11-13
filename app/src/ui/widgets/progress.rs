//! Barre de progression et jauge de robustesse du mot de passe.

use crate::ui::theme::{Weight, font, palette, radius};
use anon_vault::PasswordStrength;
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
            painter.rect_filled(rect, radius(TRACK / 2.0), p.track);
            if self.fraction > 0.0 {
                let w = (rect.width() * self.fraction).max(TRACK);
                let fill = Rect::from_min_size(rect.min, vec2(w, TRACK));
                painter.rect_filled(fill, radius(TRACK / 2.0), p.meter);
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
    /// `None` : rien de saisi.
    strength: Option<PasswordStrength>,
}

impl StrengthMeter {
    /// Jauge d'un mot de passe saisi (vide : jauge éteinte).
    pub fn of(password: &str) -> Self {
        Self {
            strength: (!password.is_empty()).then(|| anon_vault::password_strength(password)),
        }
    }
}

fn strength_label(s: Option<PasswordStrength>) -> &'static str {
    match s {
        None => "à saisir",
        Some(PasswordStrength::TropCourt) => "trop court",
        Some(PasswordStrength::Faible) => "faible",
        Some(PasswordStrength::Correct) => "correcte",
        Some(PasswordStrength::Fort) => "forte",
    }
}

impl Widget for StrengthMeter {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let level = self.strength;
        let (filled, color) = match level {
            None => (0u8, p.line),
            Some(PasswordStrength::TropCourt) => (1, p.error),
            Some(PasswordStrength::Faible) => (2, p.warning),
            Some(PasswordStrength::Correct) => (3, p.primary),
            Some(PasswordStrength::Fort) => (4, p.meter),
        };
        // Texte toujours AA : rouge d'erreur quand le mot de passe est refusé, sinon anthracite.
        let text_color = match level {
            Some(PasswordStrength::TropCourt | PasswordStrength::Faible) => p.error,
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
                let c = if i < filled { color } else { p.track };
                ui.painter().rect_filled(seg, radius(3.0), c);
            }
            let text = format!("Robustesse : {}", strength_label(level));
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
