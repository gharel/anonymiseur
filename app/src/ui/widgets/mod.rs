//! Composants à la charte Skazy Formation.

pub mod badge;
pub mod banner;
pub mod button;
pub mod card;
pub mod checkbox;
pub mod dropzone;
pub mod field;
pub mod icon;
pub mod nav;
pub mod progress;
pub mod table;

pub use badge::{Badge, BadgeSize, family_colors, family_tint};
pub use banner::{Banner, BannerKind};
pub use button::{ButtonSize, ButtonVariant, PillButton};
pub use card::Card;
pub use checkbox::{CheckState, Checkbox};
pub use dropzone::DropZone;
pub use field::TextField;
pub use icon::Icon;
pub use nav::{Stepper, TabBar};
pub use progress::{ProgressBar, StrengthMeter};
pub use table::{Column, DataTable, RowTone};

use crate::ui::theme::{Weight, font, palette};
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
            .color(p.dark),
    )
}

/// Titre de section (18 px, demi-gras).
pub fn subheading(ui: &mut Ui, text: &str) -> egui::Response {
    let p = palette();
    ui.label(
        RichText::new(text)
            .font(font(Weight::Semibold, p.text_md))
            .color(p.dark),
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
