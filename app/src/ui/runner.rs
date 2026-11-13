//! Fil de travail : les traitements longs (chargement du moteur, analyse,
//! anonymisation, ouverture et scellement de la table, restauration) tournent
//! hors du fil de l'interface. Résultats par canal, avancement et annulation
//! par un état partagé.

use crate::moteur::IaStatus;
use crate::ui::service::{BatchReport, Rejected, RestoreOutcome};
use anon_core::Session;
use anon_engine::{Analysis, Engine, InputFile, Progress};
use anon_vault::VaultError;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// État d'un fichier pendant un traitement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Pending,
    Running,
    Done,
    Failed,
}

impl FileStatus {
    pub fn label(self) -> &'static str {
        match self {
            FileStatus::Pending => "En attente",
            FileStatus::Running => "En cours",
            FileStatus::Done => "Terminé",
            FileStatus::Failed => "Erreur",
        }
    }
}

/// Avancement partagé entre le fil de travail et l'interface.
#[derive(Debug, Default)]
pub struct Shared {
    done: AtomicUsize,
    total: AtomicUsize,
    cancel: AtomicBool,
    /// Nom du fichier en cours (affichage seulement, jamais journalisé).
    label: Mutex<String>,
    /// Fichiers en erreur (indices).
    failed: Mutex<Vec<usize>>,
    /// Début du traitement.
    started: Mutex<Option<Instant>>,
}

impl Shared {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        self.done.load(Ordering::Relaxed) as f32 / total as f32
    }

    pub fn label(&self) -> String {
        self.label.lock().map(|l| l.clone()).unwrap_or_default()
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Le fichier `index` a échoué (contrôle de fuite, format illisible…).
    pub fn mark_failed(&self, index: usize) {
        if let Ok(mut f) = self.failed.lock() {
            f.push(index);
        }
    }

    /// État du fichier `index` : terminé avant le fichier en cours, en cours,
    /// en attente après lui.
    pub fn status(&self, index: usize) -> FileStatus {
        if self.failed.lock().is_ok_and(|f| f.contains(&index)) {
            return FileStatus::Failed;
        }
        let done = self.done.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        if index < done {
            FileStatus::Done
        } else if index == done && done < total {
            FileStatus::Running
        } else {
            FileStatus::Pending
        }
    }

    /// Secondes écoulées depuis le début du traitement (0 s'il n'a pas démarré).
    pub fn elapsed_secs(&self) -> u64 {
        self.started
            .lock()
            .ok()
            .and_then(|s| *s)
            .map_or(0, |t| t.elapsed().as_secs())
    }

    fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        if let Ok(mut l) = self.label.lock() {
            l.clear();
        }
        if let Ok(mut f) = self.failed.lock() {
            f.clear();
        }
        if let Ok(mut s) = self.started.lock() {
            *s = Some(Instant::now());
        }
    }
}

/// Durée lisible : « 8 s », « 1 min 05 s ».
pub fn duration_label(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min {:02} s", secs / 60, secs % 60)
    }
}

impl Progress for Shared {
    fn step(&self, done: usize, total: usize, label: &str) {
        self.done.store(done, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        if let Ok(mut l) = self.label.lock() {
            label.clone_into(&mut l);
        }
    }

    fn cancelled(&self) -> bool {
        self.is_cancel_requested()
    }
}

/// Ce que renvoie un traitement.
pub enum Msg {
    EngineReady(Arc<Engine>, IaStatus),
    Analyzed {
        inputs: Vec<InputFile>,
        rejected: Vec<Rejected>,
        analysis: Analysis,
    },
    Anonymized {
        report: BatchReport,
        session: Session,
        inputs: Vec<InputFile>,
        analysis: Analysis,
    },
    /// Table ouverte pour l'onglet Désanonymiser.
    MapOpened(Result<Session, VaultError>),
    /// Table existante reprise à l'étape Protéger (mot de passe gardé pour resceller).
    ExistingOpened(Result<Session, VaultError>),
    Restored(Vec<RestoreOutcome>),
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Msg::EngineReady(..) => "EngineReady",
            Msg::Analyzed { .. } => "Analyzed",
            Msg::Anonymized { .. } => "Anonymized",
            Msg::MapOpened(_) => "MapOpened",
            Msg::ExistingOpened(_) => "ExistingOpened",
            Msg::Restored(_) => "Restored",
        };
        f.write_str(name)
    }
}

/// Lance les traitements, un à la fois (l'interface désactive les actions
/// pendant un traitement).
pub struct Runner {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub shared: Arc<Shared>,
    running: usize,
    /// Exécute sur le fil appelant (tests et instantanés).
    synchronous: bool,
    repaint: Option<egui::Context>,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner")
            .field("running", &self.running)
            .field("synchronous", &self.synchronous)
            .finish_non_exhaustive()
    }
}

impl Runner {
    pub fn new(repaint: Option<egui::Context>) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            shared: Arc::new(Shared::default()),
            running: 0,
            synchronous: false,
            repaint,
        }
    }

    /// Variante qui exécute chaque traitement immédiatement.
    pub fn synchronous() -> Self {
        let mut r = Self::new(None);
        r.synchronous = true;
        r
    }

    pub fn is_busy(&self) -> bool {
        self.running > 0
    }

    pub fn spawn(&mut self, job: impl FnOnce(&Shared) -> Msg + Send + 'static) {
        self.shared.reset();
        self.running += 1;
        if self.synchronous {
            let msg = job(&self.shared);
            let _ = self.tx.send(msg);
            return;
        }
        let tx = self.tx.clone();
        let shared = Arc::clone(&self.shared);
        let ctx = self.repaint.clone();
        std::thread::spawn(move || {
            let msg = job(&shared);
            let _ = tx.send(msg);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// Résultats arrivés depuis le dernier appel.
    pub fn poll(&mut self) -> Vec<Msg> {
        let msgs: Vec<Msg> = self.rx.try_iter().collect();
        self.running = self.running.saturating_sub(msgs.len());
        msgs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_and_cancel() {
        let s = Shared::default();
        assert_eq!(s.fraction(), 0.0);
        s.step(1, 4, "a.txt");
        assert_eq!(s.fraction(), 0.25);
        assert_eq!(s.label(), "a.txt");
        assert!(!s.cancelled());
        s.request_cancel();
        assert!(s.cancelled());
        s.reset();
        assert!(!s.cancelled());
    }

    #[test]
    fn file_statuses() {
        let s = Shared::default();
        assert_eq!(s.status(0), FileStatus::Pending);
        assert_eq!(s.elapsed_secs(), 0);
        s.step(1, 3, "b.txt");
        s.mark_failed(0);
        assert_eq!(s.status(0), FileStatus::Failed);
        assert_eq!(s.status(1), FileStatus::Running);
        assert_eq!(s.status(2), FileStatus::Pending);
        s.step(3, 3, "");
        assert_eq!(s.status(2), FileStatus::Done);
        s.reset();
        assert_eq!(s.status(0), FileStatus::Pending);
        assert_eq!(duration_label(8), "8 s");
        assert_eq!(duration_label(65), "1 min 05 s");
    }

    #[test]
    fn threaded_job_reports_back() {
        let mut r = Runner::new(None);
        r.spawn(|_| Msg::Restored(Vec::new()));
        assert!(r.is_busy());
        let mut got = Vec::new();
        for _ in 0..200 {
            got = r.poll();
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(got.first(), Some(Msg::Restored(_))));
        assert!(!r.is_busy());
    }

    #[test]
    fn synchronous_job_is_ready_at_once() {
        let mut r = Runner::synchronous();
        r.spawn(|_| Msg::MapOpened(Err(VaultError::NotAVault)));
        assert!(matches!(r.poll().first(), Some(Msg::MapOpened(Err(_)))));
    }
}
