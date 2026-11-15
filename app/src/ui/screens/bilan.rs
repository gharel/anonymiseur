//! Étape « Bilan » (maquette C) : le verdict d'abord, puis les trois critères,
//! un sujet par échec avec sa correction conseillée (« Appliquer », « Autre »),
//! « Tout corriger », ce que l'IA pourrait deviner, et le contexte de l'envoi
//! (destinataire et questions facultatives). L'évaluation vient du fil de
//! travail ; chaque correction s'annule par Ctrl+Z.

use super::{busy_card, card_title, control_row, page_title};
use crate::ui::app::Effect;
use crate::ui::runner::Shared;
use crate::ui::shortcuts;
use crate::ui::state::anonymize::{AnonymizeState, Busy};
use crate::ui::state::bilan::{self, Assessed, QUESTIONS, SubjectView, answer_label, answer_mut};
use crate::ui::state::workspace::Workspace;
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette};
use crate::ui::widgets::nav::segmented;
use crate::ui::widgets::{
    Badge, Banner, BannerKind, Card, Icon, PillButton, ProgressBar, Select, TextField, body,
    caption, muted,
};
use anon_core::{AssessSettings, Criterion, Fix, Recipient, Verdict};
use egui::{RichText, Ui};

/// Largeur du nom d'un critère.
const CRITERION_W: f32 = 140.0;
/// Largeur maximale du sélecteur de destinataire.
const RECIPIENT_W: f32 = 340.0;
/// Largeur de la colonne des questions de contexte.
const QUESTION_W: f32 = 400.0;

pub fn show(
    ui: &mut Ui,
    st: &mut AnonymizeState,
    workspace: &mut Workspace,
    settings: &AssessSettings,
    progress: &Shared,
    fx: &mut Vec<Effect>,
) {
    let p = palette();
    page_title(
        ui,
        "Bilan avant envoi",
        "Trois critères, pour ce destinataire : peut-on isoler une personne, la relier à \
d'autres données, deviner une information sur elle ?",
    );
    if st.busy == Some(Busy::Fixing) {
        busy_card(ui, Busy::Fixing.label(), &[], progress, fx);
        return;
    }
    let current = st.assessment(settings).is_some();
    let Some(assessed) = st.bilan.latest.clone() else {
        Card::new().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = p.space(3);
            ui.add(ProgressBar::new(0.0, "Évaluation du dossier en cours"));
            muted(ui, "Le bilan s'affiche dès que l'évaluation est terminée.");
        });
        context_card(ui, st, workspace);
        return;
    };
    let a = &assessed.assessment;
    let kind = match a.verdict {
        Verdict::Anonymous => BannerKind::Success,
        Verdict::Reidentifiable { .. } => BannerKind::Warning,
        Verdict::Inconclusive => BannerKind::Info,
    };
    let line = bilan::verdict_line(a);
    let note = bilan::verdict_note(workspace.goal, a);
    Banner::new(kind, note).title(&line).show(ui);
    if !current {
        caption(
            ui,
            "Mise à jour de l'évaluation après vos dernières décisions…",
        );
    }
    criteria_card(ui, &assessed);
    let subjects = bilan::subjects(st, &assessed);
    if !subjects.is_empty() {
        fixes_card(ui, &subjects, current, fx);
    }
    guesses_card(ui, st, &assessed);
    context_card(ui, st, workspace);
}

/// Les trois critères, une ligne chacun.
fn criteria_card(ui: &mut Ui, assessed: &Assessed) {
    let p = palette();
    let a = &assessed.assessment;
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(2);
        card_title(ui, Icon::ShieldCheck, "Trois critères");
        for c in Criterion::ALL {
            control_row(ui, CONTROL_SM, |ui| {
                ui.spacing_mut().item_spacing.x = p.space(3);
                ui.allocate_ui_with_layout(
                    egui::vec2(CRITERION_W, CONTROL_SM),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new(c.label())
                                .font(font(Weight::Semibold, p.text_sm))
                                .color(p.heading),
                        );
                    },
                );
                let failing = a.summary(c).1 > 0;
                let (icon, ink) = if failing {
                    (Icon::TriangleAlert, p.error)
                } else {
                    (Icon::CircleCheck, p.accent)
                };
                icon.show(ui, ink, 16.0);
                body(ui, &bilan::criterion_line(a, c));
            });
        }
    });
}

/// Un sujet par échec, sa correction conseillée, puis « Tout corriger ».
fn fixes_card(ui: &mut Ui, subjects: &[SubjectView], current: bool, fx: &mut Vec<Effect>) {
    let p = palette();
    let stale = (!current).then_some("Patientez : l'évaluation est en cours de mise à jour.");
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::TriangleAlert, "À corriger");
        for (i, s) in subjects.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            subject(ui, i, s, stale, fx);
        }
        ui.separator();
        control_row(ui, CONTROL_SM, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            let go = super::guarded_button(
                ui,
                PillButton::primary("Tout corriger")
                    .icon(Icon::Sparkles)
                    .small(),
                stale,
            );
            if go.clicked() {
                fx.push(Effect::FixAll);
            }
            caption(
                ui,
                &format!(
                    "Applique la correction conseillée de chaque point. {} les annule toutes.",
                    shortcuts::UNDO.label()
                ),
            );
        });
    });
}

/// Un sujet : nom, critères en échec, raisons, indices, corrections.
fn subject(ui: &mut Ui, index: usize, s: &SubjectView, stale: Option<&str>, fx: &mut Vec<Effect>) {
    let p = palette();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = p.space(1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            ui.label(
                RichText::new(&s.label)
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.heading),
            );
            for c in &s.criteria {
                ui.add(Badge::error(c.label()).small());
            }
        });
        for r in &s.reasons {
            body(ui, r);
        }
        if let Some(c) = &s.clues {
            muted(ui, c);
        }
        if let Some(g) = &s.group {
            muted(ui, g);
        }
        let Some(((first, first_label), others)) = s.fixes.split_first() else {
            muted(
                ui,
                if s.fixed_with_table {
                    "La correction du tableau, proposée plus haut, corrige aussi ce groupe."
                } else {
                    "Aucune correction automatique : revenez à l'étape Vérifier."
                },
            );
            return;
        };
        control_row(ui, CONTROL_SM, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let w = (ui.available_width() - 220.0).max(160.0);
            super::truncated_text(
                ui,
                w,
                CONTROL_SM,
                RichText::new(format!("Correction : {first_label}"))
                    .font(font(Weight::Medium, p.text_sm))
                    .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = p.space(2);
                if !others.is_empty() {
                    let name = format!("Autres corrections, point {}", index + 1);
                    let r = super::guarded_button(
                        ui,
                        PillButton::ghost("Autre")
                            .icon(Icon::ChevronDown)
                            .small()
                            .accessible_label(&name),
                        stale,
                    );
                    if let Some(fix) = other_fixes_menu(&r, others) {
                        fx.push(Effect::ApplyFix(fix));
                    }
                }
                let name = format!("Appliquer : {first_label}");
                let apply = super::guarded_button(
                    ui,
                    PillButton::secondary("Appliquer")
                        .small()
                        .accessible_label(&name),
                    stale,
                );
                if apply.clicked() {
                    fx.push(Effect::ApplyFix(first.clone()));
                }
            });
        });
    });
}

/// Menu « Autre » : les autres corrections du sujet.
fn other_fixes_menu(response: &egui::Response, others: &[(Fix, String)]) -> Option<Fix> {
    let mut chosen = None;
    egui::Popup::menu(response).show(|ui| {
        ui.set_min_width(320.0);
        for (fix, label) in others {
            if ui.selectable_label(false, label).clicked() {
                chosen = Some(fix.clone());
            }
        }
    });
    chosen
}

/// « Ce que l'IA pourrait deviner », repliable.
fn guesses_card(ui: &mut Ui, st: &mut AnonymizeState, assessed: &Assessed) {
    let p = palette();
    let guesses = bilan::guesses(st, assessed);
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(2);
        let open = st.bilan.guesses_open;
        let icon = if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let label = format!("Ce que l'IA pourrait deviner ({})", guesses.len());
        if ui
            .add(PillButton::ghost(&label).icon(icon).small())
            .on_hover_text("Indices restés lisibles, personne par personne")
            .clicked()
        {
            st.bilan.guesses_open = !open;
        }
        if !open {
            return;
        }
        if guesses.is_empty() {
            muted(
                ui,
                "Rien à signaler : aucune personne isolable, aucune donnée sensible liée.",
            );
            return;
        }
        muted(
            ui,
            "Une IA croise ces indices pour deviner qui est concerné. Masquez-les ou rendez-les \
moins précis à l'étape Vérifier.",
        );
        for g in &guesses {
            body(ui, g);
        }
    });
}

/// Destinataire et questions facultatives de l'approche contextuelle.
fn context_card(ui: &mut Ui, st: &mut AnonymizeState, workspace: &mut Workspace) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::Settings, "Contexte de l'envoi");
        let recipients: Vec<(Recipient, &str)> =
            Recipient::ALL.iter().map(|r| (*r, r.label())).collect();
        control_row(ui, CONTROL_SM, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            body(ui, "Destinataire");
            let w = ui.available_width().min(RECIPIENT_W);
            Select::new(
                "destinataire",
                "Destinataire",
                &mut workspace.recipient,
                &recipients,
            )
            .width(w)
            .control_height(CONTROL_SM)
            .show(ui);
        });
        muted(
            ui,
            "Questions facultatives, reportées dans la fiche d'évaluation.",
        );
        let answers = [None, Some(true), Some(false)].map(|v| (v, answer_label(v)));
        for (i, q) in QUESTIONS.iter().enumerate() {
            let Some(value) = answer_mut(&mut st.bilan.answers, i) else {
                continue;
            };
            // Les questions dans une colonne, les réponses alignées à côté.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = p.space(3);
                let w = QUESTION_W.min(ui.available_width());
                ui.allocate_ui_with_layout(
                    egui::vec2(w, CONTROL_MD),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        // La colonne garde sa largeur quelle que soit la question.
                        ui.set_min_size(egui::vec2(w, CONTROL_MD));
                        body(ui, q);
                    },
                );
                segmented(ui, q, value, &answers);
            });
        }
        if st.bilan.answers.approach() == anon_core::Approach::Contextual {
            TextField::new(
                "Justification de vos réponses",
                &mut st.bilan.answers_justification,
            )
            .id_salt("justification-reponses")
            .placeholder("Pourquoi ces réponses écartent-elles un risque ?")
            .width(ui.available_width().min(560.0))
            .show(ui);
        }
        caption(ui, bilan::approach_line(&st.bilan.answers));
    });
}
