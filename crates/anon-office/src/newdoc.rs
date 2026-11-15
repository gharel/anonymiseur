//! Document Word neuf et minimal : un paragraphe par ligne de texte, sans
//! mise en forme. Sert à enregistrer la réponse restaurée de l'IA en `.docx`.
//!
//! Trois parties seulement : `[Content_Types].xml`, `_rels/.rels` et
//! `word/document.xml`. Word, LibreOffice et le gestionnaire `DocxHandler`
//! l'ouvrent.

use crate::xml::escape_text;
use anon_core::FormatError;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

const DOCUMENT_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#;

const DOCUMENT_TAIL: &str = "<w:sectPr/></w:body></w:document>";

/// Vrai pour un caractère admis dans un document XML 1.0.
fn xml_char(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

/// Un paragraphe : le texte de la ligne, tabulations comprises.
fn paragraph(line: &str) -> String {
    let mut p = String::from("<w:p>");
    let clean: String = line
        .chars()
        .filter(|c| xml_char(*c) && *c != '\r')
        .collect();
    if !clean.is_empty() {
        p.push_str("<w:r>");
        for (i, part) in clean.split('\t').enumerate() {
            if i > 0 {
                p.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                p.push_str(r#"<w:t xml:space="preserve">"#);
                p.push_str(&escape_text(part));
                p.push_str("</w:t>");
            }
        }
        p.push_str("</w:r>");
    }
    p.push_str("</w:p>");
    p
}

/// `word/document.xml` d'un texte : une ligne, un paragraphe.
fn document_xml(text: &str) -> String {
    let mut xml = String::from(DOCUMENT_HEAD);
    for line in text.split('\n') {
        xml.push_str(&paragraph(line));
    }
    xml.push_str(DOCUMENT_TAIL);
    xml
}

/// Document Word (`.docx`) qui contient `text`, une ligne par paragraphe. Les
/// caractères interdits en XML sont retirés.
pub fn text_to_docx(text: &str) -> Result<Vec<u8>, FormatError> {
    let werr = |_| FormatError::Other("écriture du document Word impossible".into());
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", CONTENT_TYPES.to_owned()),
        ("_rels/.rels", RELS.to_owned()),
        ("word/document.xml", document_xml(text)),
    ] {
        w.start_file(name, opts).map_err(werr)?;
        w.write_all(body.as_bytes())
            .map_err(|_| FormatError::Other("écriture du document Word impossible".into()))?;
    }
    let cur = w.finish().map_err(werr)?;
    Ok(cur.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::Handler;
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::io::Read;

    const TEXTE: &str = "# Réponse\n\nHélène Kaloï & Pierre <Garnier>\tà Dumbéa.\r\nFin\u{1}.";

    /// Lit toutes les parties du paquet.
    fn parts(bytes: &[u8]) -> Vec<(String, String)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut f = zip.by_index(i).unwrap();
                let mut s = String::new();
                f.read_to_string(&mut s).unwrap();
                (f.name().unwrap().to_string(), s)
            })
            .collect()
    }

    #[test]
    fn the_package_has_three_well_formed_parts() {
        let bytes = text_to_docx(TEXTE).unwrap();
        let parts = parts(&bytes);
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["[Content_Types].xml", "_rels/.rels", "word/document.xml"]
        );
        for (name, xml) in &parts {
            let mut r = Reader::from_str(xml);
            let mut depth = 0i32;
            loop {
                match r.read_event() {
                    Ok(Event::Start(_)) => depth += 1,
                    Ok(Event::End(_)) => depth -= 1,
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(e) => panic!("{name} : XML mal formé ({e})"),
                }
                assert!(depth >= 0, "{name}");
            }
            assert_eq!(depth, 0, "{name} : balises non refermées");
        }
    }

    #[test]
    fn the_word_handler_reads_the_text_back() {
        let bytes = text_to_docx(TEXTE).unwrap();
        let ex = crate::DocxHandler.extract(&bytes).unwrap();
        let text: Vec<&str> = ex.blocks.iter().map(|b| b.text.as_str()).collect();
        assert!(text.contains(&"# Réponse"), "{text:?}");
        assert!(
            text.iter()
                .any(|t| t.contains("Hélène Kaloï & Pierre <Garnier>") && t.contains("Dumbéa")),
            "{text:?}"
        );
        assert!(
            text.contains(&"Fin."),
            "caractère de contrôle retiré : {text:?}"
        );
    }

    #[test]
    fn empty_text_gives_an_empty_document() {
        let bytes = text_to_docx("").unwrap();
        let doc = &parts(&bytes)[2].1;
        assert!(doc.contains("<w:body><w:p></w:p><w:sectPr/>"), "{doc}");
    }
}
