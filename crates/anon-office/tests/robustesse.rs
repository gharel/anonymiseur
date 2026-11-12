//! Robustesse : aucune panique, erreurs typées sur les fichiers piégés.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{FormatError, Handler, TransformOptions};
use anon_office::{
    DocxHandler, MAX_ENTRY_UNCOMPRESSED, PptxHandler, TextHandler, XlsxHandler, handler_for,
};
use anon_replace::Replacer;
use common::*;
use proptest::prelude::*;
use std::io::{Cursor, Write};

fn exercise(h: &dyn Handler, bytes: &[u8]) {
    let session = session_of(&[(anon_core::EntityKind::Nom, "Dupont".to_string())]);
    let r = Replacer::new(&session);
    let _ = h.extract(bytes);
    let _ = h.deep_scan(bytes);
    let _ = h.transform(bytes, &r, &TransformOptions::anonymize());
}

fn all_handlers() -> [&'static dyn Handler; 4] {
    [&TextHandler, &DocxHandler, &XlsxHandler, &PptxHandler]
}

const SAMPLES: &[&str] = &[
    "contrat-prestation.docx",
    "fichier-clients.xlsx",
    "presentation-projet.pptx",
    "notes.txt",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        for h in all_handlers() {
            exercise(h, &bytes);
        }
    }

    #[test]
    fn mutated_examples_never_panic(
        which in 0..SAMPLES.len(),
        flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12),
        cut in proptest::option::of(any::<prop::sample::Index>()),
    ) {
        let name = SAMPLES[which];
        let mut bytes = read_example(name);
        for (i, b) in flips {
            let at = i.index(bytes.len());
            bytes[at] = b;
        }
        if let Some(c) = cut {
            bytes.truncate(c.index(bytes.len()));
        }
        let h = handler_for(name.rsplit('.').next().unwrap()).unwrap();
        exercise(h, &bytes);
    }
}

#[test]
fn truncated_zip_is_error() {
    let input = read_example("contrat-prestation.docx");
    for len in [0, 10, input.len() / 2, input.len() - 10] {
        assert!(DocxHandler.extract(&input[..len]).is_err());
    }
}

#[test]
fn malformed_xml_is_corrupt() {
    let z = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("word/document.xml", "<w:document><w:body><w:p></w:body>"),
    ]);
    assert!(matches!(
        DocxHandler.extract(&z),
        Err(FormatError::Corrupt { .. })
    ));
}

#[test]
fn missing_part_is_corrupt() {
    let z = make_zip(&[("word/document.xml", "<a/>")]);
    match DocxHandler.extract(&z) {
        Err(FormatError::Corrupt { part, .. }) => assert_eq!(part, "[Content_Types].xml"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn dangerous_entry_name_is_rejected() {
    let z = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("../../evil.xml", "<a/>"),
    ]);
    assert!(matches!(
        XlsxHandler.extract(&z),
        Err(FormatError::Corrupt { .. })
    ));
}

#[test]
fn encrypted_container_is_reported() {
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.resize(4096, 0);
    for h in [
        &DocxHandler as &dyn Handler,
        &XlsxHandler as &dyn Handler,
        &PptxHandler as &dyn Handler,
    ] {
        assert!(matches!(h.extract(&ole), Err(FormatError::Encrypted)));
    }
}

#[test]
fn decompression_bomb_is_rejected() {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    w.start_file("[Content_Types].xml", o).unwrap();
    w.write_all(b"<Types/>").unwrap();
    w.start_file("_rels/.rels", o).unwrap();
    w.write_all(b"<Relationships/>").unwrap();
    w.start_file("word/document.xml", o).unwrap();
    let chunk = vec![b' '; 1 << 20];
    for _ in 0..(MAX_ENTRY_UNCOMPRESSED / (1 << 20) + 1) {
        w.write_all(&chunk).unwrap();
    }
    let z = w.finish().unwrap().into_inner();
    match DocxHandler.extract(&z) {
        Err(FormatError::Corrupt { reason, .. }) => {
            assert!(reason.contains("taille"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn error_messages_carry_no_user_content() {
    let z = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("word/document.xml", "<w:t>Jean Dupont</w:x>"),
    ]);
    let e = DocxHandler.extract(&z).unwrap_err().to_string();
    assert!(!e.contains("Dupont"), "{e}");
}
