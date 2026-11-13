//! Tests qui ont besoin des modèles dans `assets/ner/` (préparés par
//! `tools/model/prepare_assets.py`). S'ils manquent, les tests échouent avec la commande
//! de préparation : ils ne sont jamais ignorés en silence.
#![cfg(feature = "ia")]
// Fichier de test : les assistants hors `#[test]` peuvent paniquer ; les mesures
// affichent leur détail (rappel, précision, manqués) avec `--nocapture`.
#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::panic)]

use anon_core::detect::{DetectOptions, Detector, Profile};
use anon_core::entities::{EntityKind, Source};
use anon_core::text::{BlockContext, BlockRole, TextBlock};
use anon_ner::{Bundle, NerDetector, PREPARE_COMMAND, Span};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Seuils minimaux (en %) du détecteur livré (aiguillage + union), mesurés en Rust.
const MIN_RECALL_PERSONS_FR_CONTROL: f64 = 98.0;
const MIN_PRECISION_PERSONS_FR_CONTROL: f64 = 92.0;
const MIN_RECALL_PERSONS_OTHER_CONTROL: f64 = 94.0;
const MIN_PRECISION_PERSONS_OTHER_CONTROL: f64 = 90.0;
const MIN_RECALL_PERSONS_EXAMPLES: f64 = 90.0;
const MIN_PRECISION_PERSONS_EXAMPLES: f64 = 93.0;
/// Part minimale (en %) de phrases du jeu de contrôle où Rust et Python donnent
/// exactement les mêmes entités (ONNX Runtime 1.31.0 côté Rust, 1.31.x côté Python).
const MIN_PYTHON_AGREEMENT: f64 = 97.0;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn detector() -> &'static NerDetector {
    static D: OnceLock<NerDetector> = OnceLock::new();
    D.get_or_init(|| {
        let dir = root().join("assets/ner");
        let d = NerDetector::from_dir(&dir).unwrap_or_else(|e| {
            panic!(
                "modèles absents de {} : {e}\nPréparer avec : {PREPARE_COMMAND}",
                dir.display()
            )
        });
        for b in Bundle::ALL {
            assert!(
                d.has(b),
                "modèle {b:?} absent ; préparer avec : {PREPARE_COMMAND}"
            );
        }
        d.warm_up()
            .unwrap_or_else(|e| panic!("chargement impossible : {e}"));
        d
    })
}

fn block(text: &str) -> TextBlock {
    TextBlock {
        text: text.to_string(),
        ctx: BlockContext::new("test", BlockRole::Body),
    }
}

fn renforce() -> DetectOptions {
    DetectOptions {
        profile: Profile::Renforce,
        ..Default::default()
    }
}

fn check_offsets(text: &str, spans: &[Span]) {
    for s in spans {
        assert!(s.start < s.end && s.end <= text.len(), "{s:?}");
        assert!(
            text.is_char_boundary(s.start) && text.is_char_boundary(s.end),
            "{s:?}"
        );
    }
}

fn persons(text: &str, spans: &[Span]) -> Vec<String> {
    spans
        .iter()
        .filter(|s| s.kind == EntityKind::Personne)
        .map(|s| text[s.start..s.end].to_string())
        .collect()
}

#[test]
fn empty_and_blank_texts() {
    let d = detector();
    for t in ["", " ", "\n\t  ", "😀"] {
        let r = d.find(t).unwrap();
        check_offsets(t, &r);
    }
    assert!(d.detect(&block(""), &renforce()).is_empty());
}

#[test]
fn accented_names_have_exact_byte_offsets() {
    let d = detector();
    let text = "Le dossier de Mme Hélène Kaloï a été transmis à M. Trần Văn Minh à Nouméa.";
    let r = d.find(text).unwrap();
    check_offsets(text, &r);
    let p = persons(text, &r);
    assert!(p.contains(&"Hélène Kaloï".to_string()), "{p:?}");
    assert!(p.iter().any(|x| x.contains("Minh")), "{p:?}");
}

#[test]
fn emoji_do_not_shift_offsets() {
    let d = detector();
    let text = "😀🎉 Réunion avec Pierre Garnier 👍 puis avec Marie-Claire de La Fontaine 🙂.";
    let r = d.find(text).unwrap();
    check_offsets(text, &r);
    let p = persons(text, &r);
    assert!(p.contains(&"Pierre Garnier".to_string()), "{p:?}");
    assert!(p.iter().any(|x| x.starts_with("Marie-Claire")), "{p:?}");
}

#[test]
fn very_long_text_uses_windows_and_keeps_offsets() {
    let d = detector();
    let filler =
        "Le paiement doit intervenir sous trente jours à compter de la réception de la facture. ";
    let mut text = String::from("Le contrat est signé par Jean Dupont. ");
    for _ in 0..120 {
        text.push_str(filler);
    }
    text.push_str("Le dossier est suivi par Hinatea Teriitahi. ");
    for _ in 0..120 {
        text.push_str(filler);
    }
    text.push_str("Fin du rapport rédigé par Olivier Nguyen.");
    let r = d.find(&text).unwrap();
    check_offsets(&text, &r);
    let p = persons(&text, &r);
    for name in ["Jean Dupont", "Hinatea Teriitahi", "Olivier Nguyen"] {
        assert!(p.contains(&name.to_string()), "{name} absent de {p:?}");
    }
}

#[test]
fn detector_contract() {
    let d = detector();
    assert_eq!(d.name(), "ner");
    let b = block("Le contrat est signé entre Atelier Bougainville SAS et Jean Dupont, à Lyon.");
    let all = d.detect(&b, &renforce());
    assert!(all.iter().all(|x| x.source == Source::Ner));
    assert!(all.iter().any(|x| x.kind == EntityKind::Personne));
    assert!(all.iter().any(|x| x.kind == EntityKind::Lieu));
    // Profil standard : pas de lieu (quasi-identifiant).
    let std = d.detect(&b, &DetectOptions::default());
    assert!(std.iter().all(|x| x.kind != EntityKind::Lieu));
    // Type désactivé.
    let mut opts = renforce();
    opts.disabled.insert(EntityKind::Personne);
    assert!(
        d.detect(&b, &opts)
            .iter()
            .all(|x| x.kind != EntityKind::Personne)
    );
}

#[test]
fn parallel_detection_matches_sequential() {
    let d = detector();
    let texts: Arc<Vec<String>> = Arc::new(
        load_jsonl("tools/model/controle/controle.jsonl")
            .into_iter()
            .take(24)
            .map(|v| v["text"].as_str().unwrap_or_default().to_string())
            .collect(),
    );
    let expected: Vec<Vec<Span>> = texts.iter().map(|t| d.find(t).unwrap()).collect();
    let handles: Vec<_> = (0..6)
        .map(|k| {
            let texts = Arc::clone(&texts);
            std::thread::spawn(move || {
                let d = detector();
                (0..texts.len())
                    .map(|i| (i + k) % texts.len())
                    .map(|i| (i, d.find(&texts[i]).unwrap()))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for h in handles {
        for (i, got) in h.join().unwrap() {
            assert_eq!(got, expected[i], "texte {i}");
        }
    }
}

// ---------------------------------------------------------------- mesures

fn load_jsonl(rel: &str) -> Vec<Value> {
    let p = root().join(rel);
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{} : {e} ; préparer avec : {PREPARE_COMMAND}", p.display()));
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[derive(Default, Debug)]
struct Stat {
    occ: usize,
    found: usize,
    pred: usize,
    good: usize,
}

impl Stat {
    fn recall(&self) -> f64 {
        if self.occ == 0 {
            100.0
        } else {
            100.0 * self.found as f64 / self.occ as f64
        }
    }
    fn precision(&self) -> f64 {
        if self.pred == 0 {
            100.0
        } else {
            100.0 * self.good as f64 / self.pred as f64
        }
    }
}

fn is_person_type(t: &str) -> bool {
    matches!(t, "PERSONNE" | "NOM" | "PRENOM")
}

/// Mêmes règles que `tools/model/bench.py` : rappel strict (tous les caractères
/// alphanumériques de l'occurrence couverts), précision par chevauchement ou inclusion.
fn score(
    text: &str,
    expected: &[(String, String)],
    spans: &[Span],
    st: &mut Stat,
    misses: &mut Vec<String>,
) {
    let preds: Vec<&Span> = spans
        .iter()
        .filter(|s| s.kind == EntityKind::Personne)
        .collect();
    let mut occs = Vec::new();
    for (t, v) in expected {
        if !is_person_type(t) || v.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(i) = text[from..].find(v.as_str()) {
            let s = from + i;
            occs.push((s, s + v.len()));
            from = s + v.len();
        }
    }
    for &(s, e) in &occs {
        st.occ += 1;
        let ok = text[s..e]
            .char_indices()
            .filter(|(_, c)| c.is_alphanumeric())
            .all(|(i, _)| preds.iter().any(|p| p.start <= s + i && s + i < p.end));
        if ok {
            st.found += 1;
        } else {
            misses.push(format!("manqué « {} » dans « {} »", &text[s..e], text));
        }
    }
    for p in preds {
        st.pred += 1;
        let ptxt = &text[p.start..p.end];
        let ok = occs.iter().any(|&(s, e)| s < p.end && p.start < e)
            || expected
                .iter()
                .any(|(t, v)| is_person_type(t) && v.contains(ptxt));
        if ok {
            st.good += 1;
        } else {
            misses.push(format!("faux positif « {ptxt} »"));
        }
    }
}

fn expected_of(v: &Value) -> Vec<(String, String)> {
    v["entites"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    (
                        e["type"].as_str().unwrap_or_default().to_string(),
                        e["valeur"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn measure(rel: &str) -> BTreeMap<String, Stat> {
    let d = detector();
    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    let mut misses = Vec::new();
    for v in load_jsonl(rel) {
        let text = v["text"].as_str().unwrap_or_default();
        let lang = match v["lang"].as_str() {
            Some("fr") => "fr",
            _ => "autres",
        };
        let spans = d.find(text).unwrap();
        check_offsets(text, &spans);
        score(
            text,
            &expected_of(&v),
            &spans,
            stats.entry(lang.into()).or_default(),
            &mut misses,
        );
    }
    println!("== {rel}");
    for (lang, s) in &stats {
        println!(
            "  personnes {lang:6} rappel {:5.1} % ({}/{})  précision {:5.1} % ({}/{})",
            s.recall(),
            s.found,
            s.occ,
            s.precision(),
            s.good,
            s.pred
        );
    }
    for m in &misses {
        println!("  {m}");
    }
    stats
}

#[test]
fn control_set_recall_and_precision() {
    let st = measure("tools/model/controle/controle.jsonl");
    let fr = &st["fr"];
    let other = &st["autres"];
    assert!(
        fr.recall() >= MIN_RECALL_PERSONS_FR_CONTROL,
        "rappel FR {:.1}",
        fr.recall()
    );
    assert!(
        fr.precision() >= MIN_PRECISION_PERSONS_FR_CONTROL,
        "précision FR {:.1}",
        fr.precision()
    );
    assert!(
        other.recall() >= MIN_RECALL_PERSONS_OTHER_CONTROL,
        "rappel autres {:.1}",
        other.recall()
    );
    assert!(
        other.precision() >= MIN_PRECISION_PERSONS_OTHER_CONTROL,
        "précision autres {:.1}",
        other.precision()
    );
}

#[test]
fn examples_recall_and_precision() {
    let st = measure("tools/model/controle/exemples_blocs.jsonl");
    let mut all = Stat::default();
    for s in st.values() {
        all.occ += s.occ;
        all.found += s.found;
        all.pred += s.pred;
        all.good += s.good;
    }
    println!(
        "  total : rappel {:.1} %, précision {:.1} %",
        all.recall(),
        all.precision()
    );
    assert!(
        all.recall() >= MIN_RECALL_PERSONS_EXAMPLES,
        "rappel {:.1}",
        all.recall()
    );
    assert!(
        all.precision() >= MIN_PRECISION_PERSONS_EXAMPLES,
        "précision {:.1}",
        all.precision()
    );
}

#[test]
fn rust_and_python_agree_on_control_set() {
    let d = detector();
    let control: BTreeMap<String, String> = load_jsonl("tools/model/controle/controle.jsonl")
        .into_iter()
        .map(|v| {
            (
                v["id"].as_str().unwrap_or_default().to_string(),
                v["text"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    for b in Bundle::ALL {
        let rel = format!("tools/model/controle/reference_{}.json", b.file_stem());
        let p = root().join(&rel);
        let refs: Value = serde_json::from_str(
            &std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("{rel} : {e} ; {PREPARE_COMMAND}")),
        )
        .unwrap();
        let refs = refs.as_array().unwrap();
        let mut same = 0;
        for r in refs {
            let id = r["id"].as_str().unwrap();
            let text = &control[id];
            let want: Vec<(String, String)> = r["entites"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["kind"].as_str().unwrap().to_string(),
                        e["valeur"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            let got: Vec<(String, String)> = d
                .find_with(b, text)
                .unwrap()
                .iter()
                .map(|s| {
                    (
                        s.kind.prefix().to_string(),
                        text[s.start..s.end].to_string(),
                    )
                })
                .collect();
            if got == want {
                same += 1;
            } else {
                println!("[{b:?}] {id} : Python {want:?} / Rust {got:?}");
            }
        }
        let pct = 100.0 * same as f64 / refs.len() as f64;
        println!(
            "[{b:?}] concordance Rust/Python : {same}/{} ({pct:.1} %)",
            refs.len()
        );
        assert!(
            pct >= MIN_PYTHON_AGREEMENT,
            "[{b:?}] concordance {pct:.1} %"
        );
    }
}
