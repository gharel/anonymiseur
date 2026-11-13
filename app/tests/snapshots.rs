//! Instantanés PNG de la galerie et de chaque écran, alimentés par une vraie
//! analyse des fichiers de `exemples/`, à deux tailles de fenêtre (plus une
//! page haute pour voir le contenu sous la ligne de flottaison).
//! Mise à jour : `UPDATE_SNAPSHOTS=force cargo test -p anonymiseur --test snapshots`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{Profile, Session};
use anon_engine::{Engine, NoProgress};
use anonymiseur::moteur::IaStatus;
use anonymiseur::ui::gallery::{self, GalleryState};
use anonymiseur::ui::platform::{NullPlatform, file_entry};
use anonymiseur::ui::service::{Outcome, anonymize_batch, read_inputs};
use anonymiseur::ui::state::Tab;
use anonymiseur::ui::state::anonymize::{Busy, Step};
use anonymiseur::ui::state::deanonymize::InputMode;
use anonymiseur::ui::{AnonApp, theme};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, SnapshotResults};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Deux tailles de fenêtre, plus une page haute pour voir tout le contenu.
const SIZES: [(f32, f32, &str); 3] = [
    (1280.0, 800.0, "1280x800"),
    (900.0, 650.0, "900x650"),
    (1280.0, 1600.0, "pleine_page"),
];
const PASSWORD: &str = "Cheval-Agrafe-Batterie-47";

fn engine() -> (Arc<Engine>, IaStatus) {
    static ENGINE: OnceLock<(Arc<Engine>, IaStatus)> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let m = anonymiseur::moteur::build();
            (Arc::new(m.engine), m.ia)
        })
        .clone()
}

fn exemple(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exemples")
        .join(name)
}

/// Fichiers analysés pour la maquette : bureautique, tableur, texte, PDF scanné.
const FICHIERS: [&str; 4] = [
    "contrat-prestation.docx",
    "fichier-clients.xlsx",
    "notes.txt",
    "courrier-scanne.pdf",
];

fn app(width: f32, height: f32, setup: impl FnOnce(&mut AnonApp)) -> Harness<'static, AnonApp> {
    let (engine, ia) = engine();
    Harness::builder()
        .with_size([width, height])
        .with_pixels_per_point(1.0)
        .build_eframe(move |cc| {
            let mut app =
                AnonApp::with_engine(&cc.egui_ctx, Box::new(NullPlatform::default()), engine, ia);
            setup(&mut app);
            app
        })
}

/// Dossier affiché dans les instantanés. Les fichiers sont réellement écrits dans
/// un dossier temporaire, mais on n'en montre pas le chemin : il dépend de la
/// machine et contient le nom de l'utilisateur.
const SHOWN_DIR: &str = "C:/Documents/Formation";

fn neutralize_paths(app: &mut AnonApp) {
    let shown = PathBuf::from(SHOWN_DIR);
    let renamed = |p: &PathBuf| shown.join(p.file_name().unwrap_or_default());
    if app.anonymize.out_dir.is_some() || app.anonymize.step == Step::Protect {
        app.anonymize.out_dir = Some(shown.clone());
    }
    if let Some(report) = app.anonymize.report.as_mut() {
        report.out_dir = shown.clone();
        if let Some(map) = report.map_path.as_mut() {
            *map = renamed(map);
        }
        for f in &mut report.files {
            if let Outcome::Written { path, .. } = &mut f.outcome {
                *path = renamed(path);
            }
        }
    }
    if app.deanonymize.out_dir.is_some() {
        app.deanonymize.out_dir = Some(shown.clone());
    }
}

fn snap(name: &str, setup: impl Fn(&mut AnonApp)) {
    let mut results = SnapshotResults::new();
    for (w, h, label) in SIZES {
        let mut harness = app(w, h, |a| {
            setup(a);
            neutralize_paths(a);
        });
        harness.run();
        harness.snapshot(format!("{name}_{label}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

/// Dépose les fichiers et lance une vraie analyse.
fn analyze(app: &mut AnonApp, profile: Profile) {
    let engine = app.engine.clone().unwrap();
    let paths: Vec<PathBuf> = FICHIERS.iter().map(|n| exemple(n)).collect();
    app.anonymize.profile = profile;
    app.anonymize.add_files(
        paths.into_iter().map(file_entry),
        &engine.supported_extensions(),
    );
    let to_read = app.anonymize.begin_analysis().unwrap();
    let (inputs, rejected) = read_inputs(&to_read);
    let analysis = engine.analyze(
        &inputs,
        &app.anonymize.analyze_options(),
        &app.session,
        &NoProgress,
    );
    app.anonymize
        .analysis_done(inputs, rejected, analysis, &app.session);
}

/// Dossier de sortie des instantanés (vidé à chaque passage).
fn out_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anonymiseur-instantanes-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Parcours complet jusqu'à « Terminé », table chiffrée écrite dans `dir`.
fn anonymize_all(app: &mut AnonApp, dir: PathBuf) {
    analyze(app, Profile::Renforce);
    app.anonymize.confirm_review().unwrap();
    app.anonymize.out_dir = Some(dir);
    app.anonymize.protect.password = PASSWORD.into();
    app.anonymize.protect.confirm = PASSWORD.into();
    let job = app.anonymize.begin_anonymize().unwrap();
    let engine = app.engine.clone().unwrap();
    let report = anonymize_batch(
        &engine,
        &job.inputs,
        &job.analysis,
        &job.never,
        &mut app.session,
        &job.protection,
        &job.out_dir,
        &NoProgress,
    );
    app.anonymize
        .anonymize_done(report, job.inputs, job.analysis);
}

#[test]
fn gallery() {
    let mut harness = Harness::builder()
        .with_size([1280.0, 2450.0])
        .with_pixels_per_point(1.0)
        .build_ui_state(
            |ui, (ready, st): &mut (bool, GalleryState)| {
                // Polices et thème installés avant la première mise en page.
                if !*ready {
                    theme::install(ui.ctx());
                    *ready = true;
                    ui.ctx().request_repaint();
                    return;
                }
                egui::Frame::new()
                    .fill(theme::palette().bg_subtle)
                    .inner_margin(24)
                    .show(ui, |ui| gallery::show(ui, st));
            },
            (false, GalleryState::default()),
        );
    harness.run();
    harness.snapshot("galerie");
}

#[test]
fn anonymize_empty() {
    snap("anonymiser_0_vide", |_| {});
}

#[test]
fn anonymize_drop() {
    snap("anonymiser_1_deposer", |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        let mut paths: Vec<PathBuf> = FICHIERS.iter().map(|n| exemple(n)).collect();
        paths.push(exemple("ancien-rapport.doc"));
        paths.push(exemple("Budget formation.gsheet"));
        app.anonymize
            .add_files(paths.into_iter().map(file_entry), &supported);
    });
}

#[test]
fn anonymize_busy() {
    snap("anonymiser_1b_analyse_en_cours", |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files(FICHIERS.iter().map(|n| file_entry(exemple(n))), &supported);
        app.anonymize.busy = Some(Busy::Analyzing);
    });
}

#[test]
fn anonymize_review() {
    snap("anonymiser_2_verifier", |app| {
        analyze(app, Profile::Renforce)
    });
}

#[test]
fn anonymize_protect() {
    snap("anonymiser_3_proteger", |app| {
        analyze(app, Profile::Standard);
        app.anonymize.confirm_review().unwrap();
        app.anonymize.protect.password = PASSWORD.into();
        app.anonymize.protect.confirm = "Cheval-Agrafe".into();
    });
}

#[test]
fn anonymize_done() {
    snap("anonymiser_4_termine", |app| {
        anonymize_all(app, out_dir("termine"));
    });
}

#[test]
fn deanonymize_empty() {
    snap("desanonymiser_1_vide", |app| app.tab = Tab::Deanonymize);
}

#[test]
fn deanonymize_locked() {
    snap("desanonymiser_2_table", |app| {
        app.tab = Tab::Deanonymize;
        let _ = app
            .deanonymize
            .choose_map(PathBuf::from("C:/Dossiers/table_anonymiseur.anonmap"));
        app.deanonymize
            .unlock_done(Err(anon_vault::VaultError::WrongPassword.to_string()));
    });
}

#[test]
fn deanonymize_result() {
    snap("desanonymiser_3_resultat", |app| {
        let dir = out_dir("resultat");
        anonymize_all(app, dir.clone());
        let map = app
            .anonymize
            .report
            .as_ref()
            .unwrap()
            .map_path
            .clone()
            .unwrap();
        app.tab = Tab::Deanonymize;
        let d = &mut app.deanonymize;
        d.choose_map(map).unwrap();
        d.password = PASSWORD.into();
        let (path, pw) = d.begin_unlock().unwrap();
        let opened =
            anon_vault::open(&std::fs::read(path).unwrap(), &pw).map_err(|e| e.to_string());
        d.unlock_done(opened);
        d.mode = InputMode::Paste;
        d.pasted = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
        let engine = app.engine.clone().unwrap();
        d.restore_text(|t, s| engine.restore_text(t, s)).unwrap();
    });
}

#[test]
fn free_text() {
    snap("texte_libre", |app| {
        app.tab = Tab::FreeText;
        let engine = app.engine.clone().unwrap();
        app.free_text.original = std::fs::read_to_string(exemple("notes.txt")).unwrap();
        let opts = app.anonymize.analyze_options();
        let session: &mut Session = &mut app.session;
        app.free_text.anonymize(|t| {
            let a = engine.analyze_text(t, &opts, session);
            engine.apply_selection(&a, session);
            (engine.anonymize_text(t, session), a.selected().count())
        });
    });
}

// --- Interactions (sans image) ---------------------------------------------

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

#[test]
fn dropped_files_are_listed_or_refused() {
    let mut harness = app(1280.0, 800.0, |_| {});
    harness.run();
    for p in [exemple("notes.txt"), exemple("vieux.xls")] {
        harness.input_mut().dropped_files.push(Arc::new(Dropped(p)));
    }
    harness.run();
    assert_eq!(harness.state().anonymize.files.len(), 1);
    assert_eq!(harness.state().anonymize.rejected.len(), 1);
    harness.get_by_label("notes.txt");
}

#[test]
fn analyse_button_runs_a_real_analysis() {
    let mut harness = app(1280.0, 1600.0, |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files([file_entry(exemple("notes.txt"))], &supported);
    });
    harness.run();
    harness.get_by_label("Analyser les fichiers").click();
    harness.run();
    assert_eq!(harness.state().anonymize.step, Step::Review);
    assert!(harness.state().anonymize.selected_count() > 0);
    harness.get_by_label("Retour").click();
    harness.run();
    assert_eq!(harness.state().anonymize.step, Step::Drop);
}

#[test]
fn tabs_switch_by_click_and_keyboard() {
    let mut harness = app(1280.0, 800.0, |_| {});
    harness.run();
    harness.get_by_label("Texte libre").click();
    harness.run();
    assert_eq!(harness.state().tab, Tab::FreeText);
    // L'onglet « Texte libre » a aussi un bouton « Anonymiser » : on vise l'onglet.
    harness
        .get_all_by_label("Anonymiser")
        .find(|n| n.accesskit_node().role() != egui::accesskit::Role::Button)
        .unwrap()
        .click();
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().tab, Tab::Deanonymize);
}

#[test]
fn free_text_buttons_use_the_session() {
    let mut harness = app(1280.0, 800.0, |app| {
        app.tab = Tab::FreeText;
        app.free_text.original = "Écrire à helene.kaloi@example.com.".into();
    });
    harness.run();
    let buttons: Vec<_> = harness
        .get_all_by_label("Anonymiser")
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .collect();
    assert_eq!(buttons.len(), 1);
    buttons[0].click();
    harness.run();
    assert!(harness.state().free_text.anonymized.contains("[EMAIL_1]"));
    assert_eq!(harness.state().session.entities.len(), 1);
    harness.get_by_label("Nouvelle session").click();
    harness.run();
    assert!(harness.state().session.entities.is_empty());
}
