//! Onglet « Désanonymiser » : table + mot de passe, réponse de l'IA, résultat.

use super::anonymize::busy_card;
use super::{card_title, control_row, file_row, page_title, truncated_text};
use crate::ui::app::Effect;
use crate::ui::runner::Shared;
use crate::ui::service::{file_name, unused_tags};
use crate::ui::state::deanonymize::{Busy, DeanonymizeState, InputMode, MapStatus};
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette, radius};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::field::{area_height_for_rows, mono_job};
use crate::ui::widgets::nav::segmented;
use crate::ui::widgets::{
    Badge, Banner, Card, DropZone, Icon, PillButton, TextField, body, caption, muted, overline,
    subheading,
};
use anon_engine::RestoreReport;
use egui::{Frame, Margin, RichText, Ui};

/// Largeur à partir de laquelle les deux cartes sont côte à côte.
const TWO_COLUMNS: f32 = 760.0;
/// Lignes visibles de la zone où coller la réponse de l'IA.
const PASTE_ROWS: usize = 8;

pub fn show(ui: &mut Ui, st: &mut DeanonymizeState, progress: &Shared, fx: &mut Vec<Effect>) {
    page_title(
        ui,
        "Désanonymiser une réponse",
        "Remettez les vraies valeurs dans le texte ou les fichiers renvoyés par l'IA.",
    );
    match st.busy {
        Some(Busy::Opening) => {
            busy_card(ui, "Ouverture de la table", progress, fx);
            return;
        }
        Some(Busy::Restoring) => {
            busy_card(ui, "Restauration des fichiers", progress, fx);
            return;
        }
        None => {}
    }
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
                if let Some(e) = &st.unlock_error {
                    Banner::error(e).show(ui);
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
                                .color(p.heading),
                        )
                        .truncate(),
                    )
                    .on_hover_text(name.as_str());
                });
                let error = st.unlock_error.as_deref();
                let r = TextField::password("Mot de passe de la table", &mut st.password)
                    .id_salt("mdp-table")
                    .error(error)
                    .show(ui);
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                control_row(ui, CONTROL_MD, |ui| {
                    let go = ui.add(
                        PillButton::primary("Ouvrir la table")
                            .icon(Icon::LockOpen)
                            .enabled(!st.password.is_empty()),
                    );
                    if go.clicked() || enter {
                        fx.push(Effect::Unlock);
                    }
                    if ui.add(PillButton::ghost("Changer de table")).clicked() {
                        fx.push(Effect::PickMap);
                    }
                });
            }
            MapStatus::Unlocked { name, entries, .. } => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = p.space(2);
                    ui.add(Badge::brand("Ouverte"));
                    ui.add(
                        egui::Label::new(
                            RichText::new(&name)
                                .font(font(Weight::Semibold, p.text_sm))
                                .color(p.heading),
                        )
                        .truncate(),
                    )
                    .on_hover_text(name.as_str());
                });
                muted(ui, &format!("{entries} correspondances."));
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
                (InputMode::File, "Fichiers"),
            ],
        );
        match st.mode {
            InputMode::Paste => {
                TextField::new("Texte renvoyé par l'IA", &mut st.pasted)
                    .placeholder("Collez ici la réponse qui contient des balises [PERSONNE_1]…")
                    .id_salt("reponse-ia")
                    .area_height(area_height_for_rows(PASTE_ROWS))
                    .monospace()
                    .show(ui);
            }
            InputMode::File => {
                if ui
                    .add(
                        DropZone::new(
                            "Déposez les fichiers renvoyés par l'IA",
                            "ou cliquez pour les choisir",
                        )
                        .height(130.0),
                    )
                    .clicked()
                {
                    fx.push(Effect::PickResponses);
                }
                let mut remove = None;
                for (i, f) in st.input_files.iter().enumerate() {
                    file_row(ui, f, |ui| {
                        let label = format!("Retirer {}", f.name);
                        if icon_button(ui, Icon::X, &label, p.text_secondary).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    st.remove_input_file(i);
                }
                if !st.input_files.is_empty() {
                    let dir = st
                        .out_dir
                        .as_deref()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    control_row(ui, CONTROL_SM, |ui| {
                        let w = (ui.available_width() - 120.0).max(80.0);
                        truncated_text(
                            ui,
                            w,
                            CONTROL_SM,
                            RichText::new(format!("Enregistrés dans : {dir}"))
                                .font(font(Weight::Regular, p.text_xs))
                                .color(p.text_secondary),
                        );
                        if ui.add(PillButton::ghost("Changer").small()).clicked() {
                            fx.push(Effect::PickRestoreDir);
                        }
                    });
                }
            }
        }
        let ready = st.is_unlocked() && st.has_input();
        let go = ui.add(
            PillButton::primary("Restaurer les valeurs")
                .icon(Icon::Undo)
                .enabled(ready),
        );
        if !st.is_unlocked() {
            caption(ui, "Ouvrez d'abord la table de correspondance.");
        }
        if go.clicked() {
            fx.push(Effect::Restore);
        }
    });
}

fn report_banners(ui: &mut Ui, r: &RestoreReport) {
    if !r.unknown.is_empty() {
        let text = format!(
            "{} : absentes de la table, laissées telles quelles. L'IA les a peut-être inventées : \
vérifiez ces passages.",
            r.unknown.join(", ")
        );
        Banner::warning(&text).title("Balises inconnues").show(ui);
    }
    let unused = unused_tags(r);
    if !unused.is_empty() {
        let text = format!(
            "{} : absentes de la réponse. L'IA les a peut-être supprimées ou réécrites.",
            unused.join(", ")
        );
        Banner::info(&text).title("Balises non retrouvées").show(ui);
    }
}

fn result_section(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    if let Some((text, r)) = &st.text_result {
        Card::new().accent(p.primary).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = p.space(3);
            control_row(ui, CONTROL_SM, |ui| {
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
                        fx.push(Effect::Copy(text.clone()));
                    }
                });
            });
            Frame::new()
                .fill(p.sunken)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(radius(p.radius_md))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let job = mono_job(text, p.text, ui.available_width());
                    ui.add(egui::Label::new(job).wrap().selectable(true));
                });
            report_banners(ui, r);
        });
    }
    if !st.file_results.is_empty() {
        Card::new().accent(p.primary).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = p.space(3);
            subheading(ui, "Fichiers restaurés");
            let mut folder = None;
            for o in &st.file_results {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = p.space(2);
                    overline(ui, &o.name);
                    match &o.result {
                        Ok((path, r)) => {
                            folder = path.parent().map(|d| d.to_path_buf());
                            ui.horizontal_wrapped(|ui| {
                                ui.add(Badge::brand(&format!("{} balises restaurées", r.restored)));
                                body(ui, &format!("Enregistré sous « {} »", file_name(path)));
                            });
                            report_banners(ui, r);
                        }
                        Err(msg) => {
                            Banner::error(msg).show(ui);
                        }
                    }
                });
            }
            if let Some(dir) = folder
                && ui
                    .add(
                        PillButton::secondary("Ouvrir le dossier")
                            .icon(Icon::FolderOpen)
                            .small(),
                    )
                    .clicked()
            {
                fx.push(Effect::OpenFolder(dir));
            }
        });
    }
}
