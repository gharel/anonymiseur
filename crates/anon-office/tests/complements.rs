//! Compléments : restauration des types Excel, zones annexes, en-têtes en
//! transformation, OpenDocument.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{
    BlockContext, BlockRole, EntityKind::*, FormatError, Handler, Replacement, TextTransform,
    TransformOptions, WarningCode,
};
use anon_office::{CsvHandler, DocxHandler, OdfHandler, XlsxHandler};
use anon_replace::{Replacer, Restorer};
use common::*;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::sync::Mutex;

fn session(values: &[(anon_core::EntityKind, &str)]) -> anon_core::Session {
    let v: Vec<_> = values.iter().map(|(k, s)| (*k, s.to_string())).collect();
    session_of(&v)
}

/// Type de chaque cellule d'une feuille : `r` → (`t`, a une valeur `<v>`).
fn cell_types(sheet: &str) -> BTreeMap<String, (String, bool)> {
    let mut out = BTreeMap::new();
    let mut r = quick_xml::Reader::from_str(sheet);
    let mut cur: Option<(String, String)> = None;
    let mut has_v = false;
    loop {
        match r.read_event().unwrap() {
            quick_xml::events::Event::Start(e) if e.name().as_ref() == "c" => {
                let mut rr = String::new();
                let mut t = "n".to_string();
                for a in e.attributes() {
                    let a = a.unwrap();
                    match a.key.as_ref() {
                        "r" => rr = a.value.to_string(),
                        "t" => t = a.value.to_string(),
                        _ => {}
                    }
                }
                cur = Some((rr, t));
                has_v = false;
            }
            quick_xml::events::Event::Start(e) if e.name().as_ref() == "v" => has_v = true,
            quick_xml::events::Event::End(e) if e.name().as_ref() == "c" => {
                if let Some((rr, t)) = cur.take() {
                    out.insert(rr, (t, has_v));
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    out
}

// 1. Restauration des cellules Excel ------------------------------------------

#[test]
fn restored_excel_cells_get_back_their_type() {
    let input = read_example("fichier-clients.xlsx");
    let s = session_of(&expected("fichier-clients.xlsx"));
    let anon = XlsxHandler
        .transform(&input, &Replacer::new(&s), &TransformOptions::anonymize())
        .unwrap();
    let anon_types = cell_types(&part(&anon.bytes, "xl/worksheets/sheet1.xml"));
    assert_eq!(anon_types["G2"].0, "inlineStr");
    let back = XlsxHandler
        .transform(
            &anon.bytes,
            &Restorer::new(&s),
            &TransformOptions::restore(),
        )
        .unwrap();
    let before = cell_types(&part(&input, "xl/worksheets/sheet1.xml"));
    let after = cell_types(&part(&back.bytes, "xl/worksheets/sheet1.xml"));
    assert_eq!(before, after);
    assert!(part(&back.bytes, "xl/worksheets/sheet1.xml").contains("<v>33612345678</v>"));
}

#[test]
fn restored_excel_date_gets_back_its_serial() {
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
    let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="1"><v>45730</v></c><c r="B1" t="inlineStr"><is><t>06 12</t></is></c></row></sheetData></worksheet>"#;
    let input = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("xl/styles.xml", styles),
        ("xl/worksheets/sheet1.xml", sheet),
    ]);
    let s = session(&[(Date, "14/03/2025"), (Tel, "06 12")]);
    let anon = XlsxHandler
        .transform(&input, &Replacer::new(&s), &TransformOptions::anonymize())
        .unwrap();
    let back = XlsxHandler
        .transform(
            &anon.bytes,
            &Restorer::new(&s),
            &TransformOptions::restore(),
        )
        .unwrap();
    let out = part(&back.bytes, "xl/worksheets/sheet1.xml");
    assert!(
        out.contains(r#"<c r="A1" s="1" t="n"><v>45730</v></c>"#),
        "{out}"
    );
    // Texte non numérique : reste une chaîne en ligne.
    assert!(
        out.contains(r#"<c r="B1" t="inlineStr"><is><t xml:space="preserve">06 12</t></is></c>"#)
    );
}

// 2. Zones annexes --------------------------------------------------------------

#[test]
fn annex_zones_are_transformed() {
    let doc = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr="HYPERLINK &quot;mailto:jean.dupont@example.com&quot; \o &quot;Jean Dupont&quot;"><w:r><w:t>lien</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#;
    let item = r#"<?xml version="1.0"?><root xmlns="urn:exemple"><client nom="x">Jean Dupont</client><tel>281234</tel></root>"#;
    let chart = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:val><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>281234</c:v></c:pt><c:pt idx="1"><c:v>12</c:v></c:pt></c:numCache></c:numRef></c:val></c:chart></c:chartSpace>"#;
    let input = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("word/document.xml", doc),
        ("customXml/item1.xml", item),
        ("word/charts/chart1.xml", chart),
    ]);
    let s = session(&[
        (Personne, "Jean Dupont"),
        (Email, "jean.dupont@example.com"),
        (Tel, "281234"),
    ]);
    let r = Replacer::new(&s);
    let ex = DocxHandler.extract(&input).unwrap();
    for v in ["mailto:jean.dupont@example.com", "Jean Dupont"] {
        assert!(ex.blocks.iter().any(|b| b.text == v), "{v}");
    }
    let out = DocxHandler
        .transform(&input, &r, &TransformOptions::anonymize())
        .unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::UntouchedLocation && !w.detail.contains("281234"))
    );
    let d = part(&out.bytes, "word/document.xml");
    assert!(
        d.contains("HYPERLINK &quot;mailto:EMAIL_1@anon.invalid&quot;"),
        "{d}"
    );
    assert!(d.contains("\\o &quot;[PERSONNE_1]&quot;"));
    let c = part(&out.bytes, "customXml/item1.xml");
    assert!(
        c.contains("<client nom=\"x\">[PERSONNE_1]</client><tel>[TEL_1]</tel>"),
        "{c}"
    );
    let ch = part(&out.bytes, "word/charts/chart1.xml");
    assert!(
        ch.contains("<c:v>0</c:v>") && ch.contains("<c:v>12</c:v>"),
        "{ch}"
    );
    assert_well_formed(&out.bytes);
    let leaks = leaks(&DocxHandler.deep_scan(&out.bytes).unwrap(), &r);
    assert!(leaks.is_empty(), "{leaks:?}");
    // Restauration du lien et du champ.
    let back = DocxHandler
        .transform(&out.bytes, &Restorer::new(&s), &TransformOptions::restore())
        .unwrap();
    assert_eq!(
        part(&back.bytes, "word/document.xml"),
        doc,
        "le champ doit revenir à l'identique"
    );
}

#[test]
fn excel_phonetic_vml_and_pivot_numbers() {
    let shared = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Kaloï</t><rPh sb="0" eb="1"><t>Kaloï</t></rPh></si></sst>"#;
    let vml = r#"<xml xmlns:v="urn:schemas-microsoft-com:vml"><v:shape><v:textbox><div><font>Hélène </font><font>Kaloï</font></div></v:textbox></v:shape></xml>"#;
    let records = r#"<pivotCacheRecords xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><r><n v="281234"/><n v="3"/></r></pivotCacheRecords>"#;
    let input = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("xl/sharedStrings.xml", shared),
        ("xl/drawings/vmlDrawing1.vml", vml),
        ("xl/pivotCache/pivotCacheRecords1.xml", records),
    ]);
    let s = session(&[(Personne, "Hélène Kaloï"), (Nom, "Kaloï"), (Tel, "281234")]);
    let r = Replacer::new(&s);
    let ex = XlsxHandler.extract(&input).unwrap();
    assert!(ex.blocks.iter().any(|b| b.text == "Hélène Kaloï"));
    let out = XlsxHandler
        .transform(&input, &r, &TransformOptions::anonymize())
        .unwrap();
    let ss = part(&out.bytes, "xl/sharedStrings.xml");
    assert!(
        ss.contains(r#"<rPh sb="0" eb="1"><t xml:space="preserve">[NOM_1]</t></rPh>"#),
        "{ss}"
    );
    let v = part(&out.bytes, "xl/drawings/vmlDrawing1.vml");
    assert!(v.contains("<font>[PERSONNE_1]</font><font></font>"), "{v}");
    let p = part(&out.bytes, "xl/pivotCache/pivotCacheRecords1.xml");
    assert!(p.contains(r#"<n v="0"/><n v="3"/>"#), "{p}");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::UntouchedLocation)
    );
    let leaks = leaks(&XlsxHandler.deep_scan(&out.bytes).unwrap(), &r);
    assert!(leaks.is_empty(), "{leaks:?}");
}

#[test]
fn unreadable_vml_is_kept_with_warning() {
    let input = make_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("xl/drawings/vmlDrawing1.vml", "<xml><br><v:textbox>x</xml>"),
    ]);
    let s = session(&[(Nom, "Kaloï")]);
    let out = XlsxHandler
        .transform(&input, &Replacer::new(&s), &TransformOptions::anonymize())
        .unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::UnsupportedPart)
    );
}

// 5. En-têtes de colonne en transformation -------------------------------------

/// Enregistre l'en-tête vu pour chaque bloc ; ne remplace rien.
#[derive(Default)]
struct Recorder(Mutex<Vec<(String, String, Option<String>)>>);
impl TextTransform for Recorder {
    fn replacements(&self, text: &str, ctx: &BlockContext) -> Vec<Replacement> {
        if ctx.role == BlockRole::Cell {
            self.0.lock().unwrap().push((
                ctx.part.clone(),
                text.to_string(),
                ctx.column_header.clone(),
            ));
        }
        Vec::new()
    }
}

#[test]
fn column_header_is_given_during_transform() {
    for (h, file, part, text, header) in [
        (
            &XlsxHandler as &dyn Handler,
            "fichier-clients.xlsx",
            "xl/worksheets/sheet1.xml!D3",
            "33612345678",
            "Téléphone",
        ),
        (
            &OdfHandler as &dyn Handler,
            "planning.ods",
            "content.xml:feuille1!E2",
            "06 12 34 56 78",
            "Téléphone",
        ),
        (
            &CsvHandler as &dyn Handler,
            "export.csv",
            "ligne 2, colonne 1",
            "DUPONT",
            "Nom",
        ),
    ] {
        let rec = Recorder::default();
        h.transform(&read_example(file), &rec, &TransformOptions::restore())
            .unwrap();
        let seen = rec.0.lock().unwrap();
        assert!(
            seen.iter()
                .any(|(p, t, hd)| p == part && t == text && hd.as_deref() == Some(header)),
            "{file} : {:?}",
            seen.iter().take(12).collect::<Vec<_>>()
        );
        // Mêmes en-têtes qu'à l'extraction.
        let ex = h.extract(&read_example(file)).unwrap();
        let b = ex.blocks.iter().find(|b| b.ctx.part == part).unwrap();
        assert_eq!(b.ctx.column_header.as_deref(), Some(header));
    }
}

#[test]
fn csv_header_row_has_its_own_header() {
    let ex = CsvHandler.extract(&read_example("export.csv")).unwrap();
    let b = &ex.blocks[0];
    assert_eq!(
        (b.text.as_str(), b.ctx.column_header.as_deref()),
        ("Nom", Some("Nom"))
    );
}

// 6. OpenDocument ---------------------------------------------------------------

const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:calcext="urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0" xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0""#;

/// Paquet OpenDocument : `mimetype` en premier, non compressé.
fn odf_zip(mime: &str, parts: &[(&str, &str)], extra_manifest: &str) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    w.start_file("mimetype", stored).unwrap();
    w.write_all(mime.as_bytes()).unwrap();
    let manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest {NS}><manifest:file-entry manifest:full-path="/" manifest:media-type="{mime}"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="Thumbnails/thumbnail.png" manifest:media-type="image/png"/><manifest:file-entry manifest:full-path="Pictures/photo.png" manifest:media-type="image/png"/>{extra_manifest}</manifest:manifest>"#
    );
    w.start_file("META-INF/manifest.xml", deflated).unwrap();
    w.write_all(manifest.as_bytes()).unwrap();
    for (n, c) in parts {
        w.start_file(*n, deflated).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.start_file("Thumbnails/thumbnail.png", stored).unwrap();
    w.write_all(b"\x89PNG miniature").unwrap();
    w.start_file("Pictures/photo.png", stored).unwrap();
    w.write_all(b"\x89PNG photo").unwrap();
    w.finish().unwrap().into_inner()
}

fn odt_fixture() -> Vec<u8> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS}><office:body><office:text><text:tracked-changes><text:changed-region text:id="c1"><text:deletion><office:change-info><dc:creator>Martin Lefèvre</dc:creator><dc:date>2026-01-01T00:00:00</dc:date></office:change-info><text:p>Ancien contact Pierre Garnier</text:p></text:deletion></text:changed-region></text:tracked-changes><text:p>Bonjour <text:span>Jean</text:span><text:s/>Dupont,<text:s text:c="3"/>suite<text:tab/>fin<office:annotation><dc:creator>Hélène Kaloï</dc:creator><dc:date>2026-01-01</dc:date><text:p>Voir Pierre Garnier</text:p></office:annotation> <text:a xlink:href="mailto:jean.dupont@example.com">écrire</text:a></text:p><text:p>Expéditeur : <text:sender-lastname>Lefèvre</text:sender-lastname></text:p><draw:frame><svg:title>Photo de Pierre Garnier</svg:title></draw:frame></office:text></office:body></office:document-content>"#
    );
    let styles = format!(
        r#"<office:document-styles {NS}><office:master-styles><style:master-page style:name="S"><style:footer><text:p>Pied : Pierre Garnier</text:p></style:footer></style:master-page></office:master-styles></office:document-styles>"#
    );
    let meta = format!(
        r#"<office:document-meta {NS}><office:meta><meta:initial-creator>Martin Lefèvre</meta:initial-creator><dc:creator>Martin Lefèvre</dc:creator><dc:title>Dossier Pierre Garnier</dc:title><meta:keyword>Garnier</meta:keyword><meta:user-defined meta:name="Client">Pierre Garnier</meta:user-defined><meta:editing-cycles>7</meta:editing-cycles><meta:printed-by>Martin Lefèvre</meta:printed-by></office:meta></office:document-meta>"#
    );
    odf_zip(
        "application/vnd.oasis.opendocument.text",
        &[
            ("content.xml", &content),
            ("styles.xml", &styles),
            ("meta.xml", &meta),
        ],
        "",
    )
}

#[test]
fn odt_paragraphs_annotations_changes_and_metadata() {
    let input = odt_fixture();
    let ex = OdfHandler.extract(&input).unwrap();
    let find = |t: &str| ex.blocks.iter().find(|b| b.text == t);
    assert!(
        find("Bonjour Jean Dupont,   suite fin écrire").is_some(),
        "{:?}",
        ex.blocks
    );
    assert_eq!(
        find("Voir Pierre Garnier").unwrap().ctx.role,
        BlockRole::Comment
    );
    assert_eq!(
        find("Pied : Pierre Garnier").unwrap().ctx.role,
        BlockRole::Footer
    );
    assert!(find("Ancien contact Pierre Garnier").is_some());
    assert_eq!(
        find("mailto:jean.dupont@example.com").unwrap().ctx.role,
        BlockRole::Hyperlink
    );
    assert!(find("Photo de Pierre Garnier").is_some());
    for m in ["Martin Lefèvre", "Hélène Kaloï", "Lefèvre"] {
        assert!(
            ex.blocks
                .iter()
                .any(|b| b.text == m && b.ctx.role == BlockRole::Metadata),
            "{m}"
        );
    }
    assert!(ex.media.iter().any(|m| m.name == "Pictures/photo.png"));

    let s = session(&[
        (Personne, "Jean Dupont"),
        (Personne, "Pierre Garnier"),
        (Personne, "Martin Lefèvre"),
        (Personne, "Hélène Kaloï"),
        (Email, "jean.dupont@example.com"),
    ]);
    let r = Replacer::new(&s);
    struct Mask;
    impl anon_core::MediaTransform for Mask {
        fn apply(&self, name: &str, _: &[u8]) -> Option<Vec<u8>> {
            (name == "Pictures/photo.png").then(|| b"masque".to_vec())
        }
    }
    let opts = TransformOptions {
        media: Some(&Mask),
        ..TransformOptions::anonymize()
    };
    let out = OdfHandler.transform(&input, &r, &opts).unwrap();
    assert_well_formed(&out.bytes);
    let c = part(&out.bytes, "content.xml");
    assert!(
        c.contains("Bonjour <text:span>[PERSONNE_1]</text:span><text:s/>,<text:s text:c=\"3\"/>"),
        "{c}"
    );
    assert!(c.contains("xlink:href=\"mailto:EMAIL_"));
    assert!(c.contains("<dc:creator></dc:creator>"));
    assert!(c.contains("<text:sender-lastname></text:sender-lastname>"));
    let m = part(&out.bytes, "meta.xml");
    for frag in [
        "<meta:initial-creator></meta:initial-creator>",
        "<meta:user-defined meta:name=\"Client\"></meta:user-defined>",
        "<meta:editing-cycles></meta:editing-cycles>",
        "<meta:printed-by></meta:printed-by>",
        "<dc:title>Dossier [PERSONNE_",
    ] {
        assert!(m.contains(frag), "{frag} : {m}");
    }
    // Miniature retirée, manifeste à jour, image remplacée.
    let names = zip_names(&out.bytes);
    assert!(!names.iter().any(|n| n.starts_with("Thumbnails/")));
    assert!(!part(&out.bytes, "META-INF/manifest.xml").contains("Thumbnails"));
    let e = zip_entries(&out.bytes);
    assert_eq!(
        e.iter().find(|x| x.0 == "Pictures/photo.png").unwrap().1,
        b"masque"
    );
    assert_mimetype_first(&out.bytes);
    let leaks = leaks(&OdfHandler.deep_scan(&out.bytes).unwrap(), &r);
    assert!(leaks.is_empty(), "{leaks:?}");
}

/// `mimetype` : première entrée, non compressée, sans champ extra.
fn assert_mimetype_first(bytes: &[u8]) {
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 0, "méthode");
    let name_len = u16::from_le_bytes([bytes[26], bytes[27]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[28], bytes[29]]);
    assert_eq!(&bytes[30..30 + name_len], b"mimetype");
    assert_eq!(extra_len, 0, "champ extra");
}

fn ods_fixture() -> Vec<u8> {
    let content = format!(
        r#"<office:document-content {NS}><office:body><office:spreadsheet><table:table table:name="Clients Dupont"><table:table-row><table:table-cell office:value-type="string"><text:p>Nom</text:p></table:table-cell><table:table-cell table:number-columns-repeated="3"/><table:table-cell office:value-type="string"><text:p>Naissance</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Tel</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="1048576"/></table:table-row><table:table-row><table:table-cell office:value-type="string" calcext:value-type="string"><text:p>Dupont</text:p></table:table-cell><table:table-cell table:number-columns-repeated="3"/><table:table-cell office:value-type="date" office:date-value="1981-05-12" calcext:value-type="date"><text:p>12/05/1981</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="281234" calcext:value-type="float"><text:p>281234</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#
    );
    odf_zip(
        "application/vnd.oasis.opendocument.spreadsheet",
        &[("content.xml", &content)],
        "",
    )
}

#[test]
fn ods_cells_dates_repeats_and_round_trip() {
    let input = ods_fixture();
    let ex = OdfHandler.extract(&input).unwrap();
    let cell = |p: &str| {
        ex.blocks
            .iter()
            .find(|b| b.ctx.part == format!("content.xml:feuille1!{p}"))
            .unwrap_or_else(|| panic!("{p} : {:?}", ex.blocks))
    };
    assert_eq!(cell("A4").text, "Dupont");
    assert_eq!(cell("A4").ctx.column_header.as_deref(), Some("Nom"));
    assert_eq!(cell("E4").text, "12/05/1981");
    assert_eq!(cell("E4").ctx.column_header.as_deref(), Some("Naissance"));
    assert_eq!(cell("F4").ctx.column_header.as_deref(), Some("Tel"));

    let s = session(&[(Nom, "Dupont"), (DateNaiss, "12/05/1981"), (Tel, "281234")]);
    let r = Replacer::new(&s);
    let out = OdfHandler
        .transform(&input, &r, &TransformOptions::anonymize())
        .unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::SheetNameSensitive)
    );
    let c = part(&out.bytes, "content.xml");
    assert!(c.contains("table:name=\"Clients Dupont\""));
    assert!(
        c.contains(r#"<table:table-cell office:value-type="string" calcext:value-type="string"><text:p>[DATE_NAISS_1]</text:p>"#),
        "{c}"
    );
    assert!(c.contains(r#"<table:table-cell office:value-type="string" calcext:value-type="string"><text:p>[TEL_1]</text:p>"#));
    assert!(c.contains("table:number-columns-repeated=\"1048576\""));
    assert!(out.bytes.len() < input.len() * 2);
    let found = leaks(&OdfHandler.deep_scan(&out.bytes).unwrap(), &r);
    assert!(
        found
            .iter()
            .all(|f| f.contains("Dupont") && f.contains("content.xml")),
        "{found:?}"
    );

    let back = OdfHandler
        .transform(&out.bytes, &Restorer::new(&s), &TransformOptions::restore())
        .unwrap();
    let b = part(&back.bytes, "content.xml");
    assert!(
        b.contains(r#"<table:table-cell office:value-type="date" calcext:value-type="date" office:date-value="1981-05-12"><text:p>12/05/1981</text:p>"#),
        "{b}"
    );
    assert!(b.contains(r#"<table:table-cell office:value-type="float" calcext:value-type="float" office:value="281234"><text:p>281234</text:p>"#));
    let blocks = |x: &[u8]| -> Vec<String> {
        OdfHandler
            .extract(x)
            .unwrap()
            .blocks
            .into_iter()
            .map(|b| b.text)
            .collect()
    };
    assert_eq!(blocks(&input), blocks(&back.bytes));
}

#[test]
fn odf_encrypted_and_examples_container() {
    let enc = odf_zip(
        "application/vnd.oasis.opendocument.text",
        &[("content.xml", "<x/>")],
        r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"><manifest:encryption-data manifest:checksum-type="SHA1/1K" manifest:checksum="AA=="/></manifest:file-entry>"#,
    );
    assert!(matches!(
        OdfHandler.extract(&enc),
        Err(FormatError::Encrypted)
    ));
    for f in ["compte-rendu.odt", "planning.ods", "support.odp"] {
        let input = read_example(f);
        let s = session_of(&expected(f));
        let out = OdfHandler
            .transform(&input, &Replacer::new(&s), &TransformOptions::anonymize())
            .unwrap();
        assert_mimetype_first(&out.bytes);
        assert_well_formed(&out.bytes);
        assert!(
            !part(&out.bytes, "meta.xml").contains("<meta:initial-creator>A"),
            "{f} : auteur"
        );
    }
    let ex = OdfHandler.extract(&read_example("support.odp")).unwrap();
    assert!(
        ex.blocks
            .iter()
            .any(|b| b.ctx.role == BlockRole::Note && b.text.contains("Pierre Garnier"))
    );
}
