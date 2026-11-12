//! Lecture et réécriture d'un paquet zip (OOXML aujourd'hui, ODF ensuite).
//!
//! Les entrées non modifiées sont recopiées brutes (données compressées
//! comprises), dans le même ordre ; les entrées modifiées sont réécrites avec
//! la même méthode de compression et la même date.

use crate::limits::{MAX_ENTRIES, MAX_ENTRY_UNCOMPRESSED};
use anon_core::FormatError;
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Signature d'un conteneur OLE (fichier Office chiffré ou ancien format binaire).
pub(crate) const OLE_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Entrée du paquet.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub name: String,
    pub is_dir: bool,
    method: CompressionMethod,
    modified: Option<DateTime>,
}

/// Sort d'une entrée lors de la réécriture.
#[derive(Debug, Clone)]
pub(crate) enum Action {
    Keep,
    Replace(Vec<u8>),
    Remove,
}

pub(crate) struct Package<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    pub entries: Vec<Entry>,
}

fn zip_err(e: ZipError) -> FormatError {
    match e {
        ZipError::UnsupportedArchive(m) if m == ZipError::PASSWORD_REQUIRED => {
            FormatError::Encrypted
        }
        ZipError::InvalidPassword => FormatError::Encrypted,
        _ => FormatError::corrupt("archive", "archive zip invalide"),
    }
}

/// Emplacement affichable d'une entrée : son nom s'il est ordinaire, sinon son rang.
pub(crate) fn label(name: &str, index: usize) -> String {
    let plain = name.len() <= 160
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-[]".contains(c));
    if plain {
        name.to_string()
    } else {
        format!("entrée n° {}", index.saturating_add(1))
    }
}

/// Refuse les noms qui sortiraient du dossier d'extraction.
fn safe_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    let b = name.as_bytes();
    if b.get(1) == Some(&b':') {
        return false;
    }
    !name.split(['/', '\\']).any(|c| c == "..")
}

impl<'a> Package<'a> {
    pub fn open(input: &'a [u8]) -> Result<Self, FormatError> {
        if input.starts_with(&OLE_SIGNATURE) {
            return Err(FormatError::Encrypted);
        }
        let mut archive = ZipArchive::new(Cursor::new(input)).map_err(zip_err)?;
        if archive.len() > MAX_ENTRIES {
            return Err(FormatError::corrupt("archive", "trop d'entrées"));
        }
        let mut entries = Vec::with_capacity(archive.len());
        let mut seen = HashSet::new();
        for i in 0..archive.len() {
            let f = archive.by_index_raw(i).map_err(zip_err)?;
            let name = f.name().map_err(zip_err)?.into_owned();
            if !safe_name(&name) {
                return Err(FormatError::corrupt(
                    label(&name, i),
                    "nom d'entrée dangereux",
                ));
            }
            if !seen.insert(name.clone()) {
                return Err(FormatError::corrupt(label(&name, i), "entrée en double"));
            }
            if f.encrypted() {
                return Err(FormatError::Encrypted);
            }
            if f.size() > MAX_ENTRY_UNCOMPRESSED {
                return Err(FormatError::corrupt(
                    label(&name, i),
                    "taille décompressée excessive",
                ));
            }
            entries.push(Entry {
                is_dir: f.is_dir(),
                method: f.compression(),
                modified: f.last_modified(),
                name,
            });
        }
        Ok(Self { archive, entries })
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name)
    }

    /// Décompresse une entrée en respectant les plafonds ; `budget` est le
    /// reste autorisé pour l'ensemble du fichier.
    pub fn read(&mut self, index: usize, budget: &mut u64) -> Result<Vec<u8>, FormatError> {
        let name = self
            .entries
            .get(index)
            .map(|e| label(&e.name, index))
            .unwrap_or_else(|| "archive".to_string());
        let f = self.archive.by_index(index).map_err(zip_err)?;
        let limit = MAX_ENTRY_UNCOMPRESSED.min(*budget);
        let mut buf = Vec::new();
        f.take(limit.saturating_add(1))
            .read_to_end(&mut buf)
            .map_err(|_| FormatError::corrupt(name.clone(), "données compressées illisibles"))?;
        let len = buf.len() as u64;
        if len > limit {
            return Err(FormatError::corrupt(name, "taille décompressée excessive"));
        }
        *budget = budget.saturating_sub(len);
        Ok(buf)
    }

    /// Écrit le paquet de sortie : une action par entrée, dans l'ordre.
    pub fn write(self, actions: &[Action]) -> Result<Vec<u8>, FormatError> {
        let Package {
            mut archive,
            entries,
        } = self;
        let werr = |_| FormatError::Other("écriture de l'archive impossible".into());
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (i, entry) in entries.iter().enumerate() {
            match actions.get(i).unwrap_or(&Action::Keep) {
                Action::Keep => {
                    let f = archive.by_index_raw(i).map_err(zip_err)?;
                    w.raw_copy_file(f).map_err(werr)?;
                }
                Action::Remove => {}
                Action::Replace(bytes) => {
                    let method = if entry.method == CompressionMethod::Stored {
                        CompressionMethod::Stored
                    } else {
                        CompressionMethod::Deflated
                    };
                    let mut o = SimpleFileOptions::default().compression_method(method);
                    if let Some(t) = entry.modified {
                        o = o.last_modified_time(t);
                    }
                    if bytes.len() as u64 >= u64::from(u32::MAX) {
                        o = o.large_file(true);
                    }
                    w.start_file(entry.name.clone(), o).map_err(werr)?;
                    w.write_all(bytes)
                        .map_err(|_| werr(ZipError::FileNotFound))?;
                }
            }
        }
        let comment: Box<[u8]> = archive.comment().into();
        if !comment.is_empty() {
            w.set_raw_comment(comment).map_err(werr)?;
        }
        let cur = w.finish().map_err(werr)?;
        Ok(cur.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (n, b, m) in entries {
            w.start_file(*n, SimpleFileOptions::default().compression_method(*m))
                .unwrap();
            w.write_all(b).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn rejects_dangerous_names() {
        for n in [
            "../evil.xml",
            "a/../../b",
            "/abs.xml",
            "C:/x.xml",
            "a\\..\\b",
        ] {
            let z = make(&[(n, b"x", CompressionMethod::Stored)]);
            assert!(Package::open(&z).is_err(), "{n}");
        }
    }

    #[test]
    fn ole_is_encrypted() {
        let mut b = OLE_SIGNATURE.to_vec();
        b.extend_from_slice(&[0; 600]);
        assert!(matches!(Package::open(&b), Err(FormatError::Encrypted)));
    }

    #[test]
    fn budget_is_enforced() {
        let big = vec![b'a'; 10_000];
        let z = make(&[("a.xml", &big, CompressionMethod::Deflated)]);
        let mut p = Package::open(&z).unwrap();
        let mut budget = 100;
        assert!(p.read(0, &mut budget).is_err());
        let mut budget = 1_000_000;
        assert_eq!(p.read(0, &mut budget).unwrap().len(), 10_000);
        assert_eq!(budget, 990_000);
    }

    #[test]
    fn rewrite_keeps_order_and_method() {
        let z = make(&[
            ("b.xml", b"<b/>", CompressionMethod::Stored),
            ("a.xml", b"<a/>", CompressionMethod::Deflated),
            ("c.bin", b"ccc", CompressionMethod::Deflated),
        ]);
        let p = Package::open(&z).unwrap();
        let out = p
            .write(&[
                Action::Replace(b"<b>1</b>".to_vec()),
                Action::Keep,
                Action::Remove,
            ])
            .unwrap();
        let mut p2 = Package::open(&out).unwrap();
        let names: Vec<_> = p2.entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["b.xml", "a.xml"]);
        assert_eq!(p2.entries[0].method, CompressionMethod::Stored);
        assert_eq!(p2.entries[1].method, CompressionMethod::Deflated);
        let mut budget = 1000;
        assert_eq!(p2.read(0, &mut budget).unwrap(), b"<b>1</b>");
    }
}
