//! Zone de dépôt de fichiers : bord en pointillés, état survolé quand des
//! fichiers passent au-dessus de la fenêtre. Un clic (ou Entrée) ouvre le
//! sélecteur : c'est à l'appelant de le faire.

use super::Icon;
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{
    Color32, Pos2, Rect, Response, RichText, Role, Sense, Shape, Stroke, StrokeKind, Ui, Widget,
    WidgetInfo, pos2, vec2,
};

#[derive(Debug, Clone)]
pub struct DropZone<'a> {
    title: &'a str,
    subtitle: &'a str,
    height: f32,
    /// Force l'état survolé (galerie, tests).
    hovered: Option<bool>,
}

impl<'a> DropZone<'a> {
    pub fn new(title: &'a str, subtitle: &'a str) -> Self {
        Self {
            title,
            subtitle,
            height: 168.0,
            hovered: None,
        }
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn force_hovered(mut self, on: bool) -> Self {
        self.hovered = Some(on);
        self
    }
}

/// Points le long d'un rectangle arrondi (sens horaire), pour le pointillé.
fn rounded_rect_path(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut pts = Vec::new();
    let corners = [
        (pos2(rect.max.x - r, rect.min.y + r), -90.0_f32),
        (pos2(rect.max.x - r, rect.max.y - r), 0.0),
        (pos2(rect.min.x + r, rect.max.y - r), 90.0),
        (pos2(rect.min.x + r, rect.min.y + r), 180.0),
    ];
    let steps = 8;
    pts.push(pos2(rect.min.x + r, rect.min.y));
    for (c, start) in corners {
        for i in 0..=steps {
            let a = (start + 90.0 * i as f32 / steps as f32).to_radians();
            pts.push(pos2(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts.push(pos2(rect.min.x + r, rect.min.y));
    pts
}

impl Widget for DropZone<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), self.height), Sense::click());
        let label = format!("{}. {}", self.title, self.subtitle);
        response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
        let files_over = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let active = self
            .hovered
            .unwrap_or(files_over || response.hovered() || response.has_focus());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let r = p.radius_lg;
            let (fill, edge) = if active {
                (p.primary_subtle, p.primary_dark)
            } else {
                (p.bg_subtle, p.border_input)
            };
            painter.rect_filled(rect, radius(r), fill);
            let path = rounded_rect_path(rect.shrink(1.0), r);
            painter.extend(Shape::dashed_line(
                &path,
                Stroke::new(if active { 2.0 } else { 1.5 }, edge),
                8.0,
                6.0,
            ));
            if response.has_focus() {
                painter.rect_stroke(
                    rect.expand(2.0),
                    radius(r + 2.0),
                    Stroke::new(2.0, p.border_focus),
                    StrokeKind::Outside,
                );
            }
            // Pastille d'icône, titre, sous-titre, centrés.
            let ink = if active {
                p.primary_dark
            } else {
                p.text_secondary
            };
            let title = egui::WidgetText::from(
                RichText::new(self.title)
                    .font(font(Weight::Semibold, p.text_base))
                    .color(if active { p.primary_dark } else { p.dark }),
            )
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                rect.width() - 32.0,
                egui::TextStyle::Body,
            );
            let sub = egui::WidgetText::from(
                RichText::new(self.subtitle)
                    .font(font(Weight::Regular, p.text_sm))
                    .color(p.text_secondary),
            )
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                rect.width() - 32.0,
                egui::TextStyle::Body,
            );
            let disc = 48.0;
            let total = disc + 12.0 + title.size().y + 4.0 + sub.size().y;
            let mut y = rect.center().y - total / 2.0;
            let disc_rect =
                Rect::from_center_size(pos2(rect.center().x, y + disc / 2.0), vec2(disc, disc));
            painter.circle_filled(
                disc_rect.center(),
                disc / 2.0,
                if active { p.white } else { p.primary_subtle },
            );
            Icon::Upload.paint(
                ui,
                Rect::from_center_size(disc_rect.center(), vec2(24.0, 24.0)),
                p.primary_dark,
            );
            y += disc + 12.0;
            let title_h = title.size().y;
            painter.galley(
                pos2(rect.center().x - title.size().x / 2.0, y),
                title,
                Color32::PLACEHOLDER,
            );
            y += title_h + 4.0;
            painter.galley(pos2(rect.center().x - sub.size().x / 2.0, y), sub, ink);
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}
