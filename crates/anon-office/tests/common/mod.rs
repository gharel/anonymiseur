//! Outils partagés des tests d'intégration.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{EntityKind, Handler, Session, Source, TextBlock};
use anon_replace::{Replacer, SessionExt};
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

pub fn exemples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exemples")
}

pub fn read_example(name: &str) -> Vec<u8> {
    std::fs::read(exemples().join(name)).expect("exemple illisible")
}

/// (type, valeur) non OCR d'un fichier d'`attendu.json`.
pub fn expected(file: &str) -> Vec<(EntityKind, String)> {
    let raw = std::fs::read_to_string(exemples().join("attendu.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let files = v["fichiers"].as_array().unwrap();
    let f = files
        .iter()
        .find(|f| f["fichier"] == file)
        .unwrap_or_else(|| panic!("{file} absent d'attendu.json"));
    f["entites"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["ocr"] != true)
        .map(|e| {
            let kind = EntityKind::from_prefix(e["type"].as_str().unwrap()).unwrap();
            (kind, e["valeur"].as_str().unwrap().to_string())
        })
        .collect()
}

pub fn session_of(values: &[(EntityKind, String)]) -> Session {
    let mut s = Session::new();
    for (k, v) in values {
        s.intern(*k, v, Source::Manual);
    }
    s
}

/// Vrai si un bloc contient la valeur (littéralement ou selon la
/// correspondance tolérante du `Replacer`).
pub fn value_in_blocks(kind: EntityKind, value: &str, blocks: &[TextBlock]) -> bool {
    if blocks.iter().any(|b| b.text.contains(value)) {
        return true;
    }
    let r = Replacer::new(&session_of(&[(kind, value.to_string())]));
    blocks.iter().any(|b| !r.matches(&b.text).is_empty())
}

/// Fuites : (emplacement, balise) de chaque valeur encore présente.
pub fn leaks(blocks: &[TextBlock], replacer: &Replacer) -> Vec<String> {
    let mut out = Vec::new();
    for b in blocks {
        for m in replacer.matches(&b.text) {
            out.push(format!(
                "{} : {} « {} »",
                b.ctx.part,
                m.tag,
                &b.text[m.start..m.end]
            ));
        }
    }
    out
}

pub fn zip_names(bytes: &[u8]) -> Vec<String> {
    let z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    z.file_names().map(|n| n.unwrap().into_owned()).collect()
}

/// Nom → (contenu décompressé, données compressées brutes, méthode).
pub fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>, Vec<u8>, zip::CompressionMethod)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    for i in 0..z.len() {
        let (name, data, method) = {
            let mut f = z.by_index(i).unwrap();
            let mut d = Vec::new();
            f.read_to_end(&mut d).unwrap();
            (f.name().unwrap().into_owned(), d, f.compression())
        };
        let mut raw = Vec::new();
        z.by_index_raw(i).unwrap().read_to_end(&mut raw).unwrap();
        out.push((name, data, raw, method));
    }
    out
}

pub fn part(bytes: &[u8], name: &str) -> String {
    let e = zip_entries(bytes);
    let (_, d, _, _) = e
        .iter()
        .find(|(n, ..)| n == name)
        .unwrap_or_else(|| panic!("{name} absent"));
    String::from_utf8(d.clone()).unwrap()
}

/// Vérifie que toutes les parties XML sont bien formées.
pub fn assert_well_formed(bytes: &[u8]) {
    for (name, data, _, _) in zip_entries(bytes) {
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        let s = std::str::from_utf8(&data).unwrap_or_else(|_| panic!("{name} non UTF-8"));
        let mut r = quick_xml::Reader::from_str(s);
        let mut depth = 0i64;
        loop {
            match r.read_event() {
                Ok(quick_xml::events::Event::Start(_)) => depth += 1,
                Ok(quick_xml::events::Event::End(_)) => depth -= 1,
                Ok(quick_xml::events::Event::Empty(e)) => {
                    // Attributs lisibles et uniques.
                    let mut seen = std::collections::HashSet::new();
                    for a in e.attributes() {
                        let a = a.unwrap_or_else(|_| panic!("{name} : attribut illisible"));
                        assert!(
                            seen.insert(a.key.0.to_string()),
                            "{name} : attribut en double"
                        );
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{name} mal formé : {e}"),
            }
        }
        assert_eq!(depth, 0, "{name} : éléments non fermés");
    }
}

/// Fabrique un zip (méthode Deflate) à partir de (nom, contenu).
pub fn make_zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, c) in entries {
        w.start_file(
            *n,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// Blocs d'extraction, sans les métadonnées retirées (`part` marqué `#`).
pub fn kept_blocks(h: &dyn Handler, bytes: &[u8]) -> Vec<TextBlock> {
    h.extract(bytes)
        .unwrap()
        .blocks
        .into_iter()
        .filter(|b| !b.ctx.part.contains('#'))
        .collect()
}
