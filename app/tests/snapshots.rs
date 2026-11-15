//! Instantanés PNG de la galerie, de l'accueil et de chaque étape du dossier de
//! travail, alimentés par une vraie analyse des fichiers de `exemples/`, à deux
//! tailles de fenêtre (plus une page haute pour voir le contenu sous la ligne
//! de flottaison), et en thème sombre à 1280 × 800. Images comparées sous
//! Windows, édition complète (`COMPARE_IMAGES`). Puis les interactions : parcours
//! complet au clavier, palette de commandes, annuler et rétablir, alignement,
//! noms des boutons, vocabulaire.
//! Mise à jour : `UPDATE_SNAPSHOTS=force cargo test -p anonymiseur --test snapshots`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{EntityKind, Goal, PixelRect, Session};
use anon_engine::{Engine, NoProgress};
use anonymiseur::moteur::IaStatus;
use anonymiseur::ui::clipboard::MemoryClipboard;
use anonymiseur::ui::gallery::{self, GalleryState};
use anonymiseur::ui::hotkeys::{self, ScriptedHotkeys};
use anonymiseur::ui::platform::{NullPlatform, file_entry};
use anonymiseur::ui::screens::express as express_screen;
use anonymiseur::ui::service::{
    Outcome, anonymize_batch, assess_folder, file_name, fix_all_folder, read_documents,
};
use anonymiseur::ui::state::anonymize::{AnonymizeState, Busy};
use anonymiseur::ui::state::deanonymize::MapStatus;
use anonymiseur::ui::state::express::{
    self, Action, ExpressReview, KnownDecisions, MaskRequest, Notice,
};
use anonymiseur::ui::state::review::{Occurrence, VerifyMode};
use anonymiseur::ui::state::toasts::ToastKind;
use anonymiseur::ui::state::workspace::{Screen, Step};
use anonymiseur::ui::theme::ThemeChoice;
use anonymiseur::ui::widgets::field::{Mark, mark_style};
use anonymiseur::ui::widgets::toast;
use anonymiseur::ui::{AnonApp, theme};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, SnapshotResults};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

/// Rendus simultanés au plus dans ce binaire. `cargo test` lance les tests sur
/// autant de fils que de cœurs ; au-delà de deux rendus wgpu à la fois, le
/// processus plante sous Windows (STATUS_ACCESS_VIOLATION). Chaque test qui crée
/// un `Harness` prend une place pour toute sa durée : le binaire passe sans
/// `--test-threads`.
const MAX_RENDUS: usize = 2;

/// Places de rendu occupées, et l'avis d'une place libérée.
static RENDUS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// Place de rendu, rendue à la fin du test (même en cas d'échec).
struct Rendu;

/// Attend une place de rendu libre.
fn rendu() -> Rendu {
    let (occupees, liberee) = &RENDUS;
    let mut n = occupees.lock().unwrap_or_else(PoisonError::into_inner);
    while *n >= MAX_RENDUS {
        n = liberee.wait(n).unwrap_or_else(PoisonError::into_inner);
    }
    *n += 1;
    Rendu
}

impl Drop for Rendu {
    fn drop(&mut self) {
        let (occupees, liberee) = &RENDUS;
        let mut n = occupees.lock().unwrap_or_else(PoisonError::into_inner);
        *n = n.saturating_sub(1);
        liberee.notify_one();
    }
}

/// Deux tailles de fenêtre, plus une page haute pour voir tout le contenu, et
/// le thème sombre.
const SIZES: [(f32, f32, &str, ThemeChoice); 4] = [
    (1280.0, 800.0, "1280x800", ThemeChoice::Light),
    (900.0, 650.0, "900x650", ThemeChoice::Light),
    (1280.0, 1600.0, "pleine_page", ThemeChoice::Light),
    (1280.0, 800.0, "sombre_1280x800", ThemeChoice::Dark),
];
const PASSWORD: &str = "Cheval-Agrafe-Batterie-47";

/// Les images de référence sont produites sous Windows avec l'édition complète. Ailleurs,
/// chaque écran est quand même rendu, sans comparaison d'image : l'édition Lite affiche un
/// autre pied de page et d'autres détections, et le rendu d'un autre système diffère de
/// quelques pixels.
const COMPARE_IMAGES: bool = cfg!(all(windows, feature = "ia"));

/// Compare l'écran déjà rendu à son image de référence, là où elle s'applique.
fn shot<S>(harness: &mut Harness<'_, S>, name: impl Into<String>) {
    if COMPARE_IMAGES {
        harness.snapshot(name);
    }
}

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
    app_with(width, height, NullPlatform::default(), setup)
}

fn app_with(
    width: f32,
    height: f32,
    platform: NullPlatform,
    setup: impl FnOnce(&mut AnonApp),
) -> Harness<'static, AnonApp> {
    let (engine, ia) = engine();
    Harness::builder()
        .with_size([width, height])
        .with_pixels_per_point(1.0)
        .build_eframe(move |cc| {
            let mut app = AnonApp::with_engine(&cc.egui_ctx, Box::new(platform), engine, ia);
            setup(&mut app);
            app
        })
}

/// Dossier affiché dans les instantanés. Les fichiers sont réellement écrits dans
/// un dossier temporaire, mais on n'en montre pas le chemin : il dépend de la
/// machine et contient le nom de l'utilisateur.
const SHOWN_DIR: &str = "C:/Documents/Formation";
/// Dossier des clés affiché dans les instantanés.
const SHOWN_KEYS: &str = "C:/Documents/Anonymiseur/Clés";
/// Nom de clé fixe des instantanés (l'heure changerait l'image).
const KEY_NAME: &str = "dossier-16-11-2026-14h05.anonmap";
/// Jour fixe des instantanés, et fin de conservation de leur clé (12 mois).
const TODAY: &str = "2026-11-16";
const KEEP_UNTIL: &str = "2027-11-16";
/// Phrase de passe fixe des instantanés (elle est tirée au hasard sinon).
const PHRASE: &str = "lagune-cerf-volant-banian-roulotte-coquillage-47";

fn neutralize_paths(app: &mut AnonApp) {
    let shown = PathBuf::from(SHOWN_DIR);
    let renamed = |p: &PathBuf| shown.join(p.file_name().unwrap_or_default());
    if app.anonymize.out_dir.is_some() {
        app.anonymize.out_dir = Some(shown.clone());
    }
    if app.anonymize.key_dir.is_some() {
        app.anonymize.key_dir = Some(PathBuf::from(SHOWN_KEYS));
    }
    if app.anonymize.key_name.is_some() {
        app.anonymize.key_name = Some(KEY_NAME.into());
    }
    if let Some(report) = app.anonymize.report.as_mut() {
        report.out_dir = shown.clone();
        if let Some(map) = report.map_path.as_mut() {
            *map = PathBuf::from(SHOWN_KEYS).join(map.file_name().unwrap_or_default());
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
    // Dates : celles d'un jour fixe (l'image ne change pas avec le calendrier).
    // Seule la date posée à l'envoi, aujourd'hui plus 12 mois, est remplacée.
    app.fixed_today = Some(TODAY.into());
    let sent = anon_vault::retention::add_months(&anon_vault::retention::today(), 12);
    if let Some(report) = app.anonymize.report.as_mut()
        && report.keep_until == sent
    {
        report.keep_until = Some(KEEP_UNTIL.into());
    }
    if let MapStatus::Unlocked { session, .. } = &mut app.deanonymize.map
        && session.keep_until == sent
    {
        session.keep_until = Some(KEEP_UNTIL.into());
    }
}

fn snap(name: &str, setup: impl Fn(&mut AnonApp)) {
    let mut results = SnapshotResults::new();
    for (w, h, label, theme) in SIZES {
        let mut harness = app(w, h, |a| {
            a.prefs.theme = theme;
            setup(a);
            neutralize_paths(a);
        });
        harness.run();
        shot(&mut harness, format!("{name}_{label}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

/// Dossier du bilan : un tableau de patients (groupes isolables, corrigés par
/// « Tout corriger ») et un texte.
const BILAN: [&str; 2] = ["patients.xlsx", "notes.txt"];

/// Évalue le dossier tout de suite (comme l'application à l'image suivante).
fn assess_now(app: &mut AnonApp) {
    let engine = app.engine.clone().unwrap();
    let settings = app.assess_settings();
    if let Some(job) = app.anonymize.begin_assessment(settings) {
        let mut a = job.analysis;
        let r = assess_folder(&engine, &mut a, &job.masks, &job.settings);
        app.anonymize.assessment_done(job.revision, job.settings, r);
    }
}

/// « Tout corriger », comme le fil de travail.
fn fix_all(app: &mut AnonApp) {
    let engine = app.engine.clone().unwrap();
    let settings = app.assess_settings();
    let job = app.anonymize.begin_fix_all(settings).unwrap();
    let mut a = job.analysis;
    let (token, result) = fix_all_folder(&engine, &mut a, &job.masks, &job.settings);
    let session = app.session.clone();
    assert!(
        app.anonymize
            .fix_all_done(a, token, job.settings, result, &session)
    );
}

/// Étape Envoyer prête : phrase de passe fixe, dossier et nom de la clé.
fn ready_to_send(app: &mut AnonApp) {
    app.anonymize.protect.generated = Some(PHRASE.into());
    app.anonymize.protect.password = PHRASE.into();
    app.anonymize.key_dir = Some(PathBuf::from(SHOWN_KEYS));
    app.anonymize.key_name = Some(KEY_NAME.into());
}

/// Ajoute les fichiers au dossier et lance une vraie analyse (étape Vérifier).
fn analyze(app: &mut AnonApp) {
    analyze_these(app, &FICHIERS);
}

fn analyze_these(app: &mut AnonApp, names: &[&str]) {
    let engine = app.engine.clone().unwrap();
    let paths: Vec<PathBuf> = names.iter().map(|n| exemple(n)).collect();
    app.anonymize.add_files(
        paths.into_iter().map(file_entry),
        &engine.supported_extensions(),
    );
    let job = app.anonymize.begin_analysis().unwrap();
    let (inputs, rejected) = read_documents(&job);
    let analysis = engine.analyze(
        &inputs,
        &app.anonymize.analyze_options(),
        &app.session,
        &NoProgress,
    );
    assert!(
        app.anonymize
            .analysis_done(inputs, rejected, analysis, &app.session)
    );
    app.workspace.enter(Step::Verify);
}

/// Dossier de sortie des instantanés (vidé à chaque passage).
fn out_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anonymiseur-instantanes-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Parcours jusqu'à l'envoi : fichiers masqués et clé chiffrée écrits dans
/// `dir`, clé ouverte en mémoire pour l'étape Réponse.
fn anonymize_all(app: &mut AnonApp, dir: PathBuf) {
    anonymize_these(app, dir, &FICHIERS);
}

/// Comme [`anonymize_all`], pour les fichiers `names`.
fn anonymize_these(app: &mut AnonApp, dir: PathBuf, names: &[&str]) {
    analyze_these(app, names);
    app.anonymize.key_dir = Some(dir.join("Clés"));
    app.anonymize.key_name = Some(KEY_NAME.into());
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
        &job.masks,
        &mut app.session,
        &job.plan,
        None,
        &NoProgress,
    );
    let map = report.map_path.clone().unwrap();
    app.deanonymize
        .open_in_memory(file_name(&map), app.session.clone());
    app.deanonymize.key_path = Some(map);
    app.deanonymize.key_id = report.key_id.clone();
    app.anonymize
        .anonymize_done(report, job.inputs, job.analysis);
    app.workspace.enter(Step::Send);
}

fn gallery_in(choice: ThemeChoice, name: &str) {
    let mut harness = Harness::builder()
        .with_size([1280.0, 2700.0])
        .with_pixels_per_point(1.0)
        .build_ui_state(
            move |ui, (ready, st): &mut (bool, GalleryState)| {
                // Polices et thème installés avant la première mise en page.
                if !*ready {
                    theme::install(ui.ctx(), choice);
                    *ready = true;
                    ui.ctx().request_repaint();
                    return;
                }
                theme::sync(ui);
                egui::Frame::new()
                    .fill(theme::palette().page)
                    .inner_margin(24)
                    .show(ui, |ui| gallery::show(ui, st));
            },
            (false, GalleryState::default()),
        );
    harness.run();
    shot(&mut harness, name);
}

#[test]
fn gallery() {
    let _rendu = rendu();
    gallery_in(ThemeChoice::Light, "galerie");
}

#[test]
fn gallery_dark() {
    let _rendu = rendu();
    gallery_in(ThemeChoice::Dark, "galerie_sombre");
}

// --- Écrans -----------------------------------------------------------------

#[test]
fn home() {
    let _rendu = rendu();
    snap("accueil_0", |_| {});
}

#[test]
fn home_with_refused_files() {
    let _rendu = rendu();
    snap("accueil_1_fichiers_refuses", |app| {
        app.add_paths(vec![
            exemple("ancien-rapport.doc"),
            exemple("Budget formation.gsheet"),
        ]);
        app.toasts = Default::default();
    });
}

#[test]
fn verify_busy() {
    let _rendu = rendu();
    snap("dossier_0_analyse_en_cours", |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files(FICHIERS.iter().map(|n| file_entry(exemple(n))), &supported);
        app.anonymize.busy = Some(Busy::Analyzing);
        app.workspace.enter(Step::Verify);
        // Deuxième fichier en cours : un terminé, deux en attente.
        app.progress().step(1, 4, "fichier-clients.xlsx");
    });
}

#[test]
fn verify() {
    let _rendu = rendu();
    snap("dossier_1_verifier", analyze);
}

#[test]
fn verify_table() {
    let _rendu = rendu();
    snap("dossier_1d_vue_tableau", |app| {
        analyze(app);
        app.anonymize.review.mode = VerifyMode::Table;
    });
}

#[test]
fn verify_selection() {
    let _rendu = rendu();
    snap("dossier_1e_selection", |app| {
        analyze_these(app, &["notes.txt"]);
        app.anonymize.review.queue_open = true;
        app.anonymize.sync_review(&app.session);
        app.anonymize.find_next("antenne", None);
    });
}

#[test]
fn verify_less_precise() {
    let _rendu = rendu();
    snap("dossier_1b_moins_precis", |app| {
        analyze_these(app, &["notes.txt"]);
        let session = app.session.clone();
        let st = &mut app.anonymize;
        for id in st.visible() {
            let general = st
                .candidate(id)
                .is_some_and(|c| matches!(c.kind, EntityKind::Age | EntityKind::Montant));
            if general {
                assert!(st.set_generalized(id, true, &session));
            }
        }
        st.always = vec![("Martinique".into(), EntityKind::Lieu)];
        // Les formes moins précises se lisent dans la liste « Masquées », dépliée.
        st.review.masked_open = true;
        st.review.queue_open = true;
    });
}

#[test]
fn verify_images() {
    let _rendu = rendu();
    snap("dossier_1c_images", |app| {
        analyze_these(app, &["carte-visite.png", "courrier-scanne.pdf"]);
        let info = app.anonymize.images()[0].clone();
        app.anonymize.add_mask(
            &info.file,
            &info.name,
            PixelRect {
                x: 50,
                y: 160,
                w: 500,
                h: 70,
            },
        );
    });
}

#[test]
fn bilan() {
    let _rendu = rendu();
    // Verdict non anonyme : un tableau dont des groupes de lignes restent isolables.
    snap("dossier_2_bilan", |app| {
        analyze_these(app, &BILAN);
        app.anonymize.bilan.guesses_open = true;
        app.workspace.enter(Step::Bilan);
    });
}

#[test]
fn bilan_corrected() {
    let _rendu = rendu();
    // Après « Tout corriger » : aucune personne reconnaissable.
    snap("dossier_2b_bilan_corrige", |app| {
        analyze_these(app, &BILAN);
        fix_all(app);
        app.workspace.enter(Step::Bilan);
    });
}

#[test]
fn send() {
    let _rendu = rendu();
    // Phrase de passe proposée, dossier des clés, conservation, fiche.
    snap("dossier_3_envoyer", |app| {
        analyze(app);
        ready_to_send(app);
        app.workspace.enter(Step::Send);
    });
}

#[test]
fn send_without_key() {
    let _rendu = rendu();
    // « Anonymiser définitivement », évaluation non concluante : confirmation.
    snap("dossier_3d_envoyer_sans_cle", |app| {
        analyze_these(app, &BILAN);
        app.workspace.goal = Goal::Anonymize;
        app.anonymize.apply_goal(Goal::Anonymize);
        app.workspace.enter(Step::Send);
    });
}

#[test]
fn sent() {
    let _rendu = rendu();
    snap("dossier_3b_envoye", |app| {
        anonymize_all(app, out_dir("envoye"));
    });
}

/// Avertissement du moteur pour une image masquée qu'un format ne conserve pas
/// à l'identique (cas d'un PDF scanné) : on l'ajoute au rapport du PDF.
fn with_unrestorable_warning(app: &mut AnonApp, tag: &str) {
    // Un dossier par test : deux tests en parallèle dans le même dossier se
    // marcheraient dessus et les fichiers seraient numérotés « _2 ».
    anonymize_all(app, out_dir(tag));
    let report = app.anonymize.report.as_mut().unwrap();
    let pdf = report
        .files
        .iter_mut()
        .find(|f| f.name == "courrier-scanne.pdf")
        .unwrap();
    let Outcome::Written { warnings, .. } = &mut pdf.outcome else {
        panic!("PDF non livré");
    };
    warnings.push(anon_core::Warning::new(
        anon_core::WarningCode::ImageNotRestorable,
        "pdf",
        "1 image(s) masquée(s) ne pourront pas être remises à la restauration dans ce format : conservez le fichier d'origine.",
    ));
}

#[test]
fn sent_unrestorable() {
    let _rendu = rendu();
    snap("dossier_3c_image_non_restaurable", |app| {
        with_unrestorable_warning(app, "non-restaurables")
    });
}

#[test]
fn response_locked_key() {
    let _rendu = rendu();
    snap("dossier_4_reponse_cle", |app| {
        app.add_paths(vec![PathBuf::from("C:/Dossiers/table_anonymiseur.anonmap")]);
        app.deanonymize
            .unlock_done(Err(anon_vault::VaultError::WrongPassword.to_string()));
    });
}

#[test]
fn response() {
    let _rendu = rendu();
    snap("dossier_4b_reponse", |app| {
        anonymize_all(app, out_dir("reponse"));
        app.workspace.enter(Step::Response);
        let engine = app.engine.clone().unwrap();
        let d = &mut app.deanonymize;
        d.pasted = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
        d.restore_text_marked(|t, s| engine.restore_text_marked(t, s))
            .unwrap();
    });
}

#[test]
fn response_expired_key() {
    let _rendu = rendu();
    // Durée de conservation dépassée : rappel en tête, destruction à confirmer.
    snap("dossier_4c_cle_expiree", |app| {
        anonymize_all(app, out_dir("cle-expiree"));
        app.workspace.enter(Step::Response);
        if let MapStatus::Unlocked { session, .. } = &mut app.deanonymize.map {
            session.keep_until = Some("2026-05-16".into());
        }
        app.deanonymize.destroy_confirm = true;
    });
}

#[test]
fn command_palette() {
    let _rendu = rendu();
    snap("palette", |app| {
        analyze_these(app, &["notes.txt"]);
        app.palette.open();
    });
}

#[test]
fn shortcuts_panel() {
    let _rendu = rendu();
    snap("raccourcis", |app| {
        app.show_shortcuts = true;
    });
}

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

fn hover_files(harness: &mut Harness<'_, AnonApp>, paths: &[PathBuf]) {
    for p in paths {
        harness.input_mut().hovered_files.push(egui::HoveredFile {
            path: Some(p.clone()),
            ..Default::default()
        });
    }
}

#[test]
fn home_drop_overlay() {
    let _rendu = rendu();
    // Voile pendant le survol de fichiers, en clair et en sombre.
    let mut results = SnapshotResults::new();
    for (theme, label) in [
        (ThemeChoice::Light, "1280x800"),
        (ThemeChoice::Dark, "sombre_1280x800"),
    ] {
        let mut h = app(1280.0, 800.0, |app| app.prefs.theme = theme);
        h.run();
        hover_files(
            &mut h,
            &[
                exemple("brief.md"),
                exemple("export.csv"),
                exemple("notes_anonymise.txt"),
            ],
        );
        h.run_steps(3);
        shot(&mut h, format!("accueil_2_voile_de_depot_{label}"));
        results.extend_harness(&mut h);
    }
    results.unwrap();
}

// --- Parcours au clavier ------------------------------------------------------

use egui::accesskit::Role;
use egui::{Key, Modifiers};

/// Ctrl+Maj (Cmd+Maj sous macOS).
fn cmd_shift() -> Modifiers {
    Modifiers::COMMAND | Modifiers::SHIFT
}

/// Dossier des exemples comme sous Documents : `…/Anonymiseur/Exemples`.
fn examples_dir(tag: &str) -> PathBuf {
    out_dir(tag).join("Anonymiseur").join("Exemples")
}

/// Tabulation jusqu'au widget `label` (au plus `max` fois).
fn tab_to(h: &mut Harness<'_, AnonApp>, label: &str, max: usize) {
    for _ in 0..max {
        if h.get_by_label(label).accesskit_node().is_focused() {
            return;
        }
        h.key_press(Key::Tab);
        h.run();
    }
    assert!(
        h.get_by_label(label).accesskit_node().is_focused(),
        "« {label} » n'a pas reçu le focus"
    );
}

/// Tabulation jusqu'au champ d'identifiant `id` (au plus `max` fois).
fn tab_to_field(h: &mut Harness<'_, AnonApp>, id: egui::Id, max: usize) {
    for _ in 0..max {
        if h.ctx.memory(|m| m.focused()) == Some(id) {
            return;
        }
        h.key_press(Key::Tab);
        h.run();
    }
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(id));
}

#[test]
fn parcours_complet_au_clavier() {
    let _rendu = rendu();
    let dir = examples_dir("clavier");
    let keys = out_dir("clavier-cles");
    let platform = NullPlatform {
        examples: Some(dir.clone()),
        keys: Some(keys.clone()),
        ..NullPlatform::default()
    };
    let mut h = app_with(1280.0, 800.0, platform, |_| {});
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Home);

    // Accueil : palette, « exemples », Entrée.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(h.state().palette.open);
    h.event(egui::Event::Text("exemples".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(!h.state().palette.open);
    assert_eq!(h.state().workspace.screen, Screen::Folder);
    assert_eq!(h.state().workspace.step, Step::Verify);
    assert_eq!(h.state().anonymize.files.len(), 17);
    let checked = h.state().anonymize.selected_count();
    assert!(checked > 10);

    // Vérifier : le tableau de la V1 est derrière « Vue tableau » (lot 7b) ;
    // une décision au clavier, annulée puis rétablie.
    tab_to(&mut h, "Vue tableau", 60);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().anonymize.review.mode, VerifyMode::Table);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run();
    tab_to(&mut h, "Tout décocher", 8);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().anonymize.selected_count(), 0);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().anonymize.selected_count(), checked);
    h.key_press_modifiers(cmd_shift(), Key::Z);
    h.run();
    assert_eq!(h.state().anonymize.selected_count(), 0);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().anonymize.selected_count(), checked);

    // Bilan : le verdict, puis « Tout corriger » depuis la palette, défait par
    // une seule annulation et rétabli.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Bilan);
    let failing = |h: &Harness<'_, AnonApp>| {
        let st = h.state();
        let a = st.anonymize.assessment(&st.assess_settings()).unwrap();
        a.assessment.subjects.iter().filter(|s| !s.passes()).count()
    };
    let before = failing(&h);
    assert!(before > 0, "des points à corriger sur les exemples");
    h.get_by_label("Tout corriger");
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.event(egui::Event::Text("tout corriger".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let after = failing(&h);
    assert!(after < before, "{after} points restants sur {before}");
    assert_eq!(
        h.state().anonymize.history.next_undo(),
        Some(anonymiseur::ui::state::history::Action::FixAll)
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(failing(&h), before, "une seule annulation");
    h.key_press_modifiers(cmd_shift(), Key::Z);
    h.run();
    assert_eq!(failing(&h), after);

    // Envoyer : une phrase de passe est proposée, la clé a son nom et son
    // dossier ; Ctrl+Entrée enregistre.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Send);
    assert!(h.state().anonymize.protect.uses_generated());
    h.get_by_label("Générer une autre");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    let report = h.state().anonymize.report.clone().unwrap();
    assert!(report.fatal.is_none());
    let map = report.map_path.clone().expect("clé écrite");
    assert_eq!(map.parent(), Some(keys.as_path()));
    assert!(file_name(&map).starts_with("dossier-"));
    assert!(report.delivered() >= 12);
    assert_eq!(h.state().session.goal, Goal::Mask);
    assert!(h.state().deanonymize.is_unlocked(), "clé gardée en mémoire");

    // Réponse : Ctrl+Maj+V colle et restaure, sans redemander le mot de passe.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Response);
    h.key_press_modifiers(cmd_shift(), Key::V);
    h.run();
    let reponse = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
    h.input_mut().events.push(egui::Event::Paste(reponse));
    h.run();
    let (text, r) = h.state().deanonymize.text_result.clone().unwrap();
    assert!(r.restored > 0);
    assert!(!text.contains("[PERSONNE_1]"));
    assert_eq!(r.unknown, vec!["[PERSONNE_99]".to_owned()]);
    h.get_by_label_contains("1 balise inconnue : [PERSONNE_99]");
    // Ctrl+Entrée à la dernière étape : copie la réponse restaurée.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert!(
        h.state()
            .toasts
            .texts()
            .contains(&"Réponse restaurée copiée")
    );
}

#[test]
fn palette_de_commandes() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, analyze);
    h.run();
    // Recherche sans accents ni majuscules, Entrée.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.event(egui::Event::Text("THEME SOMBRE".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().prefs.theme, ThemeChoice::Dark);
    assert!(!h.state().palette.open);
    // Flèches : « étape » liste les quatre étapes ; ↓ choisit Bilan.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.event(egui::Event::Text("etape".into()));
    h.run();
    h.key_press(Key::ArrowDown);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Bilan);
    // Action impossible : la raison, et la palette reste ouverte.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.event(egui::Event::Text("aller reponse".into()));
    h.run();
    h.get_by_label_contains("Enregistrez d'abord les documents");
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().palette.open);
    assert_eq!(h.state().workspace.step, Step::Bilan);
    assert!(
        h.state()
            .toasts
            .texts()
            .iter()
            .any(|t| t.starts_with("Enregistrez d'abord"))
    );
    // Échap ferme.
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().palette.open);
    // Chaque ligne affiche son raccourci.
    let entries = h.state().palette_entries();
    let undo = entries
        .iter()
        .find(|e| e.label == "Annuler la dernière décision")
        .unwrap();
    assert_eq!(
        undo.shortcut,
        Some(anonymiseur::ui::shortcuts::UNDO.label())
    );
}

#[test]
fn texte_colle_devient_un_document() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    h.input_mut().events.push(egui::Event::Paste(
        "Merci d'écrire à helene.kaloi@example.com avant lundi.".into(),
    ));
    h.run();
    let st = &h.state().anonymize;
    assert_eq!(st.pasted.len(), 1);
    assert!(st.analysis.is_some());
    assert!(
        st.analysis
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .any(|c| c.selected && c.value == "helene.kaloi@example.com")
    );
    assert_eq!(h.state().workspace.step, Step::Verify);
    h.get_by_label("Nouvelle session").click();
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Home);
    assert_eq!(h.state().anonymize.documents(), 0);
    assert!(h.state().session.entities.is_empty());
}

#[test]
fn objectif_anonymiser_definitivement_sans_cle() {
    let _rendu = rendu();
    let dir = out_dir("sans-cle");
    let mut h = app(1280.0, 1600.0, |app| {
        analyze_these(app, &["notes.txt"]);
        app.anonymize.out_dir = Some(dir.clone());
        app.workspace.home();
    });
    h.run();
    // Choix sur l'accueil, puis retour au dossier.
    h.get_by_label_contains("Anonymiser définitivement.")
        .click();
    h.run();
    assert_eq!(h.state().workspace.goal, Goal::Anonymize);
    h.get_by_label_contains("Reprendre le dossier").click();
    h.run();
    h.state_mut().workspace.enter(Step::Send);
    h.run();
    // Aucun mot de passe demandé. Sans conclusion d'anonymat, l'envoi se
    // confirme d'abord (lot 8) ; les fichiers prennent alors `_masque_sans_cle`.
    h.get_by_label("Aucune clé de restauration");
    let anonymous = {
        let st = h.state();
        let a = st.anonymize.assessment(&st.assess_settings()).unwrap();
        a.assessment.is_anonymous()
    };
    if !anonymous {
        let go = h.get_by_label("Masquer et enregistrer");
        assert!(go.accesskit_node().is_disabled(), "confirmation exigée");
        h.get_by_label("Confirmer l'envoi sans correction : fichiers masqués, sans clé")
            .click();
        h.run();
    }
    h.get_by_label("Masquer et enregistrer").click();
    h.run();
    let report = h.state().anonymize.report.clone().unwrap();
    assert_eq!(report.delivered(), 1);
    assert!(report.map_path.is_none(), "aucune clé écrite");
    let Outcome::Written { path, .. } = &report.files[0].outcome else {
        panic!("{:?}", report.files[0].outcome);
    };
    let expected = if anonymous {
        "notes_anonyme.txt"
    } else {
        "notes_masque_sans_cle.txt"
    };
    assert_eq!(file_name(path), expected);
    assert_eq!(h.state().session.goal, Goal::Anonymize);
    assert!(h.state().session.irreversible);
    assert!(!std::fs::read_dir(dir.join("")).unwrap().any(|e| {
        e.unwrap()
            .path()
            .extension()
            .is_some_and(|x| x == "anonmap")
    }));
    // L'étape Réponse dit pourquoi elle est inaccessible.
    assert!(!h.state().deanonymize.has_key());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num4);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Send);
    assert!(
        h.state()
            .toasts
            .texts()
            .iter()
            .any(|t| t.starts_with("Aucune clé"))
    );
}

#[test]
fn cle_ouverte_depuis_l_accueil_mene_a_la_reponse() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(PathBuf::from(
            "C:/d/table_anonymiseur.anonmap",
        ))));
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Folder);
    assert_eq!(h.state().workspace.step, Step::Response);
    assert!(matches!(
        h.state().deanonymize.map,
        MapStatus::Locked { .. }
    ));
    // Le mot de passe a le focus.
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(anonymiseur::ui::widgets::field::field_id(
            anonymiseur::ui::screens::response::PASSWORD_FIELD
        ))
    );
    h.get_by_label("Ouvrir la clé");
}

// --- Interactions (sans image) ---------------------------------------------

#[test]
fn dropped_files_are_analyzed_or_refused() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    for p in [
        exemple("notes.txt"),
        exemple("vieux.xls"),
        exemple("notes_anonymise.txt"),
        exemple("table_anonymiseur.anonmap"),
    ] {
        h.input_mut().dropped_files.push(Arc::new(Dropped(p)));
    }
    h.run();
    assert_eq!(h.state().anonymize.files.len(), 1);
    assert_eq!(h.state().anonymize.rejected.len(), 1);
    assert!(h.state().anonymize.analysis.is_some(), "analyse lancée");
    assert_eq!(h.state().workspace.step, Step::Verify);
    // Une seule notification, qui dit tout ; la clé est retenue pour la réponse.
    let toasts = h.state().toasts.texts();
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    let t = toasts[0];
    assert!(t.starts_with("1 fichier ajouté au dossier"), "{t}");
    assert!(t.contains("1 fichier déjà masqué ignoré"), "{t}");
    assert!(t.contains("1 fichier refusé"), "{t}");
    assert!(h.state().deanonymize.has_key());
}

#[test]
fn retour_a_l_accueil_et_reprise_du_dossier() {
    let _rendu = rendu();
    let mut h = app(1280.0, 1600.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    h.get_by_label("Accueil").click();
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Home);
    h.get_by_label("Reprendre le dossier (1 document)").click();
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Folder);
    assert_eq!(h.state().workspace.step, Step::Verify);
}

/// Onglet d'étape de l'en-tête.
fn step_tab<'h>(h: &'h Harness<'_, AnonApp>, label: &'h str) -> egui_kittest::Node<'h> {
    h.get_all_by_label(label)
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
}

#[test]
fn etapes_par_clic_et_clavier() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    step_tab(&h, "Bilan").click();
    h.run();
    assert_eq!(h.state().workspace.step, Step::Bilan);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num3);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Send);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Verify);
    // Réponse : inaccessible avant l'envoi, l'onglet est désactivé.
    assert!(step_tab(&h, "Réponse").accesskit_node().is_disabled());
    step_tab(&h, "Réponse").click();
    h.run();
    assert_eq!(h.state().workspace.step, Step::Verify);
    // Échap : étape précédente, puis accueil.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Verify);
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Home);
}

#[test]
fn annuler_retablir_une_zone_tracee() {
    let _rendu = rendu();
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &["carte-visite.png"]);
    });
    h.run();
    let canvas = h
        .get_by_label("Aperçu de carte-visite.png : tracez un rectangle pour masquer une zone")
        .rect();
    let a = canvas.min + egui::vec2(20.0, 20.0);
    let b = canvas.min + egui::vec2(120.0, 60.0);
    h.hover_at(a);
    h.run();
    h.drag_at(a);
    h.run();
    h.hover_at(b);
    h.run();
    h.drop_at(b);
    h.run();
    let masks = h.state().anonymize.masks.masks.clone();
    assert_eq!(masks.len(), 1, "{masks:?}");
    let r = masks[0].mask.rect;
    assert!(r.w > 0 && r.h > 0 && r.w > r.h, "{r:?}");
    // Annuler (bouton de la barre du dossier), rétablir (clavier).
    h.get_by_label("Annuler").click();
    h.run();
    assert!(h.state().anonymize.masks.is_empty());
    assert_eq!(h.state().toasts.texts(), ["Annulé : zone ajoutée."]);
    h.key_press_modifiers(cmd_shift(), Key::Z);
    h.run();
    assert_eq!(h.state().anonymize.masks.len(), 1);
    h.get_by_label("Retirer la zone 1").click();
    h.run();
    assert!(h.state().anonymize.masks.is_empty());
}

#[test]
fn rapport_signale_les_images_non_restaurables() {
    let _rendu = rendu();
    let mut h = app(1280.0, 2400.0, |app| {
        with_unrestorable_warning(app, "non-restaurables-rapport");
        neutralize_paths(app);
    });
    h.run();
    h.get_by_label("Images masquées non restaurables");
}

#[test]
fn bouton_exemples_sans_documents_demande_un_dossier() {
    let _rendu = rendu();
    // Dossier Documents introuvable : repli sur le dialogue, annulé ici.
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    h.get_by_label("Essayer avec des exemples").click();
    h.run();
    assert!(h.state().anonymize.files.is_empty());
    assert_eq!(h.state().workspace.screen, Screen::Home);
}

#[test]
fn bouton_exemples_reutilise_le_meme_dossier() {
    let _rendu = rendu();
    let dir = examples_dir("exemples-un-clic");
    let platform = NullPlatform {
        examples: Some(dir.clone()),
        ..NullPlatform::default()
    };
    let mut h = app_with(1280.0, 800.0, platform, |_| {});
    h.run();
    h.get_by_label("Essayer avec des exemples").click();
    h.run();
    let files = h.state().anonymize.files.clone();
    assert_eq!(files.len(), 17, "documents d'exemple seulement");
    assert!(files.iter().all(|f| f.path.starts_with(&dir)));
    assert!(
        files
            .iter()
            .all(|f| f.name != "LISEZMOI.md" && f.name != "reponse-ia-exemple.md")
    );
    assert_eq!(
        h.state().anonymize.out_dir,
        Some(dir.join("Anonymisés")),
        "sorties dans un sous-dossier"
    );
    assert_eq!(h.state().workspace.step, Step::Verify);
    // Un fichier produit par l'outil dans le dossier, un exemple modifié : au
    // second clic, rien n'est recopié ni ajouté deux fois, rien n'est écrasé.
    std::fs::write(dir.join("notes_anonymise.txt"), "sortie").unwrap();
    std::fs::write(dir.join("notes.txt"), "modifié").unwrap();
    h.get_by_label("Accueil").click();
    h.run();
    h.get_by_label("Essayer avec des exemples").click();
    h.run();
    assert_eq!(h.state().anonymize.files.len(), 17);
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "modifié"
    );
    assert!(!dir.with_file_name("Exemples_2").exists());
}

/// Rectangles des nœuds d'accessibilité nommés `labels`.
fn rects(harness: &Harness<'_, AnonApp>, labels: &[&str]) -> Vec<(String, egui::Rect)> {
    labels
        .iter()
        .map(|l| ((*l).to_owned(), harness.get_by_label(l).rect()))
        .collect()
}

/// Les contrôles d'une même ligne ont la même hauteur et le même centre
/// vertical, au pixel près.
fn assert_aligned(screen: &str, row: &[(String, egui::Rect)]) {
    let Some((first, r0)) = row.first() else {
        return;
    };
    for (name, r) in row {
        assert!(
            (r.height() - r0.height()).abs() <= 1.0,
            "{screen} : « {name} » mesure {} px de haut, « {first} » {} px",
            r.height(),
            r0.height()
        );
        assert_centered(screen, name, *r, first, *r0);
    }
}

fn assert_centered(screen: &str, name: &str, r: egui::Rect, first: &str, r0: egui::Rect) {
    assert!(
        (r.center().y - r0.center().y).abs() <= 1.0,
        "{screen} : « {name} » centré à y = {}, « {first} » à y = {}",
        r.center().y,
        r0.center().y
    );
}

#[test]
fn controles_alignes_sur_chaque_ecran() {
    let _rendu = rendu();
    use anonymiseur::ui::theme::{CONTROL_MD, CONTROL_SM};
    // Accueil : boutons de la zone de dépôt, liens.
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    let row = rects(&h, &["Choisir des fichiers", "Coller un texte"]);
    assert_eq!(row[0].1.height(), CONTROL_SM);
    assert_aligned("Accueil, dépôt", &row);
    assert_aligned(
        "Accueil, liens",
        &rects(
            &h,
            &[
                "Restaurer une réponse avec une clé",
                "Essayer avec des exemples",
            ],
        ),
    );
    // En-tête : palette et thème.
    let head = rects(&h, &["Palette de commandes (Ctrl+K)", "Thème : Système"]);
    assert_eq!(head[0].1.height(), CONTROL_MD);
    assert_aligned("En-tête", &head);

    // Vérifier, vue tableau : ajout manuel, liste « toujours », barres du
    // dossier et d'action.
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &["notes.txt"]);
        app.anonymize.review.mode = VerifyMode::Table;
    });
    h.run();
    let row = rects(&h, &["Terme à ajouter", "Type du terme ajouté", "Ajouter"]);
    assert_eq!(row[0].1.height(), CONTROL_MD);
    assert_aligned("Vérifier, ajout", &row);
    assert_aligned(
        "Vérifier, toujours",
        &rects(
            &h,
            &[
                "Terme à toujours masquer",
                "Type du terme",
                "Ajouter à la liste",
            ],
        ),
    );
    assert_aligned("Barre d'action", &rects(&h, &["Accueil", "Voir le bilan"]));
    let bar = rects(
        &h,
        &[
            "Objectif : Masquer pour l'IA",
            "Annuler",
            "Pour : Assistant IA grand public",
        ],
    );
    assert_eq!(bar[0].1.height(), CONTROL_SM);
    assert_aligned("Barre du dossier", &bar);
    // Étapes de l'en-tête centrées sur les boutons de l'en-tête.
    let theme_btn = h.get_by_label("Thème : Système").rect();
    for step in ["Vérifier", "Bilan", "Envoyer", "Réponse"] {
        let r = step_tab(&h, step).rect();
        assert_centered("En-tête, étapes", step, r, "Thème", theme_btn);
    }

    // Vérifier dans le document : boutons de la file, sélection.
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    for b in ["Masquer", "Garder", "Suivante", "Précédente"] {
        assert_eq!(h.get_by_label(b).rect().height(), CONTROL_SM, "{b}");
    }
    h.state_mut().anonymize.find_next("antenne", None);
    h.run();
    let sel = rects(
        &h,
        &[
            "Masquer partout",
            "Toujours masquer",
            "Annuler la sélection",
        ],
    );
    assert_eq!(sel[0].1.height(), CONTROL_SM);
    assert_aligned("Vérifier, sélection", &sel);

    // Envoyer : barre d'action.
    let mut h = app(1280.0, 1600.0, |app| {
        analyze_these(app, &["notes.txt"]);
        app.workspace.enter(Step::Send);
    });
    h.run();
    assert_aligned("Envoyer", &rects(&h, &["Retour", "Masquer et enregistrer"]));

    // Réponse : ouverture de la clé, puis colonnes.
    let mut h = app(1280.0, 800.0, |app| {
        app.add_paths(vec![PathBuf::from("C:/Dossiers/table_anonymiseur.anonmap")]);
    });
    h.run();
    assert_aligned(
        "Réponse, clé",
        &rects(&h, &["Ouvrir la clé", "Changer de clé"]),
    );
    let mut h = app(1280.0, 800.0, |app| {
        anonymize_all(app, out_dir("alignement"));
        app.workspace.enter(Step::Response);
    });
    h.run();
    let coller = h.get_by_label("Coller").rect();
    let copier = h.get_by_label("Copier").rect();
    assert_aligned(
        "Réponse, colonnes",
        &[("Coller".to_owned(), coller), ("Copier".to_owned(), copier)],
    );
}

#[test]
fn premiere_decision_visible_sans_defiler() {
    let _rendu = rendu();
    // Dans le document : la valeur courante de la file et ses boutons M et G,
    // au-dessus de la barre d'action, en 1280 × 800 comme en 900 × 650.
    for (w, hh) in [(1280.0, 800.0), (900.0, 650.0)] {
        let mut h = app(w, hh, analyze);
        h.run();
        let action = h.get_by_label("Voir le bilan").rect();
        for b in ["Masquer", "Garder"] {
            let r = h.get_by_label(b).rect();
            assert!(
                r.max.y < action.min.y - 8.0,
                "{w} × {hh} : « {b} » à y = {}, barre d'action à y = {}",
                r.max.y,
                action.min.y
            );
        }
        assert!(h.state().anonymize.review.current.is_some());
    }
    // Vue tableau : la première ligne.
    let mut h = app(1280.0, 800.0, |app| {
        analyze(app);
        app.anonymize.review.mode = VerifyMode::Table;
    });
    h.run();
    let st = &h.state().anonymize;
    let first = st.visible()[0];
    let label = format!("Sélectionner {}", st.candidate(first).unwrap().value);
    let row = h.get_by_label(&label).rect();
    let action = h.get_by_label("Voir le bilan").rect();
    assert!(
        row.max.y < action.min.y - 8.0,
        "première valeur à y = {}, barre d'action à y = {}",
        row.max.y,
        action.min.y
    );
}

#[test]
fn zones_de_texte_lisibles() {
    let _rendu = rendu();
    // 1280 × 800 : au moins 12 lignes visibles dans la réponse collée, texte de 15 px.
    let mut h = app(1280.0, 800.0, |app| {
        anonymize_all(app, out_dir("zones"));
        app.workspace.enter(Step::Response);
    });
    h.run();
    let area = h.get_by_label("Réponse de l'IA à restaurer").rect();
    let line = anonymiseur::ui::widgets::field::area_line_height();
    assert!(area.height() / line >= 12.0, "{} px", area.height());
    assert!(anonymiseur::ui::widgets::field::mono_font().size >= 15.0);
}

#[test]
fn choix_du_theme_depuis_l_en_tete() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    assert_eq!(h.ctx.theme(), egui::Theme::Light);
    h.get_by_label("Thème : Système").click();
    h.run();
    h.get_by_label("Sombre").click();
    h.run();
    assert_eq!(h.state().prefs.theme, ThemeChoice::Dark);
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    h.get_by_label("Thème : Sombre");
}

#[test]
fn destinataire_et_objectif_depuis_la_barre_du_dossier() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    h.get_by_label("Pour : Assistant IA grand public").click();
    h.run();
    h.get_by_label("Publication").click();
    h.run();
    assert_eq!(
        h.state().workspace.recipient,
        anon_core::Recipient::Publication
    );
    assert_eq!(
        h.state().prefs.recipient,
        anon_core::Recipient::Publication,
        "retenu pour la prochaine fois"
    );
    h.get_by_label("Objectif : Masquer pour l'IA").click();
    h.run();
    h.get_by_label("Anonymiser définitivement").click();
    h.run();
    assert_eq!(h.state().workspace.goal, Goal::Anonymize);
    assert_eq!(
        h.state().anonymize.protect.mode,
        anonymiseur::ui::state::anonymize::ProtectMode::Irreversible
    );
}

use anon_engine::Progress as _;
use anonymiseur::ui::screens::verify::TABLE_ID;
use anonymiseur::ui::widgets::DataTable;
use anonymiseur::ui::widgets::field::field_id;

#[test]
fn copie_avec_notification_et_libelle_copie() {
    let _rendu = rendu();
    let mut h = app(1280.0, 1600.0, |app| {
        anonymize_all(app, out_dir("copie"));
        neutralize_paths(app);
    });
    h.run();
    h.get_by_label("Copier la consigne pour l'IA").click();
    h.run();
    h.get_by_label("Copié");
    assert_eq!(h.state().toasts.texts(), ["Consigne copiée"]);
    h.get_by_role_and_label(Role::Status, "Consigne copiée");
    // Fermeture de la notification.
    h.get_by_label("Fermer la notification : Consigne copiée")
        .click();
    h.run();
    assert!(h.state().toasts.is_empty());
    // Le texte masqué et la consigne, en un clic.
    h.get_by_label("Copier le texte et la consigne").click();
    h.run();
    let t = h.state().toasts.texts();
    assert!(
        t.iter().any(|x| x.starts_with("Texte et consigne copiés")),
        "{t:?}"
    );
}

#[test]
fn action_desactivee_dit_pourquoi() {
    let _rendu = rendu();
    let mut h = app(1280.0, 1600.0, |app| {
        analyze_these(app, &["notes.txt"]);
        app.workspace.enter(Step::Send);
    });
    h.run();
    h.get_by_label("Choisissez un mot de passe pour la clé de restauration.");
    let go = h.get_by_label("Masquer et enregistrer");
    assert!(go.accesskit_node().is_disabled());
}

#[test]
fn raccourcis_clavier() {
    let _rendu = rendu();
    let mut h = app(1280.0, 1600.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    // Recherche dans le tableau.
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run();
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(field_id(anonymiseur::ui::app::SEARCH_FIELD))
    );
    // Échap quitte d'abord le champ, puis revient à l'accueil.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().workspace.screen, Screen::Home);
    // F1 : panneau des raccourcis.
    h.key_press(Key::F1);
    h.run();
    assert!(h.state().show_shortcuts);
    h.get_by_label("Ctrl+K");
    h.get_by_label("Ctrl+Z, Ctrl+Maj+Z");
    h.get_by_label("Ctrl+Maj+V");
    h.get_by_label("Fermer").click();
    h.run();
    assert!(!h.state().show_shortcuts);
    // Ctrl+K ouvre et ferme la palette.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(h.state().palette.open);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(!h.state().palette.open);
}

#[test]
fn tableau_tri_recherche_et_clavier() {
    let _rendu = rendu();
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &["notes.txt"]);
        app.anonymize.review.mode = VerifyMode::Table;
    });
    h.run();
    // Tri au clic sur l'en-tête.
    h.get_by_label("Trier par Valeur").click();
    h.run();
    let sort = h.state().anonymize.sort.unwrap();
    assert!(sort.ascending);
    h.get_by_label("Trier par Valeur : croissant");
    // Résumé.
    let summary = h.state().anonymize.review_summary();
    h.get_by_label(&summary);
    // Clavier : ↓ passe à la ligne suivante, Espace la coche ou la décoche, et
    // Ctrl+Z annule même quand la case a le focus.
    let visible = h.state().anonymize.visible();
    let name = |i: usize| {
        let st = &h.state().anonymize;
        format!("Sélectionner {}", st.candidate(visible[i]).unwrap().value)
    };
    let first = name(0);
    h.get_by_label(&first).focus();
    h.run();
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(DataTable::row_check_id(TABLE_ID, 1))
    );
    let before = h.state().anonymize.candidate(visible[1]).unwrap().selected;
    h.key_press(Key::Space);
    h.run();
    assert_ne!(
        h.state().anonymize.candidate(visible[1]).unwrap().selected,
        before
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(
        h.state().anonymize.candidate(visible[1]).unwrap().selected,
        before
    );
    // Recherche : seules les lignes correspondantes restent.
    h.state_mut().anonymize.search = "example.com".into();
    h.run();
    let found = h.state().anonymize.visible();
    assert!(!found.is_empty() && found.len() < visible.len());
}

#[test]
fn voile_de_depot_selon_l_etape() {
    let _rendu = rendu();
    // Accueil : le voile annonce ce qui va se passer.
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    hover_files(&mut h, &[exemple("notes.txt"), exemple("brief.md")]);
    // egui redessine sans cesse pendant le survol : quelques images suffisent.
    h.run_steps(3);
    h.get_by_label("Déposez pour ajouter 2 fichiers au dossier");
    h.input_mut().hovered_files.clear();
    h.run();
    // Réponse : les fichiers déposés sont à restaurer.
    let mut h = app(1280.0, 800.0, |app| {
        anonymize_all(app, out_dir("voile-reponse"));
        app.workspace.enter(Step::Response);
    });
    h.run();
    hover_files(&mut h, &[exemple("notes.txt")]);
    h.run_steps(3);
    h.get_by_label("Déposez pour restaurer 1 fichier renvoyé par l'IA");
    h.input_mut().hovered_files.clear();
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(exemple("notes.txt"))));
    h.run();
    assert_eq!(h.state().deanonymize.input_files.len(), 1);
}

#[test]
fn coller_et_restaurer_en_un_geste() {
    let _rendu = rendu();
    let mut h = app(1280.0, 1600.0, |app| {
        anonymize_all(app, out_dir("coller"));
        app.workspace.enter(Step::Response);
    });
    h.run();
    // La clé créée à l'envoi est ouverte : aucun mot de passe demandé.
    assert!(h.query_by_label("Mot de passe de la clé").is_none());
    h.get_by_label("Coller").click();
    h.run();
    let reponse = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
    h.input_mut().events.push(egui::Event::Paste(reponse));
    h.run();
    let (text, report) = h.state().deanonymize.text_result.clone().unwrap();
    assert!(!text.contains("[PERSONNE_1]"));
    assert_eq!(report.unknown, vec!["[PERSONNE_99]".to_owned()]);
    // Balise inconnue signalée, avec son nom.
    h.get_by_label_contains("1 balise inconnue : [PERSONNE_99]");
    h.get_by_label("Copier").click();
    h.run();
    h.get_by_label("Copié");
    assert!(
        h.state()
            .toasts
            .texts()
            .contains(&"Réponse restaurée copiée")
    );
}

#[test]
fn avancement_par_fichier() {
    let _rendu = rendu();
    let mut h = app(1280.0, 1600.0, |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files(FICHIERS.iter().map(|n| file_entry(exemple(n))), &supported);
        app.anonymize.busy = Some(Busy::Analyzing);
        app.workspace.enter(Step::Verify);
        app.progress().step(1, 4, "fichier-clients.xlsx");
    });
    h.run();
    h.get_by_label("Terminé");
    h.get_by_label("En cours");
    assert_eq!(h.get_all_by_label("En attente").count(), 2);
    h.get_by_label("Temps écoulé : 0 s");
    assert!(
        h.get_all_by_label("Annuler")
            .any(|n| n.accesskit_node().role() == Role::Button && !n.accesskit_node().is_disabled()),
        "le traitement s'annule"
    );
}

/// Préparation d'un écran.
type Setup = Box<dyn Fn(&mut AnonApp)>;

/// Écrans d'une revue complète : accueil, chaque étape, palette ouverte.
fn every_screen() -> Vec<(&'static str, Setup)> {
    vec![
        ("accueil", Box::new(|_: &mut AnonApp| {})),
        (
            "vérifier",
            Box::new(|app: &mut AnonApp| analyze_these(app, &["notes.txt", "carte-visite.png"])),
        ),
        (
            "vérifier, images",
            Box::new(|app: &mut AnonApp| {
                analyze_these(app, &["notes.txt", "carte-visite.png"]);
                app.anonymize.select_document(1);
            }),
        ),
        (
            "vérifier, vue tableau",
            Box::new(|app: &mut AnonApp| {
                analyze_these(app, &["notes.txt", "carte-visite.png"]);
                app.anonymize.review.mode = VerifyMode::Table;
            }),
        ),
        (
            "bilan",
            Box::new(|app: &mut AnonApp| {
                analyze_these(app, &["notes.txt"]);
                app.workspace.enter(Step::Bilan);
            }),
        ),
        (
            "envoyer",
            Box::new(|app: &mut AnonApp| {
                analyze_these(app, &["notes.txt"]);
                app.workspace.enter(Step::Send);
            }),
        ),
        (
            "envoyé",
            Box::new(|app: &mut AnonApp| anonymize_all(app, out_dir("revue-envoye"))),
        ),
        (
            "réponse",
            Box::new(|app: &mut AnonApp| {
                anonymize_all(app, out_dir("revue-reponse"));
                app.workspace.enter(Step::Response);
                let engine = app.engine.clone().unwrap();
                app.deanonymize.pasted =
                    std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
                let _ = app
                    .deanonymize
                    .restore_text(|t, s| engine.restore_text(t, s));
            }),
        ),
        (
            "clé à ouvrir",
            Box::new(|app: &mut AnonApp| {
                app.add_paths(vec![PathBuf::from("C:/d/table_anonymiseur.anonmap")]);
            }),
        ),
        (
            "palette",
            Box::new(|app: &mut AnonApp| {
                analyze_these(app, &["notes.txt"]);
                app.palette.open();
            }),
        ),
        (
            "raccourcis",
            Box::new(|app: &mut AnonApp| app.show_shortcuts = true),
        ),
    ]
}

#[test]
fn chaque_bouton_a_un_nom() {
    let _rendu = rendu();
    for (screen, setup) in every_screen() {
        let mut h = app(1280.0, 2400.0, setup);
        h.run();
        for n in h.get_all_by_role(Role::Button) {
            let label = n.accesskit_node().label().unwrap_or_default();
            assert!(
                !label.trim().is_empty(),
                "{screen} : bouton sans nom : {n:?}"
            );
        }
    }
}

/// Tous les textes lus par les lecteurs d'écran (libellés et valeurs).
fn all_texts(h: &Harness<'_, AnonApp>) -> Vec<String> {
    h.root()
        .children_recursive()
        .flat_map(|n| {
            let a = n.accesskit_node();
            [a.label(), a.value()]
        })
        .flatten()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn vocabulaire_v2_sur_chaque_ecran() {
    let _rendu = rendu();
    // Termes de la V1 remplacés (plan V2, section 7.4), et le mot « anonyme »
    // réservé à une évaluation réussie (invariant 9) : absents de l'interface.
    // Les noms de fichiers (suffixe de la V1, gardé jusqu'au lot 8) et les
    // données des exemples ne sont pas des textes de l'interface : exclus.
    let forbidden = [
        "anonyme",
        "table de correspondance",
        "contrôle de fuite",
        "généralis",
        "désanonymis",
        "anonymisé",
        "texte libre",
        "casse",
        "occ.",
    ];
    let examples = std::fs::read_dir(exemple(""))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    for (screen, setup) in every_screen() {
        let mut h = app(1280.0, 2400.0, setup);
        h.run();
        // Contenu des documents : valeurs, extraits, réponse collée et restaurée.
        let st = h.state();
        let mut content: Vec<String> = vec![st.deanonymize.pasted.to_lowercase()];
        if let Some((t, _)) = &st.deanonymize.text_result {
            content.push(t.to_lowercase());
        }
        if let Some(a) = &st.anonymize.analysis {
            for c in &a.candidates {
                content.push(c.value.to_lowercase());
                content.push(c.context.to_lowercase());
            }
        }
        content.retain(|c| c.chars().count() >= 4);
        for text in all_texts(&h) {
            let lower = text.to_lowercase();
            // Nom d'un fichier produit ou d'un exemple, ou contenu d'un document.
            let not_ui = lower.contains("_anonymise")
                || lower.contains('/')
                || lower.contains('\\')
                || examples.iter().any(|e| lower.contains(e.as_str()))
                || content
                    .iter()
                    .any(|c| lower.contains(c.as_str()) || c.contains(lower.as_str()));
            if not_ui {
                continue;
            }
            for word in forbidden {
                assert!(
                    !lower.contains(word),
                    "{screen} : « {word} » dans « {text} »"
                );
            }
        }
    }
}

// --- Vérifier dans le document (lot 7b) ----------------------------------------

use anonymiseur::ui::screens::document::{ValueInfo, announce, has_focus};
use anonymiseur::ui::state::anonymize::needs_confirmation;

/// Annonce, pour les lecteurs d'écran, de l'occurrence `o` du document affiché.
fn span_label(h: &Harness<'_, AnonApp>, o: Occurrence) -> String {
    let st = &h.state().anonymize;
    let span = st.span_at(o).unwrap().1;
    let c = st.candidate(span.candidate).unwrap();
    let info = ValueInfo {
        value: c.value.clone(),
        kind: c.kind,
        occurrences: c.occurrences,
        pending: st.is_undecided(c.id),
        replacement: None,
    };
    announce(&span, &info)
}

/// Le nœud focalisé porte ce libellé.
fn focused_label(h: &Harness<'_, AnonApp>, label: &str) -> bool {
    h.get_all_by_label(label)
        .any(|n| n.accesskit_node().is_focused())
}

fn is_masked(h: &Harness<'_, AnonApp>, id: usize) -> bool {
    h.state()
        .anonymize
        .candidate(id)
        .is_some_and(|c| c.selected)
}

#[test]
fn decisions_par_document_d_exemple() {
    let _rendu = rendu();
    // Plan V2, section 8 : au plus une dizaine de décisions par document
    // d'exemple (95 valeurs listées en V1). Mesure sur le dossier d'exemples
    // complet, tel que « Essayer avec des exemples » l'analyse ; le compteur de
    // la colonne Documents est la longueur de la file de chaque document.
    let names: Vec<&str> = anonymiseur::ui::service::EXAMPLES
        .iter()
        .filter(|e| e.document)
        .map(|e| e.name)
        .collect();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &names));
    h.run();
    let docs = h.state().anonymize.review_documents();
    assert_eq!(docs.len(), names.len());
    let mut lines = Vec::new();
    let mut total = 0;
    for (i, name) in docs {
        h.state_mut().anonymize.select_document(i);
        h.run();
        let st = &h.state().anonymize;
        assert_eq!(st.review_document_name().as_deref(), Some(name.as_str()));
        let queue = st.queue();
        assert!(
            queue
                .iter()
                .all(|id| st.candidate(*id).is_some_and(needs_confirmation)),
            "{name} : seules les valeurs ambiguës ou incertaines sont à décider"
        );
        assert_eq!(queue.len(), st.remaining_in(&name), "{name}");
        let sure = st.sure_values().len();
        total += queue.len();
        lines.push(format!(
            "{name} : {} à décider, {sure} masquées d'office",
            queue.len()
        ));
    }
    println!(
        "Décisions par document d'exemple :\n{}\nTotal : {total}",
        lines.join("\n")
    );
}

#[test]
fn verifier_au_clavier_dans_le_document() {
    let _rendu = rendu();
    // Un document dont les valeurs à décider viennent des dictionnaires
    // (métiers de la colonne « Poste ») : au moins deux, avec ou sans IA de
    // noms. Dans notes.txt, l'édition Lite n'en trouve qu'une.
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["planning.ods"]));
    h.run();
    let doc = h.state().anonymize.review_document_name().unwrap();
    let queue = h.state().anonymize.queue();
    assert!(queue.len() >= 2, "{} valeurs à décider", queue.len());
    assert_eq!(h.state().anonymize.review.current, Some(queue[0]));

    // Tab atteint le texte : l'occurrence courante a le focus, annoncée.
    for _ in 0..80 {
        if has_focus(&h.ctx, &doc) {
            break;
        }
        h.key_press(Key::Tab);
        h.run();
    }
    assert!(has_focus(&h.ctx, &doc), "Tab n'atteint pas le texte");
    let c0 = h.state().anonymize.review.cursor.unwrap();
    assert!(focused_label(&h, &span_label(&h, c0)));
    assert!(span_label(&h, c0).ends_with(" fois"));

    // → : occurrence suivante, le focus suit ; ← revient.
    h.key_press(Key::ArrowRight);
    h.run();
    let c1 = h.state().anonymize.review.cursor.unwrap();
    assert_ne!(c0, c1);
    assert!(
        focused_label(&h, &span_label(&h, c1)),
        "focus sur la suivante"
    );
    h.key_press(Key::ArrowLeft);
    h.run();
    assert_eq!(h.state().anonymize.review.cursor, Some(c0));

    // J : valeur à décider suivante, le texte y va ; M la masque partout.
    h.key_press(Key::J);
    h.run();
    let second = queue[1];
    assert_eq!(h.state().anonymize.review.current, Some(second));
    assert_eq!(h.state().anonymize.cursor_candidate(), Some(second));
    assert!(has_focus(&h.ctx, &doc), "le focus reste dans le texte");
    h.key_press(Key::M);
    h.run();
    assert!(is_masked(&h, second));
    assert!(!h.state().anonymize.is_undecided(second));
    let view = h.state().anonymize.document_view().unwrap();
    assert!(view.occurrences(second).all(|(_, s)| s.masked));
    // G sur la valeur sous le curseur : gardée partout.
    let under = h.state().anonymize.cursor_candidate().unwrap();
    let was = is_masked(&h, under);
    h.key_press(Key::G);
    h.run();
    assert!(!is_masked(&h, under));
    assert!(!h.state().anonymize.is_undecided(under));
    // Ctrl+Z, puis K.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(is_masked(&h, under), was, "annulé");
    // Espace bascule la valeur sous le curseur.
    let under = h.state().anonymize.cursor_candidate().unwrap();
    let before = is_masked(&h, under);
    h.key_press(Key::Space);
    h.run();
    assert_ne!(is_masked(&h, under), before);
    h.key_press(Key::K);
    h.run();
    assert!(h.state().anonymize.review.current.is_some());
    // Maj+→ : sélection au clavier, à partir de la valeur sous le curseur.
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    h.run();
    assert!(h.state().anonymize.selected_text().is_some());
    h.get_by_label("Masquer partout");
}

#[test]
fn clic_sur_un_surlignage_et_infobulle() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    // Première occurrence d'une valeur sûre, masquée d'office.
    let (o, id) = {
        let st = &h.state().anonymize;
        let view = st.document_view().unwrap();
        view.blocks
            .iter()
            .enumerate()
            .flat_map(|(bi, b)| b.spans.iter().map(move |s| (bi, *s)))
            .find(|(_, s)| s.masked && !st.is_undecided(s.candidate))
            .map(|(bi, s)| {
                (
                    Occurrence {
                        block: bi,
                        start: s.start,
                    },
                    s.candidate,
                )
            })
            .unwrap()
    };
    let label = span_label(&h, o);
    assert!(label.contains(", masqué, "), "{label}");
    let tag = h.state().anonymize.tag_preview[&id].to_string();
    // Survol : la balise, le type, le nombre d'occurrences.
    h.get_all_by_label(&label).next().unwrap().hover();
    h.run_steps(10);
    let tip = h.get_by_label_contains("Clic : garder partout");
    let node = tip.accesskit_node();
    let text = [node.label(), node.value()]
        .into_iter()
        .flatten()
        .map(|t| t.to_string())
        .find(|t| t.contains("Clic : garder partout"))
        .unwrap_or_default();
    assert!(text.starts_with(&tag), "{text}");
    assert!(text.contains(" fois"), "{text}");
    // Clic : gardée dans tout le dossier.
    h.get_all_by_label(&label).next().unwrap().click();
    h.run();
    assert!(!is_masked(&h, id));
    let view = h.state().anonymize.document_view().unwrap();
    assert!(view.occurrences(id).all(|(_, s)| !s.masked));
    let kept = label.replace(", masqué, ", ", gardé visible, ");
    assert!(h.get_all_by_label(&kept).count() >= 1);
    // Annuler (Ctrl+Z) : masquée de nouveau.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(is_masked(&h, id));
    assert_eq!(h.state().toasts.texts(), ["Annulé : valeur gardée."]);
}

#[test]
fn selection_masquee_partout_puis_annulee() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    // Recherche dans le document (/) : le passage trouvé est sélectionné.
    h.key_press(Key::Slash);
    h.run();
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(field_id(anonymiseur::ui::app::SEARCH_FIELD))
    );
    h.event(egui::Event::Text("ANTENNE".into()));
    h.run();
    assert_eq!(
        h.state().anonymize.selected_text().as_deref(),
        Some("antenne")
    );
    h.get_by_label("« antenne »");
    h.get_by_label("Masquer partout").click();
    h.run();
    let st = &h.state().anonymize;
    let c = st
        .analysis
        .as_ref()
        .unwrap()
        .candidates
        .iter()
        .find(|c| c.value == "antenne")
        .cloned()
        .unwrap();
    assert!(c.selected && c.occurrences == 1);
    assert!(!st.is_undecided(c.id));
    assert!(st.review.selection.is_none());
    assert!(st.always.is_empty());
    h.get_by_label("Annuler").click();
    h.run();
    assert!(h.state().anonymize.candidate(c.id).is_none(), "annulé");

    // À la souris : glisser sur un mot le sélectionne ; « Toujours masquer ».
    // Une occurrence affichée (le texte a défilé jusqu'à « antenne »).
    let all: Vec<Occurrence> = {
        let view = h.state().anonymize.document_view().unwrap();
        view.blocks
            .iter()
            .enumerate()
            .flat_map(|(bi, b)| {
                b.spans.iter().map(move |s| Occurrence {
                    block: bi,
                    start: s.start,
                })
            })
            .collect()
    };
    let shown = h.state().anonymize.review.visible_hint;
    let o = all
        .into_iter()
        .filter(|o| o.block > shown)
        .find(|o| h.query_all_by_label(&span_label(&h, *o)).next().is_some())
        .unwrap();
    let r = h
        .get_all_by_label(&span_label(&h, o))
        .next()
        .unwrap()
        .rect();
    let (a, b) = (
        r.left_center() + egui::vec2(1.0, 0.0),
        r.right_center() - egui::vec2(1.0, 0.0),
    );
    h.hover_at(a);
    h.run();
    h.drag_at(a);
    h.run();
    h.hover_at(b);
    h.run();
    h.drop_at(b);
    h.run();
    let text = h.state().anonymize.selected_text();
    assert!(text.as_deref().is_some_and(|t| !t.is_empty()), "{text:?}");
    h.get_by_label("Toujours masquer").click();
    h.run();
    assert_eq!(h.state().anonymize.always.len(), 1);
    h.get_by_label("Annuler").click();
    h.run();
    assert!(h.state().anonymize.always.is_empty());
}

#[test]
fn bascule_vers_la_vue_tableau_et_retour() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
    h.run();
    assert!(h.query_by_label("Trier par Valeur").is_none());
    h.get_by_label("Vue tableau").click();
    h.run();
    assert_eq!(h.state().anonymize.review.mode, VerifyMode::Table);
    h.get_by_label("Trier par Valeur");
    // Une décision prise dans le tableau sort la valeur de la file.
    let id = h.state().anonymize.queue()[0];
    let value = h.state().anonymize.candidate(id).unwrap().value.clone();
    h.state_mut().anonymize.search = value.clone();
    h.run();
    h.get_by_label(&format!("Sélectionner {value}")).click();
    h.run();
    assert!(!h.state().anonymize.is_undecided(id));
    h.get_by_label("Vue document").click();
    h.run();
    assert_eq!(h.state().anonymize.review.mode, VerifyMode::Document);
    h.get_by_label("Rechercher dans le document");
}

#[test]
fn documents_et_file_en_fenetre_etroite() {
    let _rendu = rendu();
    // 900 × 650 : deux colonnes, la file en panneau repliable ; 800 : une
    // colonne, le document se choisit en haut.
    let mut h = app(900.0, 650.0, analyze);
    h.run();
    let doc = h.get_by_label_contains("contrat-prestation.docx, ").rect();
    let paper = h.get_by_label("Rechercher dans le document").rect();
    assert!(doc.max.x < paper.min.x, "documents à gauche du texte");
    h.get_by_label("Déplier le panneau « À décider »").click();
    h.run();
    assert!(h.state().anonymize.review.queue_open);
    h.get_by_label("Replier le panneau « À décider »");
    let mut h = app(800.0, 650.0, analyze);
    h.run();
    h.get_by_label_contains("Document : ");
    assert!(
        h.query_by_label_contains("contrat-prestation.docx, ")
            .is_none()
    );
    h.get_by_label("Déplier le panneau « À décider »");
    h.get_by_label("Rechercher dans le document");
}

// --- Bilan, envoi, réponse (lot 8) -------------------------------------------

use anon_core::{Handler, Verdict};
use anonymiseur::ui::state::bilan::says_anonymous;
use anonymiseur::ui::state::history::Action as HistoryAction;

/// Verdict de l'évaluation à jour, s'il y en a une.
fn verdict(h: &Harness<'_, AnonApp>) -> Option<Verdict> {
    let st = h.state();
    st.anonymize
        .assessment(&st.assess_settings())
        .map(|a| a.assessment.verdict)
}

#[test]
fn ligne_de_bilan_dans_la_barre_de_verification() {
    let _rendu = rendu();
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &BILAN));
    h.run();
    assert_eq!(h.state().workspace.step, Step::Verify);
    let Some(Verdict::Reidentifiable { subjects }) = verdict(&h) else {
        panic!("{:?}", verdict(&h));
    };
    let line = if subjects == 1 {
        "Bilan : 1 personne reste reconnaissable".to_owned()
    } else {
        format!("Bilan : {subjects} personnes restent reconnaissables")
    };
    h.get_by_label(&line);
    // Le résumé de la vérification reste affiché à côté.
    h.get_by_label(&h.state().anonymize.review_summary());
    h.get_by_label("Voir le bilan").click();
    h.run();
    assert_eq!(h.state().workspace.step, Step::Bilan);
}

#[test]
fn bilan_appliquer_annuler_tout_corriger() {
    let _rendu = rendu();
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &BILAN);
        app.workspace.enter(Step::Bilan);
    });
    h.run();
    let before = verdict(&h).unwrap();
    assert!(!matches!(before, Verdict::Anonymous), "{before:?}");
    // Trois critères, une ligne chacun.
    for c in ["Isolement", "Recoupement", "Déduction"] {
        h.get_by_label(c);
    }
    // Appliquer la correction conseillée du premier point.
    h.get_all_by_label_contains("Appliquer : ")
        .next()
        .unwrap()
        .click();
    h.run();
    assert_eq!(
        h.state().anonymize.history.next_undo(),
        Some(HistoryAction::Fix)
    );
    assert!(
        h.state()
            .toasts
            .texts()
            .iter()
            .any(|t| t.starts_with("Correction appliquée")),
        "{:?}",
        h.state().toasts.texts()
    );
    assert!(verdict(&h).is_some(), "évaluation refaite aussitôt");
    // Ctrl+Z : la correction est défaite par l'annulation du moteur.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(
        h.state()
            .toasts
            .texts()
            .contains(&"Annulé : correction appliquée.")
    );
    assert_eq!(verdict(&h), Some(before));
    assert!(!h.state().anonymize.can_undo());
    // « Tout corriger » : plus personne de reconnaissable ; une annulation.
    h.get_by_label("Tout corriger").click();
    h.run();
    assert_eq!(verdict(&h), Some(Verdict::Anonymous));
    assert!(
        h.get_all_by_label_contains("Aucune personne reconnaissable pour ce destinataire")
            .count()
            >= 1
    );
    // Changer de destinataire relance l'évaluation, pour lui.
    h.get_by_role_and_label(Role::ComboBox, "Destinataire")
        .click();
    h.run();
    h.get_by_label("Publication").click();
    h.run();
    let st = h.state();
    assert_eq!(st.workspace.recipient, anon_core::Recipient::Publication);
    let a = st.anonymize.assessment(&st.assess_settings()).unwrap();
    assert_eq!(
        a.assessment.settings.recipient,
        anon_core::Recipient::Publication
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(verdict(&h), Some(before), "une seule annulation");
}

#[test]
fn questions_de_contexte_reportees_dans_la_fiche() {
    let _rendu = rendu();
    let save = out_dir("fiche");
    let platform = NullPlatform {
        save_to: Some(save.clone()),
        ..NullPlatform::default()
    };
    let mut h = app_with(1280.0, 2400.0, platform, |app| {
        analyze_these(app, &["notes.txt"]);
        app.workspace.enter(Step::Bilan);
    });
    h.run();
    // « Le document sera-t-il publié ? » : Non.
    let no = h.get_all_by_label("Non").nth(1).unwrap();
    no.click();
    h.run();
    assert_eq!(h.state().anonymize.bilan.answers.published, Some(false));
    h.get_by_label("Justification de vos réponses");
    // Fiche, depuis l'étape Envoyer : Markdown, à l'endroit choisi.
    h.state_mut().anonymize.bilan.fiche.legal_basis = "Intérêt légitime".into();
    h.state_mut().workspace.enter(Step::Send);
    h.run();
    h.get_by_label("Enregistrer la fiche").click();
    h.run();
    let fiche = std::fs::read_dir(&save)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "md"))
        .expect("fiche enregistrée");
    let md = std::fs::read_to_string(&fiche).unwrap();
    assert!(md.contains("Approche contextuelle"), "{md}");
    assert!(md.contains("Le document sera-t-il publié ? non"));
    assert!(md.contains("Base légale : Intérêt légitime"));
    assert!(file_name(&fiche).starts_with("fiche-evaluation-"));
    assert!(!says_anonymous(&file_name(&fiche)));
}

#[test]
fn trousseau_retenir_rouvrir_sans_mot_de_passe_detruire() {
    let _rendu = rendu();
    let dir = out_dir("trousseau");
    let keys = dir.join("Clés");
    let platform = NullPlatform {
        keys: Some(keys.clone()),
        ..NullPlatform::default()
    };
    let mut h = app_with(1280.0, 2400.0, platform, |app| {
        analyze_these(app, &["notes.txt"]);
        app.anonymize.out_dir = Some(dir.join("Anonymisés"));
    });
    h.run();
    // Envoyer : phrase proposée, « Retenir le mot de passe sur cet ordinateur ».
    step_tab(&h, "Envoyer").click();
    h.run();
    assert!(h.state().anonymize.protect.uses_generated());
    h.get_by_label("Retenir le mot de passe sur cet ordinateur")
        .click();
    h.run();
    h.get_by_label("Masquer et enregistrer").click();
    h.run();
    let report = h.state().anonymize.report.clone().unwrap();
    assert_eq!(report.remembered, Some(Ok(())));
    let map = report.map_path.clone().unwrap();
    let id = report.key_id.clone().unwrap();
    assert!(h.state().keychain().unwrap().recall(&id).unwrap().is_some());

    // Nouvelle session : la clé en mémoire est oubliée ; déposée, elle se
    // rouvre sans mot de passe (trousseau simulé).
    h.get_by_label("Nouvelle session").click();
    h.run();
    assert!(!h.state().deanonymize.has_key());
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(map.clone())));
    h.run();
    assert_eq!(h.state().workspace.step, Step::Response);
    assert!(
        h.state().deanonymize.is_unlocked(),
        "ouverte sans mot de passe"
    );
    assert!(h.query_by_label("Mot de passe de la clé").is_none());

    // Détruire : confirmation, fichier supprimé, mot de passe oublié.
    h.get_by_label("Détruire la clé").click();
    h.run();
    assert!(map.exists(), "rien n'est détruit avant la confirmation");
    h.get_by_label("Confirmer la destruction de la clé").click();
    h.run();
    assert!(!map.exists());
    assert!(h.state().keychain().unwrap().recall(&id).unwrap().is_none());
    assert!(h.state().deanonymize.destroyed);
    h.get_by_label_contains("Vos documents d'origine restent des données personnelles");
}

#[test]
fn mot_de_passe_saisi_et_retenu_a_l_ouverture() {
    let _rendu = rendu();
    // Une clé sans mot de passe retenu : il est demandé, avec « Retenir sur
    // cet ordinateur » ; la fois suivante, plus rien n'est demandé.
    let dir = out_dir("retenir-a-l-ouverture");
    let mut h = app(1280.0, 1600.0, |app| anonymize_all(app, dir.clone()));
    h.run();
    let map = h
        .state()
        .anonymize
        .report
        .clone()
        .unwrap()
        .map_path
        .unwrap();
    h.get_by_label("Nouvelle session").click();
    h.run();
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(map.clone())));
    h.run();
    assert!(!h.state().deanonymize.is_unlocked());
    tab_to_field(
        &mut h,
        field_id(anonymiseur::ui::screens::response::PASSWORD_FIELD),
        4,
    );
    h.event(egui::Event::Text(PASSWORD.into()));
    h.run();
    h.get_by_label("Retenir sur cet ordinateur").click();
    h.run();
    h.get_by_label("Ouvrir la clé").click();
    h.run();
    assert!(h.state().deanonymize.is_unlocked());
    assert!(
        h.state()
            .toasts
            .texts()
            .contains(&"Clé ouverte. Mot de passe retenu sur cet ordinateur.")
    );
    h.get_by_label("Nouvelle session").click();
    h.run();
    h.input_mut().dropped_files.push(Arc::new(Dropped(map)));
    h.run();
    assert!(
        h.state().deanonymize.is_unlocked(),
        "retenu : ouverte d'emblée"
    );
}

#[test]
fn reponse_exportee_en_md_et_docx() {
    let _rendu = rendu();
    let save = out_dir("export-reponse");
    let platform = NullPlatform {
        save_to: Some(save.clone()),
        ..NullPlatform::default()
    };
    let mut h = app_with(1280.0, 1600.0, platform, |app| {
        anonymize_all(app, out_dir("export-reponse-envoi"));
        app.workspace.enter(Step::Response);
        let engine = app.engine.clone().unwrap();
        app.deanonymize.pasted = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
        app.deanonymize
            .restore_text_marked(|t, s| engine.restore_text_marked(t, s))
            .unwrap();
    });
    h.run();
    assert!(!h.state().deanonymize.restored_marks.is_empty());
    h.get_by_label("Enregistrer en .md").click();
    h.run();
    h.get_by_label("Enregistrer en .docx").click();
    h.run();
    let (text, _) = h.state().deanonymize.text_result.clone().unwrap();
    let md = std::fs::read_to_string(save.join("reponse-restauree.md")).unwrap();
    assert_eq!(md, text);
    let docx = std::fs::read(save.join("reponse-restauree.docx")).unwrap();
    let blocks = anon_office::DocxHandler.extract(&docx).unwrap().blocks;
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap();
    assert!(blocks.iter().any(|b| b.text == first.trim_end()), "{first}");
}

/// Écrans où l'évaluation ne conclut pas à l'anonymat (invariant 9).
fn not_anonymous_screens() -> Vec<(&'static str, Setup)> {
    let mut screens = every_screen();
    screens.push((
        "bilan, tableau",
        Box::new(|app: &mut AnonApp| {
            analyze_these(app, &BILAN);
            app.anonymize.bilan.guesses_open = true;
            app.workspace.enter(Step::Bilan);
        }),
    ));
    screens.push((
        "envoyer sans clé",
        Box::new(|app: &mut AnonApp| {
            analyze_these(app, &BILAN);
            app.workspace.goal = Goal::Anonymize;
            app.anonymize.apply_goal(Goal::Anonymize);
            app.workspace.enter(Step::Send);
        }),
    ));
    screens.push((
        "envoyé sans clé",
        Box::new(|app: &mut AnonApp| {
            analyze_these(app, &BILAN);
            app.workspace.goal = Goal::Anonymize;
            app.anonymize.apply_goal(Goal::Anonymize);
            assess_now(app);
            app.anonymize.protect.confirmed = true;
            app.anonymize.out_dir = Some(out_dir("invariant-sans-cle"));
            let mut job = app.anonymize.begin_anonymize().unwrap();
            job.plan.settings = app.assess_settings();
            let engine = app.engine.clone().unwrap();
            let report = anonymize_batch(
                &engine,
                &job.inputs,
                &job.analysis,
                &job.never,
                &job.masks,
                &mut app.session,
                &job.plan,
                None,
                &NoProgress,
            );
            app.anonymize
                .anonymize_done(report, job.inputs, job.analysis);
            app.workspace.enter(Step::Send);
        }),
    ));
    screens.push((
        "réponse, clé expirée",
        Box::new(|app: &mut AnonApp| {
            anonymize_these(app, out_dir("invariant-expiree"), &BILAN);
            app.workspace.enter(Step::Response);
            if let MapStatus::Unlocked { session, .. } = &mut app.deanonymize.map {
                session.keep_until = Some("2020-01-01".into());
            }
            app.deanonymize.destroy_confirm = true;
        }),
    ));
    screens
}

#[test]
fn anonyme_reserve_a_une_evaluation_qui_conclut() {
    let _rendu = rendu();
    // Invariant 9 : le mot « anonyme » (ou « anonymes », mot entier, toutes
    // casses) n'apparaît sur aucun écran tant que l'évaluation ne conclut pas
    // à l'anonymat. « Anonymiser définitivement », « anonymiser » et
    // « Anonymiseur » sont d'autres mots. Le contenu des documents est exclu.
    let mut checked = Vec::new();
    for (screen, setup) in not_anonymous_screens() {
        let mut h = app(1280.0, 2400.0, setup);
        h.run();
        let st = h.state();
        let anonymous = st
            .anonymize
            .assessment(&st.assess_settings())
            .is_some_and(|a| a.assessment.is_anonymous());
        if anonymous {
            // Le mot y est permis : l'évaluation conclut à l'anonymat.
            continue;
        }
        checked.push(screen);
        let mut content: Vec<String> = vec![st.deanonymize.pasted.to_lowercase()];
        if let Some((t, _)) = &st.deanonymize.text_result {
            content.push(t.to_lowercase());
        }
        if let Some(a) = &st.anonymize.analysis {
            for c in &a.candidates {
                content.push(c.value.to_lowercase());
                content.push(c.context.to_lowercase());
            }
        }
        content.retain(|c| c.chars().count() >= 4);
        for text in all_texts(&h) {
            let lower = text.to_lowercase();
            if content.iter().any(|c| lower.contains(c.as_str())) {
                continue;
            }
            assert!(!says_anonymous(&text), "{screen} : « {text} »");
        }
    }
    for screen in [
        "bilan, tableau",
        "envoyer sans clé",
        "envoyé sans clé",
        "réponse, clé expirée",
    ] {
        assert!(checked.contains(&screen), "{screen} : écran non vérifié");
    }
    // Contre-épreuve : après « Tout corriger », l'objectif « Anonymiser
    // définitivement » annonce le suffixe « _anonyme ».
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &BILAN);
        app.workspace.goal = Goal::Anonymize;
        app.anonymize.apply_goal(Goal::Anonymize);
        fix_all(app);
        app.workspace.enter(Step::Bilan);
    });
    h.run();
    assert_eq!(verdict(&h), Some(Verdict::Anonymous));
    assert!(all_texts(&h).iter().any(|t| says_anonymous(t)));
}

#[test]
fn controles_alignes_bilan_envoi_reponse() {
    let _rendu = rendu();
    use anonymiseur::ui::theme::CONTROL_SM;
    // Bilan : correction conseillée, « Appliquer » et « Autre » sur une ligne.
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &BILAN);
        app.workspace.enter(Step::Bilan);
    });
    h.run();
    let apply = h
        .get_all_by_label_contains("Appliquer : ")
        .next()
        .unwrap()
        .rect();
    assert_eq!(apply.height(), CONTROL_SM);
    if let Some(other) = h
        .query_all_by_label_contains("Autres corrections, point 1")
        .next()
    {
        assert_aligned(
            "Bilan, correction",
            &[
                ("Appliquer".to_owned(), apply),
                ("Autre".to_owned(), other.rect()),
            ],
        );
    }
    assert_eq!(h.get_by_label("Tout corriger").rect().height(), CONTROL_SM);
    assert_aligned(
        "Bilan, barre d'action",
        &rects(&h, &["Retour", "Continuer"]),
    );
    // Envoyer : phrase de passe, « Copier », « Générer une autre ».
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, &["notes.txt"]);
        ready_to_send(app);
        app.workspace.enter(Step::Send);
    });
    h.run();
    let row = rects(&h, &["Copier", "Générer une autre"]);
    assert_eq!(row[0].1.height(), CONTROL_SM);
    assert_aligned("Envoyer, phrase de passe", &row);
    assert_eq!(
        h.get_by_label("Enregistrer la fiche").rect().height(),
        CONTROL_SM
    );
    // Réponse : « Copier », « .md », « .docx » sur la ligne du titre.
    let mut h = app(1280.0, 800.0, |app| {
        anonymize_all(app, out_dir("alignement-reponse"));
        app.workspace.enter(Step::Response);
    });
    h.run();
    let row = rects(
        &h,
        &["Copier", "Enregistrer en .md", "Enregistrer en .docx"],
    );
    assert_eq!(row[0].1.height(), CONTROL_SM);
    assert_aligned("Réponse, exports", &row);
}

#[test]
fn titres_de_la_reponse_sans_chevauchement() {
    let _rendu = rendu();
    use anonymiseur::ui::theme::CONTROL_SM;
    // Titres des deux colonnes et leurs boutons : sur une ligne, sans se
    // chevaucher, en 1280 × 800 comme dans la fenêtre la plus étroite. En
    // 900 × 650, « .md » et « .docx » se réduisent à leur icône (nom
    // accessible et infobulle gardés).
    for (w, hh) in [(1280.0, 800.0), (900.0, 650.0)] {
        let mut h = app(w, hh, |app| {
            anonymize_all(app, out_dir("titres-reponse"));
            app.workspace.enter(Step::Response);
        });
        h.run();
        let screen = format!("Réponse, {w} × {hh}");
        for (title, buttons) in [
            ("Réponse de l'IA", &["Coller"][..]),
            (
                "Réponse restaurée",
                &["Copier", "Enregistrer en .md", "Enregistrer en .docx"][..],
            ),
        ] {
            let t = h.get_by_label(title).rect();
            let row = rects(&h, buttons);
            for (name, r) in &row {
                assert!(
                    !t.intersects(*r),
                    "{screen} : « {title} » chevauche « {name} »"
                );
                assert!(r.max.x <= w, "{screen} : « {name} » sort de la fenêtre");
                assert_centered(&screen, name, *r, title, t);
            }
            assert_aligned(&screen, &row);
            // Les boutons ne se chevauchent pas entre eux.
            for (i, (a, ra)) in row.iter().enumerate() {
                for (b, rb) in row.iter().skip(i + 1) {
                    assert!(!ra.intersects(*rb), "{screen} : « {a} » sur « {b} »");
                }
            }
        }
        let md = h.get_by_label("Enregistrer en .md").rect();
        assert_eq!(md.height(), CONTROL_SM);
        if w < 1000.0 {
            assert_eq!(
                md.width(),
                md.height(),
                "{screen} : « .md » réduit à l'icône"
            );
            // Survol : l'infobulle nomme l'action.
            h.get_by_label("Enregistrer en .md").hover();
            h.run_steps(10);
            assert!(
                h.query_all_by_label("Enregistrer en .md").count() >= 2,
                "{screen} : infobulle de « .md »"
            );
        } else {
            assert!(md.width() > md.height(), "{screen} : « .md » en entier");
        }
    }
}

// --- Presse-papiers express -------------------------------------------------

/// Texte copié des instantanés : des valeurs sûres et « Léo » (liste
/// « toujours masquer », valeur courte), à décider.
const COPIE: &str = "Bonjour,\n\
    Jean Dupont confirme le rendez-vous avec Léo, joignable au 06 12 34 56 78.\n\
    Écrire à claire.martin@example.com pour la suite.\n\
    Léo passera au bureau de Nautilus Pacifique SARL jeudi.";

/// Vérification du texte [`COPIE`], avec une session neuve.
fn express_review() -> ExpressReview {
    let (engine, _) = engine();
    let mut st = AnonymizeState::default();
    st.always = vec![("Léo".into(), EntityKind::Personne)];
    let prepared = express::prepare(
        &engine,
        MaskRequest {
            text: COPIE.into(),
            base: Session::new(),
            options: st.analyze_options(),
            known: KnownDecisions::default(),
        },
    );
    ExpressReview::new(prepared)
}

/// Fenêtre du presse-papiers rendue seule, à sa taille (plus la marge de 8 px
/// du banc d'essai), au thème choisi.
fn express_window<S: 'static>(
    size: [f32; 2],
    choice: ThemeChoice,
    state: S,
    mut add: impl FnMut(&mut egui::Ui, &mut S) + 'static,
) -> Harness<'static, (bool, S)> {
    let mut h = Harness::builder()
        .with_size([size[0] + 16.0, size[1] + 16.0])
        .with_pixels_per_point(1.0)
        .build_ui_state(
            move |ui, (ready, st): &mut (bool, S)| {
                if !*ready {
                    theme::install(ui.ctx(), choice);
                    *ready = true;
                    ui.ctx().request_repaint();
                    return;
                }
                theme::sync(ui);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(size[0], size[1]), egui::Sense::hover());
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| add(ui, st));
            },
            (false, state),
        );
    h.run();
    h
}

fn notice_window(
    choice: ThemeChoice,
    kind: ToastKind,
    text: &str,
) -> Harness<'static, (bool, Notice)> {
    let notice = Notice {
        kind,
        text: text.into(),
        since: 0.0,
    };
    express_window(express_screen::NOTICE_SIZE, choice, notice, |ui, n| {
        express_screen::notification(ui, n);
    })
}

fn review_window(choice: ThemeChoice) -> Harness<'static, (bool, ExpressReview)> {
    express_window(
        express_screen::REVIEW_SIZE,
        choice,
        express_review(),
        |ui, r| {
            let _ = express_screen::review(ui, r);
        },
    )
}

#[test]
fn presse_papiers_notification_et_verification() {
    let _rendu = rendu();
    let mut results = SnapshotResults::new();
    for (choice, suffix) in [(ThemeChoice::Light, ""), (ThemeChoice::Dark, "_sombre")] {
        let mut h = notice_window(choice, ToastKind::Success, &express::masked_notice(12));
        shot(&mut h, format!("presse_papiers_notification{suffix}"));
        results.extend_harness(&mut h);
        let mut h = notice_window(
            choice,
            ToastKind::Warning,
            &express::restored_notice(9, &["[PERSONNE_9]".to_owned()]),
        );
        shot(
            &mut h,
            format!("presse_papiers_notification_inconnue{suffix}"),
        );
        results.extend_harness(&mut h);
        let mut h = review_window(choice);
        shot(&mut h, format!("presse_papiers_verification{suffix}"));
        results.extend_harness(&mut h);
    }
    results.unwrap();
}

#[test]
fn presse_papiers_hauteur_des_controles_et_contraste() {
    let _rendu = rendu();
    use anonymiseur::ui::theme::{AA_LARGE, AA_TEXT, CONTROL_SM, contrast, palette, set_dark};
    let h = review_window(ThemeChoice::Light);
    for b in [
        "Masquer",
        "Garder",
        "Suivante",
        "Précédente",
        "Annuler",
        express_screen::REPLACE,
    ] {
        assert_eq!(h.get_by_label(b).rect().height(), CONTROL_SM, "{b}");
    }
    let row: Vec<(String, egui::Rect)> = ["Annuler", express_screen::REPLACE]
        .iter()
        .map(|l| ((*l).to_owned(), h.get_by_label(l).rect()))
        .collect();
    assert_aligned("Vérification du presse-papiers", &row);
    // Texte et surlignages lisibles en clair et en sombre.
    for dark in [false, true] {
        set_dark(dark);
        let p = palette();
        for (name, fg, bg) in [
            ("texte de la notification", p.text, p.surface),
            ("en-tête de la notification", p.text_secondary, p.surface),
            ("texte copié", p.text, p.surface),
            ("résumé de la vérification", p.text_secondary, p.page),
            ("barre d'actions", p.text_secondary, p.surface),
        ] {
            assert!(contrast(fg, bg) >= AA_TEXT, "{name} (sombre : {dark})");
        }
        for mark in [Mark::Masked, Mark::Pending, Mark::Current] {
            let (fill, ink, line) = mark_style(mark);
            assert!(contrast(ink, fill) >= AA_TEXT, "{mark:?} (sombre : {dark})");
            assert!(line.width > 0.0);
        }
        for kind in [
            ToastKind::Success,
            ToastKind::Info,
            ToastKind::Warning,
            ToastKind::Error,
        ] {
            let (_, ink, _) = toast::style(kind);
            assert!(
                contrast(ink, p.surface) >= AA_LARGE,
                "{kind:?} (sombre : {dark})"
            );
        }
    }
    set_dark(false);
}

/// Application dont le presse-papiers et les raccourcis sont simulés.
fn express_app(
    width: f32,
    height: f32,
    setup: impl FnOnce(&mut AnonApp) + 'static,
) -> (Harness<'static, AnonApp>, MemoryClipboard, ScriptedHotkeys) {
    let clip = MemoryClipboard::default();
    let keys = ScriptedHotkeys::default();
    let (c, k) = (clip.clone(), keys.clone());
    let h = app(width, height, move |app| {
        setup(app);
        app.set_express_system(Box::new(c), Box::new(k));
    });
    (h, clip, keys)
}

/// Aucun toast ni notification ne contient de texte copié.
fn assert_no_content(h: &Harness<'_, AnonApp>, words: &[&str]) {
    let app = h.state();
    let notice = app.express.notice.as_ref().map(|n| n.text.clone());
    for t in app
        .toasts
        .texts()
        .into_iter()
        .map(str::to_owned)
        .chain(notice)
    {
        for w in words {
            assert!(!t.contains(w), "« {w} » dans « {t} »");
        }
    }
}

#[test]
fn presse_papiers_express_au_clavier() {
    let _rendu = rendu();
    let (mut h, clip, keys) = express_app(1280.0, 800.0, |app| {
        app.anonymize.always = vec![("Léo".into(), EntityKind::Personne)];
    });
    h.run();
    assert!(keys.is_enabled(), "raccourcis enregistrés au démarrage");
    let text = "Jean Dupont confirme le rendez-vous avec Léo.\nÉcrire à claire.martin@example.com.";
    let words = ["Dupont", "claire", "example.com", "Léo", "rendez-vous"];
    clip.set_text(text);
    keys.press(Action::Mask);
    h.run();
    // Une valeur à décider : vérification ouverte, presse-papiers inchangé.
    assert!(h.state().express.review.is_some());
    assert_eq!(clip.text().as_deref(), Some(text));
    assert!(clip.writes().is_empty());
    h.get_by_label(express_screen::REPLACE);
    // Ctrl+Alt+M de nouveau : la même vérification revient au premier plan.
    keys.press(Action::Mask);
    h.run();
    assert!(h.state().express.with_review(|r| r.current.is_some()) == Some(true));
    // J, K parcourent la file ; G garde chaque valeur à décider.
    let first = h.state().express.with_review(|r| r.current).flatten();
    h.key_press(egui::Key::J);
    h.run();
    h.key_press(egui::Key::K);
    h.run();
    let back = h.state().express.with_review(|r| r.current).flatten();
    let len = h
        .state()
        .express
        .with_review(|r| r.queue().len())
        .unwrap_or(0);
    if len == 1 {
        assert_eq!(back, first);
    }
    for _ in 0..len {
        h.key_press(egui::Key::G);
        h.run();
    }
    assert_eq!(
        h.state()
            .express
            .with_review(|r| r.prepared.pending_count()),
        Some(0)
    );
    // Entrée : le presse-papiers est remplacé par le texte masqué.
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().express.review.is_none());
    let masked = clip.text().unwrap();
    assert!(masked.contains("Léo"), "gardée visible");
    assert!(!masked.contains("Jean Dupont") && !masked.contains("claire.martin@example.com"));
    assert_eq!(clip.writes().last().map(|w| w.1), Some(false));
    let notice = h.state().express.notice.clone().unwrap();
    assert!(
        notice.text.ends_with("masquées, presse-papiers prêt"),
        "{}",
        notice.text
    );
    assert_no_content(&h, &words);
    // Sans dossier : clé « Presse-papiers » ouverte et document du dossier.
    {
        let app = h.state();
        assert_eq!(app.deanonymize.key_name(), Some(express::KEY_NAME));
        assert!(
            app.anonymize
                .document_names()
                .contains(&express::DOCUMENT_NAME.to_owned())
        );
        assert_eq!(app.workspace.screen, Screen::Folder);
        assert!(app.anonymize.analysis.is_some(), "document analysé");
    }
    // Restaurer : la réponse de l'IA copiée, avec une balise inventée.
    let answer = format!("{masked}\n[PERSONNE_99] est inventée.");
    clip.set_text(&answer);
    keys.press(Action::Restore);
    h.run();
    let restored = clip.text().unwrap();
    assert!(restored.starts_with(text), "texte d'origine rendu");
    assert!(
        restored.ends_with("[PERSONNE_99] est inventée."),
        "balise inconnue laissée"
    );
    assert_eq!(
        clip.writes().last().map(|w| w.1),
        Some(true),
        "vraies valeurs : sensibles"
    );
    let notice = h.state().express.notice.clone().unwrap();
    assert_eq!(notice.kind, ToastKind::Warning);
    assert!(
        notice.text.ends_with("· 1 inconnue : [PERSONNE_99]"),
        "{}",
        notice.text
    );
    assert_no_content(&h, &words);
    // Échap : vérification annulée, presse-papiers inchangé.
    h.state_mut().workspace.enter(Step::Response);
    clip.set_text("Léo rappellera.");
    keys.press(Action::Mask);
    h.run();
    assert!(h.state().express.review.is_some());
    let writes = clip.writes().len();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().express.review.is_none());
    assert_eq!(clip.writes().len(), writes);
    assert_eq!(clip.text().as_deref(), Some("Léo rappellera."));
    assert_eq!(
        h.state().express.notice.as_ref().map(|n| n.text.as_str()),
        Some(express::CANCELLED)
    );
}

#[test]
fn presse_papiers_avec_la_cle_du_dossier() {
    let _rendu = rendu();
    let (mut h, clip, keys) = express_app(1280.0, 800.0, |app| {
        anonymize_these(app, out_dir("express-dossier"), &["notes.txt"]);
        app.workspace.enter(Step::Response);
    });
    h.run();
    let key = h.state().deanonymize.session().cloned().unwrap();
    let dupont = key
        .entities
        .iter()
        .find(|e| e.value == "Jean Dupont" || e.variants.iter().any(|v| v == "Jean Dupont"))
        .map(|e| e.tag.to_string())
        .unwrap();
    clip.set_text("Merci à Jean Dupont pour son retour.");
    keys.press(Action::Mask);
    h.run();
    assert!(h.state().express.review.is_none(), "rien à décider");
    assert_eq!(
        clip.text().as_deref(),
        Some(format!("Merci à {dupont} pour son retour.").as_str()),
        "même balise que dans les fichiers du dossier"
    );
    assert_eq!(
        h.state().express.notice.as_ref().map(|n| n.text.clone()),
        Some(express::masked_notice(1))
    );
    // La clé reste celle du dossier (aucune valeur nouvelle), sous son nom.
    let after = h.state().deanonymize.session().cloned().unwrap();
    assert_eq!(after.entities, key.entities);
    assert_eq!(h.state().deanonymize.key_name(), Some(KEY_NAME));
    // Restauration avec la clé du dossier.
    clip.set_text(&format!("{dupont} a validé."));
    keys.press(Action::Restore);
    h.run();
    assert!(clip.text().unwrap().ends_with(" a validé."));
    assert!(!clip.text().unwrap().contains(&dupont));
    assert_eq!(
        h.state().express.notice.as_ref().map(|n| n.text.clone()),
        Some(express::restored_notice(1, &[]))
    );
}

#[test]
fn presse_papiers_vide_raccourci_pris_et_preference() {
    let _rendu = rendu();
    // Restaurer est déjà pris par une autre application.
    let (mut h, clip, keys) = express_app(1280.0, 800.0, |_| {});
    keys.take(Action::Restore);
    h.state_mut()
        .set_express_system(Box::new(clip.clone()), Box::new(keys.clone()));
    h.run();
    let taken = format!(
        "Raccourci {} déjà utilisé par une autre application",
        hotkeys::RESTORE.label()
    );
    let count = |h: &Harness<'_, AnonApp>| {
        h.state()
            .toasts
            .texts()
            .iter()
            .filter(|t| t.starts_with(&taken))
            .count()
    };
    assert_eq!(count(&h), 1, "signalé");
    // Une seconde fois (enregistrement refait) : pas de second toast.
    h.state_mut().toasts = Default::default();
    h.state_mut()
        .set_express_system(Box::new(clip.clone()), Box::new(keys.clone()));
    h.run();
    assert_eq!(count(&h), 0, "signalé une seule fois");
    // Presse-papiers vide ou sans texte.
    clip.set_empty();
    keys.press(Action::Mask);
    h.run();
    assert_eq!(
        h.state().express.notice.as_ref().map(|n| n.text.as_str()),
        Some(express::EMPTY)
    );
    assert!(clip.writes().is_empty());
    // Restaurer sans clé : impossible, dit pourquoi ; l'action reste dans la palette.
    clip.set_text("[PERSONNE_1] a validé.");
    h.state_mut().express_run(Action::Restore);
    assert!(
        h.state()
            .express
            .notice
            .as_ref()
            .is_some_and(|n| n.text.starts_with("Aucune clé ouverte"))
    );
    assert!(clip.writes().is_empty());
    let entries = h.state().palette_entries();
    let restore = entries
        .iter()
        .find(|e| e.label == "Restaurer le presse-papiers")
        .unwrap();
    assert!(restore.blocker.is_some());
    // Préférence désactivée depuis la palette : retenue, raccourcis retirés.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    h.run();
    h.event(egui::Event::Text("desactiver raccourcis globaux".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(!h.state().prefs.clipboard_hotkeys);
    assert!(!keys.is_enabled());
    clip.set_text("Écrire à claire.martin@example.com.");
    keys.press(Action::Mask);
    h.run();
    assert!(
        clip.writes().is_empty(),
        "raccourci retiré : rien ne se passe"
    );
    // L'action reste dans la palette, sans raccourci affiché.
    let entries = h.state().palette_entries();
    let mask = entries
        .iter()
        .find(|e| e.label == "Masquer le presse-papiers")
        .unwrap();
    assert!(mask.shortcut.is_none() && mask.enabled());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    h.run();
    h.event(egui::Event::Text("masquer le presse-papiers".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let masked = clip.text().unwrap();
    assert!(
        masked.contains("[EMAIL_") && !masked.contains("example.com"),
        "masqué"
    );
    // Système qui refuse l'écriture : notification d'erreur, presse-papiers inchangé.
    clip.refuse_writes(true);
    clip.set_text("Écrire à paul.durand@example.com.");
    h.state_mut().express_run(Action::Mask);
    h.run();
    assert_eq!(
        h.state().express.notice.as_ref().map(|n| n.text.as_str()),
        Some(express::WRITE_FAILED)
    );
    assert_eq!(
        clip.text().as_deref(),
        Some("Écrire à paul.durand@example.com.")
    );
    assert_no_content(&h, &["claire", "paul", "durand", "example.com"]);
}
