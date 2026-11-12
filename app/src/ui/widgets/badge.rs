//! Badge teinté (`Badge` de la charte) : fond `-tint`, texte `-ink`, contour de
//! la couleur de base, MAJUSCULES espacées.

use super::{TRACKING_WIDE, spaced_job};
use crate::ui::theme::{Tinted, Weight, font, palette, radius};
use anon_core::{EntityKind, Family};
use egui::{Color32, Response, Sense, Stroke, StrokeKind, Ui, Widget, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSize {
    Sm,
    Md,
}

/// Badge de catégorie ou de statut.
#[derive(Debug, Clone)]
pub struct Badge {
    text: String,
    colors: Tinted,
    size: BadgeSize,
}

impl Badge {
    pub fn new(text: &str, colors: Tinted) -> Self {
        Self {
            text: text.to_uppercase(),
            colors,
            size: BadgeSize::Md,
        }
    }

    /// Badge d'un type d'entité, coloré selon sa famille.
    pub fn entity(kind: EntityKind) -> Self {
        Self::new(kind.label(), family_tint(kind.family()))
    }

    /// Badge vert de marque (statut positif).
    pub fn brand(text: &str) -> Self {
        let p = palette();
        Self::new(
            text,
            Tinted {
                fill: p.primary_subtle,
                ink: p.primary_dark,
                edge: p.primary,
            },
        )
    }

    /// Badge neutre.
    pub fn neutral(text: &str) -> Self {
        Self::new(text, neutral_tint())
    }

    /// Badge d'avertissement (jaune Data).
    pub fn warning(text: &str) -> Self {
        Self::new(text, palette().data)
    }

    /// Badge d'erreur.
    pub fn error(text: &str) -> Self {
        let p = palette();
        Self::new(
            text,
            Tinted {
                fill: p.error_tint(),
                ink: p.error,
                edge: p.error,
            },
        )
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(BadgeSize::Sm)
    }
}

fn neutral_tint() -> Tinted {
    let p = palette();
    Tinted {
        fill: p.gray_100,
        ink: p.gray_700,
        edge: p.gray_400,
    }
}

/// Couleurs d'une famille de types : teinte, encre et couleur de base.
pub fn family_tint(family: Family) -> Tinted {
    let p = palette();
    match family {
        Family::Personnes => p.bureautique,
        Family::Organisations => p.dev,
        Family::Lieux => p.graphisme,
        Family::Contacts => p.ia,
        Family::Identifiants => p.gestion,
        Family::Finances => p.data,
        Family::Contexte => neutral_tint(),
    }
}

/// Couleurs d'un badge de famille : (fond, texte).
pub fn family_colors(family: Family) -> (Color32, Color32) {
    let t = family_tint(family);
    (t.fill, t.ink)
}

/// Nom français d'une famille.
pub fn family_label(family: Family) -> &'static str {
    match family {
        Family::Personnes => "Personnes",
        Family::Organisations => "Organisations",
        Family::Lieux => "Lieux",
        Family::Contacts => "Contacts",
        Family::Identifiants => "Identifiants",
        Family::Finances => "Finances",
        Family::Contexte => "Contexte",
    }
}

/// Toutes les familles, dans l'ordre d'affichage.
pub const FAMILIES: [Family; 7] = [
    Family::Personnes,
    Family::Organisations,
    Family::Lieux,
    Family::Contacts,
    Family::Identifiants,
    Family::Finances,
    Family::Contexte,
];

impl Widget for Badge {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette();
        let (size, pad_x, pad_y) = match self.size {
            BadgeSize::Sm => (10.0, 10.0, 3.0),
            BadgeSize::Md => (p.text_xs, 12.0, 5.0),
        };
        let galley = ui.painter().layout_job(spaced_job(
            &self.text,
            font(Weight::Semibold, size),
            self.colors.ink,
            TRACKING_WIDE,
        ));
        let border = 1.5;
        let desired = vec2(
            galley.size().x + 2.0 * (pad_x + border),
            size + 2.0 * (pad_y + border),
        );
        let (rect, response) = ui.allocate_exact_size(desired, Sense::hover());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            painter.rect(
                rect,
                radius(p.radius_full),
                self.colors.fill,
                Stroke::new(border, self.colors.edge),
                StrokeKind::Inside,
            );
            let pos = rect.center() - galley.size() / 2.0;
            painter.galley(pos, galley, self.colors.ink);
        }
        let text = self.text;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_map_to_the_requested_categories() {
        let p = palette();
        assert_eq!(
            family_colors(Family::Personnes),
            (p.bureautique.fill, p.bureautique.ink)
        );
        assert_eq!(
            family_colors(Family::Organisations),
            (p.dev.fill, p.dev.ink)
        );
        assert_eq!(
            family_colors(Family::Lieux),
            (p.graphisme.fill, p.graphisme.ink)
        );
        assert_eq!(family_colors(Family::Contacts), (p.ia.fill, p.ia.ink));
        assert_eq!(
            family_colors(Family::Identifiants),
            (p.gestion.fill, p.gestion.ink)
        );
        assert_eq!(family_colors(Family::Finances), (p.data.fill, p.data.ink));
        assert_eq!(family_colors(Family::Contexte), (p.gray_100, p.gray_700));
    }

    #[test]
    fn every_kind_has_a_family_color_and_label() {
        for kind in EntityKind::ALL {
            let b = Badge::entity(*kind);
            assert_eq!(b.text, kind.label().to_uppercase());
        }
    }
}
