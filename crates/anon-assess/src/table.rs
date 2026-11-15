//! Évaluation d'un tableau : un sujet par groupe de lignes qui partagent les
//! mêmes valeurs affichées sur les colonnes d'indices (classe d'équivalence).
//!
//! - Isolement : k-anonymat (groupe de moins de `k_min` lignes) et identifiants
//!   directs visibles.
//! - Recoupement : identifiants stables visibles.
//! - Déduction : l-diversité (tous les membres d'un groupe partagent la même
//!   valeur visible d'un attribut sensible ; exemple 12 des lignes directrices).
//! - Correction de l'isolement : algorithme glouton de type Datafly (Sweeney,
//!   2002) : tant que la part des lignes de groupes trop petits dépasse
//!   [`MAX_SUPPRESSION_RATE`], la colonne d'indices la plus variée est rendue
//!   moins précise d'un cran (`anon_dict::hierarchy`, puis « * » au-delà du
//!   dernier niveau) ; les lignes restantes sont ensuite supprimées.
//!
//! La correction de l'isolement est propre au tableau entier : elle figure dans
//! le premier échec `SmallGroup` du tableau ; les échecs `SmallGroup` suivants du
//! même tableau n'en portent pas (ils la partagent).
//!
//! Exigence envers le moteur : chaque cellule non vide d'une colonne évaluée
//! porte un `value_id`. Une valeur visible sans `value_id` ne peut pas être
//! corrigée : l'échec est signalé sans correction.

use anon_core::assess::MAX_SUPPRESSION_RATE;
use anon_core::{
    ColumnRole, Criterion, EntityKind, Failure, FailureReason, Fix, Shown, SubjectRef,
    SubjectReport, TableCell, TableInput,
};
use anon_dict::hierarchy::{generalize, max_level};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// Forme la plus générale d'une valeur, au-delà de sa hiérarchie (Datafly).
const SUPPRESSED: &str = "*";

/// Texte affiché d'une cellule dans le document produit.
fn displayed(cell: &TableCell) -> Cow<'_, str> {
    match &cell.shown {
        Shown::Masked(tag) => Cow::Owned(tag.to_string()),
        Shown::Generalized(s) => Cow::Borrowed(s.as_str()),
        Shown::Visible => Cow::Borrowed(cell.value.as_str()),
    }
}

/// Vrai si la cellule montre quelque chose de sa valeur (visible ou rendue
/// moins précise, non vide).
fn exposed(cell: &TableCell) -> bool {
    !matches!(cell.shown, Shown::Masked(_)) && !displayed(cell).trim().is_empty()
}

fn cell_at(row: &[TableCell], col: usize) -> Option<&TableCell> {
    row.get(col)
}

/// Une valeur distincte d'une colonne d'indices.
struct Distinct<'a> {
    value: &'a str,
    shown: String,
    value_id: Option<usize>,
    /// Niveau déjà atteint par la forme affichée (0 si visible ou masquée) ;
    /// `None` si elle a été rendue moins précise hors de la hiérarchie. Le plan
    /// ne revient jamais à une forme plus précise que celle-ci.
    floor: Option<u8>,
}

/// Niveau de hiérarchie auquel correspond la forme affichée d'une cellule.
fn floor_of(kind: EntityKind, cell: &TableCell) -> Option<u8> {
    match &cell.shown {
        Shown::Generalized(s) => {
            (1..=max_level(kind)).find(|l| generalize(kind, &cell.value, *l).as_deref() == Some(s))
        }
        Shown::Visible | Shown::Masked(_) => Some(0),
    }
}

/// Colonne d'indices : valeurs distinctes, valeur de chaque ligne, formes
/// internées par niveau.
struct ClueColumn<'a> {
    col: usize,
    kind: EntityKind,
    /// Niveau « * » (dernier cran).
    star: u8,
    distinct: Vec<Distinct<'a>>,
    /// Indice dans `distinct` de chaque ligne.
    row_value: Vec<u32>,
    /// Formes internées (texte → identifiant).
    forms: HashMap<String, u32>,
    form_text: Vec<String>,
    /// Pour chaque niveau calculé : identifiant de forme de chaque valeur distincte.
    by_level: HashMap<u8, Vec<u32>>,
}

impl<'a> ClueColumn<'a> {
    fn new(col: usize, kind: EntityKind, rows: &'a [Vec<TableCell>]) -> Self {
        let mut index: HashMap<(&str, Cow<'_, str>, Option<usize>), u32> = HashMap::new();
        let mut distinct = Vec::new();
        let mut row_value = Vec::with_capacity(rows.len());
        for row in rows {
            let cell = cell_at(row, col);
            let (value, shown, value_id) = match cell {
                Some(c) => (c.value.as_str(), displayed(c), c.value_id),
                None => ("", Cow::Borrowed(""), None),
            };
            let next = u32::try_from(distinct.len()).unwrap_or(u32::MAX);
            let id = *index
                .entry((value, shown.clone(), value_id))
                .or_insert_with(|| {
                    distinct.push(Distinct {
                        value,
                        shown: shown.into_owned(),
                        value_id,
                        floor: cell.map_or(Some(0), |c| floor_of(kind, c)),
                    });
                    next
                });
            row_value.push(id);
        }
        let mut c = Self {
            col,
            kind,
            star: max_level(kind).saturating_add(1),
            distinct,
            row_value,
            forms: HashMap::new(),
            form_text: Vec::new(),
            by_level: HashMap::new(),
        };
        c.ensure_level(0);
        c
    }

    fn intern(&mut self, form: String) -> u32 {
        if let Some(id) = self.forms.get(&form) {
            return *id;
        }
        let id = u32::try_from(self.form_text.len()).unwrap_or(u32::MAX);
        self.form_text.push(form.clone());
        self.forms.insert(form, id);
        id
    }

    /// Forme d'une valeur à un niveau : la hiérarchie au niveau demandé (jamais
    /// en deçà de la forme déjà affichée), à défaut au plus haut niveau inférieur
    /// qui s'applique, à défaut le texte affiché ; « * » au dernier cran (une
    /// cellule vide le reste).
    fn form(kind: EntityKind, star: u8, d: &Distinct<'_>, level: u8) -> String {
        if d.value.trim().is_empty() {
            return d.shown.clone();
        }
        if level >= star {
            return SUPPRESSED.to_string();
        }
        let Some(floor) = d.floor else {
            return d.shown.clone();
        };
        (1..=level.max(floor))
            .rev()
            .find_map(|l| generalize(kind, d.value, l))
            .unwrap_or_else(|| d.shown.clone())
    }

    fn ensure_level(&mut self, level: u8) {
        if self.by_level.contains_key(&level) {
            return;
        }
        let (kind, star) = (self.kind, self.star);
        let texts: Vec<String> = self
            .distinct
            .iter()
            .map(|d| Self::form(kind, star, d, level))
            .collect();
        let ids = texts.into_iter().map(|t| self.intern(t)).collect();
        self.by_level.insert(level, ids);
    }

    fn level_forms(&self, level: u8) -> &[u32] {
        self.by_level.get(&level).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Identifiant de forme de la ligne `row` au niveau `level` (déjà calculé).
    fn row_form(&self, row: usize, level: u8) -> u32 {
        self.row_value
            .get(row)
            .and_then(|d| usize::try_from(*d).ok())
            .and_then(|d| self.level_forms(level).get(d))
            .copied()
            .unwrap_or(u32::MAX)
    }

    /// Nombre de formes distinctes au niveau `level` (déjà calculé).
    fn variety(&self, level: u8) -> usize {
        self.level_forms(level).iter().collect::<HashSet<_>>().len()
    }
}

/// Clés de regroupement de toutes les lignes, à plat (`n × colonnes`).
fn keys(clues: &[ClueColumn<'_>], levels: &[u8], n: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(n.saturating_mul(clues.len()));
    for row in 0..n {
        for (c, level) in clues.iter().zip(levels) {
            out.push(c.row_form(row, *level));
        }
    }
    out
}

/// Lignes appartenant à un groupe de moins de `k_min` lignes.
fn violating_rows(flat: &[u32], width: usize, n: usize, k_min: u32) -> Vec<usize> {
    let key = |row: usize| -> &[u32] {
        let start = row.saturating_mul(width);
        flat.get(start..start.saturating_add(width)).unwrap_or(&[])
    };
    let mut counts: HashMap<&[u32], u32> = HashMap::with_capacity(n);
    for row in 0..n {
        *counts.entry(key(row)).or_insert(0) += 1;
    }
    (0..n)
        .filter(|row| counts.get(key(*row)).is_some_and(|c| *c < k_min))
        .collect()
}

/// Plan de correction de l'isolement : niveaux par colonne d'indices (Datafly)
/// et lignes à supprimer.
struct Plan {
    levels: Vec<u8>,
    suppressed: Vec<usize>,
    /// Vrai si la suppression dépasse [`MAX_SUPPRESSION_RATE`] : à l'utilisateur
    /// d'arbitrer, aucune suppression n'est proposée.
    over_budget: bool,
}

fn datafly(clues: &mut [ClueColumn<'_>], n: usize, k_min: u32) -> Plan {
    let width = clues.len();
    let mut levels = vec![0u8; width];
    let budget = MAX_SUPPRESSION_RATE * n as f64;
    loop {
        let flat = keys(clues, &levels, n);
        let violating = violating_rows(&flat, width, n, k_min);
        if violating.len() as f64 <= budget {
            return Plan {
                levels,
                suppressed: violating,
                over_budget: false,
            };
        }
        // Colonne la plus variée parmi celles qui peuvent encore monter d'un cran.
        let pick = clues
            .iter()
            .zip(&levels)
            .enumerate()
            .filter(|(_, (c, l))| **l < c.star)
            .max_by_key(|(i, (c, l))| (c.variety(**l), std::cmp::Reverse(*i)))
            .map(|(i, _)| i);
        let Some(i) = pick else {
            return Plan {
                levels,
                suppressed: violating,
                over_budget: true,
            };
        };
        if let (Some(level), Some(column)) = (levels.get_mut(i), clues.get_mut(i)) {
            *level = level.saturating_add(1);
            column.ensure_level(*level);
        }
    }
}

/// Corrections proposées pour l'isolement d'un tableau, ou aucune si une valeur
/// à modifier n'a pas de `value_id`.
fn plan_fixes(
    table: usize,
    clues: &[ClueColumn<'_>],
    plan: &Plan,
    rows: &[Vec<TableCell>],
) -> Vec<Fix> {
    let mut fixes = Vec::new();
    let mut items: Vec<(usize, String)> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for (c, level) in clues.iter().zip(&plan.levels) {
        if *level == 0 {
            continue;
        }
        for (d, (base, new)) in c
            .distinct
            .iter()
            .zip(c.level_forms(0).iter().zip(c.level_forms(*level)))
        {
            if base == new {
                continue;
            }
            let Some(id) = d.value_id else {
                return Vec::new();
            };
            if seen.insert(id) {
                let text = c
                    .form_text
                    .get(usize::try_from(*new).unwrap_or(usize::MAX))
                    .cloned()
                    .unwrap_or_default();
                items.push((id, text));
            }
        }
    }
    if !items.is_empty() {
        fixes.push(Fix::GeneralizeMany { items });
    }
    if !plan.suppressed.is_empty() && !plan.over_budget {
        let mut value_ids = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        for row in &plan.suppressed {
            for c in clues {
                let Some(cell) = rows.get(*row).and_then(|r| cell_at(r, c.col)) else {
                    continue;
                };
                if cell.value.trim().is_empty() {
                    continue;
                }
                let Some(id) = cell.value_id else {
                    return Vec::new();
                };
                if seen.insert(id) {
                    value_ids.push(id);
                }
            }
        }
        fixes.push(Fix::SuppressRows {
            table,
            rows: plan.suppressed.clone(),
            value_ids,
        });
    }
    fixes
}

/// Masquage des cellules exposées d'une colonne parmi des lignes, ou aucune
/// correction si l'une d'elles n'a pas de `value_id`.
fn mask_fixes(rows: &[Vec<TableCell>], members: &[usize], col: usize) -> Vec<Fix> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for m in members {
        let Some(cell) = rows.get(*m).and_then(|r| cell_at(r, col)) else {
            continue;
        };
        if !exposed(cell) {
            continue;
        }
        let Some(id) = cell.value_id else {
            return Vec::new();
        };
        if seen.insert(id) {
            ids.push(id);
        }
    }
    ids.into_iter()
        .map(|value_id| Fix::Mask { value_id })
        .collect()
}

/// Évalue un tableau : (bilans des groupes, échecs).
pub(crate) fn assess_table(
    index: usize,
    t: &TableInput,
    k_min: u32,
) -> (Vec<SubjectReport>, Vec<Failure>) {
    let rows = &t.rows;
    let n = rows.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    let mut clues: Vec<ClueColumn<'_>> = Vec::new();
    let (mut direct, mut stable, mut sensitive) = (Vec::new(), Vec::new(), Vec::new());
    for (col, role) in t.roles.iter().enumerate() {
        match *role {
            ColumnRole::Clue(kind) => clues.push(ClueColumn::new(col, kind, rows)),
            ColumnRole::Direct(kind) => direct.push((col, kind)),
            ColumnRole::Stable(kind) => stable.push((col, kind)),
            ColumnRole::Sensitive(kind) => sensitive.push((col, kind)),
            ColumnRole::Other => {}
        }
    }

    // Groupes au niveau actuel (formes affichées), dans l'ordre d'apparition.
    let width = clues.len();
    let levels0 = vec![0u8; width];
    let flat = keys(&clues, &levels0, n);
    let mut group_of: HashMap<&[u32], usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for row in 0..n {
        let start = row.saturating_mul(width);
        let key = flat.get(start..start.saturating_add(width)).unwrap_or(&[]);
        let next = members.len();
        let g = *group_of.entry(key).or_insert(next);
        if g == next {
            members.push(Vec::new());
        }
        if let Some(m) = members.get_mut(g) {
            m.push(row);
        }
    }

    let needs_plan = members
        .iter()
        .any(|m| u32::try_from(m.len()).unwrap_or(u32::MAX) < k_min);
    // La correction de l'isolement vaut pour tout le tableau (les niveaux de
    // généralisation sont communs à toutes les lignes) : elle accompagne le
    // premier échec « groupe trop petit » du tableau, sans être recopiée dans
    // les suivants, qui la partagent.
    let mut isolation_fixes = if needs_plan {
        let plan = datafly(&mut clues, n, k_min);
        plan_fixes(index, &clues, &plan, rows)
    } else {
        Vec::new()
    };

    let mut subjects = Vec::with_capacity(members.len());
    let mut failures = Vec::new();
    for (g, m) in members.iter().enumerate() {
        let subject = SubjectRef::TableGroup {
            table: index,
            group: g,
        };
        let size = u32::try_from(m.len()).unwrap_or(u32::MAX);
        let mut passed = [true; 3];
        let mut fail = |criterion: Criterion, reason: FailureReason, fixes: Vec<Fix>| {
            if let Some(p) = passed.get_mut(criterion.index()) {
                *p = false;
            }
            failures.push(Failure {
                criterion,
                subject: subject.clone(),
                reason,
                fixes,
            });
        };
        if size < k_min {
            fail(
                Criterion::Isolation,
                FailureReason::SmallGroup {
                    estimate: u64::from(size),
                },
                std::mem::take(&mut isolation_fixes),
            );
        }
        for (col, kind) in &direct {
            if m.iter().any(|r| {
                rows.get(*r)
                    .and_then(|row| cell_at(row, *col))
                    .is_some_and(exposed)
            }) {
                fail(
                    Criterion::Isolation,
                    FailureReason::DirectIdentifier { kind: *kind },
                    mask_fixes(rows, m, *col),
                );
            }
        }
        for (col, kind) in &stable {
            if m.iter().any(|r| {
                rows.get(*r)
                    .and_then(|row| cell_at(row, *col))
                    .is_some_and(exposed)
            }) {
                fail(
                    Criterion::Linkage,
                    FailureReason::StableIdentifier { kind: *kind },
                    mask_fixes(rows, m, *col),
                );
            }
        }
        for (col, _) in &sensitive {
            let mut values = m
                .iter()
                .map(|r| rows.get(*r).and_then(|row| cell_at(row, *col)));
            let first = values.next().flatten();
            let homogeneous = first.is_some_and(|f| {
                exposed(f)
                    && values.all(|c| c.is_some_and(|c| exposed(c) && displayed(c) == displayed(f)))
            });
            if homogeneous {
                fail(
                    Criterion::Inference,
                    FailureReason::HomogeneousGroup { column: *col },
                    mask_fixes(rows, m, *col),
                );
            }
        }
        // Indices visibles du groupe, ce qu'une IA pourrait croiser.
        let mut clue_ids = Vec::new();
        let mut seen = HashSet::new();
        for c in &clues {
            for r in m {
                if let Some(cell) = rows.get(*r).and_then(|row| cell_at(row, c.col))
                    && exposed(cell)
                    && let Some(id) = cell.value_id
                    && seen.insert((c.kind, id))
                {
                    clue_ids.push((c.kind, id));
                }
            }
        }
        subjects.push(SubjectReport {
            subject,
            clues: clue_ids,
            group_estimate: Some(u64::from(size)),
            passed,
        });
    }
    (subjects, failures)
}
