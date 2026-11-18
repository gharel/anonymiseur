//! Conventions d'empaquetage OOXML (Open Packaging Conventions) : relations,
//! résolution des cibles, types de contenu déclarés, et classement des parties
//! que l'on ne sait pas lire comme du XML.

use crate::ooxml::part_text;
use crate::package::Package;
use crate::xml::{Item, Walker};
use anon_core::FormatError;
use std::collections::{HashMap, HashSet};

/// Partie des types de contenu.
pub(crate) const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
/// Relations à la racine du paquet.
pub(crate) const ROOT_RELS_PART: &str = "_rels/.rels";

/// Fin du type de relation du document principal.
const REL_OFFICE_DOCUMENT: &str = "/officedocument";
/// Fin du type de relation de la table des chaînes partagées d'un classeur.
const REL_SHARED_STRINGS: &str = "/sharedstrings";
/// Fin du type de relation d'un contenu importé (`w:altChunk`).
const REL_ALT_CHUNK: &str = "/afchunk";

/// Extensions de parties binaires reconnues : recopiées sans analyse textuelle
/// quand aucun type de contenu ne dit le contraire.
const BINARY_EXTENSIONS: &[&str] = &[
    // Images.
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "bmp", "dib", "tif", "tiff", "webp", "wdp", "hdp",
    "jxr", "ico", "pict", "pct", "heic", "heif", "avif", "emf", "wmf", "emz", "wmz", "svm",
    // Polices.
    "odttf", "fntdata", "ttf", "otf", "ttc", "woff", "woff2", "eot",
    // Sons, vidéos, modèles 3D.
    "mp3", "mp4", "m4a", "m4v", "wav", "wma", "wmv", "avi", "mov", "mpg", "mpeg", "mid", "midi",
    "aif", "aiff", "aifc", "au", "snd", "ogg", "oga", "ogv", "webm", "asf", "3gp", "mkv", "flac",
    "aac", "glb",
    // Binaires Office (imprimante, macros, barres d'outils, propriétés), signature.
    "bin", "sigs",
];

/// Préfixes de types de contenu binaires.
const BINARY_TYPE_PREFIXES: &[&str] = &["image/", "audio/", "video/", "font/", "model/"];

/// Fragments de types `application/…` binaires connus dans les paquets Office.
const BINARY_APPLICATION_TOKENS: &[&str] = &[
    "octet-stream",
    "font",
    "opentype",
    "vbaproject",
    "activex",
    "oleobject",
    "printersettings",
    "digital-signature-origin",
    "attachedtoolbars",
    "customproperty",
    "msmetafile",
    "ms-photo",
];

/// Types de contenu textuels qui ne sont pas du XML lisible ici (HTML, MHT,
/// RTF, texte brut…).
const TEXT_TYPE_PREFIXES: &[&str] = &["text/", "message/", "multipart/"];
const TEXT_APPLICATION_TYPES: &[&str] = &[
    "application/rtf",
    "application/x-rtf",
    "application/xhtml+xml",
    "application/html",
    "application/json",
    "application/javascript",
    "application/x-javascript",
    "application/ecmascript",
];

/// Relation d'une partie `.rels`.
#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub id: String,
    pub ty: String,
    pub target: String,
    pub external: bool,
}

/// Nom local d'un élément canonique (`rel:Relationship`, `{uri}Default`, `Override`).
fn local_name(name: &str) -> &str {
    let name = name.rsplit_once('}').map_or(name, |(_, l)| l);
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// Relations déclarées dans une partie `.rels`.
pub(crate) fn relationships(xml: &str, part: &str) -> Result<Vec<Rel>, FormatError> {
    let mut out = Vec::new();
    let mut w = Walker::new(xml, part);
    while let Some(item) = w.next_item()? {
        if let Item::Start(t) | Item::Empty(t) = item
            && local_name(&t.name) == "Relationship"
            && let (Some(ty), Some(target)) = (t.attr_value("Type"), t.attr_value("Target"))
        {
            out.push(Rel {
                id: t.attr_value("Id").unwrap_or_default().to_string(),
                ty: ty.to_string(),
                target: target.to_string(),
                external: t
                    .attr_value("TargetMode")
                    .is_some_and(|m| m.trim().eq_ignore_ascii_case("External")),
            });
        }
    }
    Ok(out)
}

/// Dossier (barre finale comprise, vide à la racine) de la partie source
/// d'une partie de relations : `word/_rels/document.xml.rels` → `word/`.
pub(crate) fn rels_source_dir(rels_part: &str) -> &str {
    match rels_part.rsplit_once("_rels/") {
        Some((dir, _)) => dir,
        None => dir_of(rels_part),
    }
}

/// Partie de relations d'une partie : `xl/workbook.xml` → `xl/_rels/workbook.xml.rels`.
pub(crate) fn rels_part_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Dossier d'une partie, barre finale comprise (`xl/workbook.xml` → `xl/`).
pub(crate) fn dir_of(part: &str) -> &str {
    match part.rsplit_once('/') {
        Some((d, _)) => part.get(..d.len().saturating_add(1)).unwrap_or(""),
        None => "",
    }
}

/// Décodage des `%XX` d'une cible (URI) ; laissée telle quelle si invalide.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        if c == b'%'
            && let Some(hex) = s.get(i.saturating_add(1)..i.saturating_add(3))
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i = i.saturating_add(3);
            continue;
        }
        out.push(c);
        i = i.saturating_add(1);
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Nom de partie désigné par une cible de relation : absolue (`/xl/x.xml`) ou
/// relative au dossier `base_dir` (`../media/a.png`), segments `.` et `..`
/// résolus, sans barre initiale.
pub(crate) fn resolve_target(base_dir: &str, target: &str) -> String {
    let target = target.split(['#', '?']).next().unwrap_or("");
    let target = target.replace('\\', "/");
    let joined = match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None => format!("{base_dir}{target}"),
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

/// Rang d'une partie : nom exact, sinon sans tenir compte de la casse ni des
/// `%XX` (les noms de parties OPC sont insensibles à la casse).
pub(crate) fn find_part(pkg: &Package<'_>, name: &str) -> Option<usize> {
    if let Some(i) = pkg.index_of(name) {
        return Some(i);
    }
    let wanted = percent_decode(name).to_ascii_lowercase();
    pkg.entries
        .iter()
        .position(|e| !e.is_dir && percent_decode(&e.name).to_ascii_lowercase() == wanted)
}

/// Lit une partie XML par son rang : texte sans BOM.
fn read_text(
    pkg: &mut Package<'_>,
    index: usize,
    shown: &str,
    budget: &mut u64,
) -> Result<String, FormatError> {
    let bytes = pkg.read(index, budget)?;
    let (s, _) = part_text(&bytes, shown)?;
    Ok(s.to_string())
}

/// Relations d'une partie (vide si elle n'en a pas).
pub(crate) fn part_relationships(
    pkg: &mut Package<'_>,
    part: &str,
    budget: &mut u64,
) -> Result<Vec<Rel>, FormatError> {
    let rels_name = rels_part_of(part);
    let Some(i) = find_part(pkg, &rels_name) else {
        return Ok(Vec::new());
    };
    let src = read_text(pkg, i, &rels_name, budget)?;
    relationships(&src, &rels_name)
}

/// Partie principale d'un classeur Excel : cible de la relation `officeDocument`
/// de `_rels/.rels`, sinon `xl/workbook.xml`. Nom réel dans le paquet.
pub(crate) fn workbook_part(
    pkg: &mut Package<'_>,
    budget: &mut u64,
) -> Result<Option<String>, FormatError> {
    let rels = match find_part(pkg, ROOT_RELS_PART) {
        Some(i) => {
            let src = read_text(pkg, i, ROOT_RELS_PART, budget)?;
            relationships(&src, ROOT_RELS_PART)?
        }
        None => Vec::new(),
    };
    let declared = rels
        .iter()
        .filter(|r| !r.external && r.ty.to_ascii_lowercase().ends_with(REL_OFFICE_DOCUMENT))
        .find_map(|r| find_part(pkg, &resolve_target("", &r.target)));
    let found = declared.or_else(|| find_part(pkg, "xl/workbook.xml"));
    Ok(found.and_then(|i| pkg.entries.get(i).map(|e| e.name.clone())))
}

/// Table des chaînes partagées d'un classeur : cible de la relation
/// `sharedStrings` du classeur, sinon `xl/sharedStrings.xml` (casse
/// indifférente). Nom réel dans le paquet.
pub(crate) fn shared_strings_part(
    pkg: &mut Package<'_>,
    workbook: Option<&str>,
    budget: &mut u64,
) -> Result<Option<String>, FormatError> {
    let mut found = None;
    if let Some(wb) = workbook {
        let base = dir_of(wb).to_string();
        for r in part_relationships(pkg, wb, budget)? {
            if !r.external && r.ty.to_ascii_lowercase().ends_with(REL_SHARED_STRINGS) {
                found = find_part(pkg, &resolve_target(&base, &r.target));
                if found.is_some() {
                    break;
                }
            }
        }
    }
    let found = found.or_else(|| find_part(pkg, "xl/sharedStrings.xml"));
    Ok(found.and_then(|i| pkg.entries.get(i).map(|e| e.name.clone())))
}

/// Type de contenu sans paramètres (`; charset=…`), en minuscules.
fn base_type(ct: &str) -> String {
    ct.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Sort d'une partie dont le nom ne dit pas qu'elle est du XML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartKind {
    /// Déclarée comme XML (type `…+xml`) : analysée comme les parties `.xml`.
    Xml,
    /// Binaire reconnu (image, police, son…) : recopié tel quel.
    Binary,
    /// Texte non XML (HTML, MHT, RTF…) ou partie inconnue : non analysable.
    Unreadable,
}

/// Types de contenu et contenus importés (`altChunk`) d'un paquet OOXML.
#[derive(Debug, Default)]
pub(crate) struct PackageInfo {
    /// Extension en minuscules → type de contenu.
    defaults: HashMap<String, String>,
    /// Nom de partie en minuscules, sans barre initiale → type de contenu.
    overrides: HashMap<String, String>,
    /// Parties cibles d'une relation `aFChunk`, en minuscules.
    alt_chunks: HashSet<String>,
}

impl PackageInfo {
    /// Lit `[Content_Types].xml` et toutes les relations du paquet.
    pub fn read(pkg: &mut Package<'_>, budget: &mut u64) -> Result<Self, FormatError> {
        let mut info = Self::default();
        if let Some(i) = find_part(pkg, CONTENT_TYPES_PART) {
            let src = read_text(pkg, i, CONTENT_TYPES_PART, budget)?;
            let mut w = Walker::new(&src, CONTENT_TYPES_PART);
            while let Some(item) = w.next_item()? {
                let (Item::Start(t) | Item::Empty(t)) = item else {
                    continue;
                };
                let Some(ct) = t.attr_value("ContentType") else {
                    continue;
                };
                match local_name(&t.name) {
                    "Default" => {
                        if let Some(ext) = t.attr_value("Extension") {
                            let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
                            info.defaults.insert(ext, ct.to_string());
                        }
                    }
                    "Override" => {
                        if let Some(p) = t.attr_value("PartName") {
                            let key = percent_decode(resolve_target("", p.trim()).as_str())
                                .to_ascii_lowercase();
                            info.overrides.insert(key, ct.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        let rels: Vec<(usize, String)> = pkg
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_dir && e.name.to_ascii_lowercase().ends_with(".rels"))
            .map(|(i, e)| (i, e.name.clone()))
            .collect();
        for (i, name) in rels {
            let src = read_text(pkg, i, &name, budget)?;
            let base = rels_source_dir(&name).to_string();
            for r in relationships(&src, &name)? {
                if !r.external && r.ty.to_ascii_lowercase().ends_with(REL_ALT_CHUNK) {
                    let target = resolve_target(&base, &r.target);
                    info.alt_chunks
                        .insert(percent_decode(&target).to_ascii_lowercase());
                }
            }
        }
        Ok(info)
    }

    /// Vrai si la partie est la cible d'une relation `aFChunk` (contenu
    /// importé que Word affiche dans le corps du document).
    pub fn is_alt_chunk(&self, name: &str) -> bool {
        !self.alt_chunks.is_empty()
            && self
                .alt_chunks
                .contains(&percent_decode(name).to_ascii_lowercase())
    }

    /// Type de contenu déclaré : `Override` de la partie, sinon `Default` de
    /// son extension.
    pub fn content_type(&self, name: &str) -> Option<&str> {
        let key = percent_decode(name).to_ascii_lowercase();
        if let Some(ct) = self.overrides.get(&key) {
            return Some(ct);
        }
        let ext = key.rsplit_once('.').map(|(_, e)| e)?;
        if ext.contains('/') {
            return None;
        }
        self.defaults.get(ext).map(String::as_str)
    }

    /// Classe une partie dont l'extension n'est ni `.xml` ni `.rels` (ni VML).
    pub fn classify(&self, name: &str, ext: &str) -> PartKind {
        let ct = self.content_type(name).map(base_type);
        classify(ct.as_deref(), ext)
    }
}

/// Classement d'une partie hors XML d'après son type de contenu déclaré, puis
/// son extension.
fn classify(ct: Option<&str>, ext: &str) -> PartKind {
    if let Some(ct) = ct {
        if TEXT_APPLICATION_TYPES.contains(&ct) {
            return PartKind::Unreadable;
        }
        if ct == "application/xml" || ct == "text/xml" || ct.ends_with("+xml") {
            return PartKind::Xml;
        }
        if TEXT_TYPE_PREFIXES.iter().any(|p| ct.starts_with(p)) {
            return PartKind::Unreadable;
        }
        if BINARY_TYPE_PREFIXES.iter().any(|p| ct.starts_with(p))
            || (ct.starts_with("application/")
                && BINARY_APPLICATION_TOKENS.iter().any(|t| ct.contains(t)))
        {
            return PartKind::Binary;
        }
    }
    if BINARY_EXTENSIONS.contains(&ext) {
        PartKind::Binary
    } else if ext == "rdf" {
        // Métadonnées RDF/XML, lues aussi par le contrôle de fuite.
        PartKind::Xml
    } else {
        PartKind::Unreadable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_resolved() {
        assert_eq!(
            resolve_target("xl/", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve_target("xl/", "/xl/worksheets/s.xml"),
            "xl/worksheets/s.xml"
        );
        assert_eq!(
            resolve_target("xl/", "../xl/./worksheets/s.xml"),
            "xl/worksheets/s.xml"
        );
        assert_eq!(resolve_target("", "word/document.xml"), "word/document.xml");
        assert_eq!(resolve_target("word/", "afchunk.mht#x"), "word/afchunk.mht");
        assert_eq!(rels_source_dir("word/_rels/document.xml.rels"), "word/");
        assert_eq!(rels_source_dir("_rels/.rels"), "");
        assert_eq!(
            rels_part_of("xl/workbook.xml"),
            "xl/_rels/workbook.xml.rels"
        );
        assert_eq!(dir_of("xl/workbook.xml"), "xl/");
        assert_eq!(dir_of("workbook.xml"), "");
        assert_eq!(percent_decode("a%20b%zz"), "a b%zz");
    }

    #[test]
    fn parts_are_classified() {
        use PartKind::*;
        assert_eq!(classify(Some("message/rfc822"), "mht"), Unreadable);
        assert_eq!(classify(Some("text/html"), "png"), Unreadable);
        assert_eq!(classify(Some("multipart/related"), "mht"), Unreadable);
        assert_eq!(classify(Some("application/rtf"), "rtf"), Unreadable);
        assert_eq!(classify(Some("application/xhtml+xml"), "xhtml"), Unreadable);
        assert_eq!(
            classify(
                Some(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                ),
                "bin"
            ),
            Xml
        );
        assert_eq!(classify(Some("text/xml"), "dat"), Xml);
        assert_eq!(classify(Some("image/png"), "dat"), Binary);
        assert_eq!(
            classify(
                Some("application/vnd.openxmlformats-officedocument.obfuscatedFont"),
                "odttf"
            ),
            Binary
        );
        assert_eq!(classify(None, "bin"), Binary);
        assert_eq!(classify(None, "rdf"), Xml);
        assert_eq!(classify(None, "txt"), Unreadable);
        assert_eq!(classify(None, ""), Unreadable);
        assert_eq!(classify(Some("application/x-inconnu"), "dat"), Unreadable);
    }
}
