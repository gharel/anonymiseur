//! Presse-papiers du système, lu et écrit hors de la fenêtre (presse-papiers
//! express) : `arboard`, la bibliothèque qu'egui utilise déjà. Derrière un
//! trait, pour que la logique se teste avec un presse-papiers en mémoire.

use super::state::express::ClipboardRead;
use std::sync::{Arc, Mutex, PoisonError};

pub trait Clipboard {
    /// Texte du presse-papiers.
    fn read_text(&mut self) -> ClipboardRead;
    /// Remplace le presse-papiers par `text`. `sensitive` : vraies valeurs
    /// (texte restauré), tenues hors de l'historique et du presse-papiers en
    /// ligne du système. Faux si le système a refusé.
    fn write_text(&mut self, text: &str, sensitive: bool) -> bool;
}

/// Presse-papiers du système.
#[derive(Default)]
pub struct SystemClipboard {
    /// Gardé ouvert : sous Linux (X11), le contenu écrit vit tant qu'il existe.
    inner: Option<arboard::Clipboard>,
}

impl std::fmt::Debug for SystemClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SystemClipboard")
    }
}

impl SystemClipboard {
    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.inner.is_none() {
            self.inner = arboard::Clipboard::new().ok();
        }
        self.inner.as_mut()
    }
}

impl Clipboard for SystemClipboard {
    fn read_text(&mut self) -> ClipboardRead {
        let Some(c) = self.clipboard() else {
            return ClipboardRead::Unavailable;
        };
        match c.get_text() {
            Ok(t) => ClipboardRead::Text(t),
            Err(arboard::Error::ContentNotAvailable) => ClipboardRead::Empty,
            Err(_) => ClipboardRead::Unavailable,
        }
    }

    fn write_text(&mut self, text: &str, sensitive: bool) -> bool {
        let Some(c) = self.clipboard() else {
            return false;
        };
        let set = c.set();
        #[cfg(windows)]
        let set = if sensitive {
            use arboard::SetExtWindows;
            set.exclude_from_history().exclude_from_cloud()
        } else {
            set
        };
        #[cfg(target_os = "macos")]
        let set = if sensitive {
            use arboard::SetExtApple;
            set.exclude_from_history()
        } else {
            set
        };
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = sensitive;
        set.text(text.to_owned()).is_ok()
    }
}

/// Contenu du presse-papiers en mémoire, partagé entre le test et l'application.
#[derive(Default)]
pub struct MemoryState {
    pub content: Option<ClipboardRead>,
    /// Écritures reçues : (texte, sensible).
    pub writes: Vec<(String, bool)>,
    /// Le système refuse l'écriture.
    pub refuse_writes: bool,
}

impl std::fmt::Debug for MemoryState {
    /// Jamais le texte : le nombre d'écritures seulement.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryState")
            .field("content", &self.content)
            .field("writes", &self.writes.len())
            .finish_non_exhaustive()
    }
}

/// Presse-papiers en mémoire : tests et instantanés (jamais celui de
/// l'utilisateur).
#[derive(Debug, Clone, Default)]
pub struct MemoryClipboard(pub Arc<Mutex<MemoryState>>);

impl MemoryClipboard {
    fn with<R>(&self, f: impl FnOnce(&mut MemoryState) -> R) -> R {
        let mut s = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut s)
    }

    /// Le presse-papiers contient `text`.
    pub fn set_text(&self, text: &str) {
        self.with(|s| s.content = Some(ClipboardRead::Text(text.to_owned())));
    }

    /// Le presse-papiers contient autre chose que du texte (ou rien).
    pub fn set_empty(&self) {
        self.with(|s| s.content = Some(ClipboardRead::Empty));
    }

    /// Texte actuel, s'il y en a un.
    pub fn text(&self) -> Option<String> {
        self.with(|s| match &s.content {
            Some(ClipboardRead::Text(t)) => Some(t.clone()),
            _ => None,
        })
    }

    /// Écritures reçues : (texte, sensible).
    pub fn writes(&self) -> Vec<(String, bool)> {
        self.with(|s| s.writes.clone())
    }

    pub fn refuse_writes(&self, refuse: bool) {
        self.with(|s| s.refuse_writes = refuse);
    }
}

impl Clipboard for MemoryClipboard {
    fn read_text(&mut self) -> ClipboardRead {
        self.with(|s| s.content.clone().unwrap_or(ClipboardRead::Empty))
    }

    fn write_text(&mut self, text: &str, sensitive: bool) -> bool {
        self.with(|s| {
            if s.refuse_writes {
                return false;
            }
            s.writes.push((text.to_owned(), sensitive));
            s.content = Some(ClipboardRead::Text(text.to_owned()));
            true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_clipboard_is_shared_with_the_test() {
        let handle = MemoryClipboard::default();
        let mut app_side: Box<dyn Clipboard> = Box::new(handle.clone());
        assert_eq!(app_side.read_text(), ClipboardRead::Empty);
        handle.set_text("Bonjour");
        assert_eq!(app_side.read_text(), ClipboardRead::Text("Bonjour".into()));
        assert!(app_side.write_text("[PERSONNE_1]", false));
        assert_eq!(handle.text().as_deref(), Some("[PERSONNE_1]"));
        handle.refuse_writes(true);
        assert!(!app_side.write_text("x", true));
        assert_eq!(handle.writes(), [("[PERSONNE_1]".to_owned(), false)]);
    }
}
