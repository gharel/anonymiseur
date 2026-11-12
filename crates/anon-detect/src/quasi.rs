//! Quasi-identifiants : dates, âges, montants, fonctions, matricules.
//!
//! Ils n'identifient pas seuls mais leur combinaison le peut : ils ne sont
//! détectés qu'en profil « Renforcé » (voir `DetectOptions::wants`).

use std::sync::LazyLock;

use anon_core::{DetectOptions, Detection, Detector, EntityKind, TextBlock};
use regex::Regex;

use crate::fold::fold;
use crate::rules::{
    GLUE, Out, Re, char_after, edges_ok, left_ok, line_window_matches, overlaps, pat, push, re,
    right_ok, scan, skip_spaces, strict_left_ok,
};

// ---------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------

/// Une date seule est moins sûre qu'une date de naissance (0.9) : à intervalle
/// égal, `merge` garde donc `DateNaiss`.
const CONF_DATE: f32 = 0.8;
const CONF_AGE: f32 = 0.85;
const CONF_MONTANT: f32 = 0.85;
const CONF_FONCTION: f32 = 0.8;
const CONF_MATRICULE: f32 = 0.85;

/// Âge maximal plausible.
const MAX_AGE: u32 = 120;
/// Fenêtre (même ligne) où chercher un mot qui annule un âge (« depuis 10 ans »).
const WINDOW_AGE: usize = 20;
/// Mots maximum du complément d'un intitulé de poste (connecteurs compris).
const MAX_FUNCTION_WORDS: usize = 4;

const MONTH_NAMES: &[(&str, u32)] = &[
    ("janvier", 1),
    ("février", 2),
    ("fevrier", 2),
    ("mars", 3),
    ("avril", 4),
    ("mai", 5),
    ("juin", 6),
    ("juillet", 7),
    ("août", 8),
    ("aout", 8),
    ("septembre", 9),
    ("octobre", 10),
    ("novembre", 11),
    ("décembre", 12),
    ("decembre", 12),
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
    ("enero", 1),
    ("febrero", 2),
    ("marzo", 3),
    ("mayo", 5),
    ("junio", 6),
    ("julio", 7),
    ("agosto", 8),
    ("septiembre", 9),
    ("setiembre", 9),
    ("octubre", 10),
    ("noviembre", 11),
    ("diciembre", 12),
    ("abril", 4),
];

/// Intitulés qui se suffisent à eux-mêmes (fragments d'expression régulière).
const TITLES_SOLO: &[&str] = &[
    r"g[ée]rant(?:e|\(e\))?",
    r"(?:vice[- ])?pr[ée]sident(?:e|\(e\))?",
    r"pdg",
    r"dg",
    r"daf",
    r"drh",
    r"dsi",
    r"comptable",
    r"notaire",
    r"m[ée]decin",
    r"avocat(?:e|\(e\))?",
    r"consultant(?:e|\(e\))?",
    r"ing[ée]nieur(?:e|\(e\))?",
    r"assistant(?:e|\(e\))?<S>+de<S>+direction",
    r"secr[ée]taire<S>+g[ée]n[ée]ral(?:e)?",
    r"ceo|cfo|cto|coo|cio",
    r"(?:(?:project|product|sales|general|account|marketing|program)<S>+)?manager",
    r"gerente",
];

/// Intitulés qui exigent un complément (« directeur financier », « chef de projet »).
const TITLES_NEEDS: &[&str] = &[
    r"(?:vice[- ])?directeur(?:/trice|\(trice\))?",
    r"directrice",
    r"director<S>+of",
    r"head<S>+of",
    r"director(?:/a|a)?",
    r"responsable",
    r"chef(?:fe)?",
];

/// Adjectifs de complément, pliés (minuscules, sans accents).
const FUNCTION_ADJECTIVES: &[&str] = &[
    "technique",
    "commercial",
    "commerciale",
    "financier",
    "financiere",
    "general",
    "generale",
    "adjoint",
    "adjointe",
    "administratif",
    "administrative",
    "juridique",
    "informatique",
    "marketing",
    "qualite",
    "operationnel",
    "operationnelle",
    "executif",
    "executive",
    "senior",
    "junior",
    "principal",
    "principale",
    "financiero",
    "financiera",
    "tecnico",
    "tecnica",
    "adjunto",
    "adjunta",
    "administrativo",
    "juridico",
    "comercial",
    "technical",
    "financial",
];
const FUNCTION_CONNECTORS: &[&str] = &["de", "du", "des", "d'", "of", "for", "en", "del", "para"];
const FUNCTION_ARTICLES: &[&str] = &[
    "la", "le", "les", "l'", "las", "los", "the", "el", "un", "une",
];
/// Mots qui terminent un intitulé.
const FUNCTION_STOPWORDS: &[&str] = &[
    "et", "ou", "and", "or", "a", "est", "sont", "ont", "qui", "que", "chez", "à", "a", "au",
    "aux", "pour", "dans", "sur", "par", "avec", "sans", "y", "o", "is", "at", "in", "on", "who",
    "was", "has", "dit", "sera", "etait",
];

// ---------------------------------------------------------------------------
// Expressions régulières
// ---------------------------------------------------------------------------

fn month_alternation() -> String {
    let mut names: Vec<&str> = MONTH_NAMES.iter().map(|(n, _)| *n).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    names.join("|")
}

const YEAR: &str = r"(?:19|20)\d{2}";

fn titles_pattern(titles: &[&str]) -> String {
    format!(r"(?i)\b(?:{})", titles.join("|"))
}

re!(
    DATE_NUM,
    r"\b(?P<d>\d{1,2})[/.\-](?P<m>\d{1,2})[/.\-](?P<y>(?:19|20)\d{2})"
);
re!(
    DATE_ISO,
    r"\b(?P<y>(?:19|20)\d{2})-(?P<m>\d{2})-(?P<d>\d{2})"
);
re!(
    DATE_TEXT,
    &format!(
        r"(?i)\b(?P<d>\d{{1,2}})(?:er|st|nd|rd|th)?(?:<S>+de)?<S>+(?P<m>{})(?:<S>+de)?<S>+(?P<y>{YEAR})",
        month_alternation()
    )
);
re!(
    DATE_EN,
    &format!(
        r"(?i)\b(?P<m>{})<S>+(?P<d>\d{{1,2}})(?:st|nd|rd|th)?,?<S>+(?P<y>{YEAR})",
        month_alternation()
    )
);
re!(
    DATE_MY,
    &format!(
        r"(?i)\b(?P<m>{})(?:<S>+de)?<S>+(?P<y>{YEAR})",
        month_alternation()
    )
);

re!(AGE_ANS, r"(?i)\b(?P<n>\d{1,3})<S>+(?:ans|a[ñn]os)\b");
re!(
    AGE_EN,
    r"(?i)(?:\baged<S>+(?P<n>\d{1,3})\b|\b(?P<m>\d{1,3})(?:<S>+|-)years?(?:<S>+|-)old\b)"
);
re!(
    AGE_EXCL,
    r"(?i)\b(?:il y a|depuis|pendant|dans|durant|since|for|hace|desde|durante|every|tous les|chaque|apr[eè]s|over)<S>*$"
);
re!(
    AGE_NEXT_EXCL,
    r"(?i)^<S>+(?:d['’]|de\b|du\b|des\b|apr[eè]s|plus<S>+tard|auparavant)"
);

const NUM: &str = r"\d+(?:[ \x{a0}\x{202f}.,]\d{3})*(?:[.,]\d{1,2})?";
const CUR_WORD: &str =
    r"(?i:eur(?:os?)?|xpf|f[ .\x{a0}\x{202f}]?cfp|fcfp|cfp|usd|gbp|chf|aud|nzd)\b";
re!(
    MONTANT_APRES,
    &format!(r"(?:{NUM})<S>?[kKM]?<S>?(?:€|\$|£|{CUR_WORD})")
);
re!(
    MONTANT_AVANT,
    &format!(r"(?:€|\$|£|(?i:eur|usd|gbp|chf|aud|nzd|xpf)\b)<S>?(?:{NUM})")
);

re!(FONCTION_NEEDS, &titles_pattern(TITLES_NEEDS));
re!(FONCTION_SOLO, &titles_pattern(TITLES_SOLO));

re!(
    MATRICULE,
    r"(?i)(?:\bmatricule|\bmat\.|\bn[°o]\.?<S>*(?:d['’]<S>*)?agent|\bn[°o]\.?<S>*(?:de<S>+)?dossier|\br[ée]f(?:\.|[ée]rence)?<S>+dossier|\bemployee<S>+(?:id|number|no\b\.?))(?:<S>+(?:agent|salari[ée]|employ[ée]))?[ \x{a0}\x{202f}:=#°]*(?P<v>[A-Za-z0-9](?:[A-Za-z0-9\-/_]{1,18}[A-Za-z0-9]))"
);

// ---------------------------------------------------------------------------
// Détecteur
// ---------------------------------------------------------------------------

/// Détecteur de quasi-identifiants (`name() == "quasi"`).
#[derive(Debug, Default, Clone, Copy)]
pub struct QuasiDetector;

impl QuasiDetector {
    pub fn new() -> Self {
        Self
    }
}

type Recognizer = fn(&str, &mut Out);

impl Detector for QuasiDetector {
    fn name(&self) -> &'static str {
        "quasi"
    }

    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        let text = block.text.as_str();
        let recognizers: [(EntityKind, Recognizer); 5] = [
            (EntityKind::Date, date),
            (EntityKind::Age, age),
            (EntityKind::Montant, montant),
            (EntityKind::Fonction, fonction),
            (EntityKind::Matricule, matricule),
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
// Date
// ---------------------------------------------------------------------------

fn month_number(s: &str) -> Option<u32> {
    if let Ok(n) = s.parse::<u32>() {
        return Some(n);
    }
    let lower = s.to_lowercase();
    MONTH_NAMES
        .iter()
        .find(|(name, _)| *name == lower)
        .map(|(_, m)| *m)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn date(text: &str, out: &mut Out) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for re in [&DATE_NUM, &DATE_ISO, &DATE_TEXT, &DATE_EN, &DATE_MY] {
        let Some(re) = LazyLock::force(re).as_ref() else {
            continue;
        };
        for caps in re.captures_iter(text) {
            let Some(whole) = caps.get(0) else {
                continue;
            };
            let (s, e) = (whole.start(), whole.end());
            let year = caps.name("y").and_then(|m| m.as_str().parse::<u32>().ok());
            let month = caps.name("m").and_then(|m| month_number(m.as_str()));
            let day = caps
                .name("d")
                .map_or(Some(1), |m| m.as_str().parse::<u32>().ok());
            let (Some(year), Some(month), Some(day)) = (year, month, day) else {
                continue;
            };
            if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
                continue;
            }
            if !edges_ok(text, s, e, GLUE) || overlaps(&taken, s, e) {
                continue;
            }
            // « mars 2023 » précédé d'un jour (« 29 février 2023 », invalide) : pas une date.
            if caps.name("d").is_none() && !strict_left_ok(text, s) {
                continue;
            }
            push(out, text, EntityKind::Date, s, e, CONF_DATE);
            taken.push((s, e));
        }
    }
}

// ---------------------------------------------------------------------------
// Âge
// ---------------------------------------------------------------------------

fn age(text: &str, out: &mut Out) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    let mut accept = |text: &str, s: usize, e: usize, n: &str, out: &mut Out| -> bool {
        let plausible = n.parse::<u32>().is_ok_and(|v| v <= MAX_AGE);
        let after_excluded = LazyLock::force(&AGE_NEXT_EXCL)
            .as_ref()
            .is_some_and(|re| re.is_match(text.get(e..).unwrap_or("")));
        if !plausible
            || overlaps(&taken, s, e)
            || !edges_ok(text, s, e, GLUE)
            || after_excluded
            || line_window_matches(text, s, WINDOW_AGE, &AGE_EXCL)
        {
            return false;
        }
        push(out, text, EntityKind::Age, s, e, CONF_AGE);
        taken.push((s, e));
        true
    };
    if let Some(re) = LazyLock::force(&AGE_ANS).as_ref() {
        for caps in re.captures_iter(text) {
            if let (Some(whole), Some(n)) = (caps.get(0), caps.name("n")) {
                accept(text, whole.start(), whole.end(), n.as_str(), out);
            }
        }
    }
    if let Some(re) = LazyLock::force(&AGE_EN).as_ref() {
        for caps in re.captures_iter(text) {
            let n = caps.name("n").or_else(|| caps.name("m"));
            if let (Some(whole), Some(n)) = (caps.get(0), n) {
                accept(text, whole.start(), whole.end(), n.as_str(), out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Montant
// ---------------------------------------------------------------------------

fn montant(text: &str, out: &mut Out) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for re in [&MONTANT_APRES, &MONTANT_AVANT] {
        scan(re, text, |s, e| {
            if !left_ok(text, s, GLUE) || !right_ok(text, e, &[]) || overlaps(&taken, s, e) {
                return false;
            }
            push(out, text, EntityKind::Montant, s, e, CONF_MONTANT);
            taken.push((s, e));
            true
        });
    }
}

// ---------------------------------------------------------------------------
// Fonction
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Complement {
    Start,
    AfterConnector,
    Words,
    Adjectives,
}

/// Fin de l'intitulé : `title_end` si aucun complément ne suit.
fn function_end(text: &str, title_end: usize, of_title: bool) -> usize {
    let mut state = if of_title {
        Complement::AfterConnector
    } else {
        Complement::Start
    };
    let mut first_after_of = of_title;
    let mut good = title_end;
    let mut pos = title_end;
    let mut words = 0;
    while words < MAX_FUNCTION_WORDS {
        let start = skip_spaces(text, pos);
        if start == pos {
            break;
        }
        let Some(rest) = text.get(start..) else {
            break;
        };
        let len: usize = rest
            .chars()
            .take_while(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’' | '/'))
            .map(char::len_utf8)
            .sum();
        let Some(token) = rest.get(..len).filter(|t| !t.is_empty()) else {
            break;
        };
        let end = start + len;
        let lower = fold(token);
        let punct_after = char_after(text, end).is_some_and(|c| !c.is_whitespace());
        let elided = lower.starts_with("d'") && lower.len() > 2;
        let connector = FUNCTION_CONNECTORS.contains(&lower.as_str()) || elided;
        let article = FUNCTION_ARTICLES.contains(&lower.as_str());
        let adjective = lower
            .split('/')
            .next()
            .is_some_and(|a| FUNCTION_ADJECTIVES.contains(&a));
        let stop = FUNCTION_STOPWORDS.contains(&lower.as_str());
        let capital = token.chars().next().is_some_and(char::is_uppercase);
        match state {
            Complement::Start | Complement::Adjectives => {
                if adjective {
                    good = end;
                    state = Complement::Adjectives;
                } else if connector {
                    if elided {
                        good = end;
                        state = Complement::Words;
                    } else {
                        state = Complement::AfterConnector;
                    }
                } else {
                    break;
                }
            }
            Complement::AfterConnector | Complement::Words => {
                if (connector && !elided) || article {
                    state = Complement::AfterConnector;
                } else if stop || (capital && !first_after_of) {
                    break;
                } else {
                    good = end;
                    state = Complement::Words;
                    first_after_of = false;
                }
            }
        }
        words += 1;
        pos = end;
        if punct_after {
            break;
        }
    }
    good
}

fn fonction(text: &str, out: &mut Out) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for (re, needs) in [(&FONCTION_NEEDS, true), (&FONCTION_SOLO, false)] {
        scan(re, text, |s, e| {
            if !right_ok(text, e, &[]) {
                return false;
            }
            let of_title = text
                .get(s..e)
                .is_some_and(|t| t.to_lowercase().ends_with("of"));
            let end = function_end(text, e, of_title);
            if (needs && end == e) || overlaps(&taken, s, end) {
                return false;
            }
            push(out, text, EntityKind::Fonction, s, end, CONF_FONCTION);
            taken.push((s, end));
            true
        });
    }
}

// ---------------------------------------------------------------------------
// Matricule
// ---------------------------------------------------------------------------

fn matricule(text: &str, out: &mut Out) {
    let Some(re) = LazyLock::force(&MATRICULE).as_ref() else {
        return;
    };
    for caps in re.captures_iter(text) {
        let (Some(whole), Some(v)) = (caps.get(0), caps.name("v")) else {
            continue;
        };
        let has_digit = v.as_str().chars().any(|c| c.is_ascii_digit());
        if has_digit && left_ok(text, whole.start(), &[]) && right_ok(text, v.end(), &[]) {
            push(
                out,
                text,
                EntityKind::Matricule,
                v.start(),
                v.end(),
                CONF_MATRICULE,
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

    fn renforce() -> DetectOptions {
        DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        }
    }

    fn block(text: &str) -> TextBlock {
        TextBlock {
            text: text.to_string(),
            ctx: BlockContext::new("test", BlockRole::Body),
        }
    }

    fn values(text: &str, kind: EntityKind) -> Vec<String> {
        QuasiDetector
            .detect(&block(text), &renforce())
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
    fn name_and_profile() {
        assert_eq!(QuasiDetector.name(), "quasi");
        let t = "né 12/03/2024, 42 ans, 3 €, gérant, matricule AB123";
        assert!(
            QuasiDetector
                .detect(&block(t), &DetectOptions::default())
                .is_empty(),
            "rien en profil Standard"
        );
        let mut opts = renforce();
        opts.disabled.insert(EntityKind::Age);
        let kinds: Vec<_> = QuasiDetector
            .detect(&block(t), &opts)
            .iter()
            .map(|d| d.kind)
            .collect();
        assert!(!kinds.contains(&EntityKind::Age));
        assert!(kinds.contains(&EntityKind::Date));
    }

    #[test]
    fn all_regexes_compile() {
        let all: &[&Re] = &[
            &DATE_NUM,
            &DATE_ISO,
            &DATE_TEXT,
            &DATE_EN,
            &DATE_MY,
            &AGE_ANS,
            &AGE_EN,
            &AGE_EXCL,
            &AGE_NEXT_EXCL,
            &MONTANT_APRES,
            &MONTANT_AVANT,
            &FONCTION_NEEDS,
            &FONCTION_SOLO,
            &MATRICULE,
        ];
        for (i, re) in all.iter().enumerate() {
            assert!(LazyLock::force(re).is_some(), "regex #{i} ne compile pas");
        }
    }

    #[test]
    fn date_valid() {
        let k = EntityKind::Date;
        for d in [
            "12/03/2024",
            "12-03-2024",
            "12.03.2024",
            "2024-03-12",
            "12 mars 2024",
            "1er mars 2024",
            "mars 2024",
            "March 12, 2024",
            "12 March 2024",
            "12 de marzo de 2024",
            "12\u{a0}mars\u{a0}2024",
            "29 février 2024",
            "1/3/2024",
            "décembre 2025",
        ] {
            assert_found(d, k, d);
            assert_found(&format!("le {d}."), k, d);
            assert_found(&format!("({d})"), k, d);
        }
        assert_found("Réunion le 14 mars 2026, salle 3", k, "14 mars 2026");
    }

    #[test]
    fn date_rejected() {
        let k = EntityKind::Date;
        for t in [
            "2024",
            "12/2024",
            "31/02/2024",
            "32/01/2024",
            "12/13/2024",
            "29 février 2023",
            "mars",
            "12 mars",
            "1.2.3.4",
            "112/03/2024",
            "12/03/20245",
            "version 3.4.2024.1",
        ] {
            assert_none(t, k);
        }
    }

    #[test]
    fn date_naiss_stays_date_naiss_after_merge() {
        use crate::{Pipeline, RuleDetector};
        let text = "Jean, né le 12/03/1984, arrive le 14 mars 2026.";
        let p = Pipeline::new(vec![Box::new(RuleDetector), Box::new(QuasiDetector)]);
        let out = p.detect(&block(text), &renforce());
        let got: Vec<_> = out
            .iter()
            .map(|d| (d.kind, &text[d.start..d.end]))
            .collect();
        assert_eq!(
            got,
            vec![
                (EntityKind::DateNaiss, "12/03/1984"),
                (EntityKind::Date, "14 mars 2026")
            ]
        );
        // Et sans le détecteur de règles, la même date reste une simple Date.
        let p = Pipeline::new(vec![Box::new(QuasiDetector)]);
        assert_eq!(
            p.detect(&block("12/03/1984"), &renforce())[0].kind,
            EntityKind::Date
        );
    }

    #[test]
    fn age_valid() {
        let k = EntityKind::Age;
        assert_found("42 ans", k, "42 ans");
        assert_found("Marie, 42 ans.", k, "42 ans");
        assert_found("âgé de 42 ans", k, "42 ans");
        assert_found("agée de 42\u{a0}ans", k, "42\u{a0}ans");
        assert_found("aged 42", k, "aged 42");
        assert_found("42 years old", k, "42 years old");
        assert_found("a 42-year-old man", k, "42-year-old");
        assert_found("42 años", k, "42 años");
        assert_found("(0 ans)", k, "0 ans");
        assert_found("120 ans", k, "120 ans");
        assert_found("Âge : 62 ans", k, "62 ans");
    }

    #[test]
    fn age_rejected() {
        let k = EntityKind::Age;
        assert_none("150 ans d'histoire", k);
        assert_none("121 ans", k);
        assert_none("30 ans d'expérience", k);
        assert_none("20 ans de service", k);
        assert_none("il y a 5 ans", k);
        assert_none("depuis 10 ans", k);
        assert_none("desde hace 10 años", k);
        assert_none("42ans", k);
        assert_none("42 ansible", k);
        assert_none("aged 130", k);
        assert_none("1.42 ans", k);
        assert_none("42", k);
    }

    #[test]
    fn montant_valid() {
        let k = EntityKind::Montant;
        for m in [
            "3 €",
            "1 250 000 XPF",
            "1 250,50 €",
            "€ 1,250.50",
            "12 k€",
            "3,5 M€",
            "1\u{a0}250\u{a0}000\u{a0}F\u{a0}CFP",
            "1\u{202f}250\u{a0}€",
            "$1,250",
            "100 USD",
            "£20",
            "CHF 50",
            "48.500,00 EUR",
            "12 500 000 XPF",
            "10 FCFP",
            "5 AUD",
            "2 NZD",
            "100 GBP",
            "7 euros",
            "1250000 XPF",
            "10 F CFP",
        ] {
            assert_found(m, k, m);
            assert_found(&format!("({m})."), k, m);
            assert_found(&format!("pour {m} net"), k, m);
        }
    }

    #[test]
    fn montant_rejected() {
        let k = EntityKind::Montant;
        for t in [
            "2024",
            "100",
            "100 mètres",
            "12 k",
            "EUR",
            "€",
            "3 euroland",
            "$",
        ] {
            assert_none(t, k);
        }
    }

    #[test]
    fn fonction_valid() {
        let k = EntityKind::Fonction;
        assert_found("Le gérant a signé.", k, "gérant");
        assert_found("gérante", k, "gérante");
        assert_found("responsable technique", k, "responsable technique");
        assert_found(
            "la directrice des ressources humaines.",
            k,
            "directrice des ressources humaines",
        );
        assert_found("Directeur général adjoint", k, "Directeur général adjoint");
        assert_found("directeur/trice financier", k, "directeur/trice financier");
        assert_found("chef de projet", k, "chef de projet");
        assert_found("le manager", k, "manager");
        assert_found("project manager", k, "project manager");
        assert_found("PDG", k, "PDG");
        assert_found("notre DAF, M. X", k, "DAF");
        assert_found("assistante de direction", k, "assistante de direction");
        assert_found("Head of Finance", k, "Head of Finance");
        assert_found(
            "Director of Operations and Strategy",
            k,
            "Director of Operations",
        );
        assert_found("CEO", k, "CEO");
        assert_found("director financiero", k, "director financiero");
        assert_found("gerente de ventas", k, "gerente de ventas");
        assert_found("ingénieur d'études", k, "ingénieur d'études");
        assert_found(
            "directeur de la stratégie digitale et plus",
            k,
            "directeur de la stratégie digitale",
        );
        assert_found(
            "responsable\u{a0}technique",
            k,
            "responsable\u{a0}technique",
        );
        assert_found("(consultante)", k, "consultante");
        assert_found("avocat(e) au barreau", k, "avocat(e)");
    }

    #[test]
    fn fonction_rejected() {
        let k = EntityKind::Fonction;
        for t in [
            "directeur",
            "Il est responsable.",
            "responsable a signé",
            "directeur de Nautilus",
            "gérants",
            "dgfip",
            "le chef est là",
            "cheffe",
            "pdgx",
        ] {
            assert_none(t, k);
        }
    }

    #[test]
    fn matricule_valid() {
        let k = EntityKind::Matricule;
        assert_found("matricule MAT-004517", k, "MAT-004517");
        assert_found("Matricule : AB123", k, "AB123");
        assert_found("mat. 4587.", k, "4587");
        assert_found("n° agent 4587", k, "4587");
        assert_found("N° de dossier 2024-0042", k, "2024-0042");
        assert_found("réf. dossier D-778", k, "D-778");
        assert_found("employee id E12345", k, "E12345");
        assert_found("matricule agent MAT-004517 ; autre", k, "MAT-004517");
        assert_found("(matricule\u{a0}: X99)", k, "X99");
        assert_found("Véhicule : x ; matricule agent MAT-004517", k, "MAT-004517");
    }

    #[test]
    fn matricule_rejected() {
        let k = EntityKind::Matricule;
        for t in [
            "matricule",
            "matricule inconnu",
            "matricule ab",
            "le matricule ne change pas",
            "MAT-004517",
            "matricules AB123",
        ] {
            assert_none(t, k);
        }
    }

    const FRAGMENTS: &[&str] = &[
        "12/03/2024",
        "mars 2024",
        "12 mars 2024",
        "42 ans",
        "aged 42",
        "3 €",
        "1 250,50 €",
        "€ 1,250.50",
        "12 k€",
        "gérant",
        "directeur de la communication",
        "head of",
        "responsable technique",
        "matricule",
        "MAT-004517",
        "n° agent",
        "né le",
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
        "/",
        "'",
        "’",
        "d'",
        "de",
        "30 ans d'expérience",
        "XPF",
        "F CFP",
        "March 12, 2024",
    ];

    fn check(text: &str, dets: &[Detection]) {
        for d in dets {
            assert!(d.start < d.end && d.end <= text.len(), "{d:?}");
            assert!(
                text.is_char_boundary(d.start) && text.is_char_boundary(d.end),
                "{d:?}"
            );
        }
    }

    proptest! {
        #[test]
        fn never_panics_on_any_string(s in "\\PC{0,300}") {
            check(&s, &QuasiDetector.detect(&block(&s), &renforce()));
        }

        #[test]
        fn never_panics_on_structured_text(parts in proptest::collection::vec(proptest::sample::select(FRAGMENTS.to_vec()), 0..25)) {
            let s = parts.concat();
            check(&s, &QuasiDetector.detect(&block(&s), &renforce()));
            let s = parts.join(" ");
            check(&s, &QuasiDetector.detect(&block(&s), &renforce()));
        }
    }
}
