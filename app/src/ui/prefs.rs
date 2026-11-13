//! Préférences retenues d'une session à l'autre : thème, dernier niveau de
//! protection, taille de la fenêtre, bandeau d'accueil vu.
//!
//! Petit fichier JSON dans le dossier de configuration du système
//! (`%APPDATA%\Anonymiseur\preferences.json` sous Windows). Il ne contient
//! aucun chemin de fichier ni aucun contenu de l'utilisateur : seulement les
//! champs de [`Preferences`]. Un fichier absent ou abîmé donne les réglages
//! par défaut, sans erreur.

use crate::ui::theme::ThemeChoice;
use anon_core::Profile;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Nom du dossier de l'application dans le dossier de configuration.
const APP_DIR: &str = "Anonymiseur";
const FILE_NAME: &str = "preferences.json";

/// Plus petite et plus grande taille de fenêtre retenue (points).
pub const WINDOW_MIN: [f32; 2] = [900.0, 650.0];
const WINDOW_MAX: f32 = 8192.0;

/// Niveau de protection enregistré.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SavedProfile {
    #[default]
    Standard,
    Renforce,
}

impl From<Profile> for SavedProfile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Standard => SavedProfile::Standard,
            Profile::Renforce => SavedProfile::Renforce,
        }
    }
}

impl From<SavedProfile> for Profile {
    fn from(p: SavedProfile) -> Self {
        match p {
            SavedProfile::Standard => Profile::Standard,
            SavedProfile::Renforce => Profile::Renforce,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: ThemeChoice,
    pub profile: SavedProfile,
    /// Taille intérieure de la fenêtre, en points.
    pub window: Option<[f32; 2]>,
    /// Bandeau d'accueil refermé.
    pub welcome_seen: bool,
}

impl Preferences {
    /// Taille de fenêtre retenue, ramenée dans des bornes raisonnables.
    pub fn window_size(&self) -> Option<[f32; 2]> {
        let [w, h] = self.window?;
        if !(w.is_finite() && h.is_finite()) {
            return None;
        }
        Some([
            w.clamp(WINDOW_MIN[0], WINDOW_MAX),
            h.clamp(WINDOW_MIN[1], WINDOW_MAX),
        ])
    }

    /// Lit les préférences ; tout problème donne les réglages par défaut.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Écrit les préférences (dossier créé au besoin). Échec silencieux : ce ne
    /// sont que des réglages de confort.
    pub fn save(&self, path: &Path) -> bool {
        let Ok(json) = serde_json::to_string_pretty(self) else {
            return false;
        };
        if let Some(dir) = path.parent()
            && std::fs::create_dir_all(dir).is_err()
        {
            return false;
        }
        // Écriture dans un fichier voisin puis renommage : un arrêt brutal ne
        // laisse pas un fichier tronqué.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, path).is_ok()
    }
}

/// Emplacement du fichier de préférences, s'il est connu.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR).join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("anonymiseur-prefs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("sous-dossier").join(FILE_NAME)
    }

    #[test]
    fn round_trip() {
        let path = temp("aller-retour");
        let p = Preferences {
            theme: ThemeChoice::Dark,
            profile: SavedProfile::Renforce,
            window: Some([1400.0, 900.0]),
            welcome_seen: true,
        };
        assert!(p.save(&path));
        assert_eq!(Preferences::load(&path), p);
        if let Some(d) = path.parent().and_then(Path::parent) {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        let path = temp("abime");
        assert_eq!(Preferences::load(&path), Preferences::default());
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&path, "{ pas du json");
        assert_eq!(Preferences::load(&path), Preferences::default());
        // Champ inconnu ou manquant : le reste est lu.
        let _ = std::fs::write(&path, r#"{"theme":"Light","inconnu":3}"#);
        let p = Preferences::load(&path);
        assert_eq!(p.theme, ThemeChoice::Light);
        assert_eq!(p.profile, SavedProfile::Standard);
        if let Some(d) = path.parent().and_then(Path::parent) {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn file_holds_no_path_or_content() {
        let p = Preferences {
            window: Some([1280.0, 800.0]),
            ..Default::default()
        };
        let json = serde_json::to_string(&p).unwrap_or_default();
        let keys: Vec<&str> = ["theme", "profile", "window", "welcome_seen"].to_vec();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
        let found: Vec<String> = value
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        assert_eq!(found.len(), keys.len());
        assert!(found.iter().all(|k| keys.contains(&k.as_str())));
        assert!(!json.contains('/') && !json.contains('\\'));
    }

    #[test]
    fn window_size_is_clamped() {
        let p = |w| Preferences {
            window: w,
            ..Default::default()
        };
        assert_eq!(p(None).window_size(), None);
        assert_eq!(p(Some([100.0, 100.0])).window_size(), Some(WINDOW_MIN));
        assert_eq!(p(Some([f32::NAN, 700.0])).window_size(), None);
        assert_eq!(
            p(Some([1300.0, 820.0])).window_size(),
            Some([1300.0, 820.0])
        );
    }

    #[test]
    fn profile_conversion() {
        assert_eq!(Profile::from(SavedProfile::Renforce), Profile::Renforce);
        assert_eq!(
            SavedProfile::from(Profile::Standard),
            SavedProfile::Standard
        );
    }
}
