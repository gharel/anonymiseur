//! Lecture des feuilles de calcul en tableaux (`Handler::tables`), pour
//! l'évaluation des trois critères : une [`TableData`] par feuille, en-têtes =
//! première ligne non vide, valeurs telles qu'affichées (chaînes partagées
//! résolues, nombres en texte, dates au format `JJ/MM/AAAA`).
//!
//! Les lignes vides sont ignorées ; les colonnes vont de la première à la
//! dernière colonne renseignée de la feuille. Les commentaires de cellule et les
//! textes phonétiques ne font pas partie des valeurs.

use crate::limits::{MAX_TABLE_CELLS, MAX_TOTAL_UNCOMPRESSED};
use crate::ooxml::{check_odf_manifest, read_part, xlsx_read_env};
use crate::package::Package;
use crate::part::XlsxEnv;
use crate::xlsx::{serial_to_date, split_ref, x_decode};
use crate::xml::{Item, Tag, Walker};
use anon_core::{FormatError, TableData};
use std::collections::{BTreeMap, HashMap};

/// Nombre maximal d'espaces d'un `text:s` (même plafond que la lecture des blocs).
const MAX_ODF_SPACES: usize = 1024;
/// Nombre maximal de lettres d'une référence de colonne (`XFD`).
const MAX_COLUMN_LETTERS: usize = 3;

/// Formats sans tableaux (Word, PowerPoint).
pub(crate) fn none(_input: &[u8]) -> Result<Vec<TableData>, FormatError> {
    Ok(Vec::new())
}

// ---------------------------------------------------------------------------
// Grille creuse

/// Feuille en cours de lecture : lignes creuses (colonne → texte affiché).
#[derive(Default)]
struct Grid {
    rows: BTreeMap<u64, BTreeMap<u64, String>>,
    cells: usize,
}

fn too_big(part: &str) -> FormatError {
    FormatError::corrupt(part, "tableau trop grand")
}

impl Grid {
    /// Enregistre une cellule non vide.
    fn put(&mut self, row: u64, col: u64, text: String, part: &str) -> Result<(), FormatError> {
        if text.is_empty() {
            return Ok(());
        }
        self.cells = self.cells.saturating_add(1);
        if self.cells > MAX_TABLE_CELLS {
            return Err(too_big(part));
        }
        self.rows.entry(row).or_default().insert(col, text);
        Ok(())
    }

    /// Tableau rectangulaire : lignes non vides, de la première à la dernière
    /// colonne renseignée. `None` si la feuille est vide.
    fn into_table(self, sheet: String, part: &str) -> Result<Option<TableData>, FormatError> {
        let rows: Vec<BTreeMap<u64, String>> = self
            .rows
            .into_values()
            .filter(|r| r.values().any(|v| !v.trim().is_empty()))
            .collect();
        let first = rows.iter().filter_map(|r| r.keys().next().copied()).min();
        let last = rows
            .iter()
            .filter_map(|r| r.keys().next_back().copied())
            .max();
        let (Some(first), Some(last)) = (first, last) else {
            return Ok(None);
        };
        let width = usize::try_from(last.saturating_sub(first))
            .ok()
            .and_then(|w| w.checked_add(1))
            .ok_or_else(|| too_big(part))?;
        if width.saturating_mul(rows.len()) > MAX_TABLE_CELLS {
            return Err(too_big(part));
        }
        let mut dense = rows.into_iter().map(|r| {
            let mut line = vec![String::new(); width];
            for (col, text) in r {
                let slot = usize::try_from(col.saturating_sub(first))
                    .ok()
                    .and_then(|i| line.get_mut(i));
                if let Some(slot) = slot {
                    *slot = text;
                }
            }
            line
        });
        let headers = dense.next().unwrap_or_default();
        Ok(Some(TableData {
            sheet,
            headers,
            rows: dense.collect(),
        }))
    }
}

/// Complète les lignes d'un tableau à la même largeur (CSV aux lignes inégales).
pub(crate) fn rectangular(mut headers: Vec<String>, mut rows: Vec<Vec<String>>) -> TableData {
    let width = rows
        .iter()
        .map(Vec::len)
        .chain(std::iter::once(headers.len()))
        .max()
        .unwrap_or(0);
    headers.resize(width, String::new());
    for r in &mut rows {
        r.resize(width, String::new());
    }
    TableData {
        sheet: String::new(),
        headers,
        rows,
    }
}

// ---------------------------------------------------------------------------
// Excel

/// Indice de colonne d'une référence en lettres (`A` → 0, `AA` → 26).
fn column_index(letters: &str) -> Option<u64> {
    if letters.is_empty() || letters.len() > MAX_COLUMN_LETTERS {
        return None;
    }
    let mut n: u64 = 0;
    for b in letters.bytes() {
        if !b.is_ascii_uppercase() {
            return None;
        }
        n = n.checked_mul(26)?.checked_add(u64::from(b - b'A') + 1)?;
    }
    n.checked_sub(1)
}

/// Cellule Excel en cours de lecture.
struct XCell {
    row: u64,
    col: u64,
    t: Option<String>,
    style: usize,
    v: String,
    inline: String,
}

impl XCell {
    /// Texte affiché, comme à l'extraction des blocs.
    fn display(&self, env: &XlsxEnv) -> String {
        match self.t.as_deref() {
            Some("s") => self
                .v
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| env.shared.get(i))
                .cloned()
                .unwrap_or_default(),
            Some("inlineStr") => x_decode(&self.inline),
            Some("str") => x_decode(&self.v),
            Some("b") => match self.v.trim() {
                "1" => "VRAI".to_string(),
                "0" => "FAUX".to_string(),
                _ => String::new(),
            },
            Some("e" | "d") => self.v.trim().to_string(),
            _ => {
                let raw = self.v.trim();
                let is_date = env.date_styles.get(self.style).copied().unwrap_or(false);
                if is_date && !raw.is_empty() {
                    serial_to_date(raw, env.date1904).unwrap_or_else(|| raw.to_string())
                } else {
                    raw.to_string()
                }
            }
        }
    }
}

/// Feuilles du classeur, dans l'ordre : (nom, identifiant de relation).
fn workbook_sheets(xml: &str) -> Result<Vec<(String, String)>, FormatError> {
    let mut out = Vec::new();
    let mut w = Walker::new(xml, "xl/workbook.xml");
    while let Some(item) = w.next_item()? {
        if let Item::Start(t) | Item::Empty(t) = item
            && t.name == "x:sheet"
            && let (Some(name), Some(rid)) = (t.attr_value("name"), t.attr_value("r:id"))
        {
            out.push((name.to_string(), rid.to_string()));
        }
    }
    Ok(out)
}

/// Relations du classeur : identifiant → (type, cible).
fn relationships(xml: &str, part: &str) -> Result<HashMap<String, (String, String)>, FormatError> {
    let mut out = HashMap::new();
    let mut w = Walker::new(xml, part);
    while let Some(item) = w.next_item()? {
        if let Item::Start(t) | Item::Empty(t) = item
            && t.name == "rel:Relationship"
            && let (Some(id), Some(ty), Some(target)) = (
                t.attr_value("Id"),
                t.attr_value("Type"),
                t.attr_value("Target"),
            )
        {
            out.insert(id.to_string(), (ty.to_string(), target.to_string()));
        }
    }
    Ok(out)
}

/// Chemin d'une partie désignée par une cible relative à `xl/`.
fn resolve(target: &str) -> String {
    let joined = match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None => format!("xl/{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Cellules d'une feuille Excel.
fn xlsx_sheet(src: &str, part: &str, env: &XlsxEnv) -> Result<Grid, FormatError> {
    let mut w = Walker::new(src, part);
    let mut grid = Grid::default();
    let mut row: u64 = 0;
    let mut next_col: u64 = 0;
    let mut cell: Option<XCell> = None;
    while let Some(item) = w.next_item()? {
        match item {
            Item::Start(t) | Item::Empty(t) => match t.name.as_str() {
                "x:row" => {
                    row = t
                        .attr_value("r")
                        .and_then(|r| r.trim().parse::<u64>().ok())
                        .unwrap_or_else(|| row.saturating_add(1));
                    next_col = 0;
                }
                "x:c" => {
                    let pos = t
                        .attr_value("r")
                        .and_then(split_ref)
                        .and_then(|(l, r)| column_index(&l).map(|c| (c, u64::from(r))));
                    let (col, r) = pos.unwrap_or((next_col, row));
                    next_col = col.saturating_add(1);
                    if !t.empty {
                        cell = Some(XCell {
                            row: r,
                            col,
                            t: t.attr_value("t").map(str::to_string),
                            style: t
                                .attr_value("s")
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(0),
                            v: String::new(),
                            inline: String::new(),
                        });
                    }
                }
                _ => {}
            },
            Item::Text { text, .. } => {
                if let Some(c) = cell.as_mut() {
                    let stack = w.stack();
                    match stack.last().map(String::as_str) {
                        Some("x:v") => c.v.push_str(&text),
                        Some("x:t")
                            if stack.iter().any(|s| s == "x:is")
                                && !stack.iter().any(|s| s == "x:rPh") =>
                        {
                            c.inline.push_str(&text);
                        }
                        _ => {}
                    }
                }
            }
            Item::End { name, .. } => {
                if name == "x:c"
                    && let Some(c) = cell.take()
                {
                    let text = c.display(env);
                    grid.put(c.row, c.col, text, part)?;
                }
            }
            Item::Other => {}
        }
    }
    Ok(grid)
}

/// Tableaux d'un classeur Excel : une entrée par feuille de calcul non vide.
pub(crate) fn xlsx(input: &[u8]) -> Result<Vec<TableData>, FormatError> {
    let mut budget = MAX_TOTAL_UNCOMPRESSED;
    let mut pkg = Package::open(input)?;
    for required in ["[Content_Types].xml", "_rels/.rels"] {
        if pkg.index_of(required).is_none() {
            return Err(FormatError::corrupt(required, "partie manquante"));
        }
    }
    let env = xlsx_read_env(&mut pkg, &mut budget)?;
    let workbook = read_part(&mut pkg, "xl/workbook.xml", &mut budget)?
        .ok_or_else(|| FormatError::corrupt("xl/workbook.xml", "partie manquante"))?;
    let rels_part = "xl/_rels/workbook.xml.rels";
    let rels = match read_part(&mut pkg, rels_part, &mut budget)? {
        Some(s) => relationships(&s, rels_part)?,
        None => HashMap::new(),
    };
    let mut out = Vec::new();
    for (name, rid) in workbook_sheets(&workbook)? {
        let Some((ty, target)) = rels.get(&rid) else {
            continue;
        };
        if !ty.ends_with("/worksheet") {
            continue;
        }
        let part = resolve(target);
        let src = read_part(&mut pkg, &part, &mut budget)?
            .ok_or_else(|| FormatError::corrupt(part.clone(), "partie manquante"))?;
        let grid = xlsx_sheet(&src, &part, &env)?;
        if let Some(t) = grid.into_table(name, &part)? {
            out.push(t);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// OpenDocument

/// Répétition d'une ligne ou d'une cellule (au moins 1).
fn repeated(tag: &Tag, attr: &str) -> u64 {
    tag.attr_value(attr)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(1)
        .max(1)
}

/// Feuille OpenDocument en cours de lecture.
struct OSheet {
    name: String,
    depth: usize,
    grid: Grid,
    row: u64,
    open_row: Option<ORow>,
    open_cell: Option<OCell>,
}

/// Ligne OpenDocument ouverte.
struct ORow {
    rep: u64,
    /// Colonne courante.
    col: u64,
    /// Cellules non vides : (colonne, texte).
    cells: Vec<(u64, String)>,
}

/// Cellule OpenDocument ouverte.
struct OCell {
    depth: usize,
    rep: u64,
    paras: Vec<String>,
    /// Valeur des attributs `office:*value`, affichée si la cellule n'a pas de
    /// paragraphe (fichiers produits par d'autres logiciels que LibreOffice).
    fallback: String,
}

/// Attributs portant la valeur d'une cellule OpenDocument.
const ODF_CELL_VALUES: &[&str] = &[
    "office:string-value",
    "office:value",
    "office:date-value",
    "office:time-value",
    "office:boolean-value",
];

impl OSheet {
    fn on_start(&mut self, t: &Tag, depth: usize, stack: &[String]) {
        match t.name.as_str() {
            "table:table-row" if self.open_cell.is_none() => {
                let rep = repeated(t, "table:number-rows-repeated");
                if t.empty {
                    self.row = self.row.saturating_add(rep);
                } else {
                    self.open_row = Some(ORow {
                        rep,
                        col: 0,
                        cells: Vec::new(),
                    });
                }
            }
            "table:table-cell" | "table:covered-table-cell" if self.open_cell.is_none() => {
                let rep = repeated(t, "table:number-columns-repeated");
                if t.empty {
                    if let Some(r) = self.open_row.as_mut() {
                        r.col = r.col.saturating_add(rep);
                    }
                } else {
                    let fallback = ODF_CELL_VALUES
                        .iter()
                        .find_map(|a| t.attr_value(a))
                        .unwrap_or("")
                        .to_string();
                    self.open_cell = Some(OCell {
                        depth,
                        rep,
                        paras: Vec::new(),
                        fallback,
                    });
                }
            }
            "text:p" | "text:h" if !in_annotation(stack) => {
                if let Some(c) = self.open_cell.as_mut() {
                    c.paras.push(String::new());
                }
            }
            "text:s" | "text:tab" | "text:line-break" if !in_annotation(stack) => {
                let text = match t.name.as_str() {
                    "text:s" => " ".repeat(
                        t.attr_value("text:c")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(1)
                            .clamp(1, MAX_ODF_SPACES),
                    ),
                    "text:tab" => "\t".to_string(),
                    _ => "\n".to_string(),
                };
                self.push_text(&text);
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if let Some(c) = self.open_cell.as_mut()
            && let Some(last) = c.paras.last_mut()
        {
            last.push_str(text);
        }
    }

    fn on_end(&mut self, name: &str, depth_after: usize, part: &str) -> Result<(), FormatError> {
        match name {
            "table:table-cell" | "table:covered-table-cell" => {
                let Some(OCell {
                    rep,
                    paras,
                    fallback,
                    ..
                }) = self.open_cell.take_if(|c| c.depth == depth_after + 1)
                else {
                    return Ok(());
                };
                let text = if paras.is_empty() {
                    fallback
                } else {
                    paras.join("\n")
                };
                if let Some(ORow { col, cells, .. }) = self.open_row.as_mut() {
                    if !text.is_empty() {
                        let total = cells.len() as u64 + rep;
                        if total > MAX_TABLE_CELLS as u64 {
                            return Err(too_big(part));
                        }
                        for i in 0..rep {
                            cells.push((col.saturating_add(i), text.clone()));
                        }
                    }
                    *col = col.saturating_add(rep);
                }
            }
            "table:table-row" if self.open_cell.is_none() => {
                if let Some(ORow { rep, cells, .. }) = self.open_row.take() {
                    if !cells.is_empty() {
                        if rep.saturating_mul(cells.len() as u64) > MAX_TABLE_CELLS as u64 {
                            return Err(too_big(part));
                        }
                        for r in 0..rep {
                            for (col, text) in &cells {
                                self.grid.put(
                                    self.row.saturating_add(r),
                                    *col,
                                    text.clone(),
                                    part,
                                )?;
                            }
                        }
                    }
                    self.row = self.row.saturating_add(rep);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn in_annotation(stack: &[String]) -> bool {
    stack.iter().any(|s| s == "office:annotation")
}

/// Tableaux d'un classeur OpenDocument : une entrée par feuille non vide.
/// Un document texte ou une présentation n'en a pas.
pub(crate) fn ods(input: &[u8]) -> Result<Vec<TableData>, FormatError> {
    let mut budget = MAX_TOTAL_UNCOMPRESSED;
    let mut pkg = Package::open(input)?;
    check_odf_manifest(&mut pkg, &mut budget)?;
    let part = "content.xml";
    let content = read_part(&mut pkg, part, &mut budget)?
        .ok_or_else(|| FormatError::corrupt(part, "partie manquante"))?;
    let mut w = Walker::new(&content, part);
    let mut out = Vec::new();
    let mut sheet: Option<OSheet> = None;
    while let Some(item) = w.next_item()? {
        match item {
            Item::Start(t) | Item::Empty(t) => {
                let depth = w.stack().len();
                match sheet.as_mut() {
                    None => {
                        if t.name == "table:table"
                            && !t.empty
                            && w.stack().iter().any(|s| s == "office:spreadsheet")
                        {
                            sheet = Some(OSheet {
                                name: t.attr_value("table:name").unwrap_or("").to_string(),
                                depth,
                                grid: Grid::default(),
                                row: 0,
                                open_row: None,
                                open_cell: None,
                            });
                        }
                    }
                    Some(s) => s.on_start(&t, depth, w.stack()),
                }
            }
            Item::Text { text, .. } => {
                if let Some(s) = sheet.as_mut()
                    && !in_annotation(w.stack())
                    && w.stack().iter().any(|e| e == "text:p" || e == "text:h")
                {
                    s.push_text(&text);
                }
            }
            Item::End { name, .. } => {
                let depth_after = w.stack().len();
                if let Some(s) = sheet.as_mut() {
                    if name == "table:table" && depth_after + 1 == s.depth {
                        if let Some(done) = sheet.take() {
                            let label = format!("{part}:feuille{}", out.len() + 1);
                            if let Some(t) = done.grid.into_table(done.name, &label)? {
                                out.push(t);
                            }
                        }
                    } else {
                        s.on_end(&name, depth_after, part)?;
                    }
                }
            }
            Item::Other => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_and_targets() {
        assert_eq!(column_index("A"), Some(0));
        assert_eq!(column_index("Z"), Some(25));
        assert_eq!(column_index("AA"), Some(26));
        assert_eq!(column_index("XFD"), Some(16383));
        assert_eq!(column_index("ABCD"), None);
        assert_eq!(column_index(""), None);
        assert_eq!(resolve("worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve("/xl/worksheets/s.xml"), "xl/worksheets/s.xml");
        assert_eq!(resolve("../xl/./worksheets/s.xml"), "xl/worksheets/s.xml");
    }

    #[test]
    fn grid_trims_empty_rows_and_columns() {
        let mut g = Grid::default();
        g.put(3, 2, "Âge".into(), "p").unwrap();
        g.put(3, 3, "Commune".into(), "p").unwrap();
        g.put(5, 2, "62".into(), "p").unwrap();
        g.put(6, 3, "Farino".into(), "p").unwrap();
        g.put(7, 1, " ".into(), "p").unwrap();
        // La ligne 7 (espaces seulement) est vide : le tableau commence en colonne 2.
        let t = g.into_table("F".into(), "p").unwrap().unwrap();
        assert_eq!(t.headers, vec!["Âge", "Commune"]);
        assert_eq!(
            t.rows,
            vec![
                vec!["62".to_string(), "".into()],
                vec!["".to_string(), "Farino".into()],
            ]
        );
        assert!(
            Grid::default()
                .into_table("F".into(), "p")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn far_away_cell_is_bounded() {
        let mut g = Grid::default();
        for r in 0..2000 {
            g.put(r, 0, "x".into(), "p").unwrap();
        }
        g.put(1, 16383, "loin".into(), "p").unwrap();
        assert!(g.into_table("F".into(), "p").is_err());
    }
}
