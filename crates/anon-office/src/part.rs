//! Traitement en flux d'une partie XML d'un paquet (OOXML ou OpenDocument) :
//! extraction des blocs ou calcul des correctifs à appliquer.

use crate::para::{Edits, PRESERVE_ATTR, Para};
use crate::rules::{
    COLUMN_NAME_ATTRIBUTES, ElementRule, FIELD_INSTRUCTION_ATTRIBUTES, NEUTRAL_TEMPLATE,
    NUMERIC_CACHE_ATTRIBUTES, NUMERIC_CACHE_POINTS, ODF_LINK_ATTRIBUTES, ODF_PARAGRAPH_ELEMENTS,
    ODF_SENDER_PREFIX, ODF_VALUE_ATTRIBUTES, ODF_VIRTUAL_ELEMENTS, PARAGRAPH_ELEMENTS,
    RUN_TEXT_ELEMENTS, SCRUBBED_ATTRIBUTES, TEXT_ATTRIBUTES, TEXT_ELEMENTS, VIRTUAL_ELEMENTS,
    VML_TEXT_ELEMENTS,
};
use crate::xlsx::{
    col_letters, date_to_serial, formula_replacements, is_canonical_number, parent_of,
    parse_formula, parse_fr_date, serial_to_date, x_decode, x_encode,
};
use crate::xml::{Attr, Item, Patch, Tag, Walker, escape_attr, escape_text};
use anon_core::text::apply_replacements;
use anon_core::{
    BlockContext, BlockRole, FormatError, Replacement, TextBlock, TextTransform, Warning,
    WarningCode,
};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Nombre maximal d'espaces rendues pour un `text:s`.
const MAX_ODF_SPACES: usize = 1024;

/// Opération en cours.
#[derive(Clone, Copy)]
pub(crate) enum Op<'a> {
    Extract,
    Transform {
        tr: &'a dyn TextTransform,
        scrub: bool,
        restore: bool,
    },
}

/// Contexte propre à un classeur Excel.
#[derive(Debug, Default)]
pub(crate) struct XlsxEnv {
    /// Pour chaque style de cellule, vrai si c'est un format de date.
    pub date_styles: Vec<bool>,
    pub date1904: bool,
    /// Chaînes partagées.
    pub shared: Vec<String>,
    /// Noms de colonnes de tableaux renommés (ancien → nouveau), transformation.
    pub columns: HashMap<String, String>,
}

/// Dialecte d'une partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    Word,
    Excel,
    PowerPoint,
    Odf,
    /// `customXml/item*.xml` : chaque nœud de texte est un bloc.
    CustomXml,
}

/// Contexte d'une partie.
pub(crate) struct PartCtx<'a> {
    /// Emplacement affiché dans les blocs (préfixé pour un fichier embarqué).
    pub label: String,
    /// Rôle des paragraphes ordinaires.
    pub role: BlockRole,
    pub dialect: Dialect,
    pub xlsx: &'a XlsxEnv,
    /// En-têtes de colonne par emplacement de cellule (transformation).
    pub headers: &'a HashMap<String, String>,
    pub op: Op<'a>,
}

/// Résultat du traitement d'une partie.
#[derive(Debug, Default)]
pub(crate) struct PartOut {
    pub blocks: Vec<TextBlock>,
    pub patches: Vec<Patch>,
    pub count: usize,
    pub warnings: Vec<Warning>,
    /// Chaînes partagées lues (`si`), dans l'ordre.
    pub shared: Vec<String>,
    /// Indices de chaînes partagées référencés par des cellules.
    pub used_shared: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text(BlockRole),
    Shared,
    CellValue,
    Formula,
    Scrub,
    NumCache,
}

/// Quels nœuds de texte appartiennent au paragraphe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    /// Texte directement dans l'élément.
    Own,
    /// Texte des éléments porteurs (`w:t`, `a:t`…).
    Holders,
    /// OpenDocument : texte du paragraphe et de ses éléments `text:`.
    Odf,
    /// Tout texte descendant (VML).
    All,
}

struct OpenPara {
    para: Para,
    kind: Kind,
    /// Profondeur de pile de l'élément paragraphe.
    depth: usize,
    start: usize,
    capture: Capture,
    /// Échappements `_xHHHH_` d'Excel.
    xesc: bool,
    /// Élément porteur ouvert : identifiant et profondeur.
    holder: Option<(usize, usize)>,
    name: String,
    prefix: String,
    /// Emplacement de cellule (OpenDocument).
    cell_label: Option<String>,
}

struct Cell {
    r: String,
    t: Option<String>,
    t_raw: Option<Range<usize>>,
    style: usize,
    insert_at: usize,
    has_formula: bool,
    depth: usize,
}

/// Cellule OpenDocument en cours.
struct OdfCell {
    label: String,
    depth: usize,
    value_type: Option<Attr>,
    calc_type: Option<Attr>,
    values: Vec<Attr>,
    /// Préfixe du nom des attributs `office:` (`office:`).
    office_prefix: String,
    insert_at: usize,
    value_hit: bool,
    changed: bool,
    texts: Vec<String>,
}

/// Position dans un tableau OpenDocument.
#[derive(Default, Clone)]
struct OdfTable {
    index: usize,
    row: u64,
    col: u64,
    /// Répétitions de la ligne et de la cellule ouvertes.
    row_rep: u64,
    cell_rep: u64,
    depth: usize,
}

struct State<'a, 'c> {
    cx: &'c PartCtx<'a>,
    out: PartOut,
    paras: Vec<OpenPara>,
    cell: Option<Cell>,
    odf_cell: Option<OdfCell>,
    tables: Vec<OdfTable>,
    table_count: usize,
    removing: Option<(usize, usize)>,
    seen_meta: HashSet<(String, String)>,
}

/// Traite une partie XML.
pub(crate) fn process(src: &str, cx: &PartCtx<'_>) -> Result<PartOut, FormatError> {
    let mut w = Walker::new(src, &cx.label);
    let mut st = State {
        cx,
        out: PartOut::default(),
        paras: Vec::new(),
        cell: None,
        odf_cell: None,
        tables: Vec::new(),
        table_count: 0,
        removing: None,
        seen_meta: HashSet::new(),
    };
    while let Some(item) = w.next_item()? {
        match item {
            Item::Start(tag) => st.on_tag(&tag, w.stack()),
            Item::Empty(tag) => st.on_tag(&tag, w.stack()),
            Item::End { name, span } => st.on_end(&name, span, w.stack()),
            Item::Text { span, text } => st.on_text(span, &text, w.stack()),
            Item::Other => {}
        }
    }
    Ok(st.out)
}

fn text_element(name: &str, parent: Option<&str>) -> Option<(ElementRule, bool)> {
    if name.starts_with(ODF_SENDER_PREFIX) {
        return Some((ElementRule::Scrub, false));
    }
    TEXT_ELEMENTS
        .iter()
        .find(|(n, p, _)| *n == name && (p.is_none() || *p == parent))
        .map(|(_, p, r)| (*r, p == &Some("vt:vector")))
}

fn local(name: &str) -> &str {
    name.rsplit([':', '}']).next().unwrap_or(name)
}

/// Lien vers l'extérieur : porte un schéma d'URI (`mailto:`, `https:`…).
fn is_external_uri(v: &str) -> bool {
    match v.split_once(':') {
        Some((scheme, _)) => {
            scheme.len() >= 2
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        }
        None => false,
    }
}

/// Littéraux entre guillemets d'une instruction de champ Word.
fn field_literals(instr: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in instr.char_indices() {
        if c == '"' {
            match start.take() {
                Some(s) => out.push(s..i),
                None => start = Some(i + 1),
            }
        }
    }
    out
}

/// Nombre de répétitions OpenDocument (`table:number-*-repeated`).
fn repeated(tag: &Tag, attr: &str) -> u64 {
    tag.attr_value(attr)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1)
        .max(1)
}

const ODF_NUMERIC_TYPES: &[&str] = &["float", "currency", "percentage", "date", "time", "boolean"];

impl State<'_, '_> {
    fn scrub(&self) -> bool {
        matches!(self.cx.op, Op::Transform { scrub: true, .. })
    }

    fn odf(&self) -> bool {
        self.cx.dialect == Dialect::Odf
    }

    fn ctx(&self, part: String, role: BlockRole) -> BlockContext {
        let column_header = self.cx.headers.get(&part).cloned();
        BlockContext {
            part,
            role,
            column_header,
        }
    }

    fn block(&mut self, part: String, role: BlockRole, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let ctx = self.ctx(part, role);
        self.out.blocks.push(TextBlock {
            text: text.to_string(),
            ctx,
        });
    }

    /// Bloc `Metadata` d'une valeur qui sera retirée (une seule fois par partie).
    fn meta_block(&mut self, what: &str, value: &str) {
        if value.trim().is_empty() || !self.seen_meta.insert((what.to_string(), value.to_string()))
        {
            return;
        }
        let part = format!("{}#{}", self.cx.label, what);
        self.block(part, BlockRole::Metadata, value);
    }

    fn patch(&mut self, range: Range<usize>, with: impl Into<String>) {
        self.out.patches.push(Patch::new(range, with));
    }

    fn warn(&mut self, code: WarningCode, detail: &str) {
        let w = Warning::new(code, self.cx.label.clone(), detail);
        if !self.out.warnings.contains(&w) {
            self.out.warnings.push(w);
        }
    }

    fn on_tag(&mut self, tag: &Tag, stack: &[String]) {
        let depth = if tag.empty {
            stack.len() + 1
        } else {
            stack.len()
        };
        let parent = parent_of(stack, tag.empty);
        self.on_attrs(tag);

        if self.scrub() && removable(tag) {
            if tag.empty {
                self.patch(tag.span.clone(), "");
            } else if self.removing.is_none() {
                self.removing = Some((tag.span.start, depth));
            }
        }

        if self.odf() {
            self.odf_table_tag(tag, depth);
        }

        if tag.name == "x:c" && !tag.empty {
            self.cell = Some(Cell {
                r: tag.attr_value("r").unwrap_or("").to_string(),
                t: tag.attr_value("t").map(str::to_string),
                t_raw: tag.attr("t").map(|a| a.raw.clone()),
                style: tag
                    .attr_value("s")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                insert_at: tag.insert_pos(),
                has_formula: false,
                depth,
            });
        }
        if tag.name == "x:f"
            && parent == Some("x:c")
            && let Some(c) = self.cell.as_mut()
        {
            c.has_formula = true;
        }

        if let Some((kind, capture)) = self.para_kind(&tag.name, parent, stack) {
            if !tag.empty {
                let xesc = tag.name.starts_with("x:") && kind != Kind::Formula;
                let cell_label = self.odf_cell.as_ref().map(|c| c.label.clone());
                self.paras.push(OpenPara {
                    para: Para::new(),
                    kind,
                    depth,
                    start: tag.span.start,
                    capture,
                    xesc,
                    holder: None,
                    name: tag.name.clone(),
                    prefix: tag.prefix.clone(),
                    cell_label,
                });
            }
            return;
        }

        let Some(top) = self.paras.last_mut() else {
            return;
        };
        match top.capture {
            Capture::Odf => {
                if let Some((_, text)) = ODF_VIRTUAL_ELEMENTS.iter().find(|(n, _)| *n == tag.name) {
                    let n = if tag.name == "text:s" {
                        tag.attr_value("text:c")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                            .clamp(1, MAX_ODF_SPACES)
                    } else {
                        1
                    };
                    top.para.push_virtual(&text.repeat(n));
                }
                return;
            }
            Capture::All => {
                if matches!(local(&tag.name), "br" | "div" | "p")
                    && !top.para.text.is_empty()
                    && !top.para.text.ends_with(char::is_whitespace)
                {
                    top.para.push_virtual("\n");
                }
                return;
            }
            Capture::Own | Capture::Holders => {}
        }
        if !tag.empty
            && let Some((_, _, preserve)) = RUN_TEXT_ELEMENTS
                .iter()
                .find(|(n, p, _)| *n == tag.name && (p.is_none() || *p == parent))
        {
            let fix = if *preserve {
                match tag.attr("xml:space") {
                    Some(a) if a.value == "preserve" => None,
                    Some(a) => Some(Patch::new(a.raw.clone(), "preserve")),
                    None => Some(Patch::new(
                        tag.insert_pos()..tag.insert_pos(),
                        PRESERVE_ATTR,
                    )),
                }
            } else {
                None
            };
            let id = top.para.add_holder(fix);
            top.holder = Some((id, depth));
            return;
        }
        if !tag.empty
            && REVISION_SEPARATED.contains(&tag.name.as_str())
            && !top.para.text.is_empty()
            && !top.para.text.ends_with(char::is_whitespace)
        {
            top.para.push_virtual(" ");
            return;
        }
        if let Some((_, _, text)) = VIRTUAL_ELEMENTS
            .iter()
            .find(|(n, p, _)| *n == tag.name && Some(*p) == parent)
        {
            top.para.push_virtual(text);
        }
    }

    /// Lignes et cellules des tableaux OpenDocument.
    fn odf_table_tag(&mut self, tag: &Tag, depth: usize) {
        match tag.name.as_str() {
            "table:table" if !tag.empty => {
                self.table_count += 1;
                self.tables.push(OdfTable {
                    index: self.table_count,
                    depth,
                    ..OdfTable::default()
                });
            }
            "table:table-row" => {
                if let Some(t) = self.tables.last_mut() {
                    t.col = 0;
                    let rep = repeated(tag, "table:number-rows-repeated");
                    if tag.empty {
                        t.row = t.row.saturating_add(rep);
                    } else {
                        t.row_rep = rep;
                    }
                }
            }
            "table:table-cell" | "table:covered-table-cell" => {
                let Some(t) = self.tables.last_mut() else {
                    return;
                };
                let label = format!(
                    "{}:feuille{}!{}{}",
                    self.cx.label,
                    t.index,
                    col_letters(t.col),
                    t.row.saturating_add(1)
                );
                let rep = repeated(tag, "table:number-columns-repeated");
                if tag.empty {
                    t.col = t.col.saturating_add(rep);
                    return;
                }
                t.cell_rep = rep;
                let values: Vec<Attr> = tag
                    .attrs
                    .iter()
                    .filter(|a| ODF_VALUE_ATTRIBUTES.contains(&a.name.as_str()))
                    .cloned()
                    .collect();
                let value_type = tag.attr("office:value-type").cloned();
                let office_prefix = value_type
                    .as_ref()
                    .and_then(|a| a.qname.split_once(':').map(|(p, _)| format!("{p}:")))
                    .unwrap_or_else(|| "office:".to_string());
                let mut value_hit = false;
                if let Op::Transform { tr, .. } = self.cx.op {
                    let ctx = self.ctx(label.clone(), BlockRole::Cell);
                    value_hit = values
                        .iter()
                        .any(|a| !tr.replacements(&a.value, &ctx).is_empty());
                }
                self.odf_cell = Some(OdfCell {
                    label,
                    depth,
                    value_type,
                    calc_type: tag.attr("calcext:value-type").cloned(),
                    values,
                    office_prefix,
                    insert_at: tag.insert_pos(),
                    value_hit,
                    changed: false,
                    texts: Vec::new(),
                });
            }
            _ => {}
        }
    }

    fn para_kind(
        &self,
        name: &str,
        parent: Option<&str>,
        stack: &[String],
    ) -> Option<(Kind, Capture)> {
        if self.cx.dialect == Dialect::CustomXml {
            return Some((Kind::Text(BlockRole::Other), Capture::Own));
        }
        if name == "x:v" && parent == Some("x:c") {
            return Some((Kind::CellValue, Capture::Own));
        }
        if name == "c:pt" && parent.is_some_and(|p| NUMERIC_CACHE_POINTS.contains(&p)) {
            return Some((Kind::NumCache, Capture::Holders));
        }
        if VML_TEXT_ELEMENTS.contains(&name) {
            return Some((Kind::Text(self.cx.role), Capture::All));
        }
        if self.odf() && ODF_PARAGRAPH_ELEMENTS.contains(&name) {
            return Some((Kind::Text(self.odf_role(stack)), Capture::Odf));
        }
        if PARAGRAPH_ELEMENTS
            .iter()
            .any(|(n, p)| *n == name && (p.is_none() || *p == parent))
        {
            let kind = match name {
                "x:si" => Kind::Shared,
                "x:is" | "x:rPh" => Kind::Text(BlockRole::Cell),
                "x:text" => Kind::Text(BlockRole::Comment),
                _ => Kind::Text(self.cx.role),
            };
            return Some((kind, Capture::Holders));
        }
        let (rule, titles) = text_element(name, parent)?;
        if titles
            && (self.cx.dialect == Dialect::Excel || !stack.iter().any(|s| s == "ep:TitlesOfParts"))
        {
            return None;
        }
        let kind = match rule {
            ElementRule::Text(role) => Kind::Text(role),
            ElementRule::Formula => Kind::Formula,
            ElementRule::Scrub => Kind::Scrub,
        };
        Some((kind, Capture::Own))
    }

    /// Rôle d'un paragraphe OpenDocument selon ses ancêtres.
    fn odf_role(&self, stack: &[String]) -> BlockRole {
        for s in stack.iter().rev() {
            match s.as_str() {
                "office:annotation" => return BlockRole::Comment,
                "text:note-body" | "presentation:notes" => return BlockRole::Note,
                "style:header" | "style:header-left" | "style:header-first" => {
                    return BlockRole::Header;
                }
                "style:footer" | "style:footer-left" | "style:footer-first" => {
                    return BlockRole::Footer;
                }
                "table:table-cell" => return BlockRole::Cell,
                _ => {}
            }
        }
        self.cx.role
    }

    fn on_text(&mut self, span: Range<usize>, text: &str, stack: &[String]) {
        let stack_len = stack.len();
        let Some(top) = self.paras.last_mut() else {
            return;
        };
        let holder = match top.capture {
            Capture::Own if stack_len == top.depth => None,
            Capture::All => None,
            Capture::Odf => {
                let parent = stack.last().map(String::as_str).unwrap_or("");
                let ok = stack_len == top.depth
                    || (parent.starts_with("text:") && parent != "text:note-citation");
                if !ok {
                    return;
                }
                None
            }
            Capture::Holders | Capture::Own => match top.holder {
                Some((id, d)) if d == stack_len => Some(id),
                _ => return,
            },
        };
        if top.xesc {
            top.para.push_text(&x_decode(text), span, holder);
        } else {
            top.para.push_text(text, span, holder);
        }
    }

    fn on_end(&mut self, name: &str, span: Range<usize>, stack: &[String]) {
        let ended = stack.len() + 1;
        if REVISION_SEPARATED.contains(&name)
            && let Some(top) = self.paras.last_mut()
        {
            top.para.push_virtual(" ");
        }
        if let Some(top) = self.paras.last_mut()
            && top.holder.is_some_and(|(_, d)| d == ended)
        {
            top.holder = None;
        }
        if self.paras.last().is_some_and(|p| p.depth == ended)
            && let Some(p) = self.paras.pop()
        {
            self.finish(p, span.end);
        }
        if self.cell.as_ref().is_some_and(|c| c.depth == ended) {
            self.cell = None;
        }
        if self.odf_cell.as_ref().is_some_and(|c| c.depth == ended)
            && let Some(c) = self.odf_cell.take()
        {
            self.finish_odf_cell(c);
        }
        if self.odf() {
            match name {
                "table:table-cell" | "table:covered-table-cell" => {
                    if let Some(t) = self.tables.last_mut() {
                        t.col = t.col.saturating_add(t.cell_rep.max(1));
                        t.cell_rep = 1;
                    }
                }
                "table:table-row" => {
                    if let Some(t) = self.tables.last_mut() {
                        t.row = t.row.saturating_add(t.row_rep.max(1));
                        t.row_rep = 1;
                    }
                }
                "table:table" if self.tables.last().is_some_and(|t| t.depth == ended) => {
                    self.tables.pop();
                }
                _ => {}
            }
        }
        if let Some((start, d)) = self.removing
            && d == ended
        {
            self.removing = None;
            self.patch(start..span.end, "");
        }
    }

    fn apply_edits(&mut self, p: &OpenPara, reps: &[Replacement]) -> usize {
        let xesc = p.xesc;
        let encode = move |s: &str| {
            if xesc {
                escape_text(&x_encode(s))
            } else {
                escape_text(s)
            }
        };
        let Edits { patches, applied } = p.para.edits(reps, &encode);
        self.out.patches.extend(patches);
        applied
    }

    fn finish(&mut self, p: OpenPara, end: usize) {
        let label = match (&self.cell, p.name.as_str(), &p.cell_label) {
            (_, _, Some(l)) => l.clone(),
            (Some(c), "x:is", _) => format!("{}!{}", self.cx.label, c.r),
            _ => self.cx.label.clone(),
        };
        match (p.kind, self.cx.op) {
            (Kind::Text(role), Op::Extract) => self.block(label, role, &p.para.text),
            (Kind::Shared, Op::Extract) => self.out.shared.push(p.para.text.clone()),
            (Kind::Text(role), Op::Transform { tr, restore, .. }) => {
                let reps = tr.replacements(&p.para.text, &self.ctx(label, role));
                if reps.is_empty() {
                    return;
                }
                if restore && p.name == "x:is" && self.restore_number(&p, &reps, end) {
                    return;
                }
                if let Some(c) = self.odf_cell.as_mut()
                    && p.cell_label.is_some()
                {
                    c.changed = true;
                    c.texts
                        .push(apply_replacements(&p.para.text, &reps).into_owned());
                }
                self.out.count += self.apply_edits(&p, &reps);
            }
            (Kind::Shared, Op::Transform { tr, .. }) => {
                let reps = tr.replacements(&p.para.text, &self.ctx(label, BlockRole::Cell));
                self.out.count += self.apply_edits(&p, &reps);
            }
            (Kind::Scrub, Op::Extract) => {
                let what = p.name.clone();
                self.meta_block(&what, &p.para.text);
            }
            (Kind::Scrub, Op::Transform { scrub, .. }) => {
                if scrub && !p.para.text.is_empty() {
                    let rep = Replacement {
                        start: 0,
                        end: p.para.text.len(),
                        with: String::new(),
                    };
                    self.apply_edits(&p, &[rep]);
                }
            }
            (Kind::Formula, Op::Extract) => {
                for lit in parse_formula(&p.para.text).literals {
                    self.block(label.clone(), BlockRole::Formula, &lit.text);
                }
            }
            (Kind::Formula, Op::Transform { tr, .. }) => {
                let ctx = self.ctx(label, BlockRole::Formula);
                let (reps, n) = formula_replacements(&p.para.text, tr, &ctx, &self.cx.xlsx.columns);
                if !reps.is_empty() {
                    self.apply_edits(&p, &reps);
                }
                self.out.count += n;
            }
            (Kind::NumCache, Op::Extract) => {}
            (Kind::NumCache, Op::Transform { tr, .. }) => {
                let text = p.para.text.trim().to_string();
                if !text.is_empty()
                    && text != "0"
                    && !tr
                        .replacements(&text, &self.ctx(label, BlockRole::Other))
                        .is_empty()
                {
                    let rep = Replacement {
                        start: 0,
                        end: p.para.text.len(),
                        with: "0".into(),
                    };
                    self.apply_edits(&p, &[rep]);
                    self.warn(
                        WarningCode::UntouchedLocation,
                        "Valeur numérique en cache contenant une valeur sensible, remplacée par 0.",
                    );
                }
            }
            (Kind::CellValue, _) => self.cell_value(p, end),
        }
    }

    /// Restauration Excel : une chaîne en ligne redevenue un nombre canonique
    /// (ou une date `JJ/MM/AAAA` dans une cellule au format date) reprend sa
    /// forme numérique. Vrai si la cellule a été convertie.
    fn restore_number(&mut self, p: &OpenPara, reps: &[Replacement], end: usize) -> bool {
        let Some(cell) = self.cell.as_ref() else {
            return false;
        };
        let new = apply_replacements(&p.para.text, reps).into_owned();
        let is_date = self
            .cx
            .xlsx
            .date_styles
            .get(cell.style)
            .copied()
            .unwrap_or(false);
        let number = if is_date {
            date_to_serial(&new, self.cx.xlsx.date1904)
        } else {
            None
        }
        .or_else(|| is_canonical_number(&new).then(|| new.clone()));
        let Some(number) = number else {
            return false;
        };
        let t_raw = cell.t_raw.clone();
        let pre = p.prefix.clone();
        self.patch(p.start..end, format!("<{pre}v>{number}</{pre}v>"));
        if let Some(r) = t_raw {
            self.patch(r, "n");
        }
        self.out.count += reps.len();
        true
    }

    /// Fin d'une cellule OpenDocument : changement de type si son texte a changé.
    fn finish_odf_cell(&mut self, c: OdfCell) {
        let Op::Transform { restore, .. } = self.cx.op else {
            return;
        };
        let vt = c
            .value_type
            .as_ref()
            .map(|a| a.value.as_str())
            .unwrap_or("");
        let set_type = |st: &mut Self, ty: &str| {
            if let Some(a) = &c.value_type {
                st.patch(a.raw.clone(), ty.to_string());
            }
            if let Some(a) = &c.calc_type {
                st.patch(a.raw.clone(), ty.to_string());
            }
        };
        if !restore && ODF_NUMERIC_TYPES.contains(&vt) && (c.changed || c.value_hit) {
            set_type(self, "string");
            for a in &c.values {
                self.patch(a.span.clone(), "");
            }
            return;
        }
        if restore && vt == "string" && c.changed && c.texts.len() == 1 {
            let text = c.texts.first().cloned().unwrap_or_default();
            let p = c.office_prefix.clone();
            if is_canonical_number(&text) {
                set_type(self, "float");
                self.patch(c.insert_at..c.insert_at, format!(" {p}value=\"{text}\""));
            } else if let Some((y, m, d)) = parse_fr_date(&text) {
                set_type(self, "date");
                self.patch(
                    c.insert_at..c.insert_at,
                    format!(" {p}date-value=\"{y:04}-{m:02}-{d:02}\""),
                );
            }
        }
    }

    fn cell_value(&mut self, p: OpenPara, end: usize) {
        let Some(cell) = self.cell.as_ref() else {
            return;
        };
        let label = format!("{}!{}", self.cx.label, cell.r);
        let t = cell.t.clone();
        match t.as_deref() {
            Some("s") => {
                if let (Op::Extract, Ok(idx)) = (self.cx.op, p.para.text.trim().parse::<usize>())
                    && let Some(s) = self.cx.xlsx.shared.get(idx)
                {
                    let s = s.clone();
                    self.block(label, BlockRole::Cell, &s);
                    self.out.used_shared.push(idx);
                }
            }
            Some("b" | "e") => {}
            Some("str" | "d" | "inlineStr") => match self.cx.op {
                Op::Extract => self.block(label, BlockRole::Cell, &p.para.text),
                Op::Transform { tr, .. } => {
                    let reps = tr.replacements(&p.para.text, &self.ctx(label, BlockRole::Cell));
                    self.out.count += self.apply_edits(&p, &reps);
                }
            },
            _ => {
                let raw = p.para.text.trim();
                let is_date = self
                    .cx
                    .xlsx
                    .date_styles
                    .get(cell.style)
                    .copied()
                    .unwrap_or(false);
                let display = if is_date {
                    serial_to_date(raw, self.cx.xlsx.date1904).unwrap_or_else(|| raw.to_string())
                } else {
                    raw.to_string()
                };
                match self.cx.op {
                    Op::Extract => self.block(label, BlockRole::Cell, &display),
                    Op::Transform { tr, .. } => {
                        let reps = tr.replacements(&display, &self.ctx(label, BlockRole::Cell));
                        if reps.is_empty() {
                            return;
                        }
                        let new = apply_replacements(&display, &reps).into_owned();
                        if new == display {
                            return;
                        }
                        self.out.count += reps.len();
                        let has_formula = cell.has_formula;
                        let t_raw = cell.t_raw.clone();
                        let insert_at = cell.insert_at;
                        if has_formula {
                            // Valeur en cache d'une formule : on la retire, Excel recalcule.
                            self.patch(p.start..end, "");
                        } else {
                            let pre = &p.prefix;
                            let body = escape_text(&x_encode(&new));
                            self.patch(
                                p.start..end,
                                format!(
                                    "<{pre}is><{pre}t xml:space=\"preserve\">{body}</{pre}t></{pre}is>"
                                ),
                            );
                            match t_raw {
                                Some(r) => self.patch(r, "inlineStr"),
                                None => self.patch(insert_at..insert_at, " t=\"inlineStr\""),
                            }
                        }
                    }
                }
            }
        }
    }

    /// Valeur d'attribut passée par `tr` (ou `apply_uri`) : bloc ou correctif.
    fn text_attr(&mut self, a: &Attr, role: BlockRole, uri: bool) {
        let label = self.cx.label.clone();
        match self.cx.op {
            Op::Extract => self.block(label, role, &a.value),
            Op::Transform { tr, .. } => {
                let new = if uri {
                    tr.apply_uri(&a.value).into_owned()
                } else {
                    let reps = tr.replacements(&a.value, &self.ctx(label, role));
                    if reps.is_empty() {
                        return;
                    }
                    apply_replacements(&a.value, &reps).into_owned()
                };
                if new != a.value {
                    self.out.count += 1;
                    self.patch(a.raw.clone(), escape_attr(&new));
                }
            }
        }
    }

    fn on_attrs(&mut self, tag: &Tag) {
        let op = self.cx.op;
        let scrub = self.scrub();
        let label = self.cx.label.clone();
        if tag.name == "rel:Relationship"
            && let Some(a) = tag.attr("Target")
        {
            let ty = tag.attr_value("Type").unwrap_or("");
            let external = tag.attr_value("TargetMode") == Some("External");
            if ty.ends_with("/attachedTemplate") {
                match op {
                    Op::Extract => self.meta_block("modele", &a.value),
                    Op::Transform { .. } => {
                        if scrub && a.value != NEUTRAL_TEMPLATE {
                            self.patch(a.raw.clone(), escape_attr(NEUTRAL_TEMPLATE));
                        }
                    }
                }
            } else if external {
                self.text_attr(a, BlockRole::Hyperlink, true);
            }
        }
        for a in &tag.attrs {
            if let Some((_, _, role)) = TEXT_ATTRIBUTES
                .iter()
                .find(|(e, n, _)| *e == tag.name && *n == a.name)
            {
                self.text_attr(a, *role, false);
            }
            if self.odf()
                && ODF_LINK_ATTRIBUTES.contains(&a.name.as_str())
                && is_external_uri(&a.value)
            {
                self.text_attr(a, BlockRole::Hyperlink, true);
            }
            if FIELD_INSTRUCTION_ATTRIBUTES.contains(&(tag.name.as_str(), a.name.as_str())) {
                self.field_instruction(a);
            }
            if NUMERIC_CACHE_ATTRIBUTES.contains(&(tag.name.as_str(), a.name.as_str()))
                && let Op::Transform { tr, .. } = op
                && a.value != "0"
                && !tr
                    .replacements(&a.value, &self.ctx(label.clone(), BlockRole::Other))
                    .is_empty()
            {
                self.patch(a.raw.clone(), "0");
                self.warn(
                    WarningCode::UntouchedLocation,
                    "Valeur numérique en cache contenant une valeur sensible, remplacée par 0.",
                );
            }
            if COLUMN_NAME_ATTRIBUTES.contains(&(tag.name.as_str(), a.name.as_str())) {
                let name = x_decode(&a.value);
                match op {
                    Op::Extract => self.block(label.clone(), BlockRole::Cell, &name),
                    Op::Transform { .. } => {
                        if let Some(new) = self.cx.xlsx.columns.get(&name) {
                            let new = escape_attr(&x_encode(new));
                            self.out.count += 1;
                            self.patch(a.raw.clone(), new);
                        }
                    }
                }
            }
            if let Some((_, _, neutral)) = SCRUBBED_ATTRIBUTES
                .iter()
                .find(|(e, n, _)| (*e == "*" || *e == tag.name) && *n == a.name)
            {
                match op {
                    Op::Extract => {
                        let what = a.name.clone();
                        self.meta_block(&what, &a.value);
                    }
                    Op::Transform { .. } => {
                        if scrub && a.value != *neutral {
                            self.patch(a.raw.clone(), escape_attr(neutral));
                        }
                    }
                }
            }
            let sheet_name = (tag.name == "x:sheet" && a.name == "name")
                || (tag.name == "table:table" && a.name == "table:name");
            if sheet_name {
                match op {
                    Op::Extract => self.block(
                        format!("{label} (nom de feuille)"),
                        BlockRole::Other,
                        &a.value,
                    ),
                    Op::Transform { tr, .. } => {
                        let ctx = self.ctx(label.clone(), BlockRole::Other);
                        if !tr.replacements(&a.value, &ctx).is_empty() {
                            self.warn(
                                WarningCode::SheetNameSensitive,
                                "Un nom de feuille contient une valeur sensible ; il est laissé tel quel.",
                            );
                        }
                    }
                }
            }
        }
    }

    /// Instruction de champ (`w:fldSimple@w:instr`) : littéraux entre guillemets.
    fn field_instruction(&mut self, a: &Attr) {
        let hyperlink = a
            .value
            .trim_start()
            .to_ascii_uppercase()
            .starts_with("HYPERLINK");
        let role = if hyperlink {
            BlockRole::Hyperlink
        } else {
            BlockRole::Other
        };
        let label = self.cx.label.clone();
        let lits = field_literals(&a.value);
        match self.cx.op {
            Op::Extract => {
                for (k, r) in lits.into_iter().enumerate() {
                    if let Some(s) = a.value.get(r) {
                        let s = s.to_string();
                        let role = if k == 0 { role } else { BlockRole::Other };
                        self.block(label.clone(), role, &s);
                    }
                }
            }
            Op::Transform { tr, .. } => {
                let mut new = String::with_capacity(a.value.len());
                let mut cursor = 0usize;
                let mut changed = 0usize;
                for (k, r) in lits.into_iter().enumerate() {
                    let (Some(head), Some(lit)) =
                        (a.value.get(cursor..r.start), a.value.get(r.clone()))
                    else {
                        continue;
                    };
                    new.push_str(head);
                    // Pour un champ HYPERLINK, seul le premier littéral est l'URI.
                    let t = if hyperlink && k == 0 {
                        tr.apply_uri(lit).into_owned()
                    } else {
                        tr.apply(lit, &self.ctx(label.clone(), BlockRole::Other))
                            .into_owned()
                    };
                    if t != lit {
                        changed += 1;
                    }
                    new.push_str(&t.replace('"', "'"));
                    cursor = r.end;
                }
                new.push_str(a.value.get(cursor..).unwrap_or(""));
                if changed > 0 {
                    self.out.count += changed;
                    self.patch(a.raw.clone(), escape_attr(&new));
                }
            }
        }
    }
}

/// Révisions dont le texte est séparé du reste par une espace virtuelle :
/// le texte supprimé ne doit pas se coller au texte inséré qui le suit.
const REVISION_SEPARATED: &[&str] = &["w:del", "w:moveFrom"];

/// Éléments supprimés quand `scrub_metadata` : relation, type de contenu ou
/// entrée de manifeste de la miniature.
fn removable(tag: &Tag) -> bool {
    (tag.name == "rel:Relationship"
        && tag
            .attr_value("Type")
            .is_some_and(|t| t.ends_with("/metadata/thumbnail")))
        || (tag.name == "ct:Override"
            && tag
                .attr_value("PartName")
                .is_some_and(|p| p.starts_with("/docProps/thumbnail")))
        || (tag.name == "manifest:file-entry"
            && tag
                .attr_value("manifest:full-path")
                .is_some_and(|p| p.starts_with("Thumbnails/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_literals_and_uris() {
        let s = r#"HYPERLINK "mailto:a@example.com" \o "Jean""#;
        let l: Vec<_> = field_literals(s).into_iter().map(|r| &s[r]).collect();
        assert_eq!(l, vec!["mailto:a@example.com", "Jean"]);
        assert!(is_external_uri("mailto:x@example.com"));
        assert!(is_external_uri("https://example.com"));
        assert!(!is_external_uri("Pictures/a.png"));
        assert!(!is_external_uri("../Object 1"));
        assert!(!is_external_uri("C:/x"));
    }
}
