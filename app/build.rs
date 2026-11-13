//! Ressources de l'exécutable Windows : icône et informations de version.
//!
//! L'absence de console est réglée dans `main.rs` (`windows_subsystem = "windows"` hors debug).

use std::env;

/// Langue des informations de version : français (France).
const LANGUE_FR_FR: u16 = 0x040C;

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../assets/charte/icon.ico");

    // Le script s'exécute sur l'hôte : on regarde la cible, pas `cfg!(windows)`.
    let cible_windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let cible_msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if !(cible_windows && cible_msvc) {
        return Ok(());
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // Édition complète (fonctionnalité `ia`) ou Lite (`--no-default-features`).
    let (produit, fichier) = if env::var_os("CARGO_FEATURE_IA").is_some() {
        ("Anonymiseur", "Anonymiseur.exe")
    } else {
        ("Anonymiseur Lite", "Anonymiseur-Lite.exe")
    };
    let mut ressources = winresource::WindowsResource::new();
    ressources
        .set_icon("../assets/charte/icon.ico")
        .set_language(LANGUE_FR_FR)
        .set("ProductName", produit)
        .set("FileDescription", produit)
        .set("CompanyName", "Skazy Formation")
        .set("LegalCopyright", "Skazy Formation, licence AGPL-3.0")
        .set("OriginalFilename", fichier)
        .set("InternalName", "anonymiseur")
        .set("FileVersion", &version)
        .set("ProductVersion", &version);
    ressources.compile()
}
