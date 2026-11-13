//! Galerie des composants dans tous leurs états, pour les instantanés et la
//! relecture visuelle de la charte.

use super::theme::palette;
use super::widgets::badge::FAMILIES;
use super::widgets::field::field_id;
use super::widgets::nav::{filter_chip, segmented};
use super::widgets::{
    Badge, Banner, ButtonSize, ButtonVariant, Card, CheckState, Checkbox, Column, DataTable,
    DropZone, Icon, PillButton, ProgressBar, RowTone, Stepper, StrengthMeter, TabBar, TextField,
    family_tint, overline,
};
use anon_core::{EntityKind, Profile};
use egui::Ui;

/// Valeurs modifiables de la galerie.
#[derive(Debug, Clone)]
pub struct GalleryState {
    pub empty: String,
    pub filled: String,
    pub focused: String,
    pub with_error: String,
    pub password: String,
    pub profile: Profile,
    /// Donne le focus au champ « focus » à la prochaine image.
    pub focus_once: bool,
}

impl Default for GalleryState {
    fn default() -> Self {
        Self {
            empty: String::new(),
            filled: "Claire Martin".into(),
            focused: "claire.martin@example.com".into(),
            with_error: "claire.martin@".into(),
            password: "Cheval-Agrafe".into(),
            profile: Profile::Standard,
            focus_once: true,
        }
    }
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = palette().space(3);
        overline(ui, title);
        add(ui);
    });
}

pub fn show(ui: &mut Ui, st: &mut GalleryState) {
    let p = palette();
    ui.spacing_mut().item_spacing.y = p.space(6);

    section(ui, "Boutons", |ui| {
        for variant in [
            ButtonVariant::Primary,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
            ButtonVariant::Dark,
        ] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = p.space(3);
                for (size, name) in [
                    (ButtonSize::Sm, "Petit"),
                    (ButtonSize::Md, "Moyen"),
                    (ButtonSize::Lg, "Grand"),
                ] {
                    ui.add(PillButton::new(name).variant(variant).size(size));
                }
                ui.add(
                    PillButton::new("Avec icône")
                        .variant(variant)
                        .icon(Icon::Lock),
                );
                ui.add(
                    PillButton::new("Survolé")
                        .variant(variant)
                        .preview_hover(true),
                );
                ui.add(PillButton::new("Désactivé").variant(variant).enabled(false));
            });
        }
    });

    section(ui, "Badges de type", |ui| {
        ui.horizontal_wrapped(|ui| {
            for kind in [
                EntityKind::Personne,
                EntityKind::Societe,
                EntityKind::Lieu,
                EntityKind::Email,
                EntityKind::Nir,
                EntityKind::Iban,
                EntityKind::DateNaiss,
            ] {
                ui.add(Badge::entity(kind));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(Badge::brand("Réussi"));
            ui.add(Badge::warning("Moyen"));
            ui.add(Badge::error("Bloqué"));
            ui.add(Badge::neutral("Neutre"));
            ui.add(Badge::entity(EntityKind::Personne).small());
        });
    });

    section(ui, "Filtres, sélecteur, onglets, étapes", |ui| {
        ui.horizontal_wrapped(|ui| {
            for (i, fam) in FAMILIES.iter().enumerate() {
                filter_chip(
                    ui,
                    super::widgets::badge::family_label(*fam),
                    Some(i + 1),
                    i % 2 == 0,
                    family_tint(*fam),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(6);
            segmented(
                ui,
                "Profil",
                &mut st.profile,
                &[
                    (Profile::Standard, "Standard"),
                    (Profile::Renforce, "Renforcé"),
                ],
            );
            let tabs = [
                (Icon::Lock, "Anonymiser"),
                (Icon::LockOpen, "Désanonymiser"),
                (Icon::FileText, "Texte libre"),
            ];
            TabBar::new(&tabs).show(ui, 0);
        });
        Stepper::new(&["Déposer", "Vérifier", "Protéger", "Terminé"], 1).show(ui);
    });

    section(ui, "Cases à cocher", |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(6);
            ui.add(Checkbox::new(CheckState::Unchecked, "Non cochée"));
            ui.add(Checkbox::new(CheckState::Checked, "Cochée"));
            ui.add(Checkbox::new(CheckState::Mixed, "Partielle"));
            ui.add(Checkbox::new(CheckState::Checked, "Désactivée").enabled(false));
        });
    });

    section(ui, "Champs", |ui| {
        if st.focus_once {
            ui.memory_mut(|m| m.request_focus(field_id("galerie-focus")));
            st.focus_once = false;
        }
        ui.columns(2, |cols| {
            if let [a, b] = cols {
                a.spacing_mut().item_spacing.y = p.space(4);
                b.spacing_mut().item_spacing.y = p.space(4);
                TextField::new("Nom complet", &mut st.empty)
                    .placeholder("ex. Jean Dupont")
                    .required(true)
                    .id_salt("galerie-vide")
                    .show(a);
                TextField::new("Valeur saisie", &mut st.filled)
                    .hint("Texte d'aide sous le champ.")
                    .id_salt("galerie-rempli")
                    .show(a);
                TextField::new("Champ actif", &mut st.focused)
                    .id_salt("galerie-focus")
                    .show(a);
                TextField::new("Email", &mut st.with_error)
                    .id_salt("galerie-erreur")
                    .error(Some("Adresse email incomplète."))
                    .show(b);
                TextField::password("Mot de passe", &mut st.password)
                    .id_salt("galerie-mdp")
                    .show(b);
                b.add(StrengthMeter::of(&st.password));
                for pw in [
                    "",
                    "court",
                    "qmzrtxwpvk",
                    "Qmzrtx-Wpvk7",
                    "Cheval-Agrafe-Batterie-47",
                ] {
                    b.add(StrengthMeter::of(pw));
                }
            }
        });
    });

    section(ui, "Zones de dépôt", |ui| {
        ui.columns(2, |cols| {
            if let [a, b] = cols {
                a.add(DropZone::new("Déposez vos fichiers", "au repos").height(150.0));
                b.add(
                    DropZone::new("Déposez vos fichiers", "survolée")
                        .height(150.0)
                        .force_hovered(true),
                );
            }
        });
    });

    section(ui, "Bandeaux", |ui| {
        Banner::info("Information neutre, texte anthracite.")
            .title("Information")
            .show(ui);
        Banner::success("Opération réussie.").show(ui);
        Banner::warning("Vérifiez ce point avant de continuer.")
            .title("Avertissement")
            .show(ui);
        Banner::error("Le fichier n'a pas été livré.").show(ui);
    });

    section(ui, "Progression", |ui| {
        ui.add(ProgressBar::new(0.0, "En attente"));
        ui.add(ProgressBar::new(0.42, "Analyse des fichiers"));
        ui.add(ProgressBar::new(1.0, "Terminé"));
    });

    section(ui, "Cartes", |ui| {
        ui.columns(3, |cols| {
            if let [a, b, c] = cols {
                Card::new().show(a, |ui| {
                    ui.label("Carte sans barre.");
                });
                Card::new().accent(p.primary).show(b, |ui| {
                    ui.label("Barre verte de marque.");
                });
                Card::new().accent(p.bureautique.edge).show(c, |ui| {
                    ui.label("Barre de catégorie.");
                });
            }
        });
    });

    section(ui, "Icônes", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            for icon in Icon::ALL {
                icon.show(ui, p.text, 20.0);
            }
        });
    });

    section(ui, "Tableau", |ui| {
        let cols = [
            Column::fixed("Type", 180.0),
            Column::fill("Valeur", 1.0),
            Column::fixed("Balise", 140.0),
        ];
        let rows = [
            (
                EntityKind::Personne,
                "Claire Martin",
                "[PERSONNE_1]",
                true,
                false,
            ),
            (
                EntityKind::Email,
                "claire.martin@example.com",
                "[EMAIL_1]",
                true,
                false,
            ),
            (EntityKind::Age, "36 ans", "[AGE_1]", false, true),
        ];
        DataTable::new("galerie-tableau", &cols)
            .checkable(CheckState::Mixed)
            .show(
                ui,
                rows.len(),
                |i| rows.get(i).is_some_and(|r| r.3),
                |i| {
                    if rows.get(i).is_some_and(|r| r.4) {
                        RowTone::Attention
                    } else {
                        RowTone::Normal
                    }
                },
                |i| rows.get(i).map(|r| r.1.to_owned()).unwrap_or_default(),
                |ui, i, col| {
                    let Some(r) = rows.get(i) else { return };
                    match col {
                        0 => {
                            ui.add(Badge::entity(r.0));
                        }
                        1 => {
                            ui.label(r.1);
                        }
                        _ => {
                            ui.label(egui::RichText::new(r.2).monospace().color(p.primary_dark));
                        }
                    }
                },
            );
    });
}
