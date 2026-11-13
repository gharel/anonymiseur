//! Anonymiseur — application de bureau. L'interface est construite dans `ui/`.

// Pas de console sous Windows en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anonymiseur::ui::AnonApp;
use anonymiseur::ui::platform::NativePlatform;
use anonymiseur::ui::prefs::{self, Preferences};
use std::sync::Arc;

/// Taille de départ et taille minimale de la fenêtre.
const INITIAL_SIZE: [f32; 2] = [1280.0, 800.0];
const MIN_SIZE: [f32; 2] = prefs::WINDOW_MIN;

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    // Taille de la dernière session, si elle a été retenue.
    let size = prefs::default_path()
        .map(|p| Preferences::load(&p))
        .and_then(|p| p.window_size())
        .unwrap_or(INITIAL_SIZE);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Anonymiseur")
        .with_inner_size(size)
        .with_min_inner_size(MIN_SIZE)
        .with_drag_and_drop(true);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../assets/charte/icon-256.png"))
    {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    eframe::NativeOptions {
        viewport,
        renderer,
        // Les préférences sont dans un fichier à nous (`ui::prefs`), sans
        // mémoire d'egui : rien de ce qui est saisi n'est écrit sur le disque.
        persist_window: false,
        ..Default::default()
    }
}

fn run(renderer: eframe::Renderer) -> eframe::Result {
    eframe::run_native(
        "Anonymiseur",
        options(renderer),
        Box::new(|cc| {
            Ok(Box::new(AnonApp::new(
                &cc.egui_ctx,
                Box::new(NativePlatform),
            )))
        }),
    )
}

fn main() -> std::process::ExitCode {
    // wgpu d'abord ; repli sur OpenGL si aucun adaptateur n'est disponible
    // (machine virtuelle, pilote ancien).
    match run(eframe::Renderer::Wgpu) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => match run(eframe::Renderer::Glow) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => std::process::ExitCode::FAILURE,
        },
    }
}
