//! Listes blanches : ce qui passe par `tr` et ce qui est neutralisé.
//!
//! Les éléments et attributs sont désignés par leur nom canonique (voir
//! `xml.rs`) : `w:` WordprocessingML, `a:` DrawingML, `p:` PresentationML,
//! `x:` SpreadsheetML, `c:` graphiques, `rel:` relations, `ct:` types de
//! contenu, `dc:`/`cp:` propriétés principales, `ep:` propriétés étendues,
//! `cust:` propriétés personnalisées, `vt:` types de variantes. Tout attribut
//! absent de ces listes n'est jamais modifié.

use anon_core::BlockRole;

/// Couples élément/attribut dont la valeur passe par `tr` comme du texte.
///
/// - textes alternatifs des images et formes (`descr`, `title`) ;
/// - éléments partagés des caches de tableaux croisés dynamiques
///   (`pivotCacheDefinition` et `pivotCacheRecords` : `s@v`).
///
/// S'y ajoutent, traités à part car ils demandent une logique propre :
/// `rel:Relationship@Target` quand `TargetMode="External"` (via
/// `TextTransform::apply_uri`) et `x:tableColumn@name` (même texte que la
/// cellule d'en-tête, repris dans les références structurées).
pub const TEXT_ATTRIBUTES: &[(&str, &str, BlockRole)] = &[
    ("wp:docPr", "descr", BlockRole::Other),
    ("wp:docPr", "title", BlockRole::Other),
    ("p:cNvPr", "descr", BlockRole::Other),
    ("p:cNvPr", "title", BlockRole::Other),
    ("pic:cNvPr", "descr", BlockRole::Other),
    ("pic:cNvPr", "title", BlockRole::Other),
    ("xdr:cNvPr", "descr", BlockRole::Other),
    ("xdr:cNvPr", "title", BlockRole::Other),
    ("x:s", "v", BlockRole::Cell),
];

/// Noms de colonnes Excel : reçoivent exactement le texte de la cellule
/// d'en-tête transformée (table de correspondance calculée sur les
/// `tableColumn@name`), sans quoi Excel déclare le fichier corrompu. Les mêmes
/// noms sont renommés dans les références structurées des formules.
pub const COLUMN_NAME_ATTRIBUTES: &[(&str, &str)] =
    &[("x:tableColumn", "name"), ("x:cacheField", "name")];

/// Attributs neutralisés quand `scrub_metadata` : élément (`*` = tout
/// élément), attribut, valeur neutre. Auteurs et initiales de commentaires et
/// de révisions, personnes (`people.xml`, `persons/person.xml`), auteurs de
/// commentaires PowerPoint (`commentAuthors.xml`, `authors.xml`), chemin
/// absolu d'enregistrement Excel (`x15ac:absPath`).
pub const SCRUBBED_ATTRIBUTES: &[(&str, &str, &str)] = &[
    ("*", "w:author", NEUTRAL_AUTHOR),
    ("*", "w:initials", NEUTRAL_INITIALS),
    ("w15:person", "w15:author", NEUTRAL_AUTHOR),
    ("w15:presenceInfo", "w15:userId", ""),
    ("w15:presenceInfo", "w15:providerId", "None"),
    ("p:cmAuthor", "name", NEUTRAL_AUTHOR),
    ("p:cmAuthor", "initials", NEUTRAL_INITIALS),
    ("p188:author", "name", NEUTRAL_AUTHOR),
    ("p188:author", "initials", NEUTRAL_INITIALS),
    ("p188:author", "userId", ""),
    ("p188:author", "providerId", ""),
    ("tc:person", "displayName", NEUTRAL_AUTHOR),
    ("tc:person", "userId", ""),
    ("tc:person", "providerId", "None"),
    ("x15ac:absPath", "url", ""),
];

/// Nom d'auteur écrit à la place des auteurs retirés.
pub const NEUTRAL_AUTHOR: &str = "Auteur";
/// Initiales écrites à la place des initiales retirées.
pub const NEUTRAL_INITIALS: &str = "A";
/// Cible écrite à la place du modèle attaché (`settings.xml.rels`).
pub const NEUTRAL_TEMPLATE: &str = "Normal.dotm";

/// Traitement d'un élément dont tout le texte forme un bloc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementRule {
    /// Passe par `tr` avec ce rôle.
    Text(BlockRole),
    /// Formule : `tr` seulement sur les littéraux de chaîne.
    Formula,
    /// Vidé quand `scrub_metadata` ; bloc `Metadata` à l'extraction.
    Scrub,
}

/// Éléments dont tout le texte forme un bloc : nom, parent direct exigé,
/// traitement.
pub const TEXT_ELEMENTS: &[(&str, Option<&str>, ElementRule)] = &[
    // Propriétés principales.
    ("dc:title", None, ElementRule::Text(BlockRole::Metadata)),
    ("dc:subject", None, ElementRule::Text(BlockRole::Metadata)),
    ("cp:keywords", None, ElementRule::Text(BlockRole::Metadata)),
    (
        "dc:description",
        None,
        ElementRule::Text(BlockRole::Metadata),
    ),
    ("dc:creator", None, ElementRule::Scrub),
    ("cp:lastModifiedBy", None, ElementRule::Scrub),
    // Propriétés étendues.
    ("ep:Company", None, ElementRule::Scrub),
    ("ep:Manager", None, ElementRule::Scrub),
    // Titres des parties (Word, PowerPoint ; pour Excel ce sont des noms de feuilles).
    (
        "vt:lpstr",
        Some("vt:vector"),
        ElementRule::Text(BlockRole::Metadata),
    ),
    // Propriétés personnalisées (`docProps/custom.xml`).
    (
        "vt:lpwstr",
        Some("cust:property"),
        ElementRule::Text(BlockRole::Metadata),
    ),
    (
        "vt:lpstr",
        Some("cust:property"),
        ElementRule::Text(BlockRole::Metadata),
    ),
    (
        "vt:bstr",
        Some("cust:property"),
        ElementRule::Text(BlockRole::Metadata),
    ),
    // Excel : auteurs de commentaires.
    ("x:author", Some("x:authors"), ElementRule::Scrub),
    // Excel : formules (cellules, noms définis, validations, mises en forme
    // conditionnelles, colonnes calculées).
    ("x:f", Some("x:c"), ElementRule::Formula),
    ("x:definedName", None, ElementRule::Formula),
    ("x:formula1", None, ElementRule::Formula),
    ("x:formula2", None, ElementRule::Formula),
    ("x:formula", None, ElementRule::Formula),
    ("xm:f", None, ElementRule::Formula),
    ("x:calculatedColumnFormula", None, ElementRule::Formula),
    ("x:totalsRowFormula", None, ElementRule::Formula),
    // Excel : en-têtes et pieds d'impression.
    ("x:oddHeader", None, ElementRule::Text(BlockRole::Header)),
    ("x:evenHeader", None, ElementRule::Text(BlockRole::Header)),
    ("x:firstHeader", None, ElementRule::Text(BlockRole::Header)),
    ("x:oddFooter", None, ElementRule::Text(BlockRole::Footer)),
    ("x:evenFooter", None, ElementRule::Text(BlockRole::Footer)),
    ("x:firstFooter", None, ElementRule::Text(BlockRole::Footer)),
    // Commentaires en fil de discussion (Excel), commentaires PowerPoint anciens.
    ("tc:text", None, ElementRule::Text(BlockRole::Comment)),
    ("p:text", None, ElementRule::Text(BlockRole::Comment)),
    // Graphiques : nom de série littéral.
    ("c:v", Some("c:tx"), ElementRule::Text(BlockRole::Body)),
    // OpenDocument : `meta.xml`.
    ("meta:initial-creator", None, ElementRule::Scrub),
    ("meta:printed-by", None, ElementRule::Scrub),
    ("meta:user-defined", None, ElementRule::Scrub),
    ("meta:editing-cycles", None, ElementRule::Scrub),
    ("meta:editing-duration", None, ElementRule::Scrub),
    ("meta:keyword", None, ElementRule::Text(BlockRole::Metadata)),
    // OpenDocument : champs d'auteur (les champs `text:sender-*` sont traités
    // de même, voir `ODF_SENDER_PREFIX`).
    ("text:author-name", None, ElementRule::Scrub),
    ("text:author-initials", None, ElementRule::Scrub),
    // OpenDocument : textes alternatifs des cadres.
    ("svg:title", None, ElementRule::Text(BlockRole::Other)),
    ("svg:desc", None, ElementRule::Text(BlockRole::Other)),
];

/// Préfixe des champs OpenDocument « expéditeur » (nom, adresse, téléphone…),
/// vidés quand `scrub_metadata`.
pub const ODF_SENDER_PREFIX: &str = "text:sender-";

/// Paragraphes OpenDocument : tout texte situé dans le paragraphe ou dans un
/// de ses éléments `text:` (`text:span`, `text:a`…) en fait partie.
pub const ODF_PARAGRAPH_ELEMENTS: &[&str] = &["text:p", "text:h"];

/// Éléments OpenDocument rendus par un caractère (`text:s` : autant d'espaces
/// que `text:c`).
pub const ODF_VIRTUAL_ELEMENTS: &[(&str, &str)] = &[
    ("text:tab", " "),
    ("text:line-break", "\n"),
    ("text:s", " "),
];

/// Zones de texte VML (commentaires et zones de texte anciens) : tout leur
/// texte forme un bloc.
pub const VML_TEXT_ELEMENTS: &[&str] = &["v:textbox"];

/// Caches numériques : si `tr` y trouve une valeur, elle est remplacée par
/// `0` et un avertissement `UntouchedLocation` est émis. Éléments `c:pt` des
/// caches de graphiques (parent exigé), et attribut `v` des éléments `n` des
/// caches de tableaux croisés dynamiques.
pub const NUMERIC_CACHE_POINTS: &[&str] = &["c:numCache", "c:numLit"];
/// Voir [`NUMERIC_CACHE_POINTS`].
pub const NUMERIC_CACHE_ATTRIBUTES: &[(&str, &str)] = &[("x:n", "v")];

/// Attributs d'instruction de champ : seuls les littéraux entre guillemets
/// passent par `tr` (par `apply_uri` pour un champ `HYPERLINK`).
pub const FIELD_INSTRUCTION_ATTRIBUTES: &[(&str, &str)] = &[("w:fldSimple", "w:instr")];

/// Attributs de lien OpenDocument : passent par `apply_uri` s'ils portent un
/// schéma (`mailto:`, `https:`…), jamais s'ils désignent une partie du paquet.
pub const ODF_LINK_ATTRIBUTES: &[&str] = &["xlink:href"];

/// Attributs de valeur d'une cellule OpenDocument, retirés quand la cellule
/// devient une chaîne.
pub const ODF_VALUE_ATTRIBUTES: &[&str] = &[
    "office:value",
    "office:date-value",
    "office:time-value",
    "office:boolean-value",
];

/// Éléments paragraphes (texte réparti sur des runs) : nom, parent exigé.
pub const PARAGRAPH_ELEMENTS: &[(&str, Option<&str>)] = &[
    ("w:p", None),
    ("a:p", None),
    ("x:si", None),
    ("x:rPh", None),
    ("x:is", None),
    ("x:text", Some("x:comment")),
    ("c:pt", Some("c:strCache")),
    ("c:pt", Some("c:strLit")),
    ("c:pt", Some("c:lvl")),
];

/// Éléments porteurs de texte dans un paragraphe : nom, parent exigé, pose de
/// `xml:space="preserve"` en cas de modification.
pub const RUN_TEXT_ELEMENTS: &[(&str, Option<&str>, bool)] = &[
    ("w:t", None, true),
    ("w:delText", None, true),
    ("w:instrText", None, true),
    ("a:t", None, false),
    ("x:t", Some("x:si"), true),
    ("x:t", Some("x:r"), true),
    ("x:t", Some("x:is"), true),
    ("x:t", Some("x:text"), true),
    ("x:t", Some("x:rPh"), true),
    ("c:v", Some("c:pt"), false),
];

/// Éléments rendus par un caractère dans le texte du paragraphe : nom,
/// parent exigé, texte.
pub const VIRTUAL_ELEMENTS: &[(&str, &str, &str)] = &[
    ("w:tab", "w:r", " "),
    ("w:br", "w:r", "\n"),
    ("w:cr", "w:r", "\n"),
    ("a:br", "a:p", "\n"),
];
