//! Palette de commandes (Ctrl+K) : toutes les actions de l'application,
//! trouvées par une recherche tolérante (sans accents ni majuscules, mots
//! dans n'importe quel ordre), choisies aux flèches, lancées par Entrée.
//! Sans egui : la liste, le filtre et la sélection se testent seuls.

use super::anonymize::fold;
use super::express;
use super::workspace::{FolderFacts, Screen, Step, blocker};
use crate::ui::hotkeys;
use crate::ui::shortcuts::{self, Shortcut};
use crate::ui::theme::ThemeChoice;
use anon_core::{Goal, Recipient};

/// Action proposée par la palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    GoTo(Step),
    Next,
    Home,
    AddFiles,
    PasteText,
    OpenKey,
    Examples,
    SetGoal(Goal),
    SetRecipient(Recipient),
    Theme(ThemeChoice),
    Undo,
    Redo,
    PasteResponse,
    CopyInstruction,
    Find,
    Shortcuts,
    NewSession,
    /// Bilan : appliquer toutes les corrections proposées.
    FixAll,
    /// Presse-papiers express : masquer ou restaurer le texte copié.
    Clipboard(express::Action),
    /// Activer (`true`) ou désactiver les raccourcis globaux du presse-papiers.
    GlobalHotkeys(bool),
}

/// Une ligne de la palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub action: Action,
    pub label: String,
    /// Mots supplémentaires pour la recherche (synonymes), jamais affichés.
    pub keywords: &'static str,
    /// Raccourci affiché à droite.
    pub shortcut: Option<String>,
    /// Action impossible pour l'instant : la raison, affichée sous le libellé.
    pub blocker: Option<&'static str>,
}

impl Entry {
    fn new(action: Action, label: impl Into<String>, keywords: &'static str) -> Self {
        Self {
            action,
            label: label.into(),
            keywords,
            shortcut: None,
            blocker: None,
        }
    }

    fn key(mut self, s: Shortcut) -> Self {
        self.shortcut = Some(s.label());
        self
    }

    fn blocked(mut self, why: Option<&'static str>) -> Self {
        self.blocker = why;
        self
    }

    pub fn enabled(&self) -> bool {
        self.blocker.is_none()
    }
}

/// État de l'application utile à la palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    pub screen: Screen,
    pub step: Step,
    pub goal: Goal,
    pub recipient: Recipient,
    pub theme: ThemeChoice,
    pub facts: FolderFacts,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Ce qui empêche « Tout corriger » ; `None` : des corrections sont proposées.
    pub fix_all: Option<&'static str>,
    /// Raccourcis globaux du presse-papiers activés (préférence).
    pub global_hotkeys: bool,
    /// Une clé est ouverte : le presse-papiers peut être restauré.
    pub key_open: bool,
}

const NOTHING_TO_UNDO: &str = "Aucune décision à annuler.";
const NOTHING_TO_REDO: &str = "Aucune décision à rétablir.";
const FIND_ONLY_IN_VERIFY: &str = "La recherche porte sur le tableau de l'étape Vérifier.";
const ALREADY_HOME: &str = "Vous êtes déjà à l'accueil.";
/// Raison : le bilan ne propose aucune correction.
pub const NOTHING_TO_FIX: &str = "Le bilan ne propose aucune correction.";
/// Raison : aucune clé ouverte pour restaurer le presse-papiers.
pub const NO_OPEN_KEY: &str = "Aucune clé ouverte : masquez d'abord un texte copié.";

/// Toutes les actions, dans l'ordre d'affichage, avec leur état.
pub fn entries(c: &Context) -> Vec<Entry> {
    let f = &c.facts;
    let busy = f.busy.then_some(super::workspace::BUSY);
    let mut v = Vec::new();
    for (i, step) in Step::ALL.iter().enumerate() {
        let mut e = Entry::new(
            Action::GoTo(*step),
            format!("Aller à l'étape {}", step.label()),
            "étape aller dossier",
        )
        .blocked(blocker(*step, c.goal, f));
        if let Some(s) = shortcuts::STEPS.get(i) {
            e = e.key(*s);
        }
        v.push(e);
    }
    let next = match c.screen {
        Screen::Folder => c
            .step
            .next()
            .map_or(Some("Réponse est la dernière étape."), |s| {
                blocker(s, c.goal, f)
            }),
        Screen::Home => {
            Some("Ouvrez d'abord un dossier : déposez des fichiers ou collez un texte.")
        }
    };
    v.push(
        Entry::new(Action::Next, "Étape suivante", "continuer suivant avancer")
            .key(shortcuts::NEXT)
            .blocked(next),
    );
    v.push(
        Entry::new(
            Action::Home,
            "Revenir à l'accueil",
            "accueil début objectif",
        )
        .blocked((c.screen == Screen::Home).then_some(ALREADY_HOME).or(busy)),
    );
    v.push(
        Entry::new(
            Action::AddFiles,
            "Ajouter des fichiers",
            "ouvrir déposer documents choisir importer",
        )
        .key(shortcuts::OPEN)
        .blocked(busy),
    );
    v.push(
        Entry::new(
            Action::PasteText,
            "Coller un texte comme document",
            "presse-papiers texte libre coller",
        )
        .key(shortcuts::PASTE_TEXT)
        .blocked(busy),
    );
    v.push(
        Entry::new(
            Action::OpenKey,
            "Ouvrir une clé de restauration",
            "anonmap restaurer réponse table correspondance",
        )
        .blocked(busy),
    );
    v.push(
        Entry::new(
            Action::Examples,
            "Essayer avec des exemples",
            "démonstration fictifs essai",
        )
        .blocked(busy),
    );
    for g in Goal::ALL {
        let same = (c.goal == *g).then_some("C'est déjà l'objectif choisi.");
        v.push(
            Entry::new(
                Action::SetGoal(*g),
                format!("Objectif : {}", g.label()),
                "objectif but mode réversible irréversible",
            )
            .blocked(same.or(busy)),
        );
    }
    for r in Recipient::ALL {
        let same = (c.recipient == *r).then_some("C'est déjà le destinataire choisi.");
        v.push(
            Entry::new(
                Action::SetRecipient(*r),
                format!("Destinataire : {}", r.label()),
                "destinataire pour envoi partage",
            )
            .blocked(same),
        );
    }
    for t in ThemeChoice::ALL {
        let same = (c.theme == t).then_some("C'est déjà le thème choisi.");
        v.push(
            Entry::new(
                Action::Theme(t),
                format!("Thème : {}", t.label().to_lowercase()),
                "thème apparence mode couleurs sombre clair",
            )
            .blocked(same),
        );
    }
    v.push(
        Entry::new(
            Action::Undo,
            "Annuler la dernière décision",
            "annuler défaire retour",
        )
        .key(shortcuts::UNDO)
        .blocked((!c.can_undo).then_some(NOTHING_TO_UNDO).or(busy)),
    );
    v.push(
        Entry::new(
            Action::Redo,
            "Rétablir la décision annulée",
            "rétablir refaire",
        )
        .key(shortcuts::REDO)
        .blocked((!c.can_redo).then_some(NOTHING_TO_REDO).or(busy)),
    );
    v.push(
        Entry::new(
            Action::FixAll,
            "Tout corriger",
            "bilan corrections corriger évaluation reconnaissable",
        )
        .blocked(busy.or(c.fix_all)),
    );
    v.push(
        Entry::new(
            Action::PasteResponse,
            "Coller la réponse de l'IA et la restaurer",
            "réponse restaurer coller presse-papiers",
        )
        .key(shortcuts::PASTE_RESPONSE)
        .blocked(blocker(Step::Response, c.goal, f)),
    );
    v.push(Entry::new(
        Action::CopyInstruction,
        "Copier la consigne pour l'IA",
        "consigne prompt instructions balises",
    ));
    let find = (c.screen != Screen::Folder || c.step != Step::Verify || !f.analyzed)
        .then_some(FIND_ONLY_IN_VERIFY);
    v.push(
        Entry::new(
            Action::Find,
            "Rechercher dans le tableau",
            "chercher filtrer valeur",
        )
        .key(shortcuts::FIND)
        .blocked(find),
    );
    v.push(
        Entry::new(
            Action::Shortcuts,
            "Afficher les raccourcis clavier",
            "aide clavier touches",
        )
        .key(shortcuts::HELP),
    );
    v.push(
        Entry::new(
            Action::NewSession,
            "Nouvelle session",
            "recommencer vider oublier dossier clé mémoire",
        )
        .blocked(busy),
    );
    for a in express::Action::ALL {
        let mut e = Entry::new(
            Action::Clipboard(a),
            a.label(),
            "presse-papiers express texte copié coller global",
        );
        if c.global_hotkeys {
            e.shortcut = Some(hotkeys::shortcut(a).label());
        }
        let why = match a {
            express::Action::Mask => busy,
            express::Action::Restore => (!c.key_open).then_some(NO_OPEN_KEY),
        };
        v.push(e.blocked(why));
    }
    let toggle = if c.global_hotkeys {
        "Désactiver les raccourcis globaux du presse-papiers"
    } else {
        "Activer les raccourcis globaux du presse-papiers"
    };
    v.push(Entry::new(
        Action::GlobalHotkeys(!c.global_hotkeys),
        toggle,
        "raccourcis globaux presse-papiers clavier réglage préférence ctrl alt",
    ));
    v
}

/// Vrai si chaque mot de `query` se trouve dans le libellé ou les mots-clés,
/// sans tenir compte des accents ni des majuscules.
pub fn matches(query: &str, label: &str, keywords: &str) -> bool {
    let hay = fold(&format!("{label} {keywords}"));
    fold(query)
        .split(|c: char| c.is_whitespace() || c == '\'' || c == '’')
        .filter(|w| !w.is_empty())
        .all(|w| hay.contains(w))
}

/// Indices des lignes qui correspondent à `query` : celles dont le libellé
/// contient la recherche d'abord, puis celles trouvées par les mots-clés.
pub fn filter(list: &[Entry], query: &str) -> Vec<usize> {
    let q = fold(query.trim());
    let mut in_label = Vec::new();
    let mut by_keyword = Vec::new();
    for (i, e) in list.iter().enumerate() {
        if !matches(&q, &e.label, e.keywords) {
            continue;
        }
        if matches(&q, &e.label, "") {
            in_label.push(i);
        } else {
            by_keyword.push(i);
        }
    }
    in_label.extend(by_keyword);
    in_label
}

/// Palette ouverte ou fermée, recherche, ligne choisie.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    /// Position dans la liste filtrée.
    pub selected: usize,
}

impl PaletteState {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// La recherche a changé : retour à la première ligne.
    pub fn query_changed(&mut self) {
        self.selected = 0;
    }

    /// Flèches : ligne suivante ou précédente, en boucle sur `len` lignes.
    pub fn move_by(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        let cur = self.selected.min(len - 1) as isize;
        let len_i = len as isize;
        self.selected = (cur + delta).rem_euclid(len_i) as usize;
    }

    /// Action de la ligne choisie, si elle est possible.
    pub fn chosen(&self, list: &[Entry]) -> Option<Result<Action, &'static str>> {
        let shown = filter(list, &self.query);
        let i = shown.get(self.selected.min(shown.len().saturating_sub(1)))?;
        let e = list.get(*i)?;
        Some(match e.blocker {
            Some(why) => Err(why),
            None => Ok(e.action),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context {
            screen: Screen::Folder,
            step: Step::Verify,
            goal: Goal::Mask,
            recipient: Recipient::PublicAi,
            theme: ThemeChoice::System,
            facts: FolderFacts {
                documents: 4,
                analyzed: true,
                selected: 10,
                ..Default::default()
            },
            can_undo: true,
            can_redo: false,
            fix_all: Some(NOTHING_TO_FIX),
            global_hotkeys: true,
            key_open: false,
        }
    }

    fn labels(list: &[Entry], query: &str) -> Vec<String> {
        filter(list, query)
            .into_iter()
            .map(|i| list[i].label.clone())
            .collect()
    }

    #[test]
    fn search_ignores_accents_case_and_word_order() {
        let list = entries(&ctx());
        assert_eq!(labels(&list, "REPONSE")[0], "Aller à l'étape Réponse");
        assert_eq!(labels(&list, "etape bilan"), ["Aller à l'étape Bilan"]);
        assert_eq!(labels(&list, "bilan étape"), ["Aller à l'étape Bilan"]);
        // Le libellé d'abord, puis les lignes trouvées par leurs mots-clés.
        assert_eq!(labels(&list, "sombre")[0], "Thème : sombre");
        assert_eq!(labels(&list, "exemples"), ["Essayer avec des exemples"]);
        // Par un synonyme : « anonmap » trouve la clé.
        assert_eq!(labels(&list, "anonmap"), ["Ouvrir une clé de restauration"]);
        assert!(labels(&list, "xyz introuvable").is_empty());
        // Recherche vide : tout.
        assert_eq!(filter(&list, "").len(), list.len());
    }

    #[test]
    fn label_matches_come_before_keyword_matches() {
        let list = entries(&ctx());
        let found = labels(&list, "restaurer");
        assert_eq!(found[0], "Coller la réponse de l'IA et la restaurer");
        assert!(found.contains(&"Ouvrir une clé de restauration".to_owned()));
    }

    #[test]
    fn every_action_is_listed_with_its_shortcut() {
        let list = entries(&ctx());
        let find = |a: Action| list.iter().find(|e| e.action == a);
        for a in [
            Action::GoTo(Step::Verify),
            Action::GoTo(Step::Bilan),
            Action::GoTo(Step::Send),
            Action::GoTo(Step::Response),
            Action::Next,
            Action::Home,
            Action::AddFiles,
            Action::PasteText,
            Action::OpenKey,
            Action::Examples,
            Action::SetGoal(Goal::Mask),
            Action::SetGoal(Goal::Anonymize),
            Action::SetRecipient(Recipient::Publication),
            Action::Theme(ThemeChoice::Dark),
            Action::Undo,
            Action::Redo,
            Action::PasteResponse,
            Action::CopyInstruction,
            Action::Find,
            Action::Shortcuts,
            Action::NewSession,
            Action::FixAll,
            Action::Clipboard(express::Action::Mask),
            Action::Clipboard(express::Action::Restore),
            Action::GlobalHotkeys(false),
        ] {
            assert!(find(a).is_some(), "{a:?} absente");
        }
        assert_eq!(
            find(Action::Undo).and_then(|e| e.shortcut.clone()),
            Some(shortcuts::UNDO.label())
        );
        assert_eq!(
            find(Action::GoTo(Step::Send)).and_then(|e| e.shortcut.clone()),
            Some(shortcuts::STEPS[2].label())
        );
        assert!(list.iter().all(|e| !e.label.contains("anonyme")));
        assert_eq!(
            find(Action::Clipboard(express::Action::Mask)).and_then(|e| e.shortcut.clone()),
            Some(hotkeys::MASK.label())
        );
    }

    #[test]
    fn clipboard_actions_and_global_hotkeys_setting() {
        let mut c = ctx();
        let list = entries(&c);
        let get = |l: &[Entry], a: Action| l.iter().find(|e| e.action == a).cloned();
        assert_eq!(
            labels(&list, "presse papiers masquer")[0],
            "Masquer le presse-papiers"
        );
        assert_eq!(
            get(&list, Action::Clipboard(express::Action::Restore)).and_then(|e| e.blocker),
            Some(NO_OPEN_KEY)
        );
        assert!(
            get(&list, Action::GlobalHotkeys(false))
                .is_some_and(|e| e.label.starts_with("Désactiver") && e.enabled())
        );
        // Désactivés : l'action reste, sans raccourci affiché ; on peut les réactiver.
        c.global_hotkeys = false;
        c.key_open = true;
        let list = entries(&c);
        let mask = get(&list, Action::Clipboard(express::Action::Mask));
        assert!(mask.is_some_and(|e| e.shortcut.is_none() && e.enabled()));
        assert!(
            get(&list, Action::Clipboard(express::Action::Restore)).is_some_and(|e| e.enabled())
        );
        assert!(
            get(&list, Action::GlobalHotkeys(true)).is_some_and(|e| e.label.starts_with("Activer"))
        );
        assert_eq!(
            labels(&list, "raccourcis globaux")[0],
            "Activer les raccourcis globaux du presse-papiers"
        );
    }

    #[test]
    fn disabled_actions_say_why() {
        let list = entries(&ctx());
        let get = |a: Action| list.iter().find(|e| e.action == a).cloned();
        assert_eq!(
            get(Action::Redo).and_then(|e| e.blocker),
            Some(NOTHING_TO_REDO)
        );
        assert!(get(Action::Undo).is_some_and(|e| e.enabled()));
        assert_eq!(
            get(Action::GoTo(Step::Response)).and_then(|e| e.blocker),
            Some(super::super::workspace::NO_KEY_YET)
        );
        assert!(get(Action::SetGoal(Goal::Mask)).is_some_and(|e| !e.enabled()));
        assert!(get(Action::SetGoal(Goal::Anonymize)).is_some_and(|e| e.enabled()));
    }

    #[test]
    fn arrows_wrap_and_enter_runs_the_chosen_line() {
        let list = entries(&ctx());
        let mut p = PaletteState::default();
        p.open();
        p.query = "étape".into();
        p.query_changed();
        let n = filter(&list, &p.query).len();
        assert!(n >= 4);
        assert_eq!(p.chosen(&list), Some(Ok(Action::GoTo(Step::Verify))));
        p.move_by(1, n);
        assert_eq!(p.chosen(&list), Some(Ok(Action::GoTo(Step::Bilan))));
        p.move_by(-2, n);
        assert_eq!(
            p.selected,
            n - 1,
            "flèche haut sur la première : la dernière"
        );
        p.move_by(1, n);
        assert_eq!(p.selected, 0);
        // Ligne impossible : la raison, pas l'action.
        p.query = "étape réponse".into();
        p.query_changed();
        assert_eq!(
            p.chosen(&list),
            Some(Err(super::super::workspace::NO_KEY_YET))
        );
        p.query = "introuvable".into();
        assert_eq!(p.chosen(&list), None);
        p.toggle();
        assert!(!p.open && p.query.is_empty());
    }
}
