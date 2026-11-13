//! Onglet « Texte libre » : original à gauche, anonymisé à droite. Les deux
//! zones occupent la hauteur de la fenêtre (au moins 12 lignes).

use super::{control_row, page_title};
use crate::ui::app::Effect;
use crate::ui::state::freetext::FreeTextState;
use crate::ui::theme::{CONTROL_MD, palette};
use crate::ui::widgets::field::area_height_for_rows;
use crate::ui::widgets::{Banner, Card, Icon, PillButton, TextField};
use egui::Ui;

/// Largeur à partir de laquelle les deux zones sont côte à côte.
const TWO_COLUMNS: f32 = 760.0;
/// Lignes visibles au minimum dans chaque zone.
const MIN_ROWS: usize = 12;
/// Hauteur occupée autour des zones : marges, titre, libellés, boutons.
const CHROME_H: f32 = 244.0;
/// Hauteur d'un bandeau de résultat affiché au-dessus de la carte.
const NOTICE_H: f32 = 64.0;

/// Hauteur des zones pour une hauteur de fenêtre utile `view_h`.
pub fn area_height(view_h: f32, with_notice: bool) -> f32 {
    let notice = if with_notice { NOTICE_H } else { 0.0 };
    (view_h - CHROME_H - notice).max(area_height_for_rows(MIN_ROWS))
}

pub fn show(ui: &mut Ui, st: &mut FreeTextState, view_h: f32, fx: &mut Vec<Effect>) {
    let p = palette();
    page_title(
        ui,
        "Texte libre",
        "Anonymisez un court texte avant de le coller dans une IA, puis restaurez sa réponse.",
    );
    if let Some(n) = st.notice {
        let msg = n.message();
        let banner = if n.is_problem() {
            Banner::warning(&msg)
        } else {
            Banner::success(&msg)
        };
        banner.show(ui);
    }
    let two = ui.available_width() >= TWO_COLUMNS;
    let h = if two {
        area_height(view_h, st.notice.is_some())
    } else {
        area_height_for_rows(MIN_ROWS)
    };
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        let original = |ui: &mut Ui, st: &mut FreeTextState| {
            TextField::new("Texte original", &mut st.original)
                .placeholder("Saisissez ou collez votre texte…")
                .id_salt("texte-original")
                .area_height(h)
                .monospace()
                .show(ui);
        };
        let anonymized = |ui: &mut Ui, st: &mut FreeTextState| {
            TextField::new("Texte anonymisé ou réponse de l'IA", &mut st.anonymized)
                .placeholder(
                    "Le texte anonymisé apparaît ici. Collez-y la réponse de l'IA pour la restaurer.",
                )
                .id_salt("texte-anonymise")
                .area_height(h)
                .monospace()
                .show(ui);
        };
        if two {
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
        control_row(ui, CONTROL_MD, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            if ui
                .add(PillButton::primary("Anonymiser").icon(Icon::Lock))
                .clicked()
            {
                fx.push(Effect::FreeAnonymize);
            }
            if ui
                .add(PillButton::secondary("Restaurer").icon(Icon::Undo))
                .clicked()
            {
                fx.push(Effect::FreeRestore);
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
    Banner::info(
        "Le texte n'est jamais enregistré. Les balises sont celles de la session en cours : une \
même valeur garde la même balise dans les fichiers et ici. « Nouvelle session » les oublie.",
    )
    .show(ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::widgets::field::area_line_height;

    #[test]
    fn areas_fill_the_window_with_at_least_twelve_lines() {
        // 1280 × 800 : 800 − en-tête (72) − pied (40).
        let h = area_height(688.0, false);
        assert!(h >= area_height_for_rows(MIN_ROWS));
        assert!(h / area_line_height() >= 12.0, "{h}");
        // Fenêtre basse : jamais moins de 12 lignes.
        assert_eq!(area_height(300.0, true), area_height_for_rows(MIN_ROWS));
        // Plus de place, zones plus hautes.
        assert!(area_height(1000.0, false) > h);
    }
}
