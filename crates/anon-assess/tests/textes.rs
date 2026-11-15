//! Propriétés de l'évaluation des textes (données fictives générées) : la
//! première correction proposée pour un échec ne fait jamais baisser
//! l'estimation du groupe d'une personne. Performance sur 1 000 paragraphes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_assess::assess;
use anon_core::{
    AssessInput, AssessSettings, Assessment, EntityKind, FailureReason, Fix, Mention, Shown,
    SubjectRef, Tag, TextUnit,
};
use anon_dict::hierarchy::generalize;
use proptest::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Durée maximale d'une évaluation de 1 000 paragraphes de 10 mentions
/// (compilation optimisée).
const MAX_DURATION_1000: Duration = Duration::from_millis(100);
/// Paragraphes du test de performance.
const PERF_UNITS: usize = 1_000;

const PLACES: &[&str] = &[
    "Farino",
    "Nouméa",
    "Bourail",
    "Sarraméa",
    "Nancy",
    "Paris",
    "Saint-Denis",
    "Mont-Dore",
    "Atlantis",
    "98800",
    "54000",
];
const JOBS: &[&str] = &[
    "infirmière",
    "responsable technique",
    "gérant",
    "comptable",
    "astronaute",
    "directrice financière",
];
const BIRTHS: &[&str] = &["12/08/1946", "août 1971", "1988", "03/02/2001"];
const COMPANIES: &[&str] = &["Nautilus Pacifique SARL", "Atelier Bougainville SAS"];
const SENSITIVE: &[(EntityKind, &str)] = &[
    (EntityKind::Sante, "atteinte d'un diabète"),
    (EntityKind::Evenement, "licenciée pour faute grave"),
    (EntityKind::Judiciaire, "condamné pour vol"),
];

/// Une mention générée : type, valeur, et forme (0 visible, 1 masquée, 2+ un
/// niveau de hiérarchie).
type Gen = (EntityKind, String, u8);

fn clue() -> impl Strategy<Value = Gen> {
    let shown = 0u8..5;
    prop_oneof![
        (prop::sample::select(PLACES), shown.clone()).prop_map(|(v, s)| (
            EntityKind::Lieu,
            v.to_string(),
            s
        )),
        (0u32..100, shown.clone()).prop_map(|(a, s)| (EntityKind::Age, format!("{a} ans"), s)),
        (prop::sample::select(JOBS), shown.clone()).prop_map(|(v, s)| (
            EntityKind::Fonction,
            v.to_string(),
            s
        )),
        (prop::sample::select(BIRTHS), shown.clone()).prop_map(|(v, s)| (
            EntityKind::DateNaiss,
            v.to_string(),
            s
        )),
        (prop::sample::select(COMPANIES), 0u8..2).prop_map(|(v, s)| (
            EntityKind::Societe,
            v.to_string(),
            s
        )),
        (prop::sample::select(SENSITIVE), 0u8..2).prop_map(|((k, v), s)| (k, v.to_string(), s)),
        (0u8..2).prop_map(|s| (EntityKind::Matricule, "MAT-004517".to_string(), s)),
        (0u8..2).prop_map(|s| (EntityKind::Email, "x@example.com".to_string(), s)),
    ]
}

fn person() -> impl Strategy<Value = Gen> {
    // Personne masquée (indice 1 à 4) ou nom visible.
    (1u32..5, 0u8..6).prop_map(|(i, v)| {
        if v == 0 {
            (EntityKind::Personne, format!("Personne visible {i}"), 0)
        } else {
            (EntityKind::Personne, format!("Personne {i}"), 1)
        }
    })
}

fn units_strategy() -> impl Strategy<Value = Vec<Vec<Gen>>> {
    let paragraph = (
        prop::collection::vec(person(), 0..3),
        prop::collection::vec(clue(), 0..7),
        any::<u64>(),
    )
        .prop_map(|(people, clues, seed)| {
            // Personnes intercalées dans les indices, selon la graine.
            let mut out = clues;
            for (n, p) in people.into_iter().enumerate() {
                let len = out.len() + 1;
                let at = usize::try_from(seed.rotate_left(u32::try_from(n * 7).unwrap()))
                    .unwrap_or(0)
                    % len;
                out.insert(at, p);
            }
            out
        });
    prop::collection::vec(paragraph, 1..6)
}

/// Construit les paragraphes : une même valeur reçoit le même `value_id` et la
/// même forme partout (celle de sa première occurrence), comme dans le moteur ;
/// un masquage reçoit une balise par valeur (indice de la personne pour les
/// personnes).
fn build(paragraphs: &[Vec<Gen>]) -> Vec<TextUnit> {
    let mut ids: HashMap<(EntityKind, String), (usize, u8)> = HashMap::new();
    paragraphs
        .iter()
        .map(|p| TextUnit {
            file: 0,
            mentions: p
                .iter()
                .map(|(kind, value, s)| {
                    let next = ids.len();
                    let (id, s) = *ids.entry((*kind, value.clone())).or_insert((next, *s));
                    let shown = match s {
                        0 => Shown::Visible,
                        1 => {
                            let index = if kind.is_person_part() {
                                value
                                    .rsplit(' ')
                                    .next()
                                    .and_then(|n| n.parse().ok())
                                    .unwrap_or(1)
                            } else {
                                u32::try_from(id).unwrap() + 1
                            };
                            Shown::Masked(Tag::new(*kind, index))
                        }
                        l => generalize(*kind, value, l - 1)
                            .map_or(Shown::Visible, Shown::Generalized),
                    };
                    Mention {
                        value_id: id,
                        kind: *kind,
                        value: value.clone(),
                        shown,
                    }
                })
                .collect(),
        })
        .collect()
}

fn run(units: &[TextUnit], k_min: u32) -> Assessment {
    let input = AssessInput {
        texts: units.to_vec(),
        ..AssessInput::default()
    };
    let settings = AssessSettings {
        k_min,
        ..AssessSettings::default()
    };
    assess(&input, &settings)
}

/// Applique une correction comme le moteur ; un masquage reçoit une balise
/// neuve (indice hors de ceux générés).
fn apply(units: &mut [TextUnit], fix: &Fix) {
    let mut set = |id: usize, shown: &dyn Fn(EntityKind) -> Shown| {
        for m in units.iter_mut().flat_map(|u| u.mentions.iter_mut()) {
            if m.value_id == id {
                m.shown = shown(m.kind);
            }
        }
    };
    match fix {
        Fix::Mask { value_id } => {
            let index = 1000 + u32::try_from(*value_id).unwrap();
            set(*value_id, &|k| Shown::Masked(Tag::new(k, index)));
        }
        Fix::Generalize { value_id, to } => set(*value_id, &|_| Shown::Generalized(to.clone())),
        Fix::GeneralizeMany { items } => {
            for (id, to) in items {
                set(*id, &|_| Shown::Generalized(to.clone()));
            }
        }
        other => panic!("correction inattendue pour un texte : {other:?}"),
    }
}

fn estimates(a: &Assessment) -> HashMap<SubjectRef, u64> {
    a.subjects
        .iter()
        .filter_map(|s| Some((s.subject.clone(), s.group_estimate?)))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn first_fix_never_lowers_the_estimate(paragraphs in units_strategy(), k_min in 2u32..21) {
        let units = build(&paragraphs);
        let a = run(&units, k_min);
        let before = estimates(&a);
        for f in &a.failures {
            prop_assert!(
                matches!(f.subject, SubjectRef::Person { .. }),
                "sujet inattendu : {:?}", f.subject
            );
            if matches!(f.reason, FailureReason::SmallGroup { .. }) {
                prop_assert!(!f.fixes.is_empty(), "groupe trop petit sans correction");
            }
            let Some(first) = f.fixes.first() else { continue };
            let mut fixed = units.clone();
            apply(&mut fixed, first);
            let after = estimates(&run(&fixed, k_min));
            for (subject, old) in &before {
                if let Some(new) = after.get(subject) {
                    prop_assert!(
                        new >= old,
                        "{:?} : {} -> {} après {:?} ({:?})", subject, old, new, first, f.reason
                    );
                }
            }
            // Une généralisation conseillée pour l'isolement atteint k_min.
            if matches!(f.reason, FailureReason::SmallGroup { .. })
                && matches!(first, Fix::Generalize { .. } | Fix::GeneralizeMany { .. })
            {
                let new = after.get(&f.subject).copied().unwrap_or(0);
                prop_assert!(new >= u64::from(k_min), "{} < {}", new, k_min);
            }
        }
        // Un nom visible n'est jamais anonyme.
        let visible_name = units.iter().flat_map(|u| &u.mentions).any(|m| {
            m.kind.is_person_part() && matches!(m.shown, Shown::Visible)
        });
        if visible_name {
            prop_assert!(!a.is_anonymous());
        }
    }
}

/// 1 000 paragraphes de 10 mentions, chaque personne en échec (fonction et
/// société visibles) : moins de 100 ms en compilation optimisée (`cargo test
/// --release`).
#[test]
fn thousand_paragraphs_in_under_100_ms() {
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move |m: usize| {
        // Générateur congruentiel : données fictives reproductibles.
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(seed >> 33).unwrap() % m
    };
    let paragraphs: Vec<Vec<Gen>> = (0..PERF_UNITS)
        .map(|i| {
            vec![
                (EntityKind::Personne, format!("Personne {}", i + 1), 1),
                (EntityKind::Lieu, PLACES[next(PLACES.len())].to_string(), 0),
                (EntityKind::Age, format!("{} ans", next(100)), 0),
                (EntityKind::Fonction, JOBS[next(JOBS.len())].to_string(), 0),
                (
                    EntityKind::Societe,
                    COMPANIES[next(COMPANIES.len())].to_string(),
                    0,
                ),
                (
                    EntityKind::DateNaiss,
                    BIRTHS[next(BIRTHS.len())].to_string(),
                    0,
                ),
                (EntityKind::Sante, "atteinte d'un diabète".to_string(), 0),
                (EntityKind::Date, format!("{}/03/2026", 1 + next(28)), 0),
                (EntityKind::Email, format!("p{i}@example.com"), 1),
                (
                    EntityKind::Matricule,
                    format!("MAT-{:06}", next(999_999)),
                    0,
                ),
            ]
        })
        .collect();
    let units = build(&paragraphs);
    assert!(units.iter().all(|u| u.mentions.len() == 10));
    let start = Instant::now();
    let a = run(&units, 5);
    let elapsed = start.elapsed();
    println!("{PERF_UNITS} paragraphes de 10 mentions : {elapsed:?}");
    assert_eq!(a.subjects.len(), PERF_UNITS);
    assert!(
        a.failures
            .iter()
            .any(|f| matches!(f.reason, FailureReason::SmallGroup { .. }) && !f.fixes.is_empty())
    );
    if !cfg!(debug_assertions) {
        assert!(elapsed < MAX_DURATION_1000, "{elapsed:?}");
    }
}
