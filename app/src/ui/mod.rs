//! Interface egui à la charte Skazy Formation. Aucune logique métier ici.
//!
//! - `theme` : jetons de la charte appliqués à egui ;
//! - `widgets` : composants (boutons, badges, cartes, champs…) ;
//! - `state` : état et transitions (dossier de travail, étapes, décisions,
//!   palette de commandes, historique d'annulation), sans egui ;
//! - `screens` : rendu de l'accueil, de chaque étape et de la palette ;
//! - `app` : fenêtre, en-tête, barres du dossier, pied ;
//! - `runner` : fil de travail (traitements longs, avancement, annulation) ;
//! - `service` : opérations sur les fichiers, sans egui ;
//! - `prefs` : préférences retenues d'une session à l'autre ;
//! - `clipboard`, `hotkeys` : presse-papiers et raccourcis globaux du système
//!   (presse-papiers express), derrière des traits simulés dans les tests.

pub mod app;
pub mod clipboard;
pub mod gallery;
pub mod hotkeys;
pub mod platform;
pub mod prefs;
pub mod runner;
pub mod screens;
pub mod service;
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod widgets;

pub use app::AnonApp;
