//! Presse-papiers express dans l'application : raccourcis globaux, lecture et
//! écriture du presse-papiers, clé du dossier, notification et vérification
//! dans des fenêtres à part (viewports egui secondaires).
//!
//! Fenêtres à part plutôt que la fenêtre principale ramenée au premier plan :
//! la fenêtre principale peut être réduite, sur un autre écran ou au milieu
//! d'une étape ; sous Windows, ramener une fenêtre réduite exige de la
//! restaurer puis de forcer le premier plan, et sous macOS d'activer toute
//! l'application. Une petite fenêtre neuve, toujours au premier plan, apparaît
//! au-dessus de l'application où le texte a été copié, de la même façon sous
//! Windows et macOS, sans toucher à l'état de la fenêtre principale. Le
//! processus qui reçoit un raccourci global a le droit de prendre le premier
//! plan : la vérification reçoit le clavier (Entrée, Échap, M, G, J, K).
//!
//! Viewports différés, et non immédiats : fenêtre principale réduite, eframe
//! fait tourner son interface hors du contexte de la boucle d'événements et ne
//! peut pas y créer la fenêtre d'un viewport immédiat (egui s'arrête alors sur
//! « the user callback was never called ») ; il crée celle d'un viewport
//! différé au tour suivant de la boucle. Chaque fenêtre se dessine dans sa
//! propre image : la vérification est partagée (`SharedReview`).

use super::AnonApp;
use crate::ui::clipboard::Clipboard;
use crate::ui::hotkeys::{self, Hotkeys};
use crate::ui::runner::Msg;
use crate::ui::screens::express::{
    NOTICE_SIZE, NOTICE_TITLE, REVIEW_MIN, REVIEW_SIZE, REVIEW_TITLE, notice_position,
};
use crate::ui::state::deanonymize::MapStatus;
use crate::ui::state::express::{
    self, Action, ExpressReview, KnownDecisions, MaskError, MaskRequest, Notice, Prepared,
    ReviewOutcome,
};
use crate::ui::state::toasts::ToastKind;
use crate::ui::{screens, theme};
use anon_core::Session;
use egui::{ViewportBuilder, ViewportClass, ViewportCommand, ViewportId};
use std::sync::{Arc, Mutex, PoisonError};

/// Images de la fenêtre principale pendant lesquelles la vérification
/// redemande le focus, le temps que le système crée sa fenêtre.
const FOCUS_ATTEMPTS: u32 = 30;
/// Durée, après l'ouverture d'une fenêtre du presse-papiers, pendant laquelle
/// l'application vérifie souvent qu'elle est créée pour la montrer (secondes).
const REVEAL_SECONDS: f64 = 1.0;
/// Intervalle de ces vérifications (millisecondes).
const REVEAL_POLL_MS: u64 = 16;

/// Identifiants des deux fenêtres.
fn notice_id() -> ViewportId {
    ViewportId::from_hash_of("notification-presse-papiers")
}

fn review_id() -> ViewportId {
    ViewportId::from_hash_of("verification-presse-papiers")
}

impl AnonApp {
    /// Branche le presse-papiers et les raccourcis (ceux du système dans
    /// l'application, simulés dans les tests) ; enregistre les raccourcis si
    /// la préférence est active.
    pub fn set_express_system(&mut self, clipboard: Box<dyn Clipboard>, hotkeys: Box<dyn Hotkeys>) {
        self.clipboard = clipboard;
        self.hotkeys = hotkeys;
        if self.prefs.clipboard_hotkeys {
            let failed = self.hotkeys.set_enabled(true);
            self.report_hotkey_failures(&failed);
        }
    }

    /// Un toast par raccourci qui n'a pas pu être enregistré, une seule fois.
    fn report_hotkey_failures(&mut self, failed: &[Action]) {
        let palette = crate::ui::shortcuts::PALETTE.label();
        for a in failed {
            if self.express.reported.contains(a) {
                continue;
            }
            self.express.reported.push(*a);
            let key = hotkeys::shortcut(*a).label();
            self.toast(
                ToastKind::Warning,
                format!(
                    "Raccourci {key} déjà utilisé par une autre application : « {} » reste dans la palette ({palette}).",
                    a.label()
                ),
            );
        }
    }

    /// Active ou désactive les raccourcis globaux (palette) ; retenu dans les
    /// préférences.
    pub fn set_global_hotkeys(&mut self, on: bool) {
        self.prefs.clipboard_hotkeys = on;
        let failed = self.hotkeys.set_enabled(on);
        let keys = format!("{}, {}", hotkeys::MASK.label(), hotkeys::RESTORE.label());
        if on {
            // Réactivés à la demande : un nouvel échec se dit de nouveau.
            self.express.reported.clear();
            self.toast(
                ToastKind::Success,
                format!("Raccourcis globaux du presse-papiers activés ({keys})."),
            );
            self.report_hotkey_failures(&failed);
        } else {
            let palette = crate::ui::shortcuts::PALETTE.label();
            self.toast(
                ToastKind::Info,
                format!(
                    "Raccourcis globaux du presse-papiers désactivés : les actions restent dans la palette ({palette})."
                ),
            );
        }
    }

    /// Horloge des notifications (secondes).
    pub fn clock(&self) -> f64 {
        self.fixed_clock
            .unwrap_or_else(|| self.started.elapsed().as_secs_f64())
    }

    /// Notification au premier plan et toast habituel : nombres et balises
    /// seulement.
    fn notify(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        self.express.notice = Some(Notice {
            kind,
            text: text.clone(),
            since: self.clock(),
        });
        self.express_reveal_until = self.clock() + REVEAL_SECONDS;
        self.toast(kind, text);
    }

    /// À chaque image, fenêtre visible ou non (`App::logic`) : raccourcis
    /// pressés, résultat de l'analyse, échéance de la notification.
    pub fn express_tick(&mut self, ctx: &egui::Context) {
        for action in self.hotkeys.poll() {
            self.express_run(action);
        }
        for msg in self.express_runner.poll() {
            if let Msg::ExpressPrepared(p) = msg {
                self.express_prepared(*p);
            }
        }
        self.express_review_outcome();
        let now = self.clock();
        self.express.expire(now);
        self.reveal_windows(ctx);
        let wanted = self.express.notice.is_some() || self.express.review.is_some();
        if wanted && !self.express_shown {
            // Fenêtre principale réduite : son interface ne tourne plus ; elle
            // doit tourner une image pour ouvrir les fenêtres du presse-papiers,
            // qui la font ensuite tourner tant qu'elles sont visibles.
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::RequestPaintWhileHidden);
            ctx.request_repaint();
        }
        if let Some(n) = &self.express.notice {
            let left = (n.since + express::NOTICE_SECONDS - now).max(0.05);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
    }

    /// Masquer ou restaurer le texte copié (raccourci global ou palette).
    pub fn express_run(&mut self, action: Action) {
        match action {
            Action::Mask => self.express_mask(),
            Action::Restore => self.express_restore(),
        }
    }

    /// Session de départ : la clé ouverte, sinon la session du dossier.
    fn express_base(&self) -> Session {
        self.deanonymize.session().unwrap_or(&self.session).clone()
    }

    /// Décisions du dossier à reprendre pour le texte copié.
    fn known_decisions(&self) -> KnownDecisions {
        self.anonymize
            .analysis
            .as_ref()
            .map(|a| KnownDecisions::from_candidates(&a.candidates, &self.anonymize.decided))
            .unwrap_or_default()
    }

    fn express_mask(&mut self) {
        if self.express.review.is_some() {
            // Vérification déjà ouverte : elle revient au premier plan.
            self.express.with_review(|r| r.focus = true);
            self.express_focus = FOCUS_ATTEMPTS;
            self.express_reveal_until = self.clock() + REVEAL_SECONDS;
            return;
        }
        if self.express.preparing {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return self.notify(ToastKind::Warning, express::ENGINE_LOADING);
        };
        if self.is_busy() {
            return self.notify(ToastKind::Warning, express::APP_BUSY);
        }
        if matches!(self.deanonymize.map, MapStatus::Locked { .. }) {
            return self.notify(ToastKind::Warning, express::KEY_LOCKED);
        }
        let text = match express::usable_text(self.clipboard.read_text()) {
            Ok(t) => t,
            Err((kind, why)) => return self.notify(kind, why),
        };
        let request = MaskRequest {
            text,
            base: self.express_base(),
            options: self.anonymize.analyze_options(),
            known: self.known_decisions(),
        };
        self.express.preparing = true;
        self.express_runner
            .spawn(move |_| Msg::ExpressPrepared(Box::new(express::prepare(&engine, request))));
    }

    /// Texte copié analysé : vérification s'il reste des valeurs à décider,
    /// sinon remplacement du presse-papiers.
    pub(super) fn express_prepared(&mut self, prepared: Prepared) {
        self.express.preparing = false;
        if prepared.pending_count() > 0 {
            self.express.review = Some(Arc::new(Mutex::new(ExpressReview::new(prepared))));
            self.express_focus = FOCUS_ATTEMPTS;
            self.express_reveal_until = self.clock() + REVEAL_SECONDS;
        } else {
            self.express_commit(&prepared);
        }
    }

    /// Masque le texte, vérifie qu'aucune valeur validée n'y subsiste, puis
    /// remplace le presse-papiers et garde la clé ouverte.
    fn express_commit(&mut self, prepared: &Prepared) {
        let Some(engine) = self.engine.clone() else {
            return self.notify(ToastKind::Warning, express::ENGINE_LOADING);
        };
        if self.express_base().entities != prepared.base.entities {
            return self.notify(ToastKind::Warning, express::CHANGED);
        }
        match express::finish(&engine, prepared) {
            Err(MaskError::Leak) => self.notify(ToastKind::Error, express::LEAK),
            Ok(m) if m.values == 0 => self.notify(ToastKind::Info, express::NOTHING_TO_MASK),
            Ok(m) => {
                if !self.clipboard.write_text(&m.text, false) {
                    return self.notify(ToastKind::Error, express::WRITE_FAILED);
                }
                let values = m.values;
                self.install_express_session(m.session, &prepared.text);
                self.notify(ToastKind::Success, express::masked_notice(values));
            }
        }
    }

    /// La session du texte masqué devient la clé ouverte du dossier (une clé
    /// « Presse-papiers » en mémoire s'il n'y en avait pas) ; la session du
    /// dossier la suit si elle en descend. Sans dossier, le texte devient le
    /// document « presse-papiers.txt », pour restaurer ensuite.
    fn install_express_session(&mut self, session: Session, text: &str) {
        let no_folder = self.anonymize.documents() == 0 && !self.deanonymize.has_key();
        if self.deanonymize.is_unlocked() {
            self.deanonymize.replace_session(session.clone());
            self.restore_text_now();
        } else {
            self.deanonymize
                .open_in_memory(express::KEY_NAME.to_owned(), session.clone());
        }
        if express::extends(&session, &self.session) {
            let (goal, recipient) = (self.session.goal, self.session.recipient);
            self.session = Session {
                goal,
                recipient,
                ..session
            };
            if self.anonymize.analysis.is_some() {
                self.anonymize.refresh_preview(&self.session);
            }
        }
        if no_folder
            && self
                .anonymize
                .add_named_text(express::DOCUMENT_NAME, text)
                .is_ok()
        {
            self.start_analysis();
        }
    }

    fn express_restore(&mut self) {
        let Some(engine) = self.engine.clone() else {
            return self.notify(ToastKind::Warning, express::ENGINE_LOADING);
        };
        let session = match &self.deanonymize.map {
            MapStatus::Unlocked { session, .. } => session.clone(),
            MapStatus::Locked { .. } => {
                return self.notify(ToastKind::Warning, express::KEY_LOCKED);
            }
            MapStatus::None => {
                let key = hotkeys::MASK.label();
                return self.notify(ToastKind::Info, express::no_key_notice(&key));
            }
        };
        let text = match express::usable_text(self.clipboard.read_text()) {
            Ok(t) => t,
            Err((kind, why)) => return self.notify(kind, why),
        };
        let r = express::restore(&engine, &text, &session);
        if r.restored == 0 && r.unknown.is_empty() {
            return self.notify(ToastKind::Info, express::NO_TAG);
        }
        // Vraies valeurs : hors de l'historique du presse-papiers du système.
        if r.restored > 0 && !self.clipboard.write_text(&r.text, true) {
            return self.notify(ToastKind::Error, express::WRITE_FAILED);
        }
        let kind = if r.unknown.is_empty() {
            ToastKind::Success
        } else {
            ToastKind::Warning
        };
        self.notify(kind, express::restored_notice(r.restored, &r.unknown));
    }

    /// Choix fait dans la vérification : remplacer ou annuler.
    fn express_review_outcome(&mut self) {
        let outcome = self
            .express
            .with_review(|r| std::mem::take(&mut r.outcome))
            .unwrap_or_default();
        match outcome {
            ReviewOutcome::None => {}
            ReviewOutcome::Cancel => {
                self.express.review = None;
                self.notify(ToastKind::Info, express::CANCELLED);
            }
            // Écriture du dossier en cours : elle remplacerait la clé.
            ReviewOutcome::Replace if self.is_busy() => {
                self.notify(ToastKind::Warning, express::APP_BUSY);
            }
            ReviewOutcome::Replace => {
                if let Some(shared) = self.express.review.take() {
                    let prepared = shared
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .prepared
                        .clone();
                    self.express_commit(&prepared);
                }
            }
        }
    }

    /// Fenêtres du presse-papiers : notification et vérification, demandées à
    /// chaque image de la fenêtre principale tant qu'elles sont ouvertes.
    pub(super) fn express_windows(&mut self, ctx: &egui::Context) {
        self.express_shown = self.express.notice.is_some() || self.express.review.is_some();
        // Taille de l'écran retenue : fenêtre réduite, le système ne la donne plus.
        if let Some(m) = ctx.input(|i| i.viewport().monitor_size) {
            self.monitor = Some(m);
        }
        if let Some(notice) = self.express.notice.clone() {
            let monitor = self.monitor;
            let mut builder = ViewportBuilder::default()
                .with_title(NOTICE_TITLE)
                .with_inner_size(NOTICE_SIZE)
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_active(false)
                .with_taskbar(false)
                .with_visible(false);
            if let Some(pos) = notice_position(monitor) {
                builder = builder.with_position(pos);
            }
            ctx.show_viewport_deferred(notice_id(), builder, move |ui, class| {
                theme::sync(ui);
                fixed_area(ui, class, NOTICE_SIZE, |ui| {
                    screens::express::notification(ui, &notice);
                });
            });
        }
        let Some(shared) = self.express.review.clone() else {
            return;
        };
        let builder = ViewportBuilder::default()
            .with_title(REVIEW_TITLE)
            .with_inner_size(REVIEW_SIZE)
            .with_min_inner_size(REVIEW_MIN)
            .with_always_on_top()
            .with_active(true)
            .with_visible(false);
        ctx.show_viewport_deferred(review_id(), builder, move |ui, class| {
            theme::sync(ui);
            let mut r = shared.lock().unwrap_or_else(PoisonError::into_inner);
            let closing = ui.input(|i| i.viewport().close_requested());
            let mut outcome = ReviewOutcome::None;
            fixed_area(ui, class, REVIEW_SIZE, |ui| {
                outcome = screens::express::review(ui, &mut r);
            });
            if closing {
                outcome = ReviewOutcome::Cancel;
            }
            if outcome != ReviewOutcome::None {
                r.outcome = outcome;
                // La fenêtre principale traite le choix (`App::logic`).
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
        });
        self.reveal_windows(ctx);
        // Le focus va à la vérification dès que sa fenêtre existe.
        let wants_focus = self.express.with_review(|r| r.focus).unwrap_or(false);
        if wants_focus && self.express_focus > 0 {
            let focused = ctx.input(|i| i.raw.viewports.get(&review_id()).and_then(|v| v.focused));
            if focused == Some(true) {
                self.express.with_review(|r| r.focus = false);
                self.express_focus = 0;
            } else {
                ctx.send_viewport_cmd_to(review_id(), ViewportCommand::Focus);
                self.express_focus -= 1;
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        // Intégrée à la fenêtre principale (tests), la vérification a déjà
        // tourné : son choix se traite tout de suite.
        if ctx.embed_viewports() {
            self.express_review_outcome();
        }
    }
}

impl AnonApp {
    /// Montre les fenêtres du presse-papiers une fois créées. Elles naissent
    /// cachées : sous Windows, winit montre une fenêtre visible à sa taille par
    /// défaut avant de lui donner la sienne, et ce grand cadre vide se voit
    /// quand la fenêtre principale est réduite. La notification se montre sans
    /// prendre le focus (première apparition sans activation), la vérification
    /// le prend.
    fn reveal_windows(&self, ctx: &egui::Context) {
        let open = [
            (notice_id(), self.express.notice.is_some()),
            (review_id(), self.express.review.is_some()),
        ];
        for (id, wanted) in open {
            if wanted {
                // Gardée jusqu'à la création de la fenêtre ; sans effet une
                // fois visible.
                ctx.send_viewport_cmd_to(id, ViewportCommand::Visible(true));
            }
        }
        // Juste après l'ouverture, la fenêtre est créée au tour suivant de la
        // boucle : quelques images rapprochées pour la montrer aussitôt.
        if !ctx.embed_viewports() && self.clock() < self.express_reveal_until {
            ctx.request_repaint_after(std::time::Duration::from_millis(REVEAL_POLL_MS));
        }
    }
}

/// Zone de dessin d'une fenêtre du presse-papiers : toute la fenêtre, ou une
/// taille fixe quand egui l'intègre dans la fenêtre principale (tests).
fn fixed_area(
    ui: &mut egui::Ui,
    class: ViewportClass,
    size: [f32; 2],
    add: impl FnOnce(&mut egui::Ui),
) {
    let size = if class == ViewportClass::EmbeddedWindow {
        egui::vec2(size[0], size[1])
    } else {
        ui.available_size()
    };
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add);
}
