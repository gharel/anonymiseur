//! Bandeau d'information, de succès, d'avertissement ou d'erreur.
//!
//! La charte réserve `color-success`, `color-warning` et `color-info` aux fonds
//! et aux icônes : ici la barre latérale prend la couleur d'état, l'icône son
//! encre AA (3:1 minimum sur la teinte) et le texte reste anthracite.

use super::Icon;
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{Color32, Frame, Margin, Rect, Response, RichText, Ui, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    Info,
    Success,
    Warning,
    Error,
}

impl BannerKind {
    /// (fond, barre, icône, pictogramme).
    fn style(self) -> (Color32, Color32, Color32, Icon) {
        let p = palette();
        match self {
            BannerKind::Info => (p.dev.fill, p.info, p.dev.ink, Icon::Info),
            BannerKind::Success => (
                p.primary_subtle,
                p.success,
                p.primary_dark,
                Icon::ShieldCheck,
            ),
            BannerKind::Warning => (p.data.fill, p.warning, p.data.ink, Icon::TriangleAlert),
            BannerKind::Error => (p.error_tint(), p.error, p.error, Icon::TriangleAlert),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Banner<'a> {
    kind: BannerKind,
    title: Option<&'a str>,
    text: &'a str,
}

impl<'a> Banner<'a> {
    pub fn new(kind: BannerKind, text: &'a str) -> Self {
        Self {
            kind,
            title: None,
            text,
        }
    }

    pub fn info(text: &'a str) -> Self {
        Self::new(BannerKind::Info, text)
    }

    pub fn success(text: &'a str) -> Self {
        Self::new(BannerKind::Success, text)
    }

    pub fn warning(text: &'a str) -> Self {
        Self::new(BannerKind::Warning, text)
    }

    pub fn error(text: &'a str) -> Self {
        Self::new(BannerKind::Error, text)
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let p = palette();
        let (fill, bar, ink, icon) = self.kind.style();
        let r = radius(p.radius_md);
        let inner = Frame::new()
            .fill(fill)
            .corner_radius(r)
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 12,
                bottom: 12,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    icon.show(ui, ink, 20.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if let Some(title) = self.title {
                            ui.label(
                                RichText::new(title)
                                    .font(font(Weight::Semibold, p.text_sm))
                                    .color(p.dark),
                            );
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.text)
                                    .font(font(Weight::Regular, p.text_sm))
                                    .color(p.text),
                            )
                            .wrap(),
                        );
                    });
                });
            });
        let rect = inner.response.rect;
        let left = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
        ui.painter().with_clip_rect(left).rect_filled(rect, r, bar);
        inner.response
    }
}
