//! Qu'apporte l'IA ? Mesure comparée du moteur livré, édition complète (IA de noms)
//! contre édition Lite (règles, dictionnaires, colonnes, quasi-identifiants), sur :
//!
//! - le jeu de contrôle indépendant `tools/model/controle/controle.jsonl` : rappel et
//!   précision des personnes, sociétés et lieux, par langue ;
//! - tous les fichiers de `exemples/` (PDF et images compris, valeurs OCR comprises) :
//!   rappel et précision par type, pièges `ne_pas_detecter`, et valeurs encore lisibles
//!   après anonymisation avec la sélection par défaut (profils Standard et Renforcé).
//!
//! Compilé avec la fonctionnalité `ia`, le test mesure les deux éditions ; sans elle,
//! seulement l'édition Lite. Les scores sont comparés à `tests/baseline-ia.json` : le
//! test échoue si un score baisse au-delà de [`TOLERANCE_POINTS`] (ou si une valeur de
//! plus reste lisible). Mise à jour explicite : `ANON_BASELINE_IA_UPDATE=1`.
//! Affichage : `cargo test -p anonymiseur --test detection -- --nocapture`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use anon_core::{DetectOptions, EntityKind, Profile, Session};
use anon_engine::{AnalyzeOptions, AnonymizeOptions, Candidate, Engine, InputFile, NoProgress};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Baisse tolérée d'un rappel ou d'une précision, en points de pourcentage.
const TOLERANCE_POINTS: f64 = 1.0;
/// Hausse tolérée du nombre de valeurs lisibles ou de pièges détectés.
const TOLERANCE_COUNT: f64 = 0.0;
/// Variable d'environnement qui autorise la réécriture de la référence.
const UPDATE_ENV: &str = "ANON_BASELINE_IA_UPDATE";
const BASELINE: &str = "tests/baseline-ia.json";

type Scores = BTreeMap<String, f64>;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn lite() -> Engine {
    Engine::standard_builder()
        .handlers_from(anon_pdf::handler_for)
        .image_processor(Box::new(anon_pdf::MupdfImageProcessor::new()))
        .build()
}

/// Éditions mesurées dans ce binaire de test.
fn editions() -> Vec<(&'static str, Engine)> {
    #[allow(unused_mut)]
    let mut v = vec![("lite", lite())];
    #[cfg(feature = "ia")]
    {
        let m = anonymiseur::moteur::build();
        assert_eq!(
            m.ia,
            anonymiseur::moteur::IaStatus::Active,
            "modèles d'IA non chargés : uv run --project tools/model tools/model/prepare_assets.py"
        );
        v.push(("complete", m.engine));
    }
    v
}

// ------------------------------------------------------------------ outils

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// Vrai si `needle` apparaît dans `hay` comme suite de mots entiers (casse ignorée).
fn contains_words(hay: &str, needle: &str) -> bool {
    let (h, n) = (fold(hay), fold(needle));
    if n.is_empty() {
        return false;
    }
    h.match_indices(&n).any(|(i, m)| {
        let before = h[..i].chars().next_back();
        let after = h[i + m.len()..].chars().next();
        let glued = |c: Option<char>, edge: Option<char>| {
            matches!((c, edge), (Some(c), Some(e)) if c.is_alphanumeric() && e.is_alphanumeric())
        };
        !glued(before, n.chars().next()) && !glued(after, n.chars().next_back())
    })
}

fn family(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Personne | EntityKind::Nom | EntityKind::Prenom => "personnes",
        EntityKind::Societe => "societes",
        EntityKind::Lieu | EntityKind::Adresse => "lieux",
        _ => "autres",
    }
}

/// Type de comparaison pour les exemples : les parties de personne sont confondues.
fn type_key(kind: EntityKind) -> &'static str {
    if kind.is_person_part() {
        "PERSONNE"
    } else {
        kind.prefix()
    }
}

fn forms(c: &Candidate) -> impl Iterator<Item = &String> {
    std::iter::once(&c.value).chain(c.variants.iter())
}

/// Une valeur attendue est trouvée si une ligne cochée de même type en couvre le texte.
fn found(cands: &[&Candidate], same: impl Fn(EntityKind) -> bool, value: &str) -> bool {
    cands
        .iter()
        .filter(|c| same(c.kind))
        .any(|c| forms(c).any(|f| contains_words(f, value)))
}

/// Une ligne cochée est juste si elle recouvre (dans un sens ou l'autre) une valeur attendue de même type.
fn correct(
    c: &Candidate,
    expected: &[(EntityKind, String)],
    same: impl Fn(EntityKind, EntityKind) -> bool,
) -> bool {
    expected.iter().any(|(k, v)| {
        same(c.kind, *k) && forms(c).any(|f| contains_words(f, v) || contains_words(v, f))
    })
}

#[derive(Default, Clone, Copy)]
struct Count {
    occ: usize,
    hit: usize,
    pred: usize,
    good: usize,
}

impl Count {
    fn recall(&self) -> f64 {
        100.0 * self.hit as f64 / self.occ.max(1) as f64
    }
    fn precision(&self) -> f64 {
        if self.pred == 0 {
            100.0
        } else {
            100.0 * self.good as f64 / self.pred as f64
        }
    }
}

fn renforce() -> AnalyzeOptions {
    AnalyzeOptions {
        detect: DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        },
        ..Default::default()
    }
}

// --------------------------------------------------------- jeu de contrôle

fn control(engine: &Engine, scores: &mut Scores, fps: &mut Vec<String>) {
    let raw = std::fs::read_to_string(root().join("tools/model/controle/controle.jsonl")).unwrap();
    let mut counts: BTreeMap<(String, &str), Count> = BTreeMap::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).unwrap();
        let text = v["text"].as_str().unwrap();
        let lang = match v["lang"].as_str().unwrap() {
            l @ ("fr" | "en") => l.to_string(),
            _ => "autres".to_string(),
        };
        let expected: Vec<(EntityKind, String)> = v["entites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    EntityKind::from_prefix(e["type"].as_str().unwrap()).unwrap(),
                    e["valeur"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let analysis = engine.analyze_text(text, &renforce(), &Session::new());
        let selected: Vec<&Candidate> = analysis.candidates.iter().filter(|c| c.selected).collect();
        for fam in ["personnes", "societes", "lieux"] {
            let n = counts.entry((lang.clone(), fam)).or_default();
            for (k, val) in expected.iter().filter(|(k, _)| family(*k) == fam) {
                let _ = k;
                n.occ += 1;
                n.hit += usize::from(found(&selected, |c| family(c) == fam, val));
            }
            for c in selected.iter().filter(|c| family(c.kind) == fam) {
                n.pred += 1;
                if correct(c, &expected, |a, b| family(a) == family(b)) {
                    n.good += 1;
                } else {
                    fps.push(format!("contrôle {fam} « {} »", c.value));
                }
            }
        }
    }
    for ((lang, fam), n) in counts {
        scores.insert(format!("controle/{lang}/{fam}/rappel"), n.recall());
        scores.insert(format!("controle/{lang}/{fam}/precision"), n.precision());
    }
}

// ----------------------------------------------------------------- exemples

struct Example {
    name: String,
    expected: Vec<(EntityKind, String, bool)>, // type, valeur, profil renforcé
    traps: Vec<String>,
}

fn examples(engine: &Engine) -> Vec<Example> {
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("exemples/attendu.json")).unwrap(),
    )
    .unwrap();
    let ext = engine.supported_extensions();
    v["fichiers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            let n = f["fichier"].as_str().unwrap();
            n != "reponse-ia-exemple.md" && ext.contains(&n.rsplit('.').next().unwrap())
        })
        .map(|f| Example {
            name: f["fichier"].as_str().unwrap().to_string(),
            expected: f["entites"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        EntityKind::from_prefix(e["type"].as_str().unwrap()).unwrap(),
                        e["valeur"].as_str().unwrap().to_string(),
                        e["profil"] == "renforce",
                    )
                })
                .collect(),
            traps: f["ne_pas_detecter"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| t.as_str().unwrap().to_string())
                .collect(),
        })
        .collect()
}

fn inputs(list: &[Example]) -> Vec<InputFile> {
    list.iter()
        .map(|e| {
            InputFile::new(
                &e.name,
                std::fs::read(root().join("exemples").join(&e.name)).unwrap(),
            )
        })
        .collect()
}

fn example_detection(engine: &Engine, scores: &mut Scores, fps: &mut Vec<String>) {
    let list = examples(engine);
    let files = inputs(&list);
    let analysis = engine.analyze(&files, &renforce(), &Session::new(), &NoProgress);
    let mut counts: BTreeMap<&str, Count> = BTreeMap::new();
    let mut traps = 0usize;
    for (i, ex) in list.iter().enumerate() {
        let selected: Vec<&Candidate> = analysis
            .candidates
            .iter()
            .filter(|c| c.selected && c.files.iter().any(|f| *f == files[i].name))
            .collect();
        let expected: Vec<(EntityKind, String)> = ex
            .expected
            .iter()
            .map(|(k, v, _)| (*k, v.clone()))
            .collect();
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for (k, v, _) in &ex.expected {
            if seen.contains(&(type_key(*k), v.as_str())) {
                continue;
            }
            seen.push((type_key(*k), v.as_str()));
            let n = counts.entry(type_key(*k)).or_default();
            n.occ += 1;
            n.hit += usize::from(found(&selected, |c| type_key(c) == type_key(*k), v));
        }
        for c in &selected {
            let n = counts.entry(type_key(c.kind)).or_default();
            n.pred += 1;
            if correct(c, &expected, |a, b| type_key(a) == type_key(b)) {
                n.good += 1;
            } else {
                fps.push(format!("{} {} « {} »", ex.name, type_key(c.kind), c.value));
            }
            if ex
                .traps
                .iter()
                .any(|t| forms(c).any(|f| contains_words(t, f) || contains_words(f, t)))
            {
                traps += 1;
            }
        }
    }
    for (t, n) in counts {
        if n.occ == 0 {
            continue;
        }
        scores.insert(format!("exemples/{t}/rappel"), n.recall());
        scores.insert(format!("exemples/{t}/precision"), n.precision());
    }
    scores.insert("exemples/pieges_detectes".into(), traps as f64);
}

/// Valeurs attendues encore lisibles dans les fichiers produits, sélection par défaut,
/// sans correction humaine. La relecture passe par une analyse des fichiers produits
/// avec la liste « toujours anonymiser » remplie des valeurs attendues (OCR compris).
fn leftovers(engine: &Engine, profile: Profile, report: &mut Vec<String>) -> (usize, usize) {
    let list = examples(engine);
    let files = inputs(&list);
    let mut session = Session::new();
    let opts = AnalyzeOptions {
        detect: DetectOptions {
            profile,
            ..Default::default()
        },
        ..Default::default()
    };
    let analysis = engine.analyze(&files, &opts, &session, &NoProgress);
    engine.apply_selection(&analysis, &mut session);
    let probe = lite();
    let (mut total, mut left) = (0usize, 0usize);
    for (file, ex) in files.iter().zip(&list) {
        let wanted: Vec<&(EntityKind, String, bool)> = ex
            .expected
            .iter()
            .filter(|(_, _, r)| profile == Profile::Renforce || !r)
            .collect();
        total += wanted.len();
        let out =
            match engine.anonymize(file, &analysis, &mut session, &AnonymizeOptions::default()) {
                Ok(a) => a,
                Err(e) => {
                    report.push(format!("{profile:?} {} refusé : {e}", ex.name));
                    left += wanted.len();
                    continue;
                }
            };
        let always: Vec<(String, EntityKind)> =
            wanted.iter().map(|(k, v, _)| (v.clone(), *k)).collect();
        let reread = probe.analyze(
            &[InputFile::new(&out.name, out.bytes)],
            &AnalyzeOptions {
                detect: DetectOptions {
                    profile: Profile::Renforce,
                    ..Default::default()
                },
                always,
                never: Vec::new(),
            },
            &Session::new(),
            &NoProgress,
        );
        for (k, v, _) in wanted {
            let visible = reread.candidates.iter().any(|c| {
                c.occurrences > 0
                    && forms(c).any(|f| {
                        contains_words(f, v)
                            && !ex
                                .traps
                                .iter()
                                .any(|t| contains_words(t, f) && !contains_words(v, t))
                    })
            });
            if visible {
                left += 1;
                report.push(format!(
                    "{profile:?} {} : {} « {v} » lisible",
                    ex.name,
                    k.prefix()
                ));
            }
        }
    }
    (left, total)
}

// ----------------------------------------------------------------- référence

fn load_baseline() -> Option<BTreeMap<String, Scores>> {
    let raw = std::fs::read_to_string(root().join(BASELINE)).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let mut out = BTreeMap::new();
    for (ed, m) in v["editions"].as_object()? {
        let s: Scores = m
            .as_object()?
            .iter()
            .filter_map(|(k, x)| Some((k.clone(), x.as_f64()?)))
            .collect();
        out.insert(ed.clone(), s);
    }
    Some(out)
}

fn save_baseline(all: &BTreeMap<String, Scores>) {
    let mut eds = serde_json::Map::new();
    for (ed, s) in all {
        let m: serde_json::Map<String, Value> = s
            .iter()
            .map(|(k, v)| (k.clone(), Value::from((v * 10.0).round() / 10.0)))
            .collect();
        eds.insert(ed.clone(), Value::Object(m));
    }
    let doc = serde_json::json!({
        "_note": "Scores du moteur livré (app/tests/detection.rs). Rappels et précisions en %, valeurs lisibles et pièges en nombre. Mise à jour : ANON_BASELINE_IA_UPDATE=1.",
        "tolerance_points": TOLERANCE_POINTS,
        "editions": Value::Object(eds),
    });
    std::fs::write(
        root().join(BASELINE),
        serde_json::to_string_pretty(&doc).unwrap() + "\n",
    )
    .unwrap();
}

/// Plus haut est meilleur pour les rappels et précisions, plus bas pour le reste.
fn higher_is_better(key: &str) -> bool {
    key.ends_with("/rappel") || key.ends_with("/precision")
}

#[test]
fn ce_que_l_ia_apporte() {
    let mut measured: BTreeMap<String, Scores> = BTreeMap::new();
    for (name, engine) in editions() {
        let mut s = Scores::new();
        let mut fps = Vec::new();
        let mut notes = Vec::new();
        control(&engine, &mut s, &mut fps);
        example_detection(&engine, &mut s, &mut fps);
        for p in [Profile::Standard, Profile::Renforce] {
            let (l, t) = leftovers(&engine, p, &mut notes);
            let key = format!("lisibles/{}", format!("{p:?}").to_lowercase());
            s.insert(key.clone(), l as f64);
            s.insert(format!("{key}/total"), t as f64);
        }
        println!("\n== édition {name} : faux positifs cochés");
        for f in &fps {
            println!("   {f}");
        }
        println!("== édition {name} : valeurs encore lisibles");
        for n in &notes {
            println!("   {n}");
        }
        measured.insert(name.to_string(), s);
    }

    // Tableau côte à côte.
    let keys: std::collections::BTreeSet<&String> =
        measured.values().flat_map(|s| s.keys()).collect();
    println!("\n{:<44} {:>9} {:>9}", "mesure", "lite", "complète");
    for k in keys {
        let get = |e: &str| {
            measured
                .get(e)
                .and_then(|s| s.get(k))
                .map_or("-".to_string(), |v| format!("{v:.1}"))
        };
        println!("{k:<44} {:>9} {:>9}", get("lite"), get("complete"));
    }

    let update = std::env::var_os(UPDATE_ENV).is_some();
    let baseline = load_baseline();
    if update {
        let mut all = baseline.unwrap_or_default();
        for (e, s) in &measured {
            all.insert(e.clone(), s.clone());
        }
        save_baseline(&all);
        println!("\nréférence {BASELINE} mise à jour");
        return;
    }
    let baseline = baseline.unwrap_or_else(|| {
        panic!("{BASELINE} absent : lancer une fois avec {UPDATE_ENV}=1 pour l'écrire")
    });
    let mut regressions = Vec::new();
    for (ed, s) in &measured {
        let Some(base) = baseline.get(ed) else {
            regressions.push(format!(
                "édition {ed} absente de la référence ({UPDATE_ENV}=1)"
            ));
            continue;
        };
        for (k, old) in base {
            if k.ends_with("/total") {
                continue;
            }
            let Some(new) = s.get(k) else {
                regressions.push(format!("{ed} {k} : mesure disparue"));
                continue;
            };
            let worse = if higher_is_better(k) {
                *new < old - TOLERANCE_POINTS
            } else {
                *new > old + TOLERANCE_COUNT
            };
            if worse {
                regressions.push(format!("{ed} {k} : {old:.1} -> {new:.1}"));
            }
        }
    }
    assert!(
        regressions.is_empty(),
        "régressions :\n{}",
        regressions.join("\n")
    );
}
