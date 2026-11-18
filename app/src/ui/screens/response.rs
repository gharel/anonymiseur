//! Étape « Réponse » (maquette E) : la réponse collée de l'IA et sa version
//! restaurée, côte à côte, les valeurs remises surlignées. La clé reste
//! ouverte en mémoire : rien n'est redemandé. Une clé ouverte depuis l'accueil
//! s'ouvre sans mot de passe s'il est retenu sur cet ordinateur ; sinon il est
//! demandé une fois, avec « Retenir sur cet ordinateur ». Les balises
//! inconnues de la clé sont signalées, jamais devinées. Exports (copie, `.md`,
//! `.docx`), date de conservation et destruction de la clé. En dessous, les
//! fichiers renvoyés par l'IA se restaurent aussi.

use super::{busy_card, card_title, control_row, copy_button, file_row, truncated_text};
use crate::ui::app::{Effect, RestoredFormat};
use crate::ui::runner::Shared;
use crate::ui::screens::send::french_date;
use crate::ui::service::{file_name, unused_tags};
use crate::ui::shortcuts;
use crate::ui::state::deanonymize::{Busy, DeanonymizeState, MapStatus, tag_ranges};
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette, radius};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::field::{Mark, area_height_for_rows, marked_label, mono_format};
use crate::ui::widgets::{
    Banner, Card, Checkbox, Icon, PillButton, TextField, body, caption, muted, subheading,
};
use egui::{Frame, Margin, RichText, Ui};

/// Identifiant du champ « mot de passe de la clé ».
pub const PASSWORD_FIELD: &str = "mdp-cle";
/// Identifiant de la zone où coller la réponse de l'IA.
pub const RESPONSE_FIELD: &str = "reponse-ia";
/// Lignes visibles au moins dans chaque colonne.
const MIN_ROWS: usize = 12;
/// Hauteur occupée autour des colonnes : marges, ligne de la clé, titres, message.
const CHROME_H: f32 = 200.0;
/// Largeur à partir de laquelle les deux colonnes sont côte à côte.
const TWO_COLUMNS: f32 = 720.0;
/// Côté de l'icône d'un titre de colonne.
const HEAD_ICON: f32 = 20.0;
/// Titre de la colonne de droite.
const RESTORED_TITLE: &str = "Réponse restaurée";

/// Message après la destruction d'une clé.
pub const DESTROYED: &str = "Les réponses de l'IA ne peuvent plus être restaurées avec cette \
clé. Vos documents d'origine restent des données personnelles : conservez-les ou supprimez-les \
selon vos règles.";

/// Hauteur des deux zones de texte pour une zone de contenu haute de `view_h`.
pub fn area_height(view_h: f32) -> f32 {
    (view_h - CHROME_H).max(area_height_for_rows(MIN_ROWS))
}

/// Ce que l'écran doit savoir en plus de l'état de la clé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseContext {
    /// Date du jour, `AAAA-MM-JJ` (rappel de conservation).
    pub today: String,
    /// Le trousseau du système est disponible (« Retenir sur cet ordinateur »).
    pub keychain: bool,
}

pub fn show(
    ui: &mut Ui,
    st: &mut DeanonymizeState,
    ctx: &ResponseContext,
    progress: &Shared,
    view_h: f32,
    fx: &mut Vec<Effect>,
) {
    match st.busy {
        Some(Busy::Opening) => {
            busy_card(ui, "Ouverture de la clé", &[], progress, fx);
            return;
        }
        Some(Busy::Restoring) => {
            let names: Vec<String> = st.input_files.iter().map(|f| f.name.clone()).collect();
            busy_card(ui, "Restauration des fichiers", &names, progress, fx);
            return;
        }
        None => {}
    }
    match st.map.clone() {
        MapStatus::None => {
            if st.destroyed {
                Banner::success(DESTROYED).title("Clé détruite").show(ui);
            }
            Card::new().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = palette().space(3);
                card_title(ui, Icon::KeyRound, "Clé de restauration");
                muted(
                    ui,
                    "Ouvrez la clé (.anonmap) créée quand les documents ont été masqués.",
                );
                if ui
                    .add(PillButton::secondary("Ouvrir une clé").icon(Icon::FolderOpen))
                    .clicked()
                {
                    fx.push(Effect::PickKey);
                }
            });
        }
        MapStatus::Locked { name, .. } => unlock_card(ui, st, &name, ctx, fx),
        MapStatus::Unlocked { .. } => {
            key_bar(ui, st, ctx, fx);
            side_by_side(ui, st, view_h, fx);
            message(ui, st);
            files_card(ui, st, fx);
        }
    }
}

fn unlock_card(
    ui: &mut Ui,
    st: &mut DeanonymizeState,
    name: &str,
    ctx: &ResponseContext,
    fx: &mut Vec<Effect>,
) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        card_title(ui, Icon::KeyRound, "Ouvrir la clé de restauration");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            Icon::Lock.show(ui, p.text_secondary, 18.0);
            body(ui, name);
        });
        let error = st.unlock_error.as_deref();
        let r = TextField::password("Mot de passe de la clé", &mut st.password)
            .id_salt(PASSWORD_FIELD)
            .error(error)
            .width(ui.available_width().min(420.0))
            .show(ui);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let remember = st.remember && ctx.keychain;
        if ui
            .add(Checkbox::new(remember, "Retenir sur cet ordinateur").enabled(ctx.keychain))
            .clicked()
            && ctx.keychain
        {
            st.remember = !remember;
        }
        if !ctx.keychain {
            caption(
                ui,
                "Option indisponible : le trousseau du système n'est pas accessible.",
            );
        }
        let blocker = st.unlock_blocker();
        control_row(ui, CONTROL_MD, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            let go = super::guarded_button(
                ui,
                PillButton::primary("Ouvrir la clé").icon(Icon::LockOpen),
                blocker,
            );
            if (go.clicked() || enter) && blocker.is_none() {
                fx.push(Effect::Unlock);
            }
            if ui.add(PillButton::ghost("Changer de clé")).clicked() {
                fx.push(Effect::PickKey);
            }
        });
    });
}

/// En tête : la clé ouverte, sa date de conservation (rappel si elle est
/// dépassée) et « Détruire la clé », avec sa confirmation.
fn key_bar(ui: &mut Ui, st: &mut DeanonymizeState, ctx: &ResponseContext, fx: &mut Vec<Effect>) {
    let p = palette();
    let expired = st.expired_since(&ctx.today).map(french_date);
    if let Some(until) = &expired {
        Banner::warning(
            "Une clé gardée plus longtemps que prévu prolonge le traitement des données. \
Détruisez-la si vous n'en avez plus besoin.",
        )
        .title(&format!(
            "La durée de conservation de cette clé est dépassée depuis le {until}."
        ))
        .show(ui);
    }
    let name = st.key_name().unwrap_or_default().to_owned();
    let until = st.keep_until().map(french_date);
    let destroyable = st.can_destroy() && !st.destroy_confirm;
    control_row(ui, CONTROL_SM, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(2);
        Icon::KeyRound.show(ui, p.accent, 18.0);
        let w = (ui.available_width() - 360.0).max(120.0);
        truncated_text(
            ui,
            w,
            CONTROL_SM,
            RichText::new(format!("Clé ouverte : {name}"))
                .font(font(Weight::Medium, p.text_sm))
                .color(p.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            if destroyable {
                let button = if expired.is_some() {
                    PillButton::secondary("Détruire la clé")
                } else {
                    PillButton::ghost("Détruire la clé")
                };
                if ui
                    .add(button.icon(Icon::Trash).small())
                    .on_hover_text("Supprimer le fichier de la clé et son mot de passe retenu")
                    .clicked()
                {
                    st.destroy_confirm = true;
                }
            }
            if let (Some(until), None) = (&until, &expired) {
                caption(ui, &format!("À conserver jusqu'au {until}"));
            }
        });
    });
    if st.destroy_confirm && st.can_destroy() {
        destroy_confirmation(ui, st, fx);
    }
}

/// Confirmation de la destruction de la clé.
fn destroy_confirmation(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Banner::error(
        "Le fichier de la clé est supprimé et son mot de passe oublié par cet ordinateur : \
aucune réponse ne pourra plus être restaurée. Vos documents d'origine restent des données \
personnelles.",
    )
    .title("Détruire définitivement la clé ?")
    .show(ui);
    control_row(ui, CONTROL_SM, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        if ui
            .add(
                PillButton::primary("Détruire")
                    .icon(Icon::Trash)
                    .small()
                    .accessible_label("Confirmer la destruction de la clé"),
            )
            .clicked()
        {
            fx.push(Effect::DestroyKey);
        }
        if ui.add(PillButton::ghost("Garder la clé").small()).clicked() {
            st.destroy_confirm = false;
        }
    });
}

/// Largeur d'un titre de colonne (icône, titre, espaces) suivi d'actions
/// larges de `actions_w`, sur une seule ligne.
fn head_width(ui: &Ui, title: &str, actions_w: f32) -> f32 {
    let p = palette();
    let title_w = ui
        .painter()
        .layout_no_wrap(
            title.to_owned(),
            font(Weight::Semibold, p.text_md),
            p.heading,
        )
        .size()
        .x;
    HEAD_ICON + 2.0 * p.space(2) + title_w + actions_w
}

/// Titre de colonne : icône, titre, actions à droite (larges de `actions_w`).
/// Si le tout ne tient pas sur la ligne, les actions passent dessous, à droite.
fn column_head(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    actions_w: f32,
    actions: impl FnOnce(&mut Ui),
) {
    let p = palette();
    let one_line = head_width(ui, title, actions_w) <= ui.available_width();
    let right = egui::Layout::right_to_left(egui::Align::Center);
    let mut actions = Some(actions);
    control_row(ui, CONTROL_MD, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(2);
        icon.show(ui, p.accent, HEAD_ICON);
        subheading(ui, title);
        if one_line && let Some(add) = actions.take() {
            ui.with_layout(right, add);
        }
    });
    if let Some(add) = actions.take() {
        control_row(ui, CONTROL_MD, |ui| {
            ui.with_layout(right, add);
        });
    }
}

/// Boutons « Copier », « .md », « .docx » de la réponse restaurée ; `compact` :
/// « .md » et « .docx » réduits à leur icône (nom accessible et infobulle
/// « Enregistrer en .md »).
fn restored_buttons(compact: bool) -> [PillButton<'static>; 3] {
    [
        PillButton::secondary("Copier").icon(Icon::Copy).small(),
        PillButton::ghost(".md")
            .icon(Icon::Download)
            .small()
            .accessible_label("Enregistrer en .md")
            .icon_only(compact),
        PillButton::ghost(".docx")
            .icon(Icon::Download)
            .small()
            .accessible_label("Enregistrer en .docx")
            .icon_only(compact),
    ]
}

/// Largeur des boutons de la réponse restaurée, espaces compris.
fn restored_buttons_width(ui: &Ui, compact: bool) -> f32 {
    let gap = palette().space(2);
    let buttons = restored_buttons(compact);
    buttons.iter().map(|b| b.natural_width(ui)).sum::<f32>()
        + gap * buttons.len().saturating_sub(1) as f32
}

/// Vrai si « .md » et « .docx » doivent se réduire à leur icône pour que le
/// titre « Réponse restaurée » et les trois boutons tiennent sur une ligne.
fn restored_head_compact(ui: &Ui) -> bool {
    head_width(ui, RESTORED_TITLE, restored_buttons_width(ui, false)) > ui.available_width()
}

fn side_by_side(ui: &mut Ui, st: &mut DeanonymizeState, view_h: f32, fx: &mut Vec<Effect>) {
    let p = palette();
    let h = area_height(view_h);
    let paste = shortcuts::PASTE_RESPONSE.label();
    let pasted = |ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>| {
        ui.spacing_mut().item_spacing.y = p.space(2);
        let coller = PillButton::secondary("Coller")
            .icon(Icon::ClipboardPaste)
            .small()
            .kbd(&paste);
        let coller_w = coller.natural_width(ui);
        column_head(ui, Icon::Sparkles, "Réponse de l'IA", coller_w, |ui| {
            if ui
                .add(coller)
                .on_hover_text("Colle la réponse de l'IA et la restaure aussitôt")
                .clicked()
            {
                fx.push(Effect::PasteAndRestore);
            }
        });
        let r = TextField::new("Réponse de l'IA à restaurer", &mut st.pasted)
            .placeholder("Collez ici la réponse de l'IA, avec ses balises [PERSONNE_1]…")
            .id_salt(RESPONSE_FIELD)
            .hide_label()
            .area_height(h)
            .monospace()
            .show(ui);
        if r.changed() {
            fx.push(Effect::RestoreText);
        }
    };
    let restored = |ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>| {
        ui.spacing_mut().item_spacing.y = p.space(2);
        let text = st.text_result.as_ref().map(|(t, _)| t.clone());
        let ready = text.is_some();
        let none = (!ready).then_some("Collez d'abord la réponse de l'IA.");
        // Fenêtre étroite : « .md » et « .docx » réduits à leur icône, pour que
        // le titre ne passe pas sous « Copier ».
        let compact = restored_head_compact(ui);
        let actions_w = restored_buttons_width(ui, compact);
        let [copy, md, word] = restored_buttons(compact);
        column_head(ui, Icon::CircleCheck, RESTORED_TITLE, actions_w, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(2);
            let word = super::guarded_button(ui, word, none);
            if word.clicked() {
                fx.push(Effect::SaveRestored(RestoredFormat::Word));
            }
            let md = super::guarded_button(ui, md, none);
            if md.clicked() {
                fx.push(Effect::SaveRestored(RestoredFormat::Markdown));
            }
            if copy_button(ui, "copier-reponse", copy, ready).clicked() {
                match &text {
                    Some(t) => fx.push(Effect::Copy {
                        text: t.clone(),
                        toast: "Réponse restaurée copiée",
                        sensitive: true,
                    }),
                    None => fx.push(Effect::Notice("Collez d'abord la réponse de l'IA.")),
                }
            }
        });
        Frame::new()
            .fill(p.sunken)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(radius(p.radius_md))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 32.0);
                egui::ScrollArea::vertical()
                    .id_salt("reponse-restauree")
                    .max_height(h - 32.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| match &st.text_result {
                        Some((text, r)) => {
                            let mut marks: Vec<(std::ops::Range<usize>, Mark)> = st
                                .restored_marks
                                .iter()
                                .map(|m| (m.clone(), Mark::Restored))
                                .chain(
                                    tag_ranges(text, &r.unknown)
                                        .into_iter()
                                        .map(|m| (m, Mark::Unknown)),
                                )
                                .collect();
                            marks.sort_by_key(|(m, _)| m.start);
                            marked_label(ui, text, &marks, &mono_format());
                        }
                        None => {
                            muted(
                                ui,
                                "La réponse restaurée s'affiche ici, avec les vraies valeurs.",
                            );
                        }
                    });
            });
    };
    if ui.available_width() >= TWO_COLUMNS {
        // Texte aligné à gauche : la mise en page des colonnes est justifiée.
        let left = egui::Layout::top_down(egui::Align::Min);
        ui.columns(2, |cols| {
            if let [a, b] = cols {
                a.with_layout(left, |ui| pasted(ui, st, fx));
                b.with_layout(left, |ui| restored(ui, st, fx));
            }
        });
    } else {
        pasted(ui, st, fx);
        restored(ui, st, fx);
    }
}

/// Un seul message sous les colonnes : balises inconnues d'abord, sinon
/// balises de la clé absentes de la réponse.
fn message(ui: &mut Ui, st: &DeanonymizeState) {
    let Some((_, r)) = &st.text_result else {
        return;
    };
    if !r.unknown.is_empty() {
        let title = format!(
            "{} : {}",
            count_label(r.unknown.len(), "balise inconnue", "balises inconnues"),
            r.unknown.join(", ")
        );
        let text = if r.unknown.len() == 1 {
            "Elle ne figure pas dans la clé et reste telle quelle. L'IA l'a peut-être inventée."
        } else {
            "Elles ne figurent pas dans la clé et restent telles quelles. L'IA les a peut-être inventées."
        };
        Banner::error(text).title(&title).show(ui);
        return;
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

/// Fichiers renvoyés par l'IA : ajout, dossier, restauration, résultats.
fn files_card(ui: &mut Ui, st: &mut DeanonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        control_row(ui, CONTROL_SM, |ui| {
            card_title(ui, Icon::File, "Fichiers renvoyés par l'IA");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        PillButton::secondary("Ajouter des fichiers")
                            .icon(Icon::Plus)
                            .small(),
                    )
                    .on_hover_text("Ou déposez-les sur la fenêtre")
                    .clicked()
                {
                    fx.push(Effect::PickResponses);
                }
            });
        });
        if st.input_files.is_empty() && st.file_results.is_empty() {
            muted(
                ui,
                "Si l'IA a renvoyé un fichier (Word, Excel, PDF…), déposez-le ici : ses balises \
sont remplacées par les vraies valeurs dans une copie.",
            );
            return;
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
            let label = format!(
                "Restaurer {}",
                count_label(st.input_files.len(), "fichier", "fichiers")
            );
            let go = super::guarded_button(
                ui,
                PillButton::primary(&label).icon(Icon::Undo).small(),
                st.files_blocker(),
            );
            if go.clicked() {
                fx.push(Effect::RestoreFiles);
            }
        }
        let mut folder = None;
        for o in &st.file_results {
            match &o.result {
                Ok((path, r)) => {
                    folder = path.parent().map(|d| d.to_path_buf());
                    let mut line = format!(
                        "{} : {}, enregistré sous « {} »",
                        o.name,
                        count_label(r.restored, "balise restaurée", "balises restaurées"),
                        file_name(path)
                    );
                    if !r.unknown.is_empty() {
                        line.push_str(&format!(
                            " ; {} : {}",
                            count_label(r.unknown.len(), "balise inconnue", "balises inconnues"),
                            r.unknown.join(", ")
                        ));
                    }
                    body(ui, &line);
                }
                Err(msg) => {
                    Banner::error(msg).title(&o.name).show(ui);
                }
            }
        }
        if let Some(dir) = folder
            && ui
                .add(
                    PillButton::ghost("Ouvrir le dossier")
                        .icon(Icon::FolderOpen)
                        .small(),
                )
                .clicked()
        {
            fx.push(Effect::OpenFolder(dir));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::widgets::field::area_line_height;

    #[test]
    fn columns_fill_the_window_with_at_least_twelve_lines() {
        // 1280 × 800 : contenu sous l'en-tête, la barre du dossier, la barre d'action, au-dessus du pied.
        let h = area_height(560.0);
        assert!(h >= area_height_for_rows(MIN_ROWS));
        assert!(h / area_line_height() >= 12.0, "{h}");
        assert_eq!(area_height(300.0), area_height_for_rows(MIN_ROWS));
        assert!(area_height(1000.0) > h);
    }
}
