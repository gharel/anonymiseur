//! `Handler::tables` : une `TableData` par feuille, en-têtes = première ligne non
//! vide, valeurs telles qu'affichées ; aucune panique sur un fichier corrompu.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{Handler, TableData};
use anon_office::{CsvHandler, DocxHandler, OdfHandler, PptxHandler, TextHandler, XlsxHandler};
use common::*;
use std::io::{Cursor, Write};

fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (n, c) in parts {
        w.start_file(*n, o).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const X: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Classeur minimal : deux feuilles, chaînes partagées, style de date.
fn xlsx_fixture(sheet1: &str) -> Vec<u8> {
    let workbook = format!(
        r#"<workbook {X}><sheets><sheet name="Patients" sheetId="1" r:id="rId1"/><sheet name="Vide" sheetId="2" r:id="rId2"/><sheet name="Graphique" sheetId="3" r:id="rId3"/></sheets></workbook>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{REL}/worksheet" Target="/xl/worksheets/sheet2.xml"/><Relationship Id="rId3" Type="{REL}/chartsheet" Target="chartsheets/sheet1.xml"/></Relationships>"#
    );
    let shared = format!(
        r#"<sst {X}><si><t>Âge</t></si><si><t>Commune</t></si><si><r><t>Far</t></r><r><t>ino</t></r><rPh><t>ファ</t></rPh></si><si><t>Né le</t></si><si><t>a_x000D_b</t></si></sst>"#
    );
    let styles = format!(
        r#"<styleSheet {X}><numFmts count="1"><numFmt numFmtId="164" formatCode="dd/mm/yyyy"/></numFmts><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164"/></cellXfs></styleSheet>"#
    );
    let sheet2 = format!(r#"<worksheet {X}><sheetData/></worksheet>"#);
    zip_of(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &rels),
        ("xl/sharedStrings.xml", &shared),
        ("xl/styles.xml", &styles),
        ("xl/worksheets/sheet1.xml", sheet1),
        ("xl/worksheets/sheet2.xml", &sheet2),
    ])
}

fn sheet(rows: &str) -> String {
    format!(r#"<worksheet {X}><sheetData>{rows}</sheetData></worksheet>"#)
}

#[test]
fn xlsx_values_as_displayed() {
    let s = sheet(concat!(
        r#"<row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" t="s"><v>1</v></c><c r="D2" t="s"><v>3</v></c><c r="E2" t="inlineStr"><is><t>Actif</t></is></c></row>"#,
        r#"<row r="4"><c r="B4"><v>62</v></c><c r="C4" t="s"><v>2</v></c><c r="D4" s="1"><v>17025</v></c><c r="E4" t="b"><v>1</v></c><c r="F4"><f>A1</f></c></row>"#,
        r#"<row r="5"><c r="B5"><v>3.5</v></c><c r="C5" t="str"><f>"x"</f><v>calculé</v></c><c r="D5" t="s"><v>4</v></c><c r="E5" t="inlineStr"><is><r><t>en </t></r><r><t>ligne</t></r><rPh><t>ルビ</t></rPh></is></c></row>"#,
        r#"<row r="6"/>"#,
        r#"<row><c t="s"><v>2</v></c><c><v>7</v></c></row>"#,
    ));
    let t = XlsxHandler.tables(&xlsx_fixture(&s)).unwrap();
    assert_eq!(t.len(), 1, "feuille vide et graphique ignorés");
    let p = &t[0];
    assert_eq!(p.sheet, "Patients");
    // La dernière ligne, sans références, commence en colonne A : le tableau
    // s'étend de A à E (F4, formule sans valeur en cache, est vide).
    assert_eq!(
        p.headers,
        strings(&["", "Âge", "Commune", "Né le", "Actif"])
    );
    assert_eq!(
        p.rows,
        vec![
            strings(&["", "62", "Farino", "11/08/1946", "VRAI"]),
            strings(&["", "3.5", "calculé", "a\rb", "en ligne"]),
            strings(&["Farino", "7", "", "", ""]),
        ]
    );
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn xlsx_columns_start_at_first_used_column() {
    let s = sheet(concat!(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Âge</t></is></c><c r="B1" t="inlineStr"><is><t>Sexe</t></is></c></row>"#,
        r#"<row r="2"><c r="A2"><v>45</v></c><c r="B2" t="inlineStr"><is><t>F</t></is></c></row>"#,
        r#"<row><c><v>46</v></c><c t="inlineStr"><is><t>M</t></is></c></row>"#,
    ));
    let t = XlsxHandler.tables(&xlsx_fixture(&s)).unwrap();
    assert_eq!(t[0].headers, vec!["Âge", "Sexe"]);
    assert_eq!(
        t[0].rows,
        vec![
            vec!["45".to_string(), "F".to_string()],
            vec!["46".to_string(), "M".to_string()]
        ]
    );
}

#[test]
fn xlsx_example_file() {
    let t = XlsxHandler
        .tables(&read_example("fichier-clients.xlsx"))
        .unwrap();
    let clients = t.iter().find(|t| t.sheet == "Clients").unwrap();
    assert_eq!(
        clients.headers,
        vec![
            "Nom",
            "Prénom",
            "Email",
            "Téléphone",
            "Adresse",
            "RIDET",
            "Montant"
        ]
    );
    let dupont = &clients.rows[0];
    assert_eq!(dupont[0], "Dupont");
    assert_eq!(dupont[6], "1250000", "nombre en texte");
    assert!(clients.rows.iter().any(|r| r[3] == "33612345678"));
    assert!(t.iter().any(|t| t.sheet == "Notes libres"));
}

fn ods_fixture(body: &str, mime: &str) -> Vec<u8> {
    let ns = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/""#;
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {ns}><office:body>{body}</office:body></office:document-content>"#
    );
    let manifest = r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/"/></manifest:manifest>"#;
    zip_of(&[
        ("mimetype", mime),
        ("META-INF/manifest.xml", manifest),
        ("content.xml", &content),
    ])
}

const ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";

#[test]
fn ods_repeated_rows_and_cells() {
    let body = concat!(
        r#"<office:spreadsheet><table:table table:name="Suivi">"#,
        r#"<table:table-column table:number-columns-repeated="3"/>"#,
        r#"<table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="1024"/></table:table-row>"#,
        r#"<table:table-header-rows><table:table-row><table:table-cell><text:p>Âge</text:p></table:table-cell><table:table-cell><text:p>Sexe</text:p></table:table-cell><table:table-cell><text:p>Note</text:p></table:table-cell></table:table-row></table:table-header-rows>"#,
        r#"<table:table-row table:number-rows-repeated="3"><table:table-cell office:value-type="float" office:value="62"><text:p>62</text:p></table:table-cell><table:table-cell table:number-columns-repeated="2"><text:p>M</text:p><office:annotation><dc:creator>Jean Dupont</dc:creator><text:p>commentaire</text:p></office:annotation></table:table-cell></table:table-row>"#,
        r#"<table:table-row><table:table-cell/><table:table-cell><text:p>a<text:s text:c="2"/>b</text:p><text:p>c</text:p></table:table-cell></table:table-row>"#,
        r#"<table:table-row table:number-rows-repeated="1048000"><table:table-cell table:number-columns-repeated="1024"/></table:table-row>"#,
        r#"</table:table><table:table table:name="Vide"/></office:spreadsheet>"#,
    );
    let t = OdfHandler.tables(&ods_fixture(body, ODS)).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].sheet, "Suivi");
    assert_eq!(t[0].headers, vec!["Âge", "Sexe", "Note"]);
    let r = |a: &str, b: &str, c: &str| vec![a.to_string(), b.to_string(), c.to_string()];
    assert_eq!(
        t[0].rows,
        vec![
            r("62", "M", "M"),
            r("62", "M", "M"),
            r("62", "M", "M"),
            r("", "a  b\nc", "")
        ]
    );
}

/// Cellule sans paragraphe (autre logiciel que LibreOffice) : valeur de l'attribut.
#[test]
fn ods_cell_value_without_paragraph() {
    let body = concat!(
        r#"<office:spreadsheet><table:table table:name="T">"#,
        r#"<table:table-row><table:table-cell><text:p>Âge</text:p></table:table-cell></table:table-row>"#,
        r#"<table:table-row><table:table-cell office:value-type="float" office:value="62"></table:table-cell></table:table-row>"#,
        r#"</table:table></office:spreadsheet>"#,
    );
    let t = OdfHandler.tables(&ods_fixture(body, ODS)).unwrap();
    assert_eq!(t[0].rows, vec![vec!["62".to_string()]]);
}

#[test]
fn ods_text_document_has_no_table() {
    let body = r#"<office:text><table:table table:name="T"><table:table-row><table:table-cell><text:p>Nom</text:p></table:table-cell></table:table-row></table:table></office:text>"#;
    let t = OdfHandler
        .tables(&ods_fixture(
            body,
            "application/vnd.oasis.opendocument.text",
        ))
        .unwrap();
    assert!(t.is_empty());
}

#[test]
fn ods_example_file() {
    let t = OdfHandler.tables(&read_example("planning.ods")).unwrap();
    assert!(!t.is_empty());
    assert!(t.iter().all(|t| !t.headers.is_empty()));
    assert!(
        t.iter()
            .all(|t| t.rows.iter().all(|r| r.len() == t.headers.len()))
    );
}

#[test]
fn csv_table() {
    // Une ligne plus longue que l'en-tête : le tableau s'élargit, les autres
    // lignes sont complétées par des cellules vides ; lignes vides ignorées.
    let input = "\u{FEFF}Âge;Commune;Code postal\r\n\r\n62;Farino;98881\r\n\"45\";\"La Foa\";98880;en trop\r\n;;\r\n30;Thio;98829\r\n51;Voh;98833\r\n28;Poya;98827\r\n";
    let t = CsvHandler.tables(input.as_bytes()).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].headers, vec!["Âge", "Commune", "Code postal", ""]);
    assert_eq!(t[0].rows.len(), 5);
    assert_eq!(t[0].rows[0], vec!["62", "Farino", "98881", ""]);
    assert_eq!(t[0].rows[1], vec!["45", "La Foa", "98880", "en trop"]);
    assert!(
        CsvHandler
            .tables("Une phrase sans colonnes.\n".as_bytes())
            .unwrap()
            .is_empty()
    );
    let ex = CsvHandler.tables(&read_example("export.csv")).unwrap();
    assert_eq!(ex.len(), 1);
    assert!(!ex[0].rows.is_empty());
}

#[test]
fn text_formats_have_no_table() {
    let docx = read_example("contrat-prestation.docx");
    assert!(DocxHandler.tables(&docx).unwrap().is_empty());
    let pptx = read_example("presentation-projet.pptx");
    assert!(PptxHandler.tables(&pptx).unwrap().is_empty());
    assert!(TextHandler.tables(b"a;b\n1;2\n").unwrap().is_empty());
}

#[test]
fn corrupt_spreadsheets_are_errors() {
    let good = read_example("fichier-clients.xlsx");
    for cut in [10, good.len() / 3, good.len() - 30] {
        assert!(XlsxHandler.tables(&good[..cut]).is_err(), "coupé à {cut}");
    }
    assert!(XlsxHandler.tables(b"pas un zip").is_err());
    // Feuille annoncée mais absente, XML mal formé.
    let missing = zip_of(&[
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        (
            "xl/workbook.xml",
            &format!(r#"<workbook {X}><sheets><sheet name="A" r:id="rId1"/></sheets></workbook>"#),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            &format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/worksheet" Target="worksheets/absente.xml"/></Relationships>"#
            ),
        ),
    ]);
    assert!(XlsxHandler.tables(&missing).is_err());
    assert!(
        XlsxHandler
            .tables(&xlsx_fixture("<worksheet><sheetData><row>"))
            .is_err()
    );
    let ods = read_example("planning.ods");
    assert!(OdfHandler.tables(&ods[..ods.len() / 2]).is_err());
    assert!(
        OdfHandler
            .tables(&ods_fixture("<office:spreadsheet><table:table>", ODS))
            .is_err()
    );
}

#[test]
fn far_away_cell_is_refused_not_allocated() {
    let mut rows = String::new();
    for r in 1..=2000 {
        rows.push_str(&format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c></row>"#));
    }
    rows.push_str(r#"<row r="2001"><c r="XFD2001"><v>1</v></c></row>"#);
    assert!(XlsxHandler.tables(&xlsx_fixture(&sheet(&rows))).is_err());
}

#[test]
fn debug_of_tables_hides_content() {
    let t = XlsxHandler
        .tables(&read_example("fichier-clients.xlsx"))
        .unwrap();
    let s = format!("{t:?}");
    assert!(!s.contains("Dupont") && !s.contains("Clients"), "{s}");
    let _: &TableData = &t[0];
}
