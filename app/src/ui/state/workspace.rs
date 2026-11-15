//! Dossier de travail : accueil ou dossier, étape courante, objectif et
//! destinataire, et les règles de navigation entre les étapes. Sans egui.
//!
//! Une seule boucle : Vérifier, Bilan, Envoyer, Réponse. Une étape
//! inaccessible le dit en une phrase (`blocker`), affichée en infobulle et
//! dans la palette de commandes.

use anon_core::{Goal, Recipient};

/// Écran principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Home,
    Folder,
}

/// Étapes du dossier de travail, dans l'ordre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Verify,
    Bilan,
    Send,
    Response,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::Verify, Step::Bilan, Step::Send, Step::Response];

    pub fn label(self) -> &'static str {
        match self {
            Step::Verify => "Vérifier",
            Step::Bilan => "Bilan",
            Step::Send => "Envoyer",
            Step::Response => "Réponse",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Step::Verify => 0,
            Step::Bilan => 1,
            Step::Send => 2,
            Step::Response => 3,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn next(self) -> Option<Self> {
        Self::from_index(self.index() + 1)
    }

    pub fn prev(self) -> Option<Self> {
        self.index().checked_sub(1).and_then(Self::from_index)
    }
}

/// Ce que l'interface sait du dossier pour décider de la navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderFacts {
    /// Documents du dossier (fichiers et textes collés).
    pub documents: usize,
    /// Analyse disponible.
    pub analyzed: bool,
    /// Valeurs cochées (masquées).
    pub selected: usize,
    /// Fichiers enregistrés à l'étape Envoyer.
    pub written: bool,
    /// Une clé de restauration est en mémoire ou prête à ouvrir.
    pub key: bool,
    /// Un traitement est en cours.
    pub busy: bool,
}

/// Raison pour laquelle une étape n'est pas accessible.
pub const BUSY: &str = "Patientez : un traitement est en cours.";
pub const NO_DOCUMENT: &str = "Ajoutez d'abord des documents au dossier.";
pub const NOTHING_SELECTED: &str = "Cochez au moins une valeur à masquer.";
pub const NO_KEY_ANONYMIZE: &str =
    "Aucune clé avec l'objectif « Anonymiser définitivement » : aucune réponse à restaurer.";
pub const NO_KEY_YET: &str =
    "Enregistrez d'abord les documents à l'étape Envoyer : la clé de restauration y est créée.";

/// Ce qui empêche d'aller à `step`, en une phrase ; `None` si l'étape est accessible.
pub fn blocker(step: Step, goal: Goal, f: &FolderFacts) -> Option<&'static str> {
    if f.busy {
        return Some(BUSY);
    }
    match step {
        Step::Verify => (f.documents == 0 && !f.analyzed).then_some(NO_DOCUMENT),
        Step::Bilan | Step::Send => {
            if !f.analyzed {
                Some(NO_DOCUMENT)
            } else if f.selected == 0 {
                Some(NOTHING_SELECTED)
            } else {
                None
            }
        }
        Step::Response => {
            if f.key {
                None
            } else if goal == Goal::Anonymize {
                Some(NO_KEY_ANONYMIZE)
            } else {
                Some(NO_KEY_YET)
            }
        }
    }
}

/// Action principale d'une étape (bouton de la barre du bas, Ctrl+Entrée).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primary {
    /// Passer à l'étape indiquée.
    GoTo(Step),
    /// Masquer et enregistrer les documents (étape Envoyer, avant écriture).
    Write,
    /// Ouvrir le dossier des fichiers produits (objectif sans clé, après écriture).
    OpenOutput,
    /// Copier la réponse restaurée (dernière étape).
    CopyRestored,
}

/// Action principale de `step`.
pub fn primary(step: Step, goal: Goal, f: &FolderFacts) -> Primary {
    match step {
        Step::Verify => Primary::GoTo(Step::Bilan),
        Step::Bilan => Primary::GoTo(Step::Send),
        Step::Send if !f.written => Primary::Write,
        Step::Send if goal == Goal::Anonymize && !f.key => Primary::OpenOutput,
        Step::Send => Primary::GoTo(Step::Response),
        Step::Response => Primary::CopyRestored,
    }
}

/// Écran, étape, objectif et destinataire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Workspace {
    pub screen: Screen,
    pub step: Step,
    pub goal: Goal,
    pub recipient: Recipient,
}

impl Workspace {
    /// Va à `step` si elle est accessible ; sinon renvoie la raison.
    pub fn go(&mut self, step: Step, f: &FolderFacts) -> Result<(), &'static str> {
        match blocker(step, self.goal, f) {
            Some(why) => Err(why),
            None => {
                self.screen = Screen::Folder;
                self.step = step;
                Ok(())
            }
        }
    }

    /// Ouvre le dossier à l'étape `step`, sans condition (dépôt de fichiers,
    /// clé ouverte depuis l'accueil).
    pub fn enter(&mut self, step: Step) {
        self.screen = Screen::Folder;
        self.step = step;
    }

    pub fn home(&mut self) {
        self.screen = Screen::Home;
    }

    /// Étape précédente accessible (Échap) ; sinon, retour à l'accueil.
    pub fn back(&mut self, f: &FolderFacts) {
        if self.screen == Screen::Home || f.busy {
            return;
        }
        let mut step = self.step;
        while let Some(prev) = step.prev() {
            if blocker(prev, self.goal, f).is_none() {
                self.step = prev;
                return;
            }
            step = prev;
        }
        self.screen = Screen::Home;
    }

    /// Étape de la boucle : faite, en cours, à venir.
    pub fn status(&self, step: Step) -> StepStatus {
        if self.screen == Screen::Folder && step == self.step {
            StepStatus::Current
        } else if self.screen == Screen::Folder && step.index() < self.step.index() {
            StepStatus::Done
        } else {
            StepStatus::Todo
        }
    }
}

/// État d'une étape dans l'indicateur de l'en-tête.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    Current,
    Todo,
}

/// Libellé court de l'objectif : conséquence écrite sous chaque choix.
pub fn goal_summary(goal: Goal) -> &'static str {
    match goal {
        Goal::Mask => {
            "Vous récupérez la réponse de l'IA avec les vraies valeurs, grâce à une clé de restauration."
        }
        Goal::Anonymize => {
            "Aucun retour possible : les valeurs masquées ne pourront pas être restaurées."
        }
    }
}

/// Conséquence pour la clé de restauration.
pub fn goal_key_note(goal: Goal) -> &'static str {
    match goal {
        Goal::Mask => "Clé de restauration créée",
        Goal::Anonymize => "Aucune clé créée",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyzed() -> FolderFacts {
        FolderFacts {
            documents: 4,
            analyzed: true,
            selected: 12,
            ..Default::default()
        }
    }

    #[test]
    fn steps_in_order() {
        assert_eq!(Step::Verify.next(), Some(Step::Bilan));
        assert_eq!(Step::Response.next(), None);
        assert_eq!(Step::Verify.prev(), None);
        assert_eq!(Step::Response.prev(), Some(Step::Send));
        for s in Step::ALL {
            assert_eq!(Step::from_index(s.index()), Some(s));
        }
        assert_eq!(Step::from_index(4), None);
        let labels: Vec<&str> = Step::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["Vérifier", "Bilan", "Envoyer", "Réponse"]);
    }

    #[test]
    fn blocked_steps_say_why() {
        let empty = FolderFacts::default();
        assert_eq!(blocker(Step::Verify, Goal::Mask, &empty), Some(NO_DOCUMENT));
        assert_eq!(blocker(Step::Bilan, Goal::Mask, &empty), Some(NO_DOCUMENT));
        let mut f = analyzed();
        assert_eq!(blocker(Step::Verify, Goal::Mask, &f), None);
        assert_eq!(blocker(Step::Bilan, Goal::Mask, &f), None);
        assert_eq!(blocker(Step::Send, Goal::Mask, &f), None);
        assert_eq!(blocker(Step::Response, Goal::Mask, &f), Some(NO_KEY_YET));
        assert_eq!(
            blocker(Step::Response, Goal::Anonymize, &f),
            Some(NO_KEY_ANONYMIZE)
        );
        f.selected = 0;
        assert_eq!(blocker(Step::Send, Goal::Mask, &f), Some(NOTHING_SELECTED));
        f.key = true;
        assert_eq!(blocker(Step::Response, Goal::Mask, &f), None);
        f.busy = true;
        assert_eq!(blocker(Step::Verify, Goal::Mask, &f), Some(BUSY));
    }

    #[test]
    fn a_key_alone_opens_the_response_step() {
        // Clé ouverte depuis l'accueil, sans document.
        let f = FolderFacts {
            key: true,
            ..Default::default()
        };
        let mut w = Workspace::default();
        assert_eq!(w.go(Step::Verify, &f), Err(NO_DOCUMENT));
        assert_eq!(w.screen, Screen::Home);
        assert_eq!(w.go(Step::Response, &f), Ok(()));
        assert_eq!((w.screen, w.step), (Screen::Folder, Step::Response));
        // Échap saute les étapes inaccessibles : retour à l'accueil.
        w.back(&f);
        assert_eq!(w.screen, Screen::Home);
    }

    #[test]
    fn navigation_and_back() {
        let f = analyzed();
        let mut w = Workspace::default();
        w.enter(Step::Verify);
        assert_eq!(w.status(Step::Verify), StepStatus::Current);
        assert_eq!(w.go(Step::Send, &f), Ok(()));
        assert_eq!(w.status(Step::Verify), StepStatus::Done);
        assert_eq!(w.status(Step::Response), StepStatus::Todo);
        w.back(&f);
        assert_eq!(w.step, Step::Bilan);
        w.back(&f);
        w.back(&f);
        assert_eq!(w.screen, Screen::Home, "Échap à Vérifier : accueil");
        assert_eq!(w.status(Step::Verify), StepStatus::Todo);
        // Pendant un traitement, on ne quitte pas l'étape.
        w.enter(Step::Send);
        w.back(&FolderFacts { busy: true, ..f });
        assert_eq!(w.step, Step::Send);
    }

    #[test]
    fn primary_action_per_step() {
        let mut f = analyzed();
        assert_eq!(
            primary(Step::Verify, Goal::Mask, &f),
            Primary::GoTo(Step::Bilan)
        );
        assert_eq!(
            primary(Step::Bilan, Goal::Mask, &f),
            Primary::GoTo(Step::Send)
        );
        assert_eq!(primary(Step::Send, Goal::Mask, &f), Primary::Write);
        f.written = true;
        f.key = true;
        assert_eq!(
            primary(Step::Send, Goal::Mask, &f),
            Primary::GoTo(Step::Response)
        );
        f.key = false;
        assert_eq!(
            primary(Step::Send, Goal::Anonymize, &f),
            Primary::OpenOutput
        );
        assert_eq!(
            primary(Step::Response, Goal::Mask, &f),
            Primary::CopyRestored
        );
    }

    #[test]
    fn goal_texts_never_claim_anonymity() {
        for g in Goal::ALL {
            for t in [goal_summary(*g), goal_key_note(*g), g.label()] {
                assert!(!t.to_lowercase().contains("anonyme"), "{t}");
            }
        }
    }
}
