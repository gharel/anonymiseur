//! Étape « Envoyer » : avant l'écriture, la clé de restauration (mot de passe,
//! ou clé reprise) et le dossier des fichiers ; après l'écriture, les fichiers
//! prêts à envoyer, le texte et la consigne à copier, la clé créée. Reprend les
//! étapes « Protéger » et « Terminé » de la V1 ; le lot 8 les refondra.

use super::{busy_card, card_title, control_row, copy_button, file_tile, truncated_text};
use crate::ui::app::Effect;
use crate::ui::runner::Shared;
use crate::ui::service::{BatchReport, Outcome, file_name};
use crate::ui::state::anonymize::{AnonymizeState, Busy, ProtectMode};
use crate::ui::state::files::FileFormat;
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette};
use crate::ui::widgets::nav::segmented;
use crate::ui::widgets::{
    Badge, Banner, Card, Icon, PillButton, StrengthMeter, TextField, body, caption, muted,
};
use anon_core::{Goal, WarningCode};
use egui::{RichText, Ui};

/// Identifiant du premier champ de mot de passe (focus à l'arrivée sur l'étape).
pub const PASSWORD_FIELD: &str = "mdp";
/// Largeur maximale des champs.
const FIELD_MAX_W: f32 = 420.0;

pub fn show(
    ui: &mut Ui,
    st: &mut AnonymizeState,
    goal: Goal,
    progress: &Shared,
    fx: &mut Vec<Effect>,
) {
    match st.busy {
        Some(Busy::Anonymizing) => {
            busy_card(
                ui,
                Busy::Anonymizing.label(),
                &st.document_names(),
                progress,
                fx,
            );
            return;
        }
        Some(Busy::OpeningKey) => {
            busy_card(ui, Busy::OpeningKey.label(), &[], progress, fx);
            return;
        }
        _ => {}
    }
    match st.report.clone() {
        Some(report) => written(ui, &report, goal, fx),
        None => protect(ui, st, goal, fx),
    }
}

/// Avant l'écriture : clé de restauration et dossier des fichiers.
fn protect(ui: &mut Ui, st: &mut AnonymizeState, goal: Goal, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        match goal {
            Goal::Mask => key_form(ui, st, fx),
            Goal::Anonymize => {
                card_title(ui, Icon::Lock, "Aucune clé de restauration");
                body(
                    ui,
                    "Objectif « Anonymiser définitivement » : aucune clé n'est créée. Les balises \
ne pourront jamais être remplacées par les vraies valeurs.",
                );
            }
        }
    });
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::FolderOpen, "Dossier des fichiers");
        control_row(ui, CONTROL_SM, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            let dir = st
                .out_dir
                .as_deref()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "Aucun dossier choisi".to_owned());
            let w = (ui.available_width() - 120.0).max(120.0);
            truncated_text(
                ui,
                w,
                CONTROL_SM,
                RichText::new(dir)
                    .font(font(Weight::Regular, p.text_sm))
                    .color(p.text),
            );
            if ui
                .add(PillButton::ghost("Changer").small())
                .on_hover_text("Choisir le dossier où enregistrer les fichiers")
                .clicked()
            {
                fx.push(Effect::PickOutputDir);
            }
        });
        caption(
            ui,
            "Les fichiers masqués et la clé y sont enregistrés ; un fichier existant n'est jamais remplacé.",
        );
    });
}

fn key_form(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    card_title(ui, Icon::KeyRound, "Clé de restauration");
    muted(
        ui,
        "La clé relie chaque balise à sa vraie valeur. Elle est chiffrée par ce mot de passe : \
sans lui, la restauration est impossible. Notez-le en lieu sûr.",
    );
    let mut mode = st.protect.mode;
    segmented(
        ui,
        "Clé de restauration",
        &mut mode,
        &[
            (ProtectMode::NewPassword, "Nouvelle clé"),
            (ProtectMode::ExistingTable, "Reprendre une clé"),
        ],
    );
    if mode != st.protect.mode {
        st.protect.mode = mode;
        st.protect.password.clear();
        st.protect.confirm.clear();
    }
    let field_w = ui.available_width().min(FIELD_MAX_W);
    match st.protect.mode {
        ProtectMode::NewPassword | ProtectMode::Irreversible => {
            let issue = st.protect.password_issue().map(|i| i.message());
            TextField::password("Mot de passe", &mut st.protect.password)
                .id_salt(PASSWORD_FIELD)
                .required(true)
                .error(issue)
                .width(field_w)
                .show(ui);
            ui.scope(|ui| {
                ui.set_width(field_w);
                ui.add(StrengthMeter::of(&st.protect.password));
            });
            let confirm_issue = st.protect.confirm_issue().map(|i| i.message());
            TextField::password("Confirmation du mot de passe", &mut st.protect.confirm)
                .id_salt("mdp-confirmation")
                .required(true)
                .error(confirm_issue)
                .width(field_w)
                .show(ui);
        }
        ProtectMode::ExistingTable => {
            muted(
                ui,
                "Les valeurs déjà présentes dans cette clé gardent leurs balises. Une nouvelle \
clé, complétée, est enregistrée avec le même mot de passe.",
            );
            match &st.protect.existing {
                Some(t) => {
                    ui.horizontal(|ui| {
                        ui.add(Badge::brand("Ouverte"));
                        body(
                            ui,
                            &format!(
                                "{} : {}",
                                t.name,
                                count_label(t.entries, "valeur", "valeurs")
                            ),
                        );
                    });
                }
                None => {
                    let name = st
                        .protect
                        .existing_path
                        .as_deref()
                        .map(file_name)
                        .unwrap_or_else(|| "Aucune clé choisie".to_owned());
                    control_row(ui, CONTROL_SM, |ui| {
                        ui.spacing_mut().item_spacing.x = p.space(3);
                        if ui
                            .add(
                                PillButton::secondary("Choisir la clé")
                                    .icon(Icon::FolderOpen)
                                    .small(),
                            )
                            .clicked()
                        {
                            fx.push(Effect::PickExistingMap);
                        }
                        body(ui, &name);
                    });
                    let error = st.protect.existing_error.as_deref();
                    TextField::password("Mot de passe de la clé", &mut st.protect.password)
                        .id_salt("mdp-reprise")
                        .error(error)
                        .width(field_w)
                        .show(ui);
                    let ready =
                        st.protect.existing_path.is_some() && !st.protect.password.is_empty();
                    if ui
                        .add(
                            PillButton::primary("Ouvrir la clé")
                                .icon(Icon::LockOpen)
                                .enabled(ready)
                                .small(),
                        )
                        .clicked()
                    {
                        fx.push(Effect::OpenExistingMap);
                    }
                }
            }
        }
    }
}

/// Après l'écriture : fichiers prêts, deux façons d'envoyer, clé créée.
fn written(ui: &mut Ui, report: &BatchReport, goal: Goal, fx: &mut Vec<Effect>) {
    let p = palette();
    let delivered = report.delivered();
    let total = report.files.len();
    if let Some(fatal) = &report.fatal {
        Banner::error(fatal).title("Aucun fichier écrit").show(ui);
    } else {
        let mut text = if delivered == total {
            let files = if total == 1 {
                "le fichier".to_owned()
            } else {
                format!("les {total} fichiers")
            };
            format!("Vérification finale réussie : aucune valeur masquée ne subsiste dans {files}.")
        } else {
            format!(
                "{} sur {total} : les autres n'ont pas passé la vérification finale ou n'ont pas été traités.",
                count_label(delivered, "fichier prêt", "fichiers prêts")
            )
        };
        if report.cancelled {
            text.push_str(" Traitement annulé : les fichiers restants n'ont pas été traités.");
        }
        let banner = if delivered == total {
            Banner::success(&text)
        } else {
            Banner::warning(&text)
        };
        banner.title("Prêt à envoyer").show(ui);
    }
    if delivered > 0 {
        ui.columns(2, |cols| {
            if let [a, b] = cols {
                way(
                    a,
                    Icon::FolderOpen,
                    "Envoyer les fichiers",
                    "Les fichiers masqués sont enregistrés dans un dossier. Joignez-les à votre \
conversation avec l'IA.",
                    |ui| {
                        if ui
                            .add(PillButton::primary("Ouvrir le dossier des fichiers").small())
                            .clicked()
                        {
                            fx.push(Effect::OpenFolder(report.out_dir.clone()));
                        }
                    },
                );
                way(
                    b,
                    Icon::Clipboard,
                    "Envoyer le texte",
                    "Le texte masqué est copié avec la consigne, prêt à coller dans la conversation.",
                    |ui| {
                        if copy_button(
                            ui,
                            "copier-texte-consigne",
                            PillButton::secondary("Copier le texte et la consigne").small(),
                            true,
                        )
                        .clicked()
                        {
                            fx.push(Effect::CopyTextForAi);
                        }
                    },
                );
            }
        });
    }
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for f in &report.files {
            file_line(ui, &f.name, &f.outcome, fx);
        }
    });
    ui.columns(2, |cols| {
        if let [a, b] = cols {
            Card::new().show(a, |ui| {
                ui.spacing_mut().item_spacing.y = p.space(2);
                card_title(ui, Icon::Sparkles, "Consigne pour l'IA");
                muted(
                    ui,
                    "Les mots entre crochets, comme [PERSONNE_1], remplacent des données \
personnelles. L'IA doit les garder tels quels dans sa réponse.",
                );
                if copy_button(
                    ui,
                    "copier-consigne",
                    PillButton::ghost("Copier la consigne pour l'IA")
                        .icon(Icon::Copy)
                        .small(),
                    true,
                )
                .clicked()
                {
                    fx.push(Effect::Copy {
                        text: anon_engine::ai_instruction().to_owned(),
                        toast: "Consigne copiée",
                    });
                }
            });
            Card::new().show(b, |ui| {
                ui.spacing_mut().item_spacing.y = p.space(2);
                card_title(ui, Icon::KeyRound, "Clé de restauration");
                match (&report.map_path, goal) {
                    (Some(map), _) => {
                        body(ui, &file_name(map));
                        muted(
                            ui,
                            "Chiffrée par votre mot de passe et ouverte en mémoire jusqu'à la \
fin de la session : la réponse se restaure sans le redemander.",
                        );
                    }
                    (None, Goal::Anonymize) => {
                        muted(ui, "Aucune clé créée : aucun retour possible.");
                    }
                    (None, Goal::Mask) => {
                        muted(ui, "Aucune clé écrite : aucun fichier n'a été livré.");
                    }
                }
            });
        }
    });
}

/// Une façon d'envoyer : titre, explication, bouton.
fn way(ui: &mut Ui, icon: Icon, title: &str, text: &str, action: impl FnOnce(&mut Ui)) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(2);
        card_title(ui, icon, title);
        muted(ui, text);
        action(ui);
    });
}

/// Ligne d'un fichier du lot : nom produit, état, avertissements.
fn file_line(ui: &mut Ui, name: &str, outcome: &Outcome, fx: &mut Vec<Effect>) {
    let p = palette();
    let format = FileFormat::from_path(std::path::Path::new(name));
    control_row(ui, CONTROL_MD + 8.0, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        file_tile(ui, format);
        let shown = match outcome {
            Outcome::Written { path, .. } => file_name(path),
            _ => name.to_owned(),
        };
        let w = (ui.available_width() - 240.0).max(120.0);
        truncated_text(
            ui,
            w,
            CONTROL_MD,
            RichText::new(shown)
                .font(font(Weight::Semibold, p.text_sm))
                .color(p.heading),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, ink, text) = match outcome {
                Outcome::Written { replacements, .. } => (
                    Icon::CircleCheck,
                    p.accent,
                    count_label(*replacements, "valeur masquée", "valeurs masquées"),
                ),
                Outcome::Leak { .. } => (
                    Icon::CircleX,
                    p.error,
                    "Non livré : vérification finale".to_owned(),
                ),
                Outcome::Failed(_) => (Icon::CircleX, p.error, "Non traité".to_owned()),
                Outcome::Cancelled => (Icon::Clock, p.text_secondary, "Annulé".to_owned()),
            };
            ui.label(
                RichText::new(text)
                    .font(font(Weight::Semibold, p.text_xs))
                    .color(ink),
            );
            icon.show(ui, ink, 16.0);
        });
    });
    match outcome {
        Outcome::Written { warnings, .. } => {
            for w in warnings
                .iter()
                .filter(|w| w.code == WarningCode::ImageNotRestorable)
            {
                let text = format!(
                    "{} Le fichier masqué est bien protégé ; seule la restauration des images est impossible.",
                    w.detail
                );
                Banner::warning(&text)
                    .title("Images masquées non restaurables")
                    .show(ui);
            }
            for w in warnings
                .iter()
                .filter(|w| w.code != WarningCode::ImageNotRestorable)
            {
                caption(ui, &format!("{} : {}", w.part, w.detail));
            }
        }
        Outcome::Leak { tags, parts } => {
            let text = format!(
                "Après réécriture, {} restent lisibles ({}). Le fichier n'a pas été enregistré. \
Vous pouvez enregistrer son texte masqué, sans mise en forme.",
                tags.join(", "),
                parts.join(", ")
            );
            Banner::error(&text)
                .title("Vérification finale : fichier bloqué")
                .show(ui);
            if ui
                .add(
                    PillButton::secondary("Enregistrer le texte masqué (.md)")
                        .icon(Icon::Download)
                        .small(),
                )
                .clicked()
            {
                fx.push(Effect::ExportText(name.to_owned()));
            }
        }
        Outcome::Failed(msg) => {
            caption(ui, msg);
        }
        Outcome::Cancelled => {}
    }
}
