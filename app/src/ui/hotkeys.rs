//! Raccourcis globaux du presse-papiers express : Ctrl+Alt+M (masquer) et
//! Ctrl+Alt+R (restaurer), Cmd+Option sous macOS, actifs même quand la fenêtre
//! n'a pas le focus. `global-hotkey` (RegisterHotKey sous Windows, Carbon
//! sous macOS), derrière un trait pour que la logique se teste sans système.
//!
//! Enregistrement sur le fil principal (exigence de macOS) : `AnonApp::new` est
//! appelé par eframe sur ce fil, la boucle d'événements de winit déjà lancée ;
//! sous Windows, elle pompe aussi les messages de la fenêtre cachée de
//! `global-hotkey`. Chaque appui réveille l'application (`request_repaint`) :
//! aucune scrutation, aucun calcul tant que rien n'est pressé.

use super::state::express::Action;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

/// Raccourci global : touche et libellé selon le système.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalShortcut {
    pub action: Action,
    /// Lettre de la touche.
    pub letter: char,
}

impl GlobalShortcut {
    /// « Ctrl+Alt+M » (Windows), « Cmd+Option+M » (macOS).
    pub fn label(self) -> String {
        self.label_for(cfg!(target_os = "macos"))
    }

    pub fn label_for(self, mac: bool) -> String {
        let mods = if mac { "Cmd+Option+" } else { "Ctrl+Alt+" };
        format!("{mods}{}", self.letter)
    }

    fn code(self) -> Code {
        match self.action {
            Action::Mask => Code::KeyM,
            Action::Restore => Code::KeyR,
        }
    }

    fn hotkey(self) -> HotKey {
        let mods = if cfg!(target_os = "macos") {
            Modifiers::META | Modifiers::ALT
        } else {
            Modifiers::CONTROL | Modifiers::ALT
        };
        HotKey::new(Some(mods), self.code())
    }
}

/// Masquer le texte copié.
pub const MASK: GlobalShortcut = GlobalShortcut {
    action: Action::Mask,
    letter: 'M',
};
/// Restaurer le texte copié.
pub const RESTORE: GlobalShortcut = GlobalShortcut {
    action: Action::Restore,
    letter: 'R',
};
pub const ALL: [GlobalShortcut; 2] = [MASK, RESTORE];

/// Raccourci d'une action.
pub fn shortcut(action: Action) -> GlobalShortcut {
    match action {
        Action::Mask => MASK,
        Action::Restore => RESTORE,
    }
}

pub trait Hotkeys {
    /// Enregistre (`true`) ou retire les deux raccourcis. Renvoie les actions
    /// dont le raccourci n'a pas pu être enregistré (déjà pris par une autre
    /// application, système sans raccourcis globaux).
    fn set_enabled(&mut self, on: bool) -> Vec<Action>;
    /// Raccourcis pressés depuis le dernier appel.
    fn poll(&mut self) -> Vec<Action>;
}

/// Sans raccourcis globaux (tests, instantanés).
#[derive(Debug, Default)]
pub struct NoHotkeys;

impl Hotkeys for NoHotkeys {
    fn set_enabled(&mut self, _on: bool) -> Vec<Action> {
        Vec::new()
    }

    fn poll(&mut self) -> Vec<Action> {
        Vec::new()
    }
}

/// État des raccourcis simulés, partagé entre le test et l'application.
#[derive(Debug, Default)]
pub struct ScriptState {
    /// Appuis à rendre au prochain `poll`.
    pub pressed: Vec<Action>,
    /// Raccourcis déjà pris par une autre application.
    pub taken: Vec<Action>,
    /// Raccourcis enregistrés.
    pub enabled: bool,
    /// Appels à `set_enabled`.
    pub calls: Vec<bool>,
}

/// Raccourcis simulés : le test les « presse » ; un raccourci peut être déjà pris.
#[derive(Debug, Clone, Default)]
pub struct ScriptedHotkeys(pub Arc<Mutex<ScriptState>>);

impl ScriptedHotkeys {
    fn with<R>(&self, f: impl FnOnce(&mut ScriptState) -> R) -> R {
        let mut s = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut s)
    }

    /// Appui sur le raccourci de `action` (ignoré s'il n'est pas enregistré).
    pub fn press(&self, action: Action) {
        self.with(|s| {
            if s.enabled && !s.taken.contains(&action) {
                s.pressed.push(action);
            }
        });
    }

    /// Le raccourci de `action` est déjà pris par une autre application.
    pub fn take(&self, action: Action) {
        self.with(|s| s.taken.push(action));
    }

    pub fn is_enabled(&self) -> bool {
        self.with(|s| s.enabled)
    }

    pub fn calls(&self) -> Vec<bool> {
        self.with(|s| s.calls.clone())
    }
}

impl Hotkeys for ScriptedHotkeys {
    fn set_enabled(&mut self, on: bool) -> Vec<Action> {
        self.with(|s| {
            s.calls.push(on);
            s.enabled = on;
            if on { s.taken.clone() } else { Vec::new() }
        })
    }

    fn poll(&mut self) -> Vec<Action> {
        self.with(|s| std::mem::take(&mut s.pressed))
    }
}

/// Destinataire des appuis : le gestionnaire d'événements de `global-hotkey`
/// ne s'installe qu'une fois par processus ; il transmet au dernier
/// `GlobalHotkeys` créé (repli d'eframe sur OpenGL : une seconde application).
static TARGET: Mutex<Option<(Sender<u32>, egui::Context)>> = Mutex::new(None);

/// Raccourcis globaux du système.
pub struct GlobalHotkeys {
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<GlobalShortcut>,
    rx: Receiver<u32>,
}

impl std::fmt::Debug for GlobalHotkeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalHotkeys")
            .field("available", &self.manager.is_some())
            .field("registered", &self.registered)
            .finish_non_exhaustive()
    }
}

impl GlobalHotkeys {
    /// À appeler sur le fil principal. `ctx` est réveillé à chaque appui, même
    /// fenêtre réduite (`App::logic` lit alors les appuis).
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        *TARGET.lock().unwrap_or_else(PoisonError::into_inner) = Some((tx, ctx));
        GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed {
                return;
            }
            if let Some((tx, ctx)) = TARGET
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                let _ = tx.send(event.id);
                ctx.request_repaint();
            }
        }));
        Self {
            manager: GlobalHotKeyManager::new().ok(),
            registered: Vec::new(),
            rx,
        }
    }
}

impl Hotkeys for GlobalHotkeys {
    fn set_enabled(&mut self, on: bool) -> Vec<Action> {
        let Some(manager) = &self.manager else {
            return if on { Action::ALL.to_vec() } else { Vec::new() };
        };
        if !on {
            for s in self.registered.drain(..) {
                let _ = manager.unregister(s.hotkey());
            }
            return Vec::new();
        }
        let mut failed = Vec::new();
        for s in ALL {
            if self.registered.contains(&s) {
                continue;
            }
            match manager.register(s.hotkey()) {
                Ok(()) => self.registered.push(s),
                Err(_) => failed.push(s.action),
            }
        }
        failed
    }

    fn poll(&mut self) -> Vec<Action> {
        let ids: Vec<u32> = self.rx.try_iter().collect();
        ids.into_iter()
            .filter_map(|id| {
                self.registered
                    .iter()
                    .find(|s| s.hotkey().id() == id)
                    .map(|s| s.action)
            })
            .collect()
    }
}

impl Drop for GlobalHotkeys {
    fn drop(&mut self) {
        let _ = self.set_enabled(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_platform() {
        assert_eq!(MASK.label_for(false), "Ctrl+Alt+M");
        assert_eq!(RESTORE.label_for(false), "Ctrl+Alt+R");
        assert_eq!(MASK.label_for(true), "Cmd+Option+M");
        assert_eq!(RESTORE.label_for(true), "Cmd+Option+R");
        assert_eq!(shortcut(Action::Restore), RESTORE);
    }

    #[test]
    fn hotkeys_are_distinct() {
        assert_ne!(MASK.hotkey().id(), RESTORE.hotkey().id());
    }

    #[test]
    fn scripted_hotkeys_report_taken_shortcuts() {
        let handle = ScriptedHotkeys::default();
        let mut app_side: Box<dyn Hotkeys> = Box::new(handle.clone());
        handle.press(Action::Mask);
        assert!(app_side.poll().is_empty(), "pas encore enregistrés");
        handle.take(Action::Restore);
        assert_eq!(app_side.set_enabled(true), [Action::Restore]);
        handle.press(Action::Mask);
        handle.press(Action::Restore);
        assert_eq!(
            app_side.poll(),
            [Action::Mask],
            "le raccourci pris ne vient pas"
        );
        assert!(app_side.poll().is_empty());
        assert!(app_side.set_enabled(false).is_empty());
        assert!(!handle.is_enabled());
        assert_eq!(handle.calls(), [true, false]);
    }
}
