//! État de l'interface, séparé du rendu : structures et transitions pures,
//! testables sans egui.
//!
//! - `workspace` : accueil ou dossier de travail, étapes et navigation ;
//! - `anonymize` : documents, décisions de vérification, envoi ;
//! - `history` : annuler, rétablir les décisions ;
//! - `review` : vérification dans le document, file « À décider » ;
//! - `bilan` : bilan provisoire avant envoi (remplacé au lot 8) ;
//! - `deanonymize` : clé de restauration et réponse de l'IA ;
//! - `palette` : palette de commandes.

pub mod anonymize;
pub mod bilan;
pub mod deanonymize;
pub mod files;
pub mod generalize;
pub mod history;
pub mod images;
pub mod palette;
pub mod review;
pub mod toasts;
pub mod workspace;

use std::path::PathBuf;
use workspace::{Screen, Step};

/// Texte du voile affiché quand des fichiers survolent la fenêtre : ce qui
/// se passera au dépôt. `paths` : chemins connus ; `count` : fichiers survolant.
pub fn drop_hint(
    screen: Screen,
    step: Step,
    busy: bool,
    paths: &[PathBuf],
    count: usize,
) -> String {
    use toasts::count_label;
    if busy {
        return "Patientez : un traitement est en cours.".to_owned();
    }
    let maps = paths.iter().filter(|p| deanonymize::is_map(p)).count();
    let others = count.saturating_sub(maps);
    if others == 0 && maps > 0 {
        return "Déposez pour ouvrir cette clé de restauration".to_owned();
    }
    if screen == Screen::Folder && step == Step::Response {
        return format!(
            "Déposez pour restaurer {}",
            count_label(
                others,
                "fichier renvoyé par l'IA",
                "fichiers renvoyés par l'IA"
            )
        );
    }
    let outputs = paths
        .iter()
        .filter(|p| !deanonymize::is_map(p) && files::is_tool_output(p))
        .count();
    let docs = others.saturating_sub(outputs);
    if docs == 0 {
        return "Ces fichiers sont déjà masqués : ils seront ignorés".to_owned();
    }
    let mut s = format!(
        "Déposez pour ajouter {} au dossier",
        count_label(docs, "fichier", "fichiers")
    );
    if outputs == 1 {
        s.push_str(" ; 1 fichier déjà masqué sera ignoré");
    } else if outputs > 1 {
        s.push_str(&format!(
            " ; {outputs} fichiers déjà masqués seront ignorés"
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_hints_say_what_will_happen() {
        let p = |s: &str| PathBuf::from(s);
        let docs = [p("a.docx"), p("b.pdf"), p("c_anonymise.docx")];
        assert_eq!(
            drop_hint(Screen::Home, Step::Verify, false, &docs, 3),
            "Déposez pour ajouter 2 fichiers au dossier ; 1 fichier déjà masqué sera ignoré"
        );
        assert_eq!(
            drop_hint(Screen::Home, Step::Verify, false, &[p("t.anonmap")], 1),
            "Déposez pour ouvrir cette clé de restauration"
        );
        assert_eq!(
            drop_hint(
                Screen::Folder,
                Step::Bilan,
                false,
                &[p("x_restaure.txt")],
                1
            ),
            "Ces fichiers sont déjà masqués : ils seront ignorés"
        );
        assert_eq!(
            drop_hint(Screen::Folder, Step::Verify, false, &[], 4),
            "Déposez pour ajouter 4 fichiers au dossier",
            "chemins inconnus : tous comptés"
        );
        assert_eq!(
            drop_hint(Screen::Folder, Step::Response, false, &docs, 3),
            "Déposez pour restaurer 3 fichiers renvoyés par l'IA"
        );
        assert!(drop_hint(Screen::Home, Step::Verify, true, &docs, 3).starts_with("Patientez"));
    }
}
