//! Fiche d'évaluation (section 4.5 du plan de la V2, § 41 des lignes
//! directrices 02/2026 du CEPD) : document Markdown à conserver avec le dossier.
//!
//! La fiche ne contient que des balises, des catégories et des nombres : jamais
//! une valeur réelle ni une forme généralisée. Seuls les champs saisis par
//! l'utilisateur (base légale, finalité, justifications) et, s'il le demande,
//! les noms des fichiers y sont repris tels quels. Le mot « anonyme » n'y
//! figure que si le verdict est `Verdict::Anonymous` (invariant 9) : ailleurs,
//! il est retiré même des champs saisis.

use crate::analysis::Analysis;
use crate::error::extension_of;
use crate::evaluation::Techniques;
use anon_core::assess::REASSESS_MONTHS;
use anon_core::{
    Approach, Assessment, ContextAnswers, Criterion, EntityKind, FailureReason, Goal, Recipient,
    SubjectRef, Tag, Verdict,
};
use anon_dict::hierarchy::max_level;
use anon_vault::retention::add_months;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Nombre maximal de sujets détaillés dans le tableau de la fiche (les autres
/// sont comptés).
pub const FICHE_MAX_SUBJECTS: usize = 200;

/// Mot réservé au verdict d'anonymat (invariant 9).
const RESERVED_WORD: &str = "anonyme";
/// Remplace le mot réservé dans un champ saisi quand le verdict ne le permet pas.
const RESERVED_WORD_REPLACEMENT: &str = "[terme retiré]";

/// Document traité, tel que la fiche le mentionne.
#[derive(Clone, PartialEq, Eq)]
pub struct FicheDocument {
    /// Extension en minuscules (vide pour un texte collé).
    pub extension: String,
    /// Nom du fichier, seulement si l'utilisateur a demandé à le faire figurer.
    pub name: Option<String>,
}

impl fmt::Debug for FicheDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FicheDocument")
            .field("extension", &self.extension)
            .field("name", &self.name.is_some())
            .finish()
    }
}

/// Renseignements de la fiche qui ne viennent pas de l'évaluation elle-même.
#[derive(Clone, PartialEq)]
pub struct FicheMeta {
    /// Date de l'évaluation, `AAAA-MM-JJ`.
    pub date: String,
    pub tool_version: String,
    pub goal: Goal,
    pub recipient: Recipient,
    /// Réponses aux questions de l'approche contextuelle.
    pub answers: ContextAnswers,
    /// Justification des réponses qui écartent un risque (saisie).
    pub answers_justification: String,
    pub documents: Vec<FicheDocument>,
    /// Base légale de l'anonymisation (saisie, § 38).
    pub legal_basis: String,
    /// Finalité (saisie).
    pub purpose: String,
    /// Risques résiduels acceptés (saisis).
    pub accepted_risks: String,
    /// Justification des risques acceptés (saisie).
    pub risks_justification: String,
    pub techniques: Techniques,
    /// Balise de chaque valeur masquée (identifiant de valeur → balise), pour
    /// désigner les personnes.
    pub tags: BTreeMap<usize, Tag>,
}

impl fmt::Debug for FicheMeta {
    /// N'affiche ni les champs saisis ni les noms de fichiers.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FicheMeta")
            .field("date", &self.date)
            .field("tool_version", &self.tool_version)
            .field("goal", &self.goal)
            .field("recipient", &self.recipient)
            .field("answers", &self.answers)
            .field("documents", &self.documents)
            .field("techniques", &self.techniques)
            .field("tags", &self.tags.len())
            .finish_non_exhaustive()
    }
}

impl FicheMeta {
    /// Fiche préremplie à partir de l'analyse et de l'évaluation, datée de
    /// `date` (`AAAA-MM-JJ`, par exemple `anon_vault::retention::today()`).
    /// Les noms des fichiers n'y figurent que si `with_file_names`. Les champs
    /// saisis (base légale, finalité, justifications) restent vides.
    pub fn new(
        analysis: &Analysis,
        assessment: &Assessment,
        date: &str,
        with_file_names: bool,
    ) -> Self {
        Self {
            date: date.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            goal: assessment.settings.goal,
            recipient: assessment.settings.recipient,
            answers: assessment.settings.answers,
            answers_justification: String::new(),
            documents: analysis
                .files
                .iter()
                .map(|f| FicheDocument {
                    extension: extension_of(&f.name),
                    name: with_file_names.then(|| f.name.clone()),
                })
                .collect(),
            legal_basis: String::new(),
            purpose: String::new(),
            accepted_risks: String::new(),
            risks_justification: String::new(),
            techniques: analysis.techniques(),
            tags: analysis.preview_tags(&analysis.base).into_iter().collect(),
        }
    }
}

/// `AAAA-MM-JJ` → `JJ/MM/AAAA` ; une date mal formée est reprise telle quelle.
fn french_date(date: &str) -> String {
    match date.split('-').collect::<Vec<_>>().as_slice() {
        [y, m, d] if y.len() == 4 && m.len() == 2 && d.len() == 2 => format!("{d}/{m}/{y}"),
        _ => date.to_string(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n > 1 {
        format!("{n} {many}")
    } else {
        format!("{n} {one}")
    }
}

/// Destinataire dans une phrase (« pour un assistant IA grand public »).
fn recipient_phrase(r: Recipient) -> &'static str {
    match r {
        Recipient::PublicAi => "un assistant IA grand public",
        Recipient::OrganizationAi => "l'IA de votre organisation, sous contrat",
        Recipient::ExternalShare => "un partage externe",
        Recipient::Publication => "une publication ouverte",
    }
}

fn goal_detail(g: Goal) -> &'static str {
    match g {
        Goal::Mask => "pseudonymisation, réversible avec la clé de restauration",
        Goal::Anonymize => "aucune clé de restauration créée",
    }
}

fn approach_detail(a: Approach) -> &'static str {
    match a {
        Approach::Simplified => "toute voie de réidentification compte, quel que soit l'attaquant",
        Approach::Contextual => {
            "les réponses sur le destinataire, consignées ci-dessous, écartent certains risques"
        }
    }
}

/// Type de document (singulier, pluriel) d'après l'extension.
fn document_kind(ext: &str) -> (&'static str, &'static str) {
    match ext {
        "docx" | "docm" | "dotx" => ("document Word", "documents Word"),
        "xlsx" | "xlsm" | "xltx" => ("classeur Excel", "classeurs Excel"),
        "pptx" | "pptm" | "potx" => ("présentation PowerPoint", "présentations PowerPoint"),
        "odt" => ("texte OpenDocument", "textes OpenDocument"),
        "ods" => ("classeur OpenDocument", "classeurs OpenDocument"),
        "odp" => ("présentation OpenDocument", "présentations OpenDocument"),
        "pdf" => ("document PDF", "documents PDF"),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "webp" => ("image", "images"),
        "csv" | "tsv" => ("tableau CSV", "tableaux CSV"),
        "" => ("texte collé", "textes collés"),
        _ => ("fichier texte", "fichiers texte"),
    }
}

/// Niveau de généralisation en clair (hiérarchies d'`anon_dict::hierarchy`).
fn level_label(kind: EntityKind, level: Option<u8>) -> String {
    let Some(level) = level else {
        return "forme choisie à la main".into();
    };
    if level > max_level(kind) {
        return "valeur remplacée par « * »".into();
    }
    let named = match (kind, level) {
        (EntityKind::Date | EntityKind::DateNaiss, 1) => "mois et année",
        (EntityKind::Date | EntityKind::DateNaiss, 2) => "année",
        (EntityKind::Date | EntityKind::DateNaiss, 3) => "décennie",
        (EntityKind::Age, 1) => "tranche de 5 ans",
        (EntityKind::Age, 2) => "tranche de 10 ans",
        (EntityKind::Lieu, 1) => "département, province ou code postal à deux chiffres",
        (EntityKind::Lieu, 2) => "région",
        (EntityKind::Lieu, 3) => "territoire",
        (EntityKind::Montant, 1) => "tranche du premier chiffre",
        (EntityKind::Montant, 2) => "ordre de grandeur",
        (EntityKind::Montant, 3) => "deux ordres de grandeur",
        (EntityKind::Fonction, 1) => "catégorie plus large",
        (EntityKind::Fonction, 2) => "« professionnel »",
        _ => "",
    };
    if named.is_empty() {
        format!("niveau {level}")
    } else {
        format!("niveau {level}, {named}")
    }
}

/// Champ saisi par l'utilisateur : une ligne, `|` neutralisé, mot réservé
/// retiré si le verdict ne le permet pas.
fn user_text(s: &str, anonymous: bool) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let flat = flat.replace('|', "/");
    if anonymous {
        return flat;
    }
    flat.split(' ')
        .map(|w| {
            if anon_replace::key(w).contains(RESERVED_WORD) {
                RESERVED_WORD_REPLACEMENT
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn filled(s: &str, anonymous: bool) -> String {
    let t = user_text(s, anonymous);
    if t.trim().is_empty() {
        "à compléter".into()
    } else {
        t
    }
}

fn yes_no(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "oui",
        Some(false) => "non",
        None => "sans réponse (approche simplifiée sur ce point)",
    }
}

fn subject_label(subject: &SubjectRef, meta: &FicheMeta) -> String {
    match subject {
        SubjectRef::Person { value_id } => match meta.tags.get(value_id) {
            Some(tag) => format!("`{tag}`"),
            None => format!("personne non masquée (réf. {value_id})"),
        },
        SubjectRef::TableGroup { table, group } => format!(
            "tableau {}, groupe {}",
            table.saturating_add(1),
            group.saturating_add(1)
        ),
        SubjectRef::Image { file, media } => {
            format!("image `{media}` (document {})", file.saturating_add(1))
        }
    }
}

fn reason_text(subject: &SubjectRef, reason: &FailureReason, k_min: u32) -> String {
    match reason {
        FailureReason::DirectIdentifier { kind } => {
            format!("identifiant direct visible ({})", kind.label())
        }
        FailureReason::SmallGroup { estimate } => {
            let n = usize::try_from(*estimate).unwrap_or(usize::MAX);
            let size = match subject {
                SubjectRef::TableGroup { .. } => plural(n, "ligne", "lignes"),
                _ => format!("estimé à {}", plural(n, "personne", "personnes")),
            };
            format!("groupe de {size}, moins de {k_min}")
        }
        FailureReason::StableIdentifier { kind } => {
            format!("identifiant stable visible ({})", kind.label())
        }
        FailureReason::SensitiveLinked { kind } => format!(
            "donnée sensible ou événement personnel ({}) lié à une personne isolable",
            kind.label()
        ),
        FailureReason::HomogeneousGroup { column } => format!(
            "valeur sensible commune à tout le groupe (colonne {})",
            column.saturating_add(1)
        ),
        FailureReason::FaceVisible => "visage visible".into(),
        FailureReason::FacesNotChecked => {
            "visages non vérifiés (édition sans détection des visages)".into()
        }
    }
}

fn verdict_text(a: &Assessment, meta: &FicheMeta) -> String {
    let to = recipient_phrase(meta.recipient);
    let date = french_date(&meta.date);
    let approach = a.approach.label().to_lowercase();
    match a.verdict {
        Verdict::Anonymous => {
            let mut s = format!("Anonyme pour {to}, évaluation du {date}, {approach}.");
            if meta.goal == Goal::Mask {
                s.push_str(
                    " Pour vous, qui gardez la clé de restauration, le dossier reste une \
                     donnée personnelle (pseudonymisation).",
                );
            }
            s
        }
        Verdict::Reidentifiable { subjects } => format!(
            "{} pour {to} : le dossier reste une donnée personnelle (un seul sujet \
             reconnaissable suffit, § 36). Évaluation du {date}, {approach}.",
            if subjects > 1 {
                format!("{subjects} sujets restent reconnaissables")
            } else {
                format!("{subjects} sujet reste reconnaissable")
            }
        ),
        Verdict::Inconclusive => format!(
            "Évaluation non concluante pour {to} : des images n'ont pas pu être vérifiées \
             (visages non vérifiés). Le dossier reste traité comme une donnée personnelle. \
             Évaluation du {date}, {approach}."
        ),
    }
}

/// Fiche d'évaluation au format Markdown.
pub fn render_markdown(assessment: &Assessment, meta: &FicheMeta) -> String {
    let anonymous = assessment.verdict == Verdict::Anonymous;
    let k_min = assessment.settings.k_min;
    let mut md = String::new();
    let mut line = |s: &str| {
        md.push_str(s);
        md.push('\n');
    };

    line("# Fiche d'évaluation");
    line("");
    line(&format!(
        "Évaluation du {} · Anonymiseur {}",
        french_date(&meta.date),
        meta.tool_version
    ));
    line("");
    line("## Conclusion");
    line("");
    line(&verdict_text(assessment, meta));
    line("");

    line("## Contexte");
    line("");
    line("| Élément | Valeur |");
    line("|---|---|");
    line(&format!(
        "| Objectif | {} ({}) |",
        meta.goal.label(),
        goal_detail(meta.goal)
    ));
    line(&format!("| Destinataire | {} |", meta.recipient.label()));
    line(&format!(
        "| Approche | {} : {} |",
        assessment.approach.label(),
        approach_detail(assessment.approach)
    ));
    line(&format!(
        "| Taille minimale d'un groupe | {} |",
        plural(
            usize::try_from(k_min).unwrap_or(usize::MAX),
            "personne",
            "personnes"
        )
    ));
    line(
        "| Référentiel | Projet de lignes directrices 02/2026 du CEPD sur l'anonymisation \
         (version 1.0 du 7 juillet 2026, soumise à consultation) : isolement, recoupement, \
         déduction ; approche simplifiée de la version 2.0 de l'outil |",
    );
    line("");
    if assessment.approach == Approach::Contextual {
        line("### Réponses sur le destinataire");
        line("");
        line(&format!(
            "- Le destinataire connaît-il les personnes ? {}",
            yes_no(meta.answers.recipient_knows_people)
        ));
        line(&format!(
            "- Le document sera-t-il publié ? {}",
            yes_no(meta.answers.published)
        ));
        line(&format!(
            "- D'autres documents du même dossier lui seront-ils envoyés ? {}",
            yes_no(meta.answers.other_documents)
        ));
        line(&format!(
            "- Justification : {}",
            filled(&meta.answers_justification, anonymous)
        ));
        line("");
    }

    line("## Documents traités");
    line("");
    let mut kinds: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for d in &meta.documents {
        let n = kinds.entry(document_kind(&d.extension)).or_insert(0);
        *n = n.saturating_add(1);
    }
    if kinds.is_empty() {
        line("Aucun document.");
    }
    for ((one, many), n) in &kinds {
        line(&format!("- {}", plural(*n, one, many)));
    }
    let names: Vec<String> = meta
        .documents
        .iter()
        .filter_map(|d| d.name.as_deref())
        .map(|n| user_text(n, anonymous))
        .collect();
    if !names.is_empty() {
        line("");
        line("Noms des fichiers (mentionnés à votre demande) :");
        line("");
        for n in names {
            line(&format!("- {n}"));
        }
    }
    line("");

    line("## Techniques appliquées");
    line("");
    let t = &meta.techniques;
    let masked: usize = t.masked.values().sum();
    if masked > 0 {
        let detail = t
            .masked
            .iter()
            .map(|(k, n)| format!("{} : {n}", k.label()))
            .collect::<Vec<_>>()
            .join(", ");
        line(&format!(
            "- Masquage (remplacement par une balise) : {} ({detail})",
            plural(masked, "valeur", "valeurs")
        ));
    }
    if !t.generalized.is_empty() {
        line("- Rendre moins précis (généralisation) :");
        for ((kind, level), n) in &t.generalized {
            line(&format!(
                "  - {}, {} : {}",
                kind.label(),
                level_label(*kind, *level),
                plural(*n, "valeur", "valeurs")
            ));
        }
    }
    if t.suppressed_rows > 0 {
        line(&format!(
            "- Suppression de lignes : {} (indices masqués, lignes retirées de l'évaluation)",
            plural(t.suppressed_rows, "ligne", "lignes")
        ));
    }
    if t.masked_regions > 0 {
        line(&format!("- Zones d'image masquées : {}", t.masked_regions));
    }
    if masked == 0 && t.generalized.is_empty() && t.suppressed_rows == 0 && t.masked_regions == 0 {
        line("Aucune technique appliquée.");
    }
    line("");

    line("## Résultat par critère");
    line("");
    line("| Critère | Sujets qui passent | Sujets en échec |");
    line("|---|---|---|");
    for c in Criterion::ALL {
        let (ok, ko) = assessment.summary(c);
        line(&format!("| {} | {ok} | {ko} |", c.label()));
    }
    line("");
    for c in Criterion::ALL {
        line(&format!("### {}", c.label()));
        line("");
        let mut any = false;
        for f in assessment.failures_of(c) {
            any = true;
            line(&format!(
                "- {} : {}",
                subject_label(&f.subject, meta),
                reason_text(&f.subject, &f.reason, k_min)
            ));
        }
        if !any {
            line("Aucun échec.");
        }
        line("");
    }

    line("## Sujets évalués");
    line("");
    if assessment.subjects.is_empty() {
        line("Aucun sujet : aucune personne, aucun tableau à évaluer, aucune image.");
    } else {
        line("| Sujet | Isolement | Recoupement | Déduction | Groupe estimé | Indices visibles |");
        line("|---|---|---|---|---|---|");
        for s in assessment.subjects.iter().take(FICHE_MAX_SUBJECTS) {
            let result = |c: Criterion| {
                if s.passed.get(c.index()).copied().unwrap_or(false) {
                    "réussi"
                } else {
                    "échec"
                }
            };
            let clues: BTreeSet<&str> = s.clues.iter().map(|(k, _)| k.label()).collect();
            let clues = if clues.is_empty() {
                "aucun".to_string()
            } else {
                clues.into_iter().collect::<Vec<_>>().join(", ")
            };
            line(&format!(
                "| {} | {} | {} | {} | {} | {clues} |",
                subject_label(&s.subject, meta),
                result(Criterion::Isolation),
                result(Criterion::Linkage),
                result(Criterion::Inference),
                s.group_estimate
                    .map_or_else(|| "non estimé".to_string(), |n| n.to_string()),
            ));
        }
        let rest = assessment.subjects.len().saturating_sub(FICHE_MAX_SUBJECTS);
        if rest > 0 {
            line("");
            line(&format!(
                "Et {} non détaillés ici.",
                plural(rest, "autre sujet", "autres sujets")
            ));
        }
        line("");
        line(
            "Les indices visibles sont ce qu'une IA pourrait croiser pour deviner qui est \
             concerné (§ 82 et 83).",
        );
    }
    line("");

    line("## Risques résiduels acceptés");
    line("");
    line(&format!(
        "- Risques acceptés : {}",
        filled(&meta.accepted_risks, anonymous)
    ));
    line(&format!(
        "- Justification : {}",
        filled(&meta.risks_justification, anonymous)
    ));
    line(
        "- Hors de portée de l'outil : le recoupement avec des données externes qu'il ne \
         connaît pas, et les indices de style (tournures, fautes récurrentes) qui trahissent \
         un auteur.",
    );
    line("");

    line("## Base légale et finalité");
    line("");
    line(&format!(
        "- Base légale : {}",
        filled(&meta.legal_basis, anonymous)
    ));
    line(&format!(
        "- Finalité : {}",
        filled(&meta.purpose, anonymous)
    ));
    line("");

    line("## Réévaluation");
    line("");
    match add_months(&meta.date, REASSESS_MONTHS) {
        Some(d) => line(&format!(
            "Réévaluation conseillée au plus tard le {} ({REASSESS_MONTHS} mois, § 35).",
            french_date(&d)
        )),
        None => line(&format!(
            "Réévaluation conseillée dans {REASSESS_MONTHS} mois (§ 35)."
        )),
    }
    line("");
    line("Réévaluez plus tôt en cas de :");
    line("");
    line("- incident de sécurité ;");
    line("- nouvelle publication de données sur les mêmes personnes ;");
    line("- diffusion plus large que le destinataire prévu.");
    line("");

    line("## Responsabilité");
    line("");
    line(
        "L'outil documente une évaluation prudente, à titre d'aide à la décision ; ses \
         estimations donnent un ordre de grandeur. Le responsable de traitement décide et \
         répond de sa décision (RGPD, art. 5, § 2, et art. 24). Conservez cette fiche avec le \
         dossier (§ 41).",
    );
    line("");
    line("Cette fiche ne contient que des balises, des catégories et des nombres.");
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_levels() {
        assert_eq!(french_date("2026-11-15"), "15/11/2026");
        assert_eq!(french_date("bientôt"), "bientôt");
        assert_eq!(
            level_label(EntityKind::Age, Some(2)),
            "niveau 2, tranche de 10 ans"
        );
        assert_eq!(
            level_label(EntityKind::Age, Some(3)),
            "valeur remplacée par « * »"
        );
        assert_eq!(
            level_label(EntityKind::Lieu, None),
            "forme choisie à la main"
        );
    }

    #[test]
    fn reserved_word_is_removed_from_user_text() {
        assert_eq!(
            user_text("Données ANONYMES | partage", false),
            "Données [terme retiré] / partage"
        );
        assert_eq!(user_text("données anonymes", true), "données anonymes");
        assert_eq!(filled("  ", false), "à compléter");
    }
}
