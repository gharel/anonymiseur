//! Raccourcis clavier : la liste, leur libellé (Ctrl sous Windows, Cmd sous
//! macOS) et la lecture des touches au début de chaque image.

use egui::{Key, KeyboardShortcut, Modifiers};

/// Action déclenchée par un raccourci.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Ajouter des fichiers (Anonymiser) ou ouvrir une table (Désanonymiser).
    Open,
    /// Action principale de l'étape.
    Primary,
    /// Annuler le traitement en cours, sinon revenir.
    Back,
    /// Onglet 1, 2 ou 3.
    Tab(usize),
    /// Recherche dans le tableau de vérification.
    Find,
    /// Panneau des raccourcis.
    Help,
}

/// Raccourci : modificateur « commande » (Ctrl ou Cmd) et touche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub command: bool,
    pub key: Key,
}

impl Shortcut {
    const fn cmd(key: Key) -> Self {
        Self { command: true, key }
    }

    const fn plain(key: Key) -> Self {
        Self {
            command: false,
            key,
        }
    }

    pub fn egui(self) -> KeyboardShortcut {
        let m = if self.command {
            Modifiers::COMMAND
        } else {
            Modifiers::NONE
        };
        KeyboardShortcut::new(m, self.key)
    }

    /// Libellé : « Ctrl+O » (Windows), « Cmd+O » (macOS).
    pub fn label(self) -> String {
        self.label_for(cfg!(target_os = "macos"))
    }

    pub fn label_for(self, mac: bool) -> String {
        let key = key_name(self.key);
        if self.command {
            format!("{}+{key}", if mac { "Cmd" } else { "Ctrl" })
        } else {
            key.to_owned()
        }
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
        Key::O => "O",
        Key::F => "F",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        _ => "?",
    }
}

pub const OPEN: Shortcut = Shortcut::cmd(Key::O);
pub const PRIMARY: Shortcut = Shortcut::cmd(Key::Enter);
pub const BACK: Shortcut = Shortcut::plain(Key::Escape);
pub const FIND: Shortcut = Shortcut::cmd(Key::F);
pub const HELP: Shortcut = Shortcut::plain(Key::F1);
pub const TABS: [Shortcut; 3] = [
    Shortcut::cmd(Key::Num1),
    Shortcut::cmd(Key::Num2),
    Shortcut::cmd(Key::Num3),
];

/// Liste affichée dans le panneau « Raccourcis » : (touches, action).
pub fn help_rows() -> Vec<(String, &'static str)> {
    vec![
        (
            OPEN.label(),
            "Ajouter des fichiers, ou ouvrir une table dans Désanonymiser",
        ),
        (PRIMARY.label(), "Action principale de l'étape"),
        (
            BACK.label(),
            "Annuler le traitement en cours, sinon revenir",
        ),
        (
            format!(
                "{}, {}, {}",
                TABS[0].label(),
                TABS[1].label(),
                TABS[2].label()
            ),
            "Anonymiser, Désanonymiser, Texte libre",
        ),
        (FIND.label(), "Rechercher dans le tableau de vérification"),
        (
            "↑ ↓, Espace".to_owned(),
            "Parcourir le tableau, cocher ou décocher la ligne",
        ),
        (HELP.label(), "Afficher ces raccourcis"),
    ]
}

/// Texte d'infobulle avec son raccourci : « Analyser les fichiers (Ctrl+Entrée) ».
pub fn with_shortcut(text: &str, s: Shortcut) -> String {
    format!("{text} ({})", s.label())
}

/// Lit (et consomme) les raccourcis de cette image. Échap n'est pris que
/// si aucun champ n'a le focus et qu'aucun menu n'est ouvert : sinon il ferme
/// le menu ou quitte le champ, comme d'habitude.
pub fn read(ctx: &egui::Context) -> Vec<Command> {
    let mut out = Vec::new();
    let popup_open = egui::Popup::is_any_open(ctx);
    let focused = ctx.memory(|m| m.focused().is_some());
    ctx.input_mut(|i| {
        if i.consume_shortcut(&OPEN.egui()) {
            out.push(Command::Open);
        }
        if i.consume_shortcut(&PRIMARY.egui()) {
            out.push(Command::Primary);
        }
        if i.consume_shortcut(&FIND.egui()) {
            out.push(Command::Find);
        }
        for (n, s) in TABS.iter().enumerate() {
            if i.consume_shortcut(&s.egui()) {
                out.push(Command::Tab(n));
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
        assert_eq!(PRIMARY.label_for(false), "Ctrl+Entrée");
        assert_eq!(BACK.label_for(true), "Échap");
        assert_eq!(TABS[1].label_for(false), "Ctrl+2");
        assert_eq!(HELP.label_for(false), "F1");
        assert_eq!(
            with_shortcut("Analyser", PRIMARY),
            format!("Analyser ({})", PRIMARY.label())
        );
    }

    #[test]
    fn help_lists_every_shortcut() {
        let rows = help_rows();
        assert_eq!(rows.len(), 7);
        assert!(rows.iter().all(|(k, a)| !k.is_empty() && !a.is_empty()));
        assert!(
            rows.iter().all(|(k, _)| !k.contains('?')),
            "touche sans nom"
        );
    }

    #[test]
    fn shortcuts_are_consumed() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: Key::O,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        input.events.push(egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        let mut got = Vec::new();
        let _ = ctx.run_ui(input, |ui| got = read(ui.ctx()));
        assert_eq!(got, vec![Command::Open, Command::Back]);
    }
}
