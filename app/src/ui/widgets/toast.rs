//! Pile de notifications en bas à droite de la fenêtre, au-dessus du pied.
//! Chaque notification est lue par les lecteurs d'écran (rôle « statut », ou
//! « alerte » pour une erreur) et peut être fermée.

use super::Icon;
use super::button::icon_button;
use crate::ui::state::toasts::{ToastKind, Toasts};
use crate::ui::theme::{Weight, font, palette, radius};
use egui::{Align2, Area, Frame, Id, Margin, Order, Rect, RichText, Role, Sense, Stroke, Ui, vec2};

/// Largeur d'une notification.
const WIDTH: f32 = 400.0;
/// Écart entre deux notifications et avec le bord de la fenêtre.
const GAP: f32 = 8.0;

/// Couleur de barre, d'icône et pictogramme d'un type de notification.
fn style(kind: ToastKind) -> (egui::Color32, egui::Color32, Icon) {
    let p = palette();
    match kind {
        ToastKind::Success => (p.success, p.accent, Icon::CircleCheck),
        ToastKind::Info => (p.info, p.dev.ink, Icon::Info),
        ToastKind::Warning => (p.warning, p.data.ink, Icon::TriangleAlert),
        ToastKind::Error => (p.error, p.error, Icon::CircleX),
    }
}

/// Affiche les notifications ; `bottom` : hauteur réservée en bas (pied).
pub fn show(ui: &Ui, toasts: &mut Toasts, now: f64, bottom: f32) {
    toasts.expire(now);
    if let Some(next) = toasts.next_deadline(now) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.05)));
    }
    if toasts.is_empty() {
        return;
    }
    let p = palette();
    let mut close = None;
    Area::new(Id::unique("notifications"))
        .order(Order::Foreground)
        .anchor(Align2::RIGHT_BOTTOM, vec2(-24.0, -(bottom + 16.0)))
        .interactable(true)
        .show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = GAP;
            for t in toasts.items() {
                let (bar, ink, icon) = style(t.kind);
                let inner = Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(radius(p.radius_md))
                    .shadow(p.shadow_md)
                    .inner_margin(Margin {
                        left: 16,
                        right: 8,
                        top: 8,
                        bottom: 8,
                    })
                    .show(ui, |ui| {
                        ui.set_width(WIDTH - 24.0);
                        ui.horizontal(|ui| {
                            ui.set_min_height(32.0);
                            ui.spacing_mut().item_spacing.x = 12.0;
                            icon.show(ui, ink, 20.0);
                            // Texte peint (et non un libellé) : le seul nœud
                            // d'accessibilité est la notification elle-même.
                            let text_w = ui.available_width() - 40.0;
                            let galley = egui::WidgetText::from(
                                RichText::new(&t.text)
                                    .font(font(Weight::Medium, p.text_sm))
                                    .color(p.text),
                            )
                            .into_galley(
                                ui,
                                Some(egui::TextWrapMode::Wrap),
                                text_w,
                                egui::TextStyle::Body,
                            );
                            let (r, _) = ui.allocate_exact_size(
                                vec2(text_w, galley.size().y.max(20.0)),
                                Sense::hover(),
                            );
                            ui.painter().galley(
                                egui::pos2(r.min.x, r.center().y - galley.size().y / 2.0),
                                galley,
                                p.text,
                            );
                            let label = format!("Fermer la notification : {}", t.text);
                            if icon_button(ui, Icon::X, &label, p.text_secondary).clicked() {
                                close = Some(t.id);
                            }
                        });
                    });
                let rect = inner.response.rect;
                let bar_rect = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
                ui.painter()
                    .with_clip_rect(bar_rect)
                    .rect_filled(rect, radius(p.radius_md), bar);
                let role = if t.kind == ToastKind::Error {
                    Role::Alert
                } else {
                    Role::Status
                };
                let live = ui.interact(rect, Id::unique(("notification", t.id)), Sense::hover());
                let text = t.text.clone();
                live.widget_info(|| egui::WidgetInfo::labeled(role, true, &text));
            }
        });
    if let Some(id) = close {
        toasts.dismiss(id);
    }
}
