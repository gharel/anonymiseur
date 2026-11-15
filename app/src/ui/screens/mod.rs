//! Rendu des écrans : accueil, puis les quatre étapes du dossier de travail
//! (Vérifier, Bilan, Envoyer, Réponse), la palette de commandes et les
//! fenêtres du presse-papiers express. L'état vit dans `ui::state` ; les effets
//! de bord sont renvoyés à `AnonApp` sous forme d'`Effect`.

pub mod bilan;
pub mod document;
pub mod express;
pub mod home;
pub mod images;
pub mod palette;
pub mod response;
pub mod review;
pub mod send;
pub mod verify;

use super::app::Effect;
use super::runner::{FileStatus, Shared, duration_label};
use super::shortcuts::{self, with_shortcut};
use super::state::files::{FileEntry, FileFormat, human_size};
use super::theme::Tinted;
use super::theme::{CONTROL_SM, Weight, font, palette, radius};
use super::widgets::{Card, Icon, PillButton, ProgressBar, heading, muted};
use egui::{Rect, Response, RichText, Sense, Ui, Vec2, vec2};

/// Hauteur d'une ligne de la liste d'avancement.
const STATUS_ROW_H: f32 = 32.0;
/// Hauteur maximale de la liste d'avancement avant défilement.
const STATUS_LIST_MAX_H: f32 = 288.0;

/// Carte d'avancement : barre, fichier courant, liste des fichiers avec leur
/// état, temps écoulé, bouton Annuler (Échap).
pub fn busy_card(
    ui: &mut Ui,
    title: &str,
    names: &[String],
    progress: &Shared,
    fx: &mut Vec<Effect>,
) {
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
        if !names.is_empty() {
            let list_h = (names.len() as f32 * STATUS_ROW_H).min(STATUS_LIST_MAX_H);
            egui::ScrollArea::vertical()
                .id_salt("avancement")
                .max_height(STATUS_LIST_MAX_H)
                .min_scrolled_height(list_h)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, name) in names.iter().enumerate() {
                        status_row(ui, name, progress.status(i));
                    }
                });
        }
        control_row(ui, CONTROL_SM, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            if progress.is_cancel_requested() {
                muted(ui, "Annulation demandée : le fichier en cours se termine.");
            } else if ui
                .add(PillButton::secondary("Annuler").icon(Icon::X).small())
                .on_hover_text(with_shortcut("Annuler le traitement", shortcuts::BACK))
                .clicked()
            {
                fx.push(Effect::Cancel);
            }
            muted(
                ui,
                &format!("Temps écoulé : {}", duration_label(progress.elapsed_secs())),
            );
        });
    });
}

/// Ligne de la liste d'avancement : icône d'état, nom, état en toutes lettres.
fn status_row(ui: &mut Ui, name: &str, status: FileStatus) {
    let p = palette();
    let (icon, color) = match status {
        FileStatus::Pending => (Icon::Clock, p.text_secondary),
        FileStatus::Running => (Icon::LoaderCircle, p.accent),
        FileStatus::Done => (Icon::CircleCheck, p.accent),
        FileStatus::Failed => (Icon::CircleX, p.error),
    };
    control_row(ui, STATUS_ROW_H, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(2);
        icon.show(ui, color, 16.0);
        let weight = if status == FileStatus::Running {
            Weight::Semibold
        } else {
            Weight::Regular
        };
        let w = (ui.available_width() - 120.0).max(80.0);
        truncated_text(
            ui,
            w,
            STATUS_ROW_H,
            RichText::new(name)
                .font(font(weight, p.text_sm))
                .color(p.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ink = match status {
                FileStatus::Failed => p.error,
                FileStatus::Running => p.accent,
                _ => p.text_secondary,
            };
            ui.label(
                RichText::new(status.label())
                    .font(font(Weight::Medium, p.text_xs))
                    .color(ink),
            );
        });
    });
}

/// Icône et couleurs d'un format.
pub fn format_style(format: FileFormat) -> (Icon, Tinted) {
    let p = palette();
    match format {
        FileFormat::Word | FileFormat::OdfText => (Icon::FileText, p.dev),
        FileFormat::Excel | FileFormat::OdfSheet | FileFormat::Csv => {
            (Icon::FileSpreadsheet, p.graphisme)
        }
        FileFormat::PowerPoint | FileFormat::OdfSlides => (Icon::Presentation, p.ia),
        FileFormat::Pdf => (Icon::File, p.gestion),
        FileFormat::Image => (Icon::Image, p.bureautique),
        FileFormat::Text | FileFormat::GoogleShortcut | FileFormat::Unsupported => {
            (Icon::File, p.neutral)
        }
    }
}

/// Pastille carrée d'icône de format.
pub fn file_tile(ui: &mut Ui, format: FileFormat) -> Response {
    let p = palette();
    let (icon, tint) = format_style(format);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, radius(p.radius_md), tint.fill);
    icon.paint(
        ui,
        Rect::from_center_size(rect.center(), Vec2::splat(20.0)),
        tint.ink,
    );
    response
}

/// Ligne de fichier : pastille, nom, format et taille, puis `trailing` à droite.
pub fn file_row(ui: &mut Ui, file: &FileEntry, trailing: impl FnOnce(&mut Ui)) {
    let p = palette();
    ui.horizontal(|ui| {
        ui.set_min_height(48.0);
        ui.spacing_mut().item_spacing.x = p.space(3);
        file_tile(ui, file.format);
        let detail = match file.size_bytes {
            Some(b) => format!("{} · {}", file.format.label(), human_size(b)),
            None => file.format.label().to_owned(),
        };
        let trailing_w = 48.0;
        let text_w = (ui.available_width() - trailing_w).max(80.0);
        ui.allocate_ui_with_layout(
            vec2(text_w, 44.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&file.name)
                            .font(font(Weight::Semibold, p.text_sm))
                            .color(p.heading),
                    )
                    .truncate(),
                )
                .on_hover_text(file.name.as_str());
                ui.label(
                    RichText::new(detail)
                        .font(font(Weight::Regular, p.text_xs))
                        .color(p.text_secondary),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
}

/// Texte d'une ligne, aligné à gauche dans une largeur donnée, tronqué avec
/// le texte entier en infobulle.
pub fn truncated_text(ui: &mut Ui, width: f32, height: f32, text: RichText) -> Response {
    let full = text.text().to_owned();
    ui.allocate_ui_with_layout(
        vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.add(egui::Label::new(text).truncate()),
    )
    .inner
    .on_hover_text(full)
}

/// Bouton d'action désactivable : désactivé, sa raison en infobulle.
pub fn guarded_button(ui: &mut Ui, button: PillButton<'_>, blocker: Option<&str>) -> Response {
    let r = ui.add(button.enabled(blocker.is_none()));
    match blocker {
        Some(why) => r.on_hover_text(why),
        None => r,
    }
}

/// Durée pendant laquelle un bouton Copier affiche « Copié », en secondes.
const COPIED_SECONDS: f64 = 1.5;

/// Bouton Copier qui passe brièvement à « Copié » après un clic. `available` :
/// il y a quelque chose à copier (sinon pas de « Copié »).
pub fn copy_button(
    ui: &mut Ui,
    id_salt: &str,
    button: PillButton<'_>,
    available: bool,
) -> Response {
    let id = egui::Id::unique(("copie", id_salt));
    let now = ui.input(|i| i.time);
    let since: Option<f64> = ui.data(|d| d.get_temp(id));
    let copied = since.is_some_and(|t| now - t < COPIED_SECONDS);
    let r = if copied {
        if let Some(t) = since {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(
                    (t + COPIED_SECONDS - now).max(0.05),
                ));
        }
        ui.add(button.label("Copié").icon(Icon::Check))
    } else {
        ui.add(button)
    };
    if r.clicked() && available {
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    r
}

/// État vide : icône, phrase, une seule action. Renvoie vrai si l'action est cliquée.
pub fn empty_state(ui: &mut Ui, icon: Icon, text: &str, action: Option<(&str, Icon)>) -> bool {
    let p = palette();
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        ui.add_space(p.space(4));
        let (rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 24.0, p.muted);
        icon.paint(
            ui,
            Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
            p.text_secondary,
        );
        muted(ui, text);
        if let Some((label, icon)) = action {
            clicked = ui
                .add(PillButton::secondary(label).icon(icon).small())
                .clicked();
        }
        ui.add_space(p.space(2));
    });
    clicked
}

/// Titre d'écran et sous-titre.
pub fn page_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        heading(ui, title);
        muted(ui, subtitle);
    });
}

/// Ligne de contrôles de hauteur `height` : chaque élément est centré
/// verticalement sur cette hauteur, même ceux placés avant un contrôle plus haut.
pub fn control_row<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(height);
        add(ui)
    })
    .inner
}

/// Titre de carte avec icône.
pub fn card_title(ui: &mut Ui, icon: Icon, title: &str) {
    let p = palette();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = p.space(2);
        icon.show(ui, p.accent, 20.0);
        ui.label(
            RichText::new(title)
                .font(font(Weight::Semibold, p.text_md))
                .color(p.heading),
        );
    });
}
