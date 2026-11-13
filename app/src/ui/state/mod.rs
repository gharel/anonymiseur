//! État de l'interface, séparé du rendu : structures et transitions pures,
//! testables sans egui.

pub mod anonymize;
pub mod deanonymize;
pub mod files;
pub mod freetext;
pub mod generalize;
pub mod images;
pub mod toasts;

use std::path::PathBuf;

/// Onglet principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Anonymize,
    Deanonymize,
    FreeText,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Anonymize, Tab::Deanonymize, Tab::FreeText];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Anonymize => "Anonymiser",
            Tab::Deanonymize => "Désanonymiser",
            Tab::FreeText => "Texte libre",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Tab::Anonymize => 0,
            Tab::Deanonymize => 1,
            Tab::FreeText => 2,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }
}

/// Texte du voile affiché quand des fichiers survolent la fenêtre : ce qui
/// se passera au dépôt. `paths` : chemins connus ; `count` : fichiers survolant.
pub fn drop_hint(
    tab: Tab,
    step: anonymize::Step,
    busy: bool,
    paths: &[PathBuf],
    count: usize,
) -> String {
    use toasts::count_label;
    if busy {
        return "Patientez : un traitement est en cours.".to_owned();
    }
    match tab {
        Tab::Anonymize if step != anonymize::Step::Drop => {
            "Revenez à l'étape Déposer pour ajouter des fichiers.".to_owned()
        }
        Tab::Anonymize => {
            let maps = paths.iter().filter(|p| deanonymize::is_map(p)).count();
            let outputs = paths
                .iter()
                .filter(|p| !deanonymize::is_map(p) && files::is_tool_output(p))
                .count();
            let docs = count.saturating_sub(maps + outputs);
            let mut s = if docs == 0 && maps > 0 {
                "Déposez pour reprendre cette table de correspondance".to_owned()
            } else if docs == 0 {
                "Ces fichiers sont déjà anonymisés : ils seront ignorés".to_owned()
            } else {
                format!(
                    "Déposez pour ajouter {}",
                    count_label(docs, "fichier", "fichiers")
                )
            };
            if docs > 0 && outputs == 1 {
                s.push_str(" ; 1 fichier déjà anonymisé sera ignoré");
            } else if docs > 0 && outputs > 1 {
                s.push_str(&format!(
                    " ; {outputs} fichiers déjà anonymisés seront ignorés"
                ));
            }
            s
        }
        Tab::Deanonymize => {
            "Déposez la table de correspondance ou les fichiers à restaurer".to_owned()
        }
        Tab::FreeText => {
            "Déposez les fichiers dans l'onglet Anonymiser ou Désanonymiser".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_hints_say_what_will_happen() {
        use anonymize::Step;
        let p = |s: &str| PathBuf::from(s);
        let docs = [p("a.docx"), p("b.pdf"), p("c_anonymise.docx")];
        assert_eq!(
            drop_hint(Tab::Anonymize, Step::Drop, false, &docs, 3),
            "Déposez pour ajouter 2 fichiers ; 1 fichier déjà anonymisé sera ignoré"
        );
        assert_eq!(
            drop_hint(Tab::Anonymize, Step::Drop, false, &[p("t.anonmap")], 1),
            "Déposez pour reprendre cette table de correspondance"
        );
        assert_eq!(
            drop_hint(Tab::Anonymize, Step::Drop, false, &[p("x_restaure.txt")], 1),
            "Ces fichiers sont déjà anonymisés : ils seront ignorés"
        );
        assert_eq!(
            drop_hint(Tab::Anonymize, Step::Drop, false, &[], 4),
            "Déposez pour ajouter 4 fichiers",
            "chemins inconnus : tous comptés"
        );
        assert!(drop_hint(Tab::Anonymize, Step::Review, false, &docs, 3).starts_with("Revenez"));
        assert!(drop_hint(Tab::Deanonymize, Step::Drop, false, &docs, 3).contains("table"));
        assert!(drop_hint(Tab::Anonymize, Step::Drop, true, &docs, 3).starts_with("Patientez"));
    }

    #[test]
    fn tab_indices_round_trip() {
        for t in Tab::ALL {
            assert_eq!(Tab::from_index(t.index()), Some(t));
        }
        assert_eq!(Tab::from_index(3), None);
    }
}
