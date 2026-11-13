//! Parcours de l'interface de bout en bout sur les vrais exemples, sans egui :
//! état des onglets + opérations du fil de travail + moteur livré.
//!
//! analyser `exemples/` → sélection par défaut → anonymiser dans un dossier
//! temporaire → fichiers écrits et `.anonmap` → réouverture avec le mot de passe
//! → restauration ; mauvais mot de passe ; fuite simulée ; fichier refusé ;
//! non-écrasement ; annulation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{
    BlockContext, BlockRole, Extraction, FormatError, Handler, Output, Session, TextBlock,
    TextTransform, TransformOptions,
};
use anon_engine::{Engine, NoProgress, Progress};
use anonymiseur::ui::platform::file_entry;
use anonymiseur::ui::service::{
    MAP_BASENAME, Outcome, Protection, anonymize_batch, read_inputs, restore_batch,
};
use anonymiseur::ui::state::anonymize::{AnonymizeState, ProtectMode, Step};
use anonymiseur::ui::state::deanonymize::DeanonymizeState;
use anonymiseur::ui::state::files::OUTPUT_DIR_NAME;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

const PASSWORD: &str = "Cheval-Agrafe-Batterie-47";

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| anonymiseur::moteur::build().engine)
}

fn exemples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exemples")
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anonymiseur-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Exemples gérés par le moteur (sans la réponse d'IA simulée ni les notices).
fn example_paths() -> Vec<PathBuf> {
    let supported = engine().supported_extensions();
    let mut out: Vec<PathBuf> = std::fs::read_dir(exemples())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            supported.contains(&ext.as_str())
                && !name.starts_with("reponse-ia")
                && name != "LISEZMOI.md"
                && ext != "json"
        })
        .collect();
    out.sort();
    out
}

/// Copie des exemples dans un dossier de travail (sortie par défaut : sous-dossier
/// `Anonymisés` du dossier du premier fichier).
fn copy_examples(dir: &Path) -> Vec<PathBuf> {
    example_paths()
        .into_iter()
        .map(|p| {
            let to = dir.join(p.file_name().unwrap());
            std::fs::copy(&p, &to).unwrap();
            to
        })
        .collect()
}

/// Analyse par l'état de l'onglet, comme le fait `AnonApp`.
fn analyzed(paths: Vec<PathBuf>, session: &Session, progress: &dyn Progress) -> AnonymizeState {
    let mut st = AnonymizeState::default();
    let supported = engine().supported_extensions();
    st.add_files(paths.into_iter().map(file_entry), &supported);
    let to_read = st.begin_analysis().unwrap();
    let (inputs, rejected) = read_inputs(&to_read);
    let analysis = engine().analyze(&inputs, &st.analyze_options(), session, progress);
    st.analysis_done(inputs, rejected, analysis, session);
    st
}

fn anonymize(st: &mut AnonymizeState, session: &mut Session, progress: &dyn Progress) {
    st.confirm_review().unwrap();
    let job = st.begin_anonymize().unwrap();
    if let Some(s) = job.session.clone() {
        *session = s;
    }
    let report = anonymize_batch(
        engine(),
        &job.inputs,
        &job.analysis,
        &job.never,
        &job.masks,
        session,
        &job.protection,
        &job.out_dir,
        progress,
    );
    st.anonymize_done(report, job.inputs, job.analysis);
}

#[test]
fn parcours_complet_sur_les_exemples() {
    let work = temp_dir("parcours");
    let paths = copy_examples(&work);
    assert!(paths.len() >= 8, "exemples gérés : {}", paths.len());
    let mut session = Session::new();
    let mut st = analyzed(paths.clone(), &session, &NoProgress);
    assert_eq!(st.step, Step::Review);
    assert!(st.rejected.is_empty(), "{:?}", st.rejected);
    assert!(st.selected_count() > 10);
    assert!(!st.tag_preview.is_empty());
    let out = work.join(OUTPUT_DIR_NAME);
    assert_eq!(st.out_dir.as_deref(), Some(out.as_path()));
    assert!(!out.exists(), "créé seulement à l'écriture");

    st.protect.mode = ProtectMode::NewPassword;
    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut session, &NoProgress);
    assert_eq!(st.step, Step::Done);
    assert!(st.protect.password.is_empty(), "mot de passe effacé");
    let report = st.report.clone().unwrap();
    assert!(report.fatal.is_none());
    let map = report.map_path.clone().unwrap();
    assert_eq!(map, out.join(MAP_BASENAME));

    // Fichiers écrits : suffixe, aucun original modifié, aucune valeur cochée lisible.
    let mut written = Vec::new();
    for f in &report.files {
        match &f.outcome {
            Outcome::Written {
                path, replacements, ..
            } => {
                let stem = f.name.rsplit_once('.').unwrap().0;
                assert!(
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(&format!("{stem}_anonymise")),
                    "{}",
                    path.display()
                );
                assert!(*replacements > 0, "{}", f.name);
                written.push(path.clone());
            }
            Outcome::Leak { .. } => {
                // Bloqué : aucun fichier produit.
                let stem = f.name.rsplit_once('.').unwrap().0;
                assert!(!std::fs::read_dir(&out).unwrap().any(|e| {
                    e.unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{stem}_anonymise"))
                }));
            }
            other => panic!("{} : {other:?}", f.name),
        }
    }
    assert!(
        written.len() >= paths.len() - 2,
        "{} livrés sur {}",
        written.len(),
        paths.len()
    );
    for p in &paths {
        let original = exemples().join(p.file_name().unwrap());
        assert_eq!(std::fs::read(p).unwrap(), std::fs::read(original).unwrap());
    }

    // La table se rouvre avec le mot de passe et contient la session.
    let mut d = DeanonymizeState::default();
    d.choose_map(map.clone()).unwrap();
    d.password = PASSWORD.into();
    let (path, pw) = d.begin_unlock().unwrap();
    let opened = anon_vault::open(&std::fs::read(&path).unwrap(), &pw).map_err(|e| e.to_string());
    d.unlock_done(opened);
    assert!(d.is_unlocked());
    let restored_session = d.session().unwrap().clone();
    assert_eq!(restored_session.entities.len(), session.entities.len());

    // Restauration de chaque fichier livré, dans un autre dossier.
    let back = temp_dir("restaure");
    d.add_input_files(written.iter().cloned().map(file_entry));
    d.out_dir = Some(back.clone());
    let job = d.begin_restore_files().unwrap();
    let (inputs, unreadable) = read_inputs(&job.paths);
    assert!(unreadable.is_empty());
    let results = restore_batch(engine(), &inputs, &job.session, &job.out_dir, &NoProgress);
    d.restore_files_done(results.clone());
    for r in &results {
        let (path, rep) = r
            .result
            .as_ref()
            .unwrap_or_else(|e| panic!("{} : {e}", r.name));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("_restaure")
        );
        assert!(rep.unknown.is_empty(), "{} : {:?}", r.name, rep.unknown);
        let image = ["png", "jpg", "jpeg"]
            .iter()
            .any(|e| r.name.ends_with(&format!(".{e}")));
        // Une image seule ou un PDF scanné n'ont pas de balise en texte : elles
        // sont dessinées dans l'image. Rien à compter côté texte.
        let scan = r.name.starts_with("courrier-scanne");
        if !image && !scan {
            assert!(rep.restored > 0, "{}", r.name);
        }
    }
    // Texte brut : la restauration redonne l'original à la forme canonique près.
    let notes = results
        .iter()
        .find(|r| r.name.starts_with("notes"))
        .and_then(|r| r.result.as_ref().ok())
        .map(|(p, _)| std::fs::read_to_string(p).unwrap())
        .unwrap();
    assert!(!notes.contains("[PERSONNE_"), "balise non restaurée");

    // Texte collé.
    d.mode = anonymiseur::ui::state::deanonymize::InputMode::Paste;
    d.pasted = std::fs::read_to_string(exemples().join("reponse-ia-exemple.md")).unwrap();
    d.restore_text(|t, s| engine().restore_text(t, s)).unwrap();
    let (text, rep) = d.text_result.clone().unwrap();
    assert!(rep.restored >= 5);
    assert!(!text.contains("[PERSONNE_1]"));
    assert!(rep.unknown.contains(&"[PERSONNE_99]".to_owned()));

    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(&back);
}

#[test]
fn mauvais_mot_de_passe() {
    let work = temp_dir("mdp");
    let src = exemples().join("notes.txt");
    let file = work.join("notes.txt");
    std::fs::copy(&src, &file).unwrap();
    let mut session = Session::new();
    let mut st = analyzed(vec![file], &session, &NoProgress);
    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut session, &NoProgress);
    let map = st.report.as_ref().unwrap().map_path.clone().unwrap();

    let mut d = DeanonymizeState::default();
    d.choose_map(map).unwrap();
    d.password = "pas le bon".into();
    let (path, pw) = d.begin_unlock().unwrap();
    let opened = anon_vault::open(&std::fs::read(path).unwrap(), &pw).map_err(|e| e.to_string());
    d.unlock_done(opened);
    assert!(!d.is_unlocked());
    let msg = d.unlock_error.clone().unwrap();
    assert!(msg.starts_with("Mot de passe incorrect"), "{msg}");
    // Un fichier qui n'est pas une table.
    let not_map = anon_vault::open(b"bonjour", "x").map_err(|e| e.to_string());
    d.unlock_done(not_map);
    assert!(d.unlock_error.unwrap().contains("pas une table"));
    let _ = std::fs::remove_dir_all(&work);
}

/// Gestionnaire qui recopie le texte sans le modifier : toute valeur cochée
/// reste lisible, le contrôle de fuite doit bloquer le fichier.
struct Passthrough;

impl Handler for Passthrough {
    fn extensions(&self) -> &'static [&'static str] {
        &["fuite"]
    }

    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        let text = String::from_utf8_lossy(input);
        Ok(Extraction {
            blocks: text
                .lines()
                .map(|l| TextBlock {
                    text: l.to_owned(),
                    ctx: BlockContext::new("texte", BlockRole::Body),
                })
                .collect(),
            media: Vec::new(),
            warnings: Vec::new(),
        })
    }

    fn transform(
        &self,
        input: &[u8],
        _tr: &dyn TextTransform,
        _opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        Ok(Output {
            bytes: input.to_vec(),
            replacements: 0,
            warnings: Vec::new(),
        })
    }
}

static PASSTHROUGH: Passthrough = Passthrough;

#[test]
fn fuite_simulee_bloque_le_fichier() {
    let engine = Engine::standard_builder()
        .handler("fuite", &PASSTHROUGH)
        .build();
    let work = temp_dir("fuite");
    let leaky = work.join("piege.fuite");
    std::fs::write(&leaky, "Contact : helene.kaloi@example.com").unwrap();
    let ok = work.join("ok.txt");
    std::fs::write(&ok, "Écrire à helene.kaloi@example.com").unwrap();
    let (inputs, _) = read_inputs(&[leaky, ok]);
    let session0 = Session::new();
    let analysis = engine.analyze(
        &inputs,
        &anon_engine::AnalyzeOptions::default(),
        &session0,
        &NoProgress,
    );
    let mut session = Session::new();
    let report = anonymize_batch(
        &engine,
        &inputs,
        &analysis,
        &[],
        &[],
        &mut session,
        &Protection::Password(PASSWORD.into()),
        &work,
        &NoProgress,
    );
    assert!(
        matches!(&report.files[0].outcome, Outcome::Leak { tags, .. } if tags == &vec!["[EMAIL_1]".to_owned()])
    );
    assert!(matches!(report.files[1].outcome, Outcome::Written { .. }));
    assert!(
        !work.join("piege_anonymise.fuite").exists(),
        "fichier bloqué non écrit"
    );
    assert!(work.join("ok_anonymise.txt").exists());
    // Repli : le texte anonymisé reste exportable.
    let text = engine.export_text(&inputs[0], &session).unwrap();
    assert!(text.contains("[EMAIL_1]") && !text.contains("helene"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn fichiers_refuses_sans_bloquer_les_autres() {
    let work = temp_dir("refus");
    let good = work.join("notes.txt");
    std::fs::copy(exemples().join("notes.txt"), &good).unwrap();
    let old = work.join("ancien.doc");
    std::fs::write(&old, b"binaire").unwrap();
    let google = work.join("Budget.gsheet");
    std::fs::write(&google, b"{}").unwrap();
    let broken = work.join("abime.docx");
    std::fs::write(&broken, b"PK pas vraiment un docx").unwrap();
    let session = Session::new();
    let st = analyzed(vec![good, old, google, broken], &session, &NoProgress);
    assert_eq!(st.step, Step::Review);
    assert_eq!(st.files.len(), 1);
    let reasons: Vec<(&str, &str)> = st
        .rejected
        .iter()
        .map(|r| (r.name.as_str(), r.reason.as_str()))
        .collect();
    assert_eq!(reasons.len(), 3, "{reasons:?}");
    assert!(
        reasons
            .iter()
            .any(|(n, r)| *n == "ancien.doc" && r.contains("réenregistrez"))
    );
    assert!(
        reasons
            .iter()
            .any(|(n, r)| *n == "Budget.gsheet" && r.contains("Télécharger"))
    );
    assert!(reasons.iter().any(|(n, _)| *n == "abime.docx"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn aucun_fichier_ecrase() {
    let work = temp_dir("ecrasement");
    let file = work.join("notes.txt");
    std::fs::copy(exemples().join("notes.txt"), &file).unwrap();
    let mut session = Session::new();
    let out = work.join(OUTPUT_DIR_NAME);
    for round in 1..=2 {
        let mut st = analyzed(vec![file.clone()], &session, &NoProgress);
        st.protect.password = PASSWORD.into();
        st.protect.confirm = PASSWORD.into();
        anonymize(&mut st, &mut session, &NoProgress);
        let report = st.report.unwrap();
        let (expected_file, expected_map) = if round == 1 {
            ("notes_anonymise.txt", MAP_BASENAME.to_owned())
        } else {
            (
                "notes_anonymise_2.txt",
                "table_anonymiseur_2.anonmap".to_owned(),
            )
        };
        assert!(
            matches!(&report.files[0].outcome, Outcome::Written { path, .. } if path == &out.join(expected_file))
        );
        assert_eq!(report.map_path, Some(out.join(expected_map)));
    }
    // La première table reste lisible : elle n'a pas été remplacée.
    assert!(anon_vault::open(&std::fs::read(out.join(MAP_BASENAME)).unwrap(), PASSWORD).is_ok());
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn reprise_d_une_table_existante_garde_les_balises() {
    let work = temp_dir("reprise");
    let file = work.join("notes.txt");
    std::fs::copy(exemples().join("notes.txt"), &file).unwrap();
    let mut first_session = Session::new();
    let mut st = analyzed(vec![file.clone()], &first_session, &NoProgress);
    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut first_session, &NoProgress);
    let map = st.report.unwrap().map_path.unwrap();

    // Nouvelle session vide, table reprise à l'étape Protéger.
    let mut session = Session::new();
    let mut st = analyzed(vec![file], &session, &NoProgress);
    st.confirm_review().unwrap();
    st.protect.mode = ProtectMode::ExistingTable;
    st.protect.existing_path = Some(map.clone());
    st.protect.password = PASSWORD.into();
    let opened =
        anon_vault::open(&std::fs::read(&map).unwrap(), PASSWORD).map_err(|e| e.to_string());
    st.existing_opened(opened);
    assert!(st.protect.can_proceed());
    let job = st.begin_anonymize().unwrap();
    session = job.session.clone().unwrap();
    let report = anonymize_batch(
        engine(),
        &job.inputs,
        &job.analysis,
        &job.never,
        &job.masks,
        &mut session,
        &job.protection,
        &job.out_dir,
        &NoProgress,
    );
    let out = work.join(OUTPUT_DIR_NAME);
    let first = std::fs::read(out.join("notes_anonymise.txt")).unwrap();
    let second = std::fs::read(out.join("notes_anonymise_2.txt")).unwrap();
    assert_eq!(first, second, "mêmes balises");
    let new_map = report.map_path.unwrap();
    assert!(anon_vault::open(&std::fs::read(new_map).unwrap(), PASSWORD).is_ok());
    let _ = std::fs::remove_dir_all(&work);
}

/// Annule après `after` fichiers.
struct CancelAfter {
    after: usize,
    seen: AtomicUsize,
}

impl Progress for CancelAfter {
    fn step(&self, done: usize, _total: usize, _label: &str) {
        // Fichier `done` commencé : il compte comme traité.
        self.seen.store(done + 1, Ordering::Relaxed);
    }

    fn cancelled(&self) -> bool {
        self.seen.load(Ordering::Relaxed) >= self.after
    }
}

#[test]
fn annulation() {
    let work = temp_dir("annulation");
    let paths: Vec<PathBuf> = ["notes.txt", "export.csv", "brief.md"]
        .iter()
        .map(|n| {
            let to = work.join(n);
            std::fs::copy(exemples().join(n), &to).unwrap();
            to
        })
        .collect();

    // Analyse annulée dès le départ : retour à « Déposer », rien de chargé.
    let cancel_now = CancelAfter {
        after: 0,
        seen: AtomicUsize::new(0),
    };
    let session = Session::new();
    let st = analyzed(paths.clone(), &session, &cancel_now);
    assert_eq!(st.step, Step::Drop);
    assert!(st.analysis.is_none());

    // Anonymisation annulée après le premier fichier : il est livré avec sa table.
    let mut session = Session::new();
    let mut st = analyzed(paths, &session, &NoProgress);
    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    let cancel = CancelAfter {
        after: 1,
        seen: AtomicUsize::new(0),
    };
    anonymize(&mut st, &mut session, &cancel);
    let report = st.report.unwrap();
    assert!(report.cancelled);
    assert_eq!(report.delivered(), 1);
    assert!(report.map_path.is_some());
    assert!(matches!(report.files[1].outcome, Outcome::Cancelled));
    let _ = std::fs::remove_dir_all(&work);
}

// --- Exemples, liste « toujours », généralisation, zones manuelles ----------

#[test]
fn exemples_reutilises_sans_ecrasement() {
    use anonymiseur::ui::service::{EXAMPLES, prepare_examples};
    // Tous les fichiers de `exemples/` sont embarqués, sauf la vérité terrain.
    let mut on_disk: Vec<String> = std::fs::read_dir(exemples())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "attendu.json")
        .collect();
    on_disk.sort();
    let mut embedded: Vec<String> = EXAMPLES.iter().map(|e| e.name.to_owned()).collect();
    embedded.sort();
    assert_eq!(embedded, on_disk);

    let parent = temp_dir("exemples");
    let dir = parent.join("Anonymiseur").join("Exemples");
    // Le dossier existe déjà, avec un fichier modifié par l'utilisateur.
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "à garder").unwrap();
    let first = prepare_examples(&dir).unwrap();
    assert_eq!(first.dir, dir, "même dossier, jamais « _2 »");
    assert_eq!(first.created, EXAMPLES.len() - 1);
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "à garder",
        "rien n'est remplacé"
    );
    for e in EXAMPLES.iter().filter(|e| e.name != "notes.txt") {
        assert_eq!(
            std::fs::read(dir.join(e.name)).unwrap(),
            std::fs::read(exemples().join(e.name)).unwrap(),
            "{}",
            e.name
        );
    }
    // Documents chargés : tout sauf la notice et la réponse d'IA simulée.
    let names: Vec<String> = first
        .documents
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(!names.contains(&"LISEZMOI.md".to_owned()));
    assert!(!names.contains(&"reponse-ia-exemple.md".to_owned()));
    assert_eq!(names.len(), EXAMPLES.len() - 2);
    // Tous sont acceptés par le moteur livré ; la sortie va dans « Anonymisés ».
    let mut st = AnonymizeState::default();
    st.add_files(
        first.documents.iter().cloned().map(file_entry),
        &engine().supported_extensions(),
    );
    assert_eq!(st.files.len(), names.len());
    assert!(st.rejected.is_empty(), "{:?}", st.rejected);
    assert_eq!(st.out_dir, Some(dir.join(OUTPUT_DIR_NAME)));

    // Second passage, après une anonymisation : rien n'est recréé, et les
    // fichiers produits par l'outil ne sont jamais proposés.
    std::fs::write(dir.join("notes_anonymise.txt"), "sortie").unwrap();
    let second = prepare_examples(&dir).unwrap();
    assert_eq!(second.created, 0);
    assert_eq!(second.documents, first.documents);
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn fichiers_produits_jamais_reanonymises() {
    // Anonymisation de deux exemples, puis dépôt de tout le dossier, sorties comprises.
    let work = temp_dir("sorties");
    let mut paths = Vec::new();
    for n in ["notes.txt", "export.csv"] {
        let to = work.join(n);
        std::fs::copy(exemples().join(n), &to).unwrap();
        paths.push(to);
    }
    let mut session = Session::new();
    let mut st = analyzed(paths, &session, &NoProgress);
    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut session, &NoProgress);
    let out = work.join(OUTPUT_DIR_NAME);
    assert_eq!(st.report.unwrap().delivered(), 2);
    // Les sorties, recopiées à côté des sources (cas d'un ancien dossier).
    for e in std::fs::read_dir(&out).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, work.join(p.file_name().unwrap())).unwrap();
    }
    let all: Vec<PathBuf> = std::fs::read_dir(&work)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    assert_eq!(all.len(), 5, "{all:?}");
    let mut st = AnonymizeState::default();
    let report = st.add_files(
        all.into_iter().map(file_entry),
        &engine().supported_extensions(),
    );
    assert_eq!(report.added, 2);
    assert_eq!(report.outputs, 3, "deux fichiers et la table");
    let mut names: Vec<&str> = st.files.iter().map(|f| f.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["export.csv", "notes.txt"]);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn terme_toujours_retrouve_et_remplace() {
    let work = temp_dir("toujours");
    let file = work.join("notes.txt");
    std::fs::copy(exemples().join("notes.txt"), &file).unwrap();
    let mut session = Session::new();
    let mut st = analyzed(vec![file], &session, &NoProgress);
    let found = |st: &AnonymizeState| {
        st.analysis
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .find(|c| c.value == "Martinique")
            .map(|c| (c.selected, c.occurrences))
    };
    // Piège à faux positif : au plus proposé décoché.
    assert!(
        matches!(found(&st), None | Some((false, _))),
        "{:?}",
        found(&st)
    );

    // Ajout à l'écran de vérification : l'analyse est relancée.
    st.always_term = "Martinique".into();
    st.always_kind = anon_core::EntityKind::Autre;
    let paths = st.add_always().unwrap().expect("analyse relancée");
    let (inputs, rejected) = read_inputs(&paths);
    let analysis = engine().analyze(&inputs, &st.analyze_options(), &session, &NoProgress);
    st.analysis_done(inputs, rejected, analysis, &session);
    assert_eq!(found(&st), Some((true, 1)));

    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut session, &NoProgress);
    let report = st.report.unwrap();
    let Outcome::Written { path, .. } = &report.files[0].outcome else {
        panic!("{:?}", report.files[0].outcome);
    };
    let out = std::fs::read_to_string(path).unwrap();
    assert!(!out.contains("Martinique"), "{out}");
    assert!(out.contains("[AUTRE_1]"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn ligne_generalisee_non_restauree() {
    let work = temp_dir("generalisation");
    let file = work.join("notes.txt");
    std::fs::copy(exemples().join("notes.txt"), &file).unwrap();
    let mut session = Session::new();
    let mut st = AnonymizeState {
        profile: anon_core::Profile::Renforce,
        ..Default::default()
    };
    st.add_files([file_entry(file)], &engine().supported_extensions());
    let to_read = st.begin_analysis().unwrap();
    let (inputs, rejected) = read_inputs(&to_read);
    let analysis = engine().analyze(&inputs, &st.analyze_options(), &session, &NoProgress);
    st.analysis_done(inputs, rejected, analysis, &session);
    let age = st
        .visible()
        .into_iter()
        .find(|id| st.candidate(*id).is_some_and(|c| c.value == "62 ans"))
        .expect("âge détecté en Renforcé");
    assert!(st.set_generalized(age, true, &session));
    assert_eq!(st.generalized(age), Some("60-69 ans"));

    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut session, &NoProgress);
    let report = st.report.unwrap();
    let Outcome::Written { path, .. } = &report.files[0].outcome else {
        panic!("{:?}", report.files[0].outcome);
    };
    let out = std::fs::read_to_string(path).unwrap();
    assert!(
        out.contains("60-69 ans") && !out.contains("62 ans"),
        "{out}"
    );

    // Restauration : les balises reviennent, la valeur généralisée reste.
    let back = work.join("retour");
    std::fs::create_dir_all(&back).unwrap();
    let (inputs, _) = read_inputs(std::slice::from_ref(path));
    let results = restore_batch(engine(), &inputs, &session, &back, &NoProgress);
    let (restored, _) = results[0].result.as_ref().unwrap();
    let restored = std::fs::read_to_string(restored).unwrap();
    assert!(restored.contains("Pierre Garnier"), "{restored}");
    assert!(restored.contains("60-69 ans"));
    assert!(!restored.contains("62 ans"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn zone_manuelle_masquee_sur_la_carte_de_visite() {
    use anon_core::{ImageProcessor, PixelRect};
    let work = temp_dir("zone-manuelle");
    let file = work.join("carte-visite.png");
    std::fs::copy(exemples().join("carte-visite.png"), &file).unwrap();
    let mut session = Session::new();
    // Profil Standard : la fonction (« responsable commerciale ») n'est pas détectée.
    let mut st = analyzed(vec![file], &session, &NoProgress);
    let images = st.images();
    assert_eq!(images.len(), 1, "{images:?}");
    let info = images[0].clone();
    let target = {
        let image = st
            .analysis
            .as_ref()
            .unwrap()
            .image(&info.file, &info.name)
            .unwrap();
        assert!(!image.bytes.is_empty());
        image
            .ocr
            .words
            .iter()
            .find(|w| w.text.to_lowercase().contains("commerciale"))
            .map(|w| w.rect)
            .expect("mot lu par l'OCR")
    };
    let auto = st.auto_zones(0).to_vec();
    assert!(!auto.is_empty(), "nom, email et téléphone masqués");
    assert!(
        !auto.iter().any(|r| r.x <= target.x
            && r.y <= target.y
            && r.x + r.w >= target.x + target.w
            && r.y + r.h >= target.y + target.h),
        "la fonction n'est pas masquée automatiquement"
    );
    // Zone tracée à la main, un peu plus large que le mot.
    let pad = 6;
    st.masks.add(
        &info.file,
        &info.name,
        PixelRect {
            x: target.x.saturating_sub(pad),
            y: target.y.saturating_sub(pad),
            w: target.w + 2 * pad,
            h: target.h + 2 * pad,
        },
    );
    st.protect.password = PASSWORD.into();
    st.protect.confirm = PASSWORD.into();
    anonymize(&mut st, &mut session, &NoProgress);
    let report = st.report.unwrap();
    let Outcome::Written { path, .. } = &report.files[0].outcome else {
        panic!("{:?}", report.files[0].outcome);
    };
    // Ré-OCR de la sortie : le mot visé n'est plus lu.
    let page = anon_pdf::MupdfImageProcessor::new()
        .ocr(&std::fs::read(path).unwrap())
        .unwrap();
    let text = page.text_with_spans().0.to_lowercase();
    assert!(!text.contains("commerciale"), "{text}");
    assert!(!text.contains("vaitiare"));
    let _ = std::fs::remove_dir_all(&work);
}
