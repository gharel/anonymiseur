//! Texte du document à vérifier : chaque occurrence d'une valeur est surlignée
//! (« masqué » neutre, « à décider » jaune, valeur gardée sans surlignage).
//!
//! - Mise en page virtualisée : seuls les blocs visibles (plus celui du curseur
//!   et celui à montrer) sont mis en page ; les autres gardent leur hauteur
//!   mesurée, ou une estimation tant qu'ils n'ont jamais été affichés.
//! - Survol : infobulle (balise, type, « 10 fois ») ; clic : bascule de la
//!   valeur dans tout le dossier ; glisser : sélection de texte.
//! - Clavier : Tab atteint l'occurrence courante ; flèches, Espace, Maj+flèches
//!   et Échap sont rendus à l'appelant sous forme d'événements.
//! - Lecteurs d'écran : chaque surlignage visible est un nœud nommé
//!   « Jean Dupont, personne, masqué, 10 fois ».

use crate::ui::state::review::{Occurrence, TextRange};
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{Weight, font, palette, radius, with_alpha};
use anon_core::{BlockRole, EntityKind};
use anon_engine::{DocBlock, DocumentView, Span};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Color32, CornerRadius, EventFilter, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Taille du texte du document.
pub const TEXT_SIZE: f32 = 15.0;
/// Hauteur d'une ligne de texte (place pour le fond des surlignages).
pub const LINE_HEIGHT: f32 = 26.0;
/// Espace entre deux blocs.
const BLOCK_GAP: f32 = 10.0;
/// Hauteur du libellé d'emplacement (« Page 3 », « En-tête ») au-dessus d'un bloc.
const LABEL_H: f32 = 22.0;
/// Largeur moyenne estimée d'un caractère, en part de la taille du texte
/// (estimation de la hauteur d'un bloc jamais affiché).
const CHAR_WIDTH_EM: f32 = 0.5;
/// Marge intérieure du texte, à gauche et à droite.
pub const PAPER_PAD_X: f32 = 32.0;
/// Marge intérieure du texte, en haut et en bas.
const PAPER_PAD_Y: f32 = 20.0;

/// Ce que l'interface sait d'une valeur pour la dessiner et l'annoncer.
#[derive(Clone)]
pub struct ValueInfo {
    pub value: String,
    pub kind: EntityKind,
    /// Occurrences dans tout le dossier.
    pub occurrences: usize,
    /// Valeur ambiguë ou incertaine pas encore tranchée.
    pub pending: bool,
    /// Balise ou forme moins précise qui remplacera la valeur.
    pub replacement: Option<String>,
}

impl std::fmt::Debug for ValueInfo {
    /// Ni la valeur ni ce qui la remplace.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValueInfo")
            .field("kind", &self.kind)
            .field("occurrences", &self.occurrences)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

/// Style d'une occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Masked,
    Pending,
    Kept,
}

pub fn look(span: &Span, info: Option<&ValueInfo>) -> Look {
    if info.is_some_and(|i| i.pending) {
        Look::Pending
    } else if span.masked {
        Look::Masked
    } else {
        Look::Kept
    }
}

/// Type en minuscules dans une phrase (« personne »), sigles gardés (« IBAN »).
pub fn kind_in_sentence(kind: EntityKind) -> String {
    let label = kind.label();
    let mut chars = label.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if second.is_lowercase() || !second.is_alphabetic() => {
            first.to_lowercase().chain(label.chars().skip(1)).collect()
        }
        _ => label.to_owned(),
    }
}

/// Annonce d'un surlignage pour les lecteurs d'écran :
/// « Jean Dupont, personne, masqué, 10 fois ».
pub fn announce(span: &Span, info: &ValueInfo) -> String {
    let state = match (look(span, Some(info)), span.masked) {
        (Look::Pending, true) => "à décider, masqué pour l'instant",
        (Look::Pending, false) => "à décider, visible pour l'instant",
        (_, true) => "masqué",
        (_, false) => "gardé visible",
    };
    format!(
        "{}, {}, {state}, {}",
        info.value,
        kind_in_sentence(info.kind),
        count_label(info.occurrences, "fois", "fois")
    )
}

/// Infobulle d'un surlignage : ce qui remplacera la valeur, son type, le
/// nombre d'occurrences et l'effet du clic.
pub fn tooltip(span: &Span, info: &ValueInfo) -> String {
    let times = count_label(info.occurrences, "fois", "fois");
    let head = match (&info.replacement, span.masked) {
        (Some(r), true) => format!("{r} · {} · {times}", info.kind.label()),
        _ => format!("Gardé visible · {} · {times}", info.kind.label()),
    };
    let click = if span.masked {
        "Clic : garder partout"
    } else {
        "Clic : masquer partout"
    };
    if info.pending {
        format!("{head}\nÀ décider · {click}")
    } else {
        format!("{head}\n{click}")
    }
}

/// Libellé d'emplacement d'un bloc, s'il change par rapport au précédent.
fn block_label(b: &DocBlock, prev: Option<&DocBlock>) -> Option<String> {
    let of = |b: &DocBlock| -> Option<String> {
        let role = match b.role {
            BlockRole::Header => Some("En-tête"),
            BlockRole::Footer => Some("Pied de page"),
            BlockRole::Comment => Some("Commentaire"),
            BlockRole::Note => Some("Note"),
            BlockRole::Metadata => Some("Propriétés du fichier"),
            BlockRole::Hyperlink => Some("Lien"),
            BlockRole::Formula => Some("Formule"),
            _ => None,
        };
        role.map(str::to_owned).or_else(|| {
            let loc = b.location.trim();
            let page = loc.strip_prefix("page ")?;
            let n: String = page.chars().take_while(char::is_ascii_digit).collect();
            (!n.is_empty()).then(|| format!("Page {n}"))
        })
    };
    let mine = of(b)?;
    (prev.and_then(of).as_deref() != Some(mine.as_str())).then_some(mine)
}

/// Hauteur estimée d'un bloc jamais affiché.
fn estimate(b: &DocBlock, label: bool, width: f32) -> f32 {
    let per_row = (width / (TEXT_SIZE * CHAR_WIDTH_EM)).max(1.0);
    let rows: f32 = b
        .text
        .split('\n')
        .map(|line| (line.chars().count() as f32 / per_row).ceil().max(1.0))
        .sum();
    rows * LINE_HEIGHT + BLOCK_GAP + if label { LABEL_H } else { 0.0 }
}

/// Mise en page des blocs pour une largeur : libellés d'emplacement, hauteurs
/// estimées, et hauteurs mesurées des blocs déjà affichés.
#[derive(Clone, Default)]
struct Heights {
    width: f32,
    labels: Vec<Option<String>>,
    estimated: Vec<f32>,
    measured: Vec<Option<f32>>,
}

impl Heights {
    fn new(view: &DocumentView, width: f32) -> Self {
        let labels: Vec<Option<String>> = view
            .blocks
            .iter()
            .enumerate()
            .map(|(i, b)| block_label(b, i.checked_sub(1).and_then(|j| view.blocks.get(j))))
            .collect();
        let estimated = view
            .blocks
            .iter()
            .zip(&labels)
            .map(|(b, l)| estimate(b, l.is_some(), width))
            .collect();
        Self {
            width,
            labels,
            estimated,
            measured: vec![None; view.blocks.len()],
        }
    }

    fn of(&self, i: usize) -> f32 {
        self.measured
            .get(i)
            .copied()
            .flatten()
            .or_else(|| self.estimated.get(i).copied())
            .unwrap_or(LINE_HEIGHT)
    }
}

/// Événements du texte, appliqués par l'appelant à l'état.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DocEvents {
    /// Clic sur un surlignage : valeur à basculer dans tout le dossier.
    pub toggled: Option<usize>,
    /// Le curseur clavier va sur cette occurrence (clic, Tab).
    pub cursor: Option<Occurrence>,
    /// Flèches : occurrence suivante (`true`) ou précédente.
    pub step: Option<bool>,
    /// Espace ou Entrée sur l'occurrence courante.
    pub toggle_cursor: bool,
    /// Maj+→ ou Maj+← : étendre la sélection d'un mot.
    pub extend: Option<bool>,
    /// Nouvelle sélection à la souris (`Some(None)` : effacée).
    pub selection: Option<Option<TextRange>>,
    /// Le passage demandé est à l'écran.
    pub revealed: bool,
    /// Le texte a le focus clavier.
    pub focused: bool,
    /// Occurrence qui a le focus clavier.
    pub focused_at: Option<Occurrence>,
    /// Échap avec le focus dans le texte.
    pub escape: bool,
    /// Premier bloc entièrement visible à l'écran.
    pub first_visible: usize,
}

/// Bloc mis en page à cette image.
struct Placed {
    block: usize,
    pos: Pos2,
    galley: Arc<egui::Galley>,
}

/// Indice de caractère d'un octet du texte.
fn char_index(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |t| t.chars().count())
}

/// Octet d'un indice de caractère du texte.
fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(b, _)| b)
}

/// Rectangles (un par ligne) du passage `a..b` (caractères) d'une galée.
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

fn job_for(b: &DocBlock, values: &HashMap<usize, ValueInfo>, width: f32) -> LayoutJob {
    let p = palette();
    let base = TextFormat {
        font_id: font(Weight::Regular, TEXT_SIZE),
        color: p.text,
        line_height: Some(LINE_HEIGHT),
        valign: Align::Center,
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let mut pos = 0usize;
    for s in &b.spans {
        if s.start < pos {
            continue;
        }
        if let Some(t) = b.text.get(pos..s.start)
            && !t.is_empty()
        {
            job.append(t, 0.0, base.clone());
        }
        let Some(t) = b.text.get(s.start..s.end) else {
            continue;
        };
        let fmt = match look(s, values.get(&s.candidate)) {
            Look::Masked => TextFormat {
                background: p.masked.fill,
                color: p.masked.ink,
                underline: Stroke::new(1.5, p.masked.edge),
                ..base.clone()
            },
            Look::Pending => TextFormat {
                background: p.pending.fill,
                color: p.pending.ink,
                underline: Stroke::new(2.5, p.pending.edge),
                ..base.clone()
            },
            Look::Kept => base.clone(),
        };
        job.append(t, 0.0, fmt);
        pos = s.end;
    }
    if let Some(t) = b.text.get(pos..)
        && !t.is_empty()
    {
        job.append(t, 0.0, base.clone());
    }
    if job.is_empty() {
        job.append(" ", 0.0, base);
    }
    job
}

/// Identifiant du nœud d'une occurrence.
pub fn span_id(doc: &str, block: usize, start: usize) -> Id {
    Id::unique(("occurrence", doc, block, start))
}

/// Entrées du texte.
pub struct DocInput<'a> {
    pub view: &'a DocumentView,
    pub values: &'a HashMap<usize, ValueInfo>,
    pub cursor: Option<Occurrence>,
    pub selection: Option<TextRange>,
    pub reveal: Option<TextRange>,
}

impl std::fmt::Debug for DocInput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocInput")
            .field("view", &self.view)
            .field("cursor", &self.cursor)
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}

/// Dessine le texte dans tout l'espace disponible.
pub fn show(ui: &mut Ui, input: &DocInput<'_>) -> DocEvents {
    let p = palette();
    let mut ev = DocEvents::default();
    let view = input.view;
    let doc = view.name.as_str();
    let heights_id = Id::unique(("hauteurs", doc));
    let had_focus_id = Id::unique(("texte-focus", doc));
    let anchor_id = Id::unique(("debut-selection", doc));
    let prev_cursor_id = Id::unique(("curseur-precedent", doc));
    let had_focus: bool = ui.data(|d| d.get_temp(had_focus_id).unwrap_or(false));
    let prev_cursor: Option<Occurrence> = ui.data(|d| d.get_temp(prev_cursor_id)).flatten();

    let avail_w = ui.available_width();
    let width = (avail_w - 2.0 * PAPER_PAD_X - 12.0).max(120.0);
    let mut heights: Heights = ui
        .data_mut(|d| d.remove_temp(heights_id))
        .unwrap_or_default();
    if (heights.width - width).abs() > 0.5 || heights.measured.len() != view.blocks.len() {
        heights = Heights::new(view, width);
    }
    let labels = std::mem::take(&mut heights.labels);
    // Débuts des blocs (y relatif au haut du texte).
    let mut offsets = Vec::with_capacity(view.blocks.len() + 1);
    let mut y = PAPER_PAD_Y;
    for i in 0..view.blocks.len() {
        offsets.push(y);
        y += heights.of(i);
    }
    let total = y + PAPER_PAD_Y;

    // Passage à montrer hors de l'écran : on y saute d'abord par l'estimation.
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("texte-document", doc))
        .auto_shrink([false, false]);
    let jump_id = Id::unique(("saut", doc));
    if let Some(y) = ui.data_mut(|d| d.remove_temp::<f32>(jump_id)) {
        area = area.vertical_scroll_offset(y);
    }

    let mut jump_to: Option<f32> = None;
    let mut measured_changed = false;
    // Pas d'élastique au bord : la hauteur estimée des blocs jamais affichés
    // change quand ils sont mesurés, le texte ne doit pas rebondir.
    ui.spacing_mut().scroll.kinetic.rubber_band = false;
    area.show_viewport(ui, |ui, viewport| {
        let origin = ui.max_rect().min;
        let content = Rect::from_min_size(origin, vec2(avail_w.max(1.0), total));
        // Fond : sélection à la souris, clic hors des valeurs.
        let bg = ui.interact(
            content,
            Id::unique(("fond-texte", doc)),
            Sense::CLICK | Sense::DRAG,
        );

        // Blocs à mettre en page : visibles, plus curseur et passage à montrer.
        let first = offsets.partition_point(|&o| o < viewport.min.y - LINE_HEIGHT * 4.0);
        let first = first.saturating_sub(1);
        let mut wanted: Vec<usize> = (first..view.blocks.len())
            .take_while(|&i| offsets.get(i).is_some_and(|&o| o <= viewport.max.y))
            .collect();
        let visible: Vec<usize> = wanted.clone();
        // Premier bloc entièrement à l'écran.
        ev.first_visible = offsets.partition_point(|&o| o < viewport.min.y);
        for extra in [input.cursor.map(|c| c.block), input.reveal.map(|r| r.block)]
            .into_iter()
            .flatten()
        {
            if extra < view.blocks.len() && !wanted.contains(&extra) {
                wanted.push(extra);
            }
        }

        let left = origin.x + PAPER_PAD_X;
        let mut placed: Vec<Placed> = Vec::with_capacity(wanted.len());
        for &bi in &wanted {
            let Some(b) = view.blocks.get(bi) else {
                continue;
            };
            let mut top = origin.y + offsets.get(bi).copied().unwrap_or(0.0);
            let has_label = labels.get(bi).and_then(Option::as_ref);
            if let Some(label) = has_label {
                ui.painter().text(
                    pos2(left, top + LABEL_H / 2.0),
                    egui::Align2::LEFT_CENTER,
                    label.to_uppercase(),
                    font(Weight::Semibold, p.text_xs),
                    p.text_secondary,
                );
                top += LABEL_H;
            }
            let galley = ui.painter().layout_job(job_for(b, input.values, width));
            let h = galley.size().y + BLOCK_GAP + if has_label.is_some() { LABEL_H } else { 0.0 };
            if let Some(m) = heights.measured.get_mut(bi)
                && m.is_none_or(|old| (old - h).abs() > 0.5)
            {
                *m = Some(h);
                measured_changed = true;
            }
            placed.push(Placed {
                block: bi,
                pos: pos2(left, top),
                galley,
            });
        }

        for pl in &placed {
            ui.painter().galley(pl.pos, Arc::clone(&pl.galley), p.text);
        }
        // Sélection, par-dessus le texte (les surlignages ont leur propre fond).
        if let Some(sel) = input.selection
            && let Some(pl) = placed.iter().find(|pl| pl.block == sel.block)
            && let Some(b) = view.blocks.get(sel.block)
        {
            let (a, z) = (char_index(&b.text, sel.start), char_index(&b.text, sel.end));
            for r in range_rects(&pl.galley, a, z) {
                ui.painter().rect(
                    r.translate(pl.pos.to_vec2()),
                    radius(2.0),
                    with_alpha(p.primary, 0.28),
                    Stroke::new(1.0, p.border_focus),
                    StrokeKind::Inside,
                );
            }
        }

        // Occurrences : survol, clic, focus, annonce.
        let tab_target = input.cursor.or_else(|| {
            visible.iter().find_map(|bi| {
                view.blocks.get(*bi).and_then(|b| {
                    b.spans.first().map(|s| Occurrence {
                        block: *bi,
                        start: s.start,
                    })
                })
            })
        });
        let mut focused_occ = None;
        for pl in &placed {
            let Some(b) = view.blocks.get(pl.block) else {
                continue;
            };
            for s in &b.spans {
                let Some(info) = input.values.get(&s.candidate) else {
                    continue;
                };
                let occ = Occurrence {
                    block: pl.block,
                    start: s.start,
                };
                let rects: Vec<Rect> = range_rects(
                    &pl.galley,
                    char_index(&b.text, s.start),
                    char_index(&b.text, s.end),
                )
                .into_iter()
                .map(|r| r.translate(pl.pos.to_vec2()))
                .collect();
                let Some(first_rect) = rects.first().copied() else {
                    continue;
                };
                let is_cursor = input.cursor == Some(occ);
                let sense = if tab_target == Some(occ) {
                    Sense::click()
                } else {
                    Sense::CLICK
                };
                let id = span_id(doc, pl.block, s.start);
                let r = ui.interact(first_rect, id, sense);
                let label = announce(s, info);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                // Les lignes suivantes d'une valeur coupée réagissent aussi.
                let mut hovered = r.hovered();
                let mut clicked = r.clicked();
                for (k, extra) in rects.iter().enumerate().skip(1) {
                    let rr = ui.interact(*extra, id.with(k), Sense::CLICK);
                    hovered |= rr.hovered();
                    clicked |= rr.clicked();
                }
                let look = look(s, Some(info));
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    for rr in &rects {
                        ui.painter().rect_stroke(
                            rr.expand(2.0),
                            radius(4.0),
                            Stroke::new(1.5, with_alpha(p.border_focus, 0.6)),
                            StrokeKind::Outside,
                        );
                    }
                }
                if look == Look::Kept && hovered {
                    for rr in &rects {
                        ui.painter().rect_filled(
                            *rr,
                            CornerRadius::same(3),
                            with_alpha(p.hover_strong, 0.5),
                        );
                    }
                }
                let has_focus = r.has_focus();
                if is_cursor || has_focus {
                    let w = if has_focus { 2.5 } else { 1.5 };
                    for rr in &rects {
                        ui.painter().rect_stroke(
                            rr.expand(3.0),
                            radius(5.0),
                            Stroke::new(w, p.border_focus),
                            StrokeKind::Outside,
                        );
                    }
                }
                if has_focus {
                    focused_occ = Some(occ);
                }
                if hovered {
                    let tip = tooltip(s, info);
                    r.clone().on_hover_text(tip);
                }
                if clicked {
                    ev.toggled = Some(s.candidate);
                    ev.cursor = Some(occ);
                    r.request_focus();
                }
            }
        }

        // Le texte avait le focus et le curseur a bougé (flèches, J, K) : le
        // focus suit. S'il est parti par Tab, le curseur n'a pas bougé : on le
        // laisse partir.
        if had_focus
            && input.cursor != prev_cursor
            && let Some(c) = input.cursor
            && focused_occ != Some(c)
        {
            let id = span_id(doc, c.block, c.start);
            ui.memory_mut(|m| m.request_focus(id));
            ui.ctx().request_repaint();
            focused_occ = Some(c);
        } else if let Some(f) = focused_occ
            && input.cursor != Some(f)
            && ev.cursor.is_none()
        {
            // Arrivée par Tab sur la première occurrence visible.
            ev.cursor = Some(f);
        }
        ev.focused = focused_occ.is_some();
        ev.focused_at = focused_occ;
        if let Some(f) = focused_occ {
            let id = span_id(doc, f.block, f.start);
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }

        // Sélection à la souris, limitée au bloc où elle commence.
        let hit = |pos: Pos2| -> Option<(usize, usize)> {
            let pl = placed
                .iter()
                .filter(|pl| visible.contains(&pl.block))
                .min_by(|a, b| {
                    let da = dist_y(pos.y, a.pos.y, a.pos.y + a.galley.size().y);
                    let db = dist_y(pos.y, b.pos.y, b.pos.y + b.galley.size().y);
                    da.total_cmp(&db)
                })?;
            let b = view.blocks.get(pl.block)?;
            let c = pl.galley.cursor_from_pos(pos - pl.pos);
            Some((pl.block, byte_index(&b.text, c.index.0)))
        };
        // Le glissement commence là où le bouton a été enfoncé.
        if bg.drag_started()
            && let Some(pos) = ui
                .input(|i| i.pointer.press_origin())
                .or(bg.interact_pointer_pos())
            && let Some(start) = hit(pos)
        {
            ui.data_mut(|d| d.insert_temp(anchor_id, start));
        }
        let anchor: Option<(usize, usize)> = ui.data(|d| d.get_temp(anchor_id));
        if bg.dragged()
            && let Some((block, a)) = anchor
            && let Some(pos) = bg.interact_pointer_pos()
            && let Some((hb, h)) = hit(pos)
            && let Some(b) = view.blocks.get(block)
        {
            let h = if hb == block {
                h
            } else if hb > block {
                b.text.len()
            } else {
                0
            };
            let range = TextRange {
                block,
                start: a.min(h),
                end: a.max(h),
            };
            // Seulement si elle change : un glissement immobile ne redessine pas.
            let new = (range.start < range.end).then_some(range);
            if new != input.selection {
                ev.selection = Some(new);
            }
        }
        if bg.drag_stopped() {
            ui.data_mut(|d| d.remove::<(usize, usize)>(anchor_id));
        }
        if bg.clicked() && input.selection.is_some() {
            ev.selection = Some(None);
        }
        if bg.dragged() || bg.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }

        // Passage à montrer : visible, on le centre ; sinon on y saute.
        if let Some(rv) = input.reveal {
            let rect = placed
                .iter()
                .find(|pl| pl.block == rv.block)
                .and_then(|pl| {
                    let b = view.blocks.get(rv.block)?;
                    let rs = range_rects(
                        &pl.galley,
                        char_index(&b.text, rv.start),
                        char_index(&b.text, rv.end),
                    );
                    rs.into_iter()
                        .reduce(|a, b| a.union(b))
                        .map(|r| r.translate(pl.pos.to_vec2()))
                });
            let on_screen = visible.contains(&rv.block);
            match rect {
                Some(r) if on_screen => {
                    if !viewport
                        .translate(origin.to_vec2())
                        .shrink2(vec2(0.0, 24.0))
                        .contains_rect(r)
                    {
                        ui.scroll_to_rect_animation(
                            r,
                            Some(Align::Center),
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                    ev.revealed = true;
                }
                _ => {
                    let target =
                        offsets.get(rv.block).copied().unwrap_or(0.0) - viewport.height() / 3.0;
                    jump_to = Some(target.max(0.0));
                }
            }
        }

        ui.advance_cursor_after_rect(content);
    });

    // Clavier, quand une occurrence a le focus.
    if ev.focused {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::SHIFT, Key::ArrowRight) {
                ev.extend = Some(true);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowLeft) {
                ev.extend = Some(false);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight)
                || i.consume_key(Modifiers::NONE, Key::ArrowDown)
            {
                ev.step = Some(true);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft)
                || i.consume_key(Modifiers::NONE, Key::ArrowUp)
            {
                ev.step = Some(false);
            }
            if i.consume_key(Modifiers::NONE, Key::Space)
                || i.consume_key(Modifiers::NONE, Key::Enter)
            {
                ev.toggle_cursor = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                ev.escape = true;
            }
        });
        // Échap : efface la sélection, sinon quitte le texte.
        if ev.escape {
            if input.selection.is_some() {
                ev.selection = Some(None);
            } else if let Some(f) = ev.focused_at {
                let id = span_id(doc, f.block, f.start);
                ui.memory_mut(|m| m.surrender_focus(id));
            }
        }
    }
    if let Some(y) = jump_to {
        ui.data_mut(|d| d.insert_temp(jump_id, y));
        ui.ctx().request_repaint();
    }
    if measured_changed {
        ui.ctx().request_repaint();
    }
    heights.labels = labels;
    ui.data_mut(|d| {
        d.insert_temp(heights_id, heights);
        d.insert_temp(had_focus_id, ev.focused);
        d.insert_temp(prev_cursor_id, input.cursor);
    });
    ev
}

/// Distance verticale d'un point à un intervalle.
fn dist_y(y: f32, top: f32, bottom: f32) -> f32 {
    if y < top {
        top - y
    } else if y > bottom {
        y - bottom
    } else {
        0.0
    }
}

/// Repère de légende : « Masqué », « À décider ».
pub fn legend_swatch(ui: &mut Ui, look: Look, text: &str) {
    let p = palette();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(26.0, 14.0), Sense::hover());
        let (fill, edge, w) = match look {
            Look::Pending => (p.pending.fill, p.pending.edge, 3.0),
            _ => (p.masked.fill, p.masked.edge, 2.0),
        };
        ui.painter().rect_filled(rect, radius(4.0), fill);
        ui.painter().line_segment(
            [
                pos2(rect.left() + 2.0, rect.bottom() - w / 2.0),
                pos2(rect.right() - 2.0, rect.bottom() - w / 2.0),
            ],
            Stroke::new(w, edge),
        );
        ui.label(
            egui::RichText::new(text)
                .font(font(Weight::Regular, p.text_xs))
                .color(p.text_secondary),
        );
    });
}

/// Vrai si une occurrence du document `doc` avait le focus clavier à la
/// dernière image (M et G s'appliquent alors à la valeur sous le curseur).
pub fn has_focus(ctx: &egui::Context, doc: &str) -> bool {
    ctx.data(|d| {
        d.get_temp(Id::unique(("texte-focus", doc)))
            .unwrap_or(false)
    })
}

/// Couleur du fond de la page de texte.
pub fn paper_fill() -> Color32 {
    palette().surface
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moteur::IaStatus;
    use crate::ui::AnonApp;
    use crate::ui::platform::NullPlatform;
    use crate::ui::state::workspace::Step;
    use std::time::{Duration, Instant};

    /// Budget d'une image : 60 images par seconde.
    const FRAME_BUDGET: Duration = Duration::from_millis(16);
    /// Paragraphes d'une page du document fictif (environ 3 600 caractères).
    const PARAGRAPHS_PER_PAGE: usize = 12;

    /// Document fictif de `pages` pages : personnes, villes, emails en
    /// `example.com`, des paragraphes tous différents.
    fn long_text(pages: usize) -> String {
        const PRENOMS: [(&str, &str); 8] = [
            ("Hélène", "helene"),
            ("Pierre", "pierre"),
            ("Marie", "marie"),
            ("Jean", "jean"),
            ("Lucie", "lucie"),
            ("Paul", "paul"),
            ("Claire", "claire"),
            ("Louis", "louis"),
        ];
        const NOMS: [(&str, &str); 8] = [
            ("Kaloï", "kaloi"),
            ("Garnier", "garnier"),
            ("Dupont", "dupont"),
            ("Martin", "martin"),
            ("Lefèvre", "lefevre"),
            ("Bernard", "bernard"),
            ("Moreau", "moreau"),
            ("Petit", "petit"),
        ];
        const VILLES: [&str; 5] = ["Nouméa", "Dumbéa", "Lyon", "Bordeaux", "Rennes"];
        let mut s = String::new();
        for k in 0..pages * PARAGRAPHS_PER_PAGE {
            let (pr, pr_mail) = PRENOMS[k % PRENOMS.len()];
            let (no, no_mail) = NOMS[(k / PRENOMS.len()) % NOMS.len()];
            let ville = VILLES[k % VILLES.len()];
            s.push_str(&format!(
                "Le {jour} mars, {pr} {no} a rencontré l'équipe à {ville} pour le dossier {k}. \
Contact : {pr_mail}.{no_mail}@example.com. Le compte rendu précise les points abordés, les \
décisions prises et les suites à donner, dont la relecture du contrat, la mise à jour du \
planning et l'envoi du devis numéro {k} avant la fin du mois, comme convenu avec la direction.\n",
                jour = k % 28 + 1,
            ));
        }
        s
    }

    /// Une image de l'application entière, mise en page et découpée en triangles.
    fn frame(ctx: &egui::Context, app: &mut AnonApp, events: Vec<egui::Event>) -> Duration {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            events,
            ..Default::default()
        };
        let start = Instant::now();
        let mut out = ctx.run_ui(input, |ui| app.show(ui));
        let shapes = std::mem::take(&mut out.shapes);
        let _ = ctx.tessellate(shapes, out.pixels_per_point);
        let elapsed = start.elapsed();
        // En debug, epaint refuse qu'on abandonne des textures non appliquées.
        out.textures_delta.clear();
        elapsed
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn long_document_frames_stay_under_16_ms() {
        let engine = Arc::new(anon_engine::Engine::standard());
        let ctx = egui::Context::default();
        let mut app = AnonApp::with_engine(
            &ctx,
            Box::new(NullPlatform::default()),
            Arc::clone(&engine),
            IaStatus::Absente,
        );
        let text = long_text(200);
        assert!(text.len() > 600_000, "{} octets", text.len());
        let st = &mut app.anonymize;
        assert!(st.add_pasted(&text).is_ok());
        let job = st.begin_analysis().unwrap();
        let t = Instant::now();
        let analysis = engine.analyze(
            &job.texts,
            &st.analyze_options(),
            &app.session,
            &anon_engine::NoProgress,
        );
        let analyze = t.elapsed();
        let mut analysis = analysis;
        // Des valeurs à décider : les villes, marquées ambiguës.
        for c in &mut analysis.candidates {
            if c.kind == EntityKind::Lieu {
                c.ambiguous = true;
            }
        }
        let t = Instant::now();
        let built = analysis.document(0, &app.session).map(|v| v.span_count());
        let view_build = t.elapsed();
        assert!(built.is_some_and(|n| n > 0));
        assert!(
            app.anonymize
                .analysis_done(job.texts, Vec::new(), analysis, &app.session)
        );
        app.workspace.enter(Step::Verify);

        // Mise en route : polices, première vue du document (calculée une fois).
        let first = frame(&ctx, &mut app, Vec::new());
        for _ in 0..3 {
            frame(&ctx, &mut app, Vec::new());
        }
        let view = app.anonymize.document_view().unwrap();
        assert_eq!(view.blocks.len(), 200 * PARAGRAPHS_PER_PAGE);
        let spans = view.span_count();
        let values = app
            .anonymize
            .analysis
            .as_ref()
            .map_or(0, |a| a.candidates.len());

        // Trois passages du même parcours ; pour chaque image, la médiane des
        // trois mesures (l'ordinateur peut être occupé par autre chose).
        let runs: Vec<Vec<(&str, Duration)>> = (0..RUNS)
            .map(|run| scenario(&ctx, &mut app, run == 0))
            .collect();
        // Une fois le recalcul réparti terminé, la vue est exacte.
        assert!(!app.anonymize.review.refreshing());
        let exact = app
            .anonymize
            .analysis
            .as_ref()
            .and_then(|a| a.document(0, &app.session));
        assert_eq!(app.anonymize.document_view(), exact.as_ref());

        let frames = runs.first().map_or(0, Vec::len);
        assert!(runs.iter().all(|r| r.len() == frames));
        let medians: Vec<(&str, Duration)> = (0..frames)
            .map(|k| {
                let mut d: Vec<Duration> =
                    runs.iter().filter_map(|r| r.get(k).map(|x| x.1)).collect();
                d.sort();
                let what = runs.first().and_then(|r| r.get(k)).map_or("", |x| x.0);
                (what, d.get(d.len() / 2).copied().unwrap_or_default())
            })
            .collect();
        let all: Vec<Duration> = runs.iter().flatten().map(|x| x.1).collect();
        let (worst_what, worst) = medians
            .iter()
            .copied()
            .max_by_key(|(_, d)| *d)
            .unwrap_or(("", Duration::ZERO));
        let mean = all.iter().sum::<Duration>() / all.len().max(1) as u32;
        let slowest = all.iter().max().copied().unwrap_or_default();
        println!(
            "Document de 200 pages : {} octets, {} blocs, {spans} occurrences, {values} valeurs ; analyse {analyze:?}, vue du document {view_build:?}, première image {first:?} ; {frames} images × {RUNS} : moyenne {mean:?}, pire médiane {worst:?} ({worst_what}), pire mesure {slowest:?}",
            text.len(),
            200 * PARAGRAPHS_PER_PAGE,
        );
        // 16 ms ne s'exigent que d'un code optimisé (`cargo test --release`).
        if env!("ANON_OPT_LEVEL") != "0" {
            for (what, d) in &medians {
                assert!(
                    *d < FRAME_BUDGET,
                    "{what} : {d:?} (budget {FRAME_BUDGET:?})"
                );
            }
        }
    }

    /// Nombre de passages du parcours mesuré.
    const RUNS: usize = 3;

    /// Parcours mesuré : repos, défilement rapide, sauts, J, M, G et le
    /// recalcul qui suit. `first` : vérifie que M tranche bien une valeur.
    fn scenario(
        ctx: &egui::Context,
        app: &mut AnonApp,
        first: bool,
    ) -> Vec<(&'static str, Duration)> {
        let mut times = Vec::new();
        for _ in 0..5 {
            times.push(("au repos", frame(ctx, app, Vec::new())));
        }
        // Défilement rapide : de nouveaux paragraphes à chaque image.
        for _ in 0..40 {
            let wheel = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -2500.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
            };
            let hover = egui::Event::PointerMoved(pos2(640.0, 400.0));
            times.push(("défilement", frame(ctx, app, vec![hover, wheel])));
        }
        // Saut à la dernière occurrence (fin du document), puis retour au début.
        app.anonymize.review.cursor = None;
        app.anonymize.move_cursor(false);
        for _ in 0..4 {
            times.push(("saut à la fin", frame(ctx, app, Vec::new())));
        }
        app.anonymize.review.cursor = None;
        app.anonymize.move_cursor(true);
        for _ in 0..4 {
            times.push(("saut au début", frame(ctx, app, Vec::new())));
        }
        // File : J, puis des décisions (M, G) et la vue recalculée.
        assert!(!app.anonymize.queue().is_empty(), "des valeurs à décider");
        times.push(("J", frame(ctx, app, vec![key(Key::J)])));
        let decided = app.anonymize.decided.len();
        times.push(("M", frame(ctx, app, vec![key(Key::M)])));
        if first {
            assert_eq!(app.anonymize.decided.len(), decided + 1, "M a tranché");
        }
        for _ in 0..3 {
            times.push(("après M", frame(ctx, app, Vec::new())));
        }
        times.push(("G", frame(ctx, app, vec![key(Key::G)])));
        for _ in 0..40 {
            times.push(("après G", frame(ctx, app, Vec::new())));
        }
        times
    }

    fn block(text: &str, role: BlockRole, location: &str) -> DocBlock {
        DocBlock {
            text: text.to_owned(),
            role,
            location: location.to_owned(),
            spans: Vec::new(),
        }
    }

    #[test]
    fn announcements_and_tooltips_say_everything() {
        let info = ValueInfo {
            value: "Jean Dupont".into(),
            kind: EntityKind::Personne,
            occurrences: 10,
            pending: false,
            replacement: Some("[PERSONNE_1]".into()),
        };
        let span = Span {
            start: 0,
            end: 11,
            candidate: 1,
            masked: true,
        };
        assert_eq!(
            announce(&span, &info),
            "Jean Dupont, personne, masqué, 10 fois"
        );
        assert_eq!(
            tooltip(&span, &info),
            "[PERSONNE_1] · Personne · 10 fois\nClic : garder partout"
        );
        let kept = Span {
            masked: false,
            ..span
        };
        assert_eq!(
            announce(&kept, &info),
            "Jean Dupont, personne, gardé visible, 10 fois"
        );
        assert!(tooltip(&kept, &info).ends_with("Clic : masquer partout"));
        let pending = ValueInfo {
            pending: true,
            occurrences: 1,
            ..info
        };
        assert_eq!(
            announce(&span, &pending),
            "Jean Dupont, personne, à décider, masqué pour l'instant, 1 fois"
        );
        assert_eq!(look(&span, Some(&pending)), Look::Pending);
        assert_eq!(kind_in_sentence(EntityKind::Iban), "IBAN");
        assert_eq!(kind_in_sentence(EntityKind::Nir), "n° de sécurité sociale");
        assert_eq!(kind_in_sentence(EntityKind::Prenom), "prénom");
    }

    #[test]
    fn block_labels_only_when_the_place_changes() {
        let a = block("x", BlockRole::Body, "page 1");
        let b = block("y", BlockRole::Body, "page 1");
        let c = block("z", BlockRole::Body, "page 2");
        let h = block("t", BlockRole::Header, "word/header1.xml");
        assert_eq!(block_label(&a, None).as_deref(), Some("Page 1"));
        assert_eq!(block_label(&b, Some(&a)), None);
        assert_eq!(block_label(&c, Some(&b)).as_deref(), Some("Page 2"));
        assert_eq!(block_label(&h, Some(&c)).as_deref(), Some("En-tête"));
        let body = block("u", BlockRole::Body, "word/document.xml");
        assert_eq!(block_label(&body, Some(&h)), None);
    }

    #[test]
    fn estimates_grow_with_the_text() {
        let short = block("Bonjour.", BlockRole::Body, "ligne 1");
        let long = block(&"mot ".repeat(400), BlockRole::Body, "ligne 2");
        let (s, l) = (
            estimate(&short, false, 600.0),
            estimate(&long, false, 600.0),
        );
        assert_eq!(s, LINE_HEIGHT + BLOCK_GAP);
        assert!(l > 10.0 * LINE_HEIGHT, "{l}");
        assert!(estimate(&short, true, 600.0) > s);
        assert_eq!(char_index("éa", 2), 1);
        assert_eq!(byte_index("éa", 1), 2);
        assert_eq!(byte_index("éa", 9), 3);
    }
}
