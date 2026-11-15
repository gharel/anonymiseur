//! Étape Vérifier, dans le document (maquette B) : à gauche les documents du
//! dossier (décisions restantes) et les catégories ; au centre le texte
//! surligné ; à droite la file « À décider » et les valeurs masquées d'office.
//! En dessous de 1280 px, la file devient un panneau repliable au-dessus du
//! texte ; en dessous de 900 px, tout tient dans une colonne.
//!
//! Clavier (hors champ de saisie) : J et K parcourent la file, M masque, G
//! garde, / ouvre la recherche dans le document. Dans le texte : flèches,
//! Espace, Maj+flèches (sélection), Échap.

use super::document::{self, DocInput, Look, ValueInfo};
use super::images::document_images;
use super::verify::{kind_menu, rejected_banner};
use super::{format_style, truncated_text};
use crate::ui::app::SEARCH_FIELD;
use crate::ui::state::anonymize::AnonymizeState;
use crate::ui::state::files::FileFormat;
use crate::ui::state::review::VerifyMode;
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{CONTROL_SM, Weight, font, palette, radius};
use crate::ui::widgets::badge::{FAMILIES, family_label, family_tint};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::field::{Mark, field_id, marked_label};
use crate::ui::widgets::nav::{pill_menu, segmented};
use crate::ui::widgets::{Badge, Icon, PillButton, TextField, caption, muted, overline};
use anon_core::Session;
use anon_engine::SELECT_MIN_CONFIDENCE;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Key, Layout, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2,
    vec2,
};
use std::collections::HashMap;
use std::path::Path;

/// Largeur de fenêtre à partir de laquelle la file est une troisième colonne.
pub const THREE_COLUMNS_MIN_W: f32 = 1280.0;
/// Largeur de fenêtre sous laquelle tout tient dans une colonne.
pub const TWO_COLUMNS_MIN_W: f32 = 900.0;
/// Colonne des documents et des catégories.
const LEFT_W: f32 = 264.0;
const LEFT_W_NARROW: f32 = 224.0;
/// Colonne de la file « À décider ».
const RIGHT_W: f32 = 344.0;
const DOC_ROW_H: f32 = 40.0;
const CAT_ROW_H: f32 = 30.0;
/// Valeurs suivantes de la file montrées sous la valeur courante.
const NEXT_SHOWN: usize = 3;
/// Ligne de légende (ou de sélection) sous le texte.
const LEGEND_H: f32 = 48.0;
/// Hauteur maximale de la liste « Masquées » dans le panneau replié.
const MASKED_LIST_MAX_H: f32 = 220.0;
/// Largeur du champ de recherche dans le document.
const SEARCH_W: f32 = 260.0;
/// Interligne de l'extrait autour de la valeur à décider.
const EXCERPT_LINE_HEIGHT: f32 = 22.0;

/// Vrai si l'étape Vérifier montre la vue dans le document (pleine fenêtre).
pub fn active(st: &AnonymizeState) -> bool {
    st.analysis.is_some() && st.busy.is_none() && st.review.mode == VerifyMode::Document
}

/// Raccourcis de la file : J, K, M, G, /. Ignorés pendant une saisie, un
/// menu ou une fenêtre ouverte.
fn handle_keys(ui: &Ui, st: &mut AnonymizeState, session: &Session) {
    let ctx = ui.ctx();
    if ctx.text_edit_focused()
        || egui::Popup::is_any_open(ctx)
        || ctx.memory(|m| m.top_modal_layer().is_some())
    {
        return;
    }
    let doc = st.review_document_name().unwrap_or_default();
    let in_text = document::has_focus(ctx, &doc);
    let (j, k, m, g, slash) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::J),
            i.consume_key(Modifiers::NONE, Key::K),
            i.consume_key(Modifiers::NONE, Key::M),
            i.consume_key(Modifiers::NONE, Key::G),
            i.consume_key(Modifiers::NONE, Key::Slash),
        )
    });
    let mut changed = false;
    if j {
        changed |= st.queue_step(true).is_some();
    }
    if k {
        changed |= st.queue_step(false).is_some();
    }
    if m {
        changed |= st.decide_current(true, in_text, session).is_some();
    }
    if g {
        changed |= st.decide_current(false, in_text, session).is_some();
    }
    if slash {
        ctx.memory_mut(|mem| mem.request_focus(field_id(SEARCH_FIELD)));
        changed = true;
    }
    if changed {
        ctx.request_repaint();
    }
}

/// Étape Vérifier dans le document, sur tout l'espace disponible.
pub fn show(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    handle_keys(ui, st, session);
    st.sync_review(session);
    let full = ui.available_rect_before_wrap();
    let w = full.width();
    let column = |ui: &mut Ui, rect: Rect, add: &mut dyn FnMut(&mut Ui)| {
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                add(ui);
            },
        );
    };
    if w >= THREE_COLUMNS_MIN_W {
        let left = Rect::from_min_size(full.min, vec2(LEFT_W, full.height()));
        let right = Rect::from_min_max(pos2(full.max.x - RIGHT_W, full.min.y), full.max);
        let center =
            Rect::from_min_max(pos2(left.max.x, full.min.y), pos2(right.min.x, full.max.y));
        side_panel(ui, left, false);
        side_panel(ui, right, true);
        column(ui, left, &mut |ui| left_column(ui, st));
        column(ui, center, &mut |ui| {
            center_column(ui, st, session, Layout3::Wide)
        });
        column(ui, right, &mut |ui| queue_column(ui, st, session));
    } else if w >= TWO_COLUMNS_MIN_W {
        let left = Rect::from_min_size(full.min, vec2(LEFT_W_NARROW, full.height()));
        let center = Rect::from_min_max(pos2(left.max.x, full.min.y), full.max);
        side_panel(ui, left, false);
        column(ui, left, &mut |ui| left_column(ui, st));
        column(ui, center, &mut |ui| {
            center_column(ui, st, session, Layout3::Panel)
        });
    } else {
        column(ui, full, &mut |ui| {
            center_column(ui, st, session, Layout3::Single)
        });
    }
    ui.advance_cursor_after_rect(full);
}

/// Disposition de la colonne centrale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout3 {
    /// Trois colonnes : la file est à droite.
    Wide,
    /// Deux colonnes : la file est un panneau repliable au-dessus du texte.
    Panel,
    /// Une colonne : choix du document en haut, puis le panneau.
    Single,
}

/// Fond et bord d'une colonne latérale.
fn side_panel(ui: &Ui, rect: Rect, right: bool) {
    let p = palette();
    ui.painter().rect_filled(rect, 0.0, p.surface);
    let x = if right { rect.min.x } else { rect.max.x };
    ui.painter().line_segment(
        [pos2(x, rect.min.y), pos2(x, rect.max.y)],
        Stroke::new(1.0, p.border),
    );
}

/// Contenu avec une marge intérieure.
fn padded(ui: &mut Ui, x: f32, y: f32, add: impl FnOnce(&mut Ui)) {
    let rect = ui.available_rect_before_wrap().shrink2(vec2(x, y));
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
        add,
    );
}

// --- Colonne de gauche ------------------------------------------------------

fn left_column(ui: &mut Ui, st: &mut AnonymizeState) {
    let p = palette();
    padded(ui, p.space(3), p.space(4), |ui| {
        let bottom_h = CONTROL_SM + p.space(3);
        let list_h = (ui.available_height() - bottom_h).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("colonne-documents")
            .max_height(list_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(2.0);
                ui_overline(ui, "Documents");
                let mut chosen = None;
                for (i, name) in st.review_documents() {
                    let n = st.remaining_in(&name);
                    if doc_row(ui, &name, n, i == st.review.doc).clicked() {
                        chosen = Some(i);
                    }
                }
                if let Some(i) = chosen {
                    st.select_document(i);
                }
                ui.add_space(p.space(4));
                ui_overline(ui, "Catégories");
                for fam in FAMILIES {
                    let n = st.count_by_family(fam);
                    if n > 0 {
                        category_row(ui, fam, n);
                    }
                }
            });
        ui.add_space(p.space(3));
        table_button(ui, st);
    });
}

fn ui_overline(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        overline(ui, text);
    });
    ui.add_space(4.0);
}

/// Bouton « Vue tableau ».
fn table_button(ui: &mut Ui, st: &mut AnonymizeState) {
    if ui
        .add(
            PillButton::secondary("Vue tableau")
                .icon(Icon::FileSpreadsheet)
                .small(),
        )
        .on_hover_text("Toutes les valeurs dans un tableau : tri, filtres, traitements en masse")
        .clicked()
    {
        st.review.mode = VerifyMode::Table;
    }
}

/// Ligne d'un document : icône du format, nom, décisions restantes.
fn doc_row(ui: &mut Ui, name: &str, remaining: usize, selected: bool) -> egui::Response {
    let p = palette();
    let w = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(w, DOC_ROW_H), Sense::click());
    let label = format!(
        "{name}, {}",
        match remaining {
            0 => "aucune décision restante".to_owned(),
            n => count_label(n, "décision restante", "décisions restantes"),
        }
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            p.brand.fill
        } else if response.hovered() {
            p.hover
        } else {
            p.surface
        };
        painter.rect_filled(rect, radius(p.radius_md), fill);
        let (icon, tint) = format_style(FileFormat::from_path(Path::new(name)));
        let ink = if selected { p.brand.ink } else { tint.ink };
        icon.paint(
            ui,
            Rect::from_center_size(pos2(rect.min.x + 20.0, rect.center().y), vec2(18.0, 18.0)),
            ink,
        );
        // Pastille : décisions restantes.
        let count = remaining.to_string();
        let (pill_fill, pill_ink, weight) = if remaining > 0 {
            (p.pending.fill, p.data.ink, Weight::Bold)
        } else {
            (egui::Color32::TRANSPARENT, p.text_secondary, Weight::Medium)
        };
        let g = painter.layout_no_wrap(count, font(weight, p.text_xs), pill_ink);
        let pill = Rect::from_min_max(
            pos2(
                rect.max.x - 10.0 - g.size().x - 16.0,
                rect.center().y - 10.0,
            ),
            pos2(rect.max.x - 10.0, rect.center().y + 10.0),
        );
        painter.rect_filled(pill, radius(p.radius_full), pill_fill);
        painter.galley(pill.center() - g.size() / 2.0, g, pill_ink);
        // Nom, tronqué.
        let name_max = (pill.min.x - 8.0 - (rect.min.x + 38.0)).max(20.0);
        let weight = if selected {
            Weight::Semibold
        } else {
            Weight::Regular
        };
        let mut job = LayoutJob::simple_singleline(
            name.to_owned(),
            font(weight, p.text_sm),
            if selected { p.brand.ink } else { p.text },
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_max);
        let ng = painter.layout_job(job);
        painter.galley(
            pos2(rect.min.x + 38.0, rect.center().y - ng.size().y / 2.0),
            ng,
            p.text,
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect.shrink(1.0),
                radius(p.radius_md),
                Stroke::new(2.0, p.border_focus),
                StrokeKind::Inside,
            );
        }
    }
    response
        .on_hover_text(name)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Ligne d'une catégorie : pastille de couleur, nom, nombre de valeurs.
fn category_row(ui: &mut Ui, fam: anon_core::Family, n: usize) {
    let p = palette();
    let w = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(w, CAT_ROW_H), Sense::hover());
    let label = format!("{} : {n}", family_label(fam));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    let painter = ui.painter();
    painter.circle_filled(
        pos2(rect.min.x + 15.0, rect.center().y),
        5.0,
        family_tint(fam).edge,
    );
    painter.text(
        pos2(rect.min.x + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        family_label(fam),
        font(Weight::Regular, p.text_sm),
        p.text,
    );
    painter.text(
        pos2(rect.max.x - 10.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        n.to_string(),
        font(Weight::Medium, p.text_sm),
        p.text_secondary,
    );
}

// --- Colonne centrale ---------------------------------------------------------

/// Ce que le texte doit savoir de chaque valeur du document affiché.
fn value_infos(st: &AnonymizeState) -> HashMap<usize, ValueInfo> {
    let order: Vec<usize> = st.queue().into_iter().chain(st.sure_values()).collect();
    order
        .into_iter()
        .filter_map(|id| {
            let c = st.candidate(id)?;
            let replacement = st
                .generalized(id)
                .map(str::to_owned)
                .or_else(|| st.tag_preview.get(&id).map(|t| t.to_string()));
            Some((
                id,
                ValueInfo {
                    value: c.value.clone(),
                    kind: c.kind,
                    occurrences: c.occurrences,
                    pending: st.is_undecided(id),
                    replacement,
                },
            ))
        })
        .collect()
}

fn center_column(ui: &mut Ui, st: &mut AnonymizeState, session: &Session, layout: Layout3) {
    let p = palette();
    let (px, py) = match layout {
        Layout3::Wide => (p.space(6), p.space(4)),
        _ => (p.space(4), p.space(3)),
    };
    padded(ui, px, py, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        if layout == Layout3::Single {
            document_picker(ui, st);
        }
        if rejected_banner(ui, &st.rejected) {
            st.dismiss_rejected();
        }
        if layout != Layout3::Wide {
            queue_panel(ui, st, session);
        }
        let paper_h = (ui.available_height() - LEGEND_H).max(160.0);
        let paper = Rect::from_min_size(
            ui.available_rect_before_wrap().min,
            vec2(ui.available_width(), paper_h),
        );
        paper_frame(ui, paper);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(paper.shrink(1.0))
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(paper.shrink(1.0).intersect(ui.clip_rect()));
                paper_header(ui, st);
                paper_body(ui, st, session);
            },
        );
        ui.advance_cursor_after_rect(paper);
        legend_or_selection(ui, st, session);
    });
}

/// Choix du document (une colonne) et bouton « Vue tableau ».
fn document_picker(ui: &mut Ui, st: &mut AnonymizeState) {
    let p = palette();
    let docs = st.review_documents();
    let labels: Vec<(usize, String)> = docs
        .iter()
        .map(|(i, name)| (*i, format!("{name} · {}", st.remaining_in(name))))
        .collect();
    let options: Vec<(usize, &str)> = labels.iter().map(|(i, l)| (*i, l.as_str())).collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        if let Some(i) = pill_menu(ui, "Document", st.review.doc, &options) {
            st.select_document(i);
        }
        table_button(ui, st);
    });
}

/// Feuille blanche du texte : fond, bord, ombre.
fn paper_frame(ui: &Ui, rect: Rect) {
    let p = palette();
    let r = radius(p.radius_lg);
    ui.painter().add(p.shadow_sm.as_shape(rect, r));
    ui.painter().rect(
        rect,
        r,
        document::paper_fill(),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
}

/// En-tête de la feuille : nom du document, recherche dans le document.
fn paper_header(ui: &mut Ui, st: &mut AnonymizeState) {
    let p = palette();
    let name = st.review_document_name().unwrap_or_default();
    let top = ui.available_rect_before_wrap();
    let row = Rect::from_min_size(top.min, vec2(top.width(), 56.0));
    let inner = row.shrink2(vec2(document::PAPER_PAD_X - 8.0, 8.0));
    let search_w = SEARCH_W.min((inner.width() * 0.45).max(140.0));
    // Recherche à droite, nom du document à gauche.
    let search = Rect::from_min_max(pos2(inner.max.x - search_w, inner.min.y), inner.max);
    let title = Rect::from_min_max(inner.min, pos2(search.min.x - p.space(4), inner.max.y));
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(title)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let (icon, _) = format_style(FileFormat::from_path(Path::new(&name)));
            icon.show(ui, p.text_secondary, 18.0);
            let w = ui.available_width().max(40.0);
            truncated_text(
                ui,
                w,
                CONTROL_SM,
                RichText::new(&name)
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.heading),
            );
        },
    );
    // Rien à chercher dans une image seule.
    if has_text(st) {
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(search)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| search_field(ui, st, search_w),
        );
    }
    ui.advance_cursor_after_rect(row);
    ui.painter().line_segment(
        [
            pos2(row.min.x + 16.0, row.max.y),
            pos2(row.max.x - 16.0, row.max.y),
        ],
        Stroke::new(1.0, p.border),
    );
}

/// Recherche dans le document : la première occurrence est sélectionnée (à
/// masquer partout), Entrée passe à la suivante.
fn search_field(ui: &mut Ui, st: &mut AnonymizeState, w: f32) {
    let r = TextField::new("Rechercher dans le document", &mut st.review.search)
        .placeholder("Rechercher dans le document")
        .id_salt(SEARCH_FIELD)
        .hide_label()
        .width(w)
        .show_control(ui)
        .on_hover_text("Rechercher dans le document (/ ou Ctrl+F) ; Entrée : suivant");
    let needle = st.review.search.clone();
    if r.changed() {
        if st.find_next(&needle, None).is_none() {
            st.select_text(None);
        }
    } else if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        let after = st.review.selection;
        if st.find_next(&needle, after).is_some() {
            r.request_focus();
        }
    }
}

/// Corps de la feuille : texte surligné, ou images du document.
fn paper_body(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    let p = palette();
    let name = st.review_document_name().unwrap_or_default();
    let has_images = st.images().iter().any(|i| i.file == name);
    let has_text = has_text(st);
    let images_first = st
        .review
        .show_images
        .unwrap_or_else(|| st.document_view().is_none_or(|v| v.span_count() == 0));
    if has_images && has_text {
        ui.add_space(p.space(2));
        ui.horizontal(|ui| {
            ui.add_space(document::PAPER_PAD_X - 4.0);
            let mut images = images_first;
            segmented(
                ui,
                "Contenu",
                &mut images,
                &[(false, "Texte"), (true, "Images")],
            );
            if images != images_first {
                st.review.show_images = Some(images);
            }
        });
    }
    if has_images && (!has_text || images_first) {
        let rect = ui
            .available_rect_before_wrap()
            .shrink2(vec2(p.space(5), p.space(3)));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("images-document", &name))
                    .auto_shrink([false, false])
                    .show(ui, |ui| document_images(ui, st, &name));
            },
        );
        return;
    }
    if !has_text {
        ui.add_space(p.space(6));
        ui.vertical_centered(|ui| muted(ui, "Aucun texte lu dans ce document."));
        return;
    }
    let values = value_infos(st);
    let events = {
        let Some(view) = st.document_view() else {
            return;
        };
        let input = DocInput {
            view,
            values: &values,
            cursor: st.review.cursor,
            selection: st.review.selection,
            reveal: st.review.reveal,
        };
        document::show(ui, &input)
    };
    let mut changed = false;
    if let Some(o) = events.cursor {
        st.set_cursor(o);
        changed = true;
    }
    if let Some(id) = events.toggled {
        st.toggle_everywhere(id, session);
        changed = true;
    }
    if let Some(forward) = events.step {
        st.move_cursor(forward);
        changed = true;
    }
    if events.toggle_cursor
        && let Some(id) = st.cursor_candidate()
    {
        st.toggle_everywhere(id, session);
        changed = true;
    }
    if let Some(forward) = events.extend {
        st.extend_selection(forward);
        changed = true;
    }
    if let Some(sel) = events.selection {
        st.select_text(sel);
        changed = true;
    }
    if events.revealed && events.cursor.is_none() {
        st.review.reveal = None;
    }
    st.review.visible_hint = events.first_visible;
    if st.review.refreshing() {
        // Recalcul exact des blocs touchés par une décision, réparti sur les
        // images suivantes.
        ui.ctx().request_repaint();
    }
    if changed {
        ui.ctx().request_repaint();
    }
}

/// Vrai si le document affiché a du texte (pas une image seule).
fn has_text(st: &AnonymizeState) -> bool {
    st.document_view().is_some_and(|v| !v.blocks.is_empty())
}

/// Sous la feuille : légende, ou actions sur le texte sélectionné.
fn legend_or_selection(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    let p = palette();
    let rect = Rect::from_min_size(
        ui.available_rect_before_wrap().min,
        vec2(ui.available_width(), LEGEND_H),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = p.space(4);
            match st.selected_text() {
                Some(text) => selection_bar(ui, st, session, &text),
                None if has_text(st) => {
                    document::legend_swatch(ui, Look::Masked, "Masqué");
                    document::legend_swatch(ui, Look::Pending, "À décider");
                    caption(ui, "Sélectionnez du texte pour le masquer partout.");
                }
                None => {
                    caption(
                        ui,
                        "Tracez un rectangle sur l'image pour masquer une zone que la lecture a manquée.",
                    );
                }
            }
        },
    );
    ui.advance_cursor_after_rect(rect);
}

/// Texte sélectionné : son type, « Masquer partout », « Toujours masquer ».
fn selection_bar(ui: &mut Ui, st: &mut AnonymizeState, session: &Session, text: &str) {
    let p = palette();
    ui.spacing_mut().item_spacing.x = p.space(2);
    let shown = format!("« {text} »");
    let w = (ui.available_width() - 520.0).clamp(80.0, 260.0);
    truncated_text(
        ui,
        w,
        CONTROL_SM,
        RichText::new(shown)
            .font(font(Weight::Semibold, p.text_sm))
            .color(p.heading),
    );
    let kind = st.review.selection_kind;
    let type_label = format!("Type : {}", kind.label());
    let r = ui
        .add(
            PillButton::ghost(&type_label)
                .icon(Icon::ChevronDown)
                .small(),
        )
        .on_hover_text("Type de la valeur masquée");
    if let Some(k) = kind_menu(&r, kind) {
        st.review.selection_kind = k;
    }
    if ui
        .add(
            PillButton::primary("Masquer partout")
                .icon(Icon::EyeOff)
                .small(),
        )
        .on_hover_text("Masque ce texte dans tous les documents du dossier")
        .clicked()
    {
        st.mask_selection(false, session);
        ui.ctx().request_repaint();
    }
    if ui
        .add(
            PillButton::secondary("Toujours masquer")
                .icon(Icon::Plus)
                .small(),
        )
        .on_hover_text("Le masque partout et le cherche aussi dans les prochains documents")
        .clicked()
    {
        st.mask_selection(true, session);
        ui.ctx().request_repaint();
    }
    if icon_button(ui, Icon::X, "Annuler la sélection", p.text_secondary).clicked() {
        st.select_text(None);
    }
}

// --- File « À décider » ---------------------------------------------------------

/// Colonne de droite (trois colonnes).
fn queue_column(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    let p = palette();
    padded(ui, p.space(4), p.space(4), |ui| {
        // « Masquées » reste en bas de la colonne ; dépliée, sa liste prend
        // la moitié de la hauteur et défile.
        let total_h = ui.available_height();
        let bottom_h = if st.review.masked_open {
            total_h * 0.5
        } else {
            CONTROL_SM + p.space(4)
        };
        egui::ScrollArea::vertical()
            .id_salt("file-a-decider")
            .max_height((total_h - bottom_h).max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = p.space(3);
                queue_header(ui, st);
                match st.review.current {
                    Some(id) => {
                        value_card(ui, st, session, id, true);
                        next_values(ui, st, id);
                    }
                    None => done_card(ui, st),
                }
            });
        let list_h = (bottom_h - CONTROL_SM - p.space(4)).max(0.0);
        masked_section(ui, st, session, Some(list_h));
    });
}

/// « À DÉCIDER » et l'avancement de la file du document.
fn queue_header(ui: &mut Ui, st: &AnonymizeState) {
    let p = palette();
    let (done, total) = st.queue_progress();
    ui.horizontal(|ui| {
        overline(ui, "À décider");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let label = format!("{done} / {total}");
            ui.label(
                RichText::new(&label)
                    .font(font(Weight::Bold, p.text_sm))
                    .color(p.heading),
            )
            .on_hover_text(format!(
                "{} sur {total} dans ce document",
                count_label(done, "valeur décidée", "valeurs décidées")
            ));
            progress_pips(ui, done, total);
        });
    });
}

/// Pastilles d'avancement (au plus douze ; au-delà, une barre).
fn progress_pips(ui: &mut Ui, done: usize, total: usize) {
    let p = palette();
    if total == 0 {
        return;
    }
    let shown = total.min(12);
    let pip_w = if total <= 12 {
        18.0
    } else {
        120.0 / shown as f32
    };
    let w = shown as f32 * (pip_w + 3.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 6.0), Sense::hover());
    let filled = if total <= 12 {
        done
    } else {
        (done * shown).div_ceil(total)
    };
    for i in 0..shown {
        let x = rect.max.x - (shown - i) as f32 * (pip_w + 3.0) + 3.0;
        let r = Rect::from_min_size(pos2(x, rect.min.y), vec2(pip_w, 6.0));
        let c = if i < filled { p.primary_dark } else { p.track };
        ui.painter().rect_filled(r, radius(3.0), c);
    }
}

/// Pourquoi la valeur demande une décision.
fn reason(st: &AnonymizeState, id: usize) -> &'static str {
    let Some(c) = st.candidate(id) else {
        return "";
    };
    let uncertain = c.confidence < SELECT_MIN_CONFIDENCE;
    match (c.ambiguous, uncertain) {
        (true, true) => {
            "Mot courant ou valeur courte, détecté avec peu de certitude : vérifiez qu'il désigne bien une personne, un lieu ou une donnée à masquer."
        }
        (true, false) => {
            "Mot courant ou valeur courte : il peut désigner autre chose qu'une donnée à masquer."
        }
        _ => "Détection incertaine : vérifiez-la dans le texte.",
    }
}

/// Valeur courante : type, valeur, occurrences, extrait, raison, actions.
fn value_card(ui: &mut Ui, st: &mut AnonymizeState, session: &Session, id: usize, full: bool) {
    let p = palette();
    let Some(c) = st.candidate(id).cloned() else {
        return;
    };
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.5, p.pending.edge))
        .corner_radius(radius(p.radius_xl))
        .shadow(p.shadow_sm)
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = p.space(2);
            ui.add(Badge::warning(c.kind.label()).small());
            ui.add(
                egui::Label::new(
                    RichText::new(format!("« {} »", c.value))
                        .font(font(Weight::Extrabold, p.text_xl))
                        .color(p.heading),
                )
                .wrap(),
            );
            let state = match (st.decided.contains(&id), c.selected) {
                (true, true) => "décidé : masquée",
                (true, false) => "décidé : gardée visible",
                (false, true) => "masquée pour l'instant",
                (false, false) => "visible pour l'instant",
            };
            caption(
                ui,
                &format!(
                    "{} dans le dossier · {state}",
                    count_label(c.occurrences, "fois", "fois")
                ),
            );
            if full {
                excerpt_box(ui, st, id);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    Icon::Info.show(ui, p.accent, 16.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(reason(st, id))
                                .font(font(Weight::Regular, p.text_xs))
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(p.space(1));
                ui.spacing_mut().item_spacing.y = p.space(2);
                decision_buttons(ui, st, session);
            }
        });
}

/// Extrait du document autour de la valeur.
fn excerpt_box(ui: &mut Ui, st: &AnonymizeState, id: usize) {
    let p = palette();
    let Some((before, value, after)) = st.excerpt(id) else {
        return;
    };
    let base = TextFormat {
        font_id: font(Weight::Regular, p.text_sm),
        color: p.text_secondary,
        line_height: Some(EXCERPT_LINE_HEIGHT),
        ..Default::default()
    };
    // Valeur en cours de décision : fond et soulignement peints dans la
    // hauteur du texte, pas sur la ligne suivante (interligne).
    let text = format!("{before}{value}{after}");
    let marks = [(before.len()..before.len() + value.len(), Mark::Current)];
    egui::Frame::new()
        .fill(p.sunken)
        .corner_radius(radius(p.radius_md))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            marked_label(ui, &text, &marks, &base);
        });
}

/// M, G, J, K, en colonne sous la valeur courante.
fn decision_buttons(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    let p = palette();
    let out = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = p.space(2);
            [
                ui.add(
                    PillButton::primary("Masquer")
                        .icon(Icon::EyeOff)
                        .kbd("M")
                        .small()
                        .full_width(true),
                )
                .on_hover_text("Masquer cette valeur partout (M)")
                .clicked(),
                ui.add(
                    PillButton::secondary("Garder")
                        .icon(Icon::Eye)
                        .kbd("G")
                        .small()
                        .full_width(true),
                )
                .on_hover_text("Garder cette valeur visible partout (G)")
                .clicked(),
                ui.add(
                    PillButton::ghost("Suivante")
                        .icon(Icon::ChevronRight)
                        .kbd("J")
                        .small(),
                )
                .on_hover_text("Valeur à décider suivante (J)")
                .clicked(),
                ui.add(
                    PillButton::ghost("Précédente")
                        .icon(Icon::ChevronUp)
                        .kbd("K")
                        .small(),
                )
                .on_hover_text("Valeur à décider précédente (K)")
                .clicked(),
            ]
        })
        .inner;
    if out[0] {
        st.decide_current(true, false, session);
    }
    if out[1] {
        st.decide_current(false, false, session);
    }
    if out[2] {
        st.queue_step(true);
    }
    if out[3] {
        st.queue_step(false);
    }
    if out.iter().any(|x| *x) {
        ui.ctx().request_repaint();
    }
}

/// Valeurs suivantes de la file, à décider.
fn next_values(ui: &mut Ui, st: &mut AnonymizeState, current: usize) {
    let p = palette();
    let q = st.queue();
    let start = q.iter().position(|x| *x == current).map_or(0, |i| i + 1);
    let len = q.len();
    let next: Vec<usize> = (0..len)
        .filter_map(|k| q.get((start + k) % len.max(1)).copied())
        .filter(|id| *id != current && st.is_undecided(*id))
        .take(NEXT_SHOWN)
        .collect();
    if next.is_empty() {
        return;
    }
    let mut chosen = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for id in next {
            let Some(c) = st.candidate(id) else {
                continue;
            };
            let w = ui.available_width();
            let (rect, r) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
            let label = format!("{}, {}, à décider", c.value, c.kind.label());
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            let painter = ui.painter();
            if r.hovered() {
                painter.rect_filled(rect, radius(p.radius_sm), p.hover);
            }
            painter.line_segment(
                [pos2(rect.min.x, rect.max.y), pos2(rect.max.x, rect.max.y)],
                Stroke::new(1.0, p.border),
            );
            let mut job = LayoutJob::simple_singleline(
                c.value.clone(),
                font(Weight::Medium, p.text_sm),
                p.pending.ink,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(w * 0.6);
            if let Some(s) = job.sections.first_mut() {
                s.format.background = p.pending.fill;
            }
            let g = painter.layout_job(job);
            painter.galley(
                pos2(rect.min.x + 4.0, rect.center().y - g.size().y / 2.0),
                g,
                p.text,
            );
            painter.text(
                pos2(rect.max.x - 4.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                c.kind.label(),
                font(Weight::Regular, p.text_xs),
                p.text_secondary,
            );
            if r.has_focus() {
                painter.rect_stroke(
                    rect,
                    radius(p.radius_sm),
                    Stroke::new(2.0, p.border_focus),
                    StrokeKind::Inside,
                );
            }
            if r.clicked() {
                chosen = Some(id);
            }
        }
    });
    if let Some(id) = chosen {
        st.set_current(id);
    }
}

/// Plus rien à décider dans ce document.
fn done_card(ui: &mut Ui, st: &mut AnonymizeState) {
    let p = palette();
    let next = st.next_document_with_decisions();
    let next_name = next.and_then(|i| {
        st.review_documents()
            .into_iter()
            .find(|(j, _)| *j == i)
            .map(|(_, n)| n)
    });
    egui::Frame::new()
        .fill(p.brand.fill)
        .stroke(Stroke::new(1.5, p.primary))
        .corner_radius(radius(p.radius_xl))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = p.space(2);
            ui.horizontal(|ui| {
                Icon::CircleCheck.show(ui, p.accent, 20.0);
                ui.label(
                    RichText::new("Plus rien à décider dans ce document.")
                        .font(font(Weight::Semibold, p.text_sm))
                        .color(p.heading),
                );
            });
            match (next, next_name) {
                (Some(i), Some(name)) => {
                    let label = format!("Document suivant : {name}");
                    if ui
                        .add(PillButton::secondary(&label).icon(Icon::ChevronRight).small())
                        .clicked()
                    {
                        st.select_document(i);
                    }
                }
                _ => {
                    muted(ui, "Toutes les valeurs du dossier sont décidées. Les valeurs sûres restent masquées.");
                }
            }
        });
}

/// « Masquées · 90 ▸ » : valeurs sûres du document, masquées d'office ;
/// dépliée, chaque ligne est réversible. `max_h` : hauteur de la liste.
fn masked_section(ui: &mut Ui, st: &mut AnonymizeState, session: &Session, max_h: Option<f32>) {
    let p = palette();
    let sure = st.sure_values();
    if sure.is_empty() {
        return;
    }
    let n = st.sure_masked_count();
    let title = format!("Masquées · {n}");
    let open = st.review.masked_open;
    ui.painter().line_segment(
        [
            pos2(ui.min_rect().min.x, ui.cursor().min.y),
            pos2(
                ui.min_rect().min.x + ui.available_width(),
                ui.cursor().min.y,
            ),
        ],
        Stroke::new(1.0, p.border),
    );
    let toggle = ui
        .horizontal(|ui| {
            ui.set_min_height(CONTROL_SM + 4.0);
            ui.label(
                RichText::new(&title)
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.heading),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (label, icon) = if open {
                    ("Replier", Icon::ChevronUp)
                } else {
                    ("Afficher", Icon::ChevronRight)
                };
                let access = format!("{label} les valeurs masquées d'office ({n})");
                ui.add(
                    PillButton::ghost(label)
                        .icon(icon)
                        .small()
                        .accessible_label(&access),
                )
                .clicked()
            })
            .inner
        })
        .inner;
    if toggle {
        st.review.masked_open = !open;
    }
    if !st.review.masked_open {
        return;
    }
    let mut flip = None;
    let mut rows = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for id in &sure {
            let Some(c) = st.candidate(*id) else {
                continue;
            };
            let replacement = st
                .generalized(*id)
                .map(str::to_owned)
                .or_else(|| st.tag_preview.get(id).map(|t| t.to_string()));
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                ui.spacing_mut().item_spacing.x = p.space(2);
                let w = (ui.available_width() - 110.0).max(60.0);
                ui.allocate_ui_with_layout(vec2(w, 34.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let ink = if c.selected {
                        p.heading
                    } else {
                        p.text_secondary
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(&c.value)
                                .font(font(Weight::Semibold, p.text_sm))
                                .color(ink),
                        )
                        .truncate(),
                    );
                    let detail = match (&replacement, c.selected) {
                        (Some(r), true) => format!("{} · {r}", c.kind.label()),
                        _ => format!("{} · visible", c.kind.label()),
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(detail)
                                .font(font(Weight::Regular, p.text_xs))
                                .color(p.text_secondary),
                        )
                        .truncate(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (label, access) = if c.selected {
                        ("Garder", format!("Garder {} visible", c.value))
                    } else {
                        ("Masquer", format!("Masquer {}", c.value))
                    };
                    if ui
                        .add(PillButton::ghost(label).small().accessible_label(&access))
                        .clicked()
                    {
                        flip = Some(*id);
                    }
                });
            });
        }
    };
    match max_h {
        Some(h) => {
            egui::ScrollArea::vertical()
                .id_salt("valeurs-masquees")
                .max_height(h)
                .show(ui, |ui| rows(ui));
        }
        None => rows(ui),
    }
    if let Some(id) = flip {
        st.toggle_everywhere(id, session);
        ui.ctx().request_repaint();
    }
}

/// File « À décider » en panneau repliable (moins de 1280 px).
fn queue_panel(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    let p = palette();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(radius(p.radius_lg))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = p.space(2);
            let open = st.review.queue_open;
            let current = st.review.current.and_then(|id| st.candidate(id).cloned());
            // Sans icône : la ligne doit tenir dans 600 px.
            let mask = PillButton::primary("Masquer").kbd("M").small();
            let keep = PillButton::secondary("Garder").kbd("G").small();
            // Une ligne : la valeur courante à gauche, les actions à droite.
            let right_w = if current.is_some() {
                mask.natural_width(ui)
                    + keep.natural_width(ui)
                    + 3.0 * CONTROL_SM
                    + 5.0 * p.space(2)
            } else {
                CONTROL_SM + 200.0
            };
            let mut act = [false; 4];
            let toggle = ui
                .horizontal(|ui| {
                    ui.set_min_height(CONTROL_SM);
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    overline(ui, "À décider");
                    let (done, total) = st.queue_progress();
                    ui.label(
                        RichText::new(format!("{done} / {total}"))
                            .font(font(Weight::Bold, p.text_sm))
                            .color(p.heading),
                    );
                    match &current {
                        Some(c) => {
                            let w = (ui.available_width() - right_w).max(40.0);
                            let mut job = LayoutJob::default();
                            job.append(
                                &format!("« {} »", c.value),
                                0.0,
                                TextFormat::simple(font(Weight::Bold, p.text_sm), p.heading),
                            );
                            job.append(
                                &format!(" · {}", c.kind.label()),
                                0.0,
                                TextFormat::simple(
                                    font(Weight::Regular, p.text_xs),
                                    p.text_secondary,
                                ),
                            );
                            job.wrap = egui::text::TextWrapping::truncate_at_width(w);
                            ui.allocate_ui_with_layout(
                                vec2(w, CONTROL_SM),
                                Layout::left_to_right(Align::Center),
                                |ui| ui.add(egui::Label::new(job)),
                            )
                            .inner
                            .on_hover_text(format!(
                                "{} · {}",
                                c.value,
                                c.kind.label()
                            ));
                        }
                        None => {
                            muted(ui, "Plus rien à décider dans ce document.");
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = p.space(2);
                        let (icon, label) = if open {
                            (Icon::ChevronUp, "Replier le panneau « À décider »")
                        } else {
                            (Icon::ChevronDown, "Déplier le panneau « À décider »")
                        };
                        let toggle = icon_button(ui, icon, label, p.text_secondary).clicked();
                        if current.is_some() {
                            act[2] = icon_button(
                                ui,
                                Icon::ChevronRight,
                                "Suivante (J)",
                                p.text_secondary,
                            )
                            .clicked();
                            act[3] = icon_button(
                                ui,
                                Icon::ChevronUp,
                                "Précédente (K)",
                                p.text_secondary,
                            )
                            .clicked();
                            act[1] = ui
                                .add(keep)
                                .on_hover_text("Garder cette valeur visible partout (G)")
                                .clicked();
                            act[0] = ui
                                .add(mask)
                                .on_hover_text("Masquer cette valeur partout (M)")
                                .clicked();
                        } else if let Some(i) = st.next_document_with_decisions()
                            && ui
                                .add(
                                    PillButton::secondary("Document suivant")
                                        .icon(Icon::ChevronRight)
                                        .small(),
                                )
                                .clicked()
                        {
                            st.select_document(i);
                        }
                        toggle
                    })
                    .inner
                })
                .inner;
            if act[0] {
                st.decide_current(true, false, session);
            }
            if act[1] {
                st.decide_current(false, false, session);
            }
            if act[2] {
                st.queue_step(true);
            }
            if act[3] {
                st.queue_step(false);
            }
            if act.iter().any(|x| *x) {
                ui.ctx().request_repaint();
            }
            if toggle {
                st.review.queue_open = !open;
            }
            if st.review.queue_open {
                if let Some(id) = st.review.current {
                    excerpt_box(ui, st, id);
                    caption(ui, reason(st, id));
                }
                masked_section(ui, st, session, Some(MASKED_LIST_MAX_H));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_follow_the_plan() {
        // Plan V2, section 7.5 : trois colonnes à partir de 1280 px, deux en
        // dessous, une sous 900 px.
        assert_eq!(THREE_COLUMNS_MIN_W, 1280.0);
        assert_eq!(TWO_COLUMNS_MIN_W, 900.0);
        // Le texte garde au moins la moitié de la fenêtre en trois colonnes.
        const { assert!(LEFT_W + RIGHT_W < THREE_COLUMNS_MIN_W / 2.0) };
    }
}
