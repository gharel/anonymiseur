//! Détecteur à base de dictionnaires : personnes, sociétés, communes.
//!
//! Le texte est découpé en jetons (`tokens`), puis regroupé en « suites de noms » :
//! mots à majuscule initiale ou en capitales, reliés par une espace et éventuellement
//! par des particules (`de`, `du`, `van`, `Le`…). Les règles s'appliquent dans cet
//! ordre ; une règle ne propose jamais un intervalle déjà pris par une règle
//! précédente (donc de confiance supérieure ou égale) :
//!
//! 1. civilité ou titre suivi de mots capitalisés ;
//! 2. forme juridique ou mot d'amorce (société) ;
//! 3. prénom + nom (dans les deux ordres) ;
//! 4. commune (seulement si le type `Lieu` est demandé) ;
//! 5. nom ou prénom isolé, hors début de phrase.
//!
//! Un bloc entièrement en capitales ne permet pas de distinguer les noms propres :
//! seules les règles 1 et 2 s'y appliquent.

use crate::tokens::{Case, Kind, Token, tokenize};
use crate::{Tier, common_word_folded, commune_folded, first_name_folded, fold, surname_folded};
use anon_core::detect::{DetectOptions, Detector};
use anon_core::entities::{Detection, EntityKind, Source};
use anon_core::text::TextBlock;

/// Confiance d'un prénom suivi d'un nom (ou l'inverse).
pub const PERSON_CONFIDENCE: f32 = 0.85;
/// Confiance d'un nom ou d'une personne introduits par une civilité ou un titre.
pub const TITLE_NAME_CONFIDENCE: f32 = 0.8;
/// Confiance d'un nom ou d'un prénom isolé, reconnu par le seul dictionnaire.
pub const SOLO_NAME_CONFIDENCE: f32 = 0.5;
/// Confiance d'une société (forme juridique ou mot d'amorce).
pub const ORGANIZATION_CONFIDENCE: f32 = 0.8;
/// Confiance d'une commune.
pub const COMMUNE_CONFIDENCE: f32 = 0.6;

/// Nombre maximal de mots (unités) qui suivent un prénom.
const MAX_PERSON_FOLLOWERS: usize = 3;
/// Nombre maximal d'unités qui suivent une civilité.
const MAX_TITLE_UNITS: usize = 3;
/// Nombre maximal de mots capitalisés d'une raison sociale.
const MAX_ORG_WORDS: usize = 4;
/// Nombre maximal de jetons d'un nom de commune.
const MAX_COMMUNE_TOKENS: usize = 6;
/// Nombre de chiffres d'un code postal.
const POSTAL_CODE_DIGITS: usize = 5;
/// Nombre maximal d'espaces entre deux mots d'une même suite.
const MAX_GAP_SPACES: usize = 2;
/// Longueur minimale (en lettres) d'un nom en capitales isolé.
const MIN_UPPER_SOLO_LETTERS: usize = 3;
/// Palier minimal d'un nom ou prénom isolé.
const MIN_SOLO_TIER: Tier = Tier::Common;
/// Caractères qui, juste avant un mot, en font une étiquette ou un début d'élément.
const LABEL_MARKERS: &[char] = &['-', '–', '—', '•', '#', ':', ';', '>', '|'];

/// Formes juridiques, à casse exacte.
const LEGAL_FORMS: &[&str] = &[
    "SARL", "SAS", "SASU", "SA", "EURL", "SCI", "SNC", "GIE", "SELARL", "SCP", "Ltd", "Inc",
    "GmbH", "LLC",
];

/// Mots d'amorce d'une raison sociale (formes pliées).
const ORG_LEADS: &[&str] = &["societe", "ets", "cabinet", "groupe"];

/// Civilités et titres (formes pliées).
const TITLES: &[&str] = &[
    "mme",
    "mlle",
    "monsieur",
    "madame",
    "maitre",
    "docteur",
    "mademoiselle",
    "professeur",
    "mr",
    "mrs",
    "dr",
];

/// Titres ambigus avec un mot courant : ignorés dans un bloc en capitales.
const AMBIGUOUS_TITLES: &[&str] = &["me", "pr", "ms"];

/// Particules reliant les éléments d'un nom (formes pliées).
const PARTICLES: &[&str] = &[
    "de", "du", "van", "von", "ben", "d'", "del", "della", "di", "da", "ter", "ten", "der", "den",
    "el", "al", "bin", "le", "la",
];

/// Mots de liaison d'une raison sociale ou d'un nom de lieu (formes pliées).
const CONNECTORS: &[&str] = &[
    "et", "de", "du", "des", "la", "le", "les", "d'", "l'", "aux", "au", "en", "sur", "sous",
    "lez", "pour",
];

/// Mots capitalisés qui ne sont jamais des noms propres (formes pliées).
const NON_NAME_WORDS: &[&str] = &[
    "a",
    "afin",
    "ainsi",
    "alors",
    "annexe",
    "apres",
    "article",
    "au",
    "aucun",
    "aucune",
    "aussi",
    "autre",
    "autres",
    "aux",
    "avant",
    "avec",
    "avis",
    "bien",
    "bonjour",
    "bonsoir",
    "car",
    "ce",
    "ceci",
    "cela",
    "celle",
    "celui",
    "ces",
    "cet",
    "cette",
    "chapitre",
    "chaque",
    "chere",
    "cher",
    "chez",
    "comme",
    "comment",
    "contre",
    "cordialement",
    "dans",
    "date",
    "depuis",
    "des",
    "donc",
    "dont",
    "elle",
    "elles",
    "en",
    "entre",
    "envers",
    "et",
    "fait",
    "il",
    "ils",
    "je",
    "les",
    "leur",
    "leurs",
    "lors",
    "lorsque",
    "ma",
    "mais",
    "merci",
    "mes",
    "mon",
    "non",
    "nos",
    "notre",
    "nous",
    "objet",
    "on",
    "oui",
    "ou",
    "par",
    "parce",
    "pendant",
    "plus",
    "pour",
    "puis",
    "quand",
    "que",
    "quel",
    "quelle",
    "quelques",
    "qui",
    "quoi",
    "sa",
    "salutations",
    "sans",
    "section",
    "selon",
    "ses",
    "si",
    "son",
    "sous",
    "sur",
    "ta",
    "tes",
    "titre",
    "ton",
    "tous",
    "tout",
    "toute",
    "toutes",
    "tres",
    "un",
    "une",
    "vers",
    "vos",
    "votre",
    "vous",
    "janvier",
    "fevrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "aout",
    "septembre",
    "octobre",
    "novembre",
    "decembre",
    "lundi",
    "mardi",
    "mercredi",
    "jeudi",
    "vendredi",
    "samedi",
    "dimanche",
];

/// Mots précédant un nom de commune isolé et ambigu (casse minuscule exacte).
const LOCATIVE_WORDS: &[&str] = &["à", "au", "aux", "en", "vers"];

/// Détecteur à base de dictionnaires. Sans état.
#[derive(Debug, Default, Clone, Copy)]
pub struct DictionaryDetector;

impl DictionaryDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Detector for DictionaryDetector {
    fn name(&self) -> &'static str {
        "dictionary"
    }

    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        detect_text(&block.text, opts)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Normal,
    Title,
    Legal,
    OrgLead,
}

/// Un nom : particules éventuelles puis un mot capitalisé (le dernier jeton).
#[derive(Debug, Clone, Copy)]
struct Unit {
    first: usize,
    last: usize,
}

struct Analysis<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    roles: Vec<Role>,
    all_caps: bool,
    opts: &'a DetectOptions,
    out: Vec<Detection>,
    claimed: Vec<(usize, usize)>,
}

fn detect_text(text: &str, opts: &DetectOptions) -> Vec<Detection> {
    if text.is_empty() {
        return Vec::new();
    }
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return Vec::new();
    }
    let all_caps = text.chars().any(char::is_alphabetic) && !text.chars().any(char::is_lowercase);
    let mut a = Analysis {
        text,
        roles: Vec::new(),
        tokens,
        all_caps,
        opts,
        out: Vec::new(),
        claimed: Vec::new(),
    };
    a.roles = (0..a.tokens.len()).map(|i| a.role_of(i)).collect();
    let runs = a.build_runs();
    a.title_rule(&runs);
    a.initials_rule(&runs);
    a.organization_rules();
    if !a.all_caps {
        a.person_rules(&runs);
        a.commune_rule();
        a.solo_rule(&runs);
    }
    a.out.sort_by_key(|d| (d.start, d.end));
    a.out
}

/// Vrai si `pos` commence une phrase (début de bloc, après `.`, `!`, `?`, un
/// saut de ligne ou une puce), en ignorant guillemets et parenthèses ouvrants.
fn is_sentence_start(text: &str, pos: usize) -> bool {
    let Some(before) = text.get(..pos) else {
        return false;
    };
    for (idx, c) in before.char_indices().rev() {
        match c {
            '\n' | '\r' => return true,
            c if c.is_whitespace() => {}
            '«' | '“' | '"' | '(' | '[' | '\'' | '‘' => {}
            '!' | '?' | '…' | '•' | '*' => return true,
            '.' => return !follows_abbreviation(before.get(..idx).unwrap_or("")),
            _ => return false,
        }
    }
    true
}

/// Vrai si le texte se termine par une abréviation (`M`, `Mme`, `Dr`, `J`…) : le
/// point qui suit n'est alors pas une fin de phrase.
fn follows_abbreviation(before_dot: &str) -> bool {
    let word: String = before_dot
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let mut chars = word.chars();
    let (Some(first), second) = (chars.next(), chars.next()) else {
        return false;
    };
    if second.is_none() {
        return first.is_uppercase();
    }
    let folded = fold(&word);
    folded == "st" || folded == "ste" || TITLES.contains(&folded.as_str())
}

/// Vrai si `gap` est un séparateur de mots d'une même suite.
fn is_space_gap(gap: &str) -> bool {
    !gap.is_empty()
        && gap.chars().count() <= MAX_GAP_SPACES
        && gap
            .chars()
            .all(|c| matches!(c, ' ' | '\u{a0}' | '\u{202f}'))
}

impl Analysis<'_> {
    // ----- jetons ---------------------------------------------------------------

    fn token(&self, i: usize) -> Option<&Token> {
        self.tokens.get(i)
    }

    fn gap(&self, a: usize, b: usize) -> &str {
        match (self.token(a), self.token(b)) {
            (Some(x), Some(y)) => self.text.get(x.end..y.start).unwrap_or("x"),
            _ => "x",
        }
    }

    /// Jetons consécutifs dans la même suite (espace simple, ou élision collée).
    fn adjacent(&self, a: usize, b: usize) -> bool {
        let gap = self.gap(a, b);
        is_space_gap(gap)
            || (gap.is_empty() && self.token(a).is_some_and(|t| t.kind == Kind::Elision))
    }

    /// Jetons consécutifs dans une raison sociale (`&` toléré).
    fn org_adjacent(&self, a: usize, b: usize) -> bool {
        self.adjacent(a, b)
            || self.gap(a, b) == " & "
            || (self.roles.get(a) == Some(&Role::OrgLead) && self.title_gap_ok(a, b))
    }

    /// Séparateur valide après une civilité ou un mot d'amorce : espace, ou point
    /// puis espace (`M. Dupont`, `Ets. Martin`).
    fn title_gap_ok(&self, a: usize, b: usize) -> bool {
        let gap = self.gap(a, b);
        let rest = gap.strip_prefix('.').unwrap_or(gap);
        rest.is_empty() || is_space_gap(rest)
    }

    fn role_of(&self, i: usize) -> Role {
        let Some(t) = self.token(i) else {
            return Role::Normal;
        };
        if t.kind != Kind::Word {
            return Role::Normal;
        }
        let raw = t.text(self.text);
        if LEGAL_FORMS.contains(&raw) && !(self.all_caps && raw == "SA") {
            return Role::Legal;
        }
        let folded = t.folded.as_str();
        let capitalized = t.is_capitalized();
        if (capitalized && TITLES.contains(&folded))
            || (capitalized && !self.all_caps && AMBIGUOUS_TITLES.contains(&folded))
            || (t.case == Case::Initial
                && folded == "m"
                && self.text.get(t.end..).is_some_and(|r| r.starts_with('.')))
        {
            return Role::Title;
        }
        if capitalized && ORG_LEADS.contains(&folded) {
            return Role::OrgLead;
        }
        Role::Normal
    }

    fn is_particle(&self, i: usize) -> bool {
        self.roles.get(i) == Some(&Role::Normal)
            && self
                .token(i)
                .is_some_and(|t| PARTICLES.contains(&t.folded.as_str()))
    }

    /// Mot capitalisé pouvant porter un nom (ni particule, ni mot grammatical).
    fn is_name_word(&self, i: usize) -> bool {
        self.token(i).is_some_and(|t| {
            t.kind == Kind::Word
                && t.is_capitalized()
                && !PARTICLES.contains(&t.folded.as_str())
                && !NON_NAME_WORDS.contains(&t.folded.as_str())
        }) && self.roles.get(i).copied().unwrap_or(Role::Normal) == Role::Normal
    }

    fn is_lower(&self, i: usize) -> bool {
        self.token(i).is_some_and(|t| t.case == Case::Lower)
    }

    // ----- suites de noms ---------------------------------------------------------

    /// Analyse un nom à partir du jeton `j`.
    fn parse_unit(&self, j: usize, mid_run: bool, after_title: bool) -> Option<Unit> {
        let mut k = j;
        loop {
            if self.is_particle(k) {
                if self.is_lower(k) {
                    if k == j && !(mid_run || after_title) {
                        return None;
                    }
                    let folded = self.token(k)?.folded.as_str();
                    if matches!(folded, "le" | "la") {
                        let prev = self.token(k.checked_sub(1)?)?.folded.as_str();
                        if k == j || !matches!(prev, "de" | "du") {
                            return None;
                        }
                    }
                }
                if !self.adjacent(k, k + 1) {
                    return None;
                }
                k += 1;
            } else if self.is_name_word(k) {
                return Some(Unit { first: j, last: k });
            } else {
                return None;
            }
        }
    }

    fn build_runs(&self) -> Vec<Vec<Unit>> {
        let n = self.tokens.len();
        let mut runs = Vec::new();
        let mut i = 0;
        while i < n {
            let after_title =
                i > 0 && self.roles.get(i - 1) == Some(&Role::Title) && self.title_gap_ok(i - 1, i);
            let Some(first) = self.parse_unit(i, false, after_title) else {
                i += 1;
                continue;
            };
            let mut units = vec![first];
            let mut last = first.last;
            while last + 1 < n && self.adjacent(last, last + 1) {
                match self.parse_unit(last + 1, true, false) {
                    Some(u) => {
                        units.push(u);
                        last = u.last;
                    }
                    None => break,
                }
            }
            i = last + 1;
            runs.push(units);
        }
        runs
    }

    fn unit_start(&self, u: Unit) -> usize {
        self.token(u.first).map_or(0, |t| t.start)
    }

    fn unit_end(&self, u: Unit) -> usize {
        self.token(u.last).map_or(0, |t| t.end)
    }

    fn main_token(&self, u: Unit) -> Option<&Token> {
        self.token(u.last)
    }

    fn main_folded(&self, u: Unit) -> &str {
        self.main_token(u).map_or("", |t| t.folded.as_str())
    }

    fn unit_folded(&self, u: Unit) -> String {
        fold(
            self.text
                .get(self.unit_start(u)..self.unit_end(u))
                .unwrap_or(""),
        )
    }

    fn has_particle(u: Unit) -> bool {
        u.first != u.last
    }

    fn is_cap(&self, u: Unit) -> bool {
        self.main_token(u).is_some_and(|t| t.case == Case::Cap)
    }

    fn is_upper(&self, u: Unit) -> bool {
        self.main_token(u).is_some_and(|t| t.case == Case::Upper)
    }

    fn is_common(&self, u: Unit) -> bool {
        common_word_folded(self.main_folded(u))
            || (Self::has_particle(u) && common_word_folded(&self.unit_folded(u)))
    }

    /// Palier de prénom d'une unité sans particule (`Jean`, `Jean-Pierre`).
    fn first_tier(&self, u: Unit) -> Option<Tier> {
        if Self::has_particle(u) {
            return None;
        }
        let m = self.main_folded(u);
        if let Some(t) = first_name_folded(m) {
            return Some(t);
        }
        if m.contains(' ') {
            return m
                .split(' ')
                .map(first_name_folded)
                .try_fold(Tier::VeryCommon, |acc, t| t.map(|t| acc.min(t)));
        }
        None
    }

    /// Palier de nom de famille d'une unité (`Dupont`, `Le Goff`, `Martin-Dupont`).
    fn surname_tier(&self, u: Unit) -> Option<Tier> {
        if let Some(t) = surname_folded(&self.unit_folded(u)) {
            return Some(t);
        }
        let m = self.main_folded(u);
        if Self::has_particle(u)
            && let Some(t) = surname_folded(m)
        {
            return Some(t);
        }
        if m.contains(' ') {
            return m
                .split(' ')
                .map(|p| surname_folded(p).or_else(|| first_name_folded(p)))
                .try_fold(Tier::VeryCommon, |acc, t| t.map(|t| acc.min(t)));
        }
        None
    }

    fn is_sentence_start_at(&self, pos: usize) -> bool {
        is_sentence_start(self.text, pos)
    }

    // ----- émission ---------------------------------------------------------------

    fn overlaps(&self, start: usize, end: usize) -> bool {
        self.claimed.iter().any(|&(s, e)| start < e && end > s)
    }

    /// Réserve l'intervalle puis émet la détection si le type est demandé.
    /// Renvoie faux si l'intervalle est déjà pris.
    fn claim(&mut self, kind: EntityKind, start: usize, end: usize, confidence: f32) -> bool {
        if start >= end || self.overlaps(start, end) {
            return false;
        }
        self.claimed.push((start, end));
        if self.opts.wants(kind) {
            self.out.push(Detection {
                kind,
                start,
                end,
                source: Source::Dictionary,
                confidence,
            });
        }
        true
    }

    // ----- règle 1 : civilité ou titre ---------------------------------------------

    fn title_rule(&mut self, runs: &[Vec<Unit>]) {
        for run in runs {
            let Some(&head) = run.first() else {
                continue;
            };
            let f = head.first;
            let titled =
                f > 0 && self.roles.get(f - 1) == Some(&Role::Title) && self.title_gap_ok(f - 1, f);
            if !titled {
                continue;
            }
            let mut accepted: Vec<Unit> = Vec::new();
            for (k, &u) in run.iter().take(MAX_TITLE_UNITS).enumerate() {
                let ok = match k {
                    0 => {
                        !Self::has_particle(u)
                            || self.surname_tier(u).is_some()
                            || (self.is_particle(u.first) && self.is_lower(u.first))
                    }
                    _ => {
                        // En capitales, un deuxième mot n'est un nom que derrière un prénom.
                        let after_first_name = accepted
                            .first()
                            .is_some_and(|&f| self.first_tier(f).is_some());
                        (!self.all_caps || after_first_name) && self.is_follower(u, k == 1)
                    }
                };
                if !ok {
                    break;
                }
                accepted.push(u);
            }
            let (Some(&first), Some(&last)) = (accepted.first(), accepted.last()) else {
                continue;
            };
            let is_person = accepted.len() >= 2
                && (self.first_tier(first).is_some() || self.first_tier(last).is_some());
            let kind = if is_person {
                EntityKind::Personne
            } else {
                EntityKind::Nom
            };
            self.claim(
                kind,
                self.unit_start(first),
                self.unit_end(last),
                TITLE_NAME_CONFIDENCE,
            );
        }
    }

    // ----- initiales : `J. Dupont`, `J.-P. Martin` ------------------------------------

    fn initial_gap_ok(&self, a: usize, b: usize) -> bool {
        let Some(rest) = self.gap(a, b).strip_prefix('.') else {
            return false;
        };
        rest.is_empty() || rest == "-" || rest == " -" || rest == "- " || is_space_gap(rest)
    }

    /// Indice du premier jeton de la suite d'initiales qui précède le jeton `k`.
    fn initials_before(&self, k: usize) -> Option<usize> {
        let mut first = None;
        let mut next = k;
        while next > 0 {
            let prev = next - 1;
            let t = self.token(prev)?;
            if t.kind != Kind::Word
                || t.case != Case::Initial
                || self.roles.get(prev) != Some(&Role::Normal)
                || !self.text.get(t.end..).is_some_and(|r| r.starts_with('.'))
                || !self.initial_gap_ok(prev, next)
            {
                break;
            }
            first = Some(prev);
            next = prev;
        }
        first
    }

    fn initials_rule(&mut self, runs: &[Vec<Unit>]) {
        for run in runs {
            let Some(&u) = run.first() else {
                continue;
            };
            let Some(s) = self.initials_before(u.first) else {
                continue;
            };
            let titled =
                s > 0 && self.roles.get(s - 1) == Some(&Role::Title) && self.title_gap_ok(s - 1, s);
            let known = !self.all_caps
                && self.surname_tier(u).is_some()
                && (self.is_upper(u) || !self.is_common(u));
            if !(titled || known) {
                continue;
            }
            let confidence = if titled {
                TITLE_NAME_CONFIDENCE
            } else {
                PERSON_CONFIDENCE
            };
            let start = self.token(s).map_or(0, |t| t.start);
            self.claim(EntityKind::Personne, start, self.unit_end(u), confidence);
        }
    }

    // ----- règle 2 : sociétés --------------------------------------------------------

    fn is_connector(&self, i: usize) -> bool {
        self.token(i)
            .is_some_and(|t| (t.case == Case::Lower) && CONNECTORS.contains(&t.folded.as_str()))
    }

    fn is_org_word(&self, i: usize, strict: bool) -> bool {
        let Some(t) = self.token(i) else {
            return false;
        };
        if t.kind != Kind::Word || !t.is_capitalized() {
            return false;
        }
        if self.roles.get(i).copied().unwrap_or(Role::Normal) != Role::Normal {
            return false;
        }
        let f = t.folded.as_str();
        if matches!(f, "les" | "des" | "aux" | "au") {
            return !strict;
        }
        // En amont d'une forme juridique, un mot ordinaire en début de phrase
        // (`Contrat Dupont SARL`) n'appartient pas à la raison sociale.
        !NON_NAME_WORDS.contains(&f)
            && !(strict && common_word_folded(f) && self.is_sentence_start_at(t.start))
    }

    /// Dernier jeton d'une raison sociale qui suit le jeton `i`.
    fn org_forward(&self, i: usize) -> Option<usize> {
        let n = self.tokens.len();
        let mut k = i;
        let mut names = 0;
        let mut last = None;
        while k + 1 < n && names < MAX_ORG_WORDS && self.org_adjacent(k, k + 1) {
            let nxt = k + 1;
            if self.is_org_word(nxt, false) {
                names += 1;
                last = Some(nxt);
            } else if !(self.is_connector(nxt) || self.is_particle(nxt) && self.is_lower(nxt)) {
                break;
            }
            k = nxt;
        }
        last
    }

    /// Premier jeton d'une raison sociale qui précède le jeton `i`.
    fn org_backward(&self, i: usize) -> Option<usize> {
        let mut k = i;
        let mut names = 0;
        let mut first = None;
        while k > 0 && names < MAX_ORG_WORDS && self.org_adjacent(k - 1, k) {
            let prev = k - 1;
            if self.is_org_word(prev, true) {
                names += 1;
                first = Some(prev);
            } else if !(self.is_connector(prev) || self.is_particle(prev) && self.is_lower(prev)) {
                break;
            }
            k = prev;
        }
        first
    }

    fn organization_rules(&mut self) {
        for i in 0..self.tokens.len() {
            let role = self.roles.get(i).copied().unwrap_or(Role::Normal);
            let span = match role {
                Role::Legal => {
                    let start = self.token(i).map(|t| t.start);
                    let end = self.token(i).map(|t| t.end);
                    match (self.org_forward(i), self.org_backward(i)) {
                        (Some(last), _) => start.zip(self.token(last).map(|t| t.end)),
                        (None, Some(first)) => self.token(first).map(|t| t.start).zip(end),
                        (None, None) => None,
                    }
                }
                Role::OrgLead => {
                    let start = self.token(i).map(|t| t.start);
                    self.org_forward(i)
                        .and_then(|last| start.zip(self.token(last).map(|t| t.end)))
                }
                _ => None,
            };
            if let Some((s, e)) = span {
                self.claim(EntityKind::Societe, s, e, ORGANIZATION_CONFIDENCE);
            }
        }
    }

    // ----- règle 3 : prénom + nom -----------------------------------------------------

    /// Vrai si `u` peut suivre un prénom ou une civilité dans un nom complet.
    /// `allow_unknown` : accepte un mot capitalisé absent des dictionnaires.
    fn is_follower(&self, u: Unit, allow_unknown: bool) -> bool {
        if self.surname_tier(u).is_some() || self.first_tier(u).is_some() {
            return true;
        }
        // En capitales, tout est « nom propre » : seuls les noms connus comptent.
        if self.all_caps {
            return false;
        }
        if self.is_common(u) {
            return false;
        }
        if self.is_upper(u) {
            return true;
        }
        allow_unknown && !commune_folded(self.main_folded(u))
    }

    fn person_rules(&mut self, runs: &[Vec<Unit>]) {
        for run in runs {
            let mut i = 0;
            while i < run.len() {
                let span = self
                    .first_then_last(run, i)
                    .or_else(|| self.last_then_first(run, i));
                let bounds = span.and_then(|end| {
                    let (a, b) = (run.get(i)?, run.get(end)?);
                    Some((end, self.unit_start(*a), self.unit_end(*b)))
                });
                if let Some((end, start, stop)) = bounds
                    && self.claim(EntityKind::Personne, start, stop, PERSON_CONFIDENCE)
                {
                    i = end + 1;
                    continue;
                }
                i += 1;
            }
        }
    }

    /// `Prénom Nom…` : renvoie l'indice de la dernière unité du nom complet.
    fn first_then_last(&self, run: &[Unit], i: usize) -> Option<usize> {
        let head = *run.get(i)?;
        if !self.is_cap(head) {
            return None;
        }
        self.first_tier(head)?;
        // Un prénom courant et mot courant (`Rose`, `Pierre`) exige un nom connu.
        let strict = self.is_common(head);
        let mut last = i;
        for k in 1..=MAX_PERSON_FOLLOWERS {
            let Some(&f) = run.get(i + k) else {
                break;
            };
            // `Jean Dupont Marie Durand` : un prénom suivi d'un autre mot ouvre
            // une nouvelle personne.
            if k >= 2
                && self
                    .first_tier(f)
                    .is_some_and(|ft| self.surname_tier(f).is_none_or(|st| ft >= st))
                && run.get(i + k + 1).is_some()
            {
                break;
            }
            if !self.is_follower(f, k == 1 && !strict) {
                break;
            }
            last = i + k;
        }
        (last > i).then_some(last)
    }

    /// `NOM Prénom` : un à trois noms de famille connus puis un prénom connu.
    fn last_then_first(&self, run: &[Unit], i: usize) -> Option<usize> {
        let mut j = i;
        while let Some(&u) = run.get(j) {
            let surname_like = self.surname_tier(u).is_some()
                && (self.is_upper(u) || !self.is_common(u))
                && (self.is_upper(u) || self.first_tier(u).is_none());
            if !surname_like || j - i >= MAX_PERSON_FOLLOWERS {
                break;
            }
            j += 1;
        }
        if j == i {
            return None;
        }
        let &given = run.get(j)?;
        // En début de phrase, un mot ordinaire (`Voir`) peut être un nom connu :
        // on n'accepte alors que le bloc entier (cellule `Durand Marie`).
        let head = *run.get(i)?;
        if !self.is_upper(head) && self.is_sentence_start_at(self.unit_start(head)) {
            let whole = self
                .text
                .get(self.unit_end(given)..)
                .is_some_and(|r| r.trim().is_empty())
                && self
                    .text
                    .get(..self.unit_start(head))
                    .is_some_and(|r| r.trim().is_empty());
            if !whole {
                return None;
            }
        }
        (self.is_cap(given) && self.first_tier(given).is_some()).then_some(j)
    }

    // ----- règle 4 : communes -----------------------------------------------------------

    fn commune_start(&self, i: usize) -> bool {
        self.token(i).is_some_and(|t| match t.kind {
            Kind::Word => t.is_capitalized(),
            Kind::Elision => t.case == Case::Initial,
        })
    }

    fn commune_inner(&self, i: usize) -> bool {
        self.token(i).is_some_and(|t| match t.kind {
            Kind::Word => t.is_capitalized() || self.is_connector(i),
            Kind::Elision => true,
        })
    }

    fn commune_end(&self, i: usize) -> bool {
        self.token(i)
            .is_some_and(|t| t.kind == Kind::Word && t.is_capitalized())
    }

    /// Vrai si un nom de commune d'un seul mot peut être retenu : ni début de
    /// phrase, et, s'il est aussi un mot courant ou un prénom ou nom très courant,
    /// seulement après `à`, `au`, `en`… ou un code postal.
    fn single_commune_ok(&self, i: usize) -> bool {
        let Some(t) = self.token(i) else {
            return false;
        };
        if self.is_sentence_start_at(t.start) {
            return false;
        }
        let m = t.folded.as_str();
        let ambiguous =
            common_word_folded(m) || first_name_folded(m).is_some_and(|tier| tier >= Tier::Common);
        !ambiguous || self.locative_context(t.start)
    }

    fn locative_context(&self, start: usize) -> bool {
        let before = self
            .text
            .get(..start)
            .unwrap_or("")
            .trim_end_matches([' ', '\u{a0}', '\u{202f}']);
        let digits = before
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .count();
        if digits == POSTAL_CODE_DIGITS {
            return true;
        }
        let word: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphabetic())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        LOCATIVE_WORDS.contains(&word.as_str())
    }

    fn commune_rule(&mut self) {
        if !self.opts.wants(EntityKind::Lieu) {
            return;
        }
        let n = self.tokens.len();
        let mut i = 0;
        while i < n {
            if !self.commune_start(i) {
                i += 1;
                continue;
            }
            let mut ends = vec![i];
            let mut k = i;
            while k + 1 < n && k + 1 - i < MAX_COMMUNE_TOKENS && self.adjacent(k, k + 1) {
                if !self.commune_inner(k + 1) {
                    break;
                }
                k += 1;
                if self.commune_end(k) {
                    ends.push(k);
                }
            }
            let begin = self.token(i).map_or(0, |t| t.start);
            let mut matched = None;
            for &e in ends.iter().rev() {
                let end = self.token(e).map_or(0, |t| t.end);
                let key = fold(self.text.get(begin..end).unwrap_or(""));
                if commune_folded(&key) && (e != i || self.single_commune_ok(i)) {
                    matched = Some((e, end));
                    break;
                }
            }
            match matched {
                Some((e, end)) => {
                    self.claim(EntityKind::Lieu, begin, end, COMMUNE_CONFIDENCE);
                    i = e + 1;
                }
                None => i += 1,
            }
        }
    }

    // ----- règle 5 : nom ou prénom isolé --------------------------------------------------

    fn solo_rule(&mut self, runs: &[Vec<Unit>]) {
        for &u in runs.iter().flatten() {
            let (start, end) = (self.unit_start(u), self.unit_end(u));
            if self.overlaps(start, end) {
                continue;
            }
            if let Some(kind) = self.solo_kind(u) {
                self.claim(kind, start, end, SOLO_NAME_CONFIDENCE);
            }
        }
    }

    /// Vrai si le nom est une étiquette ou un début d'élément : suivi de `:`, ou
    /// précédé d'une puce, d'un titre Markdown, d'une cellule de tableau, de `:`, `;`.
    fn is_label_position(&self, u: Unit) -> bool {
        let after = self.text.get(self.unit_end(u)..).unwrap_or("");
        if after
            .trim_start_matches([' ', '\u{a0}', '\u{202f}'])
            .starts_with([':', '\u{ff1a}'])
        {
            return true;
        }
        let before = self.text.get(..self.unit_start(u)).unwrap_or("");
        let mut prev = before
            .chars()
            .rev()
            .skip_while(|c| c.is_whitespace() || matches!(c, '«' | '“' | '"' | '(' | '[' | '*'));
        match prev.next() {
            Some(c) if LABEL_MARKERS.contains(&c) => true,
            Some(')') => prev.next().is_some_and(|d| d.is_ascii_digit()),
            _ => false,
        }
    }

    fn solo_kind(&self, u: Unit) -> Option<EntityKind> {
        if self.is_label_position(u) {
            return None;
        }
        let surname = self.surname_tier(u).filter(|t| *t >= MIN_SOLO_TIER);
        let upper_ok = self.is_upper(u)
            && surname.is_some()
            && self.main_token(u).is_some_and(|t| {
                t.folded.chars().filter(|c| c.is_alphabetic()).count() >= MIN_UPPER_SOLO_LETTERS
            });
        if !(self.is_cap(u) || upper_ok) {
            return None;
        }
        if self.is_sentence_start_at(self.unit_start(u)) || self.is_common(u) {
            return None;
        }
        let given = self.first_tier(u).filter(|t| *t >= MIN_SOLO_TIER);
        let kind = match (surname, given) {
            (Some(s), Some(f)) if s >= f => EntityKind::Nom,
            (Some(_), Some(_)) | (None, Some(_)) => EntityKind::Prenom,
            (Some(_), None) => EntityKind::Nom,
            (None, None) => return None,
        };
        // Un nom de lieu connu n'est retenu que s'il est très courant comme nom.
        if commune_folded(self.main_folded(u)) {
            let strong = kind != EntityKind::Nom && given == Some(Tier::VeryCommon);
            if !strong {
                return None;
            }
        }
        Some(kind)
    }
}

#[cfg(test)]
#[path = "detector_tests.rs"]
mod tests;
