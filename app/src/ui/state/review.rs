//! Vérification dans le document (maquette B) : document affiché, file
//! « À décider », valeurs sûres repliées sous « Masquées », curseur clavier,
//! sélection de texte (« Masquer partout », « Toujours masquer ») et recherche.
//! Chaque décision passe par l'historique d'annulation de l'état. Sans egui.

use super::anonymize::{AnonymizeState, needs_confirmation};
use super::history::Action;
use anon_core::{EntityKind, Session};
use anon_engine::{Candidate, DocumentView};
use anon_replace::{fold, key};
use std::collections::{HashMap, HashSet};

/// Caractères gardés de part et d'autre de la valeur dans l'extrait de la file.
pub const EXCERPT_CHARS: usize = 60;
/// Texte recalculé au plus par image après une décision (octets) : sur un long
/// document, le recalcul exact des blocs touchés se répartit sur plusieurs
/// images pour que chacune reste sous 16 ms.
pub const REFRESH_BYTES_PER_FRAME: usize = 8 * 1024;

/// Vue de l'étape Vérifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerifyMode {
    /// Texte surligné et file « À décider » (vue principale).
    #[default]
    Document,
    /// Tableau de la V1 : tri, filtres, traitements en masse.
    Table,
}

/// Intervalle de texte d'un bloc du document affiché (octets, sur des
/// frontières de caractères).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

/// Occurrence d'une valeur dans le document affiché : bloc et début de la plage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub block: usize,
    pub start: usize,
}

/// État de la vérification dans le document.
pub struct Review {
    pub mode: VerifyMode,
    /// Document affiché : indice dans `Analysis::files`.
    pub doc: usize,
    /// Valeur courante de la file « À décider » (identifiant de candidat).
    pub current: Option<usize>,
    /// Occurrence qui a le curseur clavier dans le texte.
    pub cursor: Option<Occurrence>,
    /// Passage à faire défiler jusqu'à l'écran à la prochaine image.
    pub reveal: Option<TextRange>,
    /// Texte sélectionné (souris, Maj+flèches, recherche).
    pub selection: Option<TextRange>,
    /// Type donné au texte sélectionné quand il est masqué.
    pub selection_kind: EntityKind,
    /// Liste « Masquées » dépliée.
    pub masked_open: bool,
    /// Panneau « À décider » déplié (fenêtre de moins de 1280 px).
    pub queue_open: bool,
    /// Images affichées à la place du texte (document qui a les deux) ;
    /// `None` : les images si le texte n'a aucune valeur (PDF scanné).
    pub show_images: Option<bool>,
    /// Recherche dans le document.
    pub search: String,
    /// Vue du document `doc`, et ce qu'une décision y a rendu périmé.
    view: Option<(usize, DocumentView)>,
    dirty: Dirty,
    /// Blocs à recalculer exactement, après une décision.
    pending_blocks: Vec<usize>,
    /// Premier bloc visible à l'écran : les blocs proches sont recalculés d'abord.
    pub visible_hint: usize,
    /// Candidats du document, dans l'ordre de leur première occurrence.
    order: Vec<usize>,
}

impl Default for Review {
    fn default() -> Self {
        Self {
            mode: VerifyMode::Document,
            doc: 0,
            current: None,
            cursor: None,
            reveal: None,
            selection: None,
            selection_kind: EntityKind::Autre,
            masked_open: false,
            queue_open: false,
            show_images: None,
            search: String::new(),
            view: None,
            dirty: Dirty::Clean,
            pending_blocks: Vec::new(),
            visible_hint: 0,
            order: Vec::new(),
        }
    }
}

impl std::fmt::Debug for Review {
    /// Ni la recherche ni le texte du document.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Review")
            .field("mode", &self.mode)
            .field("doc", &self.doc)
            .field("current", &self.current)
            .field("cursor", &self.cursor)
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}

impl Review {
    /// Nouvelle analyse : tout repart de zéro, sauf la vue choisie.
    pub fn reset(&mut self) {
        let mode = self.mode;
        *self = Self {
            mode,
            ..Self::default()
        };
    }

    /// Une décision a changé ce qui est masqué : la vue sera recalculée.
    pub fn invalidate(&mut self) {
        self.dirty = Dirty::Full;
    }

    /// Seule la sélection des valeurs `ids` a changé : la vue ne recalculera
    /// que les blocs où elles apparaissent.
    pub fn invalidate_values(&mut self, ids: &[usize]) {
        match &mut self.dirty {
            Dirty::Clean => self.dirty = Dirty::Values(ids.to_vec()),
            Dirty::Values(v) => v.extend_from_slice(ids),
            Dirty::Full => {}
        }
    }

    /// Vue du document affiché (éventuellement périmée d'une décision).
    pub fn view(&self) -> Option<&DocumentView> {
        self.view.as_ref().map(|(_, v)| v)
    }

    /// Vrai si des blocs attendent encore leur recalcul exact.
    pub fn refreshing(&self) -> bool {
        !self.pending_blocks.is_empty()
    }
}

/// Ce qui est à recalculer dans la vue du document affiché.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Dirty {
    #[default]
    Clean,
    /// Seule la sélection de ces valeurs a changé.
    Values(Vec<usize>),
    /// Tout (valeur ajoutée ou retirée, type, casse, nouveau document).
    Full,
}

/// Début du mot suivant la position `at` (fin du mot pour `forward`).
fn word_edge(text: &str, at: usize, forward: bool) -> usize {
    let is_word = |c: char| c.is_alphanumeric();
    if forward {
        let rest = text.get(at..).unwrap_or("");
        let mut seen_word = false;
        for (i, c) in rest.char_indices() {
            if is_word(c) {
                seen_word = true;
            } else if seen_word {
                return at + i;
            }
        }
        text.len()
    } else {
        let head = text.get(..at).unwrap_or("");
        let mut seen_word = false;
        for (i, c) in head.char_indices().rev() {
            if is_word(c) {
                seen_word = true;
            } else if seen_word {
                return i + c.len_utf8();
            }
        }
        0
    }
}

impl AnonymizeState {
    // --- Document affiché ----------------------------------------------------

    /// Documents vérifiables du dossier : (indice dans l'analyse, nom).
    pub fn review_documents(&self) -> Vec<(usize, String)> {
        self.analysis
            .as_ref()
            .map(|a| {
                a.files
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.error.is_none())
                    .map(|(i, f)| (i, f.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Nom du document affiché.
    pub fn review_document_name(&self) -> Option<String> {
        let a = self.analysis.as_ref()?;
        a.files.get(self.review.doc).map(|f| f.name.clone())
    }

    /// Vue du document affiché, telle que calculée à la dernière synchronisation.
    pub fn document_view(&self) -> Option<&DocumentView> {
        self.review.view()
    }

    /// Recalcule ce qui doit l'être avant d'afficher : vue du document (après
    /// une décision ou un changement de document), valeur courante de la file,
    /// curseur et sélection encore valides.
    pub fn sync_review(&mut self, session: &Session) {
        let docs = self.review_documents();
        if docs.is_empty() {
            self.review.view = None;
            self.review.order.clear();
            return;
        }
        if !docs.iter().any(|(i, _)| *i == self.review.doc) {
            self.review.doc = docs.first().map_or(0, |(i, _)| *i);
        }
        let doc = self.review.doc;
        let same_doc = self.review.view.as_ref().is_some_and(|(d, _)| *d == doc);
        match (same_doc, std::mem::take(&mut self.review.dirty)) {
            (true, Dirty::Clean) => {}
            (true, Dirty::Values(ids)) => {
                // Tout de suite : les occurrences connues de ces valeurs prennent
                // leur nouvel état. Puis, image après image, le recalcul exact des
                // blocs où elles peuvent apparaître (chevauchements compris).
                let flags: HashMap<usize, bool> = ids
                    .iter()
                    .filter_map(|id| self.candidate(*id).map(|c| (*id, c.selected)))
                    .collect();
                let forms: Vec<(EntityKind, Vec<String>)> = ids
                    .iter()
                    .filter_map(|id| {
                        self.candidate(*id).map(|c| {
                            let f = std::iter::once(&c.value).chain(&c.variants).cloned();
                            (c.kind, f.collect())
                        })
                    })
                    .collect();
                if let Some((_, view)) = self.review.view.as_mut() {
                    for s in view.blocks.iter_mut().flat_map(|b| b.spans.iter_mut()) {
                        if let Some(masked) = flags.get(&s.candidate) {
                            s.masked = *masked;
                        }
                    }
                    let blocks = forms.iter().flat_map(|(kind, f)| {
                        let refs: Vec<&str> = f.iter().map(String::as_str).collect();
                        view.blocks_mentioning(*kind, &refs)
                    });
                    self.review.pending_blocks.extend(blocks);
                    self.review.pending_blocks.sort_unstable();
                    self.review.pending_blocks.dedup();
                }
            }
            _ => {
                let view = self
                    .analysis
                    .as_ref()
                    .and_then(|a| a.document(doc, session));
                self.review.view = view.map(|v| (doc, v));
                self.review.pending_blocks.clear();
                self.review.order = self.reading_order();
            }
        }
        self.refresh_pending(session);
        let queue = self.queue();
        if self.review.current.is_none_or(|c| !queue.contains(&c)) {
            self.review.current = queue.iter().copied().find(|id| self.is_undecided(*id));
        }
        if let Some(c) = self.review.cursor
            && self.span_at(c).is_none()
        {
            self.review.cursor = None;
        }
        if let Some(s) = self.review.selection
            && self
                .document_view()
                .and_then(|v| v.blocks.get(s.block))
                .is_none_or(|b| b.text.get(s.start..s.end).is_none())
        {
            self.review.selection = None;
        }
    }

    /// Recalcule exactement une part des blocs touchés par une décision, les
    /// plus proches de l'écran d'abord, dans la limite de
    /// [`REFRESH_BYTES_PER_FRAME`].
    fn refresh_pending(&mut self, session: &Session) {
        if self.review.pending_blocks.is_empty() {
            return;
        }
        let hint = self.review.visible_hint;
        let mut pending = std::mem::take(&mut self.review.pending_blocks);
        pending.sort_by_key(|b| b.abs_diff(hint));
        let (Some(a), Some((_, view))) = (self.analysis.as_ref(), self.review.view.as_mut()) else {
            return;
        };
        let mut bytes = 0usize;
        let mut now = Vec::new();
        let mut later = Vec::new();
        for b in pending {
            if bytes < REFRESH_BYTES_PER_FRAME {
                bytes += view.blocks.get(b).map_or(0, |x| x.text.len());
                now.push(b);
            } else {
                later.push(b);
            }
        }
        a.refresh_document(view, session, &now);
        later.sort_unstable();
        self.review.pending_blocks = later;
        if self.review.pending_blocks.is_empty() {
            self.review.order = self.reading_order();
        }
    }

    /// Candidats du document affiché, dans l'ordre de lecture ; ceux qui n'ont
    /// pas d'occurrence dans le texte (lus dans une image) à la fin.
    fn reading_order(&self) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        if let Some(v) = self.document_view() {
            for b in &v.blocks {
                for s in &b.spans {
                    if seen.insert(s.candidate) {
                        order.push(s.candidate);
                    }
                }
            }
        }
        if let Some(name) = self.review_document_name() {
            for c in self.candidates() {
                if c.files.contains(&name) && seen.insert(c.id) {
                    order.push(c.id);
                }
            }
        }
        order
    }

    /// Change de document affiché.
    pub fn select_document(&mut self, doc: usize) {
        if self.review.doc == doc {
            return;
        }
        self.review.doc = doc;
        self.review.view = None;
        self.review.pending_blocks.clear();
        self.review.visible_hint = 0;
        self.review.order.clear();
        self.review.current = None;
        self.review.cursor = None;
        self.review.selection = None;
        self.review.reveal = None;
        self.review.show_images = None;
    }

    // --- File « À décider » --------------------------------------------------

    fn by_id(&self) -> HashMap<usize, &Candidate> {
        self.candidates().iter().map(|c| (c.id, c)).collect()
    }

    /// Vrai si la valeur demande une décision qui n'a pas encore été prise.
    pub fn is_undecided(&self, id: usize) -> bool {
        !self.decided.contains(&id) && self.candidate(id).is_some_and(needs_confirmation)
    }

    /// Valeurs du dossier qui attendent encore une décision.
    pub fn undecided_count(&self) -> usize {
        self.candidates()
            .iter()
            .filter(|c| needs_confirmation(c) && !self.decided.contains(&c.id))
            .count()
    }

    /// Décisions restantes pour le document `name`.
    pub fn remaining_in(&self, name: &str) -> usize {
        self.candidates()
            .iter()
            .filter(|c| {
                needs_confirmation(c)
                    && !self.decided.contains(&c.id)
                    && c.files.iter().any(|f| f == name)
            })
            .count()
    }

    /// File « À décider » du document affiché, dans l'ordre de lecture :
    /// valeurs ambiguës ou incertaines, tranchées ou non.
    pub fn queue(&self) -> Vec<usize> {
        let map = self.by_id();
        self.review
            .order
            .iter()
            .copied()
            .filter(|id| map.get(id).is_some_and(|c| needs_confirmation(c)))
            .collect()
    }

    /// Avancement de la file du document : (décidées, total).
    pub fn queue_progress(&self) -> (usize, usize) {
        let q = self.queue();
        let done = q.iter().filter(|id| self.decided.contains(id)).count();
        (done, q.len())
    }

    /// Valeurs sûres du document (hors file), dans l'ordre de lecture : elles
    /// sont masquées d'office et repliées sous « Masquées ».
    pub fn sure_values(&self) -> Vec<usize> {
        let map = self.by_id();
        self.review
            .order
            .iter()
            .copied()
            .filter(|id| map.get(id).is_some_and(|c| !needs_confirmation(c)))
            .collect()
    }

    /// Nombre de valeurs sûres du document actuellement masquées.
    pub fn sure_masked_count(&self) -> usize {
        let map = self.by_id();
        self.sure_values()
            .iter()
            .filter(|id| map.get(id).is_some_and(|c| c.selected))
            .count()
    }

    /// Premier document, après celui affiché, qui attend encore des décisions.
    pub fn next_document_with_decisions(&self) -> Option<usize> {
        let docs = self.review_documents();
        let pos = docs.iter().position(|(i, _)| *i == self.review.doc)?;
        docs.iter()
            .cycle()
            .skip(pos + 1)
            .take(docs.len().saturating_sub(1))
            .find(|(_, name)| self.remaining_in(name) > 0)
            .map(|(i, _)| *i)
    }

    /// Fait de `id` la valeur courante ; le curseur et l'écran vont à sa
    /// première occurrence dans le document.
    pub fn set_current(&mut self, id: usize) {
        self.review.current = Some(id);
        self.review.selection = None;
        if let Some((block, span)) = self.document_view().and_then(|v| v.occurrences(id).next()) {
            self.review.cursor = Some(Occurrence {
                block,
                start: span.start,
            });
            self.review.reveal = Some(TextRange {
                block,
                start: span.start,
                end: span.end,
            });
        }
    }

    /// J (suivante) ou K (précédente) : valeur suivante ou précédente de la
    /// file, en reprenant au début ; l'écran défile jusqu'à son occurrence.
    pub fn queue_step(&mut self, forward: bool) -> Option<usize> {
        let q = self.queue();
        if q.is_empty() {
            return None;
        }
        let pos = self
            .review
            .current
            .and_then(|c| q.iter().position(|x| *x == c));
        let len = q.len();
        let next = match (pos, forward) {
            (Some(p), true) => (p + 1) % len,
            (Some(p), false) => (p + len - 1) % len,
            (None, true) => 0,
            (None, false) => len - 1,
        };
        let id = *q.get(next)?;
        self.set_current(id);
        Some(id)
    }

    /// Tranche une valeur, dans tout le dossier : masquée (`mask`) ou gardée
    /// visible. La valeur quitte la file ; la valeur courante passe à la
    /// suivante qui attend une décision. Renvoie faux si rien ne change.
    pub fn decide(&mut self, id: usize, mask: bool, session: &Session) -> bool {
        let Some(selected) = self.candidate(id).map(|c| c.selected) else {
            return false;
        };
        if selected == mask && self.decided.contains(&id) {
            return false;
        }
        self.checkpoint(if mask { Action::Mask } else { Action::Keep });
        self.decided.insert(id);
        if selected != mask {
            if let Some(a) = self.analysis.as_mut() {
                a.set_selected(id, mask);
            }
            self.refresh_preview_values(session, &[id]);
        }
        if self.review.current == Some(id) {
            self.advance_after(id);
        }
        true
    }

    /// M ou G : tranche la valeur sous le curseur du texte (`on_cursor`), sinon
    /// la valeur courante de la file. Renvoie la valeur tranchée.
    pub fn decide_current(
        &mut self,
        mask: bool,
        on_cursor: bool,
        session: &Session,
    ) -> Option<usize> {
        let id = if on_cursor {
            self.cursor_candidate().or(self.review.current)
        } else {
            self.review.current
        }?;
        self.decide(id, mask, session);
        Some(id)
    }

    /// Valeur suivante de la file qui attend une décision, après `id`.
    fn advance_after(&mut self, id: usize) {
        let q = self.queue();
        let start = q.iter().position(|x| *x == id).map_or(0, |p| p + 1);
        let len = q.len();
        let next = (0..len)
            .filter_map(|k| q.get((start + k) % len.max(1)).copied())
            .find(|x| *x != id && self.is_undecided(*x));
        match next {
            Some(n) => self.set_current(n),
            None => self.review.current = None,
        }
    }

    /// Clic sur un surlignage : masquée devient gardée et inversement, pour
    /// toutes les occurrences du dossier.
    pub fn toggle_everywhere(&mut self, id: usize, session: &Session) -> bool {
        let Some(selected) = self.candidate(id).map(|c| c.selected) else {
            return false;
        };
        self.decide(id, !selected, session)
    }

    // --- Curseur clavier dans le texte ----------------------------------------

    /// Plage sous l'occurrence `o` : (indice dans le bloc, plage).
    pub fn span_at(&self, o: Occurrence) -> Option<(usize, anon_engine::Span)> {
        let b = self.document_view()?.blocks.get(o.block)?;
        b.spans
            .iter()
            .enumerate()
            .find(|(_, s)| s.start == o.start)
            .or_else(|| {
                b.spans
                    .iter()
                    .enumerate()
                    .find(|(_, s)| s.start <= o.start && o.start < s.end)
            })
            .map(|(i, s)| (i, *s))
    }

    /// Valeur sous le curseur du texte.
    pub fn cursor_candidate(&self) -> Option<usize> {
        self.span_at(self.review.cursor?).map(|(_, s)| s.candidate)
    }

    /// Place le curseur sur une occurrence ; si sa valeur est dans la file,
    /// elle devient la valeur courante.
    pub fn set_cursor(&mut self, o: Occurrence) {
        let Some((_, span)) = self.span_at(o) else {
            return;
        };
        self.review.cursor = Some(Occurrence {
            block: o.block,
            start: span.start,
        });
        self.review.reveal = Some(TextRange {
            block: o.block,
            start: span.start,
            end: span.end,
        });
        if self.queue().contains(&span.candidate) {
            self.review.current = Some(span.candidate);
        }
    }

    /// Flèches : occurrence suivante ou précédente du document, quelle que
    /// soit la valeur. Sans curseur : la première (ou la dernière).
    pub fn move_cursor(&mut self, forward: bool) -> Option<Occurrence> {
        let all: Vec<Occurrence> = self
            .document_view()?
            .blocks
            .iter()
            .enumerate()
            .flat_map(|(bi, b)| {
                b.spans.iter().map(move |s| Occurrence {
                    block: bi,
                    start: s.start,
                })
            })
            .collect();
        let pos = self
            .review
            .cursor
            .and_then(|c| all.iter().position(|o| *o == c));
        let next = match (pos, forward) {
            (Some(p), true) => all.get(p + 1).or(all.get(p)),
            (Some(p), false) => all.get(p.saturating_sub(1)),
            (None, true) => all.first(),
            (None, false) => all.last(),
        }
        .copied()?;
        self.set_cursor(next);
        Some(next)
    }

    // --- Sélection de texte ---------------------------------------------------

    /// Texte sélectionné, sans les espaces des extrémités.
    pub fn selected_text(&self) -> Option<String> {
        let s = self.review.selection?;
        let t = self
            .document_view()?
            .blocks
            .get(s.block)?
            .text
            .get(s.start..s.end)?
            .trim();
        (!key(t).is_empty()).then(|| t.to_owned())
    }

    /// Sélectionne un passage ; le type proposé est celui d'une valeur
    /// connue de même forme, sinon « Autre ».
    pub fn select_text(&mut self, range: Option<TextRange>) {
        self.review.selection = range.filter(|r| r.start < r.end);
        let guess = self.selected_text().and_then(|t| {
            let k = key(&t);
            self.candidates()
                .iter()
                .find(|c| {
                    std::iter::once(&c.value)
                        .chain(c.variants.iter())
                        .any(|v| key(v) == k)
                })
                .map(|c| c.kind)
        });
        self.review.selection_kind = guess.unwrap_or(EntityKind::Autre);
    }

    /// Maj+→ (Maj+←) : étend la sélection d'un mot vers la droite (la
    /// gauche). Sans sélection, elle part de l'occurrence sous le curseur.
    pub fn extend_selection(&mut self, forward: bool) {
        let start = match self.review.selection {
            Some(s) => Some(s),
            None => self.review.cursor.and_then(|c| {
                self.span_at(c).map(|(_, s)| TextRange {
                    block: c.block,
                    start: s.start,
                    end: s.end,
                })
            }),
        };
        let Some(mut r) = start else {
            return;
        };
        let Some(text) = self
            .document_view()
            .and_then(|v| v.blocks.get(r.block))
            .map(|b| b.text.clone())
        else {
            return;
        };
        if self.review.selection.is_some() {
            if forward {
                r.end = word_edge(&text, r.end, true);
            } else {
                r.start = word_edge(&text, r.start, false);
            }
        }
        self.select_text(Some(r));
        self.review.reveal = Some(r);
    }

    /// « Masquer partout » (ou « Toujours masquer » avec `always`) : le texte
    /// sélectionné devient une valeur masquée dans tout le dossier ; avec
    /// `always`, il rejoint aussi la liste cherchée à chaque analyse.
    /// Renvoie l'identifiant de la valeur.
    pub fn mask_selection(&mut self, always: bool, session: &Session) -> Option<usize> {
        let text = self.selected_text()?;
        self.analysis.as_ref()?;
        let kind = self.review.selection_kind;
        self.checkpoint(if always {
            Action::AlwaysMask
        } else {
            Action::MaskEverywhere
        });
        if always
            && !self
                .always
                .iter()
                .any(|(t, k)| *k == kind && key(t) == key(&text))
        {
            self.always.push((text.clone(), kind));
        }
        let id = self
            .analysis
            .as_mut()
            .and_then(|a| a.add_manual(kind, &text));
        match id {
            Some(id) => {
                self.decided.insert(id);
                self.refresh_preview_values(session, &[id]);
            }
            None => self.refresh_preview(session),
        }
        self.review.selection = None;
        id
    }

    // --- Recherche dans le document ---------------------------------------------

    /// Occurrence suivante de `needle` dans le document (sans accents ni
    /// majuscules), après `after`, en reprenant au début. Elle devient la
    /// sélection et l'écran y défile.
    pub fn find_next(&mut self, needle: &str, after: Option<TextRange>) -> Option<TextRange> {
        let n = fold(needle).text.trim().to_owned();
        if n.is_empty() {
            return None;
        }
        let blocks = &self.document_view()?.blocks;
        let mut found = Vec::new();
        for (bi, b) in blocks.iter().enumerate() {
            let f = fold(&b.text);
            for (i, _) in f.text.match_indices(n.as_str()) {
                if let Some(r) = f.origin_range(&b.text, i, i + n.len()) {
                    found.push(TextRange {
                        block: bi,
                        start: r.start,
                        end: r.end,
                    });
                }
            }
        }
        let next = match after {
            Some(a) => found
                .iter()
                .find(|r| (r.block, r.start) > (a.block, a.start))
                .or(found.first()),
            None => found.first(),
        }
        .copied()?;
        self.select_text(Some(next));
        self.review.reveal = Some(next);
        Some(next)
    }

    /// Extrait du document autour de la première occurrence de `id` :
    /// (avant, valeur, après), tronqués à [`EXCERPT_CHARS`] caractères.
    pub fn excerpt(&self, id: usize) -> Option<(String, String, String)> {
        let v = self.document_view()?;
        let (bi, s) = v.occurrences(id).next()?;
        let text = &v.blocks.get(bi)?.text;
        let before = text.get(..s.start)?;
        let value = text.get(s.start..s.end)?;
        let after = text.get(s.end..)?;
        let skip = before.chars().count().saturating_sub(EXCERPT_CHARS);
        let mut b: String = before.chars().skip(skip).collect();
        if skip > 0 {
            b.insert(0, '…');
        }
        let mut a: String = after.chars().take(EXCERPT_CHARS).collect();
        if after.chars().count() > EXCERPT_CHARS {
            a.push('…');
        }
        Some((b, value.to_owned(), a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::anonymize::tests::engine;
    use crate::ui::state::files::FileEntry;
    use anon_engine::{Engine, InputFile, NoProgress};
    use std::path::PathBuf;

    const COURRIER: &str = "Rose rencontre Pierre Garnier à Nouméa.\n\
        Le gérant, Pierre Garnier, signe avec Rose.\n\
        Contact : pierre.garnier@example.com, Projet Corail.\n\
        Le gérant repart.";

    /// Dossier d'un document dont les valeurs sont posées à la main : « Rose »
    /// et « gérant » demandent une décision (ambiguës), les autres sont sûres.
    fn folder(texts: &[(&str, &str)]) -> (AnonymizeState, Session) {
        let engine = Engine::standard();
        let mut s = AnonymizeState::default();
        s.add_files(
            texts
                .iter()
                .map(|(n, _)| FileEntry::new(PathBuf::from("C:/d").join(n), Some(1))),
            &["txt"],
        );
        let _ = s.begin_analysis();
        let inputs: Vec<InputFile> = texts
            .iter()
            .map(|(n, t)| InputFile::new(*n, t.as_bytes().to_vec()))
            .collect();
        let session = Session::new();
        let mut a = engine.analyze(&inputs, &s.analyze_options(), &session, &NoProgress);
        a.candidates.clear();
        for (k, v) in [
            (EntityKind::Prenom, "Rose"),
            (EntityKind::Personne, "Pierre Garnier"),
            (EntityKind::Lieu, "Nouméa"),
            (EntityKind::Fonction, "gérant"),
            (EntityKind::Email, "pierre.garnier@example.com"),
        ] {
            a.add_manual(k, v);
        }
        for c in &mut a.candidates {
            c.source = anon_core::Source::Dictionary;
            c.ambiguous = matches!(c.value.as_str(), "Rose" | "gérant");
            // « gérant » : détection incertaine, gardée visible d'office.
            if c.value == "gérant" {
                c.confidence = 0.4;
                c.selected = false;
            }
        }
        assert!(s.analysis_done(inputs, Vec::new(), a, &session));
        s.sync_review(&session);
        (s, session)
    }

    fn id(s: &AnonymizeState, v: &str) -> usize {
        s.candidates()
            .iter()
            .find(|c| c.value == v)
            .map(|c| c.id)
            .unwrap_or(usize::MAX)
    }

    fn masked(s: &AnonymizeState, v: &str) -> bool {
        s.candidate(id(s, v)).is_some_and(|c| c.selected)
    }

    #[test]
    fn queue_lists_values_to_decide_in_reading_order() {
        let (s, _) = folder(&[("courrier.txt", COURRIER)]);
        assert_eq!(s.queue(), [id(&s, "Rose"), id(&s, "gérant")]);
        assert_eq!(s.review.current, Some(id(&s, "Rose")));
        assert_eq!(s.queue_progress(), (0, 2));
        assert_eq!(s.undecided_count(), 2);
        assert_eq!(s.remaining_in("courrier.txt"), 2);
        // Valeurs sûres, repliées sous « Masquées ».
        assert_eq!(
            s.sure_values(),
            [
                id(&s, "Pierre Garnier"),
                id(&s, "Nouméa"),
                id(&s, "pierre.garnier@example.com")
            ]
        );
        assert_eq!(s.sure_masked_count(), 3);
        assert!(
            s.review_summary().ends_with("· 2 à décider"),
            "{}",
            s.review_summary()
        );
    }

    #[test]
    fn m_and_g_decide_then_move_to_the_next_value() {
        let (mut s, session) = folder(&[("courrier.txt", COURRIER)]);
        let (rose, gerant) = (id(&s, "Rose"), id(&s, "gérant"));
        // M sur « Rose » (déjà masquée d'office) : décidée, la file avance.
        assert_eq!(s.decide_current(true, false, &session), Some(rose));
        s.sync_review(&session);
        assert!(masked(&s, "Rose"));
        assert_eq!(s.review.current, Some(gerant));
        assert_eq!(s.queue_progress(), (1, 2));
        // L'écran défile jusqu'à « gérant ».
        let r = s.review.reveal.unwrap();
        let text = &s.document_view().unwrap().blocks[r.block].text;
        assert_eq!(&text[r.start..r.end], "gérant");
        // G : gardée visible, plus rien à décider.
        assert_eq!(s.decide_current(false, false, &session), Some(gerant));
        s.sync_review(&session);
        assert!(!masked(&s, "gérant"));
        assert_eq!(s.review.current, None);
        assert_eq!(s.undecided_count(), 0);
        assert_eq!(s.remaining_in("courrier.txt"), 0);
        // Rien ne change : rien n'est retenu.
        let n = s.history.len();
        assert!(!s.decide(gerant, false, &session));
        assert_eq!(s.history.len(), n);
    }

    #[test]
    fn j_and_k_walk_the_queue_and_wrap() {
        let (mut s, _) = folder(&[("courrier.txt", COURRIER)]);
        let (rose, gerant) = (id(&s, "Rose"), id(&s, "gérant"));
        assert_eq!(s.queue_step(true), Some(gerant));
        assert_eq!(s.queue_step(true), Some(rose), "reprise au début");
        assert_eq!(s.queue_step(false), Some(gerant), "K revient en arrière");
        let c = s.review.cursor.unwrap();
        assert_eq!(s.cursor_candidate(), Some(gerant));
        assert_eq!(c.block, 1);
    }

    #[test]
    fn decisions_are_undone_and_the_value_returns_to_the_queue() {
        let (mut s, session) = folder(&[("courrier.txt", COURRIER)]);
        let gerant = id(&s, "gérant");
        s.queue_step(true);
        assert!(s.decide(gerant, true, &session));
        s.sync_review(&session);
        assert!(masked(&s, "gérant"));
        assert_eq!(s.history.next_undo(), Some(Action::Mask));
        assert_eq!(s.undo(engine(), &session), Some(Action::Mask));
        s.sync_review(&session);
        assert!(!masked(&s, "gérant"));
        assert!(s.is_undecided(gerant));
        assert_eq!(s.redo(engine(), &session), Some(Action::Mask));
        assert!(!s.is_undecided(gerant));
    }

    #[test]
    fn click_toggles_every_occurrence_of_the_folder() {
        let (mut s, session) = folder(&[
            ("courrier.txt", COURRIER),
            ("note.txt", "Pierre Garnier passe lundi."),
        ]);
        let pg = id(&s, "Pierre Garnier");
        assert!(s.toggle_everywhere(pg, &session));
        s.sync_review(&session);
        let view = s.document_view().unwrap();
        assert_eq!(view.occurrences(pg).count(), 2);
        assert!(view.occurrences(pg).all(|(_, sp)| !sp.masked));
        // Seuls les blocs touchés sont recalculés : même résultat qu'une vue neuve.
        let fresh = s.analysis.as_ref().and_then(|a| a.document(0, &session));
        assert_eq!(s.document_view(), fresh.as_ref());
        s.select_document(1);
        s.sync_review(&session);
        let view = s.document_view().unwrap();
        assert_eq!(view.name, "note.txt");
        assert!(
            view.occurrences(pg).all(|(_, sp)| !sp.masked),
            "tout le dossier"
        );
        assert_eq!(s.history.next_undo(), Some(Action::Keep));
        s.undo(engine(), &session);
        s.sync_review(&session);
        assert!(
            s.document_view()
                .unwrap()
                .occurrences(pg)
                .all(|(_, sp)| sp.masked)
        );
        let fresh = s.analysis.as_ref().and_then(|a| a.document(1, &session));
        assert_eq!(s.document_view(), fresh.as_ref(), "annulation : vue à jour");
    }

    #[test]
    fn documents_count_their_remaining_decisions() {
        let (mut s, session) = folder(&[
            ("courrier.txt", COURRIER),
            ("note.txt", "Rose passe lundi."),
        ]);
        assert_eq!(s.remaining_in("courrier.txt"), 2);
        assert_eq!(s.remaining_in("note.txt"), 1);
        assert_eq!(s.next_document_with_decisions(), Some(1));
        s.decide(id(&s, "Rose"), true, &session);
        assert_eq!(s.remaining_in("note.txt"), 0);
        assert_eq!(s.next_document_with_decisions(), None);
    }

    #[test]
    fn selection_masked_everywhere_then_undone() {
        let (mut s, session) = folder(&[
            ("courrier.txt", COURRIER),
            ("note.txt", "Le Projet Corail avance."),
        ]);
        let block = 2;
        let text = s.document_view().unwrap().blocks[block].text.clone();
        let start = text.find("Projet Corail").unwrap();
        s.select_text(Some(TextRange {
            block,
            start,
            end: start + "Projet Corail".len(),
        }));
        assert_eq!(s.selected_text().as_deref(), Some("Projet Corail"));
        assert_eq!(s.review.selection_kind, EntityKind::Autre);
        let added = s.mask_selection(false, &session).unwrap();
        s.sync_review(&session);
        assert!(s.review.selection.is_none());
        assert!(s.candidate(added).is_some_and(|c| c.selected));
        assert!(!s.is_undecided(added));
        assert_eq!(
            s.candidate(added).map(|c| c.occurrences),
            Some(2),
            "partout"
        );
        assert!(s.always.is_empty());
        assert_eq!(s.undo(engine(), &session), Some(Action::MaskEverywhere));
        assert!(s.candidate(added).is_none());
    }

    #[test]
    fn always_mask_joins_the_list_and_is_undone() {
        let (mut s, session) = folder(&[("courrier.txt", COURRIER)]);
        let text = s.document_view().unwrap().blocks[2].text.clone();
        let start = text.find("Corail").unwrap();
        s.select_text(Some(TextRange {
            block: 2,
            start,
            end: start + "Corail".len(),
        }));
        s.review.selection_kind = EntityKind::Societe;
        let added = s.mask_selection(true, &session).unwrap();
        assert_eq!(s.always, vec![("Corail".to_owned(), EntityKind::Societe)]);
        assert!(s.candidate(added).is_some_and(|c| c.selected));
        assert_eq!(s.undo(engine(), &session), Some(Action::AlwaysMask));
        assert!(s.always.is_empty(), "la liste revient aussi");
        // Une sélection vide ou faite d'espaces ne masque rien.
        s.select_text(Some(TextRange {
            block: 2,
            start: 7,
            end: 8,
        }));
        let n = s.history.len();
        assert_eq!(s.mask_selection(false, &session), None);
        assert_eq!(s.history.len(), n);
    }

    #[test]
    fn selecting_a_known_value_proposes_its_type() {
        let (mut s, _) = folder(&[("courrier.txt", COURRIER)]);
        let text = s.document_view().unwrap().blocks[0].text.clone();
        let start = text.find("Nouméa").unwrap();
        s.select_text(Some(TextRange {
            block: 0,
            start,
            end: start + "Nouméa".len(),
        }));
        assert_eq!(s.review.selection_kind, EntityKind::Lieu);
    }

    #[test]
    fn keyboard_cursor_and_word_selection() {
        let (mut s, _) = folder(&[("courrier.txt", COURRIER)]);
        // Flèches : chaque occurrence, dans l'ordre du texte.
        let first = s.move_cursor(true).unwrap();
        assert_eq!((first.block, first.start), (0, 0));
        assert_eq!(s.cursor_candidate(), Some(id(&s, "Rose")));
        let second = s.move_cursor(true).unwrap();
        assert_eq!(s.cursor_candidate(), Some(id(&s, "Pierre Garnier")));
        assert_eq!(s.move_cursor(false), Some(first));
        assert_eq!(s.move_cursor(false), Some(first), "pas avant la première");
        s.set_cursor(second);
        // Maj+→ : la sélection part de la valeur et s'étend d'un mot.
        s.extend_selection(true);
        assert_eq!(s.selected_text().as_deref(), Some("Pierre Garnier"));
        s.extend_selection(true);
        assert_eq!(s.selected_text().as_deref(), Some("Pierre Garnier à"));
        s.extend_selection(false);
        assert_eq!(
            s.selected_text().as_deref(),
            Some("rencontre Pierre Garnier à")
        );
    }

    #[test]
    fn find_in_document_ignores_accents_and_case() {
        let (mut s, _) = folder(&[("courrier.txt", COURRIER)]);
        let r = s.find_next("GERANT", None).unwrap();
        assert_eq!(r.block, 1);
        assert_eq!(s.selected_text().as_deref(), Some("gérant"));
        let r2 = s.find_next("gerant", Some(r)).unwrap();
        assert_eq!(r2.block, 3);
        assert_eq!(s.find_next("gerant", Some(r2)), Some(r), "reprise au début");
        assert_eq!(s.find_next("  ", None), None);
        assert_eq!(s.find_next("absent", None), None);
    }

    #[test]
    fn excerpt_surrounds_the_first_occurrence() {
        let (s, _) = folder(&[("courrier.txt", COURRIER)]);
        let (before, value, after) = s.excerpt(id(&s, "gérant")).unwrap();
        assert_eq!(before, "Le ");
        assert_eq!(value, "gérant");
        assert_eq!(after, ", Pierre Garnier, signe avec Rose.");
        assert_eq!(word_edge("ab cd", 0, true), 2);
        assert_eq!(word_edge("ab cd", 5, false), 3);
    }

    #[test]
    fn a_new_analysis_keeps_the_chosen_view_only() {
        let (mut s, session) = folder(&[("courrier.txt", COURRIER)]);
        s.review.mode = VerifyMode::Table;
        s.decide(id(&s, "Rose"), true, &session);
        s.review.reset();
        assert_eq!(s.review.mode, VerifyMode::Table);
        assert!(s.review.current.is_none() && s.document_view().is_none());
        assert!(!format!("{:?}", s.review).contains("Rose"));
    }
}
