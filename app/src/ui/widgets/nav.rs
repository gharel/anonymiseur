//! Navigation : onglets de l'en-tête, étapes, sélecteur segmenté, filtres.

use super::Icon;
use crate::ui::theme::{CONTROL_MD, Weight, font, palette, radius};
use egui::{
    Color32, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, pos2,
    vec2,
};

/// Onglets de l'en-tête : texte moyen, actif en vert foncé souligné (comme le
/// `Header` de la charte).
#[derive(Debug)]
pub struct TabBar<'a> {
    tabs: &'a [(Icon, &'a str)],
    /// Raccourci de chaque onglet, rappelé en infobulle.
    hints: &'a [String],
}

impl<'a> TabBar<'a> {
    pub fn new(tabs: &'a [(Icon, &'a str)]) -> Self {
        Self { tabs, hints: &[] }
    }

    pub fn hints(mut self, hints: &'a [String]) -> Self {
        self.hints = hints;
        self
    }

    /// Affiche les onglets ; renvoie l'index cliqué.
    pub fn show(self, ui: &mut Ui, active: usize) -> Option<usize> {
        let p = palette();
        let mut clicked = None;
        // La rangée est placée à sa hauteur finale : centrée sur la ligne parente.
        ui.spacing_mut().interact_size.y = CONTROL_MD;
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
                let size = vec2(pad_x * 2.0 + icon_size + 8.0 + galley.size().x, CONTROL_MD);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                let label_owned = (*label).to_owned();
                response
                    .widget_info(|| WidgetInfo::selected(Role::Tab, true, is_active, &label_owned));
                let hovered = response.hovered();
                let ink = if is_active || hovered {
                    p.accent
                } else {
                    p.text
                };
                let painter = ui.painter();
                if is_active {
                    painter.rect_filled(rect, radius(p.radius_full), p.primary_subtle);
                } else if response.is_pointer_button_down_on() {
                    painter.rect_filled(rect, radius(p.radius_full), p.hover_strong);
                } else if hovered {
                    painter.rect_filled(rect, radius(p.radius_full), p.muted);
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
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let response = match self.hints.get(i) {
                    Some(h) => response.on_hover_text(format!("{label} ({h})")),
                    None => response,
                };
                if response.clicked() {
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
                    p.heading
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
                painter.circle(
                    c,
                    disc / 2.0 - 0.75,
                    p.surface,
                    Stroke::new(1.5, p.line_strong),
                );
            }
            if done {
                Icon::Check.paint(
                    ui,
                    Rect::from_center_size(c, Vec2::splat(16.0)),
                    p.on_primary,
                );
            } else {
                let num = format!("{}", i + 1);
                let color = if current {
                    p.on_primary
                } else {
                    p.text_secondary
                };
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
                    p.line
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
        p.muted,
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
        } else if r.is_pointer_button_down_on() {
            painter.rect_filled(rect, radius(p.radius_full), p.line);
        } else if r.hovered() {
            painter.rect_filled(rect, radius(p.radius_full), p.hover_strong);
        }
        let ink = if selected { p.on_primary } else { p.text };
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
    } else if response.is_pointer_button_down_on() {
        (p.hover_strong, p.line_strong, p.text)
    } else if response.hovered() {
        (p.hover, p.line_strong, p.text)
    } else {
        (p.surface, p.line, p.text)
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

/// Icône d'un choix de thème.
fn theme_icon(choice: crate::ui::theme::ThemeChoice) -> Icon {
    use crate::ui::theme::ThemeChoice;
    match choice {
        ThemeChoice::System => Icon::Monitor,
        ThemeChoice::Light => Icon::Sun,
        ThemeChoice::Dark => Icon::Moon,
    }
}

/// Bouton d'icône de l'en-tête : menu Système, Clair, Sombre. Renvoie vrai si
/// le choix a changé.
pub fn theme_button(ui: &mut Ui, choice: &mut crate::ui::theme::ThemeChoice) -> bool {
    use crate::ui::theme::{CONTROL_MD, ThemeChoice};
    let p = palette();
    let label = format!("Thème : {}", choice.label());
    let r = super::button::icon_button_sized(ui, theme_icon(*choice), &label, p.text, CONTROL_MD);
    let mut changed = false;
    egui::Popup::menu(&r).gap(4.0).show(|ui| {
        ui.set_min_width(180.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        for c in ThemeChoice::ALL {
            let selected = *choice == c;
            let (rect, row) =
                ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
            let name = c.label().to_owned();
            row.widget_info(|| WidgetInfo::selected(Role::MenuItemRadio, true, selected, &name));
            let painter = ui.painter();
            if row.hovered() || row.has_focus() {
                painter.rect_filled(rect, radius(p.radius_sm), p.hover);
            }
            let ink = if selected { p.accent } else { p.text };
            theme_icon(c).paint(
                ui,
                Rect::from_center_size(pos2(rect.min.x + 18.0, rect.center().y), Vec2::splat(18.0)),
                ink,
            );
            let weight = if selected {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            painter.text(
                pos2(rect.min.x + 36.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                c.label(),
                font(weight, p.text_sm),
                ink,
            );
            if selected {
                Icon::Check.paint(
                    ui,
                    Rect::from_center_size(
                        pos2(rect.max.x - 16.0, rect.center().y),
                        Vec2::splat(16.0),
                    ),
                    p.accent,
                );
            }
            if row
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !selected
            {
                *choice = c;
                changed = true;
            }
        }
    });
    changed
}
