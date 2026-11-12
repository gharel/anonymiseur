//! Particularités d'Excel : formules, échappements `_xHHHH_`, dates,
//! styles numériques, références structurées.

use crate::xml::{Item, Walker};
use anon_core::{BlockContext, BlockRole, FormatError, Replacement, TextTransform};
use std::collections::HashMap;
use std::ops::Range;

// ---------------------------------------------------------------------------
// Échappements `_xHHHH_`

fn x_escape_at(s: &str) -> Option<char> {
    let b = s.as_bytes();
    if b.len() < 7 || b.first() != Some(&b'_') || b.get(1) != Some(&b'x') || b.get(6) != Some(&b'_')
    {
        return None;
    }
    let hex = s.get(2..6)?;
    if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    char::from_u32(u32::from_str_radix(hex, 16).ok()?)
}

/// Décode les séquences `_xHHHH_` d'une chaîne Excel.
pub(crate) fn x_decode(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(rest) = s.get(i..) {
        let Some(c) = rest.chars().next() else {
            break;
        };
        if c == '_'
            && let Some(d) = x_escape_at(rest)
        {
            out.push(d);
            i += 7;
            continue;
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Ré-encode ce qui doit l'être : caractères de contrôle (sauf tabulation et
/// saut de ligne) et séquences littérales qui ressembleraient à un échappement.
pub(crate) fn x_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(rest) = s.get(i..) {
        let Some(c) = rest.chars().next() else {
            break;
        };
        if c == '_' && x_escape_at(rest).is_some() {
            out.push_str("_x005F_");
        } else if (c as u32) < 0x20 && c != '\t' && c != '\n' {
            out.push_str(&format!("_x{:04X}_", c as u32));
        } else {
            out.push(c);
        }
        i += c.len_utf8();
    }
    out
}

// ---------------------------------------------------------------------------
// Dates

/// Formats numériques intégrés traités comme des dates.
fn builtin_is_date(id: u32) -> bool {
    (14..=22).contains(&id) || (45..=47).contains(&id)
}

/// Format personnalisé de date : contient j/d, m, a/y une fois retirés les
/// textes entre guillemets, sections entre crochets et caractères échappés.
pub(crate) fn custom_is_date(code: &str) -> bool {
    let mut clean = String::new();
    let mut chars = code.chars();
    let mut in_quote = false;
    let mut in_bracket = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '[' if !in_quote => in_bracket = true,
            ']' if !in_quote => in_bracket = false,
            '\\' if !in_quote => {
                chars.next();
            }
            _ if in_quote || in_bracket => {}
            _ => clean.push(c.to_ascii_lowercase()),
        }
    }
    let clean = clean.replace("am/pm", "").replace("a/p", "");
    // On ne regarde que la première section (nombres positifs).
    let first = clean.split(';').next().unwrap_or("");
    let has = |c: char| first.contains(c);
    has('d') || has('j') || has('y') || has('a') || (has('m') && !has('h') && !has('s'))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Dernier numéro de série valide (31/12/9999).
const MAX_SERIAL: f64 = 2_958_465.0;

/// Numéro de série Excel → `JJ/MM/AAAA`. `None` hors plage.
pub(crate) fn serial_to_date(raw: &str, date1904: bool) -> Option<String> {
    let v: f64 = raw.trim().parse().ok()?;
    if !v.is_finite() || !(0.0..=MAX_SERIAL).contains(&v) {
        return None;
    }
    let days = v.floor() as i64;
    let z = if date1904 {
        days_from_civil(1904, 1, 1) + days
    } else {
        if days < 1 {
            return None;
        }
        if days == 60 {
            return Some("29/02/1900".to_string());
        }
        let base = if days < 60 {
            days_from_civil(1899, 12, 31)
        } else {
            days_from_civil(1899, 12, 30)
        };
        base + days
    };
    let (y, m, d) = civil_from_days(z);
    Some(format!("{d:02}/{m:02}/{y:04}"))
}

/// Date `JJ/MM/AAAA` valide : (année, mois, jour).
pub(crate) fn parse_fr_date(s: &str) -> Option<(i64, i64, i64)> {
    let mut it = s.split('/');
    let (d, m, y) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || d.len() != 2 || m.len() != 2 || y.len() != 4 {
        return None;
    }
    let all_digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(d) || !all_digits(m) || !all_digits(y) {
        return None;
    }
    let (d, m, y): (i64, i64, i64) = (d.parse().ok()?, m.parse().ok()?, y.parse().ok()?);
    if !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    // Contrôle : la date existe (aller-retour par le calendrier).
    let leap_1900 = y == 1900 && m == 2 && d == 29;
    if !leap_1900 && civil_from_days(days_from_civil(y, m, d)) != (y, m, d) {
        return None;
    }
    Some((y, m, d))
}

/// `JJ/MM/AAAA` → numéro de série Excel (inverse de [`serial_to_date`]).
pub(crate) fn date_to_serial(s: &str, date1904: bool) -> Option<String> {
    let (y, m, d) = parse_fr_date(s)?;
    let serial = if date1904 {
        days_from_civil(y, m, d) - days_from_civil(1904, 1, 1)
    } else if (y, m, d) == (1900, 2, 29) {
        60
    } else {
        let z = days_from_civil(y, m, d);
        let s = z - days_from_civil(1899, 12, 30);
        if s < 61 {
            z - days_from_civil(1899, 12, 31)
        } else {
            s
        }
    };
    (serial >= 0 && serial as f64 <= MAX_SERIAL).then(|| serial.to_string())
}

/// Nombre écrit sous forme canonique : `^-?(0|[1-9]\d*)(\.\d+)?$`.
pub(crate) fn is_canonical_number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    digits(int) && (int == "0" || !int.starts_with('0')) && frac.is_none_or(digits)
}

/// Lettres de colonne (0 → `A`, 26 → `AA`).
pub(crate) fn col_letters(mut n: u64) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Pour chaque style de cellule (`cellXfs`), vrai s'il s'agit d'un format de date.
pub(crate) fn date_styles(styles_xml: &str) -> Result<Vec<bool>, FormatError> {
    let mut custom: HashMap<u32, String> = HashMap::new();
    let mut xfs: Vec<u32> = Vec::new();
    let mut w = Walker::new(styles_xml, "xl/styles.xml");
    while let Some(item) = w.next_item()? {
        let (Item::Start(t) | Item::Empty(t)) = item else {
            continue;
        };
        let parent = parent_of(w.stack(), t.empty);
        if t.name == "x:numFmt" {
            if let (Some(id), Some(code)) = (t.attr_value("numFmtId"), t.attr_value("formatCode"))
                && let Ok(id) = id.parse()
            {
                custom.insert(id, code.to_string());
            }
        } else if t.name == "x:xf" && parent == Some("x:cellXfs") {
            xfs.push(
                t.attr_value("numFmtId")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            );
        }
    }
    Ok(xfs
        .into_iter()
        .map(|id| match custom.get(&id) {
            Some(code) => custom_is_date(code),
            None => builtin_is_date(id),
        })
        .collect())
}

/// Parent d'une balise qui vient d'être lue (la pile contient la balise si
/// elle est ouvrante, pas si elle est vide).
pub(crate) fn parent_of(stack: &[String], empty: bool) -> Option<&str> {
    let n = if empty { 1 } else { 2 };
    stack
        .len()
        .checked_sub(n)
        .and_then(|i| stack.get(i))
        .map(|s| s.as_str())
}

/// Drapeau `date1904` du classeur.
pub(crate) fn workbook_date1904(workbook_xml: &str) -> Result<bool, FormatError> {
    let mut w = Walker::new(workbook_xml, "xl/workbook.xml");
    while let Some(item) = w.next_item()? {
        if let Item::Start(t) | Item::Empty(t) = item
            && t.name == "x:workbookPr"
        {
            return Ok(matches!(t.attr_value("date1904"), Some("1" | "true")));
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Formules

/// Littéral de chaîne d'une formule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Literal {
    /// Intervalle du contenu brut (entre les guillemets).
    pub raw: Range<usize>,
    /// Contenu, `""` ramené à `"`.
    pub text: String,
}

/// Groupe de nom de colonne dans une référence structurée.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnRef {
    raw: Range<usize>,
    name: String,
}

#[derive(Debug, Default)]
pub(crate) struct FormulaParts {
    pub literals: Vec<Literal>,
    columns: Vec<ColumnRef>,
}

/// Découpe une formule : littéraux de chaîne et noms de colonnes des
/// références structurées. Les noms de feuille entre apostrophes sont sautés.
pub(crate) fn parse_formula(f: &str) -> FormulaParts {
    let b = f.as_bytes();
    let mut parts = FormulaParts::default();
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        match c {
            b'"' => {
                let start = i + 1;
                let mut j = start;
                let mut text = Vec::new();
                loop {
                    match b.get(j) {
                        None => break,
                        Some(b'"') if b.get(j + 1) == Some(&b'"') => {
                            text.push(b'"');
                            j += 2;
                        }
                        Some(b'"') => break,
                        Some(&x) => {
                            text.push(x);
                            j += 1;
                        }
                    }
                }
                if let Ok(text) = String::from_utf8(text) {
                    parts.literals.push(Literal {
                        raw: start..j,
                        text,
                    });
                }
                i = j + 1;
            }
            b'\'' => {
                let mut j = i + 1;
                loop {
                    match b.get(j) {
                        None => break,
                        Some(b'\'') if b.get(j + 1) == Some(&b'\'') => j += 2,
                        Some(b'\'') => break,
                        Some(_) => j += 1,
                    }
                }
                i = j + 1;
            }
            b'[' => {
                i = bracket_group(f, i, &mut parts.columns);
            }
            _ => i += 1,
        }
    }
    parts
}

/// Lit un groupe `[...]` commençant en `open` ; renvoie la position après `]`.
fn bracket_group(f: &str, open: usize, out: &mut Vec<ColumnRef>) -> usize {
    let b = f.as_bytes();
    let mut j = open + 1;
    let mut nested = false;
    while let Some(&c) = b.get(j) {
        match c {
            b'\'' => j += 2,
            b'[' => {
                nested = true;
                j = bracket_group(f, j, out);
            }
            b']' => break,
            _ => j += 1,
        }
    }
    let end = j.min(b.len());
    if !nested {
        let mut start = open + 1;
        if b.get(start) == Some(&b'@') {
            start += 1;
        }
        if let Some(raw) = f.get(start..end)
            && !raw.starts_with('#')
            && !raw.is_empty()
        {
            out.push(ColumnRef {
                raw: start..end,
                name: unescape_struct(raw),
            });
        }
    }
    end + 1
}

fn unescape_struct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\'' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Échappe un nom de colonne pour une référence structurée.
pub(crate) fn escape_struct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '[' | ']' | '#' | '\'') {
            out.push('\'');
        }
        out.push(c);
    }
    out
}

/// Remplacements à appliquer à une formule : littéraux passés par `tr`,
/// noms de colonnes renommés selon `columns`. Renvoie aussi le nombre de
/// remplacements de texte effectués.
pub(crate) fn formula_replacements(
    formula: &str,
    tr: &dyn TextTransform,
    ctx: &BlockContext,
    columns: &HashMap<String, String>,
) -> (Vec<Replacement>, usize) {
    let parts = parse_formula(formula);
    let mut reps = Vec::new();
    let mut count = 0usize;
    let lctx = BlockContext {
        role: BlockRole::Formula,
        ..ctx.clone()
    };
    for lit in &parts.literals {
        let r = tr.replacements(&lit.text, &lctx);
        if r.is_empty() {
            continue;
        }
        count += r.len();
        let new = anon_core::text::apply_replacements(&lit.text, &r);
        reps.push(Replacement {
            start: lit.raw.start,
            end: lit.raw.end,
            with: new.replace('"', "\"\""),
        });
    }
    for col in &parts.columns {
        if let Some(new) = columns.get(&col.name) {
            reps.push(Replacement {
                start: col.raw.start,
                end: col.raw.end,
                with: escape_struct(new),
            });
        }
    }
    reps.sort_by_key(|r| r.start);
    (reps, count)
}

/// Colonne (lettres) et ligne d'une référence `B12`.
pub(crate) fn split_ref(r: &str) -> Option<(String, u32)> {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row = r.get(letters.len()..)?.parse().ok()?;
    if letters.is_empty() {
        return None;
    }
    Some((letters.to_ascii_uppercase(), row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_escapes_round_trip() {
        assert_eq!(x_decode("a_x000D_b_x005F_x0041_"), "a\rb_x0041_");
        assert_eq!(x_encode("a\rb_x0041_"), "a_x000D_b_x005F_x0041_");
        assert_eq!(x_decode("_x12_"), "_x12_");
    }

    #[test]
    fn dates() {
        assert_eq!(serial_to_date("45730", false).unwrap(), "14/03/2025");
        assert_eq!(serial_to_date("1", false).unwrap(), "01/01/1900");
        assert_eq!(serial_to_date("60", false).unwrap(), "29/02/1900");
        assert_eq!(serial_to_date("61", false).unwrap(), "01/03/1900");
        assert_eq!(serial_to_date("0", true).unwrap(), "01/01/1904");
        assert_eq!(serial_to_date("44268.75", true).unwrap(), "14/03/2025");
        assert!(serial_to_date("abc", false).is_none());
        assert!(serial_to_date("-1", false).is_none());
        assert!(custom_is_date("dd/mm/yyyy"));
        assert!(custom_is_date("jj/mm/aaaa"));
        assert!(custom_is_date("[$-40C]d mmmm yyyy"));
        assert!(!custom_is_date("0.00\" jours\""));
        assert!(!custom_is_date("h:mm"));
        assert!(!custom_is_date("#,##0"));
        for s in [
            "14/03/2025",
            "01/01/1900",
            "29/02/1900",
            "01/03/1900",
            "31/12/9999",
        ] {
            let n = date_to_serial(s, false).unwrap();
            assert_eq!(serial_to_date(&n, false).unwrap(), s);
            let n = date_to_serial(s, true);
            if let Some(n) = n {
                assert_eq!(serial_to_date(&n, true).unwrap(), s);
            }
        }
        assert_eq!(date_to_serial("14/03/2025", false).unwrap(), "45730");
        assert!(date_to_serial("31/02/2025", false).is_none());
        assert!(date_to_serial("3 mars", false).is_none());
    }

    #[test]
    fn canonical_numbers_and_columns() {
        for ok in ["0", "-12", "1250000", "3.5", "0.25"] {
            assert!(is_canonical_number(ok), "{ok}");
        }
        for ko in ["", "012", "1.", ".5", "1e5", "06 12", "+3", "1,5"] {
            assert!(!is_canonical_number(ko), "{ko}");
        }
        assert_eq!(col_letters(0), "A");
        assert_eq!(col_letters(25), "Z");
        assert_eq!(col_letters(26), "AA");
        assert_eq!(col_letters(701), "ZZ");
        assert_eq!(col_letters(702), "AAA");
    }

    #[test]
    fn formula_parts() {
        let f = r#"CONCATENATE("Dupont ""J""",'Feuille "x"'!A1,Tbl[[#This Row],[Nom client]],[@Col],T2[a'[b])"#;
        let p = parse_formula(f);
        let lits: Vec<_> = p.literals.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lits, vec!["Dupont \"J\""]);
        let cols: Vec<_> = p.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(cols, vec!["Nom client", "Col", "a[b"]);
        assert_eq!(&f[p.columns[0].raw.clone()], "Nom client");
    }

    #[test]
    fn struct_escape() {
        assert_eq!(escape_struct("[NOM_1]"), "'[NOM_1']");
        assert_eq!(unescape_struct("'[NOM_1']"), "[NOM_1]");
    }
}
