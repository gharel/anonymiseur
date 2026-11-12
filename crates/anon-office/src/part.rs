//! Traitement en flux d'une partie XML d'un paquet OOXML : extraction des
//! blocs ou calcul des correctifs à appliquer.

use crate::para::{Edits, PRESERVE_ATTR, Para};
use crate::rules::{
    COLUMN_NAME_ATTRIBUTES, ElementRule, NEUTRAL_TEMPLATE, PARAGRAPH_ELEMENTS, RUN_TEXT_ELEMENTS,
    SCRUBBED_ATTRIBUTES, TEXT_ATTRIBUTES, TEXT_ELEMENTS, VIRTUAL_ELEMENTS,
};
use crate::xlsx::{
    formula_replacements, parent_of, parse_formula, serial_to_date, x_decode, x_encode,
};
use crate::xml::{Attr, Item, Patch, Tag, Walker, escape_attr, escape_text};
use anon_core::text::apply_replacements;
use anon_core::{
    BlockContext, BlockRole, FormatError, Replacement, TextBlock, TextTransform, Warning,
    WarningCode,
};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Opération en cours.
#[derive(Clone, Copy)]
pub(crate) enum Op<'a> {
    Extract,
    Transform {
        tr: &'a dyn TextTransform,
        scrub: bool,
    },
}

/// Contexte propre à un classeur Excel.
#[derive(Debug, Default)]
pub(crate) struct XlsxEnv {
    /// Pour chaque style de cellule, vrai si c'est un format de date.
    pub date_styles: Vec<bool>,
    pub date1904: bool,
    /// Chaînes partagées (extraction).
    pub shared: Vec<String>,
    /// Noms de colonnes de tableaux renommés (ancien → nouveau), transformation.
    pub columns: HashMap<String, String>,
}

/// Contexte d'une partie.
pub(crate) struct PartCtx<'a> {
    /// Emplacement affiché dans les blocs (préfixé pour un fichier embarqué).
    pub label: String,
    /// Rôle des paragraphes ordinaires.
    pub role: BlockRole,
    pub excel: bool,
    pub xlsx: &'a XlsxEnv,
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
}

struct OpenPara {
    para: Para,
    kind: Kind,
    /// Profondeur de pile de l'élément paragraphe.
    depth: usize,
    start: usize,
    /// Le texte est directement dans l'élément paragraphe.
    self_text: bool,
    /// Échappements `_xHHHH_` d'Excel.
    xesc: bool,
    /// Élément porteur ouvert : identifiant et profondeur.
    holder: Option<(usize, usize)>,
    name: String,
    prefix: String,
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

struct State<'a, 'c> {
    cx: &'c PartCtx<'a>,
    out: PartOut,
    paras: Vec<OpenPara>,
    cell: Option<Cell>,
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
        removing: None,
        seen_meta: HashSet::new(),
    };
    while let Some(item) = w.next_item()? {
        match item {
            Item::Start(tag) => st.on_tag(&tag, w.stack()),
            Item::Empty(tag) => st.on_tag(&tag, w.stack()),
            Item::End { name, span } => st.on_end(&name, span, w.stack().len()),
            Item::Text { span, text } => st.on_text(span, &text, w.stack().len()),
            Item::Other => {}
        }
    }
    Ok(st.out)
}

fn text_element(name: &str, parent: Option<&str>) -> Option<(ElementRule, bool)> {
    TEXT_ELEMENTS
        .iter()
        .find(|(n, p, _)| *n == name && (p.is_none() || *p == parent))
        .map(|(_, p, r)| (*r, p == &Some("vt:vector")))
}

fn has_attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t Attr> {
    tag.attr(name)
}

impl State<'_, '_> {
    fn scrub(&self) -> bool {
        matches!(self.cx.op, Op::Transform { scrub: true, .. })
    }

    fn ctx(&self, part: String, role: BlockRole) -> BlockContext {
        BlockContext {
            part,
            role,
            column_header: None,
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

        if let Some((kind, self_text)) = self.para_kind(&tag.name, parent, stack) {
            if !tag.empty {
                let xesc = tag.name.starts_with("x:") && kind != Kind::Formula;
                self.paras.push(OpenPara {
                    para: Para::new(),
                    kind,
                    depth,
                    start: tag.span.start,
                    self_text,
                    xesc,
                    holder: None,
                    name: tag.name.clone(),
                    prefix: tag.prefix.clone(),
                });
            }
            return;
        }

        let Some(top) = self.paras.last_mut() else {
            return;
        };
        if !tag.empty
            && let Some((_, _, preserve)) = RUN_TEXT_ELEMENTS
                .iter()
                .find(|(n, p, _)| *n == tag.name && (p.is_none() || *p == parent))
        {
            let fix = if *preserve {
                match has_attr(tag, "xml:space") {
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

    fn para_kind(
        &self,
        name: &str,
        parent: Option<&str>,
        stack: &[String],
    ) -> Option<(Kind, bool)> {
        if name == "x:v" && parent == Some("x:c") {
            return Some((Kind::CellValue, true));
        }
        if PARAGRAPH_ELEMENTS
            .iter()
            .any(|(n, p)| *n == name && (p.is_none() || *p == parent))
        {
            let kind = match name {
                "x:si" => Kind::Shared,
                "x:is" => Kind::Text(BlockRole::Cell),
                "x:text" => Kind::Text(BlockRole::Comment),
                _ => Kind::Text(self.cx.role),
            };
            return Some((kind, false));
        }
        let (rule, titles) = text_element(name, parent)?;
        if titles && (self.cx.excel || !stack.iter().any(|s| s == "ep:TitlesOfParts")) {
            return None;
        }
        let kind = match rule {
            ElementRule::Text(role) => Kind::Text(role),
            ElementRule::Formula => Kind::Formula,
            ElementRule::Scrub => Kind::Scrub,
        };
        Some((kind, true))
    }

    fn on_text(&mut self, span: Range<usize>, text: &str, stack_len: usize) {
        let Some(top) = self.paras.last_mut() else {
            return;
        };
        let holder = if top.self_text && stack_len == top.depth {
            None
        } else {
            match top.holder {
                Some((id, d)) if d == stack_len => Some(id),
                _ => return,
            }
        };
        if top.xesc {
            top.para.push_text(&x_decode(text), span, holder);
        } else {
            top.para.push_text(text, span, holder);
        }
    }

    fn on_end(&mut self, name: &str, span: Range<usize>, stack_len: usize) {
        let ended = stack_len + 1;
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
        let label = match (&self.cell, p.name.as_str()) {
            (Some(c), "x:is") => format!("{}!{}", self.cx.label, c.r),
            _ => self.cx.label.clone(),
        };
        match (p.kind, self.cx.op) {
            (Kind::Text(role), Op::Extract) => self.block(label, role, &p.para.text),
            (Kind::Shared, Op::Extract) => self.out.shared.push(p.para.text.clone()),
            (Kind::Text(role), Op::Transform { tr, .. }) => {
                let reps = tr.replacements(&p.para.text, &self.ctx(label, role));
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
            (Kind::CellValue, _) => self.cell_value(p, end),
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
                match op {
                    Op::Extract => self.block(label.clone(), BlockRole::Hyperlink, &a.value),
                    Op::Transform { tr, .. } => {
                        let new = tr.apply_uri(&a.value);
                        if new != a.value {
                            let new = escape_attr(&new);
                            self.out.count += 1;
                            self.patch(a.raw.clone(), new);
                        }
                    }
                }
            }
        }
        for a in &tag.attrs {
            if let Some((_, _, role)) = TEXT_ATTRIBUTES
                .iter()
                .find(|(e, n, _)| *e == tag.name && *n == a.name)
            {
                match op {
                    Op::Extract => self.block(label.clone(), *role, &a.value),
                    Op::Transform { tr, .. } => {
                        let reps = tr.replacements(&a.value, &self.ctx(label.clone(), *role));
                        if !reps.is_empty() {
                            let new = apply_replacements(&a.value, &reps);
                            if new != a.value {
                                self.out.count += reps.len();
                                self.patch(a.raw.clone(), escape_attr(&new));
                            }
                        }
                    }
                }
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
            if tag.name == "x:sheet" && a.name == "name" {
                match op {
                    Op::Extract => self.block(
                        format!("{label} (nom de feuille)"),
                        BlockRole::Other,
                        &a.value,
                    ),
                    Op::Transform { tr, .. } => {
                        let ctx = self.ctx(label.clone(), BlockRole::Other);
                        if !tr.replacements(&a.value, &ctx).is_empty() {
                            self.out.warnings.push(Warning::new(
                                WarningCode::SheetNameSensitive,
                                label.clone(),
                                "Un nom de feuille contient une valeur sensible ; il est laissé tel quel.",
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// Révisions dont le texte est séparé du reste par une espace virtuelle :
/// le texte supprimé ne doit pas se coller au texte inséré qui le suit.
const REVISION_SEPARATED: &[&str] = &["w:del", "w:moveFrom"];

/// Éléments supprimés quand `scrub_metadata` : relation et type de contenu de la miniature.
fn removable(tag: &Tag) -> bool {
    (tag.name == "rel:Relationship"
        && tag
            .attr_value("Type")
            .is_some_and(|t| t.ends_with("/metadata/thumbnail")))
        || (tag.name == "ct:Override"
            && tag
                .attr_value("PartName")
                .is_some_and(|p| p.starts_with("/docProps/thumbnail")))
}
