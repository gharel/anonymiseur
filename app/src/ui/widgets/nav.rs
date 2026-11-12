//! Navigation : onglets de l'en-tête, étapes, sélecteur segmenté, filtres.

use super::Icon;
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{
    Color32, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, pos2,
    vec2,
};

/// Onglets de l'en-tête : texte moyen, actif en vert foncé souligné (comme le
/// `Header` de la charte).
#[derive(Debug)]
pub struct TabBar<'a> {
    tabs: &'a [(Icon, &'a str)],
}

impl<'a> TabBar<'a> {
    pub fn new(tabs: &'a [(Icon, &'a str)]) -> Self {
        Self { tabs }
    }

    /// Affiche les onglets ; renvoie l'index cliqué.
    pub fn show(self, ui: &mut Ui, active: usize) -> Option<usize> {
        let p = palette();
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            for (i, (icon, label)) in self.tabs.iter().enumerate() {
                let is_active = i == active;
                let galley = egui::WidgetText::from(
                    RichText::new(*label).font(font(Weight::Semibold, p.text_sm)),
                )
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                );
                let icon_size = 18.0;
                let pad_x = p.space(3);
                let size = vec2(pad_x * 2.0 + icon_size + 8.0 + galley.size().x, p.space(10));
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                let label_owned = (*label).to_owned();
                response
                    .widget_info(|| WidgetInfo::selected(Role::Tab, true, is_active, &label_owned));
                let hovered = response.hovered();
                let ink = if is_active || hovered {
                    p.primary_dark
                } else {
                    p.text
                };
                let painter = ui.painter();
                if is_active {
                    painter.rect_filled(rect, radius(p.radius_full), p.primary_subtle);
                } else if hovered {
                    painter.rect_filled(rect, radius(p.radius_full), p.bg_muted);
                }
                let icon_rect = Rect::from_center_size(
                    pos2(rect.min.x + pad_x + icon_size / 2.0, rect.center().y),
                    Vec2::splat(icon_size),
                );
                icon.paint(ui, icon_rect, ink);
                let text_pos = pos2(
                    icon_rect.max.x + 8.0,
                    rect.center().y - galley.size().y / 2.0,
                );
                let text_rect = Rect::from_min_size(text_pos, galley.size());
                painter.galley(text_pos, galley, ink);
                if is_active || hovered {
                    // Lien actif souligné (WCAG 1.4.1 : la couleur seule ne suffit pas).
                    let y = text_rect.max.y + 1.0;
                    painter.line_segment(
                        [pos2(text_rect.min.x, y), pos2(text_rect.max.x, y)],
                        Stroke::new(1.5, ink),
                    );
                }
                if response.has_focus() {
                    painter.rect_stroke(
                        rect.expand(1.0),
                        radius(p.radius_full),
                        Stroke::new(2.0, p.border_focus),
                        StrokeKind::Outside,
                    );
                }
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    clicked = Some(i);
                }
            }
        });
        clicked
    }
}

/// Indicateur d'étapes : pastilles numérotées reliées par un trait.
#[derive(Debug)]
pub struct Stepper<'a> {
    steps: &'a [&'a str],
    current: usize,
}

impl<'a> Stepper<'a> {
    pub fn new(steps: &'a [&'a str], current: usize) -> Self {
        Self { steps, current }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette();
        let disc = 28.0;
        let gap = 8.0;
        let line = 32.0;
        let galleys: Vec<_> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let weight = if i == self.current {
                    Weight::Semibold
                } else {
                    Weight::Medium
                };
                let color = if i <= self.current {
                    p.dark
                } else {
                    p.text_secondary
                };
                egui::WidgetText::from(RichText::new(*s).font(font(weight, p.text_sm)).color(color))
                    .into_galley(
                        ui,
                        Some(egui::TextWrapMode::Extend),
                        f32::INFINITY,
                        egui::TextStyle::Body,
                    )
            })
            .collect();
        let width: f32 = galleys.iter().map(|g| disc + gap + g.size().x).sum::<f32>()
            + line * (self.steps.len().saturating_sub(1)) as f32
            + 2.0 * gap * (self.steps.len().saturating_sub(1)) as f32;
        let (rect, response) = ui.allocate_exact_size(vec2(width, disc), Sense::hover());
        let label = format!(
            "Étape {} sur {} : {}",
            self.current + 1,
            self.steps.len(),
            self.steps.get(self.current).copied().unwrap_or_default()
        );
        response.widget_info(|| WidgetInfo::labeled(Role::Label, true, &label));
        let painter = ui.painter();
        let mut x = rect.min.x;
        let cy = rect.center().y;
        let count = galleys.len();
        for (i, g) in galleys.into_iter().enumerate() {
            let c = pos2(x + disc / 2.0, cy);
            let done = i < self.current;
            let current = i == self.current;
            if done || current {
                painter.circle_filled(c, disc / 2.0, p.primary_dark);
            } else {
                painter.circle(c, disc / 2.0 - 0.75, p.white, Stroke::new(1.5, p.gray_400));
            }
            if done {
                Icon::Check.paint(ui, Rect::from_center_size(c, Vec2::splat(16.0)), p.white);
            } else {
                let num = format!("{}", i + 1);
                let color = if current { p.white } else { p.text_secondary };
                painter.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    num,
                    font(Weight::Bold, p.text_sm),
                    color,
                );
            }
            x += disc + gap;
            let gy = cy - g.size().y / 2.0;
            let gw = g.size().x;
            painter.galley(pos2(x, gy), g, Color32::PLACEHOLDER);
            x += gw;
            if i + 1 < count {
                x += gap;
                let color = if i < self.current {
                    p.primary_dark
                } else {
                    p.gray_300
                };
                painter.line_segment([pos2(x, cy), pos2(x + line, cy)], Stroke::new(2.0, color));
                x += line + gap;
            }
        }
        response
    }
}

/// Sélecteur segmenté (deux à quatre choix exclusifs) en pilule.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    group_label: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let p = palette();
    let pad = 4.0;
    let item_h = 32.0;
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, l)| {
            egui::WidgetText::from(RichText::new(*l).font(font(Weight::Semibold, p.text_sm)))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 32.0).collect();
    let total = widths.iter().sum::<f32>() + 2.0 * pad;
    let (outer, response) = ui.allocate_exact_size(vec2(total, item_h + 2.0 * pad), Sense::hover());
    let group = group_label.to_owned();
    response.widget_info(|| WidgetInfo::labeled(Role::RadioGroup, true, &group));
    ui.painter().rect(
        outer,
        radius(p.radius_full),
        p.bg_muted,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut x = outer.min.x + pad;
    let mut changed = false;
    for (((opt, label), g), w) in options.iter().zip(galleys).zip(widths) {
        let rect = Rect::from_min_size(pos2(x, outer.min.y + pad), vec2(w, item_h));
        let id = response.id.with(label);
        let r = ui.interact(rect, id, Sense::click());
        let selected = *value == *opt;
        let label_owned = (*label).to_owned();
        r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, &label_owned));
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, radius(p.radius_full), p.primary_dark);
        } else if r.hovered() {
            painter.rect_filled(rect, radius(p.radius_full), p.gray_200);
        }
        let ink = if selected { p.white } else { p.text };
        painter.galley(rect.center() - g.size() / 2.0, g, ink);
        if r.has_focus() {
            painter.rect_stroke(
                rect.expand(1.0),
                radius(p.radius_full),
                Stroke::new(2.0, p.border_focus),
                StrokeKind::Outside,
            );
        }
        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !selected {
            *value = *opt;
            changed = true;
        }
        x += w;
    }
    let mut response = response;
    if changed {
        response.mark_changed();
    }
    response
}

/// Puce de filtre (bascule) : contour fin, active en teinte avec contour coloré.
pub fn filter_chip(
    ui: &mut Ui,
    label: &str,
    count: Option<usize>,
    selected: bool,
    tint: crate::ui::theme::Tinted,
) -> Response {
    let p = palette();
    let text = match count {
        Some(n) => format!("{label} ({n})"),
        None => label.to_owned(),
    };
    let g = egui::WidgetText::from(RichText::new(&text).font(font(Weight::Medium, p.text_sm)))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        );
    let dot = 8.0;
    let size = vec2(g.size().x + 28.0 + dot + 6.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, selected, &text));
    let painter = ui.painter();
    let (fill, edge, ink) = if selected {
        (tint.fill, tint.ink, tint.ink)
    } else if response.hovered() {
        (p.gray_50, p.gray_500, p.text)
    } else {
        (p.white, p.gray_300, p.text)
    };
    painter.rect(
        rect,
        radius(p.radius_full),
        fill,
        Stroke::new(if selected { 1.5 } else { 1.0 }, edge),
        StrokeKind::Inside,
    );
    let dc = pos2(rect.min.x + 14.0 + dot / 2.0, rect.center().y);
    painter.circle_filled(dc, dot / 2.0, tint.edge);
    painter.galley(
        pos2(dc.x + dot / 2.0 + 6.0, rect.center().y - g.size().y / 2.0),
        g,
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
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
