//! Tableau à lignes virtualisées : seules les lignes visibles sont construites.
//! En-têtes en petites majuscules, case à cocher par ligne et case
//! « tout cocher » à trois états dans l'en-tête.

use super::{CheckState, Checkbox, TRACKING_WIDE, spaced_job};
use crate::ui::theme::{Weight, font, mix, palette};
use egui::{Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

/// Largeur d'une colonne.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    /// Largeur fixe en px.
    Fixed(f32),
    /// Part de la largeur restante.
    Fill(f32),
}

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: ColumnWidth,
}

impl Column {
    pub const fn fixed(title: &'static str, px: f32) -> Self {
        Self {
            title,
            width: ColumnWidth::Fixed(px),
        }
    }

    pub const fn fill(title: &'static str, weight: f32) -> Self {
        Self {
            title,
            width: ColumnWidth::Fill(weight),
        }
    }
}

/// Mise en évidence d'une ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowTone {
    Normal,
    /// Ligne à confirmer : fond jaune pâle et filet jaune à gauche.
    Attention,
}

/// Événements produits par le tableau ; l'état est modifié par l'appelant.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TableOutput {
    pub header_toggled: bool,
    pub toggled_rows: Vec<usize>,
}

/// Largeur de la colonne de cases à cocher.
const CHECK_COL: f32 = 44.0;
const HEADER_H: f32 = 40.0;

#[derive(Debug)]
pub struct DataTable<'a> {
    id_salt: &'a str,
    columns: &'a [Column],
    row_height: f32,
    max_height: f32,
    header_check: Option<CheckState>,
}

/// Calcule la largeur de chaque colonne pour une largeur disponible.
pub fn column_widths(columns: &[Column], available: f32) -> Vec<f32> {
    let fixed: f32 = columns
        .iter()
        .map(|c| match c.width {
            ColumnWidth::Fixed(w) => w,
            ColumnWidth::Fill(_) => 0.0,
        })
        .sum();
    let weights: f32 = columns
        .iter()
        .map(|c| match c.width {
            ColumnWidth::Fill(w) => w,
            ColumnWidth::Fixed(_) => 0.0,
        })
        .sum();
    let rest = (available - fixed).max(0.0);
    columns
        .iter()
        .map(|c| match c.width {
            ColumnWidth::Fixed(w) => w,
            ColumnWidth::Fill(w) if weights > 0.0 => rest * w / weights,
            ColumnWidth::Fill(_) => 0.0,
        })
        .collect()
}

impl<'a> DataTable<'a> {
    pub fn new(id_salt: &'a str, columns: &'a [Column]) -> Self {
        Self {
            id_salt,
            columns,
            row_height: 44.0,
            max_height: 360.0,
            header_check: None,
        }
    }

    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = h;
        self
    }

    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = h;
        self
    }

    /// Ajoute une colonne de cases à cocher ; `state` est l'état de l'en-tête.
    pub fn checkable(mut self, state: CheckState) -> Self {
        self.header_check = Some(state);
        self
    }

    pub fn show(
        self,
        ui: &mut Ui,
        rows: usize,
        is_checked: impl Fn(usize) -> bool,
        tone: impl Fn(usize) -> RowTone,
        row_name: impl Fn(usize) -> String,
        mut cell: impl FnMut(&mut Ui, usize, usize),
    ) -> TableOutput {
        let p = palette();
        let mut out = TableOutput::default();
        let width = ui.available_width();
        let check_w = if self.header_check.is_some() {
            CHECK_COL
        } else {
            0.0
        };
        let widths = column_widths(self.columns, width - check_w);
        let cell_pad = 12.0;

        // En-tête.
        let (header, _) = ui.allocate_exact_size(vec2(width, HEADER_H), Sense::hover());
        ui.painter().rect_filled(
            header,
            egui::CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            },
            p.gray_50,
        );
        ui.painter().line_segment(
            [header.left_bottom(), header.right_bottom()],
            Stroke::new(1.0, p.border),
        );
        if let Some(state) = self.header_check {
            let r = Rect::from_min_size(header.min, vec2(CHECK_COL, HEADER_H));
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(r.shrink2(vec2(13.0, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let label = if state == CheckState::Checked {
                "Tout décocher"
            } else {
                "Tout cocher"
            };
            if child.add(Checkbox::bare(state, label)).clicked() {
                out.header_toggled = true;
            }
        }
        let mut x = header.min.x + check_w;
        for (col, w) in self.columns.iter().zip(&widths) {
            let galley = ui.painter().layout_job(spaced_job(
                &col.title.to_uppercase(),
                font(Weight::Semibold, p.text_xs),
                p.text_secondary,
                TRACKING_WIDE * 2.0,
            ));
            let pos = pos2(x + cell_pad, header.center().y - galley.size().y / 2.0);
            ui.painter()
                .with_clip_rect(Rect::from_min_size(
                    pos2(x, header.min.y),
                    vec2(*w, HEADER_H),
                ))
                .galley(pos, galley, p.text_secondary);
            x += w;
        }

        // Corps virtualisé.
        let row_h = self.row_height;
        let attention_fill = mix(p.white, p.data.fill, 0.85);
        ScrollArea::vertical()
            .id_salt(self.id_salt)
            .max_height(self.max_height)
            .min_scrolled_height((row_h * rows as f32).min(self.max_height))
            .auto_shrink([false, true])
            .show_rows(ui, row_h, rows, |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
                    let painter = ui.painter();
                    match tone(row) {
                        RowTone::Attention => {
                            painter.rect_filled(rect, 0.0, attention_fill);
                            painter.rect_filled(
                                Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                                0.0,
                                p.warning,
                            );
                        }
                        RowTone::Normal if resp.hovered() => {
                            painter.rect_filled(rect, 0.0, p.gray_50);
                        }
                        RowTone::Normal => {}
                    }
                    painter.line_segment(
                        [rect.left_bottom(), rect.right_bottom()],
                        Stroke::new(1.0, p.border),
                    );
                    if self.header_check.is_some() {
                        let r = Rect::from_min_size(rect.min, vec2(CHECK_COL, row_h));
                        let mut child = ui.new_child(
                            UiBuilder::new()
                                .max_rect(r.shrink2(vec2(13.0, 0.0)))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        let name = format!("Sélectionner {}", row_name(row));
                        if child.add(Checkbox::bare(is_checked(row), &name)).clicked() {
                            out.toggled_rows.push(row);
                        }
                    }
                    let mut x = rect.min.x + check_w;
                    for (col, w) in widths.iter().enumerate() {
                        let r = Rect::from_min_size(pos2(x, rect.min.y), vec2(*w, row_h))
                            .shrink2(vec2(cell_pad, 0.0));
                        let mut child = ui.new_child(
                            UiBuilder::new()
                                .max_rect(r)
                                .id_salt((row, col))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        child.set_clip_rect(r.intersect(ui.clip_rect()));
                        cell(&mut child, row, col);
                        x += w;
                    }
                }
            });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_share_the_remaining_space() {
        let cols = [
            Column::fixed("A", 100.0),
            Column::fill("B", 1.0),
            Column::fill("C", 3.0),
        ];
        let w = column_widths(&cols, 500.0);
        assert_eq!(w, vec![100.0, 100.0, 300.0]);
        // Trop étroit : les colonnes souples tombent à zéro, jamais en négatif.
        let w = column_widths(&cols, 50.0);
        assert_eq!(w, vec![100.0, 0.0, 0.0]);
    }
}
