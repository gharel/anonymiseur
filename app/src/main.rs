//! Anonymiseur — application de bureau. L'interface est construite dans `ui/`.

// Pas de console sous Windows en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anonymiseur::ui::AnonApp;
use anonymiseur::ui::platform::NativePlatform;
use std::sync::Arc;

/// Taille de départ et taille minimale de la fenêtre.
const INITIAL_SIZE: [f32; 2] = [1280.0, 800.0];
const MIN_SIZE: [f32; 2] = [900.0, 650.0];

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Anonymiseur")
        .with_inner_size(INITIAL_SIZE)
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
