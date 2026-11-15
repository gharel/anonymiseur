//! Instantanés PNG de la galerie, de l'accueil et de chaque étape du dossier de
//! travail, alimentés par une vraie analyse des fichiers de `exemples/`, à deux
//! tailles de fenêtre (plus une page haute pour voir le contenu sous la ligne
//! de flottaison), et en thème sombre à 1280 × 800. Images comparées sous
//! Windows, édition complète (`COMPARE_IMAGES`). Puis les interactions : parcours
//! complet au clavier, palette de commandes, annuler et rétablir, alignement,
//! noms des boutons, vocabulaire.
//! Mise à jour : `UPDATE_SNAPSHOTS=force cargo test -p anonymiseur --test snapshots`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{EntityKind, Goal, PixelRect};
use anon_engine::{Engine, NoProgress};
use anonymiseur::moteur::IaStatus;
use anonymiseur::ui::gallery::{self, GalleryState};
use anonymiseur::ui::platform::{NullPlatform, file_entry};
use anonymiseur::ui::service::{Outcome, anonymize_batch, file_name, read_documents};
use anonymiseur::ui::state::anonymize::Busy;
use anonymiseur::ui::state::deanonymize::MapStatus;
use anonymiseur::ui::state::review::{Occurrence, VerifyMode};
use anonymiseur::ui::state::workspace::{Screen, Step};
use anonymiseur::ui::theme::ThemeChoice;
use anonymiseur::ui::{AnonApp, theme};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, SnapshotResults};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

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

fn neutralize_paths(app: &mut AnonApp) {
    let shown = PathBuf::from(SHOWN_DIR);
    let renamed = |p: &PathBuf| shown.join(p.file_name().unwrap_or_default());
    if app.anonymize.out_dir.is_some() {
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
    analyze(app);
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
        &job.protection,
        &job.out_dir,
        &NoProgress,
    );
    let map = report.map_path.clone().unwrap();
    app.deanonymize
        .open_in_memory(file_name(&map), app.session.clone());
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
    gallery_in(ThemeChoice::Light, "galerie");
}

#[test]
fn gallery_dark() {
    gallery_in(ThemeChoice::Dark, "galerie_sombre");
}

// --- Écrans -----------------------------------------------------------------

#[test]
fn home() {
    snap("accueil_0", |_| {});
}

#[test]
fn home_with_refused_files() {
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
    snap("dossier_1_verifier", analyze);
}

#[test]
fn verify_table() {
    snap("dossier_1d_vue_tableau", |app| {
        analyze(app);
        app.anonymize.review.mode = VerifyMode::Table;
    });
}

#[test]
fn verify_selection() {
    snap("dossier_1e_selection", |app| {
        analyze_these(app, &["notes.txt"]);
        app.anonymize.review.queue_open = true;
        app.anonymize.sync_review(&app.session);
        app.anonymize.find_next("antenne", None);
    });
}

#[test]
fn verify_less_precise() {
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
    snap("dossier_2_bilan", |app| {
        analyze(app);
        app.workspace.enter(Step::Bilan);
    });
}

#[test]
fn send() {
    snap("dossier_3_envoyer", |app| {
        analyze(app);
        app.workspace.enter(Step::Send);
        app.anonymize.protect.password = PASSWORD.into();
        app.anonymize.protect.confirm = "Cheval-Agrafe".into();
    });
}

#[test]
fn sent() {
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
    snap("dossier_3c_image_non_restaurable", |app| {
        with_unrestorable_warning(app, "non-restaurables")
    });
}

#[test]
fn response_locked_key() {
    snap("dossier_4_reponse_cle", |app| {
        app.add_paths(vec![PathBuf::from("C:/Dossiers/table_anonymiseur.anonmap")]);
        app.deanonymize
            .unlock_done(Err(anon_vault::VaultError::WrongPassword.to_string()));
    });
}

#[test]
fn response() {
    snap("dossier_4b_reponse", |app| {
        anonymize_all(app, out_dir("reponse"));
        app.workspace.enter(Step::Response);
        let engine = app.engine.clone().unwrap();
        let d = &mut app.deanonymize;
        d.pasted = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
        d.restore_text(|t, s| engine.restore_text(t, s)).unwrap();
    });
}

#[test]
fn command_palette() {
    snap("palette", |app| {
        analyze_these(app, &["notes.txt"]);
        app.palette.open();
    });
}

#[test]
fn shortcuts_panel() {
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
    use anonymiseur::ui::widgets::field::field_id;
    let dir = examples_dir("clavier");
    let platform = NullPlatform {
        examples: Some(dir.clone()),
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
    assert_eq!(h.state().anonymize.files.len(), 14);
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

    // Bilan, puis Envoyer : le mot de passe a le focus à l'arrivée.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Bilan);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert_eq!(h.state().workspace.step, Step::Send);
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(field_id(anonymiseur::ui::screens::send::PASSWORD_FIELD))
    );
    h.event(egui::Event::Text(PASSWORD.into()));
    h.run();
    tab_to_field(&mut h, field_id("mdp-confirmation"), 4);
    h.event(egui::Event::Text(PASSWORD.into()));
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    let report = h.state().anonymize.report.clone().unwrap();
    assert!(report.fatal.is_none());
    assert!(report.map_path.is_some(), "clé écrite");
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
    // Aucun mot de passe demandé : l'action principale est possible.
    h.get_by_label("Aucune clé de restauration");
    h.get_by_label("Masquer et enregistrer").click();
    h.run();
    let report = h.state().anonymize.report.clone().unwrap();
    assert_eq!(report.delivered(), 1);
    assert!(report.map_path.is_none(), "aucune clé écrite");
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
    let mut h = app(1280.0, 2400.0, |app| {
        with_unrestorable_warning(app, "non-restaurables-rapport");
        neutralize_paths(app);
    });
    h.run();
    h.get_by_label("Images masquées non restaurables");
}

#[test]
fn bouton_exemples_sans_documents_demande_un_dossier() {
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
    assert_eq!(files.len(), 14, "documents d'exemple seulement");
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
    assert_eq!(h.state().anonymize.files.len(), 14);
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
    assert_aligned("Barre d'action", &rects(&h, &["Accueil", "Continuer"]));
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
    // Dans le document : la valeur courante de la file et ses boutons M et G,
    // au-dessus de la barre d'action, en 1280 × 800 comme en 900 × 650.
    for (w, hh) in [(1280.0, 800.0), (900.0, 650.0)] {
        let mut h = app(w, hh, analyze);
        h.run();
        let action = h.get_by_label("Continuer").rect();
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
    let action = h.get_by_label("Continuer").rect();
    assert!(
        row.max.y < action.min.y - 8.0,
        "première valeur à y = {}, barre d'action à y = {}",
        row.max.y,
        action.min.y
    );
}

#[test]
fn zones_de_texte_lisibles() {
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
    let mut h = app(1280.0, 800.0, |app| analyze_these(app, &["notes.txt"]));
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
