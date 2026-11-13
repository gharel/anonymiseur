//! Interface egui à la charte Skazy Formation. Aucune logique métier ici.
//!
//! - `theme` : jetons de la charte appliqués à egui ;
//! - `widgets` : composants (boutons, badges, cartes, champs…) ;
//! - `state` : état et transitions de chaque onglet, sans egui ;
//! - `screens` : rendu de chaque onglet ;
//! - `app` : fenêtre, en-tête, onglets, pied ;
//! - `runner` : fil de travail (traitements longs, avancement, annulation) ;
//! - `service` : opérations sur les fichiers, sans egui ;
//! - `prefs` : préférences retenues d'une session à l'autre.

pub mod app;
pub mod gallery;
pub mod platform;
pub mod prefs;
pub mod runner;
pub mod screens;
pub mod service;
pub mod state;
pub mod theme;
pub mod widgets;

pub use app::AnonApp;
