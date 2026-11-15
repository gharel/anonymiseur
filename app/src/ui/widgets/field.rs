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
    let marks: Vec<(std::ops::Range<usize>, Mark)> =
        marks.iter().map(|r| (r.clone(), Mark::Unknown)).collect();
    mono_job_marks(text, &marks, wrap_width)
}

/// Nature d'une plage surlignée dans un texte affiché (réponse restaurée,
/// vérification du presse-papiers, extrait autour de la valeur à décider).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Valeur remise à la place de sa balise : couleur « masqué » neutre.
    Restored,
    /// Balise inconnue de la clé : couleur d'avertissement.
    Unknown,
    /// Valeur qui sera masquée : couleur « masqué » neutre.
    Masked,
    /// Valeur à décider : jaune.
    Pending,
    /// Valeur à décider en cours : jaune, soulignement épais.
    Current,
}

/// Espace entre le texte et le bord de son surlignage, en haut et en bas.
pub const MARK_PAD_Y: f32 = 2.0;

/// Couleurs d'une plage : fond, texte, soulignement.
pub fn mark_style(mark: Mark) -> (egui::Color32, egui::Color32, Stroke) {
    let p = palette();
    match mark {
        Mark::Restored | Mark::Masked => {
            (p.masked.fill, p.masked.ink, Stroke::new(1.5, p.masked.edge))
        }
        Mark::Unknown => (p.data.fill, p.data.ink, Stroke::new(1.5, p.warning)),
        Mark::Pending => (
            p.pending.fill,
            p.pending.ink,
            Stroke::new(1.5, p.pending.edge),
        ),
        Mark::Current => (
            p.pending.fill,
            p.pending.ink,
            Stroke::new(3.0, p.pending.edge),
        ),
    }
}

/// Texte et plages surlignées (octets, triées, sans chevauchement). Seule la
/// couleur du texte change dans la mise en page : le fond et le soulignement
/// sont peints à part ([`marked_label`]). Avec un interligne plus grand que la
/// police, epaint peint le fond d'une section sur toute la hauteur de ligne à
/// partir du haut du texte : il déborde sur la ligne suivante, et le
/// soulignement tombe dans la ligne suivante, sous son fond.
pub fn marks_job(
    text: &str,
    marks: &[(std::ops::Range<usize>, Mark)],
    base: &egui::TextFormat,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut at = 0;
    for (m, kind) in marks {
        let (Some(before), Some(part)) = (text.get(at..m.start), text.get(m.clone())) else {
            continue;
        };
        job.append(before, 0.0, base.clone());
        let format = egui::TextFormat {
            color: mark_style(*kind).1,
            ..base.clone()
        };
        job.append(part, 0.0, format);
        at = m.end;
    }
    job.append(text.get(at..).unwrap_or_default(), 0.0, base.clone());
    job.wrap.max_width = wrap_width;
    job
}

/// Format de base d'un texte à balises : police à chasse fixe, interligne des
/// zones multilignes.
pub fn mono_format() -> egui::TextFormat {
    egui::TextFormat {
        font_id: mono_font(),
        color: palette().text,
        line_height: Some(area_line_height()),
        ..Default::default()
    }
}

/// Comme [`mono_job`], avec des plages surlignées (octets, triées, sans
/// chevauchement) : valeurs restaurées et balises inconnues.
pub fn mono_job_marks(
    text: &str,
    marks: &[(std::ops::Range<usize>, Mark)],
    wrap_width: f32,
) -> egui::text::LayoutJob {
    marks_job(text, marks, &mono_format(), wrap_width)
}

/// Indice de caractère d'un octet du texte.
pub fn char_index(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |t| t.chars().count())
}

/// Rectangles d'un passage `a..b` (caractères) d'une galée, un par ligne, sur
/// toute la hauteur de la ligne (zone cliquable, passage à montrer).
pub fn range_rects(galley: &egui::Galley, a: usize, b: usize) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (lo, hi) = (a.max(idx), b.min(idx + n));
        if lo < hi {
            let x0 = row.pos.x + row.x_offset((lo - idx).into());
            let x1 = row.pos.x + row.x_offset((hi - idx).into());
            out.push(Rect::from_min_max(
                pos2(x0, row.min_y()),
                pos2(x1, row.max_y()),
            ));
        }
        idx += row.char_count_including_newline().0;
    }
    out
}

/// Bande d'une ligne `line` (toute sa hauteur) à la hauteur du texte, centrée
/// dans la ligne, plus [`MARK_PAD_Y`] en haut et en bas, jamais au-delà.
pub fn text_band(line: Rect, font_height: f32) -> Rect {
    let inset = ((line.height() - font_height) / 2.0 - MARK_PAD_Y).max(0.0);
    line.shrink2(vec2(0.0, inset))
}

/// Rectangles du fond d'un passage `a..b` (caractères) d'une galée, un par
/// ligne : la largeur du passage, la hauteur du texte centrée dans la ligne
/// plus [`MARK_PAD_Y`], jamais au-delà de la ligne.
pub fn mark_rects(galley: &egui::Galley, a: usize, b: usize, font_height: f32) -> Vec<Rect> {
    range_rects(galley, a, b)
        .into_iter()
        .map(|r| text_band(r, font_height))
        .collect()
}

/// Peint un surlignage sous le texte : fond `fill` dans le rectangle `r` (de
/// [`mark_rects`], en coordonnées de l'écran), soulignement `line` à son pied.
pub fn paint_mark(painter: &egui::Painter, r: Rect, fill: egui::Color32, line: Stroke) {
    painter.rect_filled(r, radius(2.0), fill);
    painter.hline(r.x_range(), r.max.y - line.width / 2.0, line);
}

/// Rectangles du fond de chaque plage d'un texte mis en page par [`marks_job`],
/// relatifs à la galée.
pub fn marks_rects(
    galley: &egui::Galley,
    text: &str,
    marks: &[(std::ops::Range<usize>, Mark)],
    font_height: f32,
) -> Vec<(Rect, Mark)> {
    marks
        .iter()
        .flat_map(|(m, kind)| {
            let (a, b) = (char_index(text, m.start), char_index(text, m.end));
            mark_rects(galley, a, b, font_height)
                .into_iter()
                .map(move |r| (r, *kind))
        })
        .collect()
}

/// Texte surligné, sélectionnable, coupé à la largeur disponible : fonds et
/// soulignements peints sous le texte, dans la hauteur de chaque ligne.
pub fn marked_label(
    ui: &mut Ui,
    text: &str,
    marks: &[(std::ops::Range<usize>, Mark)],
    base: &egui::TextFormat,
) -> Response {
    let job = marks_job(text, marks, base, ui.available_width());
    let (pos, galley, response) = egui::Label::new(job)
        .wrap()
        .selectable(true)
        .layout_in_ui(ui);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), galley.text())
    });
    if ui.is_rect_visible(response.rect) {
        let font_height = ui.fonts_mut(|f| f.row_height(&base.font_id));
        let painter = ui.painter();
        for (r, kind) in marks_rects(&galley, text, marks, font_height) {
            let (fill, _, line) = mark_style(kind);
            paint_mark(painter, r.translate(pos.to_vec2()), fill, line);
        }
        egui::text_selection::LabelSelectionState::label_text_selection(
            ui,
            &response,
            pos,
            galley,
            base.color,
            Stroke::NONE,
        );
    }
    response
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

    /// Réponse restaurée de l'instantané `dossier_4b_reponse` : valeurs voisines
    /// séparées par « . », passage à la ligne explicite.
    const TEXT: &str = "Le contrat lie Nautilus Pacifique SARL et Atelier Bougainville SAS. \
        Jean Dupont représente le premier prestataire et Martin Lefèvre la\nseconde société.";

    fn marks() -> Vec<(Range<usize>, Mark)> {
        [
            "Nautilus Pacifique SARL",
            "Atelier Bougainville SAS",
            "Jean Dupont",
            "Martin Lefèvre",
        ]
        .iter()
        .filter_map(|v| TEXT.find(v).map(|i| (i..i + v.len(), Mark::Restored)))
        .collect()
    }

    /// Met en page `job` avec les polices d'egui ; rend aussi la hauteur de la police.
    fn layout(job: egui::text::LayoutJob) -> (std::sync::Arc<egui::Galley>, f32) {
        let ctx = egui::Context::default();
        let font = mono_font();
        let mut got = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            got = Some(ui.fonts_mut(|f| (f.layout_job(job.clone()), f.row_height(&font))));
        });
        // En debug, epaint refuse qu'on abandonne des textures non appliquées.
        out.textures_delta.clear();
        got.unwrap_or_else(|| panic!("aucune mise en page"))
    }

    #[test]
    fn sections_follow_the_marks_without_painted_backgrounds() {
        let marks = marks();
        assert_eq!(marks.len(), 4);
        let job = mono_job_marks(TEXT, &marks, 400.0);
        assert_eq!(job.text, TEXT, "le texte est repris en entier");
        // Chaque plage tombe sur des sections entières, à sa couleur (deux
        // sections voisines de même format peuvent être fusionnées).
        let (_, ink, _) = mark_style(Mark::Restored);
        for (m, _) in &marks {
            let covering: Vec<_> = job
                .sections
                .iter()
                .filter(|s| s.byte_range.start.0 < m.end && m.start < s.byte_range.end.0)
                .collect();
            assert!(!covering.is_empty(), "aucune section pour une plage");
            for s in covering {
                assert!(s.byte_range.start.0 <= m.start && m.end <= s.byte_range.end.0);
                assert_eq!(s.format.color, ink);
            }
        }
        // Fond et soulignement ne sont jamais confiés à epaint (débordement sur
        // la ligne suivante avec un interligne).
        for s in &job.sections {
            assert_eq!(s.format.background, egui::Color32::TRANSPARENT);
            assert_eq!(s.format.underline, Stroke::NONE);
        }
    }

    #[test]
    fn highlights_stay_inside_their_line() {
        let marks = marks();
        let (galley, font_height) = layout(mono_job_marks(TEXT, &marks, 400.0));
        assert!(galley.rows.len() >= 4, "le texte passe à la ligne");
        assert!(
            area_line_height() > font_height + 2.0 * MARK_PAD_Y,
            "l'interligne laisse de la place"
        );
        let rects = marks_rects(&galley, TEXT, &marks, font_height);
        // « Atelier Bougainville SAS » est coupé en fin de ligne : deux rectangles.
        assert!(rects.len() > marks.len());
        for (r, _) in &rects {
            let row = galley
                .rows
                .iter()
                .find(|row| row.min_y() <= r.center().y && r.center().y <= row.max_y())
                .unwrap_or_else(|| panic!("surlignage hors de toute ligne"));
            // Dans la ligne, jamais sur la suivante ni la précédente.
            assert!(r.min.y >= row.min_y() && r.max.y <= row.max_y());
            // Centré sur le texte : la hauteur du texte plus la marge.
            let glyph = row.glyphs.first().unwrap_or_else(|| panic!("ligne vide"));
            let text_top = row.pos.y + glyph.pos.y - glyph.font_ascent;
            assert!((r.min.y - (text_top - MARK_PAD_Y)).abs() <= 1.0);
            assert!((r.height() - (font_height + 2.0 * MARK_PAD_Y)).abs() <= 1.0);
            assert!(r.width() > 0.0);
        }
        // Le « . » entre deux valeurs voisines n'est couvert par aucun fond.
        let dot = TEXT.find("SAS. ").map(|i| i + 3).unwrap_or_default();
        let (a, b) = (char_index(TEXT, dot), char_index(TEXT, dot + 2));
        let dot_rects = mark_rects(&galley, a, b, font_height);
        for d in dot_rects {
            assert!(rects.iter().all(|(r, _)| !r.intersects(d.shrink(0.5))));
        }
    }
}
