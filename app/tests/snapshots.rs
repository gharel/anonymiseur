//! Instantanés PNG de la galerie et de chaque écran, alimentés par une vraie
//! analyse des fichiers de `exemples/`, à deux tailles de fenêtre (plus une
//! page haute pour voir le contenu sous la ligne de flottaison), et en thème
//! sombre à 1280 × 800.
//! Mise à jour : `UPDATE_SNAPSHOTS=force cargo test -p anonymiseur --test snapshots`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{EntityKind, PixelRect, Profile, Session};
use anon_engine::{Engine, NoProgress};
use anonymiseur::moteur::IaStatus;
use anonymiseur::ui::gallery::{self, GalleryState};
use anonymiseur::ui::platform::{NullPlatform, file_entry};
use anonymiseur::ui::service::{Outcome, anonymize_batch, read_inputs};
use anonymiseur::ui::state::Tab;
use anonymiseur::ui::state::anonymize::{Busy, Step};
use anonymiseur::ui::state::deanonymize::InputMode;
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
    for (w, h, label, theme) in SIZES {
        let mut harness = app(w, h, |a| {
            a.prefs.theme = theme;
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
    analyze_these(app, profile, &FICHIERS);
}

fn analyze_these(app: &mut AnonApp, profile: Profile, names: &[&str]) {
    let engine = app.engine.clone().unwrap();
    let paths: Vec<PathBuf> = names.iter().map(|n| exemple(n)).collect();
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
        &job.masks,
        &mut app.session,
        &job.protection,
        &job.out_dir,
        &NoProgress,
    );
    app.anonymize
        .anonymize_done(report, job.inputs, job.analysis);
}

fn gallery_in(choice: ThemeChoice, name: &str) {
    let mut harness = Harness::builder()
        .with_size([1280.0, 2500.0])
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
    harness.snapshot(name);
}

#[test]
fn gallery() {
    gallery_in(ThemeChoice::Light, "galerie");
}

#[test]
fn gallery_dark() {
    gallery_in(ThemeChoice::Dark, "galerie_sombre");
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
        // Deuxième fichier en cours : un terminé, deux en attente.
        app.progress().step(1, 4, "fichier-clients.xlsx");
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
    for p in [
        exemple("notes.txt"),
        exemple("vieux.xls"),
        exemple("notes_anonymise.txt"),
        exemple("table_anonymiseur.anonmap"),
    ] {
        harness.input_mut().dropped_files.push(Arc::new(Dropped(p)));
    }
    harness.run();
    assert_eq!(harness.state().anonymize.files.len(), 1);
    assert_eq!(harness.state().anonymize.rejected.len(), 1);
    harness.get_by_label("notes.txt");
    // Les fichiers produits par l'outil sont écartés, avec une notification ;
    // la table déposée est proposée à la reprise.
    let toasts: Vec<String> = harness
        .state()
        .toasts
        .texts()
        .iter()
        .map(|t| (*t).to_owned())
        .collect();
    assert!(
        toasts
            .iter()
            .any(|t| t.starts_with("1 fichier déjà anonymisé ignoré")),
        "{toasts:?}"
    );
    assert!(toasts.iter().any(|t| t == "1 fichier ajouté"), "{toasts:?}");
    assert!(harness.state().anonymize.offered_map.is_some());
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

/// Dossier des exemples comme sous Documents : `…/Anonymiseur/Exemples`.
fn examples_dir(tag: &str) -> PathBuf {
    out_dir(tag).join("Anonymiseur").join("Exemples")
}

#[test]
fn anonymize_examples_and_always() {
    snap("anonymiser_1c_exemples", |app| {
        app.load_examples(&examples_dir("exemples"));
        assert_eq!(app.anonymize.files.len(), 14);
        // Quelques fichiers seulement, pour voir la liste « toujours » sous la liste.
        app.anonymize.files.truncate(3);
        app.anonymize.always = vec![
            ("Martinique".into(), EntityKind::Lieu),
            ("Projet Corail".into(), EntityKind::Autre),
        ];
        app.anonymize.always_kind = EntityKind::Lieu;
    });
}

#[test]
fn anonymize_generalization() {
    snap("anonymiser_2b_generalisation", |app| {
        analyze_these(app, Profile::Renforce, &["notes.txt"]);
        let session = app.session.clone();
        let st = &mut app.anonymize;
        for id in st.visible() {
            let generalize = st
                .candidate(id)
                .is_some_and(|c| matches!(c.kind, EntityKind::Age | EntityKind::Montant));
            if generalize {
                assert!(st.set_generalized(id, true, &session));
            }
        }
        st.always = vec![("Martinique".into(), EntityKind::Lieu)];
    });
}

#[test]
fn anonymize_images() {
    snap("anonymiser_2c_images", |app| {
        analyze_these(
            app,
            Profile::Standard,
            &["carte-visite.png", "courrier-scanne.pdf"],
        );
        let info = app.anonymize.images()[0].clone();
        app.anonymize.masks.add(
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
fn tracer_puis_retirer_une_zone() {
    let mut harness = app(1280.0, 2400.0, |app| {
        analyze_these(app, Profile::Standard, &["carte-visite.png"]);
    });
    harness.run();
    let canvas = harness
        .get_by_label("Aperçu de carte-visite.png : tracez un rectangle pour masquer une zone")
        .rect();
    let a = canvas.min + egui::vec2(20.0, 20.0);
    let b = canvas.min + egui::vec2(120.0, 60.0);
    harness.hover_at(a);
    harness.run();
    harness.drag_at(a);
    harness.run();
    harness.hover_at(b);
    harness.run();
    harness.drop_at(b);
    harness.run();
    let masks = harness.state().anonymize.masks.masks.clone();
    assert_eq!(masks.len(), 1, "{masks:?}");
    let r = masks[0].mask.rect;
    assert!(r.w > 0 && r.h > 0 && r.w > r.h, "{r:?}");
    harness.get_by_label("Retirer la zone 1").click();
    harness.run();
    assert!(harness.state().anonymize.masks.is_empty());
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
fn anonymize_done_unrestorable() {
    snap("anonymiser_4b_image_non_restaurable", |app| {
        with_unrestorable_warning(app, "non-restaurables")
    });
}

#[test]
fn rapport_signale_les_images_non_restaurables() {
    let mut harness = app(1280.0, 2400.0, |app| {
        with_unrestorable_warning(app, "non-restaurables-rapport");
        neutralize_paths(app);
    });
    harness.run();
    harness.get_by_label("Images masquées non restaurables");
}

#[test]
fn bouton_exemples_sans_documents_demande_un_dossier() {
    // Dossier Documents introuvable : repli sur le dialogue, annulé ici.
    let mut harness = app(1280.0, 800.0, |_| {});
    harness.run();
    harness.get_by_label("Essayer avec des exemples").click();
    harness.run();
    assert!(harness.state().anonymize.files.is_empty());
}

#[test]
fn bouton_exemples_reutilise_le_meme_dossier() {
    let dir = examples_dir("exemples-un-clic");
    let platform = NullPlatform {
        examples: Some(dir.clone()),
        ..NullPlatform::default()
    };
    let mut harness = app_with(1280.0, 800.0, platform, |_| {});
    harness.run();
    harness.get_by_label("Essayer avec des exemples").click();
    harness.run();
    let files = harness.state().anonymize.files.clone();
    assert_eq!(files.len(), 14, "documents d'exemple seulement");
    assert!(files.iter().all(|f| f.path.starts_with(&dir)));
    assert!(
        files
            .iter()
            .all(|f| f.name != "LISEZMOI.md" && f.name != "reponse-ia-exemple.md")
    );
    assert_eq!(
        harness.state().anonymize.out_dir,
        Some(dir.join("Anonymisés")),
        "sorties dans un sous-dossier"
    );
    // Un fichier produit par l'outil dans le dossier, un exemple modifié : au
    // second clic, rien n'est recopié ni ajouté deux fois, rien n'est écrasé.
    std::fs::write(dir.join("notes_anonymise.txt"), "sortie").unwrap();
    std::fs::write(dir.join("notes.txt"), "modifié").unwrap();
    harness.get_by_label("Essayer avec des exemples").click();
    harness.run();
    assert_eq!(harness.state().anonymize.files.len(), 14);
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
        assert!(
            (r.center().y - r0.center().y).abs() <= 1.0,
            "{screen} : « {name} » centré à y = {}, « {first} » à y = {}",
            r.center().y,
            r0.center().y
        );
    }
}

#[test]
fn controles_alignes_sur_chaque_ecran() {
    // Déposer : ligne « toujours anonymiser ».
    let mut h = app(1280.0, 1600.0, |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files([file_entry(exemple("notes.txt"))], &supported);
    });
    h.run();
    let row = rects(
        &h,
        &[
            "Terme à toujours anonymiser",
            "Type du terme",
            "Ajouter à la liste",
        ],
    );
    assert_eq!(row[0].1.height(), theme::CONTROL_MD);
    assert_aligned("Déposer", &row);

    // Vérifier : niveau de protection, ajout manuel, barre d'actions.
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, Profile::Standard, &["notes.txt"]);
    });
    h.run();
    assert_aligned(
        "Vérifier, ajout",
        &rects(&h, &["Terme à ajouter", "Type du terme ajouté", "Ajouter"]),
    );
    assert_aligned("Vérifier, actions", &rects(&h, &["Retour", "Continuer"]));
    let level = h
        .get_all_by_label("Niveau de protection")
        .map(|n| n.rect())
        .collect::<Vec<_>>();
    assert_eq!(level.len(), 2, "libellé et sélecteur segmenté");
    assert!(
        (level[0].center().y - level[1].center().y).abs() <= 1.0,
        "libellé centré sur le sélecteur : {level:?}"
    );

    // Protéger : barre d'actions.
    let mut h = app(1280.0, 1600.0, |app| {
        analyze_these(app, Profile::Standard, &["notes.txt"]);
        app.anonymize.confirm_review().unwrap();
    });
    h.run();
    assert_aligned("Protéger", &rects(&h, &["Retour", "Anonymiser 1 fichier"]));

    // Terminé : boutons du résultat.
    let mut h = app(1280.0, 1600.0, |app| {
        anonymize_all(app, out_dir("alignement"));
        neutralize_paths(app);
    });
    h.run();
    assert_aligned(
        "Terminé",
        &rects(
            &h,
            &[
                "Ouvrir le dossier",
                "Copier la consigne pour l'IA",
                "Anonymiser d'autres fichiers",
            ],
        ),
    );

    // Désanonymiser : ouverture de la table.
    let mut h = app(1280.0, 800.0, |app| {
        app.tab = Tab::Deanonymize;
        let _ = app
            .deanonymize
            .choose_map(PathBuf::from("C:/Dossiers/table_anonymiseur.anonmap"));
    });
    h.run();
    assert_aligned(
        "Désanonymiser",
        &rects(&h, &["Ouvrir la table", "Changer de table"]),
    );

    // Texte libre : boutons sous les zones.
    let mut h = app(1280.0, 800.0, |app| app.tab = Tab::FreeText);
    h.run();
    let anonymize = h
        .get_all_by_label("Anonymiser")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap()
        .rect();
    let mut row = vec![("Anonymiser".to_owned(), anonymize)];
    row.extend(rects(&h, &["Restaurer", "Copier le texte anonymisé"]));
    assert_aligned("Texte libre", &row);

    // En-tête : onglets et bouton du thème.
    let tab = h
        .get_all_by_label("Anonymiser")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Tab)
        .unwrap()
        .rect();
    let mut row = vec![("onglet Anonymiser".to_owned(), tab)];
    row.extend(rects(&h, &["Thème : Système"]));
    assert_aligned("En-tête", &row);
}

#[test]
fn zones_de_texte_lisibles() {
    // 1280 × 800 : au moins 12 lignes visibles par zone, texte de 15 px.
    let mut h = app(1280.0, 800.0, |app| app.tab = Tab::FreeText);
    h.run();
    let area = h.get_by_label("Texte original").rect();
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

// --- Lot 2 : retours, raccourcis, tableau, dépôt, collage, accueil ---------

use anon_engine::Progress as _;
use anonymiseur::ui::screens::anonymize::TABLE_ID;
use anonymiseur::ui::widgets::DataTable;
use anonymiseur::ui::widgets::field::field_id;
use egui::accesskit::Role;
use egui::{Key, Modifiers};

fn hover_files(harness: &mut Harness<'_, AnonApp>, paths: &[PathBuf]) {
    for p in paths {
        harness.input_mut().hovered_files.push(egui::HoveredFile {
            path: Some(p.clone()),
            ..Default::default()
        });
    }
}

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
}

#[test]
fn action_desactivee_dit_pourquoi() {
    let mut h = app(1280.0, 1600.0, |app| {
        analyze_these(app, Profile::Standard, &["notes.txt"]);
        app.anonymize.confirm_review().unwrap();
    });
    h.run();
    h.get_by_label("Choisissez un mot de passe pour la table.");
    let mut h = app(1280.0, 800.0, |app| app.tab = Tab::Deanonymize);
    h.run();
    h.get_by_label("Ouvrez d'abord une table de correspondance.");
}

#[test]
fn raccourcis_clavier() {
    let mut h = app(1280.0, 1600.0, |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files([file_entry(exemple("notes.txt"))], &supported);
    });
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run();
    assert_eq!(h.state().tab, Tab::Deanonymize);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run();
    assert_eq!(h.state().tab, Tab::Anonymize);
    // Action principale : analyse.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert_eq!(h.state().anonymize.step, Step::Review);
    // Recherche dans le tableau.
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run();
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(field_id(anonymiseur::ui::app::SEARCH_FIELD))
    );
    // Échap quitte d'abord le champ, puis revient à l'étape précédente.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().anonymize.step, Step::Drop);
    // F1 : panneau des raccourcis.
    h.key_press(Key::F1);
    h.run();
    assert!(h.state().show_shortcuts);
    h.get_by_label("Fermer").click();
    h.run();
    assert!(!h.state().show_shortcuts);
    // Raccourci rappelé dans l'infobulle des onglets (texte construit ici).
    assert_eq!(
        anonymiseur::ui::shortcuts::TABS[0].label_for(false),
        "Ctrl+1"
    );
}

#[test]
fn tableau_tri_recherche_et_clavier() {
    let mut h = app(1280.0, 2400.0, |app| {
        analyze_these(app, Profile::Standard, &["notes.txt"]);
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
    // Clavier : ↓ passe à la ligne suivante, Espace la coche ou la décoche.
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
    // Recherche : seules les lignes correspondantes restent.
    h.state_mut().anonymize.search = "example.com".into();
    h.run();
    let found = h.state().anonymize.visible();
    assert!(!found.is_empty() && found.len() < visible.len());
}

#[test]
fn voile_de_depot_et_table_deposee() {
    // Anonymiser : le voile annonce ce qui va se passer.
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    hover_files(&mut h, &[exemple("notes.txt"), exemple("brief.md")]);
    // egui redessine sans cesse pendant le survol : quelques images suffisent.
    h.run_steps(3);
    h.get_by_label("Déposez pour ajouter 2 fichiers");
    h.input_mut().hovered_files.clear();
    // Une table déposée est proposée à la reprise.
    h.input_mut()
        .dropped_files
        .push(Arc::new(Dropped(PathBuf::from(
            "C:/d/table_anonymiseur.anonmap",
        ))));
    h.run();
    h.get_by_label("Reprendre cette table").click();
    h.run();
    assert_eq!(
        h.state().anonymize.protect.mode,
        anonymiseur::ui::state::anonymize::ProtectMode::ExistingTable
    );

    // Désanonymiser : la table s'ouvre, les documents s'ajoutent.
    let mut h = app(1280.0, 800.0, |app| app.tab = Tab::Deanonymize);
    h.run();
    hover_files(&mut h, &[exemple("notes.txt")]);
    h.run_steps(3);
    h.get_by_label("Déposez la table de correspondance ou les fichiers à restaurer");
    h.input_mut().hovered_files.clear();
    for p in [
        PathBuf::from("C:/d/table_anonymiseur.anonmap"),
        exemple("notes.txt"),
    ] {
        h.input_mut().dropped_files.push(Arc::new(Dropped(p)));
    }
    h.run();
    assert!(matches!(
        h.state().deanonymize.map,
        anonymiseur::ui::state::deanonymize::MapStatus::Locked { .. }
    ));
    assert_eq!(h.state().deanonymize.input_files.len(), 1);
}

/// Table ouverte dans Désanonymiser, après une vraie anonymisation.
fn unlocked(app: &mut AnonApp, tag: &str) {
    anonymize_all(app, out_dir(tag));
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
    let opened = anon_vault::open(&std::fs::read(path).unwrap(), &pw).map_err(|e| e.to_string());
    d.unlock_done(opened);
}

#[test]
fn coller_et_restaurer_en_un_geste() {
    let mut h = app(1280.0, 1600.0, |app| unlocked(app, "coller"));
    h.run();
    h.get_by_label("Coller depuis le presse-papiers").click();
    h.run();
    let reponse = std::fs::read_to_string(exemple("reponse-ia-exemple.md")).unwrap();
    h.input_mut().events.push(egui::Event::Paste(reponse));
    h.run();
    let (text, report) = h.state().deanonymize.text_result.clone().unwrap();
    assert!(!text.contains("[PERSONNE_1]"));
    assert_eq!(report.unknown, vec!["[PERSONNE_99]".to_owned()]);
    // Balise inconnue signalée par son nombre.
    h.get_by_label("1 BALISE INCONNUE");
    h.get_by_label("Copier le résultat").click();
    h.run();
    h.get_by_label("Copié");
    assert!(h.state().toasts.texts().contains(&"Résultat copié"));
}

#[test]
fn avancement_par_fichier() {
    let mut h = app(1280.0, 1600.0, |app| {
        let supported = app.engine.as_ref().unwrap().supported_extensions();
        app.anonymize
            .add_files(FICHIERS.iter().map(|n| file_entry(exemple(n))), &supported);
        app.anonymize.busy = Some(Busy::Analyzing);
        app.progress().step(1, 4, "fichier-clients.xlsx");
    });
    h.run();
    h.get_by_label("Terminé");
    h.get_by_label("En cours");
    assert_eq!(h.get_all_by_label("En attente").count(), 2);
    h.get_by_label("Temps écoulé : 0 s");
    h.get_by_label("Annuler");
}

#[test]
fn bandeau_d_accueil_refermable() {
    let mut h = app(1280.0, 800.0, |_| {});
    h.run();
    h.get_by_label_contains("Bienvenue");
    h.get_by_label("Fermer le message d'accueil").click();
    h.run();
    assert!(h.state().prefs.welcome_seen);
    assert!(h.query_by_label_contains("Bienvenue").is_none());
}

#[test]
fn chaque_bouton_a_un_nom() {
    let screens: [fn(&mut AnonApp); 4] = [
        |_| {},
        |app| analyze_these(app, Profile::Standard, &["notes.txt"]),
        |app| app.tab = Tab::Deanonymize,
        |app| app.tab = Tab::FreeText,
    ];
    for setup in screens {
        let mut h = app(1280.0, 2400.0, setup);
        h.run();
        for n in h.get_all_by_role(Role::Button) {
            let label = n.accesskit_node().label().unwrap_or_default();
            assert!(!label.trim().is_empty(), "bouton sans nom : {n:?}");
        }
    }
}

#[test]
fn anonymize_drop_overlay() {
    // Voile pendant le survol de fichiers, en clair et en sombre.
    let mut results = SnapshotResults::new();
    for (theme, label) in [
        (ThemeChoice::Light, "1280x800"),
        (ThemeChoice::Dark, "sombre_1280x800"),
    ] {
        let mut h = app(1280.0, 800.0, |app| {
            app.prefs.theme = theme;
            app.prefs.welcome_seen = true;
            let supported = app.engine.as_ref().unwrap().supported_extensions();
            app.anonymize
                .add_files([file_entry(exemple("notes.txt"))], &supported);
            app.toasts = Default::default();
        });
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
        h.snapshot(format!("anonymiser_1d_voile_de_depot_{label}"));
        results.extend_harness(&mut h);
    }
    results.unwrap();
}

#[test]
fn shortcuts_panel() {
    snap("raccourcis", |app| {
        app.show_shortcuts = true;
        app.prefs.welcome_seen = true;
    });
}
