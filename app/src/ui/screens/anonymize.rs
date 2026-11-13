//! Onglet « Anonymiser » : Déposer → Vérifier → Protéger → Terminé.

use super::images::images_card;
use super::{action_bar, card_title, file_row, file_tile, page_title};
use crate::ui::app::Effect;
use crate::ui::runner::Shared;
use crate::ui::service::{BatchReport, Outcome, Rejected, file_name};
use crate::ui::state::anonymize::{AnonymizeState, ProtectMode, Step, needs_confirmation};
use crate::ui::state::files::{FileFormat, human_size};
use crate::ui::state::generalize::rule_label;
use crate::ui::theme::{Weight, font, palette};
use crate::ui::widgets::badge::{FAMILIES, family_label, family_tint};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::field::mono_font;
use crate::ui::widgets::nav::{filter_chip, segmented};
use crate::ui::widgets::{
    Badge, Banner, Card, CheckState, Checkbox, Column, DataTable, DropZone, Icon, PillButton,
    ProgressBar, RowTone, Stepper, StrengthMeter, TextField, body, caption, muted, overline,
    subheading,
};
use anon_core::{EntityKind, Profile, Session, WarningCode};
use anon_engine::RiskLevel;
use egui::{RichText, Sense, Ui};

pub fn show(
    ui: &mut Ui,
    st: &mut AnonymizeState,
    session: &Session,
    progress: &Shared,
    fx: &mut Vec<Effect>,
) {
    page_title(
        ui,
        "Anonymiser des documents",
        "Remplacez les données personnelles par des balises avant d'envoyer vos fichiers à une IA.",
    );
    let labels: Vec<&str> = Step::ALL.iter().map(|s| s.label()).collect();
    Stepper::new(&labels, st.step.index()).show(ui);
    if let Some(busy) = st.busy {
        busy_card(ui, busy.label(), progress, fx);
        return;
    }
    match st.step {
        Step::Drop => drop_step(ui, st, fx),
        Step::Review => review_step(ui, st, session, fx),
        Step::Protect => protect_step(ui, st, fx),
        Step::Done => done_step(ui, st, fx),
    }
}

/// Carte d'avancement avec bouton Annuler.
pub fn busy_card(ui: &mut Ui, title: &str, progress: &Shared, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        let label = progress.label();
        let line = if label.is_empty() {
            title.to_owned()
        } else {
            format!("{title} : {label}")
        };
        ui.add(ProgressBar::new(progress.fraction(), &line));
        ui.horizontal(|ui| {
            if progress.is_cancel_requested() {
                muted(ui, "Annulation demandée : le fichier en cours se termine.");
            } else if ui
                .add(PillButton::secondary("Annuler").icon(Icon::X).small())
                .clicked()
            {
                fx.push(Effect::Cancel);
            }
        });
    });
}

fn rejected_banner(ui: &mut Ui, rejected: &[Rejected]) -> bool {
    if rejected.is_empty() {
        return false;
    }
    let text = rejected
        .iter()
        .map(|r| format!("{} : {}", r.name, r.reason))
        .collect::<Vec<_>>()
        .join("\n");
    Banner::warning(&text)
        .title("Fichiers non pris en compte")
        .show(ui);
    ui.add(PillButton::ghost("Masquer ces messages").small())
        .clicked()
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
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            if ui
                .add(
                    PillButton::secondary("Essayer avec des exemples")
                        .icon(Icon::Sparkles)
                        .small(),
                )
                .on_hover_text("Copie des fichiers fictifs dans un dossier de votre choix.")
                .clicked()
            {
                fx.push(Effect::TryExamples);
            }
            caption(
                ui,
                "Des documents fictifs (contrat, tableur, PDF scanné, carte de visite…) copiés dans un dossier de votre choix.",
            );
        });
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

    always_card(ui, st, fx);
    if rejected_banner(ui, &st.rejected) {
        st.dismiss_rejected();
    }
    Banner::info(
        "Ouvrez le document, puis Fichier > Télécharger, et choisissez le format Microsoft \
(Word, Excel ou PowerPoint). Déposez ensuite le fichier téléchargé.",
    )
    .title("Fichiers Google Docs, Sheets ou Slides")
    .show(ui);
    if let Some(e) = st.error {
        Banner::error(e.message()).show(ui);
    }

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
            if go.clicked()
                && let Ok(paths) = st.begin_analysis()
            {
                fx.push(Effect::Analyze(paths));
            }
        },
    );
}

/// Liste « toujours anonymiser » : termes cherchés dans chaque analyse.
fn always_card(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::Plus, "Toujours anonymiser");
        let note = if st.step == Step::Review {
            "Termes cherchés dans tous les fichiers, même si la détection les manque (nom de projet, client, lieu-dit). L'analyse est relancée à chaque modification."
        } else {
            "Termes cherchés dans tous les fichiers, même si la détection les manque (nom de projet, client, lieu-dit)."
        };
        muted(ui, note);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let error = st.always_error.map(|e| e.message());
            let field_w = (ui.available_width() - 360.0).clamp(200.0, 420.0);
            let r = TextField::new("Terme à toujours anonymiser", &mut st.always_term)
                .placeholder("Terme à toujours anonymiser")
                .id_salt("terme-toujours")
                .hide_label()
                .error(error)
                .width(field_w)
                .show(ui);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ComboBox::from_id_salt("type-toujours")
                .selected_text(st.always_kind.label())
                .width(190.0)
                .show_ui(ui, |ui| {
                    for k in EntityKind::ALL {
                        ui.selectable_value(&mut st.always_kind, *k, k.label());
                    }
                })
                .response
                .on_hover_text("Type du terme");
            let add = ui.add(PillButton::secondary("Ajouter à la liste").icon(Icon::Plus));
            if (add.clicked() || enter)
                && let Ok(Some(paths)) = st.add_always()
            {
                fx.push(Effect::Analyze(paths));
            }
        });
        if st.always_kind_needs_renforce() {
            caption(
                ui,
                "Ce type n'est recherché qu'au niveau de protection Renforcé.",
            );
        }
        let mut remove = None;
        if !st.always.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(p.space(2), p.space(2));
                for (i, (term, kind)) in st.always.iter().enumerate() {
                    ui.add(Badge::entity(*kind).small());
                    body(ui, term);
                    if icon_button(ui, Icon::X, &format!("Retirer {term}"), p.text_secondary)
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    ui.add_space(p.space(2));
                }
            });
        }
        if let Some(i) = remove
            && let Some(paths) = st.remove_always(i)
        {
            fx.push(Effect::Analyze(paths));
        }
    });
}

/// Menu balise / valeur généralisée d'une ligne : `Some(true)` pour généraliser.
fn treatment_menu(response: &egui::Response, tag: &str, general: &str) -> Option<bool> {
    let mut chosen = None;
    egui::Popup::menu(response).show(|ui| {
        ui.set_min_width(280.0);
        if ui
            .selectable_label(false, format!("Balise {tag}"))
            .clicked()
        {
            chosen = Some(false);
        }
        if ui
            .selectable_label(false, format!("Généraliser : {general} (irréversible)"))
            .clicked()
        {
            chosen = Some(true);
        }
    });
    chosen
}

/// Menu de choix du type d'une ligne.
fn kind_menu(response: &egui::Response, current: EntityKind) -> Option<EntityKind> {
    let mut chosen = None;
    egui::Popup::menu(response).show(|ui| {
        ui.set_min_width(220.0);
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for k in EntityKind::ALL {
                    if ui.selectable_label(*k == current, k.label()).clicked() {
                        chosen = Some(*k);
                    }
                }
            });
    });
    chosen
}

fn review_step(ui: &mut Ui, st: &mut AnonymizeState, session: &Session, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(4);
            ui.label(
                RichText::new("Niveau de protection")
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.dark),
            );
            let mut profile = st.profile;
            segmented(
                ui,
                "Niveau de protection",
                &mut profile,
                &[
                    (Profile::Standard, "Standard"),
                    (Profile::Renforce, "Renforcé"),
                ],
            );
            if let Some(paths) = st.set_profile(profile) {
                fx.push(Effect::Analyze(paths));
            }
        });
        let desc = match st.profile {
            Profile::Standard => "Noms, sociétés, coordonnées, identifiants et données bancaires.",
            Profile::Renforce => {
                "Standard, plus les dates, âges, fonctions, lieux et montants : combinés, ils \
permettent de reconnaître une personne."
            }
        };
        muted(ui, desc);
    });

    if rejected_banner(ui, &st.rejected) {
        st.dismiss_rejected();
    }
    let pending = st.pending_confirmations();
    if pending > 0 {
        let text = if pending == 1 {
            "1 valeur courte ou incertaine est surlignée en jaune : vérifiez-la. \
Cochez-la pour l'anonymiser ; « Ne jamais anonymiser » la retire pour toute la session."
                .to_owned()
        } else {
            format!(
                "{pending} valeurs courtes ou incertaines sont surlignées en jaune : vérifiez-les. \
Cochez celles à anonymiser ; « Ne jamais anonymiser » les retire pour toute la session."
            )
        };
        Banner::warning(&text).title("À confirmer").show(ui);
    }
    let generalizable = st
        .visible()
        .iter()
        .any(|id| st.generalization_for(*id).is_some());
    if generalizable {
        Banner::info(
            "Pour une date, un âge ou un montant, cliquez sur sa balise pour la remplacer par une valeur approchée : l'année, la tranche de dix ans ou l'ordre de grandeur. C'est irréversible : la valeur exacte ne sera pas remise à la restauration.",
        )
        .title("Généralisation")
        .show(ui);
    }

    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(p.space(2), p.space(2));
            let all = st.family_filter.is_empty();
            let brand = crate::ui::theme::Tinted {
                fill: p.primary_subtle,
                ink: p.primary_dark,
                edge: p.primary,
            };
            if filter_chip(ui, "Toutes", None, all, brand).clicked() {
                st.clear_filter();
            }
            for fam in FAMILIES {
                let n = st.count_by_family(fam);
                if n == 0 {
                    continue;
                }
                let on = st.family_filter.contains(&fam);
                if filter_chip(ui, family_label(fam), Some(n), on, family_tint(fam)).clicked() {
                    st.toggle_family(fam);
                }
            }
        });

        let columns = [
            Column::fixed("Type", 190.0),
            Column::fill("Valeur", 1.0),
            Column::fill("Contexte", 1.2),
            Column::fixed("Occ.", 60.0),
            Column::fixed("Casse", 64.0),
            Column::fixed("Balise", 176.0),
            Column::fixed("", 44.0),
        ];
        let visible = st.visible();
        let (checked, total) = st.header_state();
        let header = if checked == 0 {
            CheckState::Unchecked
        } else if checked == total {
            CheckState::Checked
        } else {
            CheckState::Mixed
        };
        let mut kind_change: Option<(usize, EntityKind)> = None;
        let mut case_change: Option<(usize, bool)> = None;
        let mut never: Option<usize> = None;
        let mut general_change: Option<(usize, bool)> = None;
        let out = {
            let row_of = |i: usize| visible.get(i).and_then(|id| st.candidate(*id));
            DataTable::new("detections", &columns)
                .checkable(header)
                .max_height(44.0 * 9.0)
                .show(
                    ui,
                    visible.len(),
                    |i| row_of(i).is_some_and(|c| c.selected),
                    |i| {
                        if row_of(i).is_some_and(needs_confirmation) {
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
                                let r = ui.add(Badge::entity(c.kind));
                                let r = ui.interact(r.rect, r.id.with("type"), Sense::click());
                                let label = format!("Type : {}. Modifier", c.kind.label());
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                                });
                                let r = r
                                    .on_hover_text("Changer le type")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if let Some(k) = kind_menu(&r, c.kind) {
                                    kind_change = Some((c.id, k));
                                }
                            }
                            1 => {
                                if needs_confirmation(c) {
                                    Icon::TriangleAlert.show(ui, p.data.ink, 16.0);
                                }
                                let mut text = RichText::new(&c.value)
                                    .font(font(Weight::Semibold, p.text_sm))
                                    .color(p.dark);
                                if !c.selected {
                                    text = text.color(p.text_secondary);
                                }
                                let r = ui.add(egui::Label::new(text).truncate());
                                if !c.variants.is_empty() {
                                    r.on_hover_text(format!(
                                        "Autres formes : {}",
                                        c.variants.join(", ")
                                    ));
                                }
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
                                body(ui, &c.occurrences.to_string());
                            }
                            4 => {
                                let label = format!("Respecter la casse de {}", c.value);
                                if ui
                                    .add(Checkbox::bare(c.case_sensitive, &label))
                                    .on_hover_text("Respecter la casse exacte")
                                    .clicked()
                                {
                                    case_change = Some((c.id, !c.case_sensitive));
                                }
                            }
                            5 => {
                                let tag = st.tag_preview.get(&c.id).map(|t| t.to_string());
                                let general = st.generalization_for(c.id);
                                let chosen = st.generalized(c.id);
                                let shown = match (&tag, chosen) {
                                    (_, Some(g)) if c.selected => {
                                        Some(
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(g)
                                                        .font(font(Weight::Medium, p.text_sm))
                                                        .color(p.data.ink),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(format!(
                                                "Valeur généralisée ({}) : irréversible",
                                                rule_label(c.kind)
                                            )),
                                        )
                                    }
                                    (Some(t), _) => Some(ui.label(
                                        RichText::new(t).font(mono_font()).color(p.primary_dark),
                                    )),
                                    _ => {
                                        caption(ui, "non remplacée");
                                        None
                                    }
                                };
                                if let (Some(r), Some(g), Some(t)) = (shown, general, &tag) {
                                    Icon::ChevronDown.show(ui, p.text_secondary, 14.0);
                                    let r = ui.interact(
                                        r.rect,
                                        r.id.with("traitement"),
                                        Sense::click(),
                                    );
                                    let label = format!("Remplacement de {} : modifier", c.value);
                                    r.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                                    });
                                    let r = r
                                        .on_hover_text("Balise ou valeur généralisée")
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if let Some(on) = treatment_menu(&r, t, &g) {
                                        general_change = Some((c.id, on));
                                    }
                                }
                            }
                            _ => {
                                let label = format!("Ne jamais anonymiser {}", c.value);
                                if icon_button(ui, Icon::EyeOff, &label, p.text_secondary).clicked()
                                {
                                    never = Some(c.id);
                                }
                            }
                        }
                    },
                )
        };
        if out.header_toggled {
            st.set_all_visible(header != CheckState::Checked, session);
        }
        for i in out.toggled_rows {
            if let Some(id) = visible.get(i) {
                st.toggle(*id, session);
            }
        }
        if let Some((id, k)) = kind_change {
            st.set_kind(id, k, session);
        }
        if let Some((id, on)) = case_change {
            st.set_case_sensitive(id, on, session);
        }
        if let Some(id) = never {
            st.never_anonymize(id, session);
        }
        if let Some((id, on)) = general_change {
            st.set_generalized(id, on, session);
        }
        if visible.is_empty() {
            muted(ui, "Aucune donnée sensible détectée dans ces fichiers.");
        }

        // Ajout manuel.
        ui.add_space(p.space(1));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let error = st.manual_error.map(|e| e.message());
            let field_w = (ui.available_width() - 360.0).clamp(200.0, 420.0);
            let r = TextField::new("Terme à ajouter", &mut st.manual_term)
                .placeholder("Ajouter un terme absent de la liste")
                .id_salt("terme-manuel")
                .hide_label()
                .error(error)
                .width(field_w)
                .show(ui);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ComboBox::from_id_salt("type-manuel")
                .selected_text(st.manual_kind.label())
                .width(190.0)
                .show_ui(ui, |ui| {
                    for k in EntityKind::ALL {
                        ui.selectable_value(&mut st.manual_kind, *k, k.label());
                    }
                })
                .response
                .on_hover_text("Type du terme ajouté");
            let add = ui.add(PillButton::secondary("Ajouter").icon(Icon::Plus));
            if add.clicked() || enter {
                let _ = st.add_manual(session);
            }
        });
        if !st.never.is_empty() {
            caption(
                ui,
                &format!(
                    "Jamais anonymisés dans cette session : {}",
                    st.never.join(", ")
                ),
            );
        }
    });

    images_card(ui, st);
    always_card(ui, st, fx);

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
            let generalized = st.generalized_count();
            let mut line = format!("{} valeurs seront anonymisées", st.selected_count());
            if generalized > 0 {
                line.push_str(&format!(", dont {generalized} généralisées (irréversible)"));
            }
            if !st.masks.is_empty() {
                line.push_str(&format!(", {} zones d'image ajoutées", st.masks.len()));
            }
            muted(ui, &line);
        },
        |ui, st| {
            let go = ui.add(
                PillButton::primary("Continuer")
                    .icon(Icon::ChevronRight)
                    .enabled(st.selected_count() > 0),
            );
            if go.clicked() {
                let _ = st.confirm_review();
            }
        },
    );
}

fn protect_step(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        card_title(ui, Icon::KeyRound, "Protéger la table de correspondance");
        ui.add(
            egui::Label::new(
                RichText::new(
                    "La table relie chaque balise à la vraie valeur. Elle est enregistrée à côté \
des fichiers, chiffrée par ce mot de passe. Sans lui, la restauration est impossible : notez-le \
en lieu sûr.",
                )
                .font(font(Weight::Regular, p.text_sm))
                .color(p.text),
            )
            .wrap(),
        );
        let mut mode = st.protect.mode;
        segmented(
            ui,
            "Protection",
            &mut mode,
            &[
                (ProtectMode::NewPassword, "Nouveau mot de passe"),
                (ProtectMode::ExistingTable, "Reprendre une table"),
                (ProtectMode::Irreversible, "Irréversible"),
            ],
        );
        if mode != st.protect.mode {
            st.protect.mode = mode;
            st.protect.password.clear();
            st.protect.confirm.clear();
        }
        let field_w = ui.available_width().min(420.0);
        match st.protect.mode {
            ProtectMode::NewPassword => {
                let issue = st.protect.password_issue().map(|i| i.message());
                TextField::password("Mot de passe", &mut st.protect.password)
                    .id_salt("mdp")
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
                    "Les valeurs déjà présentes dans cette table gardent leurs balises. Une \
nouvelle table, complétée, est enregistrée avec le même mot de passe.",
                );
                match &st.protect.existing {
                    Some(t) => {
                        ui.horizontal(|ui| {
                            ui.add(Badge::brand("Ouverte"));
                            body(ui, &format!("{} : {} correspondances", t.name, t.entries));
                        });
                    }
                    None => {
                        let name = st
                            .protect
                            .existing_path
                            .as_deref()
                            .map(file_name)
                            .unwrap_or_else(|| "Aucune table choisie".to_owned());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = p.space(3);
                            if ui
                                .add(
                                    PillButton::secondary("Choisir la table")
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
                        TextField::password("Mot de passe de la table", &mut st.protect.password)
                            .id_salt("mdp-reprise")
                            .error(error)
                            .width(field_w)
                            .show(ui);
                        let ready =
                            st.protect.existing_path.is_some() && !st.protect.password.is_empty();
                        if ui
                            .add(
                                PillButton::primary("Ouvrir la table")
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
            ProtectMode::Irreversible => {
                Banner::warning(
                    "Aucune table n'est créée : les balises ne pourront jamais être remplacées \
par les vraies valeurs. À réserver aux documents que vous n'aurez pas à restaurer.",
                )
                .show(ui);
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            Icon::FolderOpen.show(ui, p.text_secondary, 18.0);
            let dir = st
                .out_dir
                .as_deref()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "Aucun dossier choisi".to_owned());
            body(ui, &format!("Dossier de sortie : {dir}"));
            if ui.add(PillButton::ghost("Changer").small()).clicked() {
                fx.push(Effect::PickOutputDir);
            }
        });
        caption(
            ui,
            "Chaque fichier est enregistré sous « nom_anonymise » ; un fichier existant n'est \
jamais remplacé.",
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
            let label = format!("Anonymiser {} fichiers", st.inputs.len());
            let go = ui.add(
                PillButton::primary(&label)
                    .icon(Icon::Lock)
                    .enabled(st.protect.can_proceed() && st.out_dir.is_some()),
            );
            if go.clicked() {
                fx.push(Effect::Anonymize);
            }
        },
    );
}

fn done_step(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    let Some(report) = st.report.clone() else {
        return;
    };
    summary_banner(ui, &report);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        if ui
            .add(PillButton::primary("Ouvrir le dossier").icon(Icon::FolderOpen))
            .clicked()
        {
            fx.push(Effect::OpenFolder(report.out_dir.clone()));
        }
        if ui
            .add(PillButton::secondary("Copier la consigne pour l'IA").icon(Icon::Clipboard))
            .clicked()
        {
            fx.push(Effect::Copy(anon_engine::ai_instruction().to_owned()));
        }
        if ui
            .add(PillButton::ghost("Anonymiser d'autres fichiers").icon(Icon::RefreshCw))
            .clicked()
        {
            st.reset();
        }
    });
    subheading(ui, "Rapport par fichier");
    let format_of = |name: &str| FileFormat::from_path(std::path::Path::new(name));
    for f in &report.files {
        report_card(ui, &f.name, &f.outcome, format_of(&f.name), fx);
    }
}

fn summary_banner(ui: &mut Ui, report: &BatchReport) {
    let delivered = report.delivered();
    let total = report.files.len();
    let folder = report.out_dir.display().to_string();
    if let Some(fatal) = &report.fatal {
        Banner::error(fatal).title("Aucun fichier écrit").show(ui);
        return;
    }
    let mut text = format!("{delivered} fichiers sur {total} enregistrés dans {folder}.");
    if let Some(map) = &report.map_path {
        text.push_str(&format!(
            " Table de correspondance : {}. Gardez-la avec son mot de passe pour restaurer.",
            file_name(map)
        ));
    } else if delivered > 0 {
        text.push_str(" Mode irréversible : aucune table n'a été créée.");
    }
    if report.cancelled {
        text.push_str(" Traitement annulé : les fichiers restants n'ont pas été traités.");
    }
    if delivered == total {
        Banner::success(&text)
            .title("Anonymisation terminée")
            .show(ui);
    } else {
        Banner::warning(&text)
            .title("Anonymisation terminée, avec des fichiers non livrés")
            .show(ui);
    }
}

fn report_card(
    ui: &mut Ui,
    name: &str,
    outcome: &Outcome,
    format: FileFormat,
    fx: &mut Vec<Effect>,
) {
    let p = palette();
    let accent = match outcome {
        Outcome::Written { .. } => p.primary,
        Outcome::Leak { .. } | Outcome::Failed(_) => p.error,
        Outcome::Cancelled => p.gray_400,
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
                        RichText::new(name)
                            .font(font(Weight::Semibold, p.text_base))
                            .color(p.dark),
                    )
                    .truncate(),
                );
                let out = match outcome {
                    Outcome::Written { path, .. } => {
                        format!("Enregistré sous « {} »", file_name(path))
                    }
                    Outcome::Leak { .. } => "Non livré : contrôle de fuite".to_owned(),
                    Outcome::Failed(_) => "Non traité".to_owned(),
                    Outcome::Cancelled => "Annulé".to_owned(),
                };
                caption(ui, &out);
            });
        });
        match outcome {
            Outcome::Written {
                replacements,
                warnings,
                risk,
                ..
            } => {
                ui.columns(3, |cols| {
                    if let [a, b, c] = cols {
                        stat(a, "Remplacements", |ui| {
                            ui.label(
                                RichText::new(replacements.to_string())
                                    .font(font(Weight::Bold, p.text_xl))
                                    .color(p.dark),
                            );
                        });
                        stat(b, "Contrôle de fuite", |ui| {
                            ui.add(Badge::brand("Réussi"));
                        });
                        stat(c, "Risque de ré-identification", |ui| {
                            let label = risk.level.label();
                            let badge = match risk.level {
                                RiskLevel::Faible => Badge::brand(label),
                                RiskLevel::Modere => Badge::warning(label),
                                RiskLevel::Eleve => Badge::error(label),
                            };
                            ui.add(badge);
                        });
                    }
                });
                for advice in &risk.advice {
                    ui.add(
                        egui::Label::new(
                            RichText::new(advice)
                                .font(font(Weight::Regular, p.text_sm))
                                .color(p.text),
                        )
                        .wrap(),
                    );
                }
                for w in warnings
                    .iter()
                    .filter(|w| w.code == WarningCode::ImageNotRestorable)
                {
                    let text = format!(
                        "{} Le fichier anonymisé est bien protégé ; seule la restauration des images est impossible.",
                        w.detail
                    );
                    Banner::warning(&text)
                        .title("Images masquées non restaurables")
                        .show(ui);
                }
                let others: Vec<_> = warnings
                    .iter()
                    .filter(|w| w.code != WarningCode::ImageNotRestorable)
                    .collect();
                if !others.is_empty() {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        overline(ui, "Avertissements");
                        for w in others {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let icon = if w.code == WarningCode::LowOcrConfidence {
                                    Icon::Image
                                } else {
                                    Icon::TriangleAlert
                                };
                                icon.show(ui, p.data.ink, 16.0);
                                body(ui, &format!("{} : {}", w.part, w.detail));
                            });
                        }
                    });
                }
            }
            Outcome::Leak { tags, parts } => {
                let text = format!(
                    "Après réécriture, {} restent lisibles ({}). Le fichier n'a pas été enregistré. \
Vous pouvez enregistrer son texte anonymisé, sans mise en forme.",
                    tags.join(", "),
                    parts.join(", ")
                );
                Banner::error(&text).title("Contrôle de fuite : bloqué").show(ui);
                if ui
                    .add(
                        PillButton::secondary("Enregistrer le texte anonymisé (.md)")
                            .icon(Icon::Download),
                    )
                    .clicked()
                {
                    fx.push(Effect::ExportText(name.to_owned()));
                }
            }
            Outcome::Failed(msg) => {
                Banner::error(msg).show(ui);
            }
            Outcome::Cancelled => {
                muted(ui, "Traitement annulé avant ce fichier.");
            }
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
