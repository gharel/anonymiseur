//! Rendu des trois onglets. L'état vit dans `ui::state` ; les effets de bord
//! sont renvoyés à `AnonApp` sous forme d'`Effect`.

pub mod anonymize;
pub mod deanonymize;
pub mod freetext;
pub mod images;

use super::state::files::{FileEntry, FileFormat, human_size};
use super::theme::Tinted;
use super::theme::{CONTROL_MD, Weight, font, palette, radius};
use super::widgets::{Icon, heading, muted};
use egui::{Rect, Response, RichText, Sense, Ui, Vec2, vec2};

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

/// Barre d'actions de bas d'étape : `left` à gauche, `right` aligné à droite.
pub fn action_bar<S>(
    ui: &mut Ui,
    st: &mut S,
    left: impl FnOnce(&mut Ui, &mut S),
    right: impl FnOnce(&mut Ui, &mut S),
) {
    control_row(ui, CONTROL_MD, |ui| {
        ui.spacing_mut().item_spacing.x = palette().space(3);
        left(ui, st);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_height(CONTROL_MD);
            right(ui, st)
        });
    });
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
