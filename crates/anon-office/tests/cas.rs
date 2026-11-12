//! Cas précis : runs coupés, révisions, commentaires, liens, nombres,
//! formules, graphiques, notes, métadonnées, fixtures Excel construites ici.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::{EntityKind::*, Handler, TransformOptions, WarningCode};
use anon_office::{DocxHandler, PptxHandler, XlsxHandler};
use anon_replace::{Replacer, Restorer};
use common::*;

fn anonymize(h: &dyn Handler, input: &[u8], values: &[(anon_core::EntityKind, &str)]) -> Vec<u8> {
    let v: Vec<_> = values.iter().map(|(k, s)| (*k, s.to_string())).collect();
    let session = session_of(&v);
    h.transform(
        input,
        &Replacer::new(&session),
        &TransformOptions::anonymize(),
    )
    .unwrap()
    .bytes
}

fn contract() -> (Vec<u8>, Vec<u8>, anon_core::Session) {
    let input = read_example("contrat-prestation.docx");
    let session = session_of(&expected("contrat-prestation.docx"));
    let out = DocxHandler
        .transform(
            &input,
            &Replacer::new(&session),
            &TransformOptions::anonymize(),
        )
        .unwrap()
        .bytes;
    (input, out, session)
}

#[test]
fn split_name_keeps_bold_on_original_run() {
    let input = read_example("contrat-prestation.docx");
    let blocks = DocxHandler.extract(&input).unwrap().blocks;
    assert!(
        blocks
            .iter()
            .any(|b| b.text.contains("représentée par Jean Dupont, gérant."))
    );
    let out = anonymize(&DocxHandler, &input, &[(Personne, "Jean Dupont")]);
    let doc = part(&out, "word/document.xml");
    assert!(
        doc.contains(
            "<w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">[PERSONNE_1]</w:t></w:r><w:r><w:t xml:space=\"preserve\"></w:t></w:r>"
        ),
        "{doc}"
    );
}

#[test]
fn tracked_revision_comment_and_authors() {
    let (_, out, _) = contract();
    let doc = part(&out, "word/document.xml");
    assert!(doc.contains("<w:delText xml:space=\"preserve\">[PERSONNE_"));
    assert!(!doc.contains("Pierre Garnier") && !doc.contains("Olivier Nguyen"));
    assert!(doc.contains("w:author=\"Auteur\""));
    let comments = part(&out, "word/comments.xml");
    assert!(comments.contains("w:author=\"Auteur\" w:initials=\"A\""));
    assert!(comments.contains("À valider avec [PERSONNE_"));
    assert!(!comments.contains("Kaloï") && !comments.contains("06 98"));
}

#[test]
fn mailto_link_round_trip() {
    let (input, out, session) = contract();
    let rels = part(&out, "word/_rels/document.xml.rels");
    assert!(
        rels.contains("Target=\"mailto:EMAIL_")
            && rels.contains("@anon.invalid\" TargetMode=\"External\""),
        "{rels}"
    );
    let back = DocxHandler
        .transform(&out, &Restorer::new(&session), &TransformOptions::restore())
        .unwrap()
        .bytes;
    assert_eq!(
        part(&back, "word/_rels/document.xml.rels"),
        part(&input, "word/_rels/document.xml.rels")
    );
}

#[test]
fn metadata_scrubbed_and_extracted() {
    let (input, out, _) = contract();
    let ex = DocxHandler.extract(&input).unwrap();
    let meta: Vec<_> = ex
        .blocks
        .iter()
        .filter(|b| b.ctx.role == anon_core::BlockRole::Metadata)
        .map(|b| b.text.as_str())
        .collect();
    for v in [
        "Jean Dupont",
        "Martin Lefèvre",
        "Nautilus Pacifique SARL",
        "Hélène Kaloï",
    ] {
        assert!(meta.contains(&v), "{v} absent des métadonnées");
    }
    let core = part(&out, "docProps/core.xml");
    assert!(core.contains("<dc:creator></dc:creator>"), "{core}");
    assert!(core.contains("<cp:lastModifiedBy></cp:lastModifiedBy>"));
    assert!(core.contains("<dc:title>Contrat de prestation de services</dc:title>"));
    assert!(part(&out, "docProps/app.xml").contains("<Company></Company>"));
    // Miniature retirée avec sa relation.
    assert!(
        !zip_names(&out)
            .iter()
            .any(|n| n.starts_with("docProps/thumbnail"))
    );
    assert!(!part(&out, "_rels/.rels").contains("thumbnail"));
    assert_well_formed(&out);
}

#[test]
fn numeric_phones_and_formula_literal() {
    let input = read_example("fichier-clients.xlsx");
    let ex = XlsxHandler.extract(&input).unwrap();
    let d3 = ex
        .blocks
        .iter()
        .find(|b| b.ctx.part == "xl/worksheets/sheet1.xml!D3")
        .unwrap();
    assert_eq!(d3.text, "33612345678");
    assert_eq!(d3.ctx.column_header.as_deref(), Some("Téléphone"));
    assert!(
        ex.blocks
            .iter()
            .any(|b| b.ctx.role == anon_core::BlockRole::Formula
                && b.text.contains("Hélène Kaloï"))
    );
    let out = anonymize(
        &XlsxHandler,
        &input,
        &[(Tel, "33612345678"), (Personne, "Hélène Kaloï")],
    );
    let sheet = part(&out, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(
            "<c r=\"D3\" s=\"2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">[TEL_1]</t></is></c>"
        ),
        "{sheet}"
    );
    assert!(
        sheet.contains("CONCATENATE(\"Dossier suivi par [PERSONNE_1], total : \""),
        "{sheet}"
    );
    assert_well_formed(&out);
}

#[test]
fn chart_with_embedded_workbook_in_pptx() {
    let input = read_example("presentation-projet.pptx");
    let session = session_of(&expected("presentation-projet.pptx"));
    let r = Replacer::new(&session);
    let out = PptxHandler
        .transform(&input, &r, &TransformOptions::anonymize())
        .unwrap()
        .bytes;
    let chart = part(&out, "ppt/charts/chart1.xml");
    assert!(chart.contains("<c:v>[PERSONNE_"), "{chart}");
    assert!(!chart.contains("Garnier"));
    let inner = zip_entries(&out)
        .into_iter()
        .find(|e| e.0 == "ppt/embeddings/Microsoft_Excel_Sheet1.xlsx")
        .unwrap()
        .1;
    let ss = part(&inner, "xl/sharedStrings.xml");
    assert!(ss.contains("[PERSONNE_") && !ss.contains("Kaloï"), "{ss}");
    assert_well_formed(&inner);
    let notes = part(&out, "ppt/notesSlides/notesSlide1.xml");
    assert!(notes.contains("Rappeler [PERSONNE_") && notes.contains("[TEL_"));
    assert!(!notes.contains("Païta"));
    let ex = PptxHandler.extract(&input).unwrap();
    assert!(
        ex.blocks
            .iter()
            .any(|b| b.ctx.role == anon_core::BlockRole::Note && b.text.contains("06 98 76 54 32"))
    );
}

// ---------------------------------------------------------------------------
// Fixture Excel construite à la main.

const CT: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="0"/><sheets><sheet name="Suivi Dupont" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="Ref">"Dupont"</definedName></definedNames></workbook>"#;
const STYLES: &str = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="jj/mm/aaaa"/></numFmts><cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/></cellXfs></styleSheet>"#;
const SHARED: &str = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3"><si><t>Contact Dupont</t></si><si><r><rPr><b/></rPr><t xml:space="preserve">Jean </t></r><r><t>Dupont_x000D_</t></r><r><t>suite</t></r></si><si><t>Montant</t></si></sst>"#;
const SHEET: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>2</v></c></row><row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2" s="1"><v>45730</v></c><c r="C2" s="2"><v>45731</v></c><c r="D2"><f>COUNTA(Tbl[Contact Dupont])&amp;" Dupont"</f><v>2</v></c><c r="E2"><f>Tbl[[#This Row],[Contact Dupont]]</f><v>0</v></c></row></sheetData><dataValidations count="1"><dataValidation type="list" sqref="F2"><formula1>"Dupont,Martin"</formula1></dataValidation></dataValidations><headerFooter><oddHeader>&amp;LDossier Dupont</oddHeader></headerFooter></worksheet>"#;
const TABLE: &str = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Tbl" displayName="Tbl" ref="A1:B2"><tableColumns count="2"><tableColumn id="1" name="Contact Dupont"/><tableColumn id="2" name="Montant"/></tableColumns></table>"#;
const PIVOT_DEF: &str = r#"<pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cacheFields count="1"><cacheField name="Contact Dupont"><sharedItems><s v="Jean Dupont"/><s v="Autre"/></sharedItems></cacheField></cacheFields></pivotCacheDefinition>"#;
const PIVOT_REC: &str = r#"<pivotCacheRecords xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1"><r><s v="Jean Dupont"/><n v="3"/></r></pivotCacheRecords>"#;
const CORE: &str = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Jean Dupont</dc:creator><dc:title>Suivi Dupont</dc:title></cp:coreProperties>"#;
const CUSTOM: &str = r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Client"><vt:lpwstr>Jean Dupont</vt:lpwstr></property></Properties>"#;

fn fixture() -> Vec<u8> {
    make_zip(&[
        ("[Content_Types].xml", CT),
        ("_rels/.rels", RELS),
        ("docProps/core.xml", CORE),
        ("docProps/custom.xml", CUSTOM),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/styles.xml", STYLES),
        ("xl/sharedStrings.xml", SHARED),
        ("xl/worksheets/sheet1.xml", SHEET),
        ("xl/tables/table1.xml", TABLE),
        ("xl/pivotCache/pivotCacheDefinition1.xml", PIVOT_DEF),
        ("xl/pivotCache/pivotCacheRecords1.xml", PIVOT_REC),
    ])
}

#[test]
fn fixture_extraction() {
    let ex = XlsxHandler.extract(&fixture()).unwrap();
    let find = |part: &str| {
        ex.blocks
            .iter()
            .find(|b| b.ctx.part == part)
            .unwrap_or_else(|| panic!("{part} absent"))
    };
    assert_eq!(find("xl/worksheets/sheet1.xml!A1").text, "Contact Dupont");
    let a2 = find("xl/worksheets/sheet1.xml!A2");
    assert_eq!(a2.text, "Jean Dupont\rsuite");
    assert_eq!(a2.ctx.column_header.as_deref(), Some("Contact Dupont"));
    assert_eq!(find("xl/worksheets/sheet1.xml!B2").text, "14/03/2025");
    assert_eq!(find("xl/worksheets/sheet1.xml!C2").text, "15/03/2025");
    let texts: Vec<_> = ex.blocks.iter().map(|b| b.text.as_str()).collect();
    for t in [
        "Jean Dupont",
        " Dupont",
        "Dupont,Martin",
        "&LDossier Dupont",
        "Suivi Dupont",
    ] {
        assert!(texts.contains(&t), "{t} absent : {texts:?}");
    }
    assert!(ex.blocks.iter().any(|b| {
        b.ctx
            .part
            .starts_with("xl/pivotCache/pivotCacheRecords1.xml")
            && b.text == "Jean Dupont"
    }));
}

#[test]
fn fixture_table_header_formula_and_caches() {
    let input = fixture();
    let session = session_of(&[
        (Personne, "Jean Dupont".to_string()),
        (Nom, "Dupont".to_string()),
        (Date, "14/03/2025".to_string()),
    ]);
    let r = Replacer::new(&session);
    let out = XlsxHandler
        .transform(&input, &r, &TransformOptions::anonymize())
        .unwrap();
    assert_well_formed(&out.bytes);
    // Nom de feuille : signalé, jamais modifié.
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == WarningCode::SheetNameSensitive)
    );
    assert!(part(&out.bytes, "xl/workbook.xml").contains("name=\"Suivi Dupont\""));
    assert!(part(&out.bytes, "xl/workbook.xml").contains(">\"[NOM_1]\"</definedName>"));

    // En-tête, tableColumn et références structurées : exactement le même texte.
    let ss = part(&out.bytes, "xl/sharedStrings.xml");
    assert!(
        ss.contains("<si><t xml:space=\"preserve\">Contact [NOM_1]</t></si>"),
        "{ss}"
    );
    let table = part(&out.bytes, "xl/tables/table1.xml");
    assert!(
        table.contains("<tableColumn id=\"1\" name=\"Contact [NOM_1]\"/>"),
        "{table}"
    );
    let sheet = part(&out.bytes, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("COUNTA(Tbl[Contact '[NOM_1']])&amp;\" [NOM_1]\""),
        "{sheet}"
    );
    assert!(sheet.contains("Tbl[[#This Row],[Contact '[NOM_1']]]"));
    // Texte riche : le gras garde la balise, `_x000D_` est conservé.
    assert!(ss.contains(
        "<r><rPr><b/></rPr><t xml:space=\"preserve\">[PERSONNE_1]</t></r><r><t xml:space=\"preserve\">_x000D_</t></r><r><t>suite</t></r>"
    ));
    // Date au format date : devient une chaîne en ligne.
    assert!(sheet.contains(
        "<c r=\"B2\" s=\"1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">[DATE_1]</t></is></c>"
    ));
    assert!(sheet.contains("<c r=\"C2\" s=\"2\"><v>45731</v></c>"));
    // Validation, en-tête d'impression.
    assert!(sheet.contains("<formula1>\"[NOM_1],Martin\"</formula1>"));
    assert!(sheet.contains("<oddHeader>&amp;LDossier [NOM_1]</oddHeader>"));
    // Caches de TCD.
    assert!(
        part(&out.bytes, "xl/pivotCache/pivotCacheDefinition1.xml")
            .contains("<s v=\"[PERSONNE_1]\"/>")
    );
    assert!(
        part(&out.bytes, "xl/pivotCache/pivotCacheRecords1.xml")
            .contains("<s v=\"[PERSONNE_1]\"/>")
    );
    // Propriétés.
    assert!(part(&out.bytes, "docProps/core.xml").contains("<dc:creator></dc:creator>"));
    assert!(
        part(&out.bytes, "docProps/custom.xml").contains("<vt:lpwstr>[PERSONNE_1]</vt:lpwstr>")
    );

    // Seul le nom de feuille (signalé) reste.
    let found = leaks(&XlsxHandler.deep_scan(&out.bytes).unwrap(), &r);
    assert!(
        found
            .iter()
            .all(|f| f.contains("Dupont") && (f.contains("workbook.xml") || f.contains("app.xml"))),
        "{found:?}"
    );

    // Restauration : en-tête, colonne et formule reviennent ensemble.
    let back = XlsxHandler
        .transform(
            &out.bytes,
            &Restorer::new(&session),
            &TransformOptions::restore(),
        )
        .unwrap()
        .bytes;
    assert!(part(&back, "xl/tables/table1.xml").contains("name=\"Contact Dupont\""));
    assert!(part(&back, "xl/worksheets/sheet1.xml").contains("COUNTA(Tbl[Contact Dupont])"));
    assert!(part(&back, "xl/sharedStrings.xml").contains("Contact Dupont"));
}

#[test]
fn date1904_flag() {
    let wb = WORKBOOK.replace("date1904=\"0\"", "date1904=\"1\"");
    let input = make_zip(&[
        ("[Content_Types].xml", CT),
        ("_rels/.rels", RELS),
        ("xl/workbook.xml", &wb),
        ("xl/styles.xml", STYLES),
        ("xl/sharedStrings.xml", SHARED),
        ("xl/worksheets/sheet1.xml", SHEET),
    ]);
    let ex = XlsxHandler.extract(&input).unwrap();
    let b2 = ex
        .blocks
        .iter()
        .find(|b| b.ctx.part == "xl/worksheets/sheet1.xml!B2")
        .unwrap();
    assert_eq!(b2.text, "15/03/2029");
}
