//! Champ de texte et champ mot de passe : libellé demi-gras, bord
//! `color-border-input`, rayon 8 px, anneau de focus de 2 px, message d'erreur
//! en `color-error`.

use super::Icon;
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{Frame, Id, Margin, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};

/// Rayon des champs demandé pour l'application (8 px).
const FIELD_RADIUS: f32 = 8.0;

#[derive(Debug)]
pub struct TextField<'a> {
    label: &'a str,
    value: &'a mut String,
    placeholder: Option<&'a str>,
    hint: Option<&'a str>,
    error: Option<&'a str>,
    password: bool,
    required: bool,
    rows: usize,
    id_salt: Option<&'a str>,
    width: Option<f32>,
    show_label: bool,
}

impl<'a> TextField<'a> {
    pub fn new(label: &'a str, value: &'a mut String) -> Self {
        Self {
            label,
            value,
            placeholder: None,
            hint: None,
            error: None,
            password: false,
            required: false,
            rows: 1,
            id_salt: None,
            width: None,
            show_label: true,
        }
    }

    pub fn password(label: &'a str, value: &'a mut String) -> Self {
        let mut f = Self::new(label, value);
        f.password = true;
        f
    }

    pub fn placeholder(mut self, text: &'a str) -> Self {
        self.placeholder = Some(text);
        self
    }

    pub fn hint(mut self, text: &'a str) -> Self {
        self.hint = Some(text);
        self
    }

    pub fn error(mut self, text: Option<&'a str>) -> Self {
        self.error = text;
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Zone multiligne de `rows` lignes.
    pub fn multiline(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    pub fn id_salt(mut self, salt: &'a str) -> Self {
        self.id_salt = Some(salt);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Libellé lu par les lecteurs d'écran mais non affiché.
    pub fn hide_label(mut self) -> Self {
        self.show_label = false;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette();
        let width = self.width.unwrap_or_else(|| ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 4.0;
            let label_id = if self.show_label {
                let mut text = RichText::new(self.label)
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.text);
                if self.required {
                    text = RichText::new(format!("{} *", self.label))
                        .font(font(Weight::Semibold, p.text_sm))
                        .color(p.text);
                }
                Some(ui.label(text).id)
            } else {
                None
            };

            let base_id = Id::unique(("champ", self.id_salt.unwrap_or(self.label)));
            let edit_id = base_id.with("edit");
            let reveal_id = base_id.with("reveal");
            let focused = ui.memory(|m| m.has_focus(edit_id));
            let error = self.error.is_some();
            let edge = if error {
                p.error
            } else if focused {
                p.border_focus
            } else {
                p.border_input
            };
            let r = radius(FIELD_RADIUS);
            let shown: bool = ui.data(|d| d.get_temp(reveal_id).unwrap_or(false));
            let password = self.password;
            let rows = self.rows;
            let placeholder = self.placeholder;
            let label = self.label;
            let value = self.value;

            let framed = Frame::new()
                .fill(p.white)
                .stroke(Stroke::new(1.5, edge))
                .corner_radius(r)
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let reveal_w = if password { 28.0 } else { 0.0 };
                        let w = ui.available_width() - reveal_w;
                        let text_font = font(Weight::Regular, p.text_sm);
                        let mut edit = if rows > 1 {
                            TextEdit::multiline(value).desired_rows(rows)
                        } else {
                            TextEdit::singleline(value)
                        };
                        edit = edit
                            .id(edit_id)
                            .frame(Frame::NONE)
                            .margin(Margin::ZERO)
                            .font(text_font)
                            .text_color(p.text)
                            .desired_width(w)
                            .align(if rows > 1 {
                                egui::Align2::LEFT_TOP
                            } else {
                                egui::Align2::LEFT_CENTER
                            })
                            .password(password && !shown);
                        if let Some(ph) = placeholder {
                            edit = edit.hint_text(RichText::new(ph).color(p.text_secondary));
                        }
                        let response = ui.add(edit);
                        if password {
                            let (icon, tip) = if shown {
                                (Icon::EyeOff, "Masquer le mot de passe")
                            } else {
                                (Icon::Eye, "Afficher le mot de passe")
                            };
                            let (rect, eye) =
                                ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                            let tip_owned = tip.to_owned();
                            eye.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, &tip_owned)
                            });
                            let color = if eye.hovered() {
                                p.primary_dark
                            } else {
                                p.text_secondary
                            };
                            icon.paint(ui, rect, color);
                            if eye.has_focus() {
                                ui.painter().rect_stroke(
                                    rect.expand(3.0),
                                    radius(p.radius_sm),
                                    Stroke::new(2.0, p.border_focus),
                                    StrokeKind::Outside,
                                );
                            }
                            if eye.on_hover_text(tip).clicked() {
                                ui.data_mut(|d| d.insert_temp(reveal_id, !shown));
                            }
                        }
                        response
                    })
                    .inner
                });
            if focused {
                let ring = if error { p.error } else { p.border_focus };
                ui.painter().rect_stroke(
                    framed.response.rect.expand(2.0),
                    radius(FIELD_RADIUS + 2.0),
                    Stroke::new(2.0, ring),
                    StrokeKind::Outside,
                );
            }
            let mut response = framed.inner;
            if let Some(id) = label_id {
                response = response.labelled_by(id);
            } else {
                let label_owned = label.to_owned();
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, &label_owned)
                });
            }
            if let Some(msg) = self.error {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    Icon::TriangleAlert.show(ui, p.error, 14.0);
                    ui.label(
                        RichText::new(msg)
                            .font(font(Weight::Medium, p.text_xs))
                            .color(p.error),
                    );
                });
            } else if let Some(hint) = self.hint {
                ui.label(
                    RichText::new(hint)
                        .font(font(Weight::Regular, p.text_xs))
                        .color(p.text_secondary),
                );
            }
            response
        })
        .inner
    }
}

/// Identifiant du champ, pour donner le focus depuis l'extérieur.
pub fn field_id(salt: &str) -> Id {
    Id::unique(("champ", salt)).with("edit")
}
