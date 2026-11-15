//! Presse-papiers express (maquette F) : masquer le texte copié avec la clé du
//! dossier (Ctrl+Alt+M), restaurer la réponse copiée (Ctrl+Alt+R), depuis
//! n'importe quelle application. Logique sans egui ni système : le
//! presse-papiers et les raccourcis passent par les traits de `ui::clipboard`
//! et `ui::hotkeys`, simulés dans les tests.
//!
//! Masquer : `Engine::analyze_text` sur la session de départ (clé ouverte, ou
//! session du dossier), puis les valeurs déjà décidées (clé, dossier) et les
//! valeurs sûres ; s'il reste des valeurs ambiguës ou incertaines non
//! tranchées (même règle que la file « À décider »), une vérification s'ouvre.
//! Le texte masqué (`Engine::anonymize_text`) n'est rendu que si aucune valeur
//! validée n'y subsiste.
//!
//! Les notifications ne contiennent que des nombres et des balises, jamais le
//! texte copié.

use super::anonymize::needs_confirmation;
use super::toasts::{ToastKind, count_label};
use anon_core::{EntityKind, Goal, Session, Tag};
use anon_engine::{Analysis, AnalyzeOptions, Candidate, DocumentView, Engine};
use anon_replace::Replacer;
use anon_replace::session_ext::match_key;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

/// Texte copié le plus long accepté (octets) : au-delà, il se colle dans
/// l'application, où l'analyse se suit et s'annule.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Durée d'affichage de la notification, en secondes.
pub const NOTICE_SECONDS: f64 = 4.0;
/// Nom de la clé en mémoire créée par un premier masquage sans dossier.
pub const KEY_NAME: &str = "Presse-papiers";
/// Nom du document du dossier qui garde le premier texte masqué sans dossier.
pub const DOCUMENT_NAME: &str = "presse-papiers.txt";
/// Balises inconnues citées au plus dans une notification.
const UNKNOWN_SHOWN: usize = 3;

/// Presse-papiers vide, ou sans texte (image, fichiers).
pub const EMPTY: &str = "Presse-papiers vide ou sans texte";
/// Presse-papiers tenu par une autre application.
pub const CLIPBOARD_BUSY: &str = "Presse-papiers occupé par une autre application : réessayez";
/// Texte au-delà de [`MAX_TEXT_BYTES`].
pub const TOO_LONG: &str =
    "Texte copié trop long pour le presse-papiers express : collez-le dans l'application";
/// Moteur pas encore chargé (première seconde).
pub const ENGINE_LOADING: &str = "Moteur en cours de chargement : réessayez dans un instant";
/// Traitement en cours dans le dossier (analyse, écriture…).
pub const APP_BUSY: &str = "Traitement en cours dans l'application : réessayez ensuite";
/// Clé choisie mais pas encore ouverte.
pub const KEY_LOCKED: &str = "Ouvrez d'abord la clé dans l'application : presse-papiers inchangé";
/// Vérification finale en échec.
pub const LEAK: &str = "Vérification finale en échec : presse-papiers inchangé";
/// Écriture du presse-papiers refusée par le système.
pub const WRITE_FAILED: &str = "Presse-papiers non remplacé : réessayez";
/// Aucune valeur trouvée dans le texte copié.
pub const NOTHING_TO_MASK: &str = "Aucune valeur à masquer : presse-papiers inchangé";
/// Aucune balise dans le texte copié.
pub const NO_TAG: &str = "Aucune balise à restaurer : presse-papiers inchangé";
/// Vérification annulée.
pub const CANCELLED: &str = "Presse-papiers inchangé";
/// La clé ou le dossier ont changé pendant la vérification.
pub const CHANGED: &str = "La clé a changé pendant la vérification : presse-papiers inchangé";

/// Action d'un raccourci global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Masquer le texte copié.
    Mask,
    /// Restaurer le texte copié.
    Restore,
}

impl Action {
    pub const ALL: [Action; 2] = [Action::Mask, Action::Restore];

    /// Libellé de la palette et du panneau des raccourcis.
    pub fn label(self) -> &'static str {
        match self {
            Action::Mask => "Masquer le presse-papiers",
            Action::Restore => "Restaurer le presse-papiers",
        }
    }
}

/// Ce que contient le presse-papiers.
#[derive(Clone, PartialEq, Eq)]
pub enum ClipboardRead {
    Text(String),
    /// Vide, ou sans texte (image, fichiers).
    Empty,
    /// Inaccessible (tenu par une autre application).
    Unavailable,
}

impl std::fmt::Debug for ClipboardRead {
    /// Jamais le texte : sa longueur seulement.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardRead::Text(t) => write!(f, "Text({} octets)", t.len()),
            ClipboardRead::Empty => f.write_str("Empty"),
            ClipboardRead::Unavailable => f.write_str("Unavailable"),
        }
    }
}

/// Texte lisible pour le presse-papiers express, ou la notification qui dit
/// pourquoi il ne l'est pas.
pub fn usable_text(read: ClipboardRead) -> Result<String, (ToastKind, &'static str)> {
    match read {
        ClipboardRead::Text(t) if t.len() > MAX_TEXT_BYTES => Err((ToastKind::Warning, TOO_LONG)),
        ClipboardRead::Text(t) if !t.trim().is_empty() => Ok(t),
        ClipboardRead::Text(_) | ClipboardRead::Empty => Err((ToastKind::Info, EMPTY)),
        ClipboardRead::Unavailable => Err((ToastKind::Warning, CLIPBOARD_BUSY)),
    }
}

/// Décisions reprises du dossier pour le texte copié, par type et forme
/// comparable (`match_key`) : masquer (`true`) ou garder visible.
///
/// Masquer : toute valeur cochée, sûre ou tranchée par l'utilisateur. Garder :
/// seulement une valeur que l'utilisateur a tranchée (une valeur de tableau
/// ajoutée décochée pour l'évaluation n'est pas une décision).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct KnownDecisions(Vec<(EntityKind, String, bool)>);

impl std::fmt::Debug for KnownDecisions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KnownDecisions({})", self.0.len())
    }
}

impl KnownDecisions {
    pub fn from_candidates(candidates: &[Candidate], decided: &BTreeSet<usize>) -> Self {
        let mut v = Vec::new();
        for c in candidates {
            let by_user = decided.contains(&c.id);
            let mask = if c.selected && (by_user || !needs_confirmation(c)) {
                true
            } else if !c.selected && by_user {
                false
            } else {
                continue;
            };
            for form in std::iter::once(&c.value).chain(&c.variants) {
                v.push((c.kind, match_key(c.kind, form), mask));
            }
        }
        Self(v)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Décision connue pour une valeur du texte copié.
    pub fn get(&self, c: &Candidate) -> Option<bool> {
        std::iter::once(&c.value)
            .chain(&c.variants)
            .find_map(|form| {
                let k = match_key(c.kind, form);
                self.0
                    .iter()
                    .find(|(kind, key, _)| *kind == c.kind && *key == k)
                    .map(|(_, _, mask)| *mask)
            })
    }
}

/// Demande de masquage, confiée au fil de travail.
pub struct MaskRequest {
    pub text: String,
    /// Session de départ : clé ouverte, sinon session du dossier.
    pub base: Session,
    /// Réglages de l'analyse du dossier (profil, listes « toujours » et « jamais »).
    pub options: AnalyzeOptions,
    pub known: KnownDecisions,
}

impl std::fmt::Debug for MaskRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaskRequest")
            .field("len", &self.text.len())
            .field("entities", &self.base.entities.len())
            .field("known", &self.known)
            .finish_non_exhaustive()
    }
}

/// Texte copié analysé, décisions reprises.
#[derive(Clone)]
pub struct Prepared {
    pub text: String,
    /// Session de départ (objectif « Masquer pour l'IA »).
    pub base: Session,
    pub analysis: Analysis,
    /// Valeurs tranchées : déjà dans la clé, décidées dans le dossier, ou par
    /// l'utilisateur dans la vérification.
    pub decided: BTreeSet<usize>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("len", &self.text.len())
            .field("candidates", &self.analysis.candidates.len())
            .field("decided", &self.decided.len())
            .finish_non_exhaustive()
    }
}

/// Analyse le texte copié avec la session de départ et reprend les décisions
/// connues. Une valeur déjà dans la clé garde sa balise et n'est jamais redemandée.
pub fn prepare(engine: &Engine, request: MaskRequest) -> Prepared {
    let mut base = request.base;
    base.goal = Goal::Mask;
    let mut analysis = engine.analyze_text(&request.text, &request.options, &base);
    let mut decided = BTreeSet::new();
    let known: Vec<(usize, Option<bool>, bool)> = analysis
        .candidates
        .iter()
        .map(|c| (c.id, request.known.get(c), c.existing_tag.is_some()))
        .collect();
    for (id, known, in_key) in known {
        if in_key {
            decided.insert(id);
        } else if let Some(mask) = known {
            analysis.set_selected(id, mask);
            decided.insert(id);
        }
    }
    Prepared {
        text: request.text,
        base,
        analysis,
        decided,
    }
}

impl Prepared {
    pub fn candidate(&self, id: usize) -> Option<&Candidate> {
        self.analysis.candidates.iter().find(|c| c.id == id)
    }

    /// Vrai si la valeur attend une décision.
    pub fn is_pending(&self, id: usize) -> bool {
        !self.decided.contains(&id) && self.candidate(id).is_some_and(needs_confirmation)
    }

    /// Valeurs qui attendent une décision.
    pub fn pending_count(&self) -> usize {
        self.analysis
            .candidates
            .iter()
            .filter(|c| self.is_pending(c.id))
            .count()
    }

    /// Valeurs cochées : masquées si le presse-papiers est remplacé maintenant.
    pub fn masked_count(&self) -> usize {
        self.analysis
            .candidates
            .iter()
            .filter(|c| c.selected)
            .count()
    }

    /// Texte copié et occurrences de ses valeurs, selon la sélection courante.
    pub fn view(&self) -> Option<DocumentView> {
        self.analysis.document(0, &self.base)
    }
}

/// Texte masqué, prêt pour le presse-papiers.
#[derive(Clone, PartialEq)]
pub struct Masked {
    pub text: String,
    /// Session de départ et valeurs du texte copié : la clé à garder ouverte.
    pub session: Session,
    /// Valeurs distinctes masquées dans le texte.
    pub values: usize,
}

impl std::fmt::Debug for Masked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Masked")
            .field("len", &self.text.len())
            .field("values", &self.values)
            .finish_non_exhaustive()
    }
}

/// Masquage impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskError {
    /// Une valeur validée subsiste dans le texte masqué.
    Leak,
}

/// Enregistre les valeurs cochées dans une copie de la session de départ,
/// masque le texte, puis vérifie qu'aucune valeur validée n'y subsiste.
pub fn finish(engine: &Engine, prepared: &Prepared) -> Result<Masked, MaskError> {
    let mut session = prepared.base.clone();
    engine.apply_selection(&prepared.analysis, &mut session);
    let masked = engine.anonymize_text(&prepared.text, &session);
    leak_check(&prepared.text, masked, session)
}

/// Vérification finale du texte masqué : aucune valeur de `session` n'y
/// subsiste. Compte les valeurs distinctes masquées dans `original`.
pub fn leak_check(original: &str, masked: String, session: Session) -> Result<Masked, MaskError> {
    let replacer = Replacer::new(&session);
    if !replacer.matches(&masked).is_empty() {
        return Err(MaskError::Leak);
    }
    let values: BTreeSet<Tag> = replacer.matches(original).iter().map(|m| m.tag).collect();
    Ok(Masked {
        text: masked,
        session,
        values: values.len(),
    })
}

/// Texte copié restauré.
#[derive(Clone, PartialEq, Eq)]
pub struct Restored {
    pub text: String,
    /// Balises remplacées par leur valeur.
    pub restored: usize,
    /// Balises de type valide mais absentes de la clé (forme `[TYPE_N]`).
    pub unknown: Vec<String>,
}

impl std::fmt::Debug for Restored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Restored")
            .field("len", &self.text.len())
            .field("restored", &self.restored)
            .field("unknown", &self.unknown.len())
            .finish()
    }
}

/// Restaure le texte copié avec la clé ; les balises inconnues restent telles quelles.
pub fn restore(engine: &Engine, text: &str, session: &Session) -> Restored {
    let (out, report) = engine.restore_text(text, session);
    Restored {
        text: out,
        restored: report.restored,
        unknown: report.unknown,
    }
}

/// « 12 valeurs masquées, presse-papiers prêt ».
pub fn masked_notice(values: usize) -> String {
    format!(
        "{}, presse-papiers prêt",
        count_label(values, "valeur masquée", "valeurs masquées")
    )
}

/// « 9 balises restaurées · 1 inconnue : [PERSONNE_9] » : nombres et balises
/// seulement (au plus trois balises citées).
pub fn restored_notice(restored: usize, unknown: &[String]) -> String {
    let mut s = count_label(restored, "balise restaurée", "balises restaurées");
    if !unknown.is_empty() {
        let shown: Vec<&str> = unknown
            .iter()
            .take(UNKNOWN_SHOWN)
            .map(String::as_str)
            .collect();
        let more = if unknown.len() > UNKNOWN_SHOWN {
            "…"
        } else {
            ""
        };
        s.push_str(&format!(
            " · {} : {}{more}",
            count_label(unknown.len(), "inconnue", "inconnues"),
            shown.join(", ")
        ));
    }
    s
}

/// « Aucune clé ouverte » : comment en avoir une.
pub fn no_key_notice(mask_shortcut: &str) -> String {
    format!(
        "Aucune clé ouverte : masquez d'abord un texte ({mask_shortcut}), presse-papiers inchangé"
    )
}

/// Vrai si `new` prolonge `old` : chaque valeur de `old` y garde sa balise.
pub fn extends(new: &Session, old: &Session) -> bool {
    old.entities
        .iter()
        .all(|e| new.entity(e.tag).is_some_and(|n| n.value == e.value))
}

/// Notification du presse-papiers express : petite fenêtre au premier plan,
/// en bas à droite de l'écran, quelques secondes.
#[derive(Clone, PartialEq)]
pub struct Notice {
    pub kind: ToastKind,
    pub text: String,
    /// Instant d'apparition (secondes, horloge de l'application).
    pub since: f64,
}

impl std::fmt::Debug for Notice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notice")
            .field("kind", &self.kind)
            .field("since", &self.since)
            .finish_non_exhaustive()
    }
}

impl Notice {
    pub fn expired(&self, now: f64) -> bool {
        now - self.since >= NOTICE_SECONDS
    }
}

/// Ce que l'utilisateur a choisi dans la vérification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewOutcome {
    /// Rien encore.
    #[default]
    None,
    /// « Remplacer le presse-papiers » (Entrée).
    Replace,
    /// « Annuler » (Échap, ou fermeture de la fenêtre).
    Cancel,
}

/// Vérification du texte copié avant de remplacer le presse-papiers : texte
/// surligné et file des valeurs à décider (M, G, J, K).
pub struct ExpressReview {
    pub prepared: Prepared,
    /// Valeur courante de la file.
    pub current: Option<usize>,
    /// Valeurs de la file : celles qui attendaient une décision à l'ouverture.
    queued: BTreeSet<usize>,
    view: Option<DocumentView>,
    /// La fenêtre doit prendre le focus à la prochaine image.
    pub focus: bool,
    /// Faire défiler le texte jusqu'à la valeur courante à la prochaine image.
    pub reveal: bool,
    /// Choix fait dans la fenêtre, à traiter par l'application.
    pub outcome: ReviewOutcome,
}

impl std::fmt::Debug for ExpressReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpressReview")
            .field("prepared", &self.prepared)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl ExpressReview {
    pub fn new(prepared: Prepared) -> Self {
        let queued = prepared
            .analysis
            .candidates
            .iter()
            .filter(|c| prepared.is_pending(c.id))
            .map(|c| c.id)
            .collect();
        let mut r = Self {
            view: prepared.view(),
            prepared,
            current: None,
            queued,
            focus: true,
            reveal: true,
            outcome: ReviewOutcome::None,
        };
        r.current = r.queue().into_iter().find(|id| r.prepared.is_pending(*id));
        r
    }

    /// Texte surligné (sélection courante).
    pub fn view(&self) -> Option<&DocumentView> {
        self.view.as_ref()
    }

    /// File « À décider », dans l'ordre du texte : valeurs ambiguës ou
    /// incertaines qui n'étaient pas déjà tranchées.
    pub fn queue(&self) -> Vec<usize> {
        let order = self
            .view
            .as_ref()
            .map(DocumentView::candidates_in_order)
            .unwrap_or_default();
        order
            .into_iter()
            .filter(|id| self.queued.contains(id))
            .collect()
    }

    /// Avancement : (décidées, total).
    pub fn progress(&self) -> (usize, usize) {
        let q = self.queue();
        let done = q
            .iter()
            .filter(|id| !self.prepared.is_pending(**id))
            .count();
        (done, q.len())
    }

    /// J (suivante) ou K (précédente), en reprenant au début.
    pub fn step(&mut self, forward: bool) -> Option<usize> {
        let q = self.queue();
        let len = q.len();
        if len == 0 {
            return None;
        }
        let pos = self.current.and_then(|c| q.iter().position(|x| *x == c));
        let next = match (pos, forward) {
            (Some(p), true) => (p + 1) % len,
            (Some(p), false) => (p + len - 1) % len,
            (None, true) => 0,
            (None, false) => len - 1,
        };
        self.current = q.get(next).copied();
        self.reveal = true;
        self.current
    }

    /// M (masquer) ou G (garder) la valeur courante ; la file passe à la
    /// suivante qui attend une décision.
    pub fn decide(&mut self, mask: bool) -> Option<usize> {
        let id = self.current?;
        self.prepared.analysis.set_selected(id, mask);
        self.prepared.decided.insert(id);
        self.view = self.prepared.view();
        let q = self.queue();
        let start = q.iter().position(|x| *x == id).map_or(0, |p| p + 1);
        let len = q.len().max(1);
        self.current = (0..q.len())
            .filter_map(|k| q.get((start + k) % len).copied())
            .find(|x| self.prepared.is_pending(*x));
        self.reveal = true;
        Some(id)
    }
}

/// Vérification partagée avec sa fenêtre : la fenêtre (viewport egui différé)
/// se dessine dans sa propre image, même quand la fenêtre principale est réduite.
pub type SharedReview = Arc<Mutex<ExpressReview>>;

/// État du presse-papiers express dans l'application.
#[derive(Debug, Default)]
pub struct ExpressState {
    /// Texte copié en cours d'analyse sur le fil de travail.
    pub preparing: bool,
    /// Vérification ouverte.
    pub review: Option<SharedReview>,
    /// Notification affichée.
    pub notice: Option<Notice>,
    /// Raccourcis globaux dont l'échec d'enregistrement a déjà été signalé.
    pub reported: Vec<Action>,
}

impl ExpressState {
    /// Lit ou modifie la vérification ouverte.
    pub fn with_review<R>(&self, f: impl FnOnce(&mut ExpressReview) -> R) -> Option<R> {
        self.review
            .as_ref()
            .map(|r| f(&mut r.lock().unwrap_or_else(PoisonError::into_inner)))
    }

    /// Retire la notification échue.
    pub fn expire(&mut self, now: f64) {
        if self.notice.as_ref().is_some_and(|n| n.expired(now)) {
            self.notice = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::{DetectOptions, Source};
    use anon_replace::SessionExt;
    use std::sync::OnceLock;

    /// Moteur standard (règles et dictionnaires), partagé par les tests.
    fn engine() -> &'static Engine {
        static ENGINE: OnceLock<Engine> = OnceLock::new();
        ENGINE.get_or_init(Engine::standard)
    }

    fn options(always: &[(&str, EntityKind)]) -> AnalyzeOptions {
        AnalyzeOptions {
            detect: DetectOptions::default(),
            always: always.iter().map(|(t, k)| ((*t).to_owned(), *k)).collect(),
            never: Vec::new(),
        }
    }

    fn request(text: &str, base: Session, always: &[(&str, EntityKind)]) -> MaskRequest {
        MaskRequest {
            text: text.to_owned(),
            base,
            options: options(always),
            known: KnownDecisions::default(),
        }
    }

    /// Clé d'un dossier déjà envoyé : Jean Dupont y a sa balise.
    fn folder_key() -> Session {
        let mut s = Session::new();
        let _ = s.intern(
            EntityKind::Societe,
            "Nautilus Pacifique SARL",
            Source::Manual,
        );
        let _ = s.intern(EntityKind::Personne, "Jean Dupont", Source::Manual);
        s
    }

    fn tag_of(s: &Session, kind: EntityKind, value: &str) -> String {
        s.tag_for(kind, value)
            .map(|t| t.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn mask_with_the_folder_key_keeps_its_tags() {
        let base = folder_key();
        let tag = tag_of(&base, EntityKind::Personne, "Jean Dupont");
        assert!(!tag.is_empty());
        let text = "Jean Dupont confirme. Écrire à claire.martin@example.com pour la suite.";
        let p = prepare(engine(), request(text, base.clone(), &[]));
        assert_eq!(p.pending_count(), 0, "rien à décider");
        let m = finish(engine(), &p).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(m.text.contains(&tag), "même balise que dans le dossier");
        assert!(!m.text.contains("Jean Dupont"));
        assert!(!m.text.contains("claire.martin@example.com"));
        assert_eq!(m.values, 2);
        assert!(
            extends(&m.session, &base),
            "la clé du dossier est prolongée"
        );
        assert!(m.session.entities.len() > base.entities.len());
        // Restauration avec la clé prolongée : le texte d'origine revient.
        let r = restore(engine(), &m.text, &m.session);
        assert_eq!(r.text, text);
        assert_eq!(r.restored, 2);
        assert!(r.unknown.is_empty());
    }

    #[test]
    fn mask_with_a_new_session() {
        let text = "Contact : claire.martin@example.com, 06 12 34 56 78.";
        let p = prepare(engine(), request(text, Session::new(), &[]));
        let m = finish(engine(), &p).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(m.session.goal, Goal::Mask, "objectif « Masquer pour l'IA »");
        assert_eq!(m.values, 2);
        assert!(!m.text.contains("example.com"));
        assert_eq!(restore(engine(), &m.text, &m.session).text, text);
    }

    #[test]
    fn values_to_decide_open_the_review() {
        // « Léo » (liste « toujours masquer ») : valeur courte, donc à décider.
        let text = "Léo a rappelé Jean Dupont.\nLéo passera demain.";
        let always = [("Léo", EntityKind::Personne)];
        let p = prepare(engine(), request(text, folder_key(), &always));
        assert_eq!(p.pending_count(), 1);
        let mut review = ExpressReview::new(p);
        assert!(review.focus, "la fenêtre prend le focus");
        assert_eq!(review.progress(), (0, 1));
        let leo = review
            .current
            .unwrap_or_else(|| panic!("aucune valeur courante"));
        assert_eq!(
            review.prepared.candidate(leo).map(|c| c.value.as_str()),
            Some("Léo")
        );
        // Surlignée dans le texte affiché.
        let view = review.view().unwrap_or_else(|| panic!("aucune vue"));
        assert!(
            view.blocks
                .iter()
                .flat_map(|b| &b.spans)
                .any(|s| s.candidate == leo)
        );
        // J et K restent sur la seule valeur ; G la garde visible.
        assert_eq!(review.step(true), Some(leo));
        assert_eq!(review.step(false), Some(leo));
        assert_eq!(review.decide(false), Some(leo));
        assert_eq!(review.current, None);
        assert_eq!(review.progress(), (1, 1));
        assert_eq!(review.prepared.pending_count(), 0);
        let m = finish(engine(), &review.prepared).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(m.text.contains("Léo a rappelé"), "gardée visible");
        assert!(!m.text.contains("Jean Dupont"));
        // K revient sur la valeur, M la masque.
        assert_eq!(review.step(false), Some(leo));
        assert_eq!(review.decide(true), Some(leo));
        let m = finish(engine(), &review.prepared).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(!m.text.contains("Léo"));
    }

    #[test]
    fn folder_decisions_are_reused() {
        let text = "Léo a rappelé.";
        let always = [("Léo", EntityKind::Personne)];
        // Dossier où l'utilisateur a gardé « Léo » visible.
        let mut folder = engine().analyze_text(text, &options(&always), &Session::new());
        let id = folder.candidates.first().map(|c| c.id).unwrap_or_default();
        folder.set_selected(id, false);
        let known = KnownDecisions::from_candidates(&folder.candidates, &BTreeSet::from([id]));
        assert!(!known.is_empty());
        let p = prepare(
            engine(),
            MaskRequest {
                known,
                ..request(text, Session::new(), &always)
            },
        );
        assert_eq!(p.pending_count(), 0, "déjà décidée dans le dossier");
        assert_eq!(p.masked_count(), 0);
        // Une valeur décochée sans décision (valeur de tableau) n'en est pas une.
        let undecided = KnownDecisions::from_candidates(&folder.candidates, &BTreeSet::new());
        assert!(undecided.is_empty());
        // Masquée dans le dossier : masquée dans le texte copié, sans question.
        folder.set_selected(id, true);
        let known = KnownDecisions::from_candidates(&folder.candidates, &BTreeSet::from([id]));
        let p = prepare(
            engine(),
            MaskRequest {
                known,
                ..request(text, Session::new(), &always)
            },
        );
        assert_eq!(p.pending_count(), 0);
        let m = finish(engine(), &p).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(!m.text.contains("Léo"));
    }

    #[test]
    fn restore_reports_unknown_tags_and_leaves_them() {
        let key = folder_key();
        let tag = tag_of(&key, EntityKind::Personne, "Jean Dupont");
        let text = format!("{tag} valide ; [PERSONNE_99] est inventée.");
        let r = restore(engine(), &text, &key);
        assert_eq!(r.text, "Jean Dupont valide ; [PERSONNE_99] est inventée.");
        assert_eq!(r.restored, 1);
        assert_eq!(r.unknown, ["[PERSONNE_99]"]);
        assert_eq!(
            restored_notice(r.restored, &r.unknown),
            "1 balise restaurée · 1 inconnue : [PERSONNE_99]"
        );
    }

    #[test]
    fn empty_or_non_text_clipboard() {
        assert_eq!(
            usable_text(ClipboardRead::Empty),
            Err((ToastKind::Info, EMPTY))
        );
        assert_eq!(
            usable_text(ClipboardRead::Text(" \n\t".into())),
            Err((ToastKind::Info, EMPTY))
        );
        assert_eq!(
            usable_text(ClipboardRead::Unavailable),
            Err((ToastKind::Warning, CLIPBOARD_BUSY))
        );
        let long = "a".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(
            usable_text(ClipboardRead::Text(long)),
            Err((ToastKind::Warning, TOO_LONG))
        );
        assert_eq!(
            usable_text(ClipboardRead::Text("Bonjour".into())),
            Ok("Bonjour".into())
        );
    }

    #[test]
    fn leak_check_refuses_a_remaining_value() {
        let key = folder_key();
        let tag = tag_of(&key, EntityKind::Personne, "Jean Dupont");
        let still = "Jean Dupont est encore là.";
        assert_eq!(
            leak_check(still, still.to_owned(), key.clone()),
            Err(MaskError::Leak)
        );
        let ok = leak_check("Jean Dupont", tag, key);
        assert_eq!(ok.map(|m| m.values), Ok(1));
    }

    #[test]
    fn notices_hold_only_numbers_and_tags() {
        assert_eq!(
            masked_notice(12),
            "12 valeurs masquées, presse-papiers prêt"
        );
        assert_eq!(masked_notice(1), "1 valeur masquée, presse-papiers prêt");
        assert_eq!(restored_notice(9, &[]), "9 balises restaurées");
        let many: Vec<String> = (1..=5).map(|i| format!("[PERSONNE_{i}]")).collect();
        assert_eq!(
            restored_notice(0, &many),
            "0 balises restaurées · 5 inconnues : [PERSONNE_1], [PERSONNE_2], [PERSONNE_3]…"
        );
        assert!(no_key_notice("Ctrl+Alt+M").contains("Ctrl+Alt+M"));
        // Aucun texte copié dans les représentations de débogage.
        let secret = "Jean Dupont, 12 rue des Lilas";
        let p = prepare(engine(), request(secret, folder_key(), &[]));
        for dbg in [
            format!("{:?}", ClipboardRead::Text(secret.into())),
            format!("{p:?}"),
            format!("{:?}", request(secret, Session::new(), &[])),
            format!("{:?}", ExpressReview::new(p.clone())),
            format!("{:?}", finish(engine(), &p)),
            format!("{:?}", restore(engine(), secret, &folder_key())),
            format!(
                "{:?}",
                Notice {
                    kind: ToastKind::Info,
                    text: secret.into(),
                    since: 0.0
                }
            ),
        ] {
            assert!(!dbg.contains("Dupont") && !dbg.contains("Lilas"), "{dbg}");
        }
    }

    #[test]
    fn notice_lasts_four_seconds() {
        let mut st = ExpressState {
            notice: Some(Notice {
                kind: ToastKind::Success,
                text: masked_notice(3),
                since: 10.0,
            }),
            ..Default::default()
        };
        st.expire(13.9);
        assert!(st.notice.is_some());
        st.expire(14.0);
        assert!(st.notice.is_none());
    }

    #[test]
    fn extends_checks_tags_and_values() {
        let old = folder_key();
        let mut new = old.clone();
        let _ = new.intern(EntityKind::Email, "a@example.com", Source::Rule);
        assert!(extends(&new, &old));
        assert!(!extends(&old, &new));
        assert!(!extends(&Session::new(), &old));
        assert!(extends(&old, &Session::new()));
    }
}
