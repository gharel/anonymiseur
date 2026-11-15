//! Tests de l'évaluation des textes sur des cas fictifs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::assess;
use anon_core::{AssessInput, AssessSettings, Assessment, ContextAnswers, Tag, Verdict};

use EntityKind::{
    Age as AgeK, DateNaiss, Email, Fonction, Lieu, Matricule, Nom, Personne, Prenom, Sante, Societe,
};

fn visible(id: usize, kind: EntityKind, value: &str) -> Mention {
    Mention {
        value_id: id,
        kind,
        value: value.into(),
        shown: Shown::Visible,
    }
}

fn masked(id: usize, kind: EntityKind, value: &str, index: u32) -> Mention {
    Mention {
        value_id: id,
        kind,
        value: value.into(),
        shown: Shown::Masked(Tag::new(kind, index)),
    }
}

fn generalized(id: usize, kind: EntityKind, value: &str, to: &str) -> Mention {
    Mention {
        value_id: id,
        kind,
        value: value.into(),
        shown: Shown::Generalized(to.into()),
    }
}

fn unit(mentions: Vec<Mention>) -> TextUnit {
    TextUnit { file: 0, mentions }
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

/// Applique une correction comme le moteur : toutes les occurrences de la
/// valeur changent ; un masquage reçoit une balise neuve.
fn apply(units: &mut [TextUnit], fix: &Fix) {
    let set = |units: &mut [TextUnit], id: usize, shown: &dyn Fn(EntityKind) -> Shown| {
        for m in units.iter_mut().flat_map(|u| u.mentions.iter_mut()) {
            if m.value_id == id {
                m.shown = shown(m.kind);
            }
        }
    };
    match fix {
        Fix::Mask { value_id } => {
            let index = 1000 + u32::try_from(*value_id).unwrap();
            set(units, *value_id, &|k| Shown::Masked(Tag::new(k, index)));
        }
        Fix::Generalize { value_id, to } => {
            set(units, *value_id, &|_| Shown::Generalized(to.clone()));
        }
        Fix::GeneralizeMany { items } => {
            for (id, to) in items {
                set(units, *id, &|_| Shown::Generalized(to.clone()));
            }
        }
        other => panic!("correction inattendue pour un texte : {other:?}"),
    }
}

fn person(a: &Assessment, value_id: usize) -> &SubjectReport {
    a.subjects
        .iter()
        .find(|s| s.subject == SubjectRef::Person { value_id })
        .unwrap_or_else(|| panic!("sujet {value_id} absent : {:?}", a.subjects))
}

fn small_group(a: &Assessment) -> Option<&Failure> {
    a.failures
        .iter()
        .find(|f| matches!(f.reason, FailureReason::SmallGroup { .. }))
}

/// « Jean Dupont, responsable technique de Nautilus Pacifique SARL » : une
/// fonction dans une organisation nommée désigne une seule personne.
#[test]
fn named_role_in_a_named_company_is_a_group_of_one() {
    let units = vec![unit(vec![
        masked(0, Personne, "Jean Dupont", 1),
        visible(1, Fonction, "responsable technique"),
        visible(2, Societe, "Nautilus Pacifique SARL"),
    ])];
    let a = run(&units, 5);
    let s = person(&a, 0);
    assert_eq!(s.group_estimate, Some(1));
    assert_eq!(s.passed, [false, true, true]);
    assert_eq!(a.verdict, Verdict::Reidentifiable { subjects: 1 });
    let f = small_group(&a).unwrap();
    assert_eq!(f.reason, FailureReason::SmallGroup { estimate: 1 });
    // Conseillé : masquer la société (un cran) ; ensuite élargir la fonction.
    assert_eq!(f.fixes[0], Fix::Mask { value_id: 2 });
    assert!(
        f.fixes.contains(&Fix::Generalize {
            value_id: 1,
            to: "professionnel".into()
        }),
        "{:?}",
        f.fixes
    );
    // La société masquée, la personne n'est plus isolable.
    let mut fixed = units.clone();
    apply(&mut fixed, &f.fixes[0]);
    let b = run(&fixed, 5);
    println!(
        "Nautilus : groupe 1, puis {:?} une fois la société masquée",
        person(&b, 0).group_estimate
    );
    assert!(b.is_anonymous(), "{:?}", b.failures);
    // Les indices encore visibles sont listés (ce qu'une IA peut croiser).
    assert_eq!(s.clues, vec![(Fonction, 1), (Societe, 2)]);
}

/// « infirmière, Farino, 62 ans » : groupe estimé sous 5 ; la correction
/// conseillée rend la commune moins précise (province Sud).
#[test]
fn nurse_in_a_small_commune_with_exact_age() {
    let units = vec![unit(vec![
        masked(0, Personne, "Mme Kaloï", 1),
        visible(1, Fonction, "infirmière"),
        visible(2, Lieu, "Farino"),
        visible(3, AgeK, "62 ans"),
    ])];
    let a = run(&units, 5);
    let e = person(&a, 0).group_estimate.unwrap();
    println!("Infirmière, Farino, 62 ans : groupe estimé {e}");
    assert!(e < 5, "{e}");
    let f = small_group(&a).unwrap();
    assert_eq!(
        f.fixes[0],
        Fix::Generalize {
            value_id: 2,
            to: "province Sud".into()
        }
    );
    let mut fixed = units.clone();
    apply(&mut fixed, &f.fixes[0]);
    let b = run(&fixed, 5);
    let after = person(&b, 0).group_estimate.unwrap();
    println!("  après « province Sud » : {after}");
    assert!(after >= 5 && after > e);
    assert!(b.is_anonymous(), "{:?}", b.failures);
}

/// Un seuil plus haut exige de toucher plusieurs indices : `GeneralizeMany`.
#[test]
fn several_clues_generalized_together() {
    let units = vec![unit(vec![
        masked(0, Personne, "Mme Kaloï", 1),
        visible(1, Fonction, "infirmière"),
        visible(2, Lieu, "Farino"),
        visible(3, AgeK, "62 ans"),
    ])];
    let k = 20;
    let a = run(&units, k);
    let f = small_group(&a).unwrap();
    let mut fixed = units.clone();
    apply(&mut fixed, &f.fixes[0]);
    let b = run(&fixed, k);
    let after = person(&b, 0).group_estimate.unwrap();
    println!("k = {k} : {:?} -> {after}", f.fixes[0]);
    assert!(after >= u64::from(k));
    assert!(
        matches!(f.fixes[0], Fix::GeneralizeMany { .. }),
        "{:?}",
        f.fixes
    );
}

/// « une habitante de Nouméa » : grand groupe, la personne passe.
#[test]
fn resident_of_a_city_passes() {
    let units = vec![unit(vec![
        masked(0, Personne, "Mme Durand", 3),
        visible(1, Lieu, "Nouméa"),
    ])];
    let a = run(&units, 5);
    let e = person(&a, 0).group_estimate.unwrap();
    println!("Habitante de Nouméa : groupe estimé {e}");
    assert!(e > 50_000, "{e}");
    assert!(a.is_anonymous(), "{:?}", a.failures);
    assert_eq!(person(&a, 0).clues, vec![(Lieu, 1)]);
}

/// Donnée de santé liée à une personne isolable : la déduction échoue ; la même
/// donnée liée à une personne non isolable passe.
#[test]
fn health_data_fails_only_for_an_isolable_person() {
    let isolable = vec![unit(vec![
        masked(0, Personne, "Mme Kaloï", 1),
        visible(1, Fonction, "infirmière"),
        visible(2, Lieu, "Farino"),
        visible(3, AgeK, "62 ans"),
        visible(4, Sante, "atteinte d'un diabète"),
    ])];
    let a = run(&isolable, 5);
    let s = person(&a, 0);
    assert_eq!(s.passed, [false, true, false]);
    let inference: Vec<&Failure> = a.failures_of(Criterion::Inference).collect();
    assert_eq!(inference.len(), 1);
    assert_eq!(
        inference[0].reason,
        FailureReason::SensitiveLinked { kind: Sante }
    );
    assert_eq!(inference[0].fixes, vec![Fix::Mask { value_id: 4 }]);

    let crowd = vec![unit(vec![
        masked(0, Personne, "Mme Durand", 3),
        visible(1, Lieu, "Nouméa"),
        visible(4, Sante, "atteinte d'un diabète"),
    ])];
    let b = run(&crowd, 5);
    assert_eq!(person(&b, 0).passed, [true; 3]);
    assert!(b.is_anonymous(), "{:?}", b.failures);
    assert!(person(&b, 0).clues.contains(&(Sante, 4)));
}

/// Un nom visible : jamais anonyme.
#[test]
fn visible_name_is_a_direct_identifier() {
    let units = vec![unit(vec![
        visible(0, Personne, "Jean Dupont"),
        visible(1, Lieu, "Nouméa"),
        visible(2, Sante, "en arrêt maladie"),
    ])];
    let a = run(&units, 5);
    assert!(!a.is_anonymous());
    let f = a.failures_of(Criterion::Isolation).next().unwrap();
    assert_eq!(f.reason, FailureReason::DirectIdentifier { kind: Personne });
    assert_eq!(f.fixes, vec![Fix::Mask { value_id: 0 }]);
    // Isolable par son nom : la donnée de santé échoue aussi à la déduction.
    assert!(
        a.failures
            .iter()
            .any(|f| f.reason == FailureReason::SensitiveLinked { kind: Sante })
    );
    // Un prénom seul, même rendu moins précis, reste un identifiant direct.
    let units = vec![unit(vec![generalized(0, Prenom, "Jean", "J.")])];
    assert!(!run(&units, 5).is_anonymous());
    // Un email visible rattaché à une personne masquée aussi.
    let units = vec![unit(vec![
        masked(0, Personne, "Jean Dupont", 1),
        visible(1, Email, "jean.dupont@example.com"),
    ])];
    let a = run(&units, 5);
    assert_eq!(
        a.failures[0].reason,
        FailureReason::DirectIdentifier { kind: Email }
    );
}

#[test]
fn no_person_means_no_subject() {
    let units = vec![
        unit(vec![
            visible(0, Lieu, "Farino"),
            visible(1, Sante, "diabète"),
            visible(2, Matricule, "MAT-004517"),
        ]),
        unit(Vec::new()),
    ];
    let a = run(&units, 5);
    assert!(a.subjects.is_empty());
    assert_eq!(a.verdict, Verdict::Anonymous);
    assert_eq!(run(&[], 5).verdict, Verdict::Anonymous);
}

#[test]
fn stable_identifier_fails_linkage() {
    let units = vec![unit(vec![
        masked(0, Personne, "Jean Dupont", 1),
        visible(1, Lieu, "Nouméa"),
        visible(2, Matricule, "MAT-004517"),
    ])];
    let a = run(&units, 5);
    assert_eq!(person(&a, 0).passed, [true, false, true]);
    let f = a.failures_of(Criterion::Linkage).next().unwrap();
    assert_eq!(
        f.reason,
        FailureReason::StableIdentifier { kind: Matricule }
    );
    assert_eq!(f.fixes, vec![Fix::Mask { value_id: 2 }]);
}

/// Mentions d'une même personne regroupées par l'indice de leur balise
/// (`[PERSONNE_1]`, `[NOM_1]`), d'un paragraphe à l'autre.
#[test]
fn mentions_grouped_by_tag_index() {
    let units = vec![
        unit(vec![
            masked(5, Nom, "Dupont", 1),
            visible(1, AgeK, "62 ans"),
        ]),
        unit(vec![
            masked(0, Personne, "Jean Dupont", 1),
            visible(2, Lieu, "Farino"),
        ]),
        unit(vec![
            masked(7, Personne, "Marie Durand", 2),
            visible(3, Lieu, "Nouméa"),
        ]),
    ];
    let a = run(&units, 5);
    assert_eq!(a.subjects.len(), 2);
    // Désignée par sa valeur `Personne`, même vue après le nom.
    let jean = person(&a, 0);
    assert_eq!(jean.clues, vec![(AgeK, 1), (Lieu, 2)]);
    let marie = person(&a, 7);
    assert_eq!(marie.clues, vec![(Lieu, 3)]);
}

/// Plusieurs personnes dans un paragraphe : chaque indice va à la plus proche,
/// la précédente à égale distance.
#[test]
fn clues_go_to_the_nearest_person() {
    let units = vec![unit(vec![
        masked(0, Personne, "Jean Dupont", 1),
        visible(1, AgeK, "62 ans"),
        masked(2, Personne, "Marie Durand", 2),
        visible(3, Fonction, "infirmière"),
        visible(4, Lieu, "Farino"),
    ])];
    let a = run(&units, 5);
    assert_eq!(person(&a, 0).clues, vec![(AgeK, 1)]);
    assert_eq!(person(&a, 2).clues, vec![(Fonction, 3), (Lieu, 4)]);
}

#[test]
fn generalized_forms_are_read() {
    let exact = vec![unit(vec![
        masked(0, Personne, "X", 1),
        visible(1, Lieu, "Nouméa"),
        visible(2, AgeK, "62 ans"),
    ])];
    let band = vec![unit(vec![
        masked(0, Personne, "X", 1),
        visible(1, Lieu, "Nouméa"),
        generalized(2, AgeK, "62 ans", "60-69 ans"),
    ])];
    let e1 = person(&run(&exact, 5), 0).group_estimate.unwrap();
    let e2 = person(&run(&band, 5), 0).group_estimate.unwrap();
    assert!(e2 > 5 * e1, "{e1} / {e2}");
    // Lieu rendu moins précis : la population de la province.
    let province = vec![unit(vec![
        masked(0, Personne, "X", 1),
        generalized(1, Lieu, "Farino", "province Sud"),
    ])];
    let e = person(&run(&province, 5), 0).group_estimate.unwrap();
    assert_eq!(e, area("province Sud").unwrap().population);
    // Date de naissance complète : bien plus précise que l'année seule.
    let full = vec![unit(vec![
        masked(0, Personne, "X", 1),
        visible(1, Lieu, "Nouméa"),
        visible(2, DateNaiss, "12/08/1946"),
    ])];
    let year = vec![unit(vec![
        masked(0, Personne, "X", 1),
        visible(1, Lieu, "Nouméa"),
        generalized(2, DateNaiss, "12/08/1946", "1946"),
    ])];
    let f = person(&run(&full, 5), 0).group_estimate.unwrap();
    let y = person(&run(&year, 5), 0).group_estimate.unwrap();
    assert!(y > 100 * f.max(1) || f == 1, "{f} / {y}");
    assert!(birth_measure("août 1946") != birth_measure("12/08/1946"));
    assert_eq!(
        age_measure("plus de 60 ans"),
        Measure::Ages {
            from: 60,
            to: AGE_MAX,
            per_year: 1.0
        }
    );
    assert_eq!(
        age_measure("60-69 ans"),
        Measure::Ages {
            from: 60,
            to: 69,
            per_year: 1.0
        }
    );
    assert_eq!(age_measure("âge inconnu"), Measure::Nothing);
    assert_eq!(
        birth_measure("années 1940"),
        Measure::Ages {
            from: age_of_birth_year(1949),
            to: age_of_birth_year(1940),
            per_year: 1.0
        }
    );
}

/// En V2.0, l'approche simplifiée s'applique quelles que soient les réponses.
#[test]
fn verdict_ignores_context_answers() {
    let units = vec![unit(vec![
        masked(0, Personne, "Jean Dupont", 1),
        visible(1, Fonction, "responsable technique"),
        visible(2, Societe, "Nautilus Pacifique SARL"),
    ])];
    let input = AssessInput {
        texts: units,
        ..AssessInput::default()
    };
    let settings = AssessSettings {
        answers: ContextAnswers {
            recipient_knows_people: Some(false),
            published: Some(false),
            other_documents: Some(false),
        },
        ..AssessSettings::default()
    };
    let contextual = assess(&input, &settings);
    let simplified = assess(&input, &AssessSettings::default());
    assert_eq!(contextual.verdict, simplified.verdict);
    assert_eq!(contextual.failures, simplified.failures);
}

/// Les journaux et `Debug` ne montrent aucune valeur.
#[test]
fn debug_hides_values() {
    let units = vec![unit(vec![
        masked(0, Personne, "Jean Dupont", 1),
        visible(1, Lieu, "Farino"),
        visible(2, Fonction, "infirmière"),
        visible(3, AgeK, "62 ans"),
    ])];
    let a = run(&units, 5);
    let s = format!("{a:?}");
    for v in ["Jean", "Farino", "infirmi", "62 ans", "province"] {
        assert!(!s.contains(v), "{v} dans {s}");
    }
}
