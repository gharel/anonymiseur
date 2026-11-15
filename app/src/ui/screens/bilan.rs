//! Étape « Bilan » : **résumé provisoire** du lot 7a (valeurs masquées, à
//! confirmer, indices encore visibles, estimation du risque de la V1). Isolé
//! dans ce module : le lot 8 le remplace par le bilan des trois critères
//! (isolement, recoupement, déduction) et ses corrections.

use super::{card_title, control_row, page_title};
use crate::ui::state::bilan::Summary;
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{CONTROL_SM, Weight, font, palette};
use crate::ui::widgets::{Badge, Card, Icon, body, muted, overline};
use anon_engine::RiskLevel;
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, s: &Summary) {
    let p = palette();
    page_title(
        ui,
        "Bilan avant envoi",
        "Bilan provisoire : il compte les valeurs masquées et les indices restés visibles. \
Il ne dit pas encore si une personne reste reconnaissable.",
    );
    Card::new().show(ui, |ui| {
        ui.columns(3, |cols| {
            if let [a, b, c] = cols {
                let masked = if s.less_precise > 0 {
                    format!(
                        "dont {} moins {}",
                        s.less_precise,
                        if s.less_precise == 1 {
                            "précise"
                        } else {
                            "précises"
                        }
                    )
                } else {
                    count_label(s.documents, "document", "documents")
                };
                stat(a, "Valeurs masquées", s.masked, &masked);
                stat(
                    b,
                    "À confirmer",
                    s.to_confirm,
                    "valeurs courtes ou incertaines",
                );
                let kinds = count_label(s.clues.len(), "type d'indice", "types d'indices");
                stat(c, "Indices encore visibles", s.clue_total(), &kinds);
            }
        });
    });
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        control_row(ui, CONTROL_SM, |ui| {
            card_title(ui, Icon::ShieldCheck, "Risque de réidentification estimé");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let level = s.level();
                let badge = match level {
                    RiskLevel::Faible => Badge::brand(level.label()),
                    RiskLevel::Modere => Badge::warning(level.label()),
                    RiskLevel::Eleve => Badge::error(level.label()),
                };
                ui.add(badge);
            });
        });
        muted(
            ui,
            "Estimation de la version 1 : elle compte les indices (dates, âges, fonctions, lieux, \
montants) restés visibles. Combinés, ils peuvent suffire à reconnaître une personne.",
        );
        if s.clues.is_empty() {
            body(
                ui,
                "Aucun indice ne reste visible : tous ceux trouvés sont masqués.",
            );
        } else {
            overline(ui, "Indices restés visibles");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(p.space(2), p.space(2));
                for (kind, n) in &s.clues {
                    ui.add(Badge::entity(*kind).small());
                    ui.label(
                        RichText::new(count_label(*n, "fois", "fois"))
                            .font(font(Weight::Medium, p.text_sm))
                            .color(p.text),
                    );
                    ui.add_space(p.space(3));
                }
            });
            body(
                ui,
                "Pour les masquer, revenez à l'étape Vérifier et cochez-les, ou rendez-les moins \
précis (une date devient son année, un âge sa tranche de dix ans).",
            );
        }
        if s.zones > 0 {
            muted(
                ui,
                &format!(
                    "{} sur les images.",
                    count_label(s.zones, "zone tracée à la main", "zones tracées à la main")
                ),
            );
        }
    });
}

/// Chiffre clé : libellé, nombre, précision.
fn stat(ui: &mut Ui, label: &str, n: usize, detail: &str) {
    let p = palette();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        overline(ui, label);
        ui.label(
            RichText::new(n.to_string())
                .font(font(Weight::Bold, p.text_xl))
                .color(p.heading),
        );
        muted(ui, detail);
    });
}
