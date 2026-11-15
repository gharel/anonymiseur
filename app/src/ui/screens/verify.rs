//! Étape « Vérifier » : le tableau des valeurs trouvées (tableau de la V1, que
//! le lot 7b remplacera par la vue dans le document et la file « À décider »),
//! les images avec leurs zones masquées, la liste « toujours masquer ».
//! Chaque décision passe par l'état, qui la rend annulable (Ctrl+Z).

use super::images::images_card;
use super::{busy_card, card_title, control_row, empty_state};
use crate::ui::app::{Effect, SEARCH_FIELD};
use crate::ui::runner::Shared;
use crate::ui::service::Rejected;
use crate::ui::shortcuts::{self, with_shortcut};
use crate::ui::state::anonymize::{AnonymizeState, Busy, SortKey, needs_confirmation};
use crate::ui::state::generalize::rule_label;
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette};
use crate::ui::widgets::badge::{FAMILIES, family_label, family_tint};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::field::tag_font;
use crate::ui::widgets::nav::filter_chip;
use crate::ui::widgets::{
    Badge, Banner, Card, CheckState, Checkbox, Column, DataTable, Icon, PillButton, RowTone,
    Select, TextField, body, caption, muted,
};
use anon_core::{EntityKind, Session};
use egui::{RichText, Sense, Ui};

/// Identifiant du tableau de vérification.
pub const TABLE_ID: &str = "detections";
/// Hauteur d'une ligne du tableau.
const ROW_H: f32 = 44.0;
/// Lignes visibles au moins, et au plus, avant défilement du tableau.
const MIN_ROWS: f32 = 5.0;
const MAX_ROWS: f32 = 12.0;
/// Hauteur occupée au-dessus des lignes : marges, recherche, filtres, en-tête.
const TABLE_CHROME_H: f32 = 192.0;

/// Hauteur du tableau pour une zone de contenu haute de `view_h` : la première
/// valeur est visible sans défiler, le tableau remplit l'écran.
pub fn table_height(view_h: f32) -> f32 {
    (view_h - TABLE_CHROME_H).clamp(ROW_H * MIN_ROWS, ROW_H * MAX_ROWS)
}

pub fn show(
    ui: &mut Ui,
    st: &mut AnonymizeState,
    session: &Session,
    progress: &Shared,
    view_h: f32,
    fx: &mut Vec<Effect>,
) {
    if st.busy == Some(Busy::Analyzing) {
        busy_card(
            ui,
            Busy::Analyzing.label(),
            &st.document_names(),
            progress,
            fx,
        );
        return;
    }
    if st.analysis.is_none() {
        Card::new().show(ui, |ui| {
            if empty_state(
                ui,
                Icon::Inbox,
                "Aucun document analysé dans ce dossier.",
                Some(("Ajouter des fichiers", Icon::Plus)),
            ) {
                fx.push(Effect::PickDocuments);
            }
        });
        if rejected_banner(ui, &st.rejected) {
            st.dismiss_rejected();
        }
        return;
    }
    // Un seul message : les fichiers non pris en compte, s'il y en a.
    if rejected_banner(ui, &st.rejected) {
        st.dismiss_rejected();
    }
    table_card(ui, st, session, view_h);
    images_card(ui, st);
    always_card(ui, st, fx);
}

/// Fichiers refusés ou illisibles, avec la raison. Renvoie vrai si
/// l'utilisateur referme le message.
pub fn rejected_banner(ui: &mut Ui, rejected: &[Rejected]) -> bool {
    if rejected.is_empty() {
        return false;
    }
    let text = rejected
        .iter()
        .map(|r| format!("{} : {}", r.name, r.reason))
        .collect::<Vec<_>>()
        .join("\n");
    let title = count_label(
        rejected.len(),
        "fichier non pris en compte",
        "fichiers non pris en compte",
    );
    Banner::warning(&text).title(&title).show(ui);
    control_row(ui, CONTROL_SM, |ui| {
        ui.add(PillButton::ghost("Masquer ce message").small())
            .clicked()
    })
}

/// Liste « toujours masquer » : termes cherchés dans chaque analyse.
fn always_card(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::Plus, "Toujours masquer");
        muted(
            ui,
            "Termes cherchés dans tous les documents, même si la détection les manque (nom de projet, client, lieu-dit). L'analyse est relancée à chaque modification.",
        );
        let kinds = kind_options();
        let error = st.always_error.map(|e| e.message());
        let enter = control_row(ui, CONTROL_MD, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let field_w = (ui.available_width() - 380.0).clamp(200.0, 420.0);
            let r = TextField::new("Terme à toujours masquer", &mut st.always_term)
                .placeholder("Terme à toujours masquer")
                .id_salt("terme-toujours")
                .hide_label()
                .error(error.map(|_| ""))
                .width(field_w)
                .show_control(ui);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            Select::new("type-toujours", "Type du terme", &mut st.always_kind, &kinds)
                .width(200.0)
                .show(ui);
            let add = ui.add(PillButton::secondary("Ajouter à la liste").icon(Icon::Plus));
            enter || add.clicked()
        });
        field_error(ui, error);
        if enter && let Ok(Some(job)) = st.add_always() {
            fx.push(Effect::Analyze(job));
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
            && let Some(job) = st.remove_always(i)
        {
            fx.push(Effect::Analyze(job));
        }
    });
}

/// Menu balise / forme moins précise d'une ligne : `Some(true)` pour la forme moins précise.
fn treatment_menu(response: &egui::Response, tag: &str, general: &str) -> Option<bool> {
    let mut chosen = None;
    egui::Popup::menu(response).show(|ui| {
        ui.set_min_width(300.0);
        if ui
            .selectable_label(false, format!("Balise {tag}"))
            .clicked()
        {
            chosen = Some(false);
        }
        if ui
            .selectable_label(
                false,
                format!("Rendre moins précis : {general} (sans retour)"),
            )
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

/// Types proposés dans les sélecteurs, avec leur nom.
fn kind_options() -> Vec<(EntityKind, &'static str)> {
    EntityKind::ALL.iter().map(|k| (*k, k.label())).collect()
}

/// Message d'erreur sous une ligne de contrôles (le champ n'affiche que son bord rouge).
fn field_error(ui: &mut Ui, error: Option<&str>) {
    let Some(msg) = error else { return };
    let p = palette();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        Icon::TriangleAlert.show(ui, p.error, 14.0);
        ui.label(
            RichText::new(msg)
                .font(font(Weight::Medium, p.text_xs))
                .color(p.error),
        );
    });
}

/// Colonnes du tableau de vérification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Kind,
    Value,
    Excerpt,
    Count,
    Case,
    Replacement,
    Never,
}

/// Largeur à partir de laquelle le tableau montre la colonne Extrait.
const WIDE_TABLE: f32 = 960.0;

/// « 3 fois ».
fn times(n: usize) -> String {
    count_label(n, "fois", "fois")
}

fn table_card(ui: &mut Ui, st: &mut AnonymizeState, session: &Session, view_h: f32) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        review_toolbar(ui, st, session);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(p.space(2), p.space(2));
            let all = st.family_filter.is_empty();
            let brand = p.brand;
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

        // Fenêtre étroite : sans la colonne Extrait (l'extrait reste en
        // infobulle de la valeur), pour que la valeur reste lisible.
        let wide = ui.available_width() >= WIDE_TABLE;
        let mut cols = vec![
            (Col::Kind, Column::fixed("Type", 190.0).sortable()),
            (Col::Value, Column::fill("Valeur", 1.0).sortable()),
        ];
        if wide {
            cols.push((Col::Excerpt, Column::fill("Extrait", 1.2)));
        }
        cols.extend([
            (Col::Count, Column::fixed("Nombre", 92.0).right().sortable()),
            (Col::Case, Column::fixed("Majuscules", 104.0)),
            (Col::Replacement, Column::fixed("Remplacée par", 176.0)),
            (Col::Never, Column::fixed("", 44.0)),
        ]);
        let columns: Vec<Column> = cols.iter().map(|(_, c)| *c).collect();
        let col_of = |i: usize| cols.get(i).map(|(c, _)| *c);
        let index_of = |c: Col| cols.iter().position(|(x, _)| *x == c);
        let sorted = st.sort.and_then(|s| {
            let col = match s.key {
                SortKey::Kind => Col::Kind,
                SortKey::Value => Col::Value,
                SortKey::Occurrences => Col::Count,
            };
            index_of(col).map(|i| (i, s.ascending))
        });
        let jump = std::mem::take(&mut st.jump)
            .then_some(st.focus_row)
            .flatten();
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
            DataTable::new(TABLE_ID, &columns)
                .checkable(header)
                .sorted(sorted)
                .focus_row(jump)
                .max_height(table_height(view_h))
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
                        let (Some(c), Some(col)) = (row_of(i), col_of(col)) else {
                            return;
                        };
                        match col {
                            Col::Kind => {
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
                            Col::Value => {
                                if needs_confirmation(c) {
                                    Icon::TriangleAlert.show(ui, p.data.ink, 16.0);
                                }
                                let mut text = RichText::new(&c.value)
                                    .font(font(Weight::Semibold, p.text_sm))
                                    .color(p.heading);
                                if !c.selected {
                                    text = text.color(p.text_secondary);
                                }
                                let r = ui.add(egui::Label::new(text).truncate());
                                let mut tip = c.value.clone();
                                if needs_confirmation(c) {
                                    tip.push_str(
                                        "\nValeur courte ou incertaine : vérifiez-la avant de continuer.",
                                    );
                                }
                                if !c.variants.is_empty() {
                                    tip.push_str(&format!(
                                        "\nAutres formes : {}",
                                        c.variants.join(", ")
                                    ));
                                }
                                if !wide && !c.context.is_empty() {
                                    tip.push_str(&format!("\nExtrait : {}", c.context));
                                }
                                r.on_hover_text(tip);
                            }
                            Col::Excerpt => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&c.context)
                                            .font(font(Weight::Regular, p.text_sm))
                                            .color(p.text_secondary),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(c.context.as_str());
                            }
                            Col::Count => {
                                body(ui, &times(c.occurrences));
                            }
                            Col::Case => {
                                let label = format!("Respecter les majuscules de {}", c.value);
                                if ui
                                    .add(Checkbox::bare(c.case_sensitive, &label))
                                    .on_hover_text("Respecter les majuscules")
                                    .clicked()
                                {
                                    case_change = Some((c.id, !c.case_sensitive));
                                }
                            }
                            Col::Replacement => {
                                let tag = st.tag_preview.get(&c.id).map(|t| t.to_string());
                                let general = st.generalization_for(c.id);
                                let chosen = st.generalized(c.id);
                                let shown = match (&tag, chosen) {
                                    (_, Some(g)) if c.selected => Some(
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(g)
                                                    .font(font(Weight::Medium, p.text_sm))
                                                    .color(p.data.ink),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(format!(
                                            "Moins précis ({}) : sans retour à la restauration",
                                            rule_label(c.kind)
                                        )),
                                    ),
                                    (Some(t), _) => Some(ui.label(
                                        RichText::new(t).font(tag_font()).color(p.accent),
                                    )),
                                    _ => {
                                        caption(ui, "visible");
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
                                        .on_hover_text("Balise, ou forme moins précise")
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if let Some(on) = treatment_menu(&r, t, &g) {
                                        general_change = Some((c.id, on));
                                    }
                                }
                            }
                            Col::Never => {
                                let label = format!("Ne jamais masquer {}", c.value);
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
        if let Some(col) = out.sort_clicked {
            let key = match col_of(col) {
                Some(Col::Kind) => Some(SortKey::Kind),
                Some(Col::Value) => Some(SortKey::Value),
                Some(Col::Count) => Some(SortKey::Occurrences),
                _ => None,
            };
            if let Some(k) = key {
                st.toggle_sort(k);
            }
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
            let filtered = !st.search.trim().is_empty() || !st.family_filter.is_empty();
            if filtered {
                if empty_state(
                    ui,
                    Icon::Search,
                    "Aucune valeur ne correspond à la recherche ou au filtre.",
                    Some(("Tout afficher", Icon::RefreshCw)),
                ) {
                    st.search.clear();
                    st.clear_filter();
                }
            } else if empty_state(
                ui,
                Icon::Inbox,
                "Aucune donnée sensible trouvée dans ces documents.",
                Some(("Ajouter un terme", Icon::Plus)),
            ) {
                ui.memory_mut(|m| {
                    m.request_focus(crate::ui::widgets::field::field_id("terme-manuel"));
                });
            }
        }

        // Ajout manuel.
        ui.add_space(p.space(1));
        let kinds = kind_options();
        let error = st.manual_error.map(|e| e.message());
        let add = control_row(ui, CONTROL_MD, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let field_w = (ui.available_width() - 380.0).clamp(200.0, 420.0);
            let r = TextField::new("Terme à ajouter", &mut st.manual_term)
                .placeholder("Ajouter un terme absent de la liste")
                .id_salt("terme-manuel")
                .hide_label()
                .error(error.map(|_| ""))
                .width(field_w)
                .show_control(ui);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            Select::new(
                "type-manuel",
                "Type du terme ajouté",
                &mut st.manual_kind,
                &kinds,
            )
            .width(200.0)
            .show(ui);
            let add = ui.add(PillButton::secondary("Ajouter").icon(Icon::Plus));
            add.clicked() || enter
        });
        field_error(ui, error);
        if add {
            let _ = st.add_manual(session);
        }
        if !st.never.is_empty() {
            caption(
                ui,
                &format!(
                    "Jamais masqués dans cette session : {}",
                    st.never.join(", ")
                ),
            );
        }
    });
}

/// Recherche, valeur suivante à confirmer, cocher ou décocher les lignes affichées.
fn review_toolbar(ui: &mut Ui, st: &mut AnonymizeState, session: &Session) {
    let p = palette();
    let next = (st.pending_confirmations() > 0).then(|| {
        PillButton::secondary("Suivante à confirmer")
            .icon(Icon::ArrowDown)
            .small()
    });
    let uncheck = PillButton::ghost("Tout décocher").small();
    let check = PillButton::ghost("Tout cocher").small();
    control_row(ui, CONTROL_MD, |ui| {
        let gap = p.space(3);
        ui.spacing_mut().item_spacing.x = gap;
        // La recherche prend la place que laissent les boutons de droite.
        let right = next
            .as_ref()
            .map_or(0.0, |b| b.natural_width(ui) + p.space(2))
            + uncheck.natural_width(ui)
            + p.space(2)
            + check.natural_width(ui);
        let w = (ui.available_width() - right - 2.0 * gap - CONTROL_SM)
            .clamp(SEARCH_MIN_W, SEARCH_MAX_W);
        TextField::new("Rechercher dans le tableau", &mut st.search)
            .placeholder("Rechercher une valeur ou un extrait")
            .id_salt(SEARCH_FIELD)
            .hide_label()
            .width(w)
            .show_control(ui)
            .on_hover_text(with_shortcut("Rechercher dans le tableau", shortcuts::FIND));
        if !st.search.is_empty()
            && icon_button(ui, Icon::X, "Effacer la recherche", p.text_secondary).clicked()
        {
            st.search.clear();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            if let Some(b) = next
                && ui
                    .add(b)
                    .on_hover_text(
                        "Montre la prochaine valeur courte ou incertaine, surlignée en jaune",
                    )
                    .clicked()
            {
                st.focus_next_pending();
                st.jump = true;
            }
            // Le résumé (« 42 valeurs masquées sur 57 ») est le message de la
            // barre d'action : il n'est pas répété ici.
            if ui
                .add(uncheck)
                .on_hover_text("Décoche les lignes affichées (après recherche et filtre)")
                .clicked()
            {
                st.set_all_visible(false, session);
            }
            if ui
                .add(check)
                .on_hover_text("Coche les lignes affichées (après recherche et filtre)")
                .clicked()
            {
                st.set_all_visible(true, session);
            }
        });
    });
}

/// Largeur du champ de recherche du tableau, au moins et au plus.
const SEARCH_MIN_W: f32 = 180.0;
const SEARCH_MAX_W: f32 = 380.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_fills_the_screen_within_bounds() {
        // 1280 × 800 : en-tête, barre du dossier, barre d'action et pied retirés.
        let h = table_height(560.0);
        assert!((ROW_H * MIN_ROWS..=ROW_H * MAX_ROWS).contains(&h), "{h}");
        assert_eq!(table_height(100.0), ROW_H * MIN_ROWS);
        assert_eq!(table_height(5000.0), ROW_H * MAX_ROWS);
        assert_eq!(times(1), "1 fois");
        assert_eq!(times(3), "3 fois");
    }
}
