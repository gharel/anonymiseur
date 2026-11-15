//! Évaluation des trois critères branchée dans le moteur, pilotée par
//! `exemples/evaluation.json` ; corrections et annulation ; objectif
//! « Anonymiser définitivement » (R7) ; fiche d'évaluation (Markdown et PDF).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{
    AssessSettings, Assessment, ColumnRole, Criterion, EntityKind, FailureReason, Fix, Goal,
    PixelRect, Profile, Session, Shown, Source, Verdict,
};
use anon_engine::{
    AnonymizeOptions, Engine, FaceFinder, FicheMeta, InputFile, NoProgress, render_markdown,
};
use anon_replace::SessionExt;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Durée maximale d'une évaluation sur un fichier d'exemple (release). En
/// compilation de développement, le code n'est pas optimisé.
const MAX_ASSESS: Duration = if cfg!(debug_assertions) {
    Duration::from_secs(5)
} else {
    Duration::from_secs(1)
};

/// Moteur livré sans IA : bureautique, PDF, images et OCR, pas de visages.
fn engine() -> Engine {
    builder().build()
}

fn builder() -> anon_engine::EngineBuilder {
    Engine::standard_builder()
        .handlers_from(anon_pdf::handler_for)
        .image_processor(Box::new(anon_pdf::MupdfImageProcessor::new()))
}

fn profile(name: &str) -> Profile {
    match name {
        "standard" => Profile::Standard,
        "renforce" => Profile::Renforce,
        other => panic!("profil inconnu : {other}"),
    }
}

fn goal(name: &str) -> Goal {
    match name {
        "masquer" => Goal::Mask,
        "anonymiser" => Goal::Anonymize,
        other => panic!("objectif inconnu : {other}"),
    }
}

fn verdict_name(v: Verdict) -> &'static str {
    match v {
        Verdict::Anonymous => "anonyme",
        Verdict::Reidentifiable { .. } => "reidentifiable",
        Verdict::Inconclusive => "non_concluant",
    }
}

fn settings(goal: Goal) -> AssessSettings {
    AssessSettings {
        goal,
        ..AssessSettings::default()
    }
}

/// Compare une évaluation à son attendu (`verdict`, `sujets_en_echec`, `echecs`).
fn check(a: &Assessment, expected: &Value, label: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let verdict = verdict_name(a.verdict);
    if expected["verdict"] != verdict {
        errors.push(format!(
            "{label} : verdict {verdict}, attendu {}",
            expected["verdict"]
        ));
    }
    let failing = a.subjects.iter().filter(|s| !s.passes()).count();
    if expected["sujets_en_echec"].as_u64() != Some(failing as u64) {
        errors.push(format!(
            "{label} : {failing} sujets en échec, attendu {}",
            expected["sujets_en_echec"]
        ));
    }
    for (c, key) in [
        (Criterion::Isolation, "isolement"),
        (Criterion::Linkage, "recoupement"),
        (Criterion::Inference, "deduction"),
    ] {
        let n = a.summary(c).1;
        if expected["echecs"][key].as_u64() != Some(n as u64) {
            errors.push(format!(
                "{label} : {key} {n} en échec, attendu {}",
                expected["echecs"][key]
            ));
        }
    }
    errors
}

#[test]
fn evaluation_des_exemples() {
    let raw = std::fs::read_to_string(common::exemples().join("evaluation.json")).unwrap();
    let doc: Value = serde_json::from_str(&raw).unwrap();
    let engine = engine();
    let mut errors = Vec::new();
    let cases = doc["cas"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["fichier"].as_str().unwrap();
        let file = common::input(name);
        let p = profile(case["profil"].as_str().unwrap());
        let g = goal(case["objectif"].as_str().unwrap());
        let label = format!("{name} ({p:?}, {g:?})");
        let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), p);
        let s = settings(g);
        let a = engine.assess(&analysis, &s);
        errors.extend(check(&a, case, &label));
        // La fiche ne reprend aucune valeur du dossier.
        let md = render_markdown(&a, &FicheMeta::new(&analysis, &a, "2026-11-15", false));
        for c in &analysis.candidates {
            if c.value.chars().filter(|ch| ch.is_alphabetic()).count() >= 3
                && md.contains(c.value.as_str())
            {
                errors.push(format!(
                    "{label} : une valeur ({:?}) figure dans la fiche",
                    c.kind
                ));
            }
        }
        if let Some(after) = case.get("apres_tout_corriger") {
            let undo = engine.fix_all(&mut analysis, &s);
            assert!(!undo.is_empty(), "{label} : rien à corriger");
            let a = engine.assess(&analysis, &s);
            errors.extend(check(
                &a,
                after,
                &format!("{label}, après « Tout corriger »"),
            ));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// Profil Standard : les indices (lieu, fonction, âge) sont trouvés pour
/// l'évaluation mais laissés décochés ; le profil Renforcé les coche.
#[test]
fn indices_du_profil_standard_decoches() {
    let engine = engine();
    let file = common::input("compte-rendu-commission.docx");
    let standard = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let renforce = common::analyze(&engine, std::slice::from_ref(&file), Profile::Renforce);
    for (kind, value) in [
        (EntityKind::Lieu, "Farino"),
        (EntityKind::Fonction, "responsable technique"),
        (EntityKind::Age, "62 ans"),
    ] {
        let find = |a: &anon_engine::Analysis| {
            a.candidates
                .iter()
                .find(|c| {
                    c.kind == kind
                        && std::iter::once(&c.value)
                            .chain(&c.variants)
                            .any(|v| anon_replace::key(v) == anon_replace::key(value))
                })
                .map(|c| c.selected)
        };
        let seen: Vec<(&str, bool)> = standard
            .candidates
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| (c.value.as_str(), c.selected))
            .collect();
        assert_eq!(
            find(&standard),
            Some(false),
            "{kind:?} en profil Standard : {seen:?}"
        );
        assert_eq!(find(&renforce), Some(true), "{kind:?} en profil Renforcé");
    }
    // Ce qui est coché ne change pas : aucune valeur de ces types.
    assert!(
        standard
            .selected()
            .all(|c| !c.kind.is_quasi_identifier() || c.kind.is_sensitive())
    );
}

/// Après « Tout corriger » (la première correction de chaque échec, puis
/// celles qui restent), `patients.xlsx` passe l'isolement ; une seule
/// annulation redonne l'état d'avant.
#[test]
fn tout_corriger_patients() {
    let engine = engine();
    let file = common::input("patients.xlsx");
    let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let s = settings(Goal::Anonymize);
    let before = engine.assess(&analysis, &s);
    assert!(before.summary(Criterion::Isolation).1 > 0);
    let snapshot = analysis.candidates.clone();

    let undo = engine.fix_all(&mut analysis, &s);
    let after = engine.assess(&analysis, &s);
    assert_eq!(
        after.summary(Criterion::Isolation).1,
        0,
        "{:?}",
        after.failures
    );
    assert!(after.is_anonymous(), "{:?}", after.failures);
    let t = analysis.techniques();
    assert!(!t.generalized.is_empty() && t.suppressed_rows > 0, "{t:?}");

    engine.undo_fix(&mut analysis, undo);
    assert!(analysis.candidates == snapshot);
    assert_eq!(engine.assess(&analysis, &s), before);
}

/// Chaque cellule non vide d'une colonne évaluée porte une valeur de
/// l'analyse ; les valeurs ajoutées pour l'évaluation sont décochées.
#[test]
fn cellules_et_roles_de_patients() {
    let engine = engine();
    let file = common::input("patients.xlsx");
    let analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let input = engine.assess_input(&analysis);
    assert!(
        input.texts.is_empty(),
        "les cellules évaluées ne sont pas des textes"
    );
    let [table] = input.tables.as_slice() else {
        panic!("un tableau attendu : {:?}", input.tables);
    };
    assert_eq!(
        table.roles,
        vec![
            ColumnRole::Direct(EntityKind::Nom),
            ColumnRole::Clue(EntityKind::Age),
            ColumnRole::Clue(EntityKind::Lieu),
            ColumnRole::Clue(EntityKind::Lieu),
            ColumnRole::Clue(EntityKind::Autre),
            ColumnRole::Sensitive(EntityKind::Sante),
        ]
    );
    assert_eq!(table.rows.len(), 24);
    for row in &table.rows {
        for cell in row {
            let id = cell.value_id.expect("valeur attendue pour chaque cellule");
            assert!(analysis.candidate(id).is_some());
        }
        // Profil Standard : noms masqués, âges visibles (non cochés).
        assert!(matches!(row[0].shown, Shown::Masked(_)), "{:?}", row[0]);
        assert_eq!(row[1].shown, Shown::Visible);
        let age = analysis.candidate(row[1].value_id.unwrap()).unwrap();
        assert!(!age.selected && age.source == Source::Column);
    }
}

/// Moins d'une seconde par évaluation sur chaque fichier d'exemple.
#[test]
fn evaluation_rapide_sur_chaque_exemple() {
    let engine = engine();
    for f in common::handled(&engine) {
        let file = common::input(&f.name);
        let analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Renforce);
        for g in Goal::ALL {
            let t = Instant::now();
            let _ = engine.assess(&analysis, &settings(*g));
            let d = t.elapsed();
            assert!(d < MAX_ASSESS, "{} : {d:?}", f.name);
        }
    }
}

/// Détecteur de visages factice : un visage au même endroit de chaque image.
struct OneFace;

const FACE: PixelRect = PixelRect {
    x: 20,
    y: 20,
    w: 60,
    h: 70,
};

impl FaceFinder for OneFace {
    fn faces(&self, _image: &[u8]) -> Option<Vec<PixelRect>> {
        Some(vec![FACE])
    }
}

#[test]
fn visage_masque_par_une_correction() {
    let engine = builder().face_finder(Box::new(OneFace)).build();
    assert!(engine.checks_faces());
    let file = common::input("carte-visite.png");
    let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let s = settings(Goal::Anonymize);
    let a = engine.assess(&analysis, &s);
    let face = a
        .failures
        .iter()
        .find(|f| f.reason == FailureReason::FaceVisible)
        .expect("visage attendu");
    assert!(!a.is_anonymous());
    let fix = face.fixes.first().cloned().unwrap();
    assert!(matches!(&fix, Fix::MaskRegion { rect, .. } if *rect == FACE));

    let anonymize = |analysis: &anon_engine::Analysis| {
        let mut session = Session::new();
        engine.apply_selection(analysis, &mut session);
        engine
            .anonymize(&file, analysis, &mut session, &AnonymizeOptions::default())
            .unwrap()
            .report
            .replacements
    };
    let without = anonymize(&analysis);
    let undo = engine.apply_fix(&mut analysis, &fix);
    assert_eq!(analysis.masks().len(), 1);
    assert!(analysis.masks()[0].from_fix);
    let a = engine.assess(&analysis, &s);
    assert!(
        a.failures
            .iter()
            .all(|f| f.reason != FailureReason::FaceVisible),
        "{:?}",
        a.failures
    );
    // La zone est appliquée à l'anonymisation.
    assert_eq!(anonymize(&analysis), without + 1);
    engine.undo_fix(&mut analysis, undo);
    assert!(analysis.masks().is_empty());
}

/// Objectif « Anonymiser définitivement » (R7) : aucune balise reprise d'une
/// clé précédente.
#[test]
fn anonymiser_ne_reprend_pas_les_balises() {
    let engine = engine();
    let file = common::input("contrat-prestation.docx");
    let previous = |g: Goal| {
        let mut s = Session {
            goal: g,
            ..Session::default()
        };
        // Personnes fictives d'un envoi précédent : « Jean Dupont » y avait la balise 41.
        for i in 1..=40 {
            s.intern(
                EntityKind::Personne,
                &format!("Témoin Numéro {i}"),
                Source::Manual,
            );
        }
        let tag = s.intern(EntityKind::Personne, "Jean Dupont", Source::Manual);
        assert_eq!(tag.map(|t| t.to_string()).as_deref(), Some("[PERSONNE_41]"));
        s
    };
    let text_of = |bytes: &[u8]| {
        anon_office::handler_for("docx")
            .unwrap()
            .deep_scan(bytes)
            .unwrap()
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("\n")
    };
    for g in Goal::ALL {
        let mut session = previous(*g);
        let analysis = engine.analyze(
            std::slice::from_ref(&file),
            &common::opts(Profile::Standard),
            &session,
            &NoProgress,
        );
        engine.apply_selection(&analysis, &mut session);
        let out = engine
            .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
            .unwrap();
        let reused = text_of(&out.bytes).contains("[PERSONNE_41]");
        match g {
            Goal::Mask => assert!(reused, "la clé est reprise en objectif Masquer"),
            Goal::Anonymize => {
                assert!(!reused, "balise d'une clé précédente reprise");
                assert!(analysis.candidates.iter().all(|c| c.existing_tag.is_none()));
                assert!(
                    !session
                        .entities
                        .iter()
                        .any(|e| e.value == "Témoin Numéro 1")
                );
            }
        }
    }
}

/// Objectif « Anonymiser définitivement » (R7) : aucune image d'origine gardée.
#[test]
fn anonymiser_ne_garde_aucune_image_d_origine() {
    let engine = engine();
    let file = common::input("carte-visite.png");
    for g in Goal::ALL {
        let mut session = Session {
            goal: *g,
            ..Session::default()
        };
        let analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
        engine.apply_selection(&analysis, &mut session);
        engine
            .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
            .unwrap();
        match g {
            Goal::Mask => assert!(!session.media.is_empty()),
            Goal::Anonymize => assert!(session.media.is_empty()),
        }
    }
}

/// Le nom des fichiers produits suit l'objectif et le verdict (invariant 9) :
/// en « Anonymiser définitivement », le dossier est réévalué au moment de
/// l'anonymisation, jamais pris d'un verdict périmé.
#[test]
fn noms_selon_objectif_et_verdict() {
    let engine = engine();
    let file = common::input("patients.xlsx");
    let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let produce = |analysis: &anon_engine::Analysis, g: Goal| {
        let mut session = Session {
            goal: g,
            ..Session::default()
        };
        engine.apply_selection(analysis, &mut session);
        engine
            .anonymize(&file, analysis, &mut session, &AnonymizeOptions::default())
            .unwrap()
            .name
    };
    let s = settings(Goal::Anonymize);
    assert!(!engine.assess(&analysis, &s).is_anonymous());
    assert_eq!(produce(&analysis, Goal::Mask), "patients_masque.xlsx");
    assert_eq!(
        produce(&analysis, Goal::Anonymize),
        "patients_masque_sans_cle.xlsx"
    );
    let undo = engine.fix_all(&mut analysis, &s);
    assert_eq!(engine.assess(&analysis, &s).verdict, Verdict::Anonymous);
    assert_eq!(produce(&analysis, Goal::Anonymize), "patients_anonyme.xlsx");
    assert_eq!(produce(&analysis, Goal::Mask), "patients_masque.xlsx");
    // Réglages plus stricts que ceux de l'évaluation : plus « anonyme ».
    let strict = AssessSettings { k_min: 20, ..s };
    let mut session = Session {
        goal: Goal::Anonymize,
        ..Session::default()
    };
    engine.apply_selection(&analysis, &mut session);
    let out = engine
        .anonymize_with(
            &file,
            &analysis,
            &mut session,
            &AnonymizeOptions::default(),
            &strict,
        )
        .unwrap();
    assert_eq!(out.name, "patients_masque_sans_cle.xlsx");
    // Une correction annulée : le verdict n'est plus le même, le nom non plus.
    engine.undo_fix(&mut analysis, undo);
    assert_eq!(
        produce(&analysis, Goal::Anonymize),
        "patients_masque_sans_cle.xlsx"
    );
}

// ---------------------------------------------------------------------------
// Fiche d'évaluation
// ---------------------------------------------------------------------------

/// Fiche de `patients.xlsx` (profil Standard, objectif Masquer), date et
/// version fixées, avant et après « Tout corriger ».
fn fiche_patients() -> (String, String, Vec<String>) {
    let engine = engine();
    let file = common::input("patients.xlsx");
    let mut analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let s = settings(Goal::Mask);
    let render = |analysis: &anon_engine::Analysis| {
        let a = engine.assess(analysis, &s);
        let mut meta = FicheMeta::new(analysis, &a, "2026-11-15", false);
        meta.tool_version = "x.y.z".into();
        render_markdown(&a, &meta)
    };
    let before = render(&analysis);
    engine.fix_all(&mut analysis, &s);
    let after = render(&analysis);
    // Valeurs du dossier (lettres, au moins trois caractères) et formes
    // généralisées : aucune ne doit figurer dans la fiche.
    let mut secrets: Vec<String> = analysis
        .candidates
        .iter()
        .flat_map(|c| std::iter::once(c.value.clone()).chain(c.variants.iter().cloned()))
        .chain(
            analysis
                .candidates
                .iter()
                .filter_map(|c| match &c.treatment {
                    anon_core::Treatment::Generalize(g) => Some(g.clone()),
                    anon_core::Treatment::Tag => None,
                }),
        )
        .filter(|v| v.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        .collect();
    secrets.sort();
    secrets.dedup();
    assert!(secrets.iter().any(|s| s == "Farino"));
    assert!(secrets.iter().any(|s| s.contains("province")));
    (before, after, secrets)
}

#[test]
fn fiche_sans_valeur_ni_forme_generalisee() {
    let (before, after, secrets) = fiche_patients();
    for md in [&before, &after] {
        for s in &secrets {
            assert!(!md.contains(s.as_str()), "« {s} » dans la fiche");
        }
    }
    // Invariant 9 : « anonyme » seulement si le verdict le permet.
    let folded = |s: &str| anon_replace::key(s);
    assert!(!folded(&before).contains("anonyme"), "{before}");
    assert!(folded(&after).contains("anonyme pour un assistant ia grand public"));
    for title in [
        "# Fiche d'évaluation",
        "## Conclusion",
        "## Techniques appliquées",
        "## Résultat par critère",
        "## Réévaluation",
        "## Responsabilité",
    ] {
        assert!(before.contains(title), "{title}");
    }
    assert!(before.contains("15/11/2027"), "date de réévaluation");
    assert!(before.contains("lignes directrices 02/2026 du CEPD"));
    assert!(after.contains("Suppression de lignes : 1 ligne"));
    assert!(after.contains("Âge, valeur remplacée par « * »"));
}

#[test]
fn fiche_patients_instantane() {
    let (before, _, _) = fiche_patients();
    insta::assert_snapshot!("fiche_patients", before);
}

/// Un champ saisi qui qualifie le résultat d'« anonyme » est neutralisé tant
/// que le verdict ne le permet pas ; les noms de fichiers n'apparaissent que
/// sur demande.
#[test]
fn fiche_champs_saisis_et_noms() {
    let engine = engine();
    let file = InputFile::new("dossier-Farino.xlsx", common::input("patients.xlsx").bytes);
    let analysis = common::analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let a = engine.assess(&analysis, &settings(Goal::Anonymize));
    let mut meta = FicheMeta::new(&analysis, &a, "2026-11-15", false);
    meta.legal_basis = "Intérêt légitime".into();
    meta.risks_justification = "Les données sont anonymes".into();
    let md = render_markdown(&a, &meta);
    assert!(md.contains("Base légale : Intérêt légitime"));
    assert!(md.contains("Les données sont [terme retiré]"));
    assert!(!md.contains("Farino"));
    assert!(!format!("{meta:?}").contains("Intérêt"));
    let meta = FicheMeta::new(&analysis, &a, "2026-11-15", true);
    assert!(render_markdown(&a, &meta).contains("dossier-Farino.xlsx"));
}

/// PDF de la fiche : il se rouvre, contient les titres et aucune valeur du dossier.
#[test]
fn fiche_en_pdf() {
    let (before, after, secrets) = fiche_patients();
    for md in [&before, &after] {
        let pdf = anon_pdf::render_fiche_pdf(md).unwrap();
        if let Some(dir) = std::env::var_os("ANON_FICHE_OUT") {
            std::fs::write(std::path::Path::new(&dir).join("fiche.pdf"), &pdf).unwrap();
        }
        let text = anon_pdf::handler_for("pdf")
            .unwrap()
            .extract(&pdf)
            .unwrap()
            .blocks
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("\n");
        for title in [
            "Fiche d'évaluation",
            "Conclusion",
            "Techniques appliquées",
            "Résultat par critère",
            "Sujets évalués",
            "Réévaluation",
            "Responsabilité",
        ] {
            assert!(text.contains(title), "{title} absent du PDF");
        }
        for s in &secrets {
            assert!(!text.contains(s.as_str()), "« {s} » dans le PDF");
        }
    }
}
