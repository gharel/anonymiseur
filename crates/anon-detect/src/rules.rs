//! Détecteur à règles : expressions régulières et contrôles de clé.
//!
//! Un reconnaisseur par type, chacun dans sa fonction. Les offsets sont en
//! octets, sur des frontières de caractères. Une règle dont la clé est validée
//! vaut `1.0` ; sans clé, on utilise une constante nommée plus basse.
//!
//! La crate `regex` n'a pas d'assertions avant/arrière : les contrôles de
//! bordure (chiffre ou lettre accolés) sont faits à la main après chaque
//! candidat, avec reprise de la recherche juste après le début d'un candidat
//! refusé.

use std::sync::LazyLock;

use anon_core::{DetectOptions, Detection, Detector, EntityKind, Source, TextBlock};
use regex::Regex;

use crate::checksums::{
    codice_fiscale_valid, dni_es_valid, iban_length, iban_valid, luhn, nie_es_valid, nir_valid,
    niss_be_valid, siren_valid, siret_valid,
};

// ---------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------

/// Confiance d'une règle dont la clé est validée.
const CONF_KEY: f32 = 1.0;
const CONF_EMAIL: f32 = 0.95;
const CONF_URL: f32 = 0.95;
const CONF_IP: f32 = 0.9;
const CONF_TEL: f32 = 0.9;
/// Numéro local à 6 chiffres : n'existe qu'avec un mot de contexte.
const CONF_TEL_LOCAL: f32 = 0.7;
const CONF_RIDET: f32 = 0.9;
/// Identifiant dont seul le contexte (et non une clé) justifie la détection.
const CONF_ID_CONTEXT: f32 = 0.8;
/// Identifiant à clé alphabétique non contrôlable (TVA), SIREN valide.
const CONF_ID_PARTIAL: f32 = 0.9;
const CONF_PLAQUE: f32 = 0.9;
const CONF_ADRESSE: f32 = 0.85;
const CONF_DATE_NAISS: f32 = 0.9;

/// Fenêtre (en caractères) où chercher un mot de contexte avant un numéro local.
const WINDOW_TEL: usize = 25;
const WINDOW_SIRET: usize = 25;
const WINDOW_SIREN: usize = 40;
const WINDOW_ID: usize = 30;
/// Fenêtre (même ligne) des mots de contexte version / réseau.
const WINDOW_LINE: usize = 40;
const WINDOW_MONEY: usize = 25;
/// Chiffres d'un numéro international (E.164).
const INTL_MIN_DIGITS: usize = 8;
const INTL_MAX_DIGITS: usize = 15;
/// Longueur d'un numéro de carte bancaire.
const CB_MIN_DIGITS: usize = 13;
const CB_MAX_DIGITS: usize = 19;
/// Mots maximum d'un nom de voie.
const MAX_STREET_WORDS: usize = 8;
/// Caractères de séparation tolérés entre une voie et son code postal.
const MAX_POSTAL_GAP: usize = 4;

// ---------------------------------------------------------------------------
// Expressions régulières (compilées une seule fois)
// ---------------------------------------------------------------------------

type Re = LazyLock<Option<Regex>>;

/// Remplace les jetons `<S>` (espaces, y compris insécables), `<T>` (séparateurs
/// de téléphone), `<C>` (séparateurs de carte) et `<N>` (caractères d'un numéro
/// international).
fn pat(s: &str) -> String {
    s.replace("<S>", r"[ \x{a0}\x{202f}]")
        .replace("<T>", r"[ .\-\x{a0}\x{202f}]")
        .replace("<C>", r"[ \-\x{a0}\x{202f}]")
        .replace("<N>", r"[\d ().\-\x{a0}\x{202f}]")
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: Re = LazyLock::new(|| Regex::new(&pat($pat)).ok());
    };
}

re!(
    EMAIL,
    r"[\p{L}\p{N}._%+\-]+@[\p{L}\p{N}\-]+(?:\.[\p{L}\p{N}\-]+)*\.\p{L}{2,}"
);
re!(URL, r#"(?i)(?:https?://|www\.)[^\s<>"«»\x{a0}\x{202f}]+"#);
re!(IPV4, r"\b\d{1,3}(?:\.\d{1,3}){3}\b");
re!(IPV6, r"[0-9A-Fa-f:]{3,}");
re!(
    TEL_FR_INTL,
    r"(?:\+|\b00)33<T>?(?:\(0\)|0)?<T>?[1-9](?:<T>?\d{2}){4}"
);
re!(TEL_NC, r"(?:\+|\b00)687<T>?\d{2}<T>?\d{2}<T>?\d{2}");
re!(TEL_FR_NAT, r"\b0[1-9](?:<T>?\d{2}){4}");
re!(TEL_INTL, r"\+<T>?\(?[1-9]<N>{6,27}");
re!(TEL_LOCAL, r"\d{2}(?:<T>?\d{2}){2}");
re!(
    TEL_CTX,
    r"(?i)\b(?:t[ée]l[ée]phone|t[ée]l|mobile|mob|portable|fax|gsm|standard|appeler|joindre)\b"
);
re!(IBAN_START, r"\b[A-Z]{2}\d{2}");
re!(CB, r"\d(?:<C>?\d){12,18}");
re!(
    NIR,
    r"[1-478]<S>?\d{2}<S>?(?:0[1-9]|1[0-2]|[2-9]\d)<S>?(?:2[AaBb]|\d{2})<S>?\d{3}<S>?\d{3}<S>?\d{2}"
);
re!(SIRET, r"\d{3}<S>?\d{3}<S>?\d{3}<S>?\d{5}");
re!(SIREN, r"\d{3}<S>?\d{3}<S>?\d{3}");
re!(SIRET_CTX, r"(?i)\bsiret\b");
re!(SIREN_CTX, r"(?i)\bsiren\b|\brcs\b|\bimmatricul");
re!(
    RIDET,
    r"(?i)\bridet\b[^\d]{0,15}(?P<num>\d<S>?\d{3}<S>?\d{3}(?:\.\d{3})?)"
);
re!(TVA_FR, r"\bFR<S>?[0-9A-Z]{2}<S>?\d{3}<S>?\d{3}<S>?\d{3}");
re!(DNI, r"\b\d{8}-?[A-Za-z]\b");
re!(NIE, r"\b[XYZxyz]-?\d{7}-?[A-Za-z]\b");
re!(
    CODICE,
    r"\b(?i:[A-Z]{6}\d{2}[A-EHLMPR-T]\d{2}[A-Z]\d{3}[A-Z])\b"
);
re!(NISS, r"\b\d{2}[. ]?\d{2}[. ]?\d{2}[- ]?\d{3}[. ]?\d{2}\b");
re!(
    NISS_CTX,
    r"(?i)\b(?:niss|rrn|registre national|num[ée]ro national)\b"
);
re!(
    NINO,
    r"\b[A-CEGHJ-PR-TW-Z][A-CEGHJ-NPR-TW-Z]<S>?\d{2}<S>?\d{2}<S>?\d{2}<S>?[A-D]\b"
);
re!(
    NINO_CTX,
    r"(?i)\b(?:nino|national[ \x{a0}]+insurance|ni[ \x{a0}]+(?:number|no\b|num))"
);
re!(SSN, r"\b\d{3}-\d{2}-\d{4}\b");
re!(SSN_CTX, r"(?i)\b(?:ssn|social[ \x{a0}]+security)\b");
re!(PLAQUE, r"\b[A-HJ-NP-TV-Z]{2}-\d{3}-[A-HJ-NP-TV-Z]{2}\b");
re!(PLAQUE_NC, r"\b(?:\d{3,6}|\d{1,3}<S>\d{3})<S>NC\b");
re!(
    MONEY_CTX,
    r"(?i)(?:\b(?:montant|prix|total|somme|tarif|co[uû]t|payer|pay[ée]s?|xpf|cfp|francs?)\b|[€$])"
);
re!(
    VERSION_CTX,
    r"(?i)(?:\bversi[oó]n\b|\bv\b|\brelease\b|\bbuild\b|\br[ée]vision\b|\brev\b|§)"
);
re!(
    NET_CTX,
    r"(?i)\b(?:ip|serveur|server|h[ôo]te|host|passerelle|gateway|dns|ping)\b"
);
re!(
    ADDR_ES,
    r"(?i)\b(?:calle|avenida|avda|plaza|paseo|carrer)(?:\.|\b)"
);
re!(
    ADDR_IT,
    r"\b(?:Via|Viale|Piazza|Corso|VIA|VIALE|PIAZZA|CORSO)\b"
);
re!(
    ADDR_DE,
    r"\b\p{Lu}[\p{L}\-]*(?i:stra(?:ß|ss)e|str\.)<S>?\d{1,4}[a-z]?\b"
);
re!(
    ADDR_EN,
    r"\b\d{1,4}[A-Za-z]?<S>(?:\p{Lu}[\p{L}'\-]*<S>){1,4}(?:Street|Road|Avenue|Lane|Drive|Way|Square|(?:St|Rd|Ave|Ln|Dr|Sq)\b\.?)"
);
re!(
    ADDR_EN_TAIL,
    r"^(?:,<S>*\p{Lu}[\p{L}'\-]{2,}(?:<S>\p{Lu}[\p{L}'\-]{2,}){0,2}(?:,?<S>*(?:[A-Z]{1,2}\d[A-Z\d]?<S>?\d[A-Z]{2}|[A-Z]{2}<S>\d{5}(?:-\d{4})?))?|,?<S>*(?:[A-Z]{1,2}\d[A-Z\d]?<S>?\d[A-Z]{2}|[A-Z]{2}<S>\d{5}(?:-\d{4})?))"
);
re!(
    ADDR_START,
    r"(?i)\b\d{1,4}(?:<S>?(?:bis|ter|quater)\b)?(?:-\d{1,4})?,?<S>+(?:avenue|boulevard|impasse|all[ée]e|place|route|quai|cours|lotissement|r[ée]sidence|square|passage|promenade|voie|sentier|esplanade|chauss[ée]e|faubourg|hameau|cit[ée]|villa|rond-point|venelle|ruelle|parvis|traverse|rue|av|bd|bld|chemin|rte|fbg)(?:\.|\b)"
);
re!(
    BOITE_POSTALE,
    r"(?i)(?:\bB\.<S>?P\.?|\bBP\b|\bbo[iî]te<S>+postale)<S>*:?<S>*\d{1,6}\b"
);
re!(
    DATE_NAISS,
    r"(?i)(?:\bn[ée]e?(?:\(e\))?<S>+le|\bdate<S>+de<S>+naissance|\bborn<S>+on|\bdob\b)[ \x{a0}\x{202f}:]*(?P<d>(?:(?:19|20)\d{2}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])|(?:0?[1-9]|[12]\d|3[01])[/.\-](?:0?[1-9]|1[0-2])[/.\-](?:19|20)\d{2}|(?:0?[1-9]|[12]\d|3[01])(?:er)?<S>+(?:janvier|f[ée]vrier|mars|avril|mai|juin|juillet|ao[uû]t|septembre|octobre|novembre|d[ée]cembre|january|february|march|april|may|june|july|august|september|october|november|december)<S>+(?:19|20)\d{2}))"
);

// ---------------------------------------------------------------------------
// Outils
// ---------------------------------------------------------------------------

type Out = Vec<Detection>;

/// Glu entre chiffres : un chiffre collé via ces signes fait partie d'un autre nombre.
const GLUE: &[char] = &['.', '-', '/'];

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}')
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn char_before(text: &str, i: usize) -> Option<char> {
    text.get(..i)?.chars().next_back()
}

fn char_after(text: &str, i: usize) -> Option<char> {
    text.get(i..)?.chars().next()
}

fn char_len_at(text: &str, i: usize) -> usize {
    char_after(text, i).map_or(1, char::len_utf8)
}

/// Ajoute une détection si les offsets sont valides.
fn push(out: &mut Out, text: &str, kind: EntityKind, start: usize, end: usize, confidence: f32) {
    if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
        out.push(Detection {
            kind,
            start,
            end,
            source: Source::Rule,
            confidence,
        });
    }
}

/// Bordure gauche : pas de lettre/chiffre collé, ni de chiffre relié par un signe de `glue`.
fn left_ok(text: &str, start: usize, glue: &[char]) -> bool {
    match char_before(text, start) {
        None => true,
        Some(c) if is_word(c) => false,
        Some(c) if glue.contains(&c) => !char_before(text, start.saturating_sub(c.len_utf8()))
            .is_some_and(|p| p.is_ascii_digit()),
        Some(_) => true,
    }
}

/// Bordure droite, symétrique de [`left_ok`].
fn right_ok(text: &str, end: usize, glue: &[char]) -> bool {
    match char_after(text, end) {
        None => true,
        Some(c) if is_word(c) => false,
        Some(c) if glue.contains(&c) => {
            !char_after(text, end + c.len_utf8()).is_some_and(|n| n.is_ascii_digit())
        }
        Some(_) => true,
    }
}

fn edges_ok(text: &str, start: usize, end: usize, glue: &[char]) -> bool {
    left_ok(text, start, glue) && right_ok(text, end, glue)
}

/// Bordure droite stricte : refuse aussi un chiffre séparé par une espace.
fn strict_right_ok(text: &str, end: usize) -> bool {
    if !right_ok(text, end, GLUE) {
        return false;
    }
    match char_after(text, end) {
        Some(c) if is_space(c) => {
            !char_after(text, end + c.len_utf8()).is_some_and(|n| n.is_ascii_digit())
        }
        _ => true,
    }
}

/// Bordure gauche stricte : refuse aussi un chiffre séparé par une espace.
fn strict_left_ok(text: &str, start: usize) -> bool {
    if !left_ok(text, start, GLUE) {
        return false;
    }
    match char_before(text, start) {
        Some(c) if is_space(c) => !char_before(text, start.saturating_sub(c.len_utf8()))
            .is_some_and(|p| p.is_ascii_digit()),
        _ => true,
    }
}

/// Parcourt les candidats de `re` ; `accept(start, end)` renvoie vrai si le
/// candidat est retenu (la recherche reprend après lui), sinon la recherche
/// reprend un caractère après son début.
fn scan(re: &Re, text: &str, mut accept: impl FnMut(usize, usize) -> bool) {
    let Some(re) = LazyLock::force(re).as_ref() else {
        return;
    };
    let mut pos = 0;
    while pos <= text.len() {
        let Some(m) = re.find_at(text, pos) else {
            break;
        };
        let next = m.start() + char_len_at(text, m.start());
        pos = if accept(m.start(), m.end()) {
            m.end().max(next)
        } else {
            next
        };
    }
}

/// Vrai si un mot de contexte de `re` se termine dans les `max_chars`
/// caractères qui précèdent `start`.
fn context_before(text: &str, start: usize, max_chars: usize, re: &Re) -> bool {
    let Some(re) = LazyLock::force(re).as_ref() else {
        return false;
    };
    let Some(head) = text.get(..start) else {
        return false;
    };
    let total = head.chars().count();
    let window_start = head
        .char_indices()
        .rev()
        .nth(max_chars)
        .map_or(0, |(i, c)| i + c.len_utf8());
    let truncated = total > max_chars;
    let window = head.get(window_start..).unwrap_or("");
    // Une fenêtre tronquée peut couper un mot : on ignore un mot collé à son début.
    let window_chars = window.chars().count();
    let skip_first = truncated && window_chars >= max_chars;
    re.find_iter(window).any(|m| {
        if skip_first && m.start() == 0 {
            return false;
        }
        text.get(window_start + m.end()..start)
            .is_some_and(|between| between.chars().count() <= max_chars)
    })
}

fn overlaps(spans: &[(usize, usize)], s: usize, e: usize) -> bool {
    spans.iter().any(|&(a, b)| s < b && a < e)
}

// ---------------------------------------------------------------------------
// Détecteur
// ---------------------------------------------------------------------------

/// Détecteur à règles (`name() == "rules"`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RuleDetector;

impl RuleDetector {
    pub fn new() -> Self {
        Self
    }
}

type Recognizer = fn(&str, &mut Out);

impl Detector for RuleDetector {
    fn name(&self) -> &'static str {
        "rules"
    }

    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        let text = block.text.as_str();
        let recognizers: [(EntityKind, Recognizer); 14] = [
            (EntityKind::Email, email),
            (EntityKind::Url, url),
            (EntityKind::Ip, ip),
            (EntityKind::Tel, tel),
            (EntityKind::Iban, iban),
            (EntityKind::Cb, cb),
            (EntityKind::Nir, nir),
            (EntityKind::Siret, siret),
            (EntityKind::Ridet, ridet),
            (EntityKind::Identifiant, identifiant),
            (EntityKind::Plaque, plaque),
            (EntityKind::Adresse, adresse),
            (EntityKind::DateNaiss, date_naiss),
            // Réservé : un type voulu sans reconnaisseur propre ne produit rien.
            (EntityKind::Autre, |_, _| {}),
        ];
        let mut out = Vec::new();
        for (kind, recognize) in recognizers {
            if opts.wants(kind) {
                recognize(text, &mut out);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Email, URL, IP
// ---------------------------------------------------------------------------

fn email(text: &str, out: &mut Out) {
    scan(&EMAIL, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        // Ponctuation de tête que la classe du nom local a avalée.
        let lead: usize = m
            .chars()
            .take_while(|c| matches!(c, '.' | '-' | '%' | '+'))
            .map(char::len_utf8)
            .sum();
        let s = s + lead;
        if s >= e || !left_ok(text, s, &[]) {
            return false;
        }
        push(out, text, EntityKind::Email, s, e, CONF_EMAIL);
        true
    });
}

fn url(text: &str, out: &mut Out) {
    scan(&URL, text, |s, e| {
        if !left_ok(text, s, &[]) {
            return false;
        }
        let Some(m) = text.get(s..e) else {
            return false;
        };
        let lower = m.to_lowercase();
        let prefix = if lower.starts_with("https://") {
            8
        } else if lower.starts_with("http://") {
            7
        } else {
            4
        };
        let mut end = e;
        while let Some(last) = text.get(s..end).and_then(|t| t.chars().next_back()) {
            let trim = match last {
                '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '…' => true,
                ')' | ']' | '}' => {
                    let open = match last {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    let slice = text.get(s..end).unwrap_or("");
                    slice.matches(last).count() > slice.matches(open).count()
                }
                _ => false,
            };
            if !trim {
                break;
            }
            end -= last.len_utf8();
        }
        if end - s <= prefix {
            return false;
        }
        push(out, text, EntityKind::Url, s, end, CONF_URL);
        true
    });
}

/// Vrai si « version », « v » ou « § » précède le numéro : ce n'est pas une IP.
fn version_cue(text: &str, start: usize) -> bool {
    let head = text.get(..start).unwrap_or("");
    let head = head.trim_end_matches(is_space).to_lowercase();
    let not_letter_before = |rest: &str| !rest.chars().next_back().is_some_and(char::is_alphabetic);
    if let Some(rest) = head.strip_suffix("version") {
        return not_letter_before(rest);
    }
    if let Some(rest) = head.strip_suffix('v') {
        return not_letter_before(rest);
    }
    head.ends_with('§') || line_window_matches(text, start, WINDOW_LINE, &VERSION_CTX)
}

/// Vrai si `re` trouve un mot dans les `max_chars` caractères qui précèdent
/// `start`, sur la même ligne.
fn line_window_matches(text: &str, start: usize, max_chars: usize, re: &Re) -> bool {
    let Some(re) = LazyLock::force(re).as_ref() else {
        return false;
    };
    let head = text.get(..start).unwrap_or("");
    let line = head.rsplit(['\n', '\r']).next().unwrap_or("");
    let skip = line.chars().count().saturating_sub(max_chars);
    let from = line.char_indices().nth(skip).map_or(0, |(i, _)| i);
    re.is_match(line.get(from..).unwrap_or(""))
}

fn ip(text: &str, out: &mut Out) {
    scan(&IPV4, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        let octets_ok = m.split('.').all(|o| {
            (o.len() == 1 || !o.starts_with('0')) && o.parse::<u16>().is_ok_and(|v| v <= 255)
        });
        if !octets_ok || !edges_ok(text, s, e, &['.']) || version_cue(text, s) {
            return false;
        }
        // a.b.c.d avec des octets tous < 10 : plutôt une version qu'une IP.
        let all_small = m.split('.').all(|o| o.len() == 1);
        if all_small && !line_window_matches(text, s, WINDOW_LINE, &NET_CTX) {
            return false;
        }
        push(out, text, EntityKind::Ip, s, e, CONF_IP);
        true
    });
    scan(&IPV6, text, |s, mut e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        // Un « : » final isolé est de la ponctuation (« adresse : fe80::1: »).
        if m.ends_with(':') && !m.ends_with("::") {
            e -= 1;
        }
        let Some(m) = text.get(s..e) else {
            return false;
        };
        if m.matches(':').count() < 2 || m.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        if !left_ok(text, s, &[]) || !right_ok(text, e, &['.']) {
            return false;
        }
        push(out, text, EntityKind::Ip, s, e, CONF_IP);
        true
    });
}

// ---------------------------------------------------------------------------
// Téléphone
// ---------------------------------------------------------------------------

/// Fin d'un numéro international après coupe à 15 chiffres, `None` s'il est trop court.
fn intl_end(text: &str, s: usize, e: usize) -> Option<usize> {
    let m = text.get(s..e)?;
    let mut total = 0;
    let mut last_end = None;
    let mut run: Option<(usize, usize)> = None; // (début, nombre de chiffres)
    let mut accept_run = |run: (usize, usize), total: &mut usize| -> bool {
        if *total + run.1 > INTL_MAX_DIGITS {
            return false;
        }
        *total += run.1;
        last_end = Some(s + run.0 + run.1);
        true
    };
    for (i, c) in m.char_indices() {
        if c.is_ascii_digit() {
            run = Some(match run {
                Some((st, n)) => (st, n + 1),
                None => (i, 1),
            });
        } else if let Some(r) = run.take()
            && !accept_run(r, &mut total)
        {
            return finish_intl(total, last_end);
        }
    }
    if let Some(r) = run {
        accept_run(r, &mut total);
    }
    finish_intl(total, last_end)
}

fn finish_intl(total: usize, last_end: Option<usize>) -> Option<usize> {
    if total >= INTL_MIN_DIGITS {
        last_end
    } else {
        None
    }
}

fn tel(text: &str, out: &mut Out) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for re in [&TEL_FR_INTL, &TEL_NC, &TEL_FR_NAT] {
        scan(re, text, |s, e| {
            if !edges_ok(text, s, e, GLUE) || overlaps(&taken, s, e) {
                return false;
            }
            push(out, text, EntityKind::Tel, s, e, CONF_TEL);
            taken.push((s, e));
            true
        });
    }
    scan(&TEL_INTL, text, |s, e| {
        let Some(end) = intl_end(text, s, e) else {
            return false;
        };
        if overlaps(&taken, s, end) || !edges_ok(text, s, end, GLUE) {
            return false;
        }
        push(out, text, EntityKind::Tel, s, end, CONF_TEL);
        taken.push((s, end));
        true
    });
    // Numéro local à 6 chiffres : seulement avec un mot de contexte.
    scan(&TEL_LOCAL, text, |s, e| {
        if overlaps(&taken, s, e)
            || !strict_left_ok(text, s)
            || !strict_right_ok(text, e)
            || !context_before(text, s, WINDOW_TEL, &TEL_CTX)
        {
            return false;
        }
        push(out, text, EntityKind::Tel, s, e, CONF_TEL_LOCAL);
        true
    });
}

// ---------------------------------------------------------------------------
// IBAN, carte bancaire, NIR
// ---------------------------------------------------------------------------

/// Lit un IBAN de `len` caractères utiles à partir de `s` (groupes de 4 admis).
fn read_iban(text: &str, s: usize, len: usize) -> Option<usize> {
    let rest = text.get(s..)?;
    let mut count = 0;
    let mut group = 0;
    let mut spaced = false;
    let mut end = s;
    let mut chars = rest.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_ascii_alphanumeric() {
            count += 1;
            group += 1;
            end = s + i + 1;
            if count == len {
                break;
            }
            if spaced && group > 4 {
                return None;
            }
        } else if is_space(c) {
            let next_alnum = chars.peek().is_some_and(|(_, n)| n.is_ascii_alphanumeric());
            if group != 4 || !next_alnum {
                return None;
            }
            spaced = true;
            group = 0;
        } else {
            return None;
        }
    }
    (count == len).then_some(end)
}

fn iban(text: &str, out: &mut Out) {
    scan(&IBAN_START, text, |s, _| {
        let Some(len) = text.get(s..s + 2).and_then(iban_length) else {
            return false;
        };
        let Some(end) = read_iban(text, s, len) else {
            return false;
        };
        let valid = text.get(s..end).is_some_and(iban_valid);
        if !valid || !left_ok(text, s, &[]) || !right_ok(text, end, &[]) {
            return false;
        }
        push(out, text, EntityKind::Iban, s, end, CONF_KEY);
        true
    });
}

fn cb_prefix_ok(digits: &str) -> bool {
    let p2: u32 = digits.get(..2).and_then(|p| p.parse().ok()).unwrap_or(0);
    let p4: u32 = digits.get(..4).and_then(|p| p.parse().ok()).unwrap_or(0);
    digits.starts_with('4')
        || digits.starts_with('6')
        || (51..=55).contains(&p2)
        || (2221..=2720).contains(&p4)
        || p2 == 34
        || p2 == 37
}

fn cb(text: &str, out: &mut Out) {
    scan(&CB, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        if !left_ok(text, s, GLUE) {
            return false;
        }
        // Positions (octets) des chiffres du candidat.
        let positions: Vec<usize> = m
            .char_indices()
            .filter(|(_, c)| c.is_ascii_digit())
            .map(|(i, _)| s + i)
            .collect();
        for n in (CB_MIN_DIGITS..=positions.len().min(CB_MAX_DIGITS)).rev() {
            let Some(&last) = positions.get(n - 1) else {
                continue;
            };
            let end = last + 1;
            if !right_ok(text, end, GLUE) {
                continue;
            }
            let digits: String = text
                .get(s..end)
                .unwrap_or("")
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            if cb_prefix_ok(&digits) && luhn(&digits) {
                push(out, text, EntityKind::Cb, s, end, CONF_KEY);
                return true;
            }
        }
        false
    });
}

fn nir(text: &str, out: &mut Out) {
    scan(&NIR, text, |s, e| {
        let valid = text.get(s..e).is_some_and(nir_valid);
        if !valid || !edges_ok(text, s, e, GLUE) {
            return false;
        }
        push(out, text, EntityKind::Nir, s, e, CONF_KEY);
        true
    });
}

// ---------------------------------------------------------------------------
// SIRET / SIREN, RIDET
// ---------------------------------------------------------------------------

/// Reconnaît SIRET (14 chiffres) et SIREN (9 chiffres) ; les deux donnent `Siret`.
fn siret(text: &str, out: &mut Out) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    scan(&SIRET, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        let grouped = m.chars().any(is_space);
        if !edges_ok(text, s, e, GLUE)
            || !siret_valid(m)
            || !(grouped || context_before(text, s, WINDOW_SIRET, &SIRET_CTX))
        {
            return false;
        }
        push(out, text, EntityKind::Siret, s, e, CONF_KEY);
        taken.push((s, e));
        true
    });
    scan(&SIREN, text, |s, e| {
        let valid = text.get(s..e).is_some_and(siren_valid);
        if overlaps(&taken, s, e)
            || !valid
            || !left_ok(text, s, GLUE)
            || !strict_right_ok(text, e)
            || !context_before(text, s, WINDOW_SIREN, &SIREN_CTX)
        {
            return false;
        }
        push(out, text, EntityKind::Siret, s, e, CONF_KEY);
        true
    });
}

fn ridet(text: &str, out: &mut Out) {
    let Some(re) = LazyLock::force(&RIDET).as_ref() else {
        return;
    };
    for caps in re.captures_iter(text) {
        let Some(num) = caps.name("num") else {
            continue;
        };
        if right_ok(text, num.end(), GLUE) {
            push(
                out,
                text,
                EntityKind::Ridet,
                num.start(),
                num.end(),
                CONF_RIDET,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Identifiants (TVA, DNI/NIE, codice fiscale, NISS, NINO, SSN)
// ---------------------------------------------------------------------------

fn identifiant(text: &str, out: &mut Out) {
    tva_fr(text, out);
    dni_nie(text, out);
    codice_fiscale(text, out);
    niss(text, out);
    nino(text, out);
    ssn(text, out);
}

fn tva_fr(text: &str, out: &mut Out) {
    scan(&TVA_FR, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        let compact: String = m.chars().filter(|c| !is_space(*c)).collect();
        let (Some(key), Some(siren)) = (compact.get(2..4), compact.get(4..)) else {
            return false;
        };
        if !edges_ok(text, s, e, GLUE) || !siren_valid(siren) {
            return false;
        }
        let confidence = match (key.parse::<u64>(), siren.parse::<u64>()) {
            (Ok(k), Ok(n)) if (12 + 3 * (n % 97)) % 97 == k => CONF_KEY,
            (Ok(_), _) => return false,
            _ => CONF_ID_PARTIAL,
        };
        push(out, text, EntityKind::Identifiant, s, e, confidence);
        true
    });
}

fn dni_nie(text: &str, out: &mut Out) {
    scan(&DNI, text, |s, e| {
        let valid = text.get(s..e).is_some_and(dni_es_valid);
        if !valid || !edges_ok(text, s, e, &[]) {
            return false;
        }
        push(out, text, EntityKind::Identifiant, s, e, CONF_KEY);
        true
    });
    scan(&NIE, text, |s, e| {
        let valid = text.get(s..e).is_some_and(nie_es_valid);
        if !valid || !edges_ok(text, s, e, &[]) {
            return false;
        }
        push(out, text, EntityKind::Identifiant, s, e, CONF_KEY);
        true
    });
}

fn codice_fiscale(text: &str, out: &mut Out) {
    scan(&CODICE, text, |s, e| {
        let valid = text.get(s..e).is_some_and(codice_fiscale_valid);
        if !valid || !edges_ok(text, s, e, &[]) {
            return false;
        }
        push(out, text, EntityKind::Identifiant, s, e, CONF_KEY);
        true
    });
}

fn niss(text: &str, out: &mut Out) {
    scan(&NISS, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        let formatted = m.chars().any(|c| !c.is_ascii_digit());
        if !niss_be_valid(m)
            || !edges_ok(text, s, e, GLUE)
            || !(formatted || context_before(text, s, WINDOW_ID, &NISS_CTX))
        {
            return false;
        }
        push(out, text, EntityKind::Identifiant, s, e, CONF_KEY);
        true
    });
}

fn nino(text: &str, out: &mut Out) {
    const FORBIDDEN: [&str; 6] = ["BG", "GB", "NK", "KN", "TN", "NT"];
    scan(&NINO, text, |s, e| {
        let Some(prefix) = text.get(s..s + 2) else {
            return false;
        };
        if FORBIDDEN.contains(&prefix)
            || prefix == "ZZ"
            || !edges_ok(text, s, e, &[])
            || !context_before(text, s, WINDOW_ID, &NINO_CTX)
        {
            return false;
        }
        push(out, text, EntityKind::Identifiant, s, e, CONF_ID_CONTEXT);
        true
    });
}

fn ssn(text: &str, out: &mut Out) {
    scan(&SSN, text, |s, e| {
        let Some(m) = text.get(s..e) else {
            return false;
        };
        let mut parts = m.split('-');
        let (Some(area), Some(group), Some(serial)) = (
            parts.next().and_then(|p| p.parse::<u32>().ok()),
            parts.next().and_then(|p| p.parse::<u32>().ok()),
            parts.next().and_then(|p| p.parse::<u32>().ok()),
        ) else {
            return false;
        };
        let plausible = area != 0 && area != 666 && area < 900 && group != 0 && serial != 0;
        if !plausible || !edges_ok(text, s, e, &[]) || !context_before(text, s, WINDOW_ID, &SSN_CTX)
        {
            return false;
        }
        push(out, text, EntityKind::Identifiant, s, e, CONF_ID_CONTEXT);
        true
    });
}

// ---------------------------------------------------------------------------
// Plaque
// ---------------------------------------------------------------------------

fn plaque(text: &str, out: &mut Out) {
    scan(&PLAQUE, text, |s, e| {
        if !edges_ok(text, s, e, &[]) {
            return false;
        }
        push(out, text, EntityKind::Plaque, s, e, CONF_PLAQUE);
        true
    });
    // Plaque calédonienne : 3 à 6 chiffres puis « NC ».
    scan(&PLAQUE_NC, text, |s, e| {
        if !strict_left_ok(text, s)
            || !right_ok(text, e, GLUE)
            || context_before(text, s, WINDOW_MONEY, &MONEY_CTX)
        {
            return false;
        }
        push(out, text, EntityKind::Plaque, s, e, CONF_PLAQUE);
        true
    });
}

// ---------------------------------------------------------------------------
// Adresse
// ---------------------------------------------------------------------------

/// Mots qui terminent un nom de voie.
const STREET_STOPWORDS: &[&str] = &[
    "et", "ou", "mais", "puis", "qui", "que", "où", "donc", "car", "alors", "est", "sont", "était",
    "avec", "sans", "pour", "chez", "à", "près", "dans",
];
/// Abréviations dont le point final appartient au nom.
const ABBREVIATIONS: &[&str] = &[
    "st", "ste", "gal", "gén", "mal", "dr", "pr", "lt", "cdt", "col", "mgr", "pdt", "prés", "cap",
    "bd", "av",
];
/// Premiers mots de communes composées (« Le Havre », « Saint Denis »).
const CITY_PREFIXES: &[&str] = &[
    "le",
    "la",
    "les",
    "l'",
    "saint",
    "sainte",
    "st",
    "ste",
    "port",
    "mont",
    "fort",
    "grand",
    "petit",
    "villeneuve",
    "cap",
    "bourg",
];

fn is_delimiter(c: char) -> bool {
    matches!(c, ',' | ';' | '(' | ')' | '|' | ':' | '«' | '»' | '"')
}

fn is_postal_code(token: &str) -> bool {
    token.len() == 5 && token.chars().all(|c| c.is_ascii_digit())
}

/// Fin du nom de voie qui commence en `from` (juste après le type de voie).
fn street_end(text: &str, from: usize) -> Option<usize> {
    let rest = text.get(from..)?;
    let mut pos = 0;
    let mut words = 0;
    let mut capital = false;
    let mut last_end = None;
    loop {
        let after = rest.get(pos..)?;
        let skipped: usize = after
            .chars()
            .take_while(|c| is_space(*c))
            .map(char::len_utf8)
            .sum();
        if words == 0 && skipped == 0 {
            return None;
        }
        pos += skipped;
        let after = rest.get(pos..)?;
        let tok_len: usize = after
            .chars()
            .take_while(|c| !c.is_whitespace() && !is_delimiter(*c))
            .map(char::len_utf8)
            .sum();
        if tok_len == 0 || words >= MAX_STREET_WORDS {
            break;
        }
        let token = after.get(..tok_len)?;
        let word = token.trim_end_matches(['.', '!', '?', '…']);
        let lower = word.to_lowercase();
        if word.is_empty()
            || is_postal_code(word)
            || matches!(word, "-" | "–" | "—")
            || STREET_STOPWORDS.contains(&lower.as_str())
        {
            break;
        }
        let abbreviation = word.len() != token.len() && ABBREVIATIONS.contains(&lower.as_str());
        let used = if abbreviation { tok_len } else { word.len() };
        last_end = Some(from + pos + used);
        words += 1;
        capital |= word.chars().any(|c| c.is_uppercase() || c.is_ascii_digit());
        pos += tok_len;
        if word.len() != token.len() && !abbreviation {
            break; // fin de phrase
        }
    }
    if capital { last_end } else { None }
}

/// Mot de commune à partir de `pos` : lettres, tirets, apostrophes, majuscule initiale.
fn city_word(text: &str, pos: usize) -> Option<usize> {
    let rest = text.get(pos..)?;
    if !rest.chars().next()?.is_uppercase() {
        return None;
    }
    let len: usize = rest
        .chars()
        .take_while(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
        .map(char::len_utf8)
        .sum();
    Some(pos + len)
}

fn skip_spaces(text: &str, pos: usize) -> usize {
    pos + text
        .get(pos..)
        .unwrap_or("")
        .chars()
        .take_while(|c| is_space(*c))
        .map(char::len_utf8)
        .sum::<usize>()
}

/// Fin d'une commune (avec « Cedex [n] » éventuel) qui commence en `pos`.
fn city_end(text: &str, pos: usize) -> Option<usize> {
    let mut end = city_word(text, pos)?;
    let first = text.get(pos..end)?.to_lowercase();
    if CITY_PREFIXES.contains(&first.as_str()) {
        let next = skip_spaces(text, end);
        if next > end
            && let Some(e) = city_word(text, next)
        {
            end = e;
        }
    }
    let next = skip_spaces(text, end);
    let cedex = if next > end {
        city_word(text, next)
    } else {
        None
    };
    if let Some(e) = cedex
        && text
            .get(next..e)
            .is_some_and(|w| w.eq_ignore_ascii_case("cedex"))
    {
        end = e;
        let n = skip_spaces(text, end);
        let digits = text
            .get(n..)
            .unwrap_or("")
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        if n > end
            && (1..=2).contains(&digits)
            && !char_after(text, n + digits).is_some_and(is_word)
        {
            end = n + digits;
        }
    }
    Some(end)
}

/// Fin de « code postal + commune » qui suit `from` (séparateurs courts admis).
fn postal_city_end(text: &str, from: usize) -> Option<usize> {
    let rest = text.get(from..)?;
    let mut skipped_bytes = 0;
    let mut skipped_chars = 0;
    for c in rest.chars() {
        if is_space(c) || matches!(c, ',' | '-' | '–' | '\n' | '\r') {
            skipped_bytes += c.len_utf8();
            skipped_chars += 1;
            if skipped_chars > MAX_POSTAL_GAP {
                return None;
            }
        } else {
            break;
        }
    }
    let p = from + skipped_bytes;
    let code = text.get(p..p + 5)?;
    if !is_postal_code(code) || char_after(text, p + 5).is_some_and(is_word) {
        return None;
    }
    let city = skip_spaces(text, p + 5);
    if city == p + 5 {
        return None;
    }
    city_end(text, city)
}

fn adresse(text: &str, out: &mut Out) {
    scan(&ADDR_START, text, |s, e| {
        if !left_ok(text, s, GLUE) {
            return false;
        }
        let Some(name_end) = street_end(text, e) else {
            return false;
        };
        let end = postal_city_end(text, name_end).unwrap_or(name_end);
        push(out, text, EntityKind::Adresse, s, end, CONF_ADRESSE);
        true
    });
    adresse_etrangere(text, out);
    scan(&BOITE_POSTALE, text, |s, e| {
        if !left_ok(text, s, &[]) {
            return false;
        }
        let end = postal_city_end(text, e).unwrap_or(e);
        push(out, text, EntityKind::Adresse, s, end, CONF_ADRESSE);
        true
    });
}

/// Vrai si le dernier mot de `text[..end]` contient un chiffre (numéro de voie).
fn ends_with_number(text: &str, end: usize) -> bool {
    text.get(..end)
        .and_then(|h| h.split_whitespace().next_back())
        .is_some_and(|w| w.chars().any(|c| c.is_ascii_digit()) && !is_postal_code(w))
}

/// Numéro de voie placé après le nom (« Corso Italia, 5 ») : 1 à 4 chiffres.
fn trailing_number(text: &str, from: usize) -> Option<usize> {
    let rest = text.get(from..)?;
    let skipped: usize = rest
        .chars()
        .take_while(|c| *c == ',' || is_space(*c))
        .map(char::len_utf8)
        .sum();
    let digits = text
        .get(from + skipped..)?
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    let end = from + skipped + digits;
    ((1..=4).contains(&digits) && !char_after(text, end).is_some_and(is_word)).then_some(end)
}

/// Voies espagnoles, italiennes, allemandes et anglaises.
fn adresse_etrangere(text: &str, out: &mut Out) {
    // Espagnol : type, nom, numéro puis code postal et ville s'ils suivent.
    scan(&ADDR_ES, text, |s, e| {
        if !left_ok(text, s, GLUE) {
            return false;
        }
        let Some(mut end) = street_end(text, e) else {
            return false;
        };
        if !ends_with_number(text, end) {
            end = trailing_number(text, end).unwrap_or(end);
        }
        let end = postal_city_end(text, end).unwrap_or(end);
        push(out, text, EntityKind::Adresse, s, end, CONF_ADRESSE);
        true
    });
    // Italien : le numéro est obligatoire (« via » est aussi un mot français).
    scan(&ADDR_IT, text, |s, e| {
        if !left_ok(text, s, GLUE) {
            return false;
        }
        let Some(mut end) = street_end(text, e) else {
            return false;
        };
        if !ends_with_number(text, end) {
            let Some(n) = trailing_number(text, end) else {
                return false;
            };
            end = n;
        }
        let end = postal_city_end(text, end).unwrap_or(end);
        push(out, text, EntityKind::Adresse, s, end, CONF_ADRESSE);
        true
    });
    // Allemand : « …straße 12 », « …str. 12 ».
    scan(&ADDR_DE, text, |s, e| {
        let end = postal_city_end(text, e).unwrap_or(e);
        push(out, text, EntityKind::Adresse, s, end, CONF_ADRESSE);
        true
    });
    // Anglais : « 10 Downing Street, London SW1A 2AA ».
    scan(&ADDR_EN, text, |s, e| {
        let dotted = text.get(..e).is_some_and(|h| h.ends_with('.'));
        if !left_ok(text, s, GLUE) || !(dotted || right_ok(text, e, &[])) {
            return false;
        }
        let tail = LazyLock::force(&ADDR_EN_TAIL)
            .as_ref()
            .and_then(|re| re.find(text.get(e..).unwrap_or("")))
            .map_or(0, |m| m.end());
        push(out, text, EntityKind::Adresse, s, e + tail, CONF_ADRESSE);
        true
    });
}

// ---------------------------------------------------------------------------
// Date de naissance
// ---------------------------------------------------------------------------

fn date_naiss(text: &str, out: &mut Out) {
    let Some(re) = LazyLock::force(&DATE_NAISS).as_ref() else {
        return;
    };
    for caps in re.captures_iter(text) {
        let Some(d) = caps.name("d") else {
            continue;
        };
        let Some(whole) = caps.get(0) else {
            continue;
        };
        if left_ok(text, whole.start(), &[]) && right_ok(text, d.end(), &[]) {
            push(
                out,
                text,
                EntityKind::DateNaiss,
                d.start(),
                d.end(),
                CONF_DATE_NAISS,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::{BlockContext, BlockRole, Profile};
    use proptest::prelude::*;

    fn block(text: &str) -> TextBlock {
        TextBlock {
            text: text.to_string(),
            ctx: BlockContext::new("test", BlockRole::Body),
        }
    }

    fn all_kinds() -> DetectOptions {
        DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        }
    }

    fn detect(text: &str) -> Vec<Detection> {
        RuleDetector.detect(&block(text), &all_kinds())
    }

    /// Valeurs détectées pour un type, lues par `&text[start..end]`.
    fn values(text: &str, kind: EntityKind) -> Vec<String> {
        detect(text)
            .into_iter()
            .filter(|d| d.kind == kind)
            .map(|d| text[d.start..d.end].to_string())
            .collect()
    }

    fn assert_found(text: &str, kind: EntityKind, expected: &str) {
        assert_eq!(values(text, kind), vec![expected.to_string()], "{text:?}");
    }

    fn assert_none(text: &str, kind: EntityKind) {
        assert!(
            values(text, kind).is_empty(),
            "{text:?} -> {:?}",
            values(text, kind)
        );
    }

    #[test]
    fn all_regexes_compile() {
        let all: &[&Re] = &[
            &EMAIL,
            &URL,
            &IPV4,
            &IPV6,
            &TEL_FR_INTL,
            &TEL_NC,
            &TEL_FR_NAT,
            &TEL_INTL,
            &TEL_LOCAL,
            &TEL_CTX,
            &IBAN_START,
            &CB,
            &NIR,
            &SIRET,
            &SIREN,
            &SIRET_CTX,
            &SIREN_CTX,
            &RIDET,
            &TVA_FR,
            &DNI,
            &NIE,
            &CODICE,
            &NISS,
            &NISS_CTX,
            &NINO,
            &NINO_CTX,
            &SSN,
            &SSN_CTX,
            &PLAQUE,
            &ADDR_START,
            &BOITE_POSTALE,
            &DATE_NAISS,
        ];
        for (i, re) in all.iter().enumerate() {
            assert!(LazyLock::force(re).is_some(), "regex #{i} ne compile pas");
        }
    }

    #[test]
    fn detector_name() {
        assert_eq!(RuleDetector.name(), "rules");
    }

    #[test]
    fn respects_wants() {
        let mut opts = all_kinds();
        opts.disabled.insert(EntityKind::Email);
        let t = "jean@example.com 06 12 34 56 78";
        let kinds: Vec<_> = RuleDetector
            .detect(&block(t), &opts)
            .iter()
            .map(|d| d.kind)
            .collect();
        assert_eq!(kinds, vec![EntityKind::Tel]);
        let none = DetectOptions {
            disabled: EntityKind::ALL.iter().copied().collect(),
            ..Default::default()
        };
        assert!(RuleDetector.detect(&block(t), &none).is_empty());
    }

    #[test]
    fn sources_and_offsets() {
        let t = "é jean@example.com";
        let d = detect(t);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].source, Source::Rule);
        assert_eq!(&t[d[0].start..d[0].end], "jean@example.com");
    }

    #[test]
    fn email_cases() {
        let k = EntityKind::Email;
        assert_found("jean.dupont@example.com", k, "jean.dupont@example.com");
        assert_found(
            "Écrire à jean.dupont@example.com.",
            k,
            "jean.dupont@example.com",
        );
        assert_found("(marie_l@sub.example.org)", k, "marie_l@sub.example.org");
        assert_found("Mail :\u{a0}a+b@example.fr", k, "a+b@example.fr");
        assert_found("éric@example.com", k, "éric@example.com");
        assert_none("jean@", k);
        assert_none("@example.com", k);
        assert_none("jean@example", k);
        assert_none("rendez-vous à 10h", k);
    }

    #[test]
    fn url_cases() {
        let k = EntityKind::Url;
        assert_found("https://example.com/a?b=1.", k, "https://example.com/a?b=1");
        assert_found("(www.example.com)", k, "www.example.com");
        assert_found(
            "voir http://example.com/wiki/Foo_(bar).",
            k,
            "http://example.com/wiki/Foo_(bar)",
        );
        assert_found("HTTPS://EXAMPLE.COM", k, "HTTPS://EXAMPLE.COM");
        assert_found("www.example.com", k, "www.example.com");
        assert_none("https://", k);
        assert_none("www.", k);
        assert_none("awww.example.com", k);
    }

    #[test]
    fn ip_cases() {
        let k = EntityKind::Ip;
        assert_found("192.168.0.1", k, "192.168.0.1");
        assert_found("(10.0.0.1)", k, "10.0.0.1");
        assert_found("IP : 10.0.0.1.", k, "10.0.0.1");
        assert_found("hôte 255.255.255.0\u{a0}!", k, "255.255.255.0");
        assert_found("réseau 192.168.0.1/24", k, "192.168.0.1");
        assert_found("adresse fe80::1.", k, "fe80::1");
        assert_found("2001:db8::ff00:42:8329", k, "2001:db8::ff00:42:8329");
        assert_found("conversion 10.2.3.4", k, "10.2.3.4"); // test adapté : a.b.c.d à octets < 10 exige désormais un contexte réseau
        assert_none("1.2.3", k);
        assert_none("256.1.1.1", k);
        assert_none("1.2.3.4.5", k);
        assert_none("01.02.03.04", k);
        assert_none("version 1.2.3.4", k);
        assert_none("Version\u{a0}1.2.3.4", k);
        assert_none("v1.2.3.4", k);
        assert_none("§ 1.2.3.4", k);
        assert_none("à 12:30:45 précises", k);
        assert_none("std::string", k);
    }

    #[test]
    fn tel_valid() {
        let k = EntityKind::Tel;
        assert_found("+33 6 12 34 56 78", k, "+33 6 12 34 56 78");
        assert_found("+33 (0)6 12 34 56 78", k, "+33 (0)6 12 34 56 78");
        assert_found("+33612345678", k, "+33612345678");
        assert_found("Tel +33 6 12 34 56 78.", k, "+33 6 12 34 56 78");
        assert_found("(+33 6 12 34 56 78)", k, "+33 6 12 34 56 78");
        assert_found(
            "+33\u{a0}6\u{a0}12\u{a0}34\u{a0}56\u{a0}78",
            k,
            "+33\u{a0}6\u{a0}12\u{a0}34\u{a0}56\u{a0}78",
        );
        assert_found("+33 6 12 34 56 78 15 jours", k, "+33 6 12 34 56 78");
        assert_found("06 12 34 56 78", k, "06 12 34 56 78");
        assert_found("06.12.34.56.78", k, "06.12.34.56.78");
        assert_found("0612345678", k, "0612345678");
        assert_found("(06 12 34 56 78).", k, "06 12 34 56 78");
        assert_found(
            "06\u{a0}12\u{a0}34\u{a0}56\u{a0}78",
            k,
            "06\u{a0}12\u{a0}34\u{a0}56\u{a0}78",
        );
        assert_found("appelez le 01-23-45-67-89.", k, "01-23-45-67-89");
        assert_found("+687 28 12 34", k, "+687 28 12 34");
        assert_found(
            "+687\u{a0}28\u{a0}12\u{a0}34.",
            k,
            "+687\u{a0}28\u{a0}12\u{a0}34",
        );
        assert_found("+687.28.12.34", k, "+687.28.12.34");
        assert_found("+44 20 7946 0958", k, "+44 20 7946 0958");
        assert_found("+1 (202) 555-0143", k, "+1 (202) 555-0143");
        assert_found("+44 20 7946 0958 2024", k, "+44 20 7946 0958");
    }

    #[test]
    fn tel_local_needs_context() {
        let k = EntityKind::Tel;
        assert_found("Tél : 28 12 34", k, "28 12 34");
        assert_found("tel 28.12.34.", k, "28.12.34");
        assert_found("FAX\u{a0}28 12 34", k, "28 12 34");
        assert_found("Pour joindre Marie : 28 12 34", k, "28 12 34");
        assert_found("(Portable 281234)", k, "281234");
        assert_found("TÉLÉPHONE : 28 12 34", k, "28 12 34");
    }

    #[test]
    fn tel_rejected() {
        let k = EntityKind::Tel;
        assert_none("28 12 34", k);
        assert_none("Il y a 28 12 34 pommes", k);
        assert_none("Tél : blabla blabla blabla blabla 28 12 34", k);
        assert_none("hôtel 28 12 34", k);
        assert_none("Tél 28 12 34 56", k);
        assert_none("01 23 45 67 8", k);
        assert_none("+33 6 12", k);
        assert_none("1234567890", k);
        assert_none("20240612345678", k);
    }

    #[test]
    fn iban_valid_cases() {
        let k = EntityKind::Iban;
        assert_found(
            "IBAN : FR76 3000 6000 0112 3456 7890 189.",
            k,
            "FR76 3000 6000 0112 3456 7890 189",
        );
        assert_found(
            "FR7630006000011234567890189",
            k,
            "FR7630006000011234567890189",
        );
        assert_found(
            "FR76\u{a0}3000\u{a0}6000\u{a0}0112\u{a0}3456\u{a0}7890\u{a0}189",
            k,
            "FR76\u{a0}3000\u{a0}6000\u{a0}0112\u{a0}3456\u{a0}7890\u{a0}189",
        );
        assert_found(
            "(GB82 WEST 1234 5698 7654 32)",
            k,
            "GB82 WEST 1234 5698 7654 32",
        );
        assert_found(
            "NC49 3000 6000 0112 3456 7890 189 ok",
            k,
            "NC49 3000 6000 0112 3456 7890 189",
        );
        assert_found(
            "PF22 3000 6000 0112 3456 7890 189",
            k,
            "PF22 3000 6000 0112 3456 7890 189",
        );
        assert_found(
            "DE89 3704 0044 0532 0130 00",
            k,
            "DE89 3704 0044 0532 0130 00",
        );
        assert_found("BE68 5390 0754 7034.", k, "BE68 5390 0754 7034");
        assert_found(
            "FR14 2004 1010 0505 0001 3M02 606",
            k,
            "FR14 2004 1010 0505 0001 3M02 606",
        );
    }

    #[test]
    fn iban_rejected() {
        let k = EntityKind::Iban;
        assert_none("FR77 3000 6000 0112 3456 7890 189", k);
        assert_none("FR76 3000 6000 0112 3456 7890 18", k);
        assert_none("FR76 3000 6000 0112 3456 7890 1890", k);
        assert_none("XX76 3000 6000 0112 3456 7890 189", k);
        assert_none("XFR76 3000 6000 0112 3456 7890 189", k);
        assert_none("FR76 30006000 0112 3456 7890 189", k);
    }

    #[test]
    fn cb_valid_cases() {
        let k = EntityKind::Cb;
        assert_found("4111 1111 1111 1111", k, "4111 1111 1111 1111");
        assert_found("carte 4111111111111111.", k, "4111111111111111");
        assert_found("(5555-5555-5555-4444)", k, "5555-5555-5555-4444");
        assert_found("2223000048400011", k, "2223000048400011");
        assert_found("3782 822463 10005", k, "3782 822463 10005");
        assert_found(
            "6011\u{a0}1111\u{a0}1111\u{a0}1117",
            k,
            "6011\u{a0}1111\u{a0}1111\u{a0}1117",
        );
        assert_found("4111 1111 1111 1111 2024", k, "4111 1111 1111 1111");
    }

    #[test]
    fn cb_rejected() {
        let k = EntityKind::Cb;
        assert_none("4111 1111 1111 1112", k);
        assert_none("1234567890123456", k);
        assert_none("9999999999999995", k); // Luhn valide, préfixe inconnu
        assert_none("411111111111", k);
        assert_none("41111111111111111111", k);
        assert_none("x4111111111111111", k);
    }

    #[test]
    fn nir_cases() {
        let k = EntityKind::Nir;
        assert_found("1 84 03 75 120 005 49", k, "1 84 03 75 120 005 49");
        assert_found("184037512000549", k, "184037512000549");
        assert_found("2 91 07 2A 004 012 49", k, "2 91 07 2A 004 012 49");
        assert_found("(190062B00123427)", k, "190062B00123427");
        assert_found(
            "NIR\u{a0}1\u{a0}84\u{a0}03\u{a0}75\u{a0}120\u{a0}005\u{a0}49.",
            k,
            "1\u{a0}84\u{a0}03\u{a0}75\u{a0}120\u{a0}005\u{a0}49",
        );
        assert_none("1 84 03 75 120 005 48", k);
        assert_none("184037512000500", k);
        assert_none("18403751200054", k);
        assert_none("1840375120005499", k);
        assert_none("291072A00401250", k);
    }

    #[test]
    fn siret_cases() {
        let k = EntityKind::Siret;
        assert_found("SIRET : 732 829 320 00074", k, "732 829 320 00074");
        assert_found("732 829 320 00074", k, "732 829 320 00074");
        assert_found("SIRET 73282932000074.", k, "73282932000074");
        assert_found("(siret\u{a0}73282932000074)", k, "73282932000074");
    }

    #[test]
    fn siret_rejected() {
        let k = EntityKind::Siret;
        assert_none("73282932000074", k);
        assert_none("732 829 320 00075", k);
        assert_none("SIRET 73282932000075", k);
        assert_none("SIRET 7328293200007", k);
        assert_none("732 829 320", k);
    }

    #[test]
    fn siren_cases() {
        let k = EntityKind::Siret;
        assert_found("SIREN : 732 829 320", k, "732 829 320");
        assert_found("RCS Nouméa 732 829 320.", k, "732 829 320");
        assert_found("immatriculée sous le n° 732829320.", k, "732829320");
        assert_found("(SIREN 732829320)", k, "732829320");
        assert_none("SIREN 123 456 789", k);
        assert_none("fiche 732 829 320", k);
        assert_none("RCS 7328293201", k);
    }

    #[test]
    fn ridet_cases() {
        let k = EntityKind::Ridet;
        assert_found("RIDET : 1 234 567.001", k, "1 234 567.001");
        assert_found("RIDET 1234567.001.", k, "1234567.001");
        assert_found("RIDET: 0 123 456", k, "0 123 456");
        assert_found(
            "(RIDET\u{a0}1\u{a0}234\u{a0}567.001)",
            k,
            "1\u{a0}234\u{a0}567.001",
        );
        assert_found("n° RIDET 0 123 456.", k, "0 123 456");
        assert_none("1 234 567.001", k);
        assert_none(
            "RIDET numéro d'identification de l'entreprise : 1 234 567",
            k,
        );
        assert_none("RIDET 12 34", k);
    }

    #[test]
    fn identifiant_cases() {
        let k = EntityKind::Identifiant;
        assert_found("TVA FR44 732 829 320", k, "FR44 732 829 320");
        assert_found("FR44732829320.", k, "FR44732829320");
        assert_found(
            "(FR44\u{a0}732\u{a0}829\u{a0}320)",
            k,
            "FR44\u{a0}732\u{a0}829\u{a0}320",
        );
        assert_found("DNI 12345678Z.", k, "12345678Z");
        assert_found("X1234567L", k, "X1234567L");
        assert_found("CF : RSSMRA85T10A562S", k, "RSSMRA85T10A562S");
        assert_found("NISS 85.07.30-033.28", k, "85.07.30-033.28");
        assert_found("(85.07.30-033.28)", k, "85.07.30-033.28");
        assert_found("NISS 85073003328", k, "85073003328");
        assert_found("NINO : AB 12 34 56 C", k, "AB 12 34 56 C");
        assert_found("national insurance number AB123456C.", k, "AB123456C");
        assert_found("SSN : 123-45-6789", k, "123-45-6789");
        assert_found("social security 123-45-6789.", k, "123-45-6789");
    }

    #[test]
    fn identifiant_rejected() {
        let k = EntityKind::Identifiant;
        assert_none("FR45 732 829 320", k);
        assert_none("FR44 123 456 789", k);
        assert_none("12345678A", k);
        assert_none("X1234567A", k);
        assert_none("RSSMRA85T10A562T", k);
        assert_none("85.07.30-033.29", k);
        assert_none("85073003328", k);
        assert_none("AB 12 34 56 C", k);
        assert_none("NINO : BG 12 34 56 C", k);
        assert_none("123-45-6789", k);
        assert_none("SSN : 000-45-6789", k);
        assert_none("SSN : 666-45-6789", k);
        assert_none("SSN : 123-00-6789", k);
    }

    #[test]
    fn plaque_cases() {
        let k = EntityKind::Plaque;
        assert_found("AB-123-CD", k, "AB-123-CD");
        assert_found("(AB-123-CD).", k, "AB-123-CD");
        assert_found("véhicule\u{a0}EF-456-GH", k, "EF-456-GH");
        assert_none("AB-123-C", k);
        assert_none("IO-123-CD", k);
        assert_none("AB-1234-CD", k);
        assert_none("XAB-123-CD", k);
        assert_none("ab-123-cd", k);
    }

    #[test]
    fn adresse_cases() {
        let k = EntityKind::Adresse;
        assert_found(
            "12 rue de la Paix, 75002 Paris",
            k,
            "12 rue de la Paix, 75002 Paris",
        );
        assert_found("Il habite au 12 rue de la Paix.", k, "12 rue de la Paix");
        assert_found(
            "3 bis avenue Victor Hugo 75116 Paris.",
            k,
            "3 bis avenue Victor Hugo 75116 Paris",
        );
        assert_found(
            "45, boulevard Haussmann, 75009 Paris",
            k,
            "45, boulevard Haussmann, 75009 Paris",
        );
        assert_found("12 av. Foch (Nouméa)", k, "12 av. Foch");
        assert_found(
            "8 impasse des Lilas\n98800 Nouméa Cedex",
            k,
            "8 impasse des Lilas\n98800 Nouméa Cedex",
        );
        assert_found(
            "12\u{a0}rue\u{a0}de\u{a0}la\u{a0}Paix, 75002\u{a0}Paris",
            k,
            "12\u{a0}rue\u{a0}de\u{a0}la\u{a0}Paix, 75002\u{a0}Paris",
        );
        assert_found(
            "12 RUE DE LA PAIX 75002 PARIS",
            k,
            "12 RUE DE LA PAIX 75002 PARIS",
        );
        assert_found(
            "5 allée des Tilleuls, 76600 Le Havre.",
            k,
            "5 allée des Tilleuls, 76600 Le Havre",
        );
        assert_found("9 place de l'Étoile", k, "9 place de l'Étoile");
        assert_found(
            "10 route de la Baie, 98800 Nouméa Cedex 2.",
            k,
            "10 route de la Baie, 98800 Nouméa Cedex 2",
        );
        assert_found(
            "12 rue de la Paix, 75002 et ensuite",
            k,
            "12 rue de la Paix",
        );
    }

    #[test]
    fn adresse_two_streets_and_rejected() {
        let k = EntityKind::Adresse;
        assert_eq!(
            values("12 rue de la Paix et 14 rue Truc", k),
            vec!["12 rue de la Paix".to_string(), "14 rue Truc".to_string()]
        );
        assert_none("12 rue", k);
        assert_none("J'ai 3 places libres", k);
        assert_none("il y a 12 place de parking", k);
        assert_none("rue de la Paix", k);
        assert_none("12 cours de maths", k);
    }

    #[test]
    fn ip_version_context_on_the_line() {
        let k = EntityKind::Ip;
        assert_none("Version du gabarit : 1.2.3.4.", k);
        assert_none("Versión del informe: 1.2.3.4.", k);
        assert_none("Release notes, build 10.2.3.4", k);
        assert_none("révision 10.2.3.4", k);
        assert_none("rev. 10.2.3.4", k);
        assert_none("1.2.3.4", k); // octets < 10 sans contexte réseau
        assert_found("Serveur : 1.2.3.4", k, "1.2.3.4");
        assert_found("ping 1.2.3.4.", k, "1.2.3.4");
        assert_found("DNS 8.8.8.8", k, "8.8.8.8");
        assert_found("Adresse IP : 1.2.3.4", k, "1.2.3.4");
        assert_found("hôte 10.2.3.4", k, "10.2.3.4");
        // le mot de contexte doit être sur la même ligne
        assert_found("Version 2\nIP 10.2.3.4", k, "10.2.3.4");
        // « conversion » n'est pas « version »
        assert_found("conversion 10.2.3.4", k, "10.2.3.4");
    }

    #[test]
    fn plaque_caledonienne() {
        let k = EntityKind::Plaque;
        assert_found(
            "Véhicule de service : 123 456 NC ; matricule",
            k,
            "123 456 NC",
        );
        assert_found("(123456 NC).", k, "123456 NC");
        assert_found("plaque 123\u{a0}456\u{a0}NC", k, "123\u{a0}456\u{a0}NC");
        assert_found("plaque 123 NC", k, "123 NC");
        assert_none("Montant : 123 456 NC", k);
        assert_none("Total 1 500 NC", k);
        assert_none("prix 25000 NC", k);
        assert_none("12 NC", k);
        assert_none("1234567 NC", k);
        assert_none("2 123 456 NC", k);
        assert_none("123 456 NCA", k);
        assert_none("123456NC", k);
    }

    #[test]
    fn adresses_etrangeres_valides() {
        let k = EntityKind::Adresse;
        assert_found(
            "Dirección: Calle de las Acacias 12, 28013 Madrid.",
            k,
            "Calle de las Acacias 12, 28013 Madrid",
        );
        assert_found("Avenida de América, 5", k, "Avenida de América, 5");
        assert_found(
            "Paseo del Prado 3 28014 Madrid",
            k,
            "Paseo del Prado 3 28014 Madrid",
        );
        assert_found("Plaza Mayor", k, "Plaza Mayor");
        assert_found(
            "Carrer de Mallorca 401, 08013 Barcelona",
            k,
            "Carrer de Mallorca 401, 08013 Barcelona",
        );
        assert_found("Via Roma 12, 00184 Roma.", k, "Via Roma 12, 00184 Roma");
        assert_found("Corso Italia, 5", k, "Corso Italia, 5");
        assert_found("Piazza Navona 1", k, "Piazza Navona 1");
        assert_found(
            "Hauptstraße 12, 10115 Berlin",
            k,
            "Hauptstraße 12, 10115 Berlin",
        );
        assert_found("Karl-Marx-Str. 5.", k, "Karl-Marx-Str. 5");
        assert_found("Goethestrasse 7", k, "Goethestrasse 7");
        assert_found(
            "10 Downing Street, London SW1A 2AA.",
            k,
            "10 Downing Street, London SW1A 2AA",
        );
        assert_found("221B Baker Street.", k, "221B Baker Street");
        assert_found(
            "42 Wallaby Way, Springfield, IL 62704",
            k,
            "42 Wallaby Way, Springfield, IL 62704",
        );
        assert_found("12 Oak Rd.", k, "12 Oak Rd.");
        assert_found("5 Main St, Boston", k, "5 Main St, Boston");
        assert_found(
            "Calle\u{a0}Mayor\u{a0}5, 28013\u{a0}Madrid",
            k,
            "Calle\u{a0}Mayor\u{a0}5, 28013\u{a0}Madrid",
        );
    }

    #[test]
    fn adresses_etrangeres_rejetees() {
        let k = EntityKind::Adresse;
        assert_none("via Internet", k);
        assert_none("Via Internet", k);
        assert_none("la calle es larga", k);
        assert_none("Straße 12", k);
        assert_none("Hauptstraße", k);
        assert_none("12 oak street", k);
        assert_none("3 Stoppen Streets", k);
        assert_none("des gens via Roma", k);
    }

    #[test]
    fn boite_postale_cases() {
        let k = EntityKind::Adresse;
        assert_found("BP 1234", k, "BP 1234");
        assert_found(
            "B.P. 1234 98800 Nouméa Cedex",
            k,
            "B.P. 1234 98800 Nouméa Cedex",
        );
        assert_found("Boîte postale 12", k, "Boîte postale 12");
        assert_found(
            "BP 1234, 98800 NOUMEA CEDEX.",
            k,
            "BP 1234, 98800 NOUMEA CEDEX",
        );
        assert_found("(BP\u{a0}1234)", k, "BP\u{a0}1234");
        assert_found("BP: 77", k, "BP: 77");
        assert_none("BPX 1234", k);
        assert_none("BP", k);
    }

    #[test]
    fn date_naiss_cases() {
        let k = EntityKind::DateNaiss;
        assert_found("né le 12/03/1984.", k, "12/03/1984");
        assert_found("Née le 12-03-1984", k, "12-03-1984");
        assert_found("né(e) le 12 mars 1984", k, "12 mars 1984");
        assert_found("date de naissance : 1984-03-12", k, "1984-03-12");
        assert_found("born on 12 March 1984", k, "12 March 1984");
        assert_found("DOB: 12.03.1984", k, "12.03.1984");
        assert_found("(né le 1er mars 1984)", k, "1er mars 1984");
        assert_found(
            "Date\u{a0}de\u{a0}naissance\u{a0}:\u{a0}12/03/1984",
            k,
            "12/03/1984",
        );
        assert_found("né le 12 août 1984", k, "12 août 1984");
        assert_none("le 12/03/1984", k);
        assert_none("né le 32/13/1984", k);
        assert_none("né le 12/03/19845", k);
        assert_none("signé le 12/03/1984", k);
    }

    #[test]
    fn mixed_paragraph() {
        let t = "Mme X, née le 12/03/1984, joignable au 06 12 34 56 78 ou jean@example.com (IBAN FR76 3000 6000 0112 3456 7890 189).";
        let d = detect(t);
        let kinds: Vec<_> = d.iter().map(|x| x.kind).collect();
        for k in [
            EntityKind::DateNaiss,
            EntityKind::Tel,
            EntityKind::Email,
            EntityKind::Iban,
        ] {
            assert!(kinds.contains(&k), "{k:?} manquant dans {kinds:?}");
        }
        for x in &d {
            assert!(t.is_char_boundary(x.start) && t.is_char_boundary(x.end));
        }
    }

    const FRAGMENTS: &[&str] = &[
        "jean@example.com",
        "https://example.com/x",
        "192.168.0.1",
        "fe80::1",
        "+33 6 12 34 56 78",
        "06 12 34 56 78",
        "+687 28 12 34",
        "Tél :",
        "28 12 34",
        "FR76 3000 6000 0112 3456 7890 189",
        "4111 1111 1111 1111",
        "1 84 03 75 120 005 49",
        "SIRET",
        "732 829 320 00074",
        "RIDET",
        "1 234 567.001",
        "AB-123-CD",
        "12 rue de la Paix, 75002 Paris",
        "BP 1234",
        "98800 Nouméa Cedex",
        "né le 12/03/1984",
        "\u{a0}",
        "\u{202f}",
        " ",
        "é",
        "😀",
        "\n",
        "(",
        ")",
        ".",
        ",",
        "-",
        "+",
        ":",
        "version",
        "§",
        "SSN",
        "123-45-6789",
        "RSSMRA85T10A562S",
        "X1234567L",
        "FR44 732 829 320",
    ];

    fn check_offsets(text: &str, dets: &[Detection]) {
        for d in dets {
            assert!(d.start < d.end, "{d:?}");
            assert!(d.end <= text.len(), "{d:?} hors bornes");
            assert!(text.is_char_boundary(d.start), "{d:?}");
            assert!(text.is_char_boundary(d.end), "{d:?}");
        }
    }

    proptest! {
        #[test]
        fn never_panics_on_any_string(s in "\\PC{0,300}") {
            let dets = detect(&s);
            check_offsets(&s, &dets);
        }

        #[test]
        fn never_panics_on_any_chars(s in proptest::collection::vec(any::<char>(), 0..200)) {
            let s: String = s.into_iter().collect();
            let dets = detect(&s);
            check_offsets(&s, &dets);
        }

        #[test]
        fn never_panics_on_structured_text(parts in proptest::collection::vec(proptest::sample::select(FRAGMENTS.to_vec()), 0..25)) {
            let s = parts.concat();
            let dets = detect(&s);
            check_offsets(&s, &dets);
            let spaced = parts.join(" ");
            let dets = detect(&spaced);
            check_offsets(&spaced, &dets);
        }
    }
}
