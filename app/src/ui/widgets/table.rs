//! Tableau à lignes virtualisées : seules les lignes visibles sont construites.
//! En-têtes en petites majuscules (triables au clic), case à cocher par ligne
//! et case « tout cocher » à trois états dans l'en-tête.
//!
//! Clavier : quand la case d'une ligne a le focus, ↑ et ↓ passent à la ligne
//! voisine (le tableau défile pour la montrer) et Espace la coche.

use super::{CheckState, Checkbox, Icon, TRACKING_WIDE, spaced_job};
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{
    FocusDirection, Id, Key, Rect, Role, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder,
    Vec2, WidgetInfo, pos2, vec2,
};

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
    /// Contenu aligné à droite (nombres).
    pub right: bool,
    /// Le clic sur l'en-tête trie le tableau.
    pub sortable: bool,
}

impl Column {
    pub const fn fixed(title: &'static str, px: f32) -> Self {
        Self {
            title,
            width: ColumnWidth::Fixed(px),
            right: false,
            sortable: false,
        }
    }

    pub const fn fill(title: &'static str, weight: f32) -> Self {
        Self {
            title,
            width: ColumnWidth::Fill(weight),
            right: false,
            sortable: false,
        }
    }

    pub const fn right(mut self) -> Self {
        self.right = true;
        self
    }

    pub const fn sortable(mut self) -> Self {
        self.sortable = true;
        self
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
    /// Colonne dont l'en-tête a été cliqué (tri).
    pub sort_clicked: Option<usize>,
}

/// Largeur de la colonne de cases à cocher.
const CHECK_COL: f32 = 44.0;
const HEADER_H: f32 = 40.0;
/// Taille de l'icône de tri.
const SORT_ICON: f32 = 14.0;

#[derive(Debug)]
pub struct DataTable<'a> {
    id_salt: &'a str,
    columns: &'a [Column],
    row_height: f32,
    max_height: f32,
    header_check: Option<CheckState>,
    /// Colonne triée et sens (vrai : croissant).
    sort: Option<(usize, bool)>,
    /// Ligne à montrer et à qui donner le focus (une fois).
    focus_row: Option<usize>,
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

/// Décalage de défilement qui montre la ligne `row` entière.
pub fn offset_to_show(row: usize, row_h: f32, offset: f32, view_h: f32) -> f32 {
    let top = row as f32 * row_h;
    let bottom = top + row_h;
    if top < offset {
        top
    } else if bottom > offset + view_h {
        bottom - view_h
    } else {
        offset
    }
}

/// Défilement et hauteur visible de l'image précédente.
#[derive(Debug, Clone, Copy, Default)]
struct ScrollMemo {
    offset: f32,
    view_h: f32,
}

impl<'a> DataTable<'a> {
    pub fn new(id_salt: &'a str, columns: &'a [Column]) -> Self {
        Self {
            id_salt,
            columns,
            row_height: 44.0,
            max_height: 360.0,
            header_check: None,
            sort: None,
            focus_row: None,
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

    /// Colonne triée (indice dans les colonnes) et sens.
    pub fn sorted(mut self, sort: Option<(usize, bool)>) -> Self {
        self.sort = sort;
        self
    }

    /// Fait défiler jusqu'à la ligne `row` et lui donne le focus.
    pub fn focus_row(mut self, row: Option<usize>) -> Self {
        self.focus_row = row;
        self
    }

    fn id(&self) -> Id {
        Id::unique(("tableau", self.id_salt))
    }

    /// Identifiant de la case à cocher d'une ligne.
    pub fn row_check_id(id_salt: &str, row: usize) -> Id {
        Id::unique(("tableau", id_salt)).with(("case", row))
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
        let table_id = self.id();
        let width = ui.available_width();
        let check_w = if self.header_check.is_some() {
            CHECK_COL
        } else {
            0.0
        };
        let widths = column_widths(self.columns, width - check_w);
        let cell_pad = 12.0;
        let row_h = self.row_height;

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
            p.sunken,
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
                "Décocher les lignes affichées"
            } else {
                "Cocher les lignes affichées"
            };
            if child
                .add(Checkbox::bare(state, label))
                .on_hover_text(label)
                .clicked()
            {
                out.header_toggled = true;
            }
        }
        let mut x = header.min.x + check_w;
        for (i, (col, w)) in self.columns.iter().zip(&widths).enumerate() {
            let cell_rect = Rect::from_min_size(pos2(x, header.min.y), vec2(*w, HEADER_H));
            let active = self.sort.filter(|(c, _)| *c == i).map(|(_, asc)| asc);
            let mut ink = p.text_secondary;
            if col.sortable {
                let r = ui.interact(cell_rect, table_id.with(("tri", i)), Sense::click());
                let label = match active {
                    Some(true) => format!("Trier par {} : croissant", col.title),
                    Some(false) => format!("Trier par {} : décroissant", col.title),
                    None => format!("Trier par {}", col.title),
                };
                r.widget_info(|| WidgetInfo::labeled(Role::ColumnHeader, true, &label));
                if r.is_pointer_button_down_on() {
                    ui.painter().rect_filled(
                        cell_rect.shrink(4.0),
                        radius(p.radius_sm),
                        p.hover_strong,
                    );
                } else if r.hovered() {
                    ui.painter()
                        .rect_filled(cell_rect.shrink(4.0), radius(p.radius_sm), p.hover);
                }
                if r.hovered() || active.is_some() {
                    ink = p.accent;
                }
                if r.has_focus() {
                    ui.painter().rect_stroke(
                        cell_rect.shrink(3.0),
                        radius(p.radius_sm),
                        Stroke::new(2.0, p.border_focus),
                        StrokeKind::Outside,
                    );
                }
                if r.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Trier")
                    .clicked()
                {
                    out.sort_clicked = Some(i);
                }
            }
            let galley = ui.painter().layout_job(spaced_job(
                &col.title.to_uppercase(),
                font(Weight::Semibold, p.text_xs),
                ink,
                TRACKING_WIDE * 2.0,
            ));
            let icon_w = if col.sortable { SORT_ICON + 4.0 } else { 0.0 };
            let content_w = galley.size().x + icon_w;
            let left = if col.right {
                cell_rect.max.x - cell_pad - content_w
            } else {
                x + cell_pad
            };
            let pos = pos2(left, header.center().y - galley.size().y / 2.0);
            let painter = ui.painter().with_clip_rect(cell_rect);
            let gw = galley.size().x;
            painter.galley(pos, galley, ink);
            if col.sortable {
                let icon = match active {
                    Some(true) => Icon::ChevronUp,
                    Some(false) => Icon::ChevronDown,
                    None => Icon::ChevronsUpDown,
                };
                icon.paint(
                    ui,
                    Rect::from_center_size(
                        pos2(left + gw + 4.0 + SORT_ICON / 2.0, header.center().y),
                        Vec2::splat(SORT_ICON),
                    ),
                    ink,
                );
            }
            x += w;
        }

        // Clavier : ↑ / ↓ depuis la case d'une ligne.
        let check_id = |row: usize| Self::row_check_id(self.id_salt, row);
        let memo_id = table_id.with("defilement");
        let memo: ScrollMemo = ui.data(|d| d.get_temp(memo_id).unwrap_or_default());
        let mut target = self.focus_row.filter(|r| *r < rows);
        if self.header_check.is_some() && target.is_none() {
            let focused = ui.memory(|m| m.focused());
            let current = focused.and_then(|f| (0..rows).find(|r| check_id(*r) == f));
            if let Some(cur) = current {
                let (up, down) = ui.input(|i| {
                    (
                        i.key_pressed(Key::ArrowUp) && i.modifiers.is_none(),
                        i.key_pressed(Key::ArrowDown) && i.modifiers.is_none(),
                    )
                });
                if down || up {
                    let next = if down {
                        (cur + 1).min(rows.saturating_sub(1))
                    } else {
                        cur.saturating_sub(1)
                    };
                    target = Some(next);
                }
            }
        }
        if let Some(t) = target {
            ui.memory_mut(|m| {
                m.request_focus(check_id(t));
                // Le déplacement d'egui (vers le widget le plus proche) est annulé.
                m.move_focus(FocusDirection::None);
            });
        }
        let view_h = if memo.view_h > 0.0 {
            memo.view_h
        } else {
            self.max_height
        };
        let forced_offset = target.map(|t| offset_to_show(t, row_h, memo.offset, view_h));
        let focused_row = ui
            .memory(|m| m.focused())
            .and_then(|f| (0..rows).find(|r| check_id(*r) == f));

        // Corps virtualisé.
        let mut area = ScrollArea::vertical()
            .id_salt(self.id_salt)
            .max_height(self.max_height)
            .min_scrolled_height((row_h * rows as f32).min(self.max_height))
            .auto_shrink([false, true]);
        if let Some(o) = forced_offset {
            area = area.vertical_scroll_offset(o);
        }
        let scrolled = area.show_rows(ui, row_h, rows, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
                let painter = ui.painter();
                match tone(row) {
                    RowTone::Attention => {
                        painter.rect_filled(rect, 0.0, p.attention_row);
                        painter.rect_filled(
                            Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                            0.0,
                            p.warning,
                        );
                    }
                    RowTone::Normal if resp.hovered() => {
                        painter.rect_filled(rect, 0.0, p.hover);
                    }
                    RowTone::Normal => {}
                }
                painter.line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    Stroke::new(1.0, p.border),
                );
                if focused_row == Some(row) || target == Some(row) {
                    // Ligne courante au clavier : contour vert.
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        radius(p.radius_sm),
                        Stroke::new(2.0, p.border_focus),
                        StrokeKind::Inside,
                    );
                }
                if self.header_check.is_some() {
                    let r = Rect::from_min_size(rect.min, vec2(CHECK_COL, row_h));
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .max_rect(r.shrink2(vec2(13.0, 0.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let name = format!("Sélectionner {}", row_name(row));
                    if child
                        .add(Checkbox::bare(is_checked(row), &name).id(check_id(row)))
                        .clicked()
                    {
                        out.toggled_rows.push(row);
                    }
                }
                let mut x = rect.min.x + check_w;
                for (col, w) in widths.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(x, rect.min.y), vec2(*w, row_h))
                        .shrink2(vec2(cell_pad, 0.0));
                    let right = self.columns.get(col).is_some_and(|c| c.right);
                    let layout = if right {
                        egui::Layout::right_to_left(egui::Align::Center)
                    } else {
                        egui::Layout::left_to_right(egui::Align::Center)
                    };
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .max_rect(r)
                            .id_salt((row, col))
                            .layout(layout),
                    );
                    child.set_clip_rect(r.intersect(ui.clip_rect()));
                    cell(&mut child, row, col);
                    x += w;
                }
            }
        });
        let memo = ScrollMemo {
            offset: scrolled.state.clamped_offset().y,
            view_h: scrolled.inner_rect.height(),
        };
        ui.data_mut(|d| d.insert_temp(memo_id, memo));
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

    #[test]
    fn scrolling_shows_the_whole_row() {
        // Vue de 100 px, lignes de 40 px.
        assert_eq!(offset_to_show(0, 40.0, 0.0, 100.0), 0.0);
        assert_eq!(offset_to_show(2, 40.0, 0.0, 100.0), 20.0, "ligne 3 coupée");
        assert_eq!(
            offset_to_show(1, 40.0, 60.0, 100.0),
            40.0,
            "ligne au-dessus"
        );
        assert_eq!(offset_to_show(2, 40.0, 30.0, 100.0), 30.0, "déjà visible");
    }
}
