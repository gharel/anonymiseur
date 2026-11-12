//! Texte brut : `txt`, `md`, `csv`, `json`, `log`.
//!
//! Encodage détecté : UTF-8 (avec ou sans BOM), UTF-16 avec BOM, sinon
//! Windows-1252. La sortie garde l'encodage, le BOM et les fins de ligne.
//! Un bloc par ligne (pour un CSV, la ligne entière).

use crate::xml::{Patch, apply_patches};
use anon_core::{
    BlockContext, BlockRole, Extraction, FormatError, Handler, Output, TextBlock, TextTransform,
    TransformOptions,
};

/// Texte brut.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextHandler;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8 { bom: bool },
    Utf16Le,
    Utf16Be,
    Windows1252,
}

/// Caractères Windows-1252 des octets 0x80 à 0x9F (les octets non définis
/// correspondent aux caractères de contrôle C1 de même valeur).
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
];

fn decode(input: &[u8]) -> Result<(String, Encoding), FormatError> {
    if let Some(rest) = input.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let s = std::str::from_utf8(rest)
            .map_err(|_| FormatError::corrupt("texte", "UTF-8 invalide"))?;
        return Ok((s.to_string(), Encoding::Utf8 { bom: true }));
    }
    if let Some(rest) = input.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((utf16(rest, u16::from_le_bytes)?, Encoding::Utf16Le));
    }
    if let Some(rest) = input.strip_prefix(&[0xFE, 0xFF]) {
        return Ok((utf16(rest, u16::from_be_bytes)?, Encoding::Utf16Be));
    }
    if let Ok(s) = std::str::from_utf8(input) {
        return Ok((s.to_string(), Encoding::Utf8 { bom: false }));
    }
    let s = input
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP1252_HIGH
                .get(usize::from(b - 0x80))
                .copied()
                .unwrap_or('\u{FFFD}'),
            _ => char::from(b),
        })
        .collect();
    Ok((s, Encoding::Windows1252))
}

fn utf16(bytes: &[u8], f: fn([u8; 2]) -> u16) -> Result<String, FormatError> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(FormatError::corrupt("texte", "UTF-16 tronqué"));
    }
    let units = pairs.iter().map(|p| f(*p));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| FormatError::corrupt("texte", "UTF-16 invalide"))
}

fn encode(s: &str, enc: Encoding) -> Result<Vec<u8>, FormatError> {
    Ok(match enc {
        Encoding::Utf8 { bom } => {
            let mut v = if bom {
                vec![0xEF, 0xBB, 0xBF]
            } else {
                Vec::new()
            };
            v.extend_from_slice(s.as_bytes());
            v
        }
        Encoding::Utf16Le => {
            let mut v = vec![0xFF, 0xFE];
            for u in s.encode_utf16() {
                v.extend_from_slice(&u.to_le_bytes());
            }
            v
        }
        Encoding::Utf16Be => {
            let mut v = vec![0xFE, 0xFF];
            for u in s.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            v
        }
        Encoding::Windows1252 => {
            let mut v = Vec::with_capacity(s.len());
            for c in s.chars() {
                let code = c as u32;
                if code < 0x80 || (0xA0..=0xFF).contains(&code) {
                    v.push(code as u8);
                } else if let Some(i) = CP1252_HIGH.iter().position(|&h| h == c) {
                    v.push(0x80 + i as u8);
                } else {
                    return Err(FormatError::Other(
                        "caractère non représentable dans l'encodage du fichier".into(),
                    ));
                }
            }
            v
        }
    })
}

/// Découpe en lignes : (contenu, fin de ligne d'origine).
fn lines(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        if c == b'\n' || c == b'\r' {
            let eol_len = if c == b'\r' && b.get(i + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            out.push((
                s.get(start..i).unwrap_or(""),
                s.get(i..i + eol_len).unwrap_or(""),
            ));
            i += eol_len;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < s.len() {
        out.push((s.get(start..).unwrap_or(""), ""));
    }
    out
}

fn ctx(n: usize) -> BlockContext {
    BlockContext::new(format!("ligne {}", n + 1), BlockRole::Body)
}

fn extract_lines(s: &str) -> Vec<TextBlock> {
    lines(s)
        .into_iter()
        .enumerate()
        .filter(|(_, (l, _))| !l.trim().is_empty())
        .map(|(n, (l, _))| TextBlock {
            text: l.to_string(),
            ctx: ctx(n),
        })
        .collect()
}

fn transform_lines(s: &str, tr: &dyn TextTransform) -> (String, usize) {
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    for (n, (line, eol)) in lines(s).into_iter().enumerate() {
        let reps = tr.replacements(line, &ctx(n));
        count += reps.len();
        out.push_str(&anon_core::text::apply_replacements(line, &reps));
        out.push_str(eol);
    }
    (out, count)
}

fn finish(input: &[u8], out: &str, count: usize, enc: Encoding) -> Result<Output, FormatError> {
    let bytes = if count == 0 {
        input.to_vec()
    } else {
        encode(out, enc)?
    };
    Ok(Output {
        bytes,
        replacements: count,
        warnings: Vec::new(),
    })
}

impl Handler for TextHandler {
    fn extensions(&self) -> &'static [&'static str] {
        &["txt", "md", "json", "log"]
    }

    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        let (s, _) = decode(input)?;
        Ok(Extraction {
            blocks: extract_lines(&s),
            ..Default::default()
        })
    }

    fn transform(
        &self,
        input: &[u8],
        tr: &dyn TextTransform,
        _opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        let (s, enc) = decode(input)?;
        let (out, count) = transform_lines(&s, tr);
        finish(input, &out, count, enc)
    }
}

// ---------------------------------------------------------------------------
// CSV et TSV

/// CSV et TSV : un bloc par cellule, en-têtes de colonne renseignés.
#[derive(Debug, Clone, Copy, Default)]
pub struct CsvHandler;

/// Séparateurs essayés, par ordre de préférence à égalité.
const CSV_SEPARATORS: &[u8] = b";,\t";
/// Nombre d'enregistrements examinés pour détecter le séparateur.
const CSV_SAMPLE_RECORDS: usize = 20;
/// Part minimale (en %) d'enregistrements ayant autant de champs que l'en-tête.
const CSV_MIN_CONSISTENCY_PERCENT: usize = 80;

#[derive(Debug, Clone)]
struct Field {
    /// Intervalle brut du champ (guillemets compris).
    raw: std::ops::Range<usize>,
    value: String,
    quoted: bool,
}

/// Découpe en enregistrements ; les guillemets peuvent contenir séparateurs et
/// sauts de ligne, `""` vaut `"`. Au plus `max` enregistrements.
fn parse_csv(s: &str, sep: u8, max: usize) -> Vec<Vec<Field>> {
    let b = s.as_bytes();
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut i = 0usize;
    loop {
        let start = i;
        let mut value = Vec::new();
        let quoted = b.get(i) == Some(&b'"');
        if quoted {
            i += 1;
            loop {
                match b.get(i) {
                    None => break,
                    Some(b'"') if b.get(i + 1) == Some(&b'"') => {
                        value.push(b'"');
                        i += 2;
                    }
                    Some(b'"') => {
                        i += 1;
                        break;
                    }
                    Some(&c) => {
                        value.push(c);
                        i += 1;
                    }
                }
            }
        }
        while let Some(&c) = b.get(i) {
            if c == sep || c == b'\n' || c == b'\r' {
                break;
            }
            value.push(c);
            i += 1;
        }
        fields.push(Field {
            raw: start..i,
            value: String::from_utf8_lossy(&value).into_owned(),
            quoted,
        });
        match b.get(i) {
            Some(&c) if c == sep => i += 1,
            Some(&c) => {
                i += if c == b'\r' && b.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                records.push(std::mem::take(&mut fields));
                if i >= b.len() || records.len() >= max {
                    break;
                }
            }
            None => {
                records.push(std::mem::take(&mut fields));
                break;
            }
        }
    }
    records
}

fn is_blank(r: &[Field]) -> bool {
    r.iter().all(|f| f.value.trim().is_empty())
}

/// Séparateur d'un fichier en colonnes, `None` s'il n'a pas de structure en colonnes.
fn detect_separator(s: &str) -> Option<u8> {
    let mut best: Option<(usize, u8)> = None;
    for &sep in CSV_SEPARATORS {
        let recs: Vec<_> = parse_csv(s, sep, CSV_SAMPLE_RECORDS * 2)
            .into_iter()
            .filter(|r| !is_blank(r))
            .take(CSV_SAMPLE_RECORDS)
            .collect();
        let Some(n) = recs.first().map(Vec::len) else {
            continue;
        };
        if n < 2 {
            continue;
        }
        let same = recs.iter().filter(|r| r.len() == n).count();
        if same * 100 >= recs.len() * CSV_MIN_CONSISTENCY_PERCENT
            && best.is_none_or(|(bn, _)| n > bn)
        {
            best = Some((n, sep));
        }
    }
    best.map(|(_, sep)| sep)
}

fn cell_ctx(r: usize, c: usize, header: Option<&String>) -> BlockContext {
    BlockContext {
        part: format!("ligne {}, colonne {}", r + 1, c + 1),
        role: BlockRole::Cell,
        column_header: header.cloned(),
    }
}

fn encode_field(value: &str, sep: u8, quoted: bool) -> String {
    let needs = quoted
        || value
            .bytes()
            .any(|b| b == sep || b == b'"' || b == b'\n' || b == b'\r');
    if needs {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

impl Handler for CsvHandler {
    fn extensions(&self) -> &'static [&'static str] {
        &["csv", "tsv"]
    }

    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        let (s, _) = decode(input)?;
        let Some(sep) = detect_separator(&s) else {
            return Ok(Extraction {
                blocks: extract_lines(&s),
                ..Default::default()
            });
        };
        let recs = parse_csv(&s, sep, usize::MAX);
        let header: Vec<String> = recs
            .first()
            .map(|r| r.iter().map(|f| f.value.clone()).collect())
            .unwrap_or_default();
        let mut blocks = Vec::new();
        for (r, rec) in recs.iter().enumerate() {
            for (c, f) in rec.iter().enumerate() {
                if !f.value.trim().is_empty() {
                    blocks.push(TextBlock {
                        text: f.value.clone(),
                        ctx: cell_ctx(r, c, header.get(c)),
                    });
                }
            }
        }
        Ok(Extraction {
            blocks,
            ..Default::default()
        })
    }

    fn transform(
        &self,
        input: &[u8],
        tr: &dyn TextTransform,
        _opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        let (s, enc) = decode(input)?;
        let Some(sep) = detect_separator(&s) else {
            let (out, count) = transform_lines(&s, tr);
            return finish(input, &out, count, enc);
        };
        let recs = parse_csv(&s, sep, usize::MAX);
        let header: Vec<String> = recs
            .first()
            .map(|r| r.iter().map(|f| f.value.clone()).collect())
            .unwrap_or_default();
        let mut patches = Vec::new();
        let mut count = 0usize;
        for (r, rec) in recs.iter().enumerate() {
            for (c, f) in rec.iter().enumerate() {
                let reps = tr.replacements(&f.value, &cell_ctx(r, c, header.get(c)));
                if reps.is_empty() {
                    continue;
                }
                count += reps.len();
                let new = anon_core::text::apply_replacements(&f.value, &reps);
                patches.push(Patch::new(f.raw.clone(), encode_field(&new, sep, f.quoted)));
            }
        }
        let out = apply_patches(&s, patches);
        finish(input, &out, count, enc)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::Replacement;

    struct Upper;
    impl TextTransform for Upper {
        fn replacements(&self, text: &str, _: &BlockContext) -> Vec<Replacement> {
            text.match_indices("Kaloï")
                .map(|(i, m)| Replacement {
                    start: i,
                    end: i + m.len(),
                    with: "[NOM_1]".into(),
                })
                .collect()
        }
    }

    fn round(input: &[u8]) -> Vec<u8> {
        TextHandler
            .transform(input, &Upper, &TransformOptions::anonymize())
            .unwrap()
            .bytes
    }

    #[test]
    fn keeps_bom_and_line_endings() {
        let input = "\u{FEFF}a Kaloï\r\nb\rc Kaloï\n".as_bytes();
        let out = round(input);
        assert_eq!(out, "\u{FEFF}a [NOM_1]\r\nb\rc [NOM_1]\n".as_bytes());
    }

    #[test]
    fn utf16_and_cp1252() {
        let mut le = vec![0xFF, 0xFE];
        for u in "x Kaloï\n".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        let out = round(&le);
        let (s, enc) = decode(&out).unwrap();
        assert_eq!(enc, Encoding::Utf16Le);
        assert_eq!(s, "x [NOM_1]\n");

        let cp = b"\x80 Kalo\xEF\r\n".to_vec();
        let ex = TextHandler.extract(&cp).unwrap();
        assert_eq!(ex.blocks[0].text, "€ Kaloï");
        assert_eq!(round(&cp), b"\x80 [NOM_1]\r\n".to_vec());
    }

    #[test]
    fn one_block_per_line() {
        let ex = TextHandler.extract(b"Nom;Tel\nDupont;06\n\n").unwrap();
        assert_eq!(ex.blocks.len(), 2);
        assert_eq!(ex.blocks[1].text, "Dupont;06");
        assert_eq!(ex.blocks[1].ctx.part, "ligne 2");
    }

    #[test]
    fn untouched_is_identical() {
        let input = b"rien \xE9 changer\r\n";
        assert_eq!(round(input), input.to_vec());
    }

    /// Remplace chaque occurrence d'un texte par un autre.
    struct Swap(&'static str, &'static str);
    impl TextTransform for Swap {
        fn replacements(&self, text: &str, _: &BlockContext) -> Vec<Replacement> {
            text.match_indices(self.0)
                .map(|(i, m)| Replacement {
                    start: i,
                    end: i + m.len(),
                    with: self.1.into(),
                })
                .collect()
        }
    }

    #[test]
    fn csv_cells_with_headers_and_quotes() {
        let input = "\u{FEFF}Nom;Note\r\n\"Kaloï; H.\";\"dit \"\"ok\"\"\r\nsuite\"\r\nDupont;x\r\n";
        let ex = CsvHandler.extract(input.as_bytes()).unwrap();
        let t: Vec<_> = ex
            .blocks
            .iter()
            .map(|b| (b.text.as_str(), b.ctx.column_header.as_deref()))
            .collect();
        assert_eq!(
            t,
            vec![
                ("Nom", Some("Nom")),
                ("Note", Some("Note")),
                ("Kaloï; H.", Some("Nom")),
                ("dit \"ok\"\r\nsuite", Some("Note")),
                ("Dupont", Some("Nom")),
                ("x", Some("Note")),
            ]
        );
        assert!(ex.blocks.iter().all(|b| b.ctx.role == BlockRole::Cell));
        // Valeur sans guillemets qui reçoit un séparateur : remise entre guillemets.
        let out = CsvHandler
            .transform(
                input.as_bytes(),
                &Swap("Dupont", "D; \"P\""),
                &TransformOptions::restore(),
            )
            .unwrap();
        assert_eq!(
            String::from_utf8(out.bytes).unwrap(),
            "\u{FEFF}Nom;Note\r\n\"Kaloï; H.\";\"dit \"\"ok\"\"\r\nsuite\"\r\n\"D; \"\"P\"\"\";x\r\n"
        );
        let out = CsvHandler
            .transform(input.as_bytes(), &Upper, &TransformOptions::anonymize())
            .unwrap();
        assert!(
            String::from_utf8(out.bytes)
                .unwrap()
                .contains("\"[NOM_1]; H.\";")
        );
    }

    #[test]
    fn tsv_and_non_columnar() {
        let ex = CsvHandler.extract(b"a\tb\tc,d\n1\t2\t3,4\n").unwrap();
        assert_eq!(ex.blocks.len(), 6);
        assert_eq!(ex.blocks[5].text, "3,4");
        let ex = CsvHandler
            .extract("Une phrase sans colonnes.\nUne autre ligne.\n".as_bytes())
            .unwrap();
        assert_eq!(ex.blocks.len(), 2);
        assert_eq!(ex.blocks[0].ctx.role, BlockRole::Body);
    }
}
