//! Instantanés PNG de la galerie et de chaque écran de la maquette, à deux
//! tailles de fenêtre. Mise à jour : `UPDATE_SNAPSHOTS=force cargo test -p anonymiseur`.

use anon_core::Profile;
use anonymiseur::ui::gallery::{self, GalleryState};
use anonymiseur::ui::mock;
use anonymiseur::ui::platform::NullPlatform;
use anonymiseur::ui::state::Tab;
use anonymiseur::ui::state::anonymize::Step;
use anonymiseur::ui::state::files::FileEntry;
use anonymiseur::ui::{AnonApp, theme};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, SnapshotResults};

/// Deux tailles de fenêtre, plus une page haute pour voir le contenu sous la ligne de flottaison.
const SIZES: [(f32, f32, &str); 3] = [
    (1280.0, 800.0, "1280x800"),
    (900.0, 650.0, "900x650"),
    (1280.0, 1500.0, "pleine_page"),
];

fn app(width: f32, height: f32, setup: impl FnOnce(&mut AnonApp)) -> Harness<'static, AnonApp> {
    Harness::builder()
        .with_size([width, height])
        .with_pixels_per_point(1.0)
        .build_eframe(|cc| {
            let mut app = AnonApp::new(&cc.egui_ctx, Box::new(NullPlatform::default()));
            setup(&mut app);
            app
        })
}

fn snap(name: &str, setup: impl Fn(&mut AnonApp)) {
    let mut results = SnapshotResults::new();
    for (w, h, label) in SIZES {
        let mut harness = app(w, h, &setup);
        harness.run();
        harness.snapshot(format!("{name}_{label}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}

fn with_files(app: &mut AnonApp) {
    app.anonymize.add_files(mock::files());
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
fn anonymize_drop() {
    snap("anonymiser_1_deposer", |app| {
        with_files(app);
        app.anonymize.add_files(vec![FileEntry::new(
            "C:/Dossiers/Budget formation.gsheet".into(),
            Some(176),
        )]);
    });
}

#[test]
fn anonymize_drop_empty() {
    snap("anonymiser_0_vide", |_| {});
}

#[test]
fn anonymize_review() {
    snap("anonymiser_2_verifier", |app| {
        with_files(app);
        let _ = app.anonymize.start_review(mock::candidates());
        app.anonymize.review.profile = Profile::Renforce;
    });
}

#[test]
fn anonymize_protect() {
    snap("anonymiser_3_proteger", |app| {
        with_files(app);
        let _ = app.anonymize.start_review(mock::candidates());
        let _ = app.anonymize.confirm_review();
        app.anonymize.protect.password = "Cheval-Agrafe-Batterie-47".into();
        app.anonymize.protect.confirm = "Cheval-Agrafe".into();
    });
}

#[test]
fn anonymize_done() {
    snap("anonymiser_4_termine", |app| {
        with_files(app);
        let _ = app.anonymize.start_review(mock::candidates());
        let _ = app.anonymize.confirm_review();
        app.anonymize.protect.irreversible = true;
        let _ = app.anonymize.finish(mock::reports());
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
            .choose_map("C:/Dossiers/Contrat (anonymisé).anonmap".into());
        app.deanonymize.unlock_error =
            Some(anonymiseur::ui::state::deanonymize::UnlockError::WrongPassword);
    });
}

#[test]
fn deanonymize_result() {
    snap("desanonymiser_3_resultat", |app| {
        app.tab = Tab::Deanonymize;
        let d = &mut app.deanonymize;
        let _ = d.choose_map("C:/Dossiers/Contrat (anonymisé).anonmap".into());
        d.password = "secret".into();
        let _ = d.unlock(|_, _| Ok((12, "12 novembre 2026".into())));
        d.pasted = mock::AI_RESPONSE.into();
        let _ = d.restore(mock::restore);
    });
}

#[test]
fn free_text() {
    snap("texte_libre", |app| {
        app.tab = Tab::FreeText;
        app.free_text.original = mock::FREE_TEXT.into();
        app.free_text.anonymize(mock::anonymize_text);
    });
}

// --- Interactions (sans image) ---------------------------------------------

#[test]
fn dropped_file_appears_in_the_list() {
    let mut harness = app(1280.0, 800.0, |_| {});
    harness.run();
    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }
    }
    harness
        .input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(
            "C:/Dossiers/Note interne.docx".into(),
        )));
    harness.run();
    assert_eq!(harness.state().anonymize.files.len(), 1);
    harness.get_by_label("Note interne.docx");
}

#[test]
fn tabs_switch_by_click() {
    let mut harness = app(1280.0, 800.0, |_| {});
    harness.run();
    harness.get_by_label("Texte libre").click();
    harness.run();
    assert_eq!(harness.state().tab, Tab::FreeText);
    harness.get_by_label("Désanonymiser").click();
    harness.run();
    assert_eq!(harness.state().tab, Tab::Deanonymize);
}

#[test]
fn analyse_button_moves_to_review_and_back() {
    // Fenêtre haute : le bouton « Retour » est sous la ligne de flottaison à 800 px.
    let mut harness = app(1280.0, 1500.0, with_files);
    harness.run();
    harness.get_by_label("Analyser les fichiers").click();
    harness.run();
    assert_eq!(harness.state().anonymize.step, Step::Review);
    harness.get_by_label("Retour").click();
    harness.run();
    assert_eq!(harness.state().anonymize.step, Step::Drop);
}

#[test]
fn keyboard_reaches_the_tabs() {
    let mut harness = app(1280.0, 800.0, |_| {});
    harness.run();
    // Tab donne le focus au premier élément interactif : l'onglet « Anonymiser ».
    harness.key_press(egui::Key::Tab);
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().tab, Tab::Deanonymize);
}

#[test]
fn free_text_buttons_work() {
    let mut harness = app(1280.0, 800.0, |app| {
        app.tab = Tab::FreeText;
        app.free_text.original = mock::FREE_TEXT.into();
    });
    harness.run();
    let buttons: Vec<_> = harness
        .get_all_by_label("Anonymiser")
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .collect();
    assert_eq!(buttons.len(), 1);
    if let Some(b) = buttons.first() {
        b.click();
    }
    harness.run();
    assert!(
        harness
            .state()
            .free_text
            .anonymized
            .contains("[PERSONNE_1]")
    );
}
