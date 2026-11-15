//! Navigation : étapes du dossier, palette de commandes, sélecteur segmenté,
//! menus en pilule, filtres, thème.

use super::{Icon, keycap_size, paint_keycap};
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette, radius};
use egui::{
    Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, pos2, vec2,
};

/// Une étape de l'indicateur : libellé, état, raison si elle est inaccessible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepItem<'a> {
    pub label: &'a str,
    pub done: bool,
    pub current: bool,
    /// Étape inaccessible : la raison, en infobulle.
    pub blocked: Option<&'a str>,
    /// Raccourci rappelé en infobulle.
    pub shortcut: Option<&'a str>,
}

/// Écart entre deux étapes (chevron compris).
const STEP_GAP: f32 = 24.0;
/// Marge horizontale d'une étape.
const STEP_PAD: f32 = 14.0;
/// Coche d'une étape faite et son écart avec le libellé.
const STEP_CHECK: f32 = 16.0 + 6.0;

/// Indicateur d'étapes du dossier, en pilules reliées par des chevrons :
/// étape courante pleine, étapes faites cochées, étapes à venir en gris.
/// Chaque étape est un onglet cliquable ; renvoie l'index cliqué.
#[derive(Debug)]
pub struct StepBar<'a> {
    steps: &'a [StepItem<'a>],
}

impl<'a> StepBar<'a> {
    pub fn new(steps: &'a [StepItem<'a>]) -> Self {
        Self { steps }
    }

    fn item_width(ui: &Ui, s: &StepItem<'_>) -> f32 {
        let p = palette();
        let g = ui.painter().layout_no_wrap(
            s.label.to_owned(),
            font(Weight::Semibold, p.text_sm),
            p.text,
        );
        let check = if s.done { STEP_CHECK } else { 0.0 };
        g.size().x + check + 2.0 * STEP_PAD
    }

    /// Largeur totale, pour centrer la barre.
    pub fn width(&self, ui: &Ui) -> f32 {
        let items: f32 = self.steps.iter().map(|s| Self::item_width(ui, s)).sum();
        items + STEP_GAP * self.steps.len().saturating_sub(1) as f32
    }

    pub fn show(self, ui: &mut Ui) -> Option<usize> {
        let p = palette();
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let count = self.steps.len();
            for (i, s) in self.steps.iter().enumerate() {
                let size = vec2(Self::item_width(ui, s), CONTROL_SM);
                let enabled = s.blocked.is_none();
                let sense = if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, response) = ui.allocate_exact_size(size, sense);
                let label = s.label.to_owned();
                let current = s.current;
                response.widget_info(|| WidgetInfo::selected(Role::Tab, enabled, current, &label));
                let hovered = enabled && response.hovered();
                let (fill, ink) = if s.current {
                    (Some(p.primary_dark), p.on_primary)
                } else if hovered {
                    (Some(p.muted), p.accent)
                } else if s.done {
                    (None, p.accent)
                } else {
                    (None, p.text_secondary)
                };
                if let Some(f) = fill {
                    ui.painter().rect_filled(rect, radius(p.radius_full), f);
                }
                let mut x = rect.min.x + STEP_PAD;
                if s.done {
                    Icon::Check.paint(
                        ui,
                        Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(16.0)),
                        ink,
                    );
                    x += STEP_CHECK;
                }
                let galley = ui.painter().layout_no_wrap(
                    s.label.to_owned(),
                    font(Weight::Semibold, p.text_sm),
                    ink,
                );
                let gy = rect.center().y - galley.size().y / 2.0;
                ui.painter().galley(pos2(x, gy), galley, ink);
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect.expand(2.0),
                        radius(p.radius_full),
                        Stroke::new(2.0, p.border_focus),
                        StrokeKind::Outside,
                    );
                }
                let tip = match (s.blocked, s.shortcut) {
                    (Some(why), _) => why.to_owned(),
                    (None, Some(k)) => format!("Aller à l'étape {} ({k})", s.label),
                    (None, None) => format!("Aller à l'étape {}", s.label),
                };
                let response = if enabled {
                    response.on_hover_cursor(egui::CursorIcon::PointingHand)
                } else {
                    response
                };
                if response.on_hover_text(tip).clicked() && enabled {
                    clicked = Some(i);
                }
                if i + 1 < count {
                    let (sep, _) =
                        ui.allocate_exact_size(vec2(STEP_GAP, CONTROL_SM), Sense::hover());
                    Icon::ChevronRight.paint(
                        ui,
                        Rect::from_center_size(sep.center(), Vec2::splat(16.0)),
                        p.line_strong,
                    );
                }
            }
        });
        clicked
    }
}

/// Largeur minimale du bouton de la palette, avec son libellé.
const COMMAND_MIN_W: f32 = 220.0;

/// Bouton de l'en-tête qui ouvre la palette de commandes : loupe,
/// « Commandes » et la touche. `compact` : sans le mot, pour les fenêtres étroites.
pub fn command_button(ui: &mut Ui, shortcut: &str, compact: bool) -> Response {
    let p = palette();
    let galley = ui.painter().layout_no_wrap(
        "Commandes".to_owned(),
        font(Weight::Regular, p.text_sm),
        p.text_secondary,
    );
    let kbd = keycap_size(ui, shortcut);
    let pad = 12.0;
    let icon = 18.0;
    let width = if compact {
        pad + icon + 8.0 + kbd.x + 8.0
    } else {
        (pad + icon + 8.0 + galley.size().x + 16.0 + kbd.x + 8.0).max(COMMAND_MIN_W)
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, CONTROL_MD), Sense::click());
    let name = format!("Palette de commandes ({shortcut})");
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &name));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        ui.painter().rect(
            rect,
            radius(p.radius_md),
            if hovered { p.hover } else { p.surface },
            Stroke::new(1.0, if hovered { p.accent } else { p.border_input }),
            StrokeKind::Inside,
        );
        let mut x = rect.min.x + pad;
        Icon::Search.paint(
            ui,
            Rect::from_center_size(pos2(x + icon / 2.0, rect.center().y), Vec2::splat(icon)),
            p.text_secondary,
        );
        x += icon + 8.0;
        if !compact {
            let gy = rect.center().y - galley.size().y / 2.0;
            ui.painter().galley(pos2(x, gy), galley, p.text_secondary);
        }
        let k = Rect::from_center_size(pos2(rect.max.x - 8.0 - kbd.x / 2.0, rect.center().y), kbd);
        paint_keycap(ui, k, shortcut, false, true);
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                radius(p.radius_md + 2.0),
                Stroke::new(2.0, p.border_focus),
                StrokeKind::Outside,
            );
        }
    }
    response
        .on_hover_text(format!("Toutes les actions, par recherche ({shortcut})"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Pilule « Libellé : valeur ▾ » qui ouvre un menu de choix exclusifs.
/// Renvoie le choix fait dans le menu.
pub fn pill_menu<T: PartialEq + Copy>(
    ui: &mut Ui,
    prefix: &str,
    value: T,
    options: &[(T, &str)],
) -> Option<T> {
    let p = palette();
    let current = options
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| *l)
        .unwrap_or_default();
    let pre = ui.painter().layout_no_wrap(
        format!("{prefix} : "),
        font(Weight::Regular, p.text_sm),
        p.text,
    );
    let val = ui.painter().layout_no_wrap(
        current.to_owned(),
        font(Weight::Semibold, p.text_sm),
        p.heading,
    );
    let pad = 14.0;
    let width = pad + pre.size().x + val.size().x + 6.0 + 16.0 + pad;
    let (rect, response) = ui.allocate_exact_size(vec2(width, CONTROL_SM), Sense::click());
    let name = format!("{prefix} : {current}");
    response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, &name));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        ui.painter().rect(
            rect,
            radius(p.radius_full),
            if hovered { p.hover } else { p.surface },
            Stroke::new(1.0, if hovered { p.accent } else { p.border_input }),
            StrokeKind::Inside,
        );
        let cy = rect.center().y;
        let mut x = rect.min.x + pad;
        let pre_w = pre.size().x;
        ui.painter()
            .galley(pos2(x, cy - pre.size().y / 2.0), pre, p.text);
        x += pre_w;
        let val_w = val.size().x;
        ui.painter()
            .galley(pos2(x, cy - val.size().y / 2.0), val, p.heading);
        x += val_w + 6.0;
        Icon::ChevronDown.paint(
            ui,
            Rect::from_center_size(pos2(x + 8.0, cy), Vec2::splat(16.0)),
            p.text_secondary,
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                radius(p.radius_full),
                Stroke::new(2.0, p.border_focus),
                StrokeKind::Outside,
            );
        }
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut chosen = None;
    egui::Popup::menu(&response).gap(4.0).show(|ui| {
        ui.set_min_width(rect.width().max(260.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        for (v, label) in options {
            let selected = *v == value;
            let (row_rect, row) =
                ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
            let name = (*label).to_owned();
            row.widget_info(|| WidgetInfo::selected(Role::MenuItemRadio, true, selected, &name));
            if row.hovered() || row.has_focus() {
                ui.painter()
                    .rect_filled(row_rect, radius(p.radius_sm), p.hover);
            }
            let ink = if selected { p.accent } else { p.text };
            let weight = if selected {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            ui.painter().text(
                pos2(row_rect.min.x + 12.0, row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                *label,
                font(weight, p.text_sm),
                ink,
            );
            if selected {
                Icon::Check.paint(
                    ui,
                    Rect::from_center_size(
                        pos2(row_rect.max.x - 16.0, row_rect.center().y),
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
                chosen = Some(*v);
            }
        }
    });
    chosen
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
