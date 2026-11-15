//! Composants à la charte Skazy Formation.

pub mod badge;
pub mod banner;
pub mod button;
pub mod card;
pub mod checkbox;
pub mod choice;
pub mod dropzone;
pub mod field;
pub mod icon;
pub mod nav;
pub mod progress;
pub mod select;
pub mod table;
pub mod toast;

pub use badge::{Badge, BadgeSize, family_colors, family_tint};
pub use banner::{Banner, BannerKind};
pub use button::{ButtonSize, ButtonVariant, PillButton};
pub use card::Card;
pub use checkbox::{CheckState, Checkbox};
pub use choice::ChoiceCard;
pub use dropzone::DropZone;
pub use field::TextField;
pub use icon::Icon;
pub use nav::{StepBar, StepItem};
pub use progress::{ProgressBar, StrengthMeter};
pub use select::Select;
pub use table::{Column, DataTable, RowTone};

use crate::ui::theme::{Weight, font, palette, radius, with_alpha};

/// Marges d'une touche dessinée dans un bouton ou une ligne de la palette.
const KEYCAP_PAD: egui::Vec2 = egui::vec2(6.0, 3.0);

fn keycap_galley(ui: &Ui, text: &str, color: Color32) -> std::sync::Arc<egui::Galley> {
    let p = palette();
    ui.painter()
        .layout_no_wrap(text.to_owned(), font(Weight::Semibold, p.text_xs), color)
}

/// Taille d'une touche (« Ctrl+K ») dessinée par [`paint_keycap`].
pub fn keycap_size(ui: &Ui, text: &str) -> egui::Vec2 {
    keycap_galley(ui, text, Color32::PLACEHOLDER).size() + 2.0 * KEYCAP_PAD
}

/// Dessine une touche dans `rect` : sur un bouton primaire (`on_primary`),
/// fond vert plus foncé et texte blanc ; ailleurs, fond gris et texte courant.
/// Désactivée, la touche prend l'opacité 0,5 du bouton.
pub fn paint_keycap(ui: &Ui, rect: egui::Rect, text: &str, on_primary: bool, enabled: bool) {
    let p = palette();
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.5) };
    let (fill, edge, ink) = if on_primary {
        (
            p.primary_darker,
            with_alpha(p.on_primary, 0.35),
            p.on_primary,
        )
    } else {
        (p.muted, p.line_strong, p.text)
    };
    let (fill, edge, ink) = (dim(fill), dim(edge), dim(ink));
    let painter = ui.painter();
    painter.rect(
        rect,
        radius(p.radius_sm),
        fill,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let g = keycap_galley(ui, text, ink);
    painter.galley(rect.center() - g.size() / 2.0, g, ink);
}
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText, Ui};

/// Espacement des lettres de la charte (`tracking-wide`, 0,04 em).
pub const TRACKING_WIDE: f32 = 0.04;

/// Mise en page d'un texte d'une ligne avec espacement des lettres.
pub fn spaced_job(text: &str, font_id: FontId, color: Color32, tracking_em: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let spacing = font_id.size * tracking_em;
    job.append(
        text,
        0.0,
        TextFormat {
            font_id,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    job.wrap.max_rows = 1;
    job
}

/// Titre de page (24 px, gras).
pub fn heading(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    ui.label(
        RichText::new(text)
            .font(font(Weight::Bold, p.text_xl))
            .color(p.heading),
    )
}

/// Titre de section (18 px, demi-gras).
pub fn subheading(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    ui.label(
        RichText::new(text)
            .font(font(Weight::Semibold, p.text_md))
            .color(p.heading),
    )
}

/// Texte courant.
pub fn body(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    ui.label(
        RichText::new(text)
            .font(font(Weight::Regular, p.text_sm))
            .color(p.text),
    )
}

/// Texte secondaire (gris AA).
pub fn muted(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    ui.label(
        RichText::new(text)
            .font(font(Weight::Regular, p.text_sm))
            .color(p.text_secondary),
    )
}

/// Petit texte secondaire (12 px).
pub fn caption(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    ui.label(
        RichText::new(text)
            .font(font(Weight::Regular, p.text_xs))
            .color(p.text_secondary),
    )
}

/// Touche de clavier (panneau des raccourcis) : texte à chasse fixe dans un cadre.
pub fn keycap(ui: &mut Ui, keys: &str) -> egui::Response {
    let p = palette();
    egui::Frame::new()
        .fill(p.muted)
        .stroke(egui::Stroke::new(1.0, p.line_strong))
        .corner_radius(crate::ui::theme::radius(p.radius_sm))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(keys)
                    .font(egui::FontId::monospace(p.text_sm - 1.0))
                    .color(p.heading),
            )
        })
        .inner
}

/// Libellé de groupe : petit, demi-gras, majuscules espacées.
pub fn overline(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    let job = spaced_job(
        &text.to_uppercase(),
        font(Weight::Semibold, p.text_xs),
        p.text_secondary,
        0.08,
    );
    ui.label(job)
}
