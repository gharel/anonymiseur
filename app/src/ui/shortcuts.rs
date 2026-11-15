//! Raccourcis clavier : la liste, leur libellé (Ctrl sous Windows, Cmd sous
//! macOS) et la lecture des touches au début de chaque image.

use egui::{Key, KeyboardShortcut, Modifiers};

/// Action déclenchée par un raccourci.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Ajouter des fichiers au dossier.
    Open,
    /// Ouvrir ou fermer la palette de commandes.
    Palette,
    /// Étape suivante (ou action principale de la dernière étape).
    Next,
    /// Annuler le traitement en cours, sinon revenir.
    Back,
    /// Annuler la dernière décision de vérification.
    Undo,
    /// Rétablir la décision annulée.
    Redo,
    /// Coller la réponse de l'IA et la restaurer.
    PasteResponse,
    /// Recherche dans le tableau de vérification.
    Find,
    /// Panneau des raccourcis.
    Help,
    /// Aller à l'étape `n` du dossier (0 : Vérifier).
    Step(usize),
}

/// Raccourci : modificateur « commande » (Ctrl ou Cmd), Maj, et touche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub command: bool,
    pub shift: bool,
    pub key: Key,
}

impl Shortcut {
    const fn cmd(key: Key) -> Self {
        Self {
            command: true,
            shift: false,
            key,
        }
    }

    const fn cmd_shift(key: Key) -> Self {
        Self {
            command: true,
            shift: true,
            key,
        }
    }

    const fn plain(key: Key) -> Self {
        Self {
            command: false,
            shift: false,
            key,
        }
    }

    pub fn egui(self) -> KeyboardShortcut {
        let mut m = if self.command {
            Modifiers::COMMAND
        } else {
            Modifiers::NONE
        };
        if self.shift {
            m |= Modifiers::SHIFT;
        }
        KeyboardShortcut::new(m, self.key)
    }

    /// Libellé : « Ctrl+O » (Windows), « Cmd+O » (macOS), « Ctrl+Maj+Z ».
    pub fn label(self) -> String {
        self.label_for(cfg!(target_os = "macos"))
    }

    pub fn label_for(self, mac: bool) -> String {
        let mut s = String::new();
        if self.command {
            s.push_str(if mac { "Cmd+" } else { "Ctrl+" });
        }
        if self.shift {
            s.push_str("Maj+");
        }
        s.push_str(key_name(self.key));
        s
    }
}

fn key_name(key: Key) -> &'static str {
    match key {
        Key::Enter => "Entrée",
        Key::Escape => "Échap",
        Key::Space => "Espace",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::F1 => "F1",
        Key::F => "F",
        Key::K => "K",
        Key::O => "O",
        Key::V => "V",
        Key::Z => "Z",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        _ => "?",
    }
}

pub const OPEN: Shortcut = Shortcut::cmd(Key::O);
pub const PALETTE: Shortcut = Shortcut::cmd(Key::K);
pub const NEXT: Shortcut = Shortcut::cmd(Key::Enter);
pub const BACK: Shortcut = Shortcut::plain(Key::Escape);
pub const UNDO: Shortcut = Shortcut::cmd(Key::Z);
pub const REDO: Shortcut = Shortcut::cmd_shift(Key::Z);
/// Coller un texte : il devient un document du dossier (événement « coller »
/// du système, lu par l'application ; affiché seulement).
pub const PASTE_TEXT: Shortcut = Shortcut::cmd(Key::V);
pub const PASTE_RESPONSE: Shortcut = Shortcut::cmd_shift(Key::V);
pub const FIND: Shortcut = Shortcut::cmd(Key::F);
pub const HELP: Shortcut = Shortcut::plain(Key::F1);
/// Étapes du dossier : Vérifier, Bilan, Envoyer, Réponse.
pub const STEPS: [Shortcut; 4] = [
    Shortcut::cmd(Key::Num1),
    Shortcut::cmd(Key::Num2),
    Shortcut::cmd(Key::Num3),
    Shortcut::cmd(Key::Num4),
];

/// Liste affichée dans le panneau « Raccourcis » : (touches, action).
pub fn help_rows() -> Vec<(String, &'static str)> {
    vec![
        (PALETTE.label(), "Palette de commandes : toutes les actions"),
        (OPEN.label(), "Ajouter des fichiers au dossier de travail"),
        (
            PASTE_TEXT.label(),
            "Coller un texte : il devient un document du dossier",
        ),
        (NEXT.label(), "Étape suivante"),
        (
            BACK.label(),
            "Annuler le traitement en cours, sinon revenir",
        ),
        (
            format!("{}, {}", UNDO.label(), REDO.label()),
            "Annuler, rétablir une décision de vérification",
        ),
        (
            format!("{} à {}", STEPS[0].label(), STEPS[3].label()),
            "Aller à l'étape Vérifier, Bilan, Envoyer ou Réponse",
        ),
        (
            PASTE_RESPONSE.label(),
            "Coller la réponse de l'IA et la restaurer",
        ),
        (FIND.label(), "Rechercher dans le tableau de vérification"),
        (
            "↑ ↓, Espace".to_owned(),
            "Parcourir le tableau, cocher ou décocher la valeur",
        ),
        (HELP.label(), "Afficher ces raccourcis"),
    ]
}

/// Texte d'infobulle avec son raccourci : « Continuer (Ctrl+Entrée) ».
pub fn with_shortcut(text: &str, s: Shortcut) -> String {
    format!("{text} ({})", s.label())
}

/// Lit (et consomme) les raccourcis de cette image.
///
/// - Les raccourcis avec Maj sont lus avant leur forme simple : egui accepte
///   Ctrl+Maj+Z pour Ctrl+Z.
/// - Annuler et rétablir ne sont pas pris quand un champ de saisie a le focus :
///   le champ garde son propre historique de saisie.
/// - Échap n'est pris que si aucun champ n'a le focus et qu'aucun menu n'est
///   ouvert : sinon il ferme le menu ou quitte le champ, comme d'habitude.
pub fn read(ctx: &egui::Context) -> Vec<Command> {
    let mut out = Vec::new();
    let popup_open = egui::Popup::is_any_open(ctx);
    let focused = ctx.memory(|m| m.focused().is_some());
    let typing = ctx.text_edit_focused();
    ctx.input_mut(|i| {
        if i.consume_shortcut(&PALETTE.egui()) {
            out.push(Command::Palette);
        }
        if i.consume_shortcut(&OPEN.egui()) {
            out.push(Command::Open);
        }
        if i.consume_shortcut(&NEXT.egui()) {
            out.push(Command::Next);
        }
        if i.consume_shortcut(&PASTE_RESPONSE.egui()) {
            out.push(Command::PasteResponse);
        }
        if !typing {
            if i.consume_shortcut(&REDO.egui()) {
                out.push(Command::Redo);
            }
            if i.consume_shortcut(&UNDO.egui()) {
                out.push(Command::Undo);
            }
        }
        if i.consume_shortcut(&FIND.egui()) {
            out.push(Command::Find);
        }
        for (n, s) in STEPS.iter().enumerate() {
            if i.consume_shortcut(&s.egui()) {
                out.push(Command::Step(n));
            }
        }
        if i.consume_shortcut(&HELP.egui()) {
            out.push(Command::Help);
        }
        if !popup_open && !focused && i.consume_shortcut(&BACK.egui()) {
            out.push(Command::Back);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_platform() {
        assert_eq!(OPEN.label_for(false), "Ctrl+O");
        assert_eq!(OPEN.label_for(true), "Cmd+O");
        assert_eq!(NEXT.label_for(false), "Ctrl+Entrée");
        assert_eq!(BACK.label_for(true), "Échap");
        assert_eq!(STEPS[1].label_for(false), "Ctrl+2");
        assert_eq!(HELP.label_for(false), "F1");
        assert_eq!(PALETTE.label_for(false), "Ctrl+K");
        assert_eq!(REDO.label_for(false), "Ctrl+Maj+Z");
        assert_eq!(PASTE_RESPONSE.label_for(true), "Cmd+Maj+V");
        assert_eq!(
            with_shortcut("Continuer", NEXT),
            format!("Continuer ({})", NEXT.label())
        );
    }

    #[test]
    fn help_lists_every_shortcut() {
        let rows = help_rows();
        assert_eq!(rows.len(), 11);
        assert!(rows.iter().all(|(k, a)| !k.is_empty() && !a.is_empty()));
        assert!(
            rows.iter().all(|(k, _)| !k.contains('?')),
            "touche sans nom"
        );
        for s in [PALETTE, UNDO, REDO, PASTE_RESPONSE, NEXT, HELP, OPEN] {
            assert!(
                rows.iter().any(|(k, _)| k.contains(&s.label())),
                "{} absent du panneau",
                s.label()
            );
        }
    }

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn read_events(events: Vec<egui::Event>) -> Vec<Command> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut got = Vec::new();
        let mut output = ctx.run_ui(input, |ui| got = read(ui.ctx()));
        // En debug, epaint refuse qu'on abandonne des textures non appliquées (atlas de police).
        output.textures_delta.clear();
        got
    }

    #[test]
    fn shortcuts_are_consumed() {
        let got = read_events(vec![
            key(Key::O, Modifiers::COMMAND),
            key(Key::Escape, Modifiers::NONE),
        ]);
        assert_eq!(got, vec![Command::Open, Command::Back]);
    }

    #[test]
    fn shifted_shortcuts_are_not_taken_for_plain_ones() {
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(read_events(vec![key(Key::Z, shift)]), vec![Command::Redo]);
        assert_eq!(
            read_events(vec![key(Key::Z, Modifiers::COMMAND)]),
            vec![Command::Undo]
        );
        assert_eq!(
            read_events(vec![key(Key::V, shift)]),
            vec![Command::PasteResponse]
        );
        assert_eq!(
            read_events(vec![
                key(Key::K, Modifiers::COMMAND),
                key(Key::Num4, Modifiers::COMMAND)
            ]),
            vec![Command::Palette, Command::Step(3)]
        );
    }
}
