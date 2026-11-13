//! Aperçu des images analysées à l'étape « Vérifier » : zones masquées pour la
//! sélection courante, zones tracées à la main (ajout à la souris, retrait).

use super::card_title;
use crate::ui::state::anonymize::{AnonymizeState, ImageInfo};
use crate::ui::state::images::Fit;
use crate::ui::theme::{Weight, font, palette, radius};
use crate::ui::widgets::button::icon_button;
use crate::ui::widgets::nav::filter_chip;
use crate::ui::widgets::{Badge, Banner, Card, Icon, PillButton, body, caption, muted};
use egui::{
    Color32, ColorImage, CornerRadius, Id, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Ui, pos2, vec2,
};

/// Hauteur maximale de l'aperçu, en points.
const CANVAS_MAX_H: f32 = 460.0;
/// Côté maximal de la texture d'aperçu : les grandes pages sont réduites.
const TEXTURE_MAX: u32 = 2048;

/// Texture d'aperçu et dimensions de l'image d'origine.
#[derive(Clone)]
struct Preview {
    texture: Option<TextureHandle>,
    size: [u32; 2],
}

/// Nom affiché d'une image : le fichier seul pour une image isolée.
pub fn image_label(info: &ImageInfo) -> String {
    if info.name == "image" {
        info.file.clone()
    } else {
        format!("{} · {}", info.file, info.name)
    }
}

/// Décode l'image une fois et la garde en mémoire d'egui.
fn preview(ui: &Ui, info: &ImageInfo, bytes: &[u8]) -> Preview {
    let id = Id::unique(("apercu-image", &info.file, &info.name, bytes.len()));
    if let Some(p) = ui.data(|d| d.get_temp::<Preview>(id)) {
        return p;
    }
    let p = match image::load_from_memory(bytes) {
        Ok(img) => {
            let size = [img.width(), img.height()];
            let img = if img.width() > TEXTURE_MAX || img.height() > TEXTURE_MAX {
                img.thumbnail(TEXTURE_MAX, TEXTURE_MAX)
            } else {
                img
            };
            let rgba = img.to_rgba8();
            let color = ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            );
            Preview {
                texture: Some(ui.ctx().load_texture(
                    format!("apercu-{}", id.value()),
                    color,
                    TextureOptions::LINEAR,
                )),
                size,
            }
        }
        Err(_) => Preview {
            texture: None,
            size: [info.width, info.height],
        },
    };
    ui.data_mut(|d| d.insert_temp(id, p.clone()));
    p
}

fn to_rect(r: [f32; 4]) -> Rect {
    Rect::from_min_max(pos2(r[0], r[1]), pos2(r[2], r[3]))
}

/// Carré de légende.
fn swatch(ui: &mut Ui, fill: Color32, edge: Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(2),
            fill,
            Stroke::new(1.5, edge),
            StrokeKind::Inside,
        );
        caption(ui, text);
    });
}

fn auto_colors() -> (Color32, Color32) {
    let p = palette();
    (
        Color32::from_rgba_unmultiplied(p.dark.r(), p.dark.g(), p.dark.b(), 215),
        p.dark,
    )
}

fn manual_colors() -> (Color32, Color32) {
    let p = palette();
    let c = p.primary_dark;
    (Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 120), c)
}

/// Carte « Images » de l'étape Vérifier. Ne s'affiche que s'il y a des images.
pub fn images_card(ui: &mut Ui, st: &mut AnonymizeState) {
    let images = st.images();
    if images.is_empty() {
        return;
    }
    let p = palette();
    let idx = st.image_index.min(images.len().saturating_sub(1));
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::Image, "Images : zones masquées");
        muted(
            ui,
            "Les zones sombres seront masquées avec la sélection actuelle. Tracez un rectangle à \
la souris pour masquer aussi une zone que la lecture a manquée : signature, tampon, texte manuscrit.",
        );
        let low: Vec<String> = images
            .iter()
            .filter(|i| i.low_confidence)
            .map(image_label)
            .collect();
        if !low.is_empty() {
            let text = format!(
                "{} : le texte a été mal lu. Des données peuvent avoir échappé à la détection : \
regardez ces images et tracez les zones manquantes.",
                low.join(", ")
            );
            Banner::warning(&text).title("Lecture incertaine").show(ui);
        }
        if images.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(p.space(2), p.space(2));
                let brand = crate::ui::theme::Tinted {
                    fill: p.primary_subtle,
                    ink: p.primary_dark,
                    edge: p.primary,
                };
                for (i, info) in images.iter().enumerate() {
                    let n = st.masks.of_image(&info.file, &info.name).len();
                    let count = (n > 0).then_some(n);
                    if filter_chip(ui, &image_label(info), count, i == idx, brand).clicked() {
                        st.image_index = i;
                    }
                }
            });
        }
        let Some(info) = images.get(idx) else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            ui.label(
                RichText::new(image_label(info))
                    .font(font(Weight::Semibold, p.text_sm))
                    .color(p.dark),
            );
            if info.low_confidence {
                ui.add(Badge::warning("Lecture incertaine").small());
            }
            let (af, ae) = auto_colors();
            swatch(ui, af, ae, "Masquage automatique");
            let (mf, me) = manual_colors();
            swatch(ui, mf, me, "Zone ajoutée");
        });
        canvas(ui, st, idx, info);
        manual_list(ui, st, info);
    });
}

fn canvas(ui: &mut Ui, st: &mut AnonymizeState, idx: usize, info: &ImageInfo) {
    let p = palette();
    let Some(bytes) = st
        .analysis
        .as_ref()
        .and_then(|a| a.image(&info.file, &info.name))
        .map(|i| i.bytes)
    else {
        return;
    };
    let pv = preview(ui, info, bytes);
    // Coordonnées des zones : pixels lus par l'OCR, sinon ceux de l'image.
    let (w, h) = if info.width > 0 && info.height > 0 {
        (info.width, info.height)
    } else {
        (pv.size[0], pv.size[1])
    };
    let Some(fit) = Fit::new(0.0, 0.0, ui.available_width(), CANVAS_MAX_H, w, h) else {
        caption(ui, "Aperçu indisponible pour cette image.");
        return;
    };
    let (sw, sh) = fit.size();
    let (rect, response) = ui.allocate_exact_size(vec2(sw, sh), Sense::drag());
    let fit = fit.at(rect.left(), rect.top());
    let label = format!(
        "Aperçu de {} : tracez un rectangle pour masquer une zone",
        image_label(info)
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &label));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, radius(0.0), p.gray_100);
    match &pv.texture {
        Some(t) => {
            painter.image(
                t.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Aperçu indisponible pour ce format",
                font(Weight::Regular, p.text_sm),
                p.text_secondary,
            );
        }
    }
    let (af, ae) = auto_colors();
    for r in st.auto_zones(idx) {
        painter.rect(
            to_rect(fit.to_screen(*r)),
            CornerRadius::ZERO,
            af,
            Stroke::new(1.0, ae),
            StrokeKind::Inside,
        );
    }
    let (mf, me) = manual_colors();
    for r in st.masks.of_image(&info.file, &info.name) {
        painter.rect(
            to_rect(fit.to_screen(r)),
            CornerRadius::ZERO,
            mf,
            Stroke::new(2.0, me),
            StrokeKind::Inside,
        );
    }
    painter.rect_stroke(
        rect,
        radius(0.0),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );

    // Tracé à la souris.
    let start_id = response.id.with("debut-trace");
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        ui.data_mut(|d| d.insert_temp(start_id, pos));
    }
    let start: Option<Pos2> = ui.data(|d| d.get_temp(start_id));
    if let Some(start) = start {
        let current = response
            .interact_pointer_pos()
            .or(ui.ctx().pointer_latest_pos());
        if let Some(cur) = current {
            painter.rect_stroke(
                Rect::from_two_pos(start, cur),
                CornerRadius::ZERO,
                Stroke::new(2.0, me),
                StrokeKind::Inside,
            );
            if response.drag_stopped() {
                if let Some(r) = fit.drag_to_rect((start.x, start.y), (cur.x, cur.y)) {
                    st.masks.add(&info.file, &info.name, r);
                }
                ui.data_mut(|d| d.remove::<Pos2>(start_id));
            }
        }
        if !response.dragged() && !response.drag_stopped() {
            ui.data_mut(|d| d.remove::<Pos2>(start_id));
        }
    }
    response.on_hover_cursor(egui::CursorIcon::Crosshair);
}

fn manual_list(ui: &mut Ui, st: &mut AnonymizeState, info: &ImageInfo) {
    let p = palette();
    let zones = st.masks.of_image(&info.file, &info.name);
    if zones.is_empty() {
        caption(ui, "Aucune zone ajoutée sur cette image.");
        return;
    }
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = p.space(2);
        for (i, r) in zones.iter().enumerate() {
            body(ui, &format!("Zone {} ({} × {} px)", i + 1, r.w, r.h));
            if icon_button(
                ui,
                Icon::X,
                &format!("Retirer la zone {}", i + 1),
                p.text_secondary,
            )
            .clicked()
            {
                remove = Some(i);
            }
        }
    });
    if let Some(i) = remove {
        st.masks.remove(&info.file, &info.name, i);
    }
    if ui
        .add(
            PillButton::ghost("Retirer les zones de cette image")
                .icon(Icon::Trash)
                .small(),
        )
        .clicked()
    {
        st.masks.clear_image(&info.file, &info.name);
    }
}
