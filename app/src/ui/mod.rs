//! Interface egui à la charte Skazy Formation. Aucune logique métier ici.
//!
//! - `theme` : jetons de la charte appliqués à egui ;
//! - `widgets` : composants (boutons, badges, cartes, champs…) ;
//! - `state` : état et transitions de chaque onglet, sans egui ;
//! - `screens` : rendu de chaque onglet ;
//! - `app` : fenêtre, en-tête, onglets, pied ;
//! - `mock` : données factices de la maquette.

pub mod app;
pub mod gallery;
pub mod mock;
pub mod platform;
pub mod screens;
pub mod state;
pub mod theme;
pub mod widgets;

pub use app::AnonApp;
