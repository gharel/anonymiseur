//! Champ de texte et champ mot de passe : libellé demi-gras, bord
//! `color-border-input`, rayon 8 px, anneau de focus de 2 px, message d'erreur
//! en `color-error`.
//!
//! Un champ d'une ligne a la hauteur d'un contrôle ([`CONTROL_MD`] par défaut) :
//! posé à côté d'un sélecteur ou d'un bouton, il est aligné sur eux. Le champ
//! entier est la zone cliquable et le nœud d'accessibilité.
//!
//! Une zone multiligne a une hauteur fixe : le texte défile à l'intérieur, en
//! police à chasse fixe de 15 px avec un interligne confortable.

use super::Icon;
use crate::ui::theme::{
    CONTROL_MD, FIELD_BORDER, FIELD_PAD_X, FIELD_RADIUS, TEXT_AREA_LEADING, TEXT_AREA_SIZE, Weight,
    font, palette, radius,
};
use egui::{
    Frame, Id, Margin, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit,
    Ui, UiBuilder, Vec2, pos2, vec2,
};

/// Côté de la zone cliquable de l'œil (afficher le mot de passe).
const REVEAL: f32 = 28.0;
/// Marge verticale intérieure d'une zone multiligne.
const AREA_PAD_Y: f32 = 10.0;

#[derive(Debug)]
pub struct TextField<'a> {
    label: &'a str,
    value: &'a mut String,
    placeholder: Option<&'a str>,
    hint: Option<&'a str>,
    error: Option<&'a str>,
    password: bool,
    required: bool,
    /// Hauteur d'une zone multiligne (`None` : champ d'une ligne).
    area_height: Option<f32>,
    id_salt: Option<&'a str>,
    width: Option<f32>,
    height: f32,
    show_label: bool,
    monospace: bool,
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
            area_height: None,
            id_salt: None,
            width: None,
            height: CONTROL_MD,
            show_label: true,
            monospace: false,
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

    /// Zone multiligne d'au moins `rows` lignes visibles.
    pub fn multiline(self, rows: usize) -> Self {
        let h = area_height_for_rows(rows.max(1));
        self.area_height(h)
    }

    /// Zone multiligne de hauteur totale `height` (bord compris).
    pub fn area_height(mut self, height: f32) -> Self {
        self.area_height = Some(height.max(area_height_for_rows(1)));
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

    /// Hauteur d'un champ d'une ligne (une des hauteurs de contrôle du thème).
    pub fn control_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Texte à chasse fixe : les balises `[SOCIETE_1]` restent lisibles (le
    /// tiret bas de Georama est très large).
    pub fn monospace(mut self) -> Self {
        self.monospace = true;
        self
    }

    /// Libellé lu par les lecteurs d'écran mais non affiché.
    pub fn hide_label(mut self) -> Self {
        self.show_label = false;
        self
    }

    /// Libellé, champ, puis message d'erreur ou texte d'aide.
    pub fn show(self, ui: &mut Ui) -> Response {
        self.render(ui, true)
    }

    /// Le champ seul, sans message dessous : pour une ligne de contrôles où le
    /// champ doit avoir exactement la hauteur des autres. Une erreur ne colore
    /// que le bord ; le message est affiché par l'appelant sous la ligne.
    pub fn show_control(self, ui: &mut Ui) -> Response {
        self.render(ui, false)
    }

    fn render(self, ui: &mut Ui, messages: bool) -> Response {
        let p = palette();
        let width = self.width.unwrap_or_else(|| ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 4.0;
            let label_id = if self.show_label {
                let text = if self.required {
                    format!("{} *", self.label)
                } else {
                    self.label.to_owned()
                };
                let text = RichText::new(text)
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.text);
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
            let shown: bool = ui.data(|d| d.get_temp(reveal_id).unwrap_or(false));
            let height = self.area_height.unwrap_or(self.height);
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let hovered = ui.rect_contains_pointer(rect);
            let edge = if !error && !focused && hovered {
                p.accent
            } else {
                edge
            };
            ui.painter().rect(
                rect,
                radius(FIELD_RADIUS),
                p.surface,
                Stroke::new(FIELD_BORDER, edge),
                StrokeKind::Inside,
            );
            let text_font = if self.monospace {
                mono_font()
            } else {
                font(Weight::Regular, p.text_sm)
            };
            let hint = self.placeholder.map(|ph| {
                RichText::new(ph)
                    .font(text_font.clone())
                    .color(p.text_secondary)
            });

            let mut response = match self.area_height {
                None => {
                    let reveal_w = if self.password { REVEAL } else { 0.0 };
                    let mut edit = TextEdit::singleline(self.value)
                        .id(edit_id)
                        // Le cadre est dessiné par le champ ; la marge passe par
                        // le cadre (egui ignore `margin` quand un cadre est donné).
                        .frame(Frame::new().inner_margin(Margin {
                            left: FIELD_PAD_X as i8,
                            right: (FIELD_PAD_X + reveal_w) as i8,
                            top: 0,
                            bottom: 0,
                        }))
                        .font(text_font)
                        .text_color(p.text)
                        .desired_width(rect.width())
                        .min_size(rect.size())
                        .align(egui::Align2::LEFT_CENTER)
                        .password(self.password && !shown);
                    if let Some(h) = hint {
                        edit = edit.hint_text(h);
                    }
                    let r = ui.place(rect, edit);
                    if self.password {
                        reveal_button(ui, rect, shown, reveal_id);
                    }
                    r
                }
                Some(_) => {
                    let inner = rect.shrink2(vec2(FIELD_PAD_X - 4.0, AREA_PAD_Y));
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .max_rect(inner)
                            .id_salt(base_id.with("zone")),
                    );
                    child.spacing_mut().extra_text_line_spacing = TEXT_AREA_LEADING;
                    ScrollArea::vertical()
                        .id_salt(base_id.with("defilement"))
                        .max_height(inner.height())
                        .auto_shrink([false, false])
                        .show(&mut child, |ui| {
                            let mut edit = TextEdit::multiline(self.value)
                                .id(edit_id)
                                .frame(Frame::new().inner_margin(Margin::symmetric(4, 0)))
                                .font(text_font)
                                .text_color(p.text)
                                .desired_width(ui.available_width())
                                .min_size(vec2(ui.available_width(), inner.height()))
                                .align(egui::Align2::LEFT_TOP);
                            if let Some(h) = hint {
                                edit = edit.hint_text(h);
                            }
                            ui.add(edit)
                        })
                        .inner
                }
            };

            if focused {
                let ring = if error { p.error } else { p.border_focus };
                ui.painter().rect_stroke(
                    rect.expand(2.0),
                    radius(FIELD_RADIUS + 2.0),
                    Stroke::new(2.0, ring),
                    StrokeKind::Outside,
                );
            }
            if let Some(id) = label_id {
                response = response.labelled_by(id);
            } else {
                let label_owned = self.label.to_owned();
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, &label_owned)
                });
            }
            response = response.on_hover_cursor(egui::CursorIcon::Text);
            if !messages {
                return response;
            }
            if let Some(msg) = self.error {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    Icon::TriangleAlert.show(ui, p.error, 14.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(msg)
                                .font(font(Weight::Medium, p.text_xs))
                                .color(p.error),
                        )
                        .wrap(),
                    );
                });
            } else if let Some(hint) = self.hint {
                ui.add(
                    egui::Label::new(
                        RichText::new(hint)
                            .font(font(Weight::Regular, p.text_xs))
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
            }
            response
        })
        .inner
    }
}

/// Œil « afficher / masquer le mot de passe », dans le champ, à droite.
fn reveal_button(ui: &mut Ui, field: Rect, shown: bool, reveal_id: Id) {
    let p = palette();
    let (icon, tip) = if shown {
        (Icon::EyeOff, "Masquer le mot de passe")
    } else {
        (Icon::Eye, "Afficher le mot de passe")
    };
    let rect = Rect::from_center_size(
        pos2(
            field.max.x - FIELD_PAD_X / 2.0 - REVEAL / 2.0,
            field.center().y,
        ),
        Vec2::splat(REVEAL),
    );
    let eye = ui.interact(rect, reveal_id.with("bouton"), Sense::click());
    let tip_owned = tip.to_owned();
    eye.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &tip_owned));
    let color = if eye.hovered() {
        p.accent
    } else {
        p.text_secondary
    };
    if eye.hovered() {
        ui.painter()
            .rect_filled(rect, radius(p.radius_full), p.muted);
    }
    icon.paint(
        ui,
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        color,
    );
    if eye.has_focus() {
        ui.painter().rect_stroke(
            rect,
            radius(p.radius_full),
            Stroke::new(2.0, p.border_focus),
            StrokeKind::Outside,
        );
    }
    if eye
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        ui.data_mut(|d| d.insert_temp(reveal_id, !shown));
    }
}

/// Hauteur d'une ligne de texte d'une zone multiligne (interligne compris).
pub fn area_line_height() -> f32 {
    // Hack : hauteur de ligne ≈ 1,17 × la taille ; interligne en plus.
    (TEXT_AREA_SIZE * 1.17).round() + TEXT_AREA_LEADING
}

/// Hauteur totale d'une zone multiligne de `rows` lignes visibles.
pub fn area_height_for_rows(rows: usize) -> f32 {
    rows as f32 * area_line_height() + 2.0 * AREA_PAD_Y
}

/// Mise en page d'un texte à balises (police à chasse fixe de 15 px,
/// interligne des zones multilignes), coupé à `wrap_width`.
pub fn mono_job(text: &str, color: egui::Color32, wrap_width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: mono_font(),
            color,
            line_height: Some(area_line_height()),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap_width;
    job
}

/// Comme [`mono_job`], avec les plages `marks` (octets) surlignées : fond
/// d'avertissement et texte foncé, pour les balises inconnues.
pub fn mono_job_marked(
    text: &str,
    marks: &[std::ops::Range<usize>],
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let p = palette();
    let mut job = egui::text::LayoutJob::default();
    let normal = egui::TextFormat {
        font_id: mono_font(),
        color: p.text,
        line_height: Some(area_line_height()),
        ..Default::default()
    };
    let marked = egui::TextFormat {
        background: p.data.fill,
        color: p.data.ink,
        underline: egui::Stroke::new(1.5, p.warning),
        ..normal.clone()
    };
    let mut at = 0;
    for m in marks {
        let (Some(before), Some(tag)) = (text.get(at..m.start), text.get(m.clone())) else {
            continue;
        };
        job.append(before, 0.0, normal.clone());
        job.append(tag, 0.0, marked.clone());
        at = m.end;
    }
    job.append(text.get(at..).unwrap_or_default(), 0.0, normal);
    job.wrap.max_width = wrap_width;
    job
}

/// Police à chasse fixe d'une balise isolée dans un tableau (taille du texte courant).
pub fn tag_font() -> egui::FontId {
    egui::FontId::monospace(palette().text_sm - 1.0)
}

/// Police à chasse fixe des textes contenant des balises.
pub fn mono_font() -> egui::FontId {
    egui::FontId::monospace(TEXT_AREA_SIZE)
}

/// Identifiant du champ, pour donner le focus depuis l'extérieur.
pub fn field_id(salt: &str) -> Id {
    Id::unique(("champ", salt)).with("edit")
}
