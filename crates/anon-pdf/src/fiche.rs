//! Fiche d'évaluation en PDF : mise en page A4 d'un Markdown simple, celui
//! qu'écrit `anon_engine::fiche::render_markdown` (titres `#` à `###`,
//! paragraphes, listes à puces imbriquées, tableaux `| … |`, gras `**…**`,
//! code `` `…` `` rendu en texte ordinaire).
//!
//! Rendu par MuPDF (déjà lié) : polices de base Helvetica et Helvetica gras en
//! codage Latin, qui couvre les lettres accentuées du français ; les quelques
//! signes hors de ce codage (apostrophe typographique, points de suspension,
//! tirets longs, « œ ») sont remplacés par leur équivalent simple avant la
//! mesure et l'écriture. Coupure des lignes au mot, tableaux à colonnes
//! proportionnelles et en-tête répété en haut de page, pied de page numéroté.

use anon_core::FormatError;
use mupdf::pdf::{PdfDocument, PdfWriteOptions};
use mupdf::shape::{FinishOptions, PdfColor, Shape, TextOptions};
use mupdf::{Font, Point, Size};

/// Largeur d'une page A4 (pt).
pub const A4_WIDTH_PT: f32 = 595.0;
/// Hauteur d'une page A4 (pt).
pub const A4_HEIGHT_PT: f32 = 842.0;
/// Marges gauche et droite (pt).
pub const FICHE_SIDE_MARGIN_PT: f32 = 56.0;
/// Marges haute et basse (pt), pied de page compris dans la marge basse.
pub const FICHE_VERTICAL_MARGIN_PT: f32 = 60.0;
/// Taille du texte courant (pt).
pub const FICHE_BODY_SIZE_PT: f32 = 10.0;

const H1_SIZE: f32 = 18.0;
const H2_SIZE: f32 = 13.5;
const H3_SIZE: f32 = 11.5;
const TABLE_SIZE: f32 = 8.5;
const FOOTER_SIZE: f32 = 8.0;
/// Interligne, en multiple de la taille du texte.
const LINE_FACTOR: f32 = 1.35;
/// Espace avant un titre, en multiple de sa taille.
const HEADING_GAP_FACTOR: f32 = 0.9;
/// Espace après un paragraphe, une liste ou un tableau (pt).
const BLOCK_GAP_PT: f32 = 5.0;
/// Retrait d'un niveau de liste (pt).
const LIST_INDENT_PT: f32 = 14.0;
/// Marge intérieure d'une cellule de tableau (pt).
const CELL_PADDING_PT: f32 = 3.0;
/// Marge ajoutée à la largeur naturelle d'une colonne : une cellule qui tient
/// sur une ligne n'est pas coupée par un arrondi (pt).
const WIDTH_SLACK_PT: f32 = 1.0;
/// Épaisseur des filets (pt).
const RULE_WIDTH_PT: f32 = 0.5;
/// Gris des filets et du pied de page.
const RULE_GRAY: f32 = 0.6;
/// Distance du pied de page au bas de la page (pt).
const FOOTER_FROM_BOTTOM_PT: f32 = 30.0;
/// Nombre maximal de pages (une fiche en compte quelques-unes).
const MAX_PAGES: usize = 500;

fn err() -> FormatError {
    FormatError::Other("création du PDF de la fiche impossible".into())
}

/// Ramène un texte au codage Latin des polices de base : les signes courants
/// hors codage sont remplacés par leur équivalent, les autres par « ? ».
fn latin(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '\u{02BC}' => out.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{201E}' => out.push('"'),
            '\u{2026}' => out.push_str("..."),
            '\u{2010}'..='\u{2015}' | '\u{2212}' => out.push('-'),
            '\u{0153}' => out.push_str("oe"),
            '\u{0152}' => out.push_str("OE"),
            '\u{20AC}' => out.push_str("EUR"),
            '\u{2192}' => out.push_str("->"),
            '\u{2264}' => out.push_str("<="),
            '\u{2265}' => out.push_str(">="),
            '\u{2022}' => out.push('-'),
            '\u{00A0}' | '\u{202F}' | '\u{2009}' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => out.push(' '),
            c if (0x7F..0xA0).contains(&(c as u32)) || (c as u32) > 0xFF => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

/// Morceau de texte d'une ligne Markdown : texte et graisse.
#[derive(Debug, Clone, PartialEq)]
struct Run {
    text: String,
    bold: bool,
}

/// Morceaux d'une ligne : `**` bascule le gras, les accents graves sont retirés.
fn runs(line: &str, bold: bool) -> Vec<Run> {
    let mut out = Vec::new();
    for (i, part) in line.split("**").enumerate() {
        let text = latin(&part.replace('`', ""));
        if !text.is_empty() {
            out.push(Run {
                text,
                bold: bold ^ (i % 2 == 1),
            });
        }
    }
    out
}

/// Mesure du texte avec les polices de base.
struct Metrics {
    regular: Font,
    bold: Font,
}

impl Metrics {
    fn new() -> Result<Self, FormatError> {
        Ok(Self {
            regular: Font::new("Helvetica").map_err(|_| err())?,
            bold: Font::new("Helvetica-Bold").map_err(|_| err())?,
        })
    }

    fn width(&self, text: &str, bold: bool, size: f32) -> f32 {
        let font = if bold { &self.bold } else { &self.regular };
        let mut w = 0.0;
        for c in text.chars() {
            let Ok(code) = i32::try_from(u32::from(c)) else {
                continue;
            };
            if let Ok(g) = font.encode_character(code)
                && let Ok(a) = font.advance_glyph(g)
            {
                w += a;
            }
        }
        w * size
    }
}

/// Élément à dessiner sur une page.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    Text {
        x: f32,
        baseline: f32,
        text: String,
        bold: bool,
        size: f32,
    },
    Rule {
        x0: f32,
        x1: f32,
        y: f32,
    },
}

/// Mot positionné dans une ligne.
struct Word {
    text: String,
    bold: bool,
    width: f32,
}

/// Mise en page : pages successives et position verticale courante.
struct Layout {
    metrics: Metrics,
    pages: Vec<Vec<Op>>,
    /// Haut de l'espace libre sur la page courante (pt depuis le haut).
    y: f32,
}

const LEFT: f32 = FICHE_SIDE_MARGIN_PT;
const RIGHT: f32 = A4_WIDTH_PT - FICHE_SIDE_MARGIN_PT;
const TOP: f32 = FICHE_VERTICAL_MARGIN_PT;
const BOTTOM: f32 = A4_HEIGHT_PT - FICHE_VERTICAL_MARGIN_PT;

impl Layout {
    fn new() -> Result<Self, FormatError> {
        Ok(Self {
            metrics: Metrics::new()?,
            pages: vec![Vec::new()],
            y: TOP,
        })
    }

    fn new_page(&mut self) -> Result<(), FormatError> {
        if self.pages.len() >= MAX_PAGES {
            return Err(err());
        }
        self.pages.push(Vec::new());
        self.y = TOP;
        Ok(())
    }

    /// Réserve `height` pt, sur une nouvelle page si besoin.
    fn reserve(&mut self, height: f32) -> Result<(), FormatError> {
        if self.y + height > BOTTOM && self.y > TOP {
            self.new_page()?;
        }
        Ok(())
    }

    fn push(&mut self, op: Op) {
        if let Some(p) = self.pages.last_mut() {
            p.push(op);
        }
    }

    /// Mots d'une suite de morceaux, mesurés.
    fn words(&self, runs: &[Run], size: f32) -> Vec<Word> {
        let mut out = Vec::new();
        for r in runs {
            for w in r.text.split(' ').filter(|w| !w.is_empty()) {
                out.push(Word {
                    text: w.to_string(),
                    bold: r.bold,
                    width: self.metrics.width(w, r.bold, size),
                });
            }
        }
        out
    }

    /// Lignes d'un texte coupé au mot dans une largeur ; un mot trop long est
    /// coupé à la lettre.
    fn wrap(&self, runs: &[Run], size: f32, width: f32) -> Vec<Vec<(String, bool, f32)>> {
        let space = self.metrics.width(" ", false, size);
        let mut lines: Vec<Vec<(String, bool, f32)>> = Vec::new();
        let mut line: Vec<(String, bool, f32)> = Vec::new();
        let mut x = 0.0f32;
        for w in self.words(runs, size) {
            let mut pieces = vec![w];
            if pieces.first().is_some_and(|p| p.width > width) {
                pieces = self.split_long(pieces.remove(0), size, width);
            }
            for p in pieces {
                let gap = if line.is_empty() { 0.0 } else { space };
                if !line.is_empty() && x + gap + p.width > width {
                    lines.push(std::mem::take(&mut line));
                    x = 0.0;
                }
                let gap = if line.is_empty() { 0.0 } else { space };
                line.push((p.text, p.bold, x + gap));
                x += gap + p.width;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    fn split_long(&self, w: Word, size: f32, width: f32) -> Vec<Word> {
        let mut out = Vec::new();
        let mut cur = String::new();
        for c in w.text.chars() {
            let mut next = cur.clone();
            next.push(c);
            if !cur.is_empty() && self.metrics.width(&next, w.bold, size) > width {
                out.push(Word {
                    width: self.metrics.width(&cur, w.bold, size),
                    text: std::mem::take(&mut cur),
                    bold: w.bold,
                });
                next = c.to_string();
            }
            cur = next;
        }
        if !cur.is_empty() {
            out.push(Word {
                width: self.metrics.width(&cur, w.bold, size),
                text: cur,
                bold: w.bold,
            });
        }
        out
    }

    /// Paragraphe (ou élément de liste) à partir de `x`, avec une puce éventuelle.
    fn paragraph(
        &mut self,
        runs: &[Run],
        size: f32,
        x: f32,
        bullet: Option<f32>,
    ) -> Result<(), FormatError> {
        let line_h = size * LINE_FACTOR;
        let lines = self.wrap(runs, size, RIGHT - x);
        for (i, line) in lines.into_iter().enumerate() {
            self.reserve(line_h)?;
            let baseline = self.y + size;
            if i == 0
                && let Some(bx) = bullet
            {
                self.push(Op::Text {
                    x: bx,
                    baseline,
                    text: "-".into(),
                    bold: false,
                    size,
                });
            }
            for (text, bold, dx) in line {
                self.push(Op::Text {
                    x: x + dx,
                    baseline,
                    text,
                    bold,
                    size,
                });
            }
            self.y += line_h;
        }
        Ok(())
    }

    fn heading(&mut self, text: &str, size: f32) -> Result<(), FormatError> {
        let gap = if self.y > TOP {
            size * HEADING_GAP_FACTOR
        } else {
            0.0
        };
        // Un titre n'est jamais laissé seul en bas de page.
        self.reserve(gap + size * LINE_FACTOR * 3.0)?;
        if self.y > TOP {
            self.y += gap;
        }
        self.paragraph(&runs(text, true), size, LEFT, None)
    }

    /// Cellules d'une ligne de tableau, coupées dans leur colonne.
    fn wrap_row(
        &self,
        cells: &[Vec<Run>],
        widths: &[f32],
        bold: bool,
    ) -> Vec<Vec<Vec<(String, bool, f32)>>> {
        cells
            .iter()
            .zip(widths)
            .map(|(c, w)| {
                let runs: Vec<Run> = c
                    .iter()
                    .map(|r| Run {
                        text: r.text.clone(),
                        bold: r.bold || bold,
                    })
                    .collect();
                self.wrap(&runs, TABLE_SIZE, (w - 2.0 * CELL_PADDING_PT).max(1.0))
            })
            .collect()
    }

    fn row_height(&self, cells: &[Vec<Run>], widths: &[f32], bold: bool) -> f32 {
        let n = self
            .wrap_row(cells, widths, bold)
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .max(1);
        n as f32 * TABLE_SIZE * LINE_FACTOR + 2.0 * CELL_PADDING_PT
    }

    /// Dessine une ligne de tableau à la position courante, filet dessous.
    fn draw_row(&mut self, cells: &[Vec<Run>], widths: &[f32], bold: bool) {
        let line_h = TABLE_SIZE * LINE_FACTOR;
        let height = self.row_height(cells, widths, bold);
        let mut x = LEFT;
        for (lines, w) in self.wrap_row(cells, widths, bold).into_iter().zip(widths) {
            for (i, line) in lines.into_iter().enumerate() {
                let baseline = self.y + CELL_PADDING_PT + i as f32 * line_h + TABLE_SIZE;
                for (text, b, dx) in line {
                    self.push(Op::Text {
                        x: x + CELL_PADDING_PT + dx,
                        baseline,
                        text,
                        bold: b,
                        size: TABLE_SIZE,
                    });
                }
            }
            x += w;
        }
        self.y += height;
        self.push(Op::Rule {
            x0: LEFT,
            x1: LEFT + widths.iter().sum::<f32>(),
            y: self.y,
        });
    }

    fn table(&mut self, rows: &[Vec<String>]) -> Result<(), FormatError> {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return Ok(());
        }
        let cells: Vec<Vec<Vec<Run>>> = rows
            .iter()
            .map(|r| {
                (0..cols)
                    .map(|c| runs(r.get(c).map_or("", String::as_str), false))
                    .collect()
            })
            .collect();
        // Largeur naturelle de chaque colonne (texte sur une ligne).
        let natural: Vec<f32> = (0..cols)
            .map(|c| {
                cells
                    .iter()
                    .filter_map(|r| r.get(c))
                    .map(|runs| {
                        runs.iter()
                            .map(|r| self.metrics.width(&r.text, true, TABLE_SIZE))
                            .sum::<f32>()
                            + 2.0 * CELL_PADDING_PT
                            + WIDTH_SLACK_PT
                    })
                    .fold(2.0 * CELL_PADDING_PT, f32::max)
            })
            .collect();
        let available = RIGHT - LEFT;
        let total: f32 = natural.iter().sum();
        let widths: Vec<f32> = if total <= available {
            natural
        } else {
            // Chaque colonne garde au moins sa part égale (ou sa largeur
            // naturelle si elle est plus petite) ; le reste va aux plus larges.
            let fair = available / cols as f32;
            let base: Vec<f32> = natural.iter().map(|n| n.min(fair)).collect();
            let extra = available - base.iter().sum::<f32>();
            let want: f32 = natural.iter().zip(&base).map(|(n, b)| n - b).sum();
            natural
                .iter()
                .zip(&base)
                .map(|(n, b)| {
                    if want > 0.0 {
                        b + extra * (n - b) / want
                    } else {
                        *b
                    }
                })
                .collect()
        };
        let right = LEFT + widths.iter().sum::<f32>();
        let head = cells.first();
        for (i, row) in cells.iter().enumerate() {
            let header = i == 0;
            let height = self.row_height(row, &widths, header);
            if header {
                // L'en-tête n'est jamais laissé seul en bas de page.
                let next = cells
                    .get(1)
                    .map_or(0.0, |r| self.row_height(r, &widths, false));
                self.reserve(height + next)?;
                self.push(Op::Rule {
                    x0: LEFT,
                    x1: right,
                    y: self.y,
                });
            } else if self.y + height > BOTTOM && self.y > TOP {
                // Nouvelle page au milieu du tableau : l'en-tête est répété.
                self.new_page()?;
                if let Some(h) = head {
                    self.push(Op::Rule {
                        x0: LEFT,
                        x1: right,
                        y: self.y,
                    });
                    self.draw_row(h, &widths, true);
                }
            }
            self.draw_row(row, &widths, header);
        }
        Ok(())
    }
}

/// Bloc Markdown.
#[derive(Debug, Clone, PartialEq)]
enum Block {
    Heading(usize, String),
    Paragraph(String),
    Item(usize, String),
    Table(Vec<Vec<String>>),
}

fn table_cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn blocks(markdown: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let mut table: Vec<Vec<String>> = Vec::new();
    let flush = |out: &mut Vec<Block>, para: &mut Vec<&str>, table: &mut Vec<Vec<String>>| {
        if !para.is_empty() {
            out.push(Block::Paragraph(para.join(" ")));
            para.clear();
        }
        if !table.is_empty() {
            out.push(Block::Table(std::mem::take(table)));
        }
    };
    for raw in markdown.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with('|') {
            if !para.is_empty() {
                out.push(Block::Paragraph(para.join(" ")));
                para.clear();
            }
            if !is_separator(trimmed) {
                table.push(table_cells(trimmed));
            }
            continue;
        }
        if !table.is_empty() {
            out.push(Block::Table(std::mem::take(&mut table)));
        }
        if trimmed.is_empty() {
            flush(&mut out, &mut para, &mut table);
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=3).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
            flush(&mut out, &mut para, &mut table);
            out.push(Block::Heading(
                hashes,
                trimmed.get(hashes..).unwrap_or("").trim().to_string(),
            ));
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            flush(&mut out, &mut para, &mut table);
            let level = (line.len() - trimmed.len()) / 2;
            out.push(Block::Item(level, item.to_string()));
            continue;
        }
        para.push(trimmed);
    }
    flush(&mut out, &mut para, &mut table);
    out
}

/// Mise en page du Markdown : opérations de dessin, page par page.
fn layout(markdown: &str) -> Result<Vec<Vec<Op>>, FormatError> {
    let mut l = Layout::new()?;
    let mut previous_was_item = false;
    for b in blocks(markdown) {
        let is_item = matches!(b, Block::Item(..));
        if previous_was_item && !is_item {
            l.y += BLOCK_GAP_PT;
        }
        match b {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => H1_SIZE,
                    2 => H2_SIZE,
                    _ => H3_SIZE,
                };
                l.heading(&text, size)?;
                l.y += BLOCK_GAP_PT * 0.5;
            }
            Block::Paragraph(text) => {
                l.paragraph(&runs(&text, false), FICHE_BODY_SIZE_PT, LEFT, None)?;
                l.y += BLOCK_GAP_PT;
            }
            Block::Item(level, text) => {
                let indent = LEFT + LIST_INDENT_PT * (level as f32 + 1.0);
                l.paragraph(
                    &runs(&text, false),
                    FICHE_BODY_SIZE_PT,
                    indent,
                    Some(indent - LIST_INDENT_PT * 0.7),
                )?;
            }
            Block::Table(rows) => {
                l.table(&rows)?;
                l.y += BLOCK_GAP_PT * 2.0;
            }
        }
        previous_was_item = is_item;
    }
    Ok(l.pages)
}

/// PDF A4 de la fiche d'évaluation écrite en Markdown. Aucun contenu n'est
/// ajouté ni journalisé ; l'erreur ne cite pas le texte.
pub fn render_fiche_pdf(markdown: &str) -> Result<Vec<u8>, FormatError> {
    let pages = layout(markdown)?;
    let total = pages.len();
    let mut doc = PdfDocument::new();
    let black = Some(PdfColor::rgb(0.0, 0.0, 0.0));
    let gray = Some(PdfColor::rgb(RULE_GRAY, RULE_GRAY, RULE_GRAY));
    let metrics = Metrics::new()?;
    for (i, ops) in pages.into_iter().enumerate() {
        let mut page = doc.new_page(Size::A4).map_err(|_| err())?;
        let mut shape = Shape::new(&mut page).map_err(|_| err())?;
        let footer = latin(&format!("Fiche d'évaluation · page {} / {total}", i + 1));
        let fw = metrics.width(&footer, false, FOOTER_SIZE);
        let ops = ops.into_iter().chain(std::iter::once(Op::Text {
            x: (A4_WIDTH_PT - fw) / 2.0,
            baseline: A4_HEIGHT_PT - FOOTER_FROM_BOTTOM_PT,
            text: footer,
            bold: false,
            size: FOOTER_SIZE,
        }));
        for op in ops {
            match op {
                Op::Text {
                    x,
                    baseline,
                    text,
                    bold,
                    size,
                } => {
                    let fill = if size == FOOTER_SIZE {
                        gray.clone()
                    } else {
                        black.clone()
                    };
                    shape
                        .insert_text(
                            Point::new(x, baseline),
                            &text,
                            &TextOptions {
                                fontsize: size,
                                fontname: if bold { "hebo" } else { "helv" }.into(),
                                fill,
                                ..Default::default()
                            },
                        )
                        .map_err(|_| err())?;
                }
                Op::Rule { x0, x1, y } => {
                    shape
                        .draw_line(Point::new(x0, y), Point::new(x1, y))
                        .and_then(|s| {
                            s.finish(&FinishOptions {
                                color: gray.clone(),
                                width: RULE_WIDTH_PT,
                                close_path: false,
                                ..Default::default()
                            })
                        })
                        .map_err(|_| err())?;
                }
            }
        }
        shape.commit(&mut doc, true).map_err(|_| err())?;
    }
    let mut o = PdfWriteOptions::default();
    o.set_garbage_level(4)
        .set_clean(true)
        .set_compress(true)
        .set_incremental(false)
        .set_encryption(mupdf::pdf::Encryption::None);
    let mut out = Vec::new();
    doc.write_to_with_options(&mut out, o).map_err(|_| err())?;
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use anon_core::Handler;

    const SAMPLE: &str = "# Fiche d'évaluation\n\n\
        Évaluation du 15/11/2026 · Anonymiseur 0.1.1\n\n\
        ## Conclusion\n\n\
        3 sujets restent reconnaissables pour un assistant IA grand public : le dossier \
        reste une donnée personnelle (un seul sujet reconnaissable suffit, § 36).\n\n\
        ## Résultat par critère\n\n\
        | Critère | Sujets qui passent | Sujets en échec |\n|---|---|---|\n\
        | Isolement | 4 | 3 |\n| Recoupement | 7 | 0 |\n| Déduction | 7 | 0 |\n\n\
        ### Isolement\n\n\
        - `[PERSONNE_2]` : groupe estimé à 1 personne, moins de 5\n  - niveau imbriqué\n\n\
        ## Réévaluation\n\n\
        Réévaluez plus tôt en cas d'incident de sécurité ; « guillemets » et cœur’s…\n";

    fn text_of(pdf: &[u8]) -> String {
        crate::PdfHandler
            .extract(pdf)
            .unwrap()
            .blocks
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn markdown_blocks() {
        let b = blocks(SAMPLE);
        assert_eq!(b[0], Block::Heading(1, "Fiche d'évaluation".into()));
        assert!(
            b.iter()
                .any(|x| matches!(x, Block::Table(rows) if rows.len() == 4))
        );
        assert!(b.contains(&Block::Item(1, "niveau imbriqué".into())));
        assert_eq!(
            runs("a **b** c", false),
            vec![
                Run {
                    text: "a ".into(),
                    bold: false
                },
                Run {
                    text: "b".into(),
                    bold: true
                },
                Run {
                    text: " c".into(),
                    bold: false
                }
            ]
        );
        assert_eq!(latin("cœur’s…"), "coeur's...");
        assert_eq!(latin("é à ç « » § ·"), "é à ç « » § ·");
    }

    #[test]
    fn pdf_reopens_with_french_text() {
        let pdf = render_fiche_pdf(SAMPLE).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let text = text_of(&pdf);
        for expected in [
            "Fiche d'évaluation",
            "Conclusion",
            "Résultat par critère",
            "Déduction",
            "Réévaluation",
            "[PERSONNE_2]",
            "« guillemets »",
            "coeur's...",
            "page 1 / 1",
        ] {
            assert!(text.contains(expected), "{expected} absent de :\n{text}");
        }
    }

    #[test]
    fn long_documents_and_tables_span_pages() {
        let mut md = String::from("# Titre\n\n| Sujet | Résultat |\n|---|---|\n");
        for i in 0..200 {
            md.push_str(&format!("| tableau 1, groupe {i} | réussi |\n"));
        }
        md.push_str("\nUnmotextrêmementlongquinetientpassurunelignedelapageetquidoitêtrecoupéàlalettreavantlamargedroite\n");
        let pdf = render_fiche_pdf(&md).unwrap();
        let text = text_of(&pdf);
        assert!(text.contains("groupe 199"));
        assert!(text.contains("page 2 /"), "{text}");
        // L'en-tête du tableau est répété en haut de chaque page.
        assert!(text.matches("Sujet").count() >= 2);
        let layout = layout(&md).unwrap();
        for ops in &layout {
            for op in ops {
                if let Op::Text { x, baseline, .. } = op {
                    assert!(*x >= LEFT - 0.01 && *x <= RIGHT, "{x}");
                    assert!(*baseline <= A4_HEIGHT_PT, "{baseline}");
                }
            }
        }
    }

    #[test]
    fn empty_and_odd_markdown() {
        assert!(render_fiche_pdf("").is_ok());
        assert!(render_fiche_pdf("|\n|---|\n||\n#\n- \n**").is_ok());
        assert!(render_fiche_pdf("\u{1F600} \u{0000} \u{FFFF}").is_ok());
    }
}
