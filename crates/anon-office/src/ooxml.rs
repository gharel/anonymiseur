//! Gestionnaires Word, Excel et PowerPoint (Office Open XML).

use crate::limits::{DEEP_SCAN_CHUNK, MAX_EMBED_DEPTH, MAX_TOTAL_UNCOMPRESSED};
use crate::package::{Action, Package, label};
use crate::part::{Op, PartCtx, PartOut, XlsxEnv, process};
use crate::xlsx::{date_styles, split_ref, workbook_date1904, x_decode};
use crate::xml::{Item, Walker, all_strings, apply_patches};
use anon_core::{
    BlockContext, BlockRole, Extraction, FormatError, Handler, MediaItem, MediaTransform, Output,
    TextBlock, TextTransform, TransformOptions, Warning, WarningCode,
};
use std::collections::{BTreeMap, HashSet};

/// Famille de document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flavor {
    Word,
    Excel,
    PowerPoint,
}

/// Images matricielles renvoyées dans `Extraction::media`.
const RASTER_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"];
/// Images vectorielles non analysées.
const VECTOR_EXTENSIONS: &[&str] = &["emf", "wmf", "emz", "wmz"];

/// Word : `docx`, `docm`, `dotx`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DocxHandler;
/// Excel : `xlsx`, `xlsm`, `xltx`.
#[derive(Debug, Clone, Copy, Default)]
pub struct XlsxHandler;
/// PowerPoint : `pptx`, `pptm`, `potx`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PptxHandler;

fn flavor_for(ext: &str) -> Option<Flavor> {
    match ext {
        "docx" | "docm" | "dotx" | "dotm" => Some(Flavor::Word),
        "xlsx" | "xlsm" | "xltx" | "xltm" => Some(Flavor::Excel),
        "pptx" | "pptm" | "potx" | "potm" => Some(Flavor::PowerPoint),
        _ => None,
    }
}

macro_rules! ooxml_handler {
    ($t:ty, $flavor:expr, $exts:expr) => {
        impl Handler for $t {
            fn extensions(&self) -> &'static [&'static str] {
                $exts
            }

            fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
                let mut budget = MAX_TOTAL_UNCOMPRESSED;
                let r = run(input, $flavor, &Mode::Extract, 0, "", &mut budget)?;
                Ok(Extraction {
                    blocks: r.blocks,
                    media: r.media,
                    warnings: r.warnings,
                })
            }

            fn deep_scan(&self, input: &[u8]) -> Result<Vec<TextBlock>, FormatError> {
                let mut budget = MAX_TOTAL_UNCOMPRESSED;
                deep(input, $flavor, 0, "", &mut budget)
            }

            fn transform(
                &self,
                input: &[u8],
                tr: &dyn TextTransform,
                opts: &TransformOptions<'_>,
            ) -> Result<Output, FormatError> {
                let mut budget = MAX_TOTAL_UNCOMPRESSED;
                let mode = Mode::Transform {
                    tr,
                    scrub: opts.scrub_metadata,
                    media: opts.media,
                };
                let r = run(input, $flavor, &mode, 0, "", &mut budget)?;
                Ok(Output {
                    bytes: r.bytes.unwrap_or_else(|| input.to_vec()),
                    replacements: r.count,
                    warnings: r.warnings,
                })
            }
        }
    };
}

ooxml_handler!(DocxHandler, Flavor::Word, &["docx", "docm", "dotx"]);
ooxml_handler!(XlsxHandler, Flavor::Excel, &["xlsx", "xlsm", "xltx"]);
ooxml_handler!(PptxHandler, Flavor::PowerPoint, &["pptx", "pptm", "potx"]);

enum Mode<'a> {
    Extract,
    Transform {
        tr: &'a dyn TextTransform,
        scrub: bool,
        media: Option<&'a dyn MediaTransform>,
    },
}

impl Mode<'_> {
    fn op(&self) -> Op<'_> {
        match self {
            Mode::Extract => Op::Extract,
            Mode::Transform { tr, scrub, .. } => Op::Transform {
                tr: *tr,
                scrub: *scrub,
            },
        }
    }
}

#[derive(Default)]
struct RunOut {
    blocks: Vec<TextBlock>,
    media: Vec<MediaItem>,
    warnings: Vec<Warning>,
    count: usize,
    bytes: Option<Vec<u8>>,
    changed: bool,
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_xml_part(name: &str) -> bool {
    matches!(ext_of(name).as_str(), "xml" | "rels")
}

/// Rôle des paragraphes ordinaires d'une partie.
fn part_role(name: &str) -> BlockRole {
    let n = name.to_ascii_lowercase();
    let file = n.rsplit('/').next().unwrap_or("");
    if n.starts_with("docprops/") {
        BlockRole::Metadata
    } else if file.starts_with("header") {
        BlockRole::Header
    } else if file.starts_with("footer") {
        BlockRole::Footer
    } else if file.contains("comment") {
        BlockRole::Comment
    } else if file.starts_with("footnotes")
        || file.starts_with("endnotes")
        || n.contains("notesslides/")
        || n.contains("notesmasters/")
    {
        BlockRole::Note
    } else {
        BlockRole::Body
    }
}

/// Texte UTF-8 d'une partie XML, sans BOM, et ce BOM.
fn part_text<'b>(bytes: &'b [u8], part: &str) -> Result<(&'b str, &'b [u8]), FormatError> {
    let (bom, body) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (bytes.get(..3).unwrap_or(&[]), rest),
        None => (&[][..], bytes),
    };
    let s = std::str::from_utf8(body)
        .map_err(|_| FormatError::corrupt(part, "encodage XML non pris en charge"))?;
    Ok((s, bom))
}

fn read_part(
    pkg: &mut Package<'_>,
    name: &str,
    budget: &mut u64,
) -> Result<Option<String>, FormatError> {
    let Some(i) = pkg.index_of(name) else {
        return Ok(None);
    };
    let bytes = pkg.read(i, budget)?;
    let (s, _) = part_text(&bytes, name)?;
    Ok(Some(s.to_string()))
}

/// Prépare le contexte Excel : styles de date, système de dates, chaînes
/// partagées (extraction), noms de colonnes renommés (transformation).
fn xlsx_env(
    pkg: &mut Package<'_>,
    mode: &Mode<'_>,
    prefix: &str,
    budget: &mut u64,
) -> Result<XlsxEnv, FormatError> {
    let mut env = XlsxEnv::default();
    if let Some(s) = read_part(pkg, "xl/styles.xml", budget)? {
        env.date_styles = date_styles(&s)?;
    }
    if let Some(s) = read_part(pkg, "xl/workbook.xml", budget)? {
        env.date1904 = workbook_date1904(&s)?;
    }
    match mode {
        Mode::Extract => {
            if let Some(s) = read_part(pkg, "xl/sharedStrings.xml", budget)? {
                let tmp = XlsxEnv::default();
                let cx = PartCtx {
                    label: format!("{prefix}xl/sharedStrings.xml"),
                    role: BlockRole::Cell,
                    excel: true,
                    xlsx: &tmp,
                    op: Op::Extract,
                };
                env.shared = process(&s, &cx)?.shared;
            }
        }
        Mode::Transform { tr, .. } => {
            let tables: Vec<String> = pkg
                .entries
                .iter()
                .filter(|e| e.name.starts_with("xl/tables/") && is_xml_part(&e.name))
                .map(|e| e.name.clone())
                .collect();
            for t in tables {
                let Some(s) = read_part(pkg, &t, budget)? else {
                    continue;
                };
                let mut w = Walker::new(&s, &t);
                while let Some(item) = w.next_item()? {
                    if let Item::Start(tag) | Item::Empty(tag) = item
                        && tag.name == "x:tableColumn"
                        && let Some(name) = tag.attr_value("name")
                    {
                        let name = x_decode(name);
                        let ctx = BlockContext::new(format!("{prefix}{t}"), BlockRole::Cell);
                        let new = tr.apply(&name, &ctx);
                        if new != name {
                            let new = new.into_owned();
                            env.columns.insert(name, new);
                        }
                    }
                }
            }
        }
    }
    Ok(env)
}

/// Lecture ou réécriture d'un paquet OOXML, fichiers embarqués compris.
fn run(
    input: &[u8],
    flavor: Flavor,
    mode: &Mode<'_>,
    depth: usize,
    prefix: &str,
    budget: &mut u64,
) -> Result<RunOut, FormatError> {
    let mut pkg = Package::open(input)?;
    for required in ["[Content_Types].xml", "_rels/.rels"] {
        if pkg.index_of(required).is_none() {
            return Err(FormatError::corrupt(required, "partie manquante"));
        }
    }
    let env = if flavor == Flavor::Excel {
        xlsx_env(&mut pkg, mode, prefix, budget)?
    } else {
        XlsxEnv::default()
    };
    let scrub = matches!(mode, Mode::Transform { scrub: true, .. });
    let mut out = RunOut::default();
    let mut actions = Vec::with_capacity(pkg.entries.len());
    let mut used_shared: HashSet<usize> = HashSet::new();
    let entries = pkg.entries.clone();
    for (i, entry) in entries.iter().enumerate() {
        let name = entry.name.as_str();
        let lname = name.to_ascii_lowercase();
        let ext = ext_of(name);
        let shown = format!("{prefix}{}", label(name, i));
        if entry.is_dir {
            actions.push(Action::Keep);
            continue;
        }
        if lname.starts_with("docprops/thumbnail.") {
            actions.push(if scrub { Action::Remove } else { Action::Keep });
            if scrub {
                out.changed = true;
            }
            continue;
        }
        if lname.contains("/media/") && VECTOR_EXTENSIONS.contains(&ext.as_str()) {
            out.warnings.push(Warning::new(
                WarningCode::ImageNotProcessed,
                shown,
                "Image vectorielle non analysée.",
            ));
            actions.push(Action::Keep);
            continue;
        }
        if lname.contains("/media/") && RASTER_EXTENSIONS.contains(&ext.as_str()) {
            match mode {
                Mode::Extract => {
                    let bytes = pkg.read(i, budget)?;
                    out.media.push(MediaItem { name: shown, bytes });
                    actions.push(Action::Keep);
                }
                Mode::Transform { media: Some(m), .. } => {
                    let bytes = pkg.read(i, budget)?;
                    match m.apply(&shown, &bytes) {
                        Some(new) if new != bytes => {
                            out.changed = true;
                            actions.push(Action::Replace(new));
                        }
                        _ => actions.push(Action::Keep),
                    }
                }
                Mode::Transform { .. } => actions.push(Action::Keep),
            }
            continue;
        }
        if lname.ends_with("vbaproject.bin") {
            out.warnings.push(Warning::new(
                WarningCode::MacroPresent,
                shown,
                "Macros VBA présentes, non analysées.",
            ));
            actions.push(Action::Keep);
            continue;
        }
        if lname.contains("/embeddings/") || lname.contains("/activex/") {
            match flavor_for(&ext) {
                Some(f) if lname.contains("/embeddings/") => {
                    if depth >= MAX_EMBED_DEPTH {
                        return Err(FormatError::corrupt(shown, "imbrication excessive"));
                    }
                    let bytes = pkg.read(i, budget)?;
                    let inner = run(&bytes, f, mode, depth + 1, &format!("{shown}/"), budget)?;
                    out.blocks.extend(inner.blocks);
                    out.media.extend(inner.media);
                    out.warnings.extend(inner.warnings);
                    out.count += inner.count;
                    match inner.bytes {
                        Some(b) if inner.changed => {
                            out.changed = true;
                            actions.push(Action::Replace(b));
                        }
                        _ => actions.push(Action::Keep),
                    }
                }
                _ => {
                    if ext == "bin" || lname.contains("/embeddings/") {
                        out.warnings.push(Warning::new(
                            WarningCode::EmbeddedBinary,
                            shown,
                            "Objet embarqué binaire non analysé.",
                        ));
                    }
                    actions.push(Action::Keep);
                }
            }
            continue;
        }
        if !is_xml_part(name) {
            actions.push(Action::Keep);
            continue;
        }
        let bytes = pkg.read(i, budget)?;
        let (src, bom) = part_text(&bytes, &shown)?;
        let cx = PartCtx {
            label: shown.clone(),
            role: part_role(name),
            excel: flavor == Flavor::Excel,
            xlsx: &env,
            op: mode.op(),
        };
        let res: PartOut = process(src, &cx)?;
        out.warnings.extend(res.warnings);
        out.count += res.count;
        used_shared.extend(res.used_shared);
        let mut blocks = res.blocks;
        if name.starts_with("xl/worksheets/") {
            fill_column_headers(&mut blocks, &shown);
        }
        out.blocks.extend(blocks);
        if res.patches.is_empty() {
            actions.push(Action::Keep);
        } else {
            let mut new = bom.to_vec();
            new.extend_from_slice(apply_patches(src, res.patches).as_bytes());
            if new == bytes {
                actions.push(Action::Keep);
            } else {
                out.changed = true;
                actions.push(Action::Replace(new));
            }
        }
    }
    if matches!(mode, Mode::Extract) {
        let part = format!("{prefix}xl/sharedStrings.xml");
        for (i, s) in env.shared.iter().enumerate() {
            if !used_shared.contains(&i) && !s.trim().is_empty() {
                out.blocks.push(TextBlock {
                    text: s.clone(),
                    ctx: BlockContext::new(part.clone(), BlockRole::Cell),
                });
            }
        }
    }
    if matches!(mode, Mode::Transform { .. }) {
        out.bytes = Some(pkg.write(&actions)?);
    }
    Ok(out)
}

/// `column_header` de chaque cellule : texte de la première ligne renseignée
/// de sa colonne (les autres lignes seulement).
fn fill_column_headers(blocks: &mut [TextBlock], part: &str) {
    let prefix = format!("{part}!");
    let mut headers: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let mut refs = Vec::with_capacity(blocks.len());
    for b in blocks.iter() {
        let r = b
            .ctx
            .part
            .strip_prefix(&prefix)
            .filter(|_| b.ctx.role == BlockRole::Cell)
            .and_then(split_ref);
        if let Some((col, row)) = &r {
            let e = headers.entry(col.clone()).or_insert((*row, b.text.clone()));
            if *row < e.0 {
                *e = (*row, b.text.clone());
            }
        }
        refs.push(r);
    }
    for (b, r) in blocks.iter_mut().zip(refs) {
        if let Some((col, row)) = r
            && let Some((hrow, text)) = headers.get(&col)
            && *hrow < row
        {
            b.ctx.column_header = Some(text.clone());
        }
    }
}

/// Lecture exhaustive : blocs recollés, tous les nœuds de texte, toutes les
/// valeurs d'attributs, noms des entrées, fichiers embarqués.
fn deep(
    input: &[u8],
    flavor: Flavor,
    depth: usize,
    prefix: &str,
    budget: &mut u64,
) -> Result<Vec<TextBlock>, FormatError> {
    let mut pkg = Package::open(input)?;
    let mut blocks = Vec::new();
    let names: Vec<String> = pkg.entries.iter().map(|e| e.name.clone()).collect();
    push_chunks(
        &mut blocks,
        &format!("{prefix}(noms des entrées)"),
        names.iter().map(|n| x_decode(n)).collect(),
    );
    let env = XlsxEnv::default();
    let entries = pkg.entries.clone();
    for (i, entry) in entries.iter().enumerate() {
        if entry.is_dir {
            continue;
        }
        let name = entry.name.as_str();
        let lname = name.to_ascii_lowercase();
        let ext = ext_of(name);
        let shown = format!("{prefix}{}", label(name, i));
        if lname.contains("/embeddings/")
            && let Some(f) = flavor_for(&ext)
        {
            if depth >= MAX_EMBED_DEPTH {
                return Err(FormatError::corrupt(shown, "imbrication excessive"));
            }
            let bytes = pkg.read(i, budget)?;
            blocks.extend(deep(&bytes, f, depth + 1, &format!("{shown}/"), budget)?);
            continue;
        }
        if !matches!(ext.as_str(), "xml" | "rels" | "vml") {
            continue;
        }
        let bytes = pkg.read(i, budget)?;
        let src =
            String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes))
                .into_owned();
        let cx = PartCtx {
            label: shown.clone(),
            role: part_role(name),
            excel: flavor == Flavor::Excel,
            xlsx: &env,
            op: Op::Extract,
        };
        if let Ok(res) = process(&src, &cx) {
            blocks.extend(res.blocks.into_iter().map(|mut b| {
                b.ctx.role = BlockRole::Other;
                b
            }));
            push_chunks(&mut blocks, &shown, res.shared);
        }
        let (texts, attrs) = all_strings(&src, &shown);
        let mut items = Vec::with_capacity(texts.len() + attrs.len());
        for t in texts.into_iter().chain(attrs) {
            let d = x_decode(&t);
            if d != t {
                items.push(d);
            }
            items.push(t);
        }
        push_chunks(&mut blocks, &shown, items);
    }
    Ok(blocks)
}

/// Regroupe des chaînes en blocs d'environ `DEEP_SCAN_CHUNK` octets.
fn push_chunks(blocks: &mut Vec<TextBlock>, part: &str, items: Vec<String>) {
    let mut cur = String::new();
    for s in items {
        if !cur.is_empty() && cur.len() + s.len() > DEEP_SCAN_CHUNK {
            blocks.push(TextBlock {
                text: std::mem::take(&mut cur),
                ctx: BlockContext::new(part, BlockRole::Other),
            });
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(&s);
    }
    if !cur.trim().is_empty() {
        blocks.push(TextBlock {
            text: cur,
            ctx: BlockContext::new(part, BlockRole::Other),
        });
    }
}
