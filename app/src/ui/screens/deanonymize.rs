//! Onglet « Désanonymiser » : table + mot de passe, réponse de l'IA, résultat.

use super::{card_title, file_row, page_title};
use crate::ui::app::Effect;
use crate::ui::mock;
use crate::ui::state::deanonymize::{DeanonymizeState, InputMode, MapStatus, UnlockError};
use crate::ui::theme::{Weight, font, palette, radius};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::nav::segmented;
use crate::ui::widgets::{
    Badge, Banner, Card, DropZone, Icon, PillButton, TextField, caption, muted, subheading,
};
use egui::{Frame, Margin, RichText, Ui};

/// Largeur à partir de laquelle les deux cartes sont côte à côte.
const TWO_COLUMNS: f32 = 760.0;

pub fn show(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    page_title(
        ui,
        "Désanonymiser une réponse",
        "Remettez les vraies valeurs dans le texte ou le fichier renvoyé par l'IA.",
    );
    if ui.available_width() >= TWO_COLUMNS {
        ui.columns(2, |cols| {
            if let [a, b] = cols {
                a.spacing_mut().item_spacing.y = palette().space(4);
                b.spacing_mut().item_spacing.y = palette().space(4);
                map_card(a, st, fx);
                input_card(b, st, fx);
            }
        });
    } else {
        map_card(ui, st, fx);
        input_card(ui, st, fx);
    }
    if let Some(e) = st.restore_error {
        Banner::error(e.message()).show(ui);
    }
    result_section(ui, st, fx);
}

fn map_card(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        card_title(ui, Icon::KeyRound, "1. Table de correspondance");
        match st.map.clone() {
            MapStatus::None => {
                muted(
                    ui,
                    "Fichier .anonmap créé à l'étape Protéger, à côté des fichiers anonymisés.",
                );
                if ui
                    .add(PillButton::secondary("Choisir la table").icon(Icon::FolderOpen))
                    .clicked()
                {
                    fx.push(Effect::PickMap);
                }
                if let Some(e) = st.unlock_error {
                    Banner::error(e.message()).show(ui);
                }
            }
            MapStatus::Locked { name, .. } => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    Icon::Lock.show(ui, p.text_secondary, 18.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&name)
                                .font(font(Weight::Semibold, p.text_sm))
                                .color(p.dark),
                        )
                        .truncate(),
                    );
                });
                let error = st.unlock_error.map(UnlockError::message);
                let r = TextField::password("Mot de passe de la table", &mut st.password)
                    .id_salt("mdp-table")
                    .error(error)
                    .show(ui);
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let go = ui.add(PillButton::primary("Déverrouiller").icon(Icon::LockOpen));
                    if go.clicked() || enter {
                        // Branchement futur : ouverture par anon-vault.
                        let _ = st.unlock(|_, _| Ok((12, "12 novembre 2026".to_owned())));
                    }
                    if ui.add(PillButton::ghost("Changer de table")).clicked() {
                        fx.push(Effect::PickMap);
                    }
                });
            }
            MapStatus::Unlocked {
                name,
                entries,
                created,
            } => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    ui.add(Badge::brand("Déverrouillée"));
                    ui.add(
                        egui::Label::new(
                            RichText::new(&name)
                                .font(font(Weight::Semibold, p.text_sm))
                                .color(p.dark),
                        )
                        .truncate(),
                    );
                });
                muted(
                    ui,
                    &format!("{entries} correspondances, créée le {created}."),
                );
                if ui
                    .add(
                        PillButton::ghost("Refermer la table")
                            .icon(Icon::Lock)
                            .small(),
                    )
                    .clicked()
                {
                    st.lock();
                }
            }
        }
    });
}

fn input_card(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        card_title(ui, Icon::Sparkles, "2. Réponse de l'IA");
        segmented(
            ui,
            "Source",
            &mut st.mode,
            &[
                (InputMode::Paste, "Coller du texte"),
                (InputMode::File, "Fichier"),
            ],
        );
        match st.mode {
            InputMode::Paste => {
                TextField::new("Texte renvoyé par l'IA", &mut st.pasted)
                    .placeholder("Collez ici la réponse qui contient des balises [PERSONNE_1]…")
                    .id_salt("reponse-ia")
                    .multiline(6)
                    .show(ui);
            }
            InputMode::File => match &st.input_file {
                None => {
                    if ui
                        .add(
                            DropZone::new(
                                "Déposez le fichier renvoyé par l'IA",
                                "ou cliquez pour le choisir",
                            )
                            .height(150.0),
                        )
                        .clicked()
                    {
                        fx.push(Effect::PickResponse);
                    }
                }
                Some(f) => {
                    let mut clear = false;
                    file_row(ui, f, |ui| {
                        if icon_button(ui, Icon::X, "Retirer le fichier", p.text_secondary)
                            .clicked()
                        {
                            clear = true;
                        }
                    });
                    if clear {
                        st.input_file = None;
                    }
                }
            },
        }
        let ready = st.is_unlocked() && st.has_input();
        let go = ui.add(
            PillButton::primary("Restaurer les valeurs")
                .icon(Icon::Undo)
                .enabled(ready),
        );
        if !st.is_unlocked() {
            caption(ui, "Déverrouillez d'abord la table de correspondance.");
        }
        if go.clicked() {
            // Branchement futur : restauration par anon-engine.
            let _ = st.restore(mock::restore);
        }
    });
}

fn result_section(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    let Some(r) = &st.result else { return };
    Card::new().accent(p.primary).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        ui.horizontal(|ui| {
            subheading(ui, "Résultat");
            ui.add(Badge::brand(&format!("{} balises restaurées", r.restored)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        PillButton::secondary("Copier le résultat")
                            .icon(Icon::Copy)
                            .small(),
                    )
                    .clicked()
                {
                    fx.push(Effect::Copy(r.text.clone()));
                }
            });
        });
        Frame::new()
            .fill(p.bg_subtle)
            .corner_radius(radius(p.radius_md))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(&r.text)
                            .font(font(Weight::Regular, p.text_sm))
                            .color(p.text),
                    )
                    .wrap()
                    .selectable(true),
                );
            });
        if !r.unknown_tags.is_empty() {
            let text = format!(
                "{} : absentes de la table, l'IA les a peut-être inventées. Vérifiez ces passages.",
                r.unknown_tags.join(", ")
            );
            Banner::warning(&text).title("Balises inconnues").show(ui);
        }
        if !r.missing_tags.is_empty() {
            let text = format!(
                "{} : absentes de la réponse. L'IA les a peut-être supprimées ou réécrites.",
                r.missing_tags.join(", ")
            );
            Banner::info(&text).title("Balises non retrouvées").show(ui);
        }
    });
}
