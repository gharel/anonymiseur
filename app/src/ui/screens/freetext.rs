//! Onglet « Texte libre » : original à gauche, anonymisé à droite.

use super::page_title;
use crate::ui::app::Effect;
use crate::ui::mock;
use crate::ui::state::freetext::{FreeTextNotice, FreeTextState};
use crate::ui::theme::palette;
use crate::ui::widgets::{Banner, Card, Icon, PillButton, TextField};
use egui::Ui;

/// Largeur à partir de laquelle les deux zones sont côte à côte.
const TWO_COLUMNS: f32 = 760.0;
const ROWS: usize = 12;

pub fn show(ui: &mut Ui, st: &mut FreeTextState, fx: &mut Vec<Effect>) {
    let p = palette();
    page_title(
        ui,
        "Texte libre",
        "Anonymisez un court texte avant de le coller dans une IA, puis restaurez sa réponse.",
    );
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        let original = |ui: &mut Ui, st: &mut FreeTextState| {
            TextField::new("Texte original", &mut st.original)
                .placeholder("Saisissez ou collez votre texte…")
                .id_salt("texte-original")
                .multiline(ROWS)
                .show(ui);
        };
        let anonymized = |ui: &mut Ui, st: &mut FreeTextState| {
            TextField::new("Texte anonymisé ou réponse de l'IA", &mut st.anonymized)
                .placeholder("Le texte anonymisé apparaît ici. Collez-y la réponse de l'IA pour la restaurer.")
                .id_salt("texte-anonymise")
                .multiline(ROWS)
                .show(ui);
        };
        if ui.available_width() >= TWO_COLUMNS {
            ui.columns(2, |cols| {
                if let [a, b] = cols {
                    original(a, st);
                    anonymized(b, st);
                }
            });
        } else {
            original(ui, st);
            anonymized(ui, st);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            if ui
                .add(PillButton::primary("Anonymiser").icon(Icon::Lock))
                .clicked()
            {
                // Branchement futur : anonymisation par anon-engine.
                st.anonymize(mock::anonymize_text);
            }
            if ui
                .add(PillButton::secondary("Restaurer").icon(Icon::Undo))
                .clicked()
            {
                st.restore(mock::restore_text);
            }
            if ui
                .add(PillButton::ghost("Copier le texte anonymisé").icon(Icon::Copy))
                .clicked()
                && let Some(text) = st.copy()
            {
                fx.push(Effect::Copy(text));
            }
        });
    });
    if let Some(n) = st.notice {
        let msg = n.message();
        let banner = match n {
            FreeTextNotice::NothingToAnonymize | FreeTextNotice::NothingToRestore => {
                Banner::warning(&msg)
            }
            _ => Banner::success(&msg),
        };
        banner.show(ui);
    }
    Banner::info(
        "Le texte n'est jamais enregistré. Les correspondances de cette page sont oubliées à la \
fermeture de l'application.",
    )
    .show(ui);
}
