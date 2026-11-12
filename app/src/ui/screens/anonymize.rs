//! Onglet « Anonymiser » : Déposer → Vérifier → Protéger → Terminé.

use super::{action_bar, card_title, file_row, file_tile, page_title};
use crate::ui::app::Effect;
use crate::ui::mock;
use crate::ui::state::anonymize::{AnonymizeState, FileReport, LeakCheck, Risk, Step};
use crate::ui::state::files::human_size;
use crate::ui::theme::{Weight, font, palette};
use crate::ui::widgets::badge::{FAMILIES, family_label, family_tint};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::checkbox::toggle;
use crate::ui::widgets::nav::{filter_chip, segmented};
use crate::ui::widgets::{
    Badge, Banner, Card, CheckState, Column, DataTable, DropZone, Icon, PillButton, RowTone,
    Stepper, StrengthMeter, TextField, body, caption, muted, overline, subheading,
};
use anon_core::{EntityKind, Profile};
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    page_title(
        ui,
        "Anonymiser des documents",
        "Remplacez les données personnelles par des balises avant d'envoyer vos fichiers à une IA.",
    );
    let labels: Vec<&str> = Step::ALL.iter().map(|s| s.label()).collect();
    Stepper::new(&labels, st.step.index()).show(ui);
    match st.step {
        Step::Drop => drop_step(ui, st, fx),
        Step::Review => review_step(ui, st),
        Step::Protect => protect_step(ui, st),
        Step::Done => done_step(ui, st, fx),
    }
}

fn drop_step(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        let zone = ui.add(DropZone::new(
            "Déposez vos fichiers ici ou cliquez pour les choisir",
            "Word, Excel, PowerPoint, OpenDocument, PDF, texte, CSV, images",
        ));
        if zone.clicked() {
            fx.push(Effect::PickDocuments);
        }
        if !st.files.is_empty() {
            overline(ui, &format!("{} fichiers prêts", st.files.len()));
            let mut remove = None;
            for (i, f) in st.files.iter().enumerate() {
                file_row(ui, f, |ui| {
                    let label = format!("Retirer {}", f.name);
                    if icon_button(ui, Icon::Trash, &label, p.text_secondary).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                st.remove_file(i);
            }
        }
    });

    if !st.rejected.is_empty() {
        let names: Vec<&str> = st.rejected.iter().map(|f| f.name.as_str()).collect();
        let text = format!(
            "Non ajoutés : {}. Les raccourcis Google ne contiennent pas le document : ouvrez-le, \
puis Fichier → Télécharger au format Word, Excel ou PowerPoint.",
            names.join(", ")
        );
        Banner::warning(&text).title("Fichiers ignorés").show(ui);
    }
    Banner::info(
        "Ouvrez le document, puis Fichier → Télécharger, et choisissez le format Microsoft \
(Word, Excel ou PowerPoint). Déposez ensuite le fichier téléchargé.",
    )
    .title("Fichiers Google Docs, Sheets ou Slides")
    .show(ui);

    action_bar(
        ui,
        st,
        |ui, st| {
            let total: u64 = st.files.iter().filter_map(|f| f.size_bytes).sum();
            if st.files.is_empty() {
                muted(ui, "Aucun fichier pour l'instant.");
            } else {
                muted(
                    ui,
                    &format!("{} fichiers, {}", st.files.len(), human_size(total)),
                );
            }
        },
        |ui, st| {
            let go = ui.add(
                PillButton::primary("Analyser les fichiers")
                    .icon(Icon::Search)
                    .enabled(!st.files.is_empty()),
            );
            if go.clicked() {
                // Branchement futur : analyse par anon-engine.
                let _ = st.start_review(mock::candidates());
            }
        },
    );
}

fn review_step(ui: &mut Ui, st: &mut AnonymizeState) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(4);
            ui.label(
                RichText::new("Niveau de protection")
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.dark),
            );
            segmented(
                ui,
                "Niveau de protection",
                &mut st.review.profile,
                &[
                    (Profile::Standard, "Standard"),
                    (Profile::Renforce, "Renforcé"),
                ],
            );
        });
        let desc = match st.review.profile {
            Profile::Standard => "Noms, sociétés, coordonnées, identifiants et données bancaires.",
            Profile::Renforce => {
                "Standard, plus les dates, âges, fonctions, lieux et montants : combinés, ils \
permettent de reconnaître une personne."
            }
        };
        muted(ui, desc);
    });

    let pending = st.review.pending_confirmations();
    if pending > 0 {
        let text = format!(
            "{pending} détections incertaines sont surlignées en jaune. Cochez celles à \
anonymiser, laissez les autres décochées."
        );
        Banner::warning(&text).title("À confirmer").show(ui);
    }

    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        // Filtres par famille.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(p.space(2), p.space(2));
            let all = st.review.family_filter.is_empty();
            let brand = crate::ui::theme::Tinted {
                fill: p.primary_subtle,
                ink: p.primary_dark,
                edge: p.primary,
            };
            if filter_chip(ui, "Toutes", None, all, brand).clicked() {
                st.review.clear_filter();
            }
            for fam in FAMILIES {
                let n = st.review.count_by_family(fam);
                if n == 0 {
                    continue;
                }
                let on = st.review.family_filter.contains(&fam);
                if filter_chip(ui, family_label(fam), Some(n), on, family_tint(fam)).clicked() {
                    st.review.toggle_family(fam);
                }
            }
        });

        let columns = [
            Column::fixed("Type", 196.0),
            Column::fill("Valeur", 1.0),
            Column::fill("Contexte", 1.3),
            Column::fixed("Occurrences", 120.0),
            Column::fixed("Balise", 144.0),
        ];
        let visible = st.review.visible();
        let (checked, total) = st.review.header_state();
        let header = if checked == 0 {
            CheckState::Unchecked
        } else if checked == total {
            CheckState::Checked
        } else {
            CheckState::Mixed
        };
        let rows = &st.review.candidates;
        let row_of = |i: usize| visible.get(i).and_then(|idx| rows.get(*idx));
        let out = DataTable::new("detections", &columns)
            .checkable(header)
            .max_height(44.0 * 8.0)
            .show(
                ui,
                visible.len(),
                |i| row_of(i).is_some_and(|c| c.selected),
                |i| {
                    if row_of(i).is_some_and(|c| c.to_confirm) {
                        RowTone::Attention
                    } else {
                        RowTone::Normal
                    }
                },
                |i| row_of(i).map(|c| c.value.clone()).unwrap_or_default(),
                |ui, i, col| {
                    let Some(c) = row_of(i) else { return };
                    match col {
                        0 => {
                            ui.add(Badge::entity(c.kind));
                        }
                        1 => {
                            if c.to_confirm {
                                Icon::TriangleAlert.show(ui, p.data.ink, 16.0);
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&c.value)
                                        .font(font(Weight::Semibold, p.text_sm))
                                        .color(p.dark),
                                )
                                .truncate(),
                            );
                        }
                        2 => {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&c.context)
                                        .font(font(Weight::Regular, p.text_sm))
                                        .color(p.text_secondary),
                                )
                                .truncate(),
                            );
                        }
                        3 => {
                            let text = if c.occurrences == 0 {
                                "—".to_owned()
                            } else {
                                c.occurrences.to_string()
                            };
                            body(ui, &text);
                        }
                        _ => {
                            ui.label(
                                RichText::new(c.tag.to_string())
                                    .monospace()
                                    .color(p.primary_dark),
                            );
                        }
                    }
                },
            );
        if out.header_toggled {
            st.review.set_all_visible(header != CheckState::Checked);
        }
        for i in out.toggled_rows {
            if let Some(idx) = visible.get(i) {
                st.review.toggle(*idx);
            }
        }

        // Ajout manuel.
        ui.add_space(p.space(1));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let error = st.review.manual_error.map(|e| e.message());
            let field_w = (ui.available_width() - 360.0).clamp(200.0, 420.0);
            let r = TextField::new("Terme à ajouter", &mut st.review.manual_term)
                .placeholder("Ajouter un terme absent de la liste")
                .id_salt("terme-manuel")
                .hide_label()
                .error(error)
                .width(field_w)
                .show(ui);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ComboBox::from_id_salt("type-manuel")
                .selected_text(st.review.manual_kind.label())
                .width(190.0)
                .show_ui(ui, |ui| {
                    for k in EntityKind::ALL {
                        ui.selectable_value(&mut st.review.manual_kind, *k, k.label());
                    }
                })
                .response
                .on_hover_text("Type du terme ajouté");
            let add = ui.add(PillButton::secondary("Ajouter").icon(Icon::Plus));
            if add.clicked() || enter {
                let _ = st.review.add_manual();
            }
        });
    });

    if let Some(e) = st.error {
        Banner::error(e.message()).show(ui);
    }
    action_bar(
        ui,
        st,
        |ui, st| {
            if ui
                .add(PillButton::secondary("Retour").icon(Icon::Undo))
                .clicked()
            {
                st.back();
            }
            muted(
                ui,
                &format!("{} valeurs seront anonymisées", st.review.selected_count()),
            );
        },
        |ui, st| {
            let go = ui.add(
                PillButton::primary("Continuer")
                    .icon(Icon::ChevronRight)
                    .enabled(st.review.selected_count() > 0),
            );
            if go.clicked() {
                let _ = st.confirm_review();
            }
        },
    );
}

fn protect_step(ui: &mut Ui, st: &mut AnonymizeState) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        card_title(ui, Icon::KeyRound, "Protéger la table de correspondance");
        ui.add(
            egui::Label::new(
                RichText::new(
                    "La table relie chaque balise à la vraie valeur. Elle reste sur votre \
ordinateur, chiffrée par ce mot de passe. Sans lui, la restauration est impossible : notez-le \
en lieu sûr.",
                )
                .font(font(Weight::Regular, p.text_sm))
                .color(p.text),
            )
            .wrap(),
        );
        let field_w = ui.available_width().min(420.0);
        let disabled = st.protect.irreversible;
        ui.add_enabled_ui(!disabled, |ui| {
            let issue = st.protect.password_issue().map(|i| i.message());
            TextField::password("Mot de passe", &mut st.protect.password)
                .id_salt("mdp")
                .required(true)
                .error(issue)
                .width(field_w)
                .show(ui);
            let strength = st.protect.strength();
            ui.scope(|ui| {
                ui.set_width(field_w);
                ui.add(StrengthMeter::new(strength));
            });
            let confirm_issue = st.protect.confirm_issue().map(|i| i.message());
            TextField::password("Confirmation du mot de passe", &mut st.protect.confirm)
                .id_salt("mdp-confirmation")
                .required(true)
                .error(confirm_issue)
                .width(field_w)
                .show(ui);
        });
        toggle(
            ui,
            &mut st.protect.irreversible,
            "Mode irréversible : ne pas créer de table de correspondance",
        );
        if st.protect.irreversible {
            Banner::warning(
                "Les balises ne pourront jamais être remplacées par les vraies valeurs. À \
réserver aux documents que vous n'aurez pas à restaurer.",
            )
            .show(ui);
        }
        caption(
            ui,
            "Les fichiers anonymisés sont enregistrés à côté des originaux, avec « (anonymisé) » \
dans leur nom.",
        );
    });
    if let Some(e) = st.error {
        Banner::error(e.message()).show(ui);
    }
    action_bar(
        ui,
        st,
        |ui, st| {
            if ui
                .add(PillButton::secondary("Retour").icon(Icon::Undo))
                .clicked()
            {
                st.back();
            }
        },
        |ui, st| {
            let label = format!("Anonymiser {} fichiers", st.files.len());
            let go = ui.add(
                PillButton::primary(&label)
                    .icon(Icon::Lock)
                    .enabled(st.protect.can_proceed()),
            );
            if go.clicked() {
                // Branchement futur : traitement par anon-engine.
                let _ = st.finish(mock::reports());
            }
        },
    );
}

fn done_step(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    let delivered = st.delivered_count();
    let total = st.reports.len();
    let folder = st
        .files
        .first()
        .and_then(|f| f.path.parent())
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let blocked = total - delivered;
    let text = if blocked == 0 {
        format!("{delivered} fichiers anonymisés, enregistrés dans {folder}.")
    } else {
        format!(
            "{delivered} fichiers sur {total} enregistrés dans {folder}. {blocked} fichier \
bloqué par le contrôle de fuite : il n'a pas été livré."
        )
    };
    Banner::success(&text)
        .title("Anonymisation terminée")
        .show(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        if ui
            .add(PillButton::primary("Ouvrir le dossier").icon(Icon::FolderOpen))
            .clicked()
        {
            fx.push(Effect::OpenOutputFolder);
        }
        if ui
            .add(PillButton::secondary("Copier la consigne pour l'IA").icon(Icon::Clipboard))
            .clicked()
        {
            fx.push(Effect::Copy(st.ai_instruction()));
        }
        if ui
            .add(PillButton::ghost("Nouvelle anonymisation").icon(Icon::RefreshCw))
            .clicked()
        {
            st.reset();
        }
    });
    subheading(ui, "Rapport par fichier");
    let format_of = |name: &str| {
        st.files
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.format)
            .unwrap_or(crate::ui::state::files::FileFormat::Unsupported)
    };
    for r in &st.reports {
        report_card(ui, r, format_of(&r.name));
    }
}

fn report_card(ui: &mut Ui, r: &FileReport, format: crate::ui::state::files::FileFormat) {
    let p = palette();
    let accent = match r.leak_check {
        LeakCheck::Passed => p.primary,
        LeakCheck::Blocked(_) => p.error,
    };
    Card::new().accent(accent).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            file_tile(ui, format);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&r.name)
                            .font(font(Weight::Semibold, p.text_base))
                            .color(p.dark),
                    )
                    .truncate(),
                );
                let out = match r.leak_check {
                    LeakCheck::Passed => format!("Enregistré sous « {} »", r.output_name),
                    LeakCheck::Blocked(_) => "Non livré".to_owned(),
                };
                caption(ui, &out);
            });
        });
        ui.columns(3, |cols| {
            if let [a, b, c] = cols {
                stat(a, "Remplacements", |ui| {
                    ui.label(
                        RichText::new(r.replacements.to_string())
                            .font(font(Weight::Bold, p.text_xl))
                            .color(p.dark),
                    );
                });
                stat(b, "Contrôle de fuite", |ui| match r.leak_check {
                    LeakCheck::Passed => {
                        ui.add(Badge::brand("Réussi"));
                    }
                    LeakCheck::Blocked(n) => {
                        ui.add(Badge::error(&format!("Bloqué : {n} valeur restante")));
                    }
                });
                stat(c, "Risque de ré-identification", |ui| {
                    let badge = match r.risk {
                        Risk::Low => Badge::brand(r.risk.label()),
                        Risk::Medium => Badge::warning(r.risk.label()),
                        Risk::High => Badge::error(r.risk.label()),
                    };
                    ui.add(badge);
                });
            }
        });
        if let Some(reason) = &r.risk_reason {
            ui.add(
                egui::Label::new(
                    RichText::new(reason)
                        .font(font(Weight::Regular, p.text_sm))
                        .color(p.text),
                )
                .wrap(),
            );
        }
        if !r.warnings.is_empty() {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                overline(ui, "Avertissements");
                for w in &r.warnings {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        Icon::TriangleAlert.show(ui, p.data.ink, 16.0);
                        body(ui, w);
                    });
                }
            });
        }
    });
}

fn stat(ui: &mut Ui, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        overline(ui, label);
        value(ui);
    });
}
