//! Lecture XML en flux avec positions d'octets, noms canoniques résolus par
//! espace de noms, et réécriture par correctifs ponctuels.
//!
//! Principe : on ne re-sérialise jamais un document. Chaque événement est
//! repéré par son intervalle d'octets dans la source ; une transformation
//! produit une liste de [`Patch`] (remplacer tel intervalle par tel texte) et
//! tout le reste est recopié octet pour octet.

use crate::limits::MAX_XML_DEPTH;
use anon_core::FormatError;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::ops::Range;

/// Espaces de noms connus et préfixe canonique utilisé dans ce crate. Les
/// règles (`rules.rs`) désignent les éléments par ce préfixe, quel que soit
/// le préfixe réellement employé dans le fichier.
const KNOWN_NAMESPACES: &[(&str, &str)] = &[
    (
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        "w",
    ),
    ("http://purl.oclc.org/ooxml/wordprocessingml/main", "w"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://purl.oclc.org/ooxml/drawingml/main", "a"),
    (
        "http://schemas.openxmlformats.org/presentationml/2006/main",
        "p",
    ),
    ("http://purl.oclc.org/ooxml/presentationml/main", "p"),
    (
        "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
        "x",
    ),
    ("http://purl.oclc.org/ooxml/spreadsheetml/main", "x"),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/chart",
        "c",
    ),
    ("http://purl.oclc.org/ooxml/drawingml/chart", "c"),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        "wp",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
        "wp",
    ),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/picture",
        "pic",
    ),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing",
        "xdr",
    ),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/diagram",
        "dgm",
    ),
    (
        "http://schemas.microsoft.com/office/drawing/2008/diagram",
        "dsp",
    ),
    (
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        "r",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/relationships",
        "r",
    ),
    (
        "http://schemas.openxmlformats.org/package/2006/relationships",
        "rel",
    ),
    (
        "http://schemas.openxmlformats.org/package/2006/content-types",
        "ct",
    ),
    ("http://purl.org/dc/elements/1.1/", "dc"),
    ("http://purl.org/dc/terms/", "dcterms"),
    (
        "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
        "cp",
    ),
    (
        "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
        "ep",
    ),
    (
        "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
        "cust",
    ),
    (
        "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
        "vt",
    ),
    (
        "http://schemas.microsoft.com/office/word/2012/wordml",
        "w15",
    ),
    (
        "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main",
        "x14",
    ),
    ("http://schemas.microsoft.com/office/excel/2006/main", "xm"),
    (
        "http://schemas.microsoft.com/office/spreadsheetml/2010/11/ac",
        "x15ac",
    ),
    (
        "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments",
        "tc",
    ),
    (
        "http://schemas.microsoft.com/office/powerpoint/2018/8/main",
        "p188",
    ),
    ("http://www.w3.org/XML/1998/namespace", "xml"),
];

/// Attribut d'une balise ouvrante.
#[derive(Debug, Clone)]
pub(crate) struct Attr {
    /// Nom canonique (`w:author`, `name`…).
    pub name: String,
    /// Valeur décodée (entités résolues).
    pub value: String,
    /// Intervalle de la valeur brute (sans guillemets) dans la source.
    pub raw: Range<usize>,
}

/// Balise ouvrante ou vide.
#[derive(Debug, Clone)]
pub(crate) struct Tag {
    /// Nom canonique (`w:t`, `x:c`, `rel:Relationship`…).
    pub name: String,
    /// Préfixe réellement employé, deux-points compris (`"w:"` ou `""`).
    pub prefix: String,
    pub attrs: Vec<Attr>,
    pub span: Range<usize>,
    pub empty: bool,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name == name)
    }

    pub fn attr_value(&self, name: &str) -> Option<&str> {
        self.attr(name).map(|a| a.value.as_str())
    }

    /// Position où insérer un attribut : juste avant `>` ou `/>`.
    pub fn insert_pos(&self) -> usize {
        let back = if self.empty { 2 } else { 1 };
        self.span.end.saturating_sub(back)
    }
}

/// Élément de flux.
#[derive(Debug)]
pub(crate) enum Item {
    Start(Tag),
    Empty(Tag),
    End {
        name: String,
        span: Range<usize>,
    },
    /// Texte, référence d'entité ou section CDATA, décodé.
    Text {
        span: Range<usize>,
        text: String,
    },
    Other,
}

/// Lecteur en flux. Après `Item::Start`, la pile contient l'élément ouvert ;
/// après `Item::End`, elle ne le contient plus ; `Item::Empty` n'y entre pas.
pub(crate) struct Walker<'a> {
    src: &'a str,
    reader: Reader<&'a [u8]>,
    part: String,
    scopes: Vec<Vec<(String, String)>>,
    stack: Vec<String>,
}

fn to_usize(v: u64) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

impl<'a> Walker<'a> {
    pub fn new(src: &'a str, part: &str) -> Self {
        let reader = Reader::from_str(src);
        Self {
            src,
            reader,
            part: part.to_string(),
            scopes: Vec::new(),
            stack: Vec::new(),
        }
    }

    fn bad(&self) -> FormatError {
        FormatError::corrupt(self.part.clone(), "XML mal formé")
    }

    /// Éléments ouverts, du plus externe au plus interne.
    pub fn stack(&self) -> &[String] {
        &self.stack
    }

    pub fn next_item(&mut self) -> Result<Option<Item>, FormatError> {
        let before = to_usize(self.reader.buffer_position());
        let ev = self.reader.read_event().map_err(|_| self.bad())?;
        let after = to_usize(self.reader.buffer_position());
        let span = before..after;
        match ev {
            Event::Start(_) => {
                if self.stack.len() >= MAX_XML_DEPTH {
                    return Err(FormatError::corrupt(
                        self.part.clone(),
                        "imbrication excessive",
                    ));
                }
                let tag = self.parse_tag(span, false)?;
                self.stack.push(tag.name.clone());
                Ok(Some(Item::Start(tag)))
            }
            Event::Empty(_) => {
                let tag = self.parse_tag(span, true)?;
                self.scopes.pop();
                Ok(Some(Item::Empty(tag)))
            }
            Event::End(_) => {
                let name = self.stack.pop().ok_or_else(|| self.bad())?;
                self.scopes.pop();
                Ok(Some(Item::End { name, span }))
            }
            Event::Text(t) => Ok(Some(Item::Text {
                span,
                text: t.xml10_content().into_owned(),
            })),
            Event::CData(c) => Ok(Some(Item::Text {
                span,
                text: normalize_eol(&c.into_inner()),
            })),
            Event::GeneralRef(r) => {
                let name = r.into_inner();
                let text = resolve_entity(&name).unwrap_or_else(|| format!("&{name};"));
                Ok(Some(Item::Text { span, text }))
            }
            Event::Eof => {
                if self.stack.is_empty() {
                    Ok(None)
                } else {
                    Err(self.bad())
                }
            }
            _ => Ok(Some(Item::Other)),
        }
    }

    fn parse_tag(&mut self, span: Range<usize>, empty: bool) -> Result<Tag, FormatError> {
        let raw = self.src.get(span.clone()).ok_or_else(|| self.bad())?;
        let (qname, raw_attrs) = scan_tag(raw, span.start).ok_or_else(|| self.bad())?;
        let mut scope = Vec::new();
        for (k, _, v) in &raw_attrs {
            if *k == "xmlns" {
                scope.push((String::new(), decode_entities(v)));
            } else if let Some(p) = k.strip_prefix("xmlns:") {
                scope.push((p.to_string(), decode_entities(v)));
            }
        }
        self.scopes.push(scope);
        let name = self.canonical(qname, true);
        let prefix = match qname.split_once(':') {
            Some((p, _)) => format!("{p}:"),
            None => String::new(),
        };
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        for (k, start, v) in raw_attrs {
            if k == "xmlns" || k.starts_with("xmlns:") {
                continue;
            }
            attrs.push(Attr {
                name: self.canonical(k, false),
                value: normalize_attr(&decode_entities(v)),
                raw: start..start.saturating_add(v.len()),
            });
        }
        Ok(Tag {
            name,
            prefix,
            attrs,
            span,
            empty,
        })
    }

    fn lookup(&self, prefix: &str) -> Option<&str> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter())
            .find(|(p, _)| p == prefix)
            .map(|(_, u)| u.as_str())
    }

    /// Nom canonique : préfixe connu selon l'URI d'espace de noms.
    fn canonical(&self, qname: &str, element: bool) -> String {
        let (prefix, local) = match qname.split_once(':') {
            Some((p, l)) => (p, l),
            None if element => ("", qname),
            None => return qname.to_string(),
        };
        if prefix == "xml" {
            return format!("xml:{local}");
        }
        match self.lookup(prefix) {
            Some(uri) => match KNOWN_NAMESPACES.iter().find(|(u, _)| *u == uri) {
                Some((_, p)) => format!("{p}:{local}"),
                None => format!("{{{uri}}}{local}"),
            },
            None => qname.to_string(),
        }
    }
}

/// Attribut brut : nom qualifié, position absolue de la valeur, valeur brute.
type RawAttr<'r> = (&'r str, usize, &'r str);

/// Découpe une balise brute (`<nom a="v" …>` ou `<nom …/>`) : nom qualifié et,
/// pour chaque attribut, nom qualifié, position absolue de la valeur brute et
/// valeur brute. `None` si la balise est mal formée.
fn scan_tag(raw: &str, base: usize) -> Option<(&str, Vec<RawAttr<'_>>)> {
    let b = raw.as_bytes();
    let mut i = 1usize;
    let name_start = i;
    while let Some(&c) = b.get(i) {
        if c.is_ascii_whitespace() || c == b'/' || c == b'>' {
            break;
        }
        i += 1;
    }
    let name = raw.get(name_start..i)?;
    if name.is_empty() {
        return None;
    }
    let mut attrs = Vec::new();
    loop {
        while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
            i += 1;
        }
        match b.get(i) {
            None => return None,
            Some(b'>') | Some(b'/') => break,
            _ => {}
        }
        let ks = i;
        while let Some(&c) = b.get(i) {
            if c == b'=' || c.is_ascii_whitespace() || c == b'>' || c == b'/' {
                break;
            }
            i += 1;
        }
        let key = raw.get(ks..i)?;
        while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
            i += 1;
        }
        if b.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
            i += 1;
        }
        let q = *b.get(i)?;
        if q != b'"' && q != b'\'' {
            return None;
        }
        i += 1;
        let vs = i;
        loop {
            let c = *b.get(i)?;
            if c == q {
                break;
            }
            i += 1;
        }
        let val = raw.get(vs..i)?;
        i += 1;
        attrs.push((key, base.saturating_add(vs), val));
    }
    Some((name, attrs))
}

/// Entité prédéfinie ou référence de caractère.
fn resolve_entity(name: &str) -> Option<String> {
    let s = match name {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse::<u32>().ok()?,
            };
            if code == 0 {
                return None;
            }
            return char::from_u32(code).map(String::from);
        }
    };
    Some(s.to_string())
}

/// Décode les entités d'une chaîne brute ; une entité inconnue reste telle quelle.
pub(crate) fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(pos) = rest.find('&') {
        out.push_str(rest.get(..pos).unwrap_or(""));
        let after = rest.get(pos + 1..).unwrap_or("");
        match after.find(';') {
            Some(end) if end <= 32 => {
                let name = after.get(..end).unwrap_or("");
                match resolve_entity(name) {
                    Some(t) => out.push_str(&t),
                    None => {
                        out.push('&');
                        out.push_str(name);
                        out.push(';');
                    }
                }
                rest = after.get(end + 1..).unwrap_or("");
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn normalize_eol(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Normalisation XML d'une valeur d'attribut littérale.
fn normalize_attr(s: &str) -> String {
    if s.contains(['\t', '\n', '\r']) {
        s.replace("\r\n", " ").replace(['\t', '\n', '\r'], " ")
    } else {
        s.to_string()
    }
}

/// Échappe un texte d'élément.
pub(crate) fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// Échappe une valeur d'attribut (guillemets simples ou doubles).
pub(crate) fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// Remplacement d'un intervalle d'octets de la source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Patch {
    pub range: Range<usize>,
    pub with: String,
}

impl Patch {
    pub fn new(range: Range<usize>, with: impl Into<String>) -> Self {
        Self {
            range,
            with: with.into(),
        }
    }
}

/// Applique des correctifs ; ceux qui en chevauchent un précédent sont ignorés.
pub(crate) fn apply_patches(src: &str, mut patches: Vec<Patch>) -> String {
    patches.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(a.range.end.cmp(&b.range.end))
    });
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0usize;
    for p in patches {
        if p.range.start < cursor || p.range.end < p.range.start || p.range.end > src.len() {
            continue;
        }
        let Some(head) = src.get(cursor..p.range.start) else {
            continue;
        };
        out.push_str(head);
        out.push_str(&p.with);
        cursor = p.range.end;
    }
    out.push_str(src.get(cursor..).unwrap_or(""));
    out
}

/// Tous les textes et valeurs d'attributs d'une partie, décodés. Repli
/// grossier si le XML est illisible : texte hors balises et valeurs entre
/// guillemets.
pub(crate) fn all_strings(src: &str, part: &str) -> (Vec<String>, Vec<String>) {
    let mut texts = Vec::new();
    let mut attrs = Vec::new();
    let mut w = Walker::new(src, part);
    let mut ok = true;
    let mut current = String::new();
    loop {
        match w.next_item() {
            Ok(Some(Item::Text { text, .. })) => current.push_str(&text),
            Ok(Some(Item::Start(t))) | Ok(Some(Item::Empty(t))) => {
                flush(&mut current, &mut texts);
                for a in t.attrs {
                    if !a.value.trim().is_empty() {
                        attrs.push(a.value);
                    }
                }
            }
            Ok(Some(_)) => flush(&mut current, &mut texts),
            Ok(None) => break,
            Err(_) => {
                ok = false;
                break;
            }
        }
    }
    flush(&mut current, &mut texts);
    if !ok {
        texts.clear();
        attrs.clear();
        crude_strings(src, &mut texts, &mut attrs);
    }
    (texts, attrs)
}

fn flush(current: &mut String, out: &mut Vec<String>) {
    if !current.trim().is_empty() {
        out.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn crude_strings(src: &str, texts: &mut Vec<String>, attrs: &mut Vec<String>) {
    let mut in_tag = false;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for c in src.chars() {
        match (in_tag, quote) {
            (false, _) => {
                if c == '<' {
                    if !cur.trim().is_empty() {
                        texts.push(decode_entities(&cur));
                    }
                    cur.clear();
                    in_tag = true;
                } else {
                    cur.push(c);
                }
            }
            (true, None) => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                    cur.clear();
                } else if c == '>' {
                    in_tag = false;
                    cur.clear();
                }
            }
            (true, Some(q)) => {
                if c == q {
                    if !cur.trim().is_empty() {
                        attrs.push(decode_entities(&cur));
                    }
                    cur.clear();
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
        }
    }
    if !in_tag && !cur.trim().is_empty() {
        texts.push(decode_entities(&cur));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(src: &str) -> Vec<Item> {
        let mut w = Walker::new(src, "t.xml");
        let mut v = Vec::new();
        while let Some(i) = w.next_item().unwrap() {
            v.push(i);
        }
        v
    }

    #[test]
    fn spans_and_canonical_names() {
        let src = r#"<?xml version="1.0"?><ns0:doc xmlns:ns0="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><ns0:t a='1'>x &amp; &#233;</ns0:t><ns0:br/></ns0:doc>"#;
        let it = items(src);
        let mut names = Vec::new();
        let mut text = String::new();
        for i in &it {
            match i {
                Item::Start(t) | Item::Empty(t) => {
                    assert!(src[t.span.clone()].starts_with('<'));
                    names.push(t.name.clone());
                }
                Item::Text { span, text: t } => {
                    assert!(!src[span.clone()].contains('<'));
                    text.push_str(t);
                }
                _ => {}
            }
        }
        assert_eq!(names, vec!["w:doc", "w:t", "w:br"]);
        assert_eq!(text, "x & é");
    }

    #[test]
    fn attribute_ranges_point_to_raw_values() {
        let src = r#"<a b="x&amp;y" c = 'z'/>"#;
        let it = items(src);
        let Item::Empty(t) = &it[0] else { panic!() };
        assert_eq!(&src[t.attrs[0].raw.clone()], "x&amp;y");
        assert_eq!(t.attrs[0].value, "x&y");
        assert_eq!(&src[t.attrs[1].raw.clone()], "z");
        assert_eq!(t.insert_pos(), src.len() - 2);
    }

    #[test]
    fn malformed_is_error() {
        for bad in ["<a><b></a>", "<a>", "<a b=c/>", "<a></b>"] {
            let mut w = Walker::new(bad, "t.xml");
            let mut err = false;
            loop {
                match w.next_item() {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(_) => {
                        err = true;
                        break;
                    }
                }
            }
            assert!(err, "{bad}");
        }
    }

    #[test]
    fn patches_apply_in_order() {
        let p = vec![
            Patch::new(4..5, "Z"),
            Patch::new(0..1, "AA"),
            Patch::new(2..2, "+"),
        ];
        assert_eq!(apply_patches("abcdef", p), "AAb+cdZf");
    }

    #[test]
    fn crude_fallback_collects() {
        let (t, a) = all_strings("<a x=\"Jean\">Dupont<b></a>", "p");
        assert!(t.iter().any(|s| s.contains("Dupont")));
        assert!(a.iter().any(|s| s.contains("Jean")));
    }
}
