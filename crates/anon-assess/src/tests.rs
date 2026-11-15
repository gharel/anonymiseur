//! Tests unitaires sur des tableaux construits à la main (données fictives).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use anon_core::{
    ColumnRole, Criterion, EntityKind, Fix, ImageInput, PixelRect, Shown, SubjectRef, TableCell,
    TableInput, Tag,
};
use std::collections::HashMap;

/// Tableau à partir de lignes de texte ; chaque valeur distincte d'une colonne
/// reçoit son `value_id`, comme le fera le moteur.
fn table(roles: &[ColumnRole], rows: &[&[&str]]) -> TableInput {
    let mut ids: HashMap<(usize, String), usize> = HashMap::new();
    let rows = rows
        .iter()
        .map(|r| {
            r.iter()
                .enumerate()
                .map(|(c, v)| {
                    let next = ids.len();
                    let id = *ids.entry((c, v.to_string())).or_insert(next);
                    TableCell {
                        value: v.to_string(),
                        value_id: (!v.is_empty()).then_some(id),
                        shown: Shown::Visible,
                    }
                })
                .collect()
        })
        .collect();
    TableInput {
        file: 0,
        sheet: "Feuille fictive".into(),
        headers: (0..roles.len()).map(|i| format!("col{i}")).collect(),
        roles: roles.to_vec(),
        rows,
    }
}

fn input(tables: Vec<TableInput>) -> AssessInput {
    AssessInput {
        tables,
        ..AssessInput::default()
    }
}

fn settings(k_min: u32) -> AssessSettings {
    AssessSettings {
        k_min,
        ..AssessSettings::default()
    }
}

use ColumnRole::{Clue, Direct, Other, Sensitive, Stable};
use EntityKind::{Age, Autre, DateNaiss, Lieu, Nom, Sante};

/// Exemple 12 des lignes directrices : les hommes nés en 1946 dont le code
/// postal commence par 54 ont tous la même maladie. Le groupe compte assez de
/// lignes (isolement réussi) mais la déduction échoue.
#[test]
fn guidelines_example_12() {
    let roles = [Clue(Autre), Clue(DateNaiss), Clue(Lieu), Sensitive(Sante)];
    let mut rows: Vec<&[&str]> = Vec::new();
    for _ in 0..6 {
        rows.push(&["M", "1946", "54***", "sclérose en plaques"]);
    }
    let diverse = ["asthme", "diabète", "migraine", "asthme", "eczéma"];
    let women: Vec<[&str; 4]> = diverse.iter().map(|d| ["F", "1946", "54***", *d]).collect();
    for r in &women {
        rows.push(r);
    }
    let men_57: Vec<[&str; 4]> = diverse.iter().map(|d| ["M", "1958", "57***", *d]).collect();
    for r in &men_57 {
        rows.push(r);
    }
    let a = assess(&input(vec![table(&roles, &rows)]), &settings(5));

    assert_eq!(a.subjects.len(), 3);
    let men_1946 = SubjectRef::TableGroup { table: 0, group: 0 };
    let report = a.subjects.iter().find(|s| s.subject == men_1946).unwrap();
    assert_eq!(report.group_estimate, Some(6));
    assert_eq!(report.passed, [true, true, false]);
    assert_eq!(a.failures.len(), 1);
    let f = &a.failures[0];
    assert_eq!(f.criterion, Criterion::Inference);
    assert_eq!(f.reason, FailureReason::HomogeneousGroup { column: 3 });
    assert_eq!(f.subject, men_1946);
    // Une seule valeur à masquer : la maladie partagée.
    assert_eq!(f.fixes.len(), 1);
    assert!(matches!(f.fixes[0], Fix::Mask { .. }));
    assert_eq!(a.verdict, Verdict::Reidentifiable { subjects: 1 });
    assert_eq!(a.summary(Criterion::Isolation), (3, 0));
    assert_eq!(a.summary(Criterion::Inference), (2, 1));
}

#[test]
fn masked_sensitive_values_are_not_homogeneous() {
    let roles = [Clue(Autre), Sensitive(Sante)];
    let mut t = table(&roles, &[&["M", "sclérose"][..]; 5]);
    for r in &mut t.rows {
        r[1].shown = Shown::Masked(Tag::new(Sante, 1));
    }
    let a = assess(&input(vec![t]), &settings(5));
    assert!(a.is_anonymous(), "{:?}", a.failures);
}

/// Âge, commune et code postal précis : chaque ligne est unique. La correction
/// rend les indices moins précis jusqu'à des groupes d'au moins `k_min` lignes.
#[test]
fn small_groups_get_a_generalization_plan() {
    let roles = [Clue(Age), Clue(Lieu), Other];
    let communes = ["Farino", "Bourail", "La Foa", "Moindou", "Sarraméa"];
    let ages: Vec<String> = (0..40).map(|i| format!("{} ans", 60 + i % 10)).collect();
    let rows: Vec<Vec<&str>> = (0..40)
        .map(|i| vec![ages[i].as_str(), communes[i % communes.len()], "x"])
        .collect();
    let rows: Vec<&[&str]> = rows.iter().map(Vec::as_slice).collect();
    let t = table(&roles, &rows);
    let a = assess(&input(vec![t.clone()]), &settings(5));

    let small: Vec<_> = a
        .failures
        .iter()
        .filter(|f| matches!(f.reason, FailureReason::SmallGroup { .. }))
        .collect();
    assert!(!small.is_empty());
    // Correction commune au tableau, dans le premier échec seulement.
    assert!(!small[0].fixes.is_empty());
    assert!(small.iter().skip(1).all(|f| f.fixes.is_empty()));
    let after = apply(&t, &small[0].fixes);
    for (size, suppressed) in group_sizes(&t, &after) {
        assert!(size >= 5 || suppressed, "groupe de {size}");
    }
    // Les communes deviennent « province Sud ».
    let Fix::GeneralizeMany { items } = &small[0].fixes[0] else {
        panic!("{:?}", small[0].fixes);
    };
    assert!(items.iter().any(|(_, to)| to == "province Sud"));
}

/// Lignes du tableau après les corrections : (forme affichée des indices, supprimée).
fn apply(t: &TableInput, fixes: &[Fix]) -> Vec<(Vec<String>, bool)> {
    let mut to: HashMap<usize, &str> = HashMap::new();
    let mut gone: Vec<usize> = Vec::new();
    for f in fixes {
        match f {
            Fix::GeneralizeMany { items } => {
                for (id, s) in items {
                    to.insert(*id, s);
                }
            }
            Fix::SuppressRows { rows, .. } => gone.extend(rows),
            other => panic!("correction inattendue : {other:?}"),
        }
    }
    t.rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let clues = t
                .roles
                .iter()
                .zip(r)
                .filter(|(role, _)| matches!(role, Clue(_)))
                .map(|(_, c)| {
                    c.value_id
                        .and_then(|id| to.get(&id))
                        .map_or(c.value.clone(), |s| s.to_string())
                })
                .collect();
            (clues, gone.contains(&i))
        })
        .collect()
}

/// Taille du groupe de chaque ligne non supprimée, et si la ligne est supprimée.
fn group_sizes(_t: &TableInput, after: &[(Vec<String>, bool)]) -> Vec<(usize, bool)> {
    let mut counts: HashMap<&Vec<String>, usize> = HashMap::new();
    for (k, gone) in after {
        if !gone {
            *counts.entry(k).or_insert(0) += 1;
        }
    }
    after
        .iter()
        .map(|(k, gone)| (counts.get(k).copied().unwrap_or(0), *gone))
        .collect()
}

/// Une valeur déjà rendue moins précise (« années 1940 ») ne redevient jamais
/// plus précise (« août 1946 ») quand le plan généralise sa colonne.
#[test]
fn plan_never_makes_a_value_more_precise() {
    let roles = [Clue(DateNaiss)];
    let old: Vec<String> = (1..=10).map(|d| format!("{d:02}/08/1946")).collect();
    let recent: Vec<String> = (1..=10).map(|d| format!("{d:02}/03/1985")).collect();
    let rows: Vec<[&str; 1]> = old.iter().chain(&recent).map(|v| [v.as_str()]).collect();
    let rows: Vec<&[&str]> = rows.iter().map(|r| r.as_slice()).collect();
    let mut t = table(&roles, &rows);
    for r in t.rows.iter_mut().take(10) {
        r[0].shown = Shown::Generalized("années 1940".into());
    }
    let old_ids: Vec<usize> = t
        .rows
        .iter()
        .take(10)
        .filter_map(|r| r[0].value_id)
        .collect();
    let a = assess(&input(vec![t]), &settings(5));
    let fixes: Vec<&Fix> = a.failures.iter().flat_map(|f| &f.fixes).collect();
    let Some(Fix::GeneralizeMany { items }) = fixes.first() else {
        panic!("{fixes:?}");
    };
    assert_eq!(items.len(), 10, "seules les dates de 1985 changent");
    assert!(
        items
            .iter()
            .all(|(id, to)| !old_ids.contains(id) && to == "mars 1985")
    );
}

#[test]
fn few_outliers_are_suppressed_without_generalizing() {
    let roles = [Clue(Age)];
    let mut rows: Vec<&[&str]> = vec![&["40 ans"]; 60];
    rows.push(&["97 ans"]);
    let t = table(&roles, &rows);
    let a = assess(&input(vec![t]), &settings(5));
    let f = a
        .failures
        .iter()
        .find(|f| matches!(f.reason, FailureReason::SmallGroup { estimate: 1 }))
        .unwrap();
    assert_eq!(f.fixes.len(), 1, "{:?}", f.fixes);
    let Fix::SuppressRows {
        table,
        rows,
        value_ids,
    } = &f.fixes[0]
    else {
        panic!("{:?}", f.fixes);
    };
    assert_eq!((*table, rows.as_slice()), (0, &[60usize][..]));
    assert_eq!(value_ids.len(), 1);
}

/// Les valeurs courantes (« M », « F ») ne sont jamais masquées par la
/// suppression d'une ligne : seules les valeurs rares le sont.
#[test]
fn suppressed_rows_only_mask_rare_values() {
    let roles = [Clue(Autre), Clue(Age)];
    let mut rows: Vec<&[&str]> = vec![&["M", "40 ans"]; 30];
    rows.extend(vec![&["F", "41 ans"][..]; 30]);
    rows.push(&["M", "97 ans"]);
    let t = table(&roles, &rows);
    let id_m = t.rows[0][0].value_id.unwrap();
    let id_f = t.rows[30][0].value_id.unwrap();
    let id_age = t.rows[60][1].value_id.unwrap();
    let a = assess(&input(vec![t]), &settings(5));
    let ids: Vec<usize> = a
        .failures
        .iter()
        .flat_map(|f| &f.fixes)
        .filter_map(|f| match f {
            Fix::SuppressRows { value_ids, .. } => Some(value_ids.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!ids.is_empty(), "{:?}", a.failures);
    assert!(!ids.contains(&id_m) && !ids.contains(&id_f), "{ids:?}");
    assert!(ids.contains(&id_age), "{ids:?}");
}

#[test]
fn direct_and_stable_identifiers() {
    let roles = [Direct(Nom), Stable(EntityKind::Matricule), Clue(Autre)];
    let rows: Vec<[&str; 3]> = (0..5)
        .map(|i| {
            [
                ["Dupont", "Martin", "Kaloï", "Garnier", "Nguyen"][i],
                "",
                "F",
            ]
        })
        .collect();
    let mut rows: Vec<&[&str]> = rows.iter().map(|r| r.as_slice()).collect();
    rows.push(&["", "AB-1234", "F"]);
    let mut t = table(&roles, &rows);
    let a = assess(&input(vec![t.clone()]), &settings(5));
    let direct: Vec<_> = a.failures_of(Criterion::Isolation).collect();
    assert_eq!(direct.len(), 1);
    assert_eq!(
        direct[0].reason,
        FailureReason::DirectIdentifier { kind: Nom }
    );
    assert_eq!(direct[0].fixes.len(), 5, "un masquage par nom");
    let linkage: Vec<_> = a.failures_of(Criterion::Linkage).collect();
    assert_eq!(
        linkage[0].reason,
        FailureReason::StableIdentifier {
            kind: EntityKind::Matricule
        }
    );

    // Une fois masqués, plus d'échec.
    for r in &mut t.rows {
        for c in r.iter_mut().take(2) {
            c.shown = Shown::Masked(Tag::new(Nom, 1));
        }
    }
    assert!(assess(&input(vec![t]), &settings(5)).is_anonymous());
}

#[test]
fn value_without_id_cannot_be_fixed() {
    let roles = [Direct(Nom), Clue(Age)];
    let mut t = table(&roles, &[&["Dupont", "40 ans"], &["Martin", "41 ans"]]);
    t.rows[0][0].value_id = None;
    t.rows[1][0].value_id = None;
    t.rows[1][1].value_id = None;
    let a = assess(&input(vec![t]), &settings(2));
    assert!(!a.failures.is_empty());
    for f in &a.failures {
        assert!(f.fixes.is_empty(), "{f:?}");
    }
}

#[test]
fn images_and_verdicts() {
    let rect = PixelRect {
        x: 1,
        y: 2,
        w: 30,
        h: 40,
    };
    let unchecked = ImageInput {
        file: 1,
        media: "page 1".into(),
        faces: None,
    };
    let with_faces = ImageInput {
        file: 2,
        media: "ppt/media/image1.png".into(),
        faces: Some(vec![rect, rect]),
    };
    let no_face = ImageInput {
        file: 3,
        media: "image.png".into(),
        faces: Some(Vec::new()),
    };
    let only = |images: Vec<ImageInput>| {
        assess(
            &AssessInput {
                images,
                ..AssessInput::default()
            },
            &settings(5),
        )
    };
    let a = only(vec![unchecked.clone(), no_face.clone()]);
    assert_eq!(a.verdict, Verdict::Inconclusive);
    assert_eq!(a.subjects.len(), 1);
    assert_eq!(a.failures[0].reason, FailureReason::FacesNotChecked);

    let a = only(vec![unchecked, with_faces]);
    assert_eq!(a.verdict, Verdict::Reidentifiable { subjects: 1 });
    let face = a
        .failures
        .iter()
        .find(|f| f.reason == FailureReason::FaceVisible)
        .unwrap();
    assert_eq!(face.fixes.len(), 2, "une zone par visage");
    assert!(matches!(face.fixes[0], Fix::MaskRegion { file: 2, .. }));

    assert_eq!(only(vec![no_face]).verdict, Verdict::Anonymous);
    assert_eq!(only(Vec::new()).verdict, Verdict::Anonymous);
}

#[test]
fn k_min_is_clamped_and_kept() {
    let roles = [Clue(Autre)];
    let t = table(&roles, &[&["F"][..]; 3]);
    let a = assess(&input(vec![t.clone()]), &settings(1));
    assert_eq!(a.settings.k_min, 2);
    assert!(a.is_anonymous());
    let a = assess(&input(vec![t]), &settings(500));
    assert_eq!(a.settings.k_min, 20);
    assert!(!a.is_anonymous());
}

#[test]
fn empty_and_ragged_tables_do_not_panic() {
    let roles = [Clue(Age), Sensitive(Sante), Direct(Nom)];
    let mut t = table(&roles, &[]);
    assert!(
        assess(&input(vec![t.clone()]), &settings(5))
            .subjects
            .is_empty()
    );
    t.rows = vec![
        vec![],
        vec![TableCell {
            value: "40 ans".into(),
            value_id: Some(1),
            shown: Shown::Visible,
        }],
    ];
    let a = assess(&input(vec![t]), &settings(5));
    assert_eq!(a.subjects.len(), 2);
}

#[test]
fn debug_hides_values() {
    let roles = [Clue(Lieu), Sensitive(Sante)];
    let t = table(&roles, &[&["Farino", "sclérose"]]);
    let a = assess(&input(vec![t]), &settings(5));
    let s = format!("{a:?}");
    for secret in ["Farino", "sclérose", "province", "Feuille"] {
        assert!(!s.contains(secret), "{s}");
    }
}
