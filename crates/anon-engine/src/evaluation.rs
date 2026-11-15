//! Évaluation des trois critères dans le moteur : construction de l'entrée
//! d'`anon-assess` à partir de l'analyse, valeurs des cellules de tableau,
//! corrections et leur annulation, techniques appliquées (fiche d'évaluation).
//!
//! - **Textes** : un [`TextUnit`] par bloc gardé en mémoire (et par image lue
//!   par OCR) ; les mentions sont trouvées par les mêmes `Replacer` que
//!   l'anonymisation : d'abord les valeurs cochées (balise prévisionnelle ou
//!   forme généralisée), puis, dans le reste du texte, les valeurs décochées
//!   (visibles). Les cellules d'une colonne de tableau évaluée n'y sont pas :
//!   le tableau les évalue.
//! - **Tableaux** : rôle de chaque colonne d'après l'en-tête (colonnes
//!   d'`anon-detect`, puis [`EVALUATION_HEADERS`]), à défaut d'après le type des
//!   valeurs trouvées ; chaque cellule non vide d'une colonne évaluée a une
//!   valeur dans l'analyse (ajoutée décochée à l'analyse au besoin).
//! - **Images** : visages trouvés par le détecteur injecté, moins ceux que
//!   couvre une zone masquée ; `None` sans détecteur (édition Lite).

use crate::analysis::{Analysis, Candidate, ImageMask, SuppressedRows};
use crate::mentions::{MaskedFinder, ValueFinder};
use anon_core::{
    AssessInput, BlockContext, BlockRole, ColumnRole, EntityKind, Fix, ImageInput, Mention,
    PixelRect, Shown, TableCell, TableData, TableInput, Tag, TextTransform, TextUnit, Treatment,
};
use anon_detect::columns::kind_for_header;
use anon_dict::hierarchy::{generalize, max_level};
use anon_replace::key;
use anon_replace::session_ext::match_key;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// Part minimale des cellules non vides d'une colonne sans en-tête connu dont
/// les valeurs ont le même type, pour donner ce type à la colonne.
pub const COLUMN_KIND_MIN_SHARE: f64 = 0.5;
/// Part minimale de la surface d'un visage couverte par une zone masquée pour
/// le considérer comme masqué.
pub const FACE_COVERED_MIN_SHARE: f64 = 0.9;
/// Nombre maximal de passes de « Tout corriger » (une correction peut en
/// appeler une autre : généralisation, puis suppression des lignes restantes).
pub const FIX_ALL_MAX_ROUNDS: usize = 4;
/// Longueur minimale (caractères) d'un en-tête de [`EVALUATION_HEADERS`] pour
/// être reconnu dans un en-tête plus long (même règle qu'`anon-detect`).
const MIN_CONTAINED_HEADER_CHARS: usize = 4;

/// En-têtes propres à l'évaluation, en plus de ceux d'`anon-detect` : indices
/// et attributs que la détection ne cherche pas comme valeurs (sexe, code
/// postal, profession, diagnostic, numéro de dossier…). Clés pliées.
pub const EVALUATION_HEADERS: &[(&str, ColumnRole)] = &[
    ("sexe", ColumnRole::Clue(EntityKind::Autre)),
    ("genre", ColumnRole::Clue(EntityKind::Autre)),
    ("sex", ColumnRole::Clue(EntityKind::Autre)),
    ("gender", ColumnRole::Clue(EntityKind::Autre)),
    ("code postal", ColumnRole::Clue(EntityKind::Lieu)),
    ("cp", ColumnRole::Clue(EntityKind::Lieu)),
    ("postal code", ColumnRole::Clue(EntityKind::Lieu)),
    ("zip", ColumnRole::Clue(EntityKind::Lieu)),
    ("departement", ColumnRole::Clue(EntityKind::Lieu)),
    ("province", ColumnRole::Clue(EntityKind::Lieu)),
    ("region", ColumnRole::Clue(EntityKind::Lieu)),
    ("pays", ColumnRole::Clue(EntityKind::Lieu)),
    (
        "annee de naissance",
        ColumnRole::Clue(EntityKind::DateNaiss),
    ),
    ("naissance", ColumnRole::Clue(EntityKind::DateNaiss)),
    ("profession", ColumnRole::Clue(EntityKind::Fonction)),
    ("metier", ColumnRole::Clue(EntityKind::Fonction)),
    ("emploi", ColumnRole::Clue(EntityKind::Fonction)),
    ("occupation", ColumnRole::Clue(EntityKind::Fonction)),
    ("diagnostic", ColumnRole::Sensitive(EntityKind::Sante)),
    ("diagnosis", ColumnRole::Sensitive(EntityKind::Sante)),
    ("pathologie", ColumnRole::Sensitive(EntityKind::Sante)),
    ("maladie", ColumnRole::Sensitive(EntityKind::Sante)),
    ("handicap", ColumnRole::Sensitive(EntityKind::Sante)),
    (
        "traitement medical",
        ColumnRole::Sensitive(EntityKind::Sante),
    ),
    ("religion", ColumnRole::Sensitive(EntityKind::Religion)),
    ("syndicat", ColumnRole::Sensitive(EntityKind::Syndicat)),
    (
        "orientation sexuelle",
        ColumnRole::Sensitive(EntityKind::Orientation),
    ),
    (
        "origine ethnique",
        ColumnRole::Sensitive(EntityKind::Origine),
    ),
    (
        "opinion politique",
        ColumnRole::Sensitive(EntityKind::Opinion),
    ),
    (
        "condamnation",
        ColumnRole::Sensitive(EntityKind::Judiciaire),
    ),
    (
        "casier judiciaire",
        ColumnRole::Sensitive(EntityKind::Judiciaire),
    ),
    ("numero client", ColumnRole::Stable(EntityKind::Identifiant)),
    ("n client", ColumnRole::Stable(EntityKind::Identifiant)),
    ("id client", ColumnRole::Stable(EntityKind::Identifiant)),
    ("identifiant", ColumnRole::Stable(EntityKind::Identifiant)),
    (
        "numero de dossier",
        ColumnRole::Stable(EntityKind::Identifiant),
    ),
    ("n dossier", ColumnRole::Stable(EntityKind::Identifiant)),
    ("dossier", ColumnRole::Stable(EntityKind::Identifiant)),
    ("customer id", ColumnRole::Stable(EntityKind::Identifiant)),
];

/// Rôle d'une colonne dont les valeurs sont de ce type.
pub fn role_for_kind(kind: EntityKind) -> ColumnRole {
    use EntityKind::*;
    match kind {
        Personne | Nom | Prenom | Email | Tel | Adresse | Iban | Nir | Cb | Ip | Url | Plaque => {
            ColumnRole::Direct(kind)
        }
        Matricule | Identifiant | Siret | Ridet => ColumnRole::Stable(kind),
        Evenement => ColumnRole::Sensitive(kind),
        k if k.is_sensitive() => ColumnRole::Sensitive(k),
        _ => ColumnRole::Clue(kind),
    }
}

fn role_kind(role: ColumnRole) -> Option<EntityKind> {
    match role {
        ColumnRole::Direct(k)
        | ColumnRole::Stable(k)
        | ColumnRole::Clue(k)
        | ColumnRole::Sensitive(k) => Some(k),
        ColumnRole::Other => None,
    }
}

fn header_tokens(s: &str) -> Vec<String> {
    key(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Rôle d'après l'en-tête : colonnes d'`anon-detect`, puis [`EVALUATION_HEADERS`]
/// (correspondance exacte, sinon la clé la plus longue contenue).
fn header_role(header: &str) -> Option<ColumnRole> {
    if let Some(kind) = kind_for_header(header) {
        return Some(role_for_kind(kind));
    }
    let h = header_tokens(header);
    if h.is_empty() {
        return None;
    }
    let mut best: Option<(usize, ColumnRole)> = None;
    for (k, role) in EVALUATION_HEADERS {
        let k = header_tokens(k);
        let chars: usize = k.iter().map(|t| t.chars().count()).sum();
        let hit = k == h
            || (chars >= MIN_CONTAINED_HEADER_CHARS
                && !k.is_empty()
                && h.windows(k.len()).any(|w| w == k.as_slice()));
        if hit && best.is_none_or(|(n, _)| chars > n) {
            best = Some((chars, *role));
        }
    }
    best.map(|(_, r)| r)
}

/// Toutes les valeurs de l'analyse (module `mentions`).
fn lookup(a: &Analysis, keep: impl Fn(&Candidate) -> bool) -> ValueFinder {
    ValueFinder::new(a, keep)
}

/// Ce que montrera le document produit : valeurs cochées sur la session de
/// départ de l'analyse (module `mentions`).
fn preview(a: &Analysis) -> MaskedFinder {
    MaskedFinder::new(a, &a.base)
}

impl MaskedFinder {
    fn shown(&self, tag: Tag) -> Shown {
        match self.treatment(tag) {
            Some(Treatment::Generalize(g)) => Shown::Generalized(g.clone()),
            _ => Shown::Masked(tag),
        }
    }

    /// Forme affichée d'une cellule entière.
    fn cell(&self, text: &str) -> Shown {
        let found = self.matches(text);
        match found.as_slice() {
            [] => Shown::Visible,
            [m] if text.get(m.start..m.end) == Some(text.trim()) => self.shown(m.tag),
            _ => {
                let ctx = BlockContext::new("cellule", BlockRole::Cell);
                Shown::Generalized(self.replacer.apply(text, &ctx).into_owned())
            }
        }
    }
}

/// Index (type, clé) → valeur, formes et variantes comprises.
fn value_index(a: &Analysis) -> HashMap<(EntityKind, String), usize> {
    let mut out = HashMap::new();
    for c in &a.candidates {
        for v in std::iter::once(&c.value).chain(c.variants.iter()) {
            out.entry((c.kind, match_key(c.kind, v))).or_insert(c.id);
        }
    }
    out
}

/// Valeur de l'analyse qui correspond à une cellule entière : même type et
/// même valeur, sinon une seule valeur qui couvre toute la cellule.
fn cell_value(
    text: &str,
    kind: Option<EntityKind>,
    all: &ValueFinder,
    index: &HashMap<(EntityKind, String), usize>,
) -> Option<usize> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(id) = kind.and_then(|k| index.get(&(k, match_key(k, trimmed)))) {
        return Some(*id);
    }
    match all.find(text).as_slice() {
        [f] if text.get(f.start..f.end) == Some(trimmed) => Some(f.id),
        _ => None,
    }
}

/// Rôles des colonnes d'un tableau : en-tête, sinon type majoritaire des
/// valeurs déjà connues de l'analyse.
fn roles(
    t: &TableData,
    a: &Analysis,
    all: &ValueFinder,
    index: &HashMap<(EntityKind, String), usize>,
) -> Vec<ColumnRole> {
    t.headers
        .iter()
        .enumerate()
        .map(|(col, header)| {
            if let Some(role) = header_role(header) {
                return role;
            }
            let mut kinds: BTreeMap<EntityKind, usize> = BTreeMap::new();
            let mut filled = 0usize;
            for cell in t.rows.iter().filter_map(|r| r.get(col)) {
                if cell.trim().is_empty() {
                    continue;
                }
                filled = filled.saturating_add(1);
                if let Some(c) = cell_value(cell, None, all, index).and_then(|id| a.candidate(id)) {
                    let n = kinds.entry(c.kind).or_insert(0);
                    *n = n.saturating_add(1);
                }
            }
            match kinds.into_iter().max_by_key(|(_, n)| *n) {
                Some((kind, n))
                    if filled > 0 && n as f64 >= COLUMN_KIND_MIN_SHARE * filled as f64 =>
                {
                    role_for_kind(kind)
                }
                _ => ColumnRole::Other,
            }
        })
        .collect()
}

/// Ajoute à l'analyse, décochée, la valeur de chaque cellule non vide d'une
/// colonne évaluée qui n'y figure pas encore (exigence d'`anon-assess` : sans
/// valeur, un échec ne peut pas être corrigé). Rien n'est coché ni décoché.
pub(crate) fn register_table_values(a: &mut Analysis) {
    if a.store.iter().all(|f| f.tables.is_empty()) {
        return;
    }
    let all = lookup(a, |_| true);
    let index = value_index(a);
    let never: HashSet<String> = a.never.iter().map(|n| key(n)).collect();
    let mut seen: HashSet<(EntityKind, String)> = HashSet::new();
    let mut to_add: Vec<(EntityKind, String, usize)> = Vec::new();
    for (fi, fd) in a.store.iter().enumerate() {
        for t in &fd.tables {
            let roles = roles(t, a, &all, &index);
            for row in &t.rows {
                for (col, cell) in row.iter().enumerate() {
                    let Some(kind) = roles.get(col).copied().and_then(role_kind) else {
                        continue;
                    };
                    let trimmed = cell.trim();
                    if trimmed.is_empty()
                        || never.contains(&key(trimmed))
                        || cell_value(cell, Some(kind), &all, &index).is_some()
                    {
                        continue;
                    }
                    if seen.insert((kind, match_key(kind, trimmed))) {
                        to_add.push((kind, trimmed.to_string(), fi));
                    }
                }
            }
        }
    }
    if to_add.is_empty() {
        return;
    }
    for (kind, value, file) in to_add {
        a.push_table_value(kind, &value, file);
    }
    a.recount();
}

/// Place d'un tableau de l'entrée d'évaluation dans l'analyse.
struct TableLayout {
    file: usize,
    sheet: usize,
    /// Lignes de `TableData::rows` gardées (hors lignes supprimées), dans l'ordre.
    rows: Vec<usize>,
}

/// Tableaux évalués (au moins une colonne évaluée), dans l'ordre de l'entrée
/// d'évaluation, avec leurs rôles.
fn tables(a: &Analysis) -> Vec<(TableLayout, Vec<ColumnRole>)> {
    let all = lookup(a, |_| true);
    let index = value_index(a);
    let mut out = Vec::new();
    for (fi, fd) in a.store.iter().enumerate() {
        for (si, t) in fd.tables.iter().enumerate() {
            let roles = roles(t, a, &all, &index);
            if roles.iter().all(|r| *r == ColumnRole::Other) {
                continue;
            }
            let gone: HashSet<usize> = a
                .suppressed
                .iter()
                .filter(|s| s.file == fi && s.sheet == si)
                .flat_map(|s| s.rows.iter().copied())
                .collect();
            let rows = (0..t.rows.len()).filter(|r| !gone.contains(r)).collect();
            out.push((
                TableLayout {
                    file: fi,
                    sheet: si,
                    rows,
                },
                roles,
            ));
        }
    }
    out
}

/// Fraction de la surface de `face` couverte par `mask`.
fn covered(face: &PixelRect, mask: &PixelRect) -> f64 {
    let x0 = face.x.max(mask.x);
    let y0 = face.y.max(mask.y);
    let x1 = face
        .x
        .saturating_add(face.w)
        .min(mask.x.saturating_add(mask.w));
    let y1 = face
        .y
        .saturating_add(face.h)
        .min(mask.y.saturating_add(mask.h));
    let area = u64::from(face.w) * u64::from(face.h);
    if x1 <= x0 || y1 <= y0 || area == 0 {
        return 0.0;
    }
    (u64::from(x1 - x0) * u64::from(y1 - y0)) as f64 / area as f64
}

/// Entrée de l'évaluation. `masks_apply` : les zones masquées seront bien
/// appliquées à l'anonymisation (traitement d'images présent).
pub(crate) fn build_input(a: &Analysis, masks_apply: bool) -> AssessInput {
    let prev = preview(a);
    let all = lookup(a, |_| true);
    let index = value_index(a);
    let visible = lookup(a, |c| !c.selected && !a.table_values.contains(&c.id));

    // Tableaux.
    let mut tables_in = Vec::new();
    let mut evaluated: HashMap<usize, HashSet<String>> = HashMap::new();
    for (layout, roles) in tables(a) {
        let Some(t) = a
            .store
            .get(layout.file)
            .and_then(|f| f.tables.get(layout.sheet))
        else {
            continue;
        };
        let headers = evaluated.entry(layout.file).or_default();
        for (h, r) in t.headers.iter().zip(&roles) {
            if *r != ColumnRole::Other {
                headers.insert(h.clone());
            }
        }
        let rows = layout
            .rows
            .iter()
            .filter_map(|r| t.rows.get(*r))
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(col, cell)| {
                        let kind = roles.get(col).copied().and_then(role_kind);
                        TableCell {
                            value: cell.clone(),
                            value_id: cell_value(cell, kind, &all, &index),
                            shown: prev.cell(cell),
                        }
                    })
                    .collect()
            })
            .collect();
        tables_in.push(TableInput {
            file: layout.file,
            sheet: t.sheet.clone(),
            headers: t.headers.clone(),
            roles,
            rows,
        });
    }

    // Textes.
    let mut texts = Vec::new();
    for (fi, fd) in a.store.iter().enumerate() {
        let skip = evaluated.get(&fi);
        let blocks = fd
            .blocks
            .iter()
            .filter(|b| b.ctx.role != BlockRole::Metadata)
            .filter(|b| {
                !(b.ctx.role == BlockRole::Cell
                    && b.ctx
                        .column_header
                        .as_ref()
                        .is_some_and(|h| skip.is_some_and(|s| s.contains(h))))
            })
            .map(|b| b.text.clone());
        let ocr = fd.images.iter().map(|i| i.ocr.text_with_spans().0);
        for text in blocks.chain(ocr) {
            let mentions = mentions(a, &prev, &visible, &text);
            if !mentions.is_empty() {
                texts.push(TextUnit { file: fi, mentions });
            }
        }
    }

    // Images.
    let mut images = Vec::new();
    for (fi, fd) in a.store.iter().enumerate() {
        let masks: Vec<(&str, PixelRect)> = a
            .masks()
            .iter()
            .filter(|m| m.file == fi)
            .map(|m| (m.media.as_str(), m.rect))
            .collect();
        let unmasked = |media: &str, faces: &Option<Vec<PixelRect>>| {
            faces.as_ref().map(|faces| {
                faces
                    .iter()
                    .filter(|f| {
                        !masks_apply
                            || !masks.iter().any(|(m, r)| {
                                *m == media && covered(f, r) >= FACE_COVERED_MIN_SHARE
                            })
                    })
                    .copied()
                    .collect()
            })
        };
        for img in &fd.images {
            images.push(ImageInput {
                file: fi,
                media: img.name.clone(),
                faces: unmasked(&img.name, &img.faces),
            });
        }
        for u in &fd.unread_media {
            images.push(ImageInput {
                file: fi,
                media: u.name.clone(),
                faces: unmasked(&u.name, &u.faces),
            });
        }
    }

    AssessInput {
        texts,
        tables: tables_in,
        images,
    }
}

/// Mentions d'un texte, dans l'ordre : valeurs cochées telles que les
/// remplacera l'anonymisation, puis valeurs décochées (visibles) ailleurs.
fn mentions(a: &Analysis, prev: &MaskedFinder, visible: &ValueFinder, text: &str) -> Vec<Mention> {
    let masked = prev.matches(text);
    let mut out: Vec<(usize, Mention)> = Vec::new();
    for m in &masked {
        if let Some(c) = prev.owner(m.tag).and_then(|id| a.candidate(id)) {
            out.push((
                m.start,
                Mention {
                    value_id: c.id,
                    kind: c.kind,
                    value: c.value.clone(),
                    shown: prev.shown(m.tag),
                },
            ));
        }
    }
    for m in visible.find(text) {
        if masked.iter().any(|s| s.start < m.end && m.start < s.end) {
            continue;
        }
        if let Some(c) = a.candidate(m.id) {
            out.push((
                m.start,
                Mention {
                    value_id: c.id,
                    kind: c.kind,
                    value: c.value.clone(),
                    shown: Shown::Visible,
                },
            ));
        }
    }
    out.sort_by_key(|(start, _)| *start);
    out.into_iter().map(|(_, m)| m).collect()
}

// ---------------------------------------------------------------------------
// Corrections
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum UndoStep {
    /// État d'une valeur à remettre.
    Value {
        id: usize,
        selected: bool,
        treatment: Treatment,
    },
    /// Zone masquée ajoutée : à retirer.
    Mask { id: usize },
    /// Zone masquée retirée par une annulation : à remettre (rétablir).
    MaskBack(ImageMask),
    /// Lignes supprimées ajoutées : à retirer.
    Rows { id: usize },
    /// Lignes supprimées retirées par une annulation : à remettre (rétablir).
    RowsBack(SuppressedRows),
}

/// De quoi annuler une ou plusieurs corrections ([`crate::Engine::undo_fix`]),
/// ou, après une annulation, de quoi les rétablir.
#[derive(Clone, PartialEq, Default)]
pub struct FixUndo {
    steps: Vec<UndoStep>,
}

impl fmt::Debug for FixUndo {
    /// N'affiche que le nombre d'étapes (une forme généralisée dérive d'une valeur).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixUndo")
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl FixUndo {
    /// Vrai si la correction n'a rien changé.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Ajoute les étapes d'une correction appliquée après celles-ci.
    pub fn extend(&mut self, later: FixUndo) {
        self.steps.extend(later.steps);
    }

    /// Coche une valeur avec ce traitement, en gardant son état d'avant (une
    /// seule fois par valeur).
    fn set(&mut self, a: &mut Analysis, id: usize, treatment: Treatment) {
        let Some(c) = a.candidates.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if c.selected && c.treatment == treatment {
            return;
        }
        let known = self
            .steps
            .iter()
            .any(|s| matches!(s, UndoStep::Value { id: i, .. } if *i == id));
        if !known {
            self.steps.push(UndoStep::Value {
                id,
                selected: c.selected,
                treatment: c.treatment.clone(),
            });
        }
        c.selected = true;
        c.treatment = treatment;
    }
}

/// Applique une correction de l'évaluation.
pub(crate) fn apply_fix(a: &mut Analysis, fix: &Fix) -> FixUndo {
    let mut undo = FixUndo::default();
    match fix {
        Fix::Mask { value_id } => undo.set(a, *value_id, Treatment::Tag),
        Fix::Generalize { value_id, to } => {
            undo.set(a, *value_id, Treatment::Generalize(to.clone()));
        }
        Fix::GeneralizeMany { items } => {
            for (id, to) in items {
                undo.set(a, *id, Treatment::Generalize(to.clone()));
            }
        }
        Fix::SuppressRows {
            table,
            rows,
            value_ids,
        } => {
            for id in value_ids {
                undo.set(a, *id, Treatment::Tag);
            }
            let layouts = tables(a);
            if let Some((layout, _)) = layouts.get(*table) {
                let original: Vec<usize> = rows
                    .iter()
                    .filter_map(|r| layout.rows.get(*r).copied())
                    .collect();
                if !original.is_empty() {
                    let id = a.next_suppressed_id();
                    a.suppressed.push(SuppressedRows {
                        id,
                        file: layout.file,
                        sheet: layout.sheet,
                        rows: original,
                    });
                    undo.steps.push(UndoStep::Rows { id });
                }
            }
        }
        Fix::MaskRegion { file, media, rect } => {
            if let Some(id) = a.push_mask(*file, media, *rect, true) {
                undo.steps.push(UndoStep::Mask { id });
            }
        }
    }
    undo
}

/// Annule des corrections : l'analyse revient exactement à son état d'avant.
/// Renvoie de quoi les rétablir : passé de nouveau à `undo_fix`, il redonne
/// exactement l'état d'après (et de quoi l'annuler encore).
pub(crate) fn undo_fix(a: &mut Analysis, undo: FixUndo) -> FixUndo {
    let mut back = FixUndo::default();
    for step in undo.steps.into_iter().rev() {
        match step {
            UndoStep::Value {
                id,
                selected,
                treatment,
            } => {
                if let Some(c) = a.candidates.iter_mut().find(|c| c.id == id) {
                    back.steps.push(UndoStep::Value {
                        id,
                        selected: c.selected,
                        treatment: std::mem::replace(&mut c.treatment, treatment),
                    });
                    c.selected = selected;
                }
            }
            UndoStep::Mask { id } => {
                if let Some(m) = a.take_mask(id) {
                    back.steps.push(UndoStep::MaskBack(m));
                }
            }
            UndoStep::MaskBack(m) => {
                back.steps.push(UndoStep::Mask { id: m.id });
                a.put_mask(m);
            }
            UndoStep::Rows { id } => {
                if let Some(pos) = a.suppressed.iter().position(|s| s.id == id) {
                    back.steps
                        .push(UndoStep::RowsBack(a.suppressed.remove(pos)));
                }
            }
            UndoStep::RowsBack(rows) => {
                back.steps.push(UndoStep::Rows { id: rows.id });
                let pos = a.suppressed.partition_point(|s| s.id < rows.id);
                a.suppressed.insert(pos, rows);
            }
        }
    }
    back
}

// ---------------------------------------------------------------------------
// Techniques appliquées
// ---------------------------------------------------------------------------

/// Techniques appliquées au dossier, pour la fiche d'évaluation : seulement des
/// types et des nombres.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Techniques {
    /// Valeurs remplacées par une balise, par type.
    pub masked: BTreeMap<EntityKind, usize>,
    /// Valeurs rendues moins précises, par type et niveau de hiérarchie
    /// (`anon_dict::hierarchy`) : `Some(n)` jusqu'à `max_level(type)`,
    /// `Some(max_level + 1)` pour une valeur remplacée par « * », `None` pour
    /// une forme choisie hors hiérarchie.
    pub generalized: BTreeMap<(EntityKind, Option<u8>), usize>,
    /// Lignes de tableau supprimées (indices masqués, lignes hors évaluation).
    pub suppressed_rows: usize,
    /// Zones d'image masquées à la main ou par une correction.
    pub masked_regions: usize,
}

/// Forme la plus générale d'une valeur (au-delà de sa hiérarchie).
const SUPPRESSED_FORM: &str = "*";

/// Niveau de hiérarchie d'une forme généralisée.
fn level_of(kind: EntityKind, value: &str, form: &str) -> Option<u8> {
    if form.trim() == SUPPRESSED_FORM {
        return Some(max_level(kind).saturating_add(1));
    }
    (1..=max_level(kind)).find(|l| generalize(kind, value, *l).as_deref() == Some(form))
}

impl Analysis {
    /// Techniques appliquées d'après la sélection courante.
    pub fn techniques(&self) -> Techniques {
        let mut t = Techniques::default();
        for c in self.candidates.iter().filter(|c| c.selected) {
            match &c.treatment {
                Treatment::Tag => {
                    let n = t.masked.entry(c.kind).or_insert(0);
                    *n = n.saturating_add(1);
                }
                Treatment::Generalize(g) => {
                    let n = t
                        .generalized
                        .entry((c.kind, level_of(c.kind, &c.value, g)))
                        .or_insert(0);
                    *n = n.saturating_add(1);
                }
            }
        }
        t.suppressed_rows = self.suppressed.iter().map(|s| s.rows.len()).sum();
        t.masked_regions = self.masks().len();
        t
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{AnalyzeOptions, Engine};
    use anon_core::Session;
    use proptest::prelude::*;

    #[test]
    fn header_roles() {
        assert_eq!(
            header_role("Nom"),
            Some(ColumnRole::Direct(EntityKind::Nom))
        );
        assert_eq!(header_role("Âge"), Some(ColumnRole::Clue(EntityKind::Age)));
        assert_eq!(
            header_role("Code postal"),
            Some(ColumnRole::Clue(EntityKind::Lieu))
        );
        assert_eq!(
            header_role("Sexe"),
            Some(ColumnRole::Clue(EntityKind::Autre))
        );
        assert_eq!(
            header_role("Diagnostic principal"),
            Some(ColumnRole::Sensitive(EntityKind::Sante))
        );
        assert_eq!(
            header_role("N° client"),
            Some(ColumnRole::Stable(EntityKind::Identifiant))
        );
        assert_eq!(header_role("Remarques"), None);
        // « cp » est trop court pour être cherché dans un en-tête plus long.
        assert_eq!(header_role("cp de livraison"), None);
    }

    #[test]
    fn kinds_to_roles() {
        assert_eq!(
            role_for_kind(EntityKind::Email),
            ColumnRole::Direct(EntityKind::Email)
        );
        assert_eq!(
            role_for_kind(EntityKind::Matricule),
            ColumnRole::Stable(EntityKind::Matricule)
        );
        assert_eq!(
            role_for_kind(EntityKind::Sante),
            ColumnRole::Sensitive(EntityKind::Sante)
        );
        assert_eq!(
            role_for_kind(EntityKind::Montant),
            ColumnRole::Clue(EntityKind::Montant)
        );
    }

    #[test]
    fn face_coverage() {
        let face = PixelRect {
            x: 10,
            y: 10,
            w: 10,
            h: 10,
        };
        assert!((covered(&face, &face) - 1.0).abs() < 1e-9);
        let half = PixelRect {
            x: 15,
            y: 0,
            w: 100,
            h: 100,
        };
        assert!((covered(&face, &half) - 0.5).abs() < 1e-9);
        let away = PixelRect {
            x: 50,
            y: 50,
            w: 5,
            h: 5,
        };
        assert_eq!(covered(&face, &away), 0.0);
    }

    #[test]
    fn generalization_levels() {
        assert_eq!(level_of(EntityKind::Age, "42 ans", "*"), Some(3));
        assert_eq!(
            level_of(
                EntityKind::Age,
                "42",
                &generalize(EntityKind::Age, "42", 2).unwrap_or_default()
            ),
            Some(2)
        );
        assert_eq!(level_of(EntityKind::Age, "42", "la quarantaine"), None);
    }

    fn sample() -> Analysis {
        let engine = Engine::standard();
        let text = "Jean Dupont, 42 ans, habite à Farino. Contact : jean.dupont@example.com.\n\
                    Nautilus Pacifique SARL emploie Marie Durand depuis le 12/03/2019.";
        let mut opts = AnalyzeOptions::default();
        opts.detect.profile = anon_core::Profile::Renforce;
        engine.analyze_text(text, &opts, &Session::new())
    }

    /// État observable de l'analyse touché par les corrections.
    fn state(a: &Analysis) -> (Vec<Candidate>, Vec<crate::ImageMask>, Vec<SuppressedRows>) {
        (
            a.candidates.clone(),
            a.masks().to_vec(),
            a.suppressed.clone(),
        )
    }

    fn arb_fix(ids: Vec<usize>) -> impl Strategy<Value = Fix> {
        let pick = proptest::sample::select(ids);
        prop_oneof![
            pick.clone().prop_map(|value_id| Fix::Mask { value_id }),
            (pick.clone(), "[a-z]{1,8}")
                .prop_map(|(value_id, to)| Fix::Generalize { value_id, to }),
            proptest::collection::vec((pick.clone(), "[a-z]{1,8}"), 0..4)
                .prop_map(|items| Fix::GeneralizeMany { items }),
            proptest::collection::vec(pick, 0..4).prop_map(|value_ids| Fix::SuppressRows {
                table: 0,
                rows: vec![0],
                value_ids,
            }),
            (0u32..50, 0u32..50).prop_map(|(x, y)| Fix::MaskRegion {
                file: 0,
                media: "image".into(),
                rect: PixelRect { x, y, w: 5, h: 5 },
            }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Appliquer une suite de corrections puis les annuler dans l'ordre
        /// inverse redonne exactement l'état d'avant.
        #[test]
        fn apply_then_undo_restores_state(
            fixes in {
                let ids: Vec<usize> = sample().candidates.iter().map(|c| c.id).chain([999]).collect();
                proptest::collection::vec(arb_fix(ids), 1..6)
            }
        ) {
            let mut a = sample();
            // État de départ varié : une valeur décochée, une autre généralisée.
            if let Some(first) = a.candidates.first().map(|c| c.id) {
                a.set_selected(first, false);
            }
            if let Some(last) = a.candidates.last().map(|c| c.id) {
                a.set_treatment(last, Treatment::Generalize("x".into()));
            }
            a.add_mask(0, "image", PixelRect { x: 1, y: 1, w: 2, h: 2 });
            let before = state(&a);
            let mut undos = Vec::new();
            for f in &fixes {
                undos.push(apply_fix(&mut a, f));
            }
            for u in undos.into_iter().rev() {
                undo_fix(&mut a, u);
            }
            prop_assert!(state(&a) == before);
        }

        /// Annuler puis rétablir (le jeton rendu par `undo_fix`) redonne
        /// exactement l'état corrigé, et le jeton de rétablissement annule
        /// encore : Ctrl+Z, Ctrl+Maj+Z, Ctrl+Z.
        #[test]
        fn undo_then_redo_restores_the_fixed_state(
            fixes in {
                let ids: Vec<usize> = sample().candidates.iter().map(|c| c.id).chain([999]).collect();
                proptest::collection::vec(arb_fix(ids), 1..6)
            }
        ) {
            let mut a = sample();
            if let Some(first) = a.candidates.first().map(|c| c.id) {
                a.set_selected(first, false);
            }
            a.add_mask(0, "image", PixelRect { x: 1, y: 1, w: 2, h: 2 });
            let before = state(&a);
            let mut undo = FixUndo::default();
            for f in &fixes {
                undo.extend(apply_fix(&mut a, f));
            }
            let fixed = state(&a);
            let redo = undo_fix(&mut a, undo);
            prop_assert!(state(&a) == before);
            let undo = undo_fix(&mut a, redo);
            prop_assert!(state(&a) == fixed);
            undo_fix(&mut a, undo);
            prop_assert!(state(&a) == before);
        }
    }

    #[test]
    fn a_copy_assesses_like_the_original() {
        let a = sample();
        let copy = a.clone();
        let input = build_input(&a, true);
        assert_eq!(build_input(&copy, true), input);
        assert_eq!(copy.candidates, a.candidates);
        // Les données gardées en mémoire sont partagées, pas recopiées.
        assert!(std::sync::Arc::ptr_eq(&copy.store, &a.store));
    }

    #[test]
    fn fixes_change_the_selection() {
        let mut a = sample();
        let Some(c) = a
            .candidates
            .iter()
            .find(|c| c.kind == EntityKind::Lieu)
            .cloned()
        else {
            panic!("lieu attendu");
        };
        a.set_selected(c.id, false);
        let undo = apply_fix(
            &mut a,
            &Fix::Generalize {
                value_id: c.id,
                to: "province Sud".into(),
            },
        );
        let now = a.candidate(c.id).cloned();
        assert!(now.as_ref().is_some_and(|n| n.selected));
        assert_eq!(
            now.map(|n| n.treatment),
            Some(Treatment::Generalize("province Sud".into()))
        );
        assert!(!undo.is_empty());
        assert!(!format!("{undo:?}").contains("province"));
        undo_fix(&mut a, undo);
        assert!(a.candidate(c.id).is_some_and(|n| !n.selected));
        // Une correction sur une valeur inconnue ne change rien.
        assert!(apply_fix(&mut a, &Fix::Mask { value_id: 999 }).is_empty());
    }
}
