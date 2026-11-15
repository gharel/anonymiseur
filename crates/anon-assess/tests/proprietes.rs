//! Propriétés de l'évaluation des tableaux (données fictives générées) :
//! après application des corrections proposées, chaque groupe atteint `k_min`
//! ou ses lignes sont supprimées ; la suppression reste bornée. Performance sur
//! 100 000 lignes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_assess::{MAX_SUPPRESSION_RATE, assess};
use anon_core::{
    AssessInput, AssessSettings, ColumnRole, EntityKind, FailureReason, Fix, Shown, TableCell,
    TableInput,
};
use proptest::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Durée maximale d'une évaluation de 100 000 lignes (compilation optimisée).
const MAX_DURATION_100K: Duration = Duration::from_secs(1);
/// Nombre de lignes du test de performance.
const PERF_ROWS: usize = 100_000;

const COMMUNES: &[&str] = &[
    "Farino", "Bourail", "Koné", "Nouméa", "Païta", "Lifou", "Nancy", "Metz", "Lyon", "Paris",
    "Brest", "Dumbéa", "Thio", "Poya", "Voh",
];

/// Tableau dont chaque valeur distincte d'une colonne a son `value_id`.
fn table(roles: Vec<ColumnRole>, rows: Vec<Vec<String>>) -> TableInput {
    let mut ids: HashMap<(usize, String), usize> = HashMap::new();
    let rows = rows
        .into_iter()
        .map(|r| {
            r.into_iter()
                .enumerate()
                .map(|(c, value)| {
                    let next = ids.len();
                    let id = *ids.entry((c, value.clone())).or_insert(next);
                    TableCell {
                        value,
                        value_id: Some(id),
                        shown: Shown::Visible,
                    }
                })
                .collect()
        })
        .collect();
    TableInput {
        file: 0,
        sheet: String::new(),
        headers: roles.iter().map(|_| String::new()).collect(),
        roles,
        rows,
    }
}

fn settings(k_min: u32) -> AssessSettings {
    AssessSettings {
        k_min,
        ..AssessSettings::default()
    }
}

/// Ligne générée : âge, rang de la commune, date de naissance (jour, mois,
/// année), deux premiers chiffres du code postal.
type GeneratedRow = (u32, usize, (u32, u32, u32), u32);

fn rows_strategy() -> impl Strategy<Value = Vec<GeneratedRow>> {
    let row = (
        0u32..100,
        0usize..COMMUNES.len(),
        (1u32..=28, 1u32..=12, 1930u32..2010),
        10u32..30,
    );
    prop::collection::vec(row, 20..160)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn corrections_reach_k_min(rows in rows_strategy(), k_min in 2u32..10, columns in 1usize..=4) {
        let roles: Vec<ColumnRole> = [
            ColumnRole::Clue(EntityKind::Age),
            ColumnRole::Clue(EntityKind::Lieu),
            ColumnRole::Clue(EntityKind::DateNaiss),
            ColumnRole::Clue(EntityKind::Lieu),
        ]
        .into_iter()
        .take(columns)
        .collect();
        let text_rows: Vec<Vec<String>> = rows
            .iter()
            .map(|(age, commune, (d, m, y), cp)| {
                vec![
                    format!("{age} ans"),
                    COMMUNES[*commune].to_string(),
                    format!("{d:02}/{m:02}/{y}"),
                    format!("{cp}000"),
                ]
                .into_iter()
                .take(columns)
                .collect()
            })
            .collect();
        let t = table(roles, text_rows);
        let input = AssessInput { tables: vec![t.clone()], ..AssessInput::default() };
        let a = assess(&input, &settings(k_min));

        let fixes: Vec<&Fix> = a.failures.iter()
            .filter(|f| matches!(f.reason, FailureReason::SmallGroup { .. }))
            .flat_map(|f| &f.fixes)
            .collect();
        let mut to: HashMap<usize, &str> = HashMap::new();
        let mut gone: Vec<usize> = Vec::new();
        for f in &fixes {
            match f {
                Fix::GeneralizeMany { items } => {
                    for (id, s) in items {
                        prop_assert!(to.insert(*id, s).is_none(), "valeur généralisée deux fois");
                    }
                }
                Fix::SuppressRows { table, rows, .. } => {
                    prop_assert_eq!(*table, 0);
                    gone.extend(rows);
                }
                other => prop_assert!(false, "correction inattendue : {:?}", other),
            }
        }
        let has_small = a.failures.iter().any(|f| matches!(f.reason, FailureReason::SmallGroup { .. }));
        prop_assert_eq!(has_small, !fixes.is_empty(), "un échec d'isolement sans correction");
        prop_assert!(gone.len() as f64 <= MAX_SUPPRESSION_RATE * t.rows.len() as f64);

        let after: Vec<Vec<String>> = t.rows.iter().map(|r| {
            r.iter().map(|c| c.value_id.and_then(|id| to.get(&id)).map_or(c.value.clone(), |s| s.to_string())).collect()
        }).collect();
        let mut counts: HashMap<&Vec<String>, u32> = HashMap::new();
        for (i, k) in after.iter().enumerate() {
            if !gone.contains(&i) {
                *counts.entry(k).or_insert(0) += 1;
            }
        }
        for (i, k) in after.iter().enumerate() {
            prop_assert!(gone.contains(&i) || counts[k] >= k_min, "ligne {} : groupe de {}", i, counts[k]);
        }

        // Réévaluation après correction (lignes supprimées retirées) : plus aucun
        // groupe trop petit.
        let kept: Vec<Vec<String>> = after.into_iter().enumerate()
            .filter(|(i, _)| !gone.contains(i)).map(|(_, r)| r).collect();
        if !kept.is_empty() {
            let t2 = table(t.roles.clone(), kept);
            let a2 = assess(&AssessInput { tables: vec![t2], ..AssessInput::default() }, &settings(k_min));
            let still_small = a2
                .failures
                .iter()
                .any(|f| matches!(f.reason, FailureReason::SmallGroup { .. }));
            prop_assert!(!still_small, "groupe trop petit après correction");
        }
    }
}

/// 100 000 lignes, quatre colonnes d'indices et un attribut sensible : moins
/// d'une seconde en compilation optimisée (`cargo test --release`).
#[test]
fn hundred_thousand_rows_in_under_a_second() {
    let diseases = [
        "asthme",
        "diabète",
        "migraine",
        "eczéma",
        "arthrose",
        "sclérose",
    ];
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move |m: u64| {
        // Générateur congruentiel : données fictives reproductibles.
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % m
    };
    let rows: Vec<Vec<String>> = (0..PERF_ROWS)
        .map(|_| {
            vec![
                format!("{} ans", next(100)),
                COMMUNES[next(COMMUNES.len() as u64) as usize].to_string(),
                ["F", "M"][next(2) as usize].to_string(),
                format!(
                    "{:02}/{:02}/{}",
                    next(28) + 1,
                    next(12) + 1,
                    1930 + next(80)
                ),
                diseases[next(diseases.len() as u64) as usize].to_string(),
            ]
        })
        .collect();
    let roles = vec![
        ColumnRole::Clue(EntityKind::Age),
        ColumnRole::Clue(EntityKind::Lieu),
        ColumnRole::Clue(EntityKind::Autre),
        ColumnRole::Clue(EntityKind::DateNaiss),
        ColumnRole::Sensitive(EntityKind::Sante),
    ];
    let input = AssessInput {
        tables: vec![table(roles, rows)],
        ..AssessInput::default()
    };
    let start = Instant::now();
    let a = assess(&input, &settings(5));
    let elapsed = start.elapsed();
    assert!(!a.subjects.is_empty());
    assert!(
        a.failures
            .iter()
            .any(|f| matches!(f.reason, FailureReason::SmallGroup { .. }) && !f.fixes.is_empty())
    );
    if !cfg!(debug_assertions) {
        assert!(elapsed < MAX_DURATION_100K, "{elapsed:?}");
    }
}
