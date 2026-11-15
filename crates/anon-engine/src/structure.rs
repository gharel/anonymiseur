//! Contrôle de fuite : identifiants structurels d'un paquet Office ou OpenDocument.
//!
//! `Handler::deep_scan` lit **toutes** les valeurs d'attributs d'un paquet, y
//! compris celles qui ne décrivent que sa structure : numéro de ligne
//! (`<row r="19">`), plage (`spans="1:6"`), priorité d'un style
//! (`w:uiPriority="35"`), dimensions, ancrage d'un dessin (`<xdr:row>19</xdr:row>`).
//! Une valeur validée courte, comme un âge « 19 », y était trouvée à tort et
//! bloquait le fichier.
//!
//! Ce module compte, partie par partie, les occurrences de chaque balise dans
//! ces seuls emplacements structurels : attributs et éléments d'une liste fermée
//! ([`STRUCTURAL_ATTRIBUTES`], [`STRUCTURAL_VAL_ELEMENTS`],
//! [`STRUCTURAL_TEXT_ELEMENTS`]) dont la valeur n'est faite que de chiffres, de
//! lettres majuscules (références de cellule) et de séparateurs. Le contrôle de
//! fuite retire ce nombre des occurrences trouvées dans la même partie : chaque
//! occurrence structurelle n'en compense qu'une, donc une occurrence de contenu
//! (texte, attribut hors liste, cache de tableau croisé, valeur d'une cellule
//! OpenDocument) reste toujours comptée. Toute anomalie de lecture ne retire rien.

use anon_core::Tag;
use anon_office::{MAX_ENTRIES, MAX_ENTRY_UNCOMPRESSED, MAX_TOTAL_UNCOMPRESSED};
use anon_replace::Replacer;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// Longueur maximale d'un nom d'entrée repris tel quel comme emplacement (même
/// règle que `anon-office`, qui nomme sinon l'entrée par son rang).
const PLAIN_NAME_MAX_BYTES: usize = 160;
/// Longueur maximale d'une valeur structurelle.
const MAX_STRUCTURAL_CHARS: usize = 64;

/// Attributs (nom local) qui ne portent jamais de contenu : références,
/// indices, tailles et positions (SpreadsheetML, WordprocessingML, DrawingML,
/// OpenDocument). Les attributs qui peuvent porter une valeur saisie (`v`,
/// `val`, `value`, `name`, `descr`, `title`, `formula`…) n'y sont pas.
const STRUCTURAL_ATTRIBUTES: &[&str] = &[
    // SpreadsheetML : références, styles, vues, mise en page.
    "r",
    "ref",
    "sqref",
    "spans",
    "s",
    "min",
    "max",
    "width",
    "customWidth",
    "customHeight",
    "ht",
    "outlineLevel",
    "outlineLevelRow",
    "outlineLevelCol",
    "count",
    "uniqueCount",
    "sheetId",
    "numFmtId",
    "fontId",
    "fillId",
    "borderId",
    "xfId",
    "dxfId",
    "dyDescent",
    "activeTab",
    "firstSheet",
    "topLeftCell",
    "activeCell",
    "defaultRowHeight",
    "defaultColWidth",
    "baseColWidth",
    "workbookViewId",
    "zoomScale",
    "zoomScaleNormal",
    "zoomScalePageLayoutView",
    "zoomScaleSheetLayoutView",
    "windowWidth",
    "windowHeight",
    "xWindow",
    "yWindow",
    "tabRatio",
    "xSplit",
    "ySplit",
    "iconId",
    "priority",
    "indexed",
    "theme",
    "tint",
    "rgb",
    "authorId",
    "cm",
    "vm",
    "paperSize",
    "scale",
    "fitToWidth",
    "fitToHeight",
    "horizontalDpi",
    "verticalDpi",
    "copies",
    "firstPageNumber",
    "left",
    "right",
    "top",
    "bottom",
    "header",
    "footer",
    "id",
    // WordprocessingML : styles, espacements, retraits, sections, révisions.
    "uiPriority",
    "after",
    "before",
    "line",
    "afterLines",
    "beforeLines",
    "firstLine",
    "hanging",
    "start",
    "end",
    "gutter",
    "space",
    "linePitch",
    "charSpace",
    "num",
    "w",
    "h",
    "code",
    "abstractNumId",
    "numId",
    "ilvl",
    "rsidR",
    "rsidRPr",
    "rsidRDefault",
    "rsidP",
    "rsidTr",
    "rsidSect",
    "rsidDel",
    "paraId",
    "textId",
    "distT",
    "distB",
    "distL",
    "distR",
    "tblpX",
    "tblpY",
    "percent",
    "pos",
    // DrawingML : positions, tailles, marges, effets.
    "x",
    "y",
    "cx",
    "cy",
    "rot",
    "l",
    "t",
    "b",
    "lIns",
    "tIns",
    "rIns",
    "bIns",
    "marL",
    "marR",
    "indent",
    "lvl",
    "spc",
    "kern",
    "baseline",
    "sz",
    "defTabSz",
    "idx",
    "ang",
    "dist",
    "dir",
    "blurRad",
    "stA",
    "endA",
    "stPos",
    "endPos",
    "sx",
    "sy",
    "kx",
    "ky",
    "fov",
    "rad",
    "numCol",
    "spcCol",
    // OpenDocument : répétitions et fusions de cellules, niveaux.
    "number-columns-repeated",
    "number-rows-repeated",
    "number-columns-spanned",
    "number-rows-spanned",
    "number-matrix-columns-spanned",
    "number-matrix-rows-spanned",
    "z-index",
    "outline-level",
    "start-value",
    "level",
];

/// Éléments (nom local) dont l'attribut `val` est un réglage de mise en forme :
/// taille de police, interligne, priorité de style, transformations de couleur,
/// indices et réglages d'un graphique. Ailleurs, `val` peut porter une valeur
/// saisie (filtre automatique, champ de formulaire, variable de document) et
/// reste compté.
const STRUCTURAL_VAL_ELEMENTS: &[&str] = &[
    "sz",
    "szCs",
    "kern",
    "position",
    "spacing",
    "w",
    "uiPriority",
    "outlineLvl",
    "ilvl",
    "numId",
    "abstractNumId",
    "start",
    "lvlRestart",
    "defaultTabStop",
    "gridSpan",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "family",
    "charset",
    "alpha",
    "alphaMod",
    "alphaOff",
    "lumMod",
    "lumOff",
    "shade",
    "tint",
    "satMod",
    "satOff",
    "hueMod",
    "hueOff",
    "hue",
    "sat",
    "lum",
    "red",
    "green",
    "blue",
    "spcPct",
    "spcPts",
    "buSzPct",
    "buSzPts",
    "idx",
    "order",
    "axId",
    "crossAx",
    "ptCount",
    "gapWidth",
    "overlap",
    "holeSize",
    "firstSliceAng",
    "explosion",
    "lblOffset",
    "tickLblSkip",
    "tickMarkSkip",
    "rotX",
    "rotY",
    "depthPercent",
    "hPercent",
    "gapDepth",
    "bubbleScale",
];

/// Éléments (nom local) dont le texte est structurel : ancrage d'un dessin
/// dans une feuille, statistiques du document (`docProps/app.xml`).
const STRUCTURAL_TEXT_ELEMENTS: &[&str] = &[
    "row",
    "col",
    "rowOff",
    "colOff",
    "Pages",
    "Words",
    "Characters",
    "CharactersWithSpaces",
    "Lines",
    "Paragraphs",
    "TotalTime",
    "DocSecurity",
    "Slides",
    "Notes",
    "HiddenSlides",
    "MMClips",
    "AppVersion",
    "revision",
];

/// Emplacement d'une entrée, tel que `anon-office` le donne dans `deep_scan`.
fn label(name: &str, index: usize) -> String {
    let plain = name.len() <= PLAIN_NAME_MAX_BYTES
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-[] ".contains(c));
    if plain {
        name.to_string()
    } else {
        format!("entrée n° {}", index.saturating_add(1))
    }
}

/// Vrai pour une valeur faite de chiffres, de lettres majuscules (références de
/// cellule, couleurs) et de séparateurs, avec au moins un chiffre.
fn structural_value(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.len() <= MAX_STRUCTURAL_CHARS
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() || "$:.,;-+% ".contains(c))
}

fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// Occurrences structurelles d'un texte, ajoutées au compte de la partie.
fn credit(counts: &mut HashMap<Tag, usize>, replacer: &Replacer, text: &str) {
    if !structural_value(text) {
        return;
    }
    for m in replacer.matches(text) {
        let n = counts.entry(m.tag).or_insert(0);
        *n = n.saturating_add(1);
    }
}

fn credit_attributes(counts: &mut HashMap<Tag, usize>, replacer: &Replacer, e: &BytesStart<'_>) {
    let element = local_name(e.name().as_ref()).to_string();
    for attr in e.attributes().with_checks(false).flatten() {
        let key = local_name(attr.key.as_ref());
        let structural = STRUCTURAL_ATTRIBUTES.contains(&key)
            || (key == "val" && STRUCTURAL_VAL_ELEMENTS.contains(&element.as_str()));
        if !structural {
            continue;
        }
        if let Ok(value) = attr.normalized_value(XmlVersion::Explicit1_0) {
            credit(counts, replacer, &value);
        }
    }
}

/// Texte accumulé depuis la dernière balise : compté s'il appartient à un
/// élément structurel et qu'aucune entité, section CDATA ou commentaire ne s'y
/// mêle.
struct PendingText {
    text: String,
    pure: bool,
}

impl PendingText {
    fn flush(&mut self, counts: &mut HashMap<Tag, usize>, replacer: &Replacer, stack: &[String]) {
        let top = stack.last().map(String::as_str).unwrap_or("");
        if self.pure && STRUCTURAL_TEXT_ELEMENTS.contains(&top) {
            credit(counts, replacer, &self.text);
        }
        self.text.clear();
        self.pure = true;
    }
}

/// Occurrences structurelles d'une partie XML. `None` si elle est mal formée :
/// rien n'est alors retiré pour cette partie.
fn part_counts(src: &str, replacer: &Replacer) -> Option<HashMap<Tag, usize>> {
    let mut reader = Reader::from_str(src);
    reader.config_mut().check_end_names = false;
    let mut counts: HashMap<Tag, usize> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending = PendingText {
        text: String::new(),
        pure: true,
    };
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                pending.flush(&mut counts, replacer, &stack);
                credit_attributes(&mut counts, replacer, &e);
                stack.push(local_name(e.name().as_ref()).to_string());
            }
            Event::Empty(e) => {
                pending.flush(&mut counts, replacer, &stack);
                credit_attributes(&mut counts, replacer, &e);
            }
            Event::End(_) => {
                pending.flush(&mut counts, replacer, &stack);
                stack.pop()?;
            }
            Event::Text(t) => pending.text.push_str(&t.xml10_content()),
            Event::Eof if stack.is_empty() => break,
            Event::Eof => return None,
            // Référence d'entité, CDATA, commentaire : le texte n'est plus sûr.
            _ => pending.pure = false,
        }
    }
    Some(counts)
}

/// Occurrences de chaque balise dans les emplacements structurels de chaque
/// partie XML d'un paquet (clé : emplacement tel que donné par `deep_scan`).
/// Vide si les octets ne sont pas un paquet lisible ; les fichiers embarqués ne
/// sont pas parcourus (rien n'y est retiré).
pub(crate) fn structural_counts(
    bytes: &[u8],
    replacer: &Replacer,
) -> HashMap<(String, Tag), usize> {
    let mut out = HashMap::new();
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return out;
    };
    if archive.len() > MAX_ENTRIES {
        return out;
    }
    let mut budget = MAX_TOTAL_UNCOMPRESSED;
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        let Ok(name) = entry.name().map(|n| n.into_owned()) else {
            continue;
        };
        let lname = name.to_ascii_lowercase();
        let ext = lname.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if entry.is_dir()
            || lname.contains("/embeddings/")
            || !matches!(ext, "xml" | "rels" | "vml" | "rdf")
        {
            continue;
        }
        let limit = MAX_ENTRY_UNCOMPRESSED.min(budget);
        if entry.size() > limit {
            continue;
        }
        let mut raw = Vec::new();
        if (&mut entry).take(limit).read_to_end(&mut raw).is_err() {
            continue;
        }
        budget = budget.saturating_sub(raw.len() as u64);
        let body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
        let src = String::from_utf8_lossy(body);
        let Some(counts) = part_counts(&src, replacer) else {
            continue;
        };
        let part = label(&name, i);
        for (tag, n) in counts {
            out.insert((part.clone(), tag), n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::{EntityKind, Session, Source};
    use anon_replace::SessionExt;

    fn replacer(values: &[(EntityKind, &str)]) -> (Replacer, Vec<Tag>) {
        let mut s = Session::new();
        let tags = values
            .iter()
            .filter_map(|(k, v)| s.intern(*k, v, Source::Manual))
            .collect();
        (Replacer::new(&s), tags)
    }

    #[test]
    fn structural_values() {
        for ok in [
            "19",
            "A19",
            "1:19",
            "$B$2:$C$19",
            "0.75",
            "FF000019",
            "B2 C5",
        ] {
            assert!(structural_value(ok), "{ok}");
        }
        for ko in ["", "ABC", "Dupont 19", "rId19", "19 ans", "é19"] {
            assert!(!structural_value(ko), "{ko}");
        }
    }

    #[test]
    fn counts_only_listed_locations() {
        let (r, tags) = replacer(&[(EntityKind::Age, "19")]);
        let tag = tags[0];
        let xml = r#"<worksheet><sheetData>
            <row r="19" spans="1:19"><c r="A19" s="19"><v>19</v></c></row>
            </sheetData>
            <autoFilter><filterColumn><filters><filter val="19"/></filters></filterColumn></autoFilter>
            <x:sz val="19"/><pivot><n v="19"/></pivot>
            <xdr:row>19</xdr:row><t>19</t></worksheet>"#;
        let counts = part_counts(xml, &r).unwrap();
        // r="19", spans="1:19", s="19", <sz val>, <xdr:row> ; pas la valeur
        // de la cellule, ni le filtre, ni le cache, ni le texte.
        assert_eq!(counts.get(&tag), Some(&5));
    }

    #[test]
    fn malformed_part_gives_nothing() {
        let (r, _) = replacer(&[(EntityKind::Age, "19")]);
        assert!(part_counts(r#"<row r="19"><c>"#, &r).is_none());
        assert!(structural_counts(b"pas un zip", &r).is_empty());
    }

    #[test]
    fn labels_follow_anon_office() {
        assert_eq!(
            label("xl/worksheets/sheet1.xml", 3),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(label("xl/données.xml", 3), "entrée n° 4");
    }
}
