//! Bilan avant envoi (écran C) : l'évaluation des trois critères du CEPD
//! (isolement, recoupement, déduction) pour le destinataire, dite en phrases :
//! verdict, une ligne par critère, un sujet par échec (indices visibles,
//! groupe estimé, corrections), ce que l'IA pourrait deviner. Réponses aux
//! questions de contexte et champs saisis de la fiche d'évaluation. Sans egui.
//!
//! L'évaluation est calculée sur le fil de travail, sur une copie de
//! l'analyse ([`AnonymizeState::revision`] dit si elle est encore à jour).
//! Invariant 9 : le mot « anonyme » n'apparaît que si elle conclut à l'anonymat.

use super::anonymize::AnonymizeState;
use super::toasts::count_label;
use crate::ui::service::{FolderAssessment, TableName};
use anon_core::assess::K_MIN_DEFAULT;
use anon_core::{
    AssessSettings, Assessment, ContextAnswers, Criterion, EntityKind, FailureReason, Fix, Goal,
    Recipient, SubjectRef, Treatment, Verdict,
};
use std::fmt;

/// Évaluation reçue du fil de travail.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessed {
    /// Révision des décisions évaluées ([`AnonymizeState::revision`]).
    pub revision: u64,
    pub assessment: Assessment,
    /// Tableaux évalués, dans l'ordre de l'évaluation.
    pub tables: Vec<TableName>,
}

/// Champs saisis de la fiche d'évaluation (facultatifs).
#[derive(Clone, PartialEq, Default)]
pub struct FicheFields {
    pub legal_basis: String,
    pub purpose: String,
    pub accepted_risks: String,
    pub risks_justification: String,
    /// Faire figurer le nom des fichiers dans la fiche.
    pub with_file_names: bool,
}

impl fmt::Debug for FicheFields {
    /// Aucun champ saisi (contenu de l'utilisateur).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FicheFields")
            .field("with_file_names", &self.with_file_names)
            .finish_non_exhaustive()
    }
}

/// État du bilan : dernière évaluation, demande en cours, contexte, fiche.
#[derive(Clone, PartialEq)]
pub struct BilanState {
    pub latest: Option<Assessed>,
    /// Évaluation demandée et pas encore reçue (révision, réglages).
    pub pending: Option<(u64, AssessSettings)>,
    /// Réponses aux trois questions de contexte (approche contextuelle).
    pub answers: ContextAnswers,
    /// Justification des réponses, reprise dans la fiche.
    pub answers_justification: String,
    /// Taille minimale d'un groupe (`K_MIN_DEFAULT`).
    pub k_min: u32,
    /// « Ce que l'IA pourrait deviner » déplié.
    pub guesses_open: bool,
    pub fiche: FicheFields,
}

impl Default for BilanState {
    fn default() -> Self {
        Self {
            latest: None,
            pending: None,
            answers: ContextAnswers::default(),
            answers_justification: String::new(),
            k_min: K_MIN_DEFAULT,
            guesses_open: false,
            fiche: FicheFields::default(),
        }
    }
}

impl fmt::Debug for BilanState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BilanState")
            .field("latest", &self.latest.as_ref().map(|a| a.revision))
            .field("pending", &self.pending.map(|p| p.0))
            .field("answers", &self.answers)
            .finish_non_exhaustive()
    }
}

impl BilanState {
    /// Réglages de l'évaluation pour cet objectif et ce destinataire.
    pub fn settings(&self, goal: Goal, recipient: Recipient) -> AssessSettings {
        AssessSettings {
            goal,
            recipient,
            answers: self.answers,
            k_min: self.k_min,
        }
    }

    /// Dernière évaluation, si elle porte sur ces décisions et ces réglages.
    pub fn current(&self, revision: u64, settings: &AssessSettings) -> Option<&Assessed> {
        self.latest
            .as_ref()
            .filter(|a| a.revision == revision && a.assessment.settings == *settings)
    }

    /// Une évaluation est à demander : la dernière est périmée et rien n'est
    /// en cours pour ces décisions et ces réglages.
    pub fn needs(&self, revision: u64, settings: &AssessSettings) -> bool {
        self.current(revision, settings).is_none() && self.pending != Some((revision, *settings))
    }

    /// Évaluation reçue. Une évaluation plus ancienne que la dernière gardée
    /// est ignorée.
    pub fn received(&mut self, revision: u64, settings: AssessSettings, result: FolderAssessment) {
        if self.pending == Some((revision, settings)) {
            self.pending = None;
        }
        if self.latest.as_ref().is_some_and(|a| a.revision > revision) {
            return;
        }
        self.latest = Some(Assessed {
            revision,
            assessment: result.assessment,
            tables: result.tables,
        });
    }

    /// Nouvelle analyse : l'évaluation repart de zéro ; le contexte et la
    /// fiche sont gardés.
    pub fn reset(&mut self) {
        self.latest = None;
        self.pending = None;
    }
}

// ---------------------------------------------------------------------------
// Phrases
// ---------------------------------------------------------------------------

/// Type d'une valeur en minuscule initiale, sigles gardés (« âge », « IBAN »).
pub fn kind_word(kind: EntityKind) -> String {
    let label = kind.label();
    let mut chars = label.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if second.is_lowercase() || !second.is_alphabetic() => {
            first.to_lowercase().chain(label.chars().skip(1)).collect()
        }
        _ => label.to_owned(),
    }
}

/// Vrai si l'évaluation ne peut pas conclure faute d'avoir vérifié les visages.
fn faces_unchecked(a: &Assessment) -> bool {
    a.failures
        .iter()
        .any(|f| f.reason == FailureReason::FacesNotChecked)
}

/// Le verdict en une phrase (vocabulaire du plan de la V2, § 7.4).
pub fn verdict_line(a: &Assessment) -> String {
    match a.verdict {
        Verdict::Anonymous => "Aucune personne reconnaissable pour ce destinataire.".to_owned(),
        Verdict::Reidentifiable { subjects: 1 } => {
            "1 personne reste reconnaissable. Corrigez-la avant l'envoi.".to_owned()
        }
        Verdict::Reidentifiable { subjects } => {
            format!("{subjects} personnes restent reconnaissables. Corrigez-les avant l'envoi.")
        }
        Verdict::Inconclusive if faces_unchecked(a) => {
            "Évaluation non concluante : visages non vérifiés (édition Lite).".to_owned()
        }
        Verdict::Inconclusive => {
            "Évaluation non concluante : des images n'ont pas pu être vérifiées.".to_owned()
        }
    }
}

/// Ce que le verdict veut dire pour l'envoi, selon l'objectif.
pub fn verdict_note(goal: Goal, a: &Assessment) -> &'static str {
    match (goal, a.is_anonymous()) {
        (Goal::Mask, true) => {
            "Le masquage protège cet envoi, et la clé de restauration vous permet de retrouver \
les vraies valeurs dans la réponse."
        }
        (Goal::Mask, false) => {
            "Vous pouvez envoyer : le masquage protège cet envoi, et la clé de restauration \
remettra les vraies valeurs dans la réponse. Corriger réduit ce que l'IA peut deviner."
        }
        (Goal::Anonymize, true) => {
            "Les fichiers seront nommés « _anonyme ». Enregistrez la fiche d'évaluation avec le \
dossier."
        }
        (Goal::Anonymize, false) => {
            "Sans correction, les fichiers seront enregistrés masqués, sans clé, sous le suffixe \
« _masque_sans_cle » : l'étape Envoyer vous demandera de confirmer."
        }
    }
}

/// Ligne de la barre d'action de l'étape Vérifier (maquette B).
pub fn short_line(a: Option<&Assessment>) -> String {
    match a.map(|a| a.verdict) {
        None => "Bilan : évaluation en cours…".to_owned(),
        Some(Verdict::Anonymous) => "Bilan : aucune personne reconnaissable".to_owned(),
        Some(Verdict::Reidentifiable { subjects: 1 }) => {
            "Bilan : 1 personne reste reconnaissable".to_owned()
        }
        Some(Verdict::Reidentifiable { subjects }) => {
            format!("Bilan : {subjects} personnes restent reconnaissables")
        }
        Some(Verdict::Inconclusive) => {
            "Bilan : évaluation non concluante (visages non vérifiés)".to_owned()
        }
    }
}

/// Résultat d'un critère en une ligne (`Assessment::summary`).
pub fn criterion_line(a: &Assessment, c: Criterion) -> String {
    let (ok, ko) = a.summary(c);
    match c {
        Criterion::Isolation => {
            let protected = count_label(ok, "personne protégée", "personnes protégées");
            if ok + ko == 0 {
                "Aucune personne à évaluer".to_owned()
            } else if ko == 0 {
                format!("{protected} · aucune isolable")
            } else if ko == 1 {
                format!("{protected} · 1 isolable")
            } else {
                format!("{protected} · {ko} isolables")
            }
        }
        Criterion::Linkage => {
            if ko == 0 {
                "Aucun identifiant stable".to_owned()
            } else {
                format!(
                    "{} par un identifiant stable",
                    count_label(ko, "personne reliable", "personnes reliables")
                )
            }
        }
        Criterion::Inference => {
            if ko == 0 {
                "Aucune donnée sensible liée à une personne isolable".to_owned()
            } else {
                format!(
                    "{} à une donnée sensible",
                    count_label(ko, "personne isolable liée", "personnes isolables liées")
                )
            }
        }
    }
}

/// Valeur telle qu'elle reste lisible : la forme moins précise si elle est
/// choisie, sinon la valeur.
fn readable(st: &AnonymizeState, id: usize) -> Option<String> {
    let c = st.candidate(id)?;
    Some(match (&c.treatment, c.selected) {
        (Treatment::Generalize(g), true) => g.clone(),
        _ => c.value.clone(),
    })
}

/// Nom d'un sujet : balise de la personne (ou sa valeur si elle reste
/// visible), tableau et groupe, image.
pub fn subject_label(st: &AnonymizeState, assessed: &Assessed, subject: &SubjectRef) -> String {
    match subject {
        SubjectRef::Person { value_id } => match st.tag_preview.get(value_id) {
            Some(tag) => tag.to_string(),
            None => readable(st, *value_id)
                .map(|v| format!("« {v} »"))
                .unwrap_or_else(|| "Personne".to_owned()),
        },
        SubjectRef::TableGroup { table, group } => {
            let name = assessed.tables.get(*table).map(|t| {
                if t.sheet.trim().is_empty() || t.sheet == t.file {
                    t.file.clone()
                } else {
                    format!("{} · {}", t.file, t.sheet)
                }
            });
            format!(
                "{} · groupe {}",
                name.unwrap_or_else(|| format!("Tableau {}", table.saturating_add(1))),
                group.saturating_add(1)
            )
        }
        SubjectRef::Image { file, media } => {
            let name = st
                .analysis
                .as_ref()
                .and_then(|a| a.files.get(*file))
                .map(|f| f.name.clone())
                .unwrap_or_default();
            if name.is_empty() || name == *media {
                format!("Image {media}")
            } else {
                format!("Image {media} · {name}")
            }
        }
    }
}

/// Raison d'un échec en une phrase.
fn reason_line(
    assessed: &Assessed,
    subject: &SubjectRef,
    reason: &FailureReason,
    k_min: u32,
) -> String {
    match reason {
        FailureReason::DirectIdentifier { kind } => {
            format!("Identifiant direct visible : {}.", kind_word(*kind))
        }
        FailureReason::SmallGroup { estimate } => {
            let n = usize::try_from(*estimate).unwrap_or(usize::MAX);
            match subject {
                SubjectRef::TableGroup { .. } => format!(
                    "Groupe de {}, moins de {k_min}.",
                    count_label(n, "ligne", "lignes")
                ),
                _ => format!(
                    "Groupe estimé : {}, moins de {k_min}.",
                    count_label(n, "personne", "personnes")
                ),
            }
        }
        FailureReason::StableIdentifier { kind } => {
            format!("Identifiant stable visible : {}.", kind_word(*kind))
        }
        FailureReason::SensitiveLinked { kind } => {
            format!("Donnée sensible liée à la personne : {}.", kind_word(*kind))
        }
        FailureReason::HomogeneousGroup { column } => {
            let header = match subject {
                SubjectRef::TableGroup { table, .. } => assessed
                    .tables
                    .get(*table)
                    .and_then(|t| t.headers.get(*column))
                    .cloned(),
                _ => None,
            };
            match header {
                Some(h) => format!("Tout le groupe partage la même valeur de « {h} »."),
                None => "Tout le groupe partage la même donnée sensible.".to_owned(),
            }
        }
        FailureReason::FaceVisible => "Un visage reste visible.".to_owned(),
        FailureReason::FacesNotChecked => {
            "Visages non vérifiés : cette édition ne les détecte pas.".to_owned()
        }
    }
}

/// Correction en clair (valeurs affichées à l'écran seulement).
pub fn fix_label(st: &AnonymizeState, fix: &Fix) -> String {
    let value = |id: usize| readable(st, id).unwrap_or_default();
    match fix {
        Fix::Mask { value_id } => match st.candidate(*value_id) {
            Some(c) => format!("Masquer « {} » ({})", value(*value_id), kind_word(c.kind)),
            None => "Masquer la valeur".to_owned(),
        },
        Fix::Generalize { value_id, to } => format!(
            "Rendre moins précis : « {} » devient « {to} »",
            value(*value_id)
        ),
        Fix::GeneralizeMany { items } => {
            let mut kinds: Vec<String> = items
                .iter()
                .filter_map(|(id, _)| st.candidate(*id).map(|c| kind_word(c.kind)))
                .collect();
            kinds.sort();
            kinds.dedup();
            let what = if items.len() == 1 {
                "Rendre moins précise 1 valeur".to_owned()
            } else {
                format!("Rendre moins précises {} valeurs", items.len())
            };
            if kinds.is_empty() {
                what
            } else {
                format!("{what} ({})", kinds.join(", "))
            }
        }
        Fix::SuppressRows { rows, .. } => format!(
            "Masquer les indices de {}",
            count_label(rows.len(), "ligne", "lignes")
        ),
        Fix::MaskRegion { .. } => "Masquer le visage".to_owned(),
    }
}

/// Un sujet en échec, tel que l'écran le montre.
#[derive(Clone, PartialEq)]
pub struct SubjectView {
    pub subject: SubjectRef,
    pub label: String,
    /// Critères en échec, dans l'ordre.
    pub criteria: Vec<Criterion>,
    /// Une phrase par échec.
    pub reasons: Vec<String>,
    /// « 3 indices visibles : fonction, société, âge. »
    pub clues: Option<String>,
    /// « Groupe estimé : 1 personne. » (si aucune raison ne le dit déjà).
    pub group: Option<String>,
    /// Corrections, la conseillée d'abord, avec leur libellé.
    pub fixes: Vec<(Fix, String)>,
    /// Groupe d'un tableau sans correction propre : celle d'un autre groupe
    /// du même tableau (rendre une colonne moins précise) le corrige aussi.
    pub fixed_with_table: bool,
}

impl fmt::Debug for SubjectView {
    /// Ni le nom du sujet ni les libellés (ils peuvent citer une valeur).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubjectView")
            .field("subject", &self.subject)
            .field("criteria", &self.criteria)
            .field("fixes", &self.fixes.len())
            .finish_non_exhaustive()
    }
}

/// Un sujet par échec, dans l'ordre des échecs.
pub fn subjects(st: &AnonymizeState, assessed: &Assessed) -> Vec<SubjectView> {
    let a = &assessed.assessment;
    let k_min = a.settings.k_min;
    let mut out: Vec<SubjectView> = Vec::new();
    for f in &a.failures {
        let i = match out.iter().position(|s| s.subject == f.subject) {
            Some(i) => i,
            None => {
                out.push(SubjectView {
                    subject: f.subject.clone(),
                    label: subject_label(st, assessed, &f.subject),
                    criteria: Vec::new(),
                    reasons: Vec::new(),
                    clues: None,
                    group: None,
                    fixes: Vec::new(),
                    fixed_with_table: false,
                });
                out.len().saturating_sub(1)
            }
        };
        let Some(view) = out.get_mut(i) else {
            continue;
        };
        if !view.criteria.contains(&f.criterion) {
            view.criteria.push(f.criterion);
        }
        let reason = reason_line(assessed, &f.subject, &f.reason, k_min);
        if !view.reasons.contains(&reason) {
            view.reasons.push(reason);
        }
        for fix in &f.fixes {
            if !view.fixes.iter().any(|(x, _)| x == fix) {
                view.fixes.push((fix.clone(), fix_label(st, fix)));
            }
        }
    }
    for view in &mut out {
        view.criteria.sort();
        let Some(report) = a.subjects.iter().find(|s| s.subject == view.subject) else {
            continue;
        };
        // Types des indices, sans « autre » (colonne sans type connu).
        let mut kinds: Vec<EntityKind> = Vec::new();
        for (k, _) in &report.clues {
            if !kinds.contains(k) && *k != EntityKind::Autre {
                kinds.push(*k);
            }
        }
        let count = count_label(report.clues.len(), "indice visible", "indices visibles");
        if !kinds.is_empty() {
            let names: Vec<String> = kinds.iter().map(|k| kind_word(*k)).collect();
            view.clues = Some(format!("{count} : {}.", names.join(", ")));
        } else if !report.clues.is_empty() {
            view.clues = Some(format!("{count}."));
        }
        let said = view.reasons.iter().any(|r| r.starts_with("Groupe"));
        if let (Some(n), false) = (report.group_estimate, said) {
            let n = usize::try_from(n).unwrap_or(usize::MAX);
            view.group = Some(format!(
                "Groupe estimé : {}.",
                count_label(n, "personne", "personnes")
            ));
        }
    }
    // Un tableau reçoit une seule correction de colonne, portée par l'un de
    // ses groupes : elle corrige aussi les autres.
    let tables_with_fix: Vec<usize> = out
        .iter()
        .filter(|v| !v.fixes.is_empty())
        .filter_map(|v| match v.subject {
            SubjectRef::TableGroup { table, .. } => Some(table),
            _ => None,
        })
        .collect();
    for view in &mut out {
        if let SubjectRef::TableGroup { table, .. } = view.subject {
            view.fixed_with_table = view.fixes.is_empty() && tables_with_fix.contains(&table);
        }
    }
    out
}

/// « Ce que l'IA pourrait deviner » : pour chaque personne isolable ou liée à
/// une donnée sensible, les indices restés lisibles, en une phrase.
pub fn guesses(st: &AnonymizeState, assessed: &Assessed) -> Vec<String> {
    let a = &assessed.assessment;
    let mut out = Vec::new();
    for s in &a.subjects {
        let failed = |c: Criterion| !s.passed.get(c.index()).copied().unwrap_or(true);
        if !(failed(Criterion::Isolation) || failed(Criterion::Inference)) {
            continue;
        }
        let mut items: Vec<String> = Vec::new();
        for (kind, id) in &s.clues {
            if let Some(v) = readable(st, *id) {
                let item = if *kind == EntityKind::Autre {
                    format!("« {v} »")
                } else {
                    format!("{} « {v} »", kind_word(*kind))
                };
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        let label = subject_label(st, assessed, &s.subject);
        out.push(match items.as_slice() {
            [] => format!("{label} : aucun indice lisible, mais la personne reste isolable."),
            [one] => format!("{label} : {one} reste lisible."),
            [rest @ .., last] => {
                format!("{label} : {} et {last} restent lisibles.", rest.join(", "))
            }
        });
    }
    out
}

/// Libellé d'une réponse aux questions de contexte.
pub fn answer_label(v: Option<bool>) -> &'static str {
    match v {
        None => "Sans réponse",
        Some(true) => "Oui",
        Some(false) => "Non",
    }
}

/// Les trois questions de l'approche contextuelle (plan de la V2, § 4.2).
pub const QUESTIONS: [&str; 3] = [
    "Le destinataire connaît-il les personnes ?",
    "Le document sera-t-il publié ?",
    "D'autres documents du même dossier lui seront-ils envoyés ?",
];

/// Réponse à la question `i` de [`QUESTIONS`].
pub fn answer_mut(answers: &mut ContextAnswers, i: usize) -> Option<&mut Option<bool>> {
    match i {
        0 => Some(&mut answers.recipient_knows_people),
        1 => Some(&mut answers.published),
        2 => Some(&mut answers.other_documents),
        _ => None,
    }
}

/// L'approche suivie, dite pour l'écran (la V2.0 reste prudente : décision 11).
pub fn approach_line(answers: &ContextAnswers) -> &'static str {
    match answers.approach() {
        anon_core::Approach::Simplified => {
            "Approche simplifiée : toute voie de réidentification compte, quel que soit le \
destinataire."
        }
        anon_core::Approach::Contextual => {
            "Vos réponses sont reportées dans la fiche d'évaluation avec leur justification. \
L'évaluation reste prudente : toute voie de réidentification compte."
        }
    }
}

/// Vrai si `text` contient le mot « anonyme » ou « anonymes » (mot entier,
/// sans tenir compte des majuscules ; « _anonyme » d'un nom de fichier
/// compte) : réservé à une évaluation qui conclut à l'anonymat (invariant 9).
/// « anonymiser », « Anonymiseur » ou « anonymat » ne comptent pas.
pub fn says_anonymous(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "anonyme" || w == "anonymes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::anonymize::tests::{assess_now, engine};
    use crate::ui::state::files::FileEntry;
    use anon_core::{Approach, Failure, Session, SubjectReport};
    use anon_engine::{InputFile, NoProgress};
    use std::path::PathBuf;

    /// Évaluation construite à la main.
    fn assessment(verdict: Verdict, failures: Vec<Failure>) -> Assessment {
        Assessment {
            settings: AssessSettings::default(),
            approach: Approach::Simplified,
            subjects: Vec::new(),
            failures,
            verdict,
        }
    }

    fn failure(reason: FailureReason) -> Failure {
        Failure {
            criterion: Criterion::Isolation,
            subject: SubjectRef::Person { value_id: 1 },
            reason,
            fixes: Vec::new(),
        }
    }

    #[test]
    fn verdict_lines_follow_the_vocabulary() {
        let one = assessment(Verdict::Reidentifiable { subjects: 1 }, Vec::new());
        assert_eq!(
            verdict_line(&one),
            "1 personne reste reconnaissable. Corrigez-la avant l'envoi."
        );
        let three = assessment(Verdict::Reidentifiable { subjects: 3 }, Vec::new());
        assert_eq!(
            verdict_line(&three),
            "3 personnes restent reconnaissables. Corrigez-les avant l'envoi."
        );
        let ok = assessment(Verdict::Anonymous, Vec::new());
        assert_eq!(
            verdict_line(&ok),
            "Aucune personne reconnaissable pour ce destinataire."
        );
        let lite = assessment(
            Verdict::Inconclusive,
            vec![failure(FailureReason::FacesNotChecked)],
        );
        assert_eq!(
            verdict_line(&lite),
            "Évaluation non concluante : visages non vérifiés (édition Lite)."
        );
        assert_eq!(
            short_line(Some(&one)),
            "Bilan : 1 personne reste reconnaissable"
        );
        assert_eq!(short_line(None), "Bilan : évaluation en cours…");
        // « Masquer pour l'IA » : le verdict n'empêche pas d'envoyer.
        let note = verdict_note(Goal::Mask, &one);
        assert!(note.contains("Vous pouvez envoyer") && note.contains("clé de restauration"));
        assert!(verdict_note(Goal::Anonymize, &one).contains("_masque_sans_cle"));
    }

    #[test]
    fn the_word_anonyme_needs_an_anonymous_verdict() {
        assert!(says_anonymous("Anonyme pour un assistant"));
        assert!(says_anonymous("patients_anonyme.xlsx"));
        assert!(says_anonymous("Données ANONYMES."));
        for ok in [
            "Anonymiser définitivement",
            "anonymiser",
            "Anonymiseur",
            "anonymat",
            "anonymisé",
            "_masque_sans_cle",
        ] {
            assert!(!says_anonymous(ok), "{ok}");
        }
        // Aucune phrase d'un verdict non concluant ou négatif ne le dit.
        let not_anonymous = [
            assessment(Verdict::Reidentifiable { subjects: 1 }, Vec::new()),
            assessment(Verdict::Reidentifiable { subjects: 4 }, Vec::new()),
            assessment(
                Verdict::Inconclusive,
                vec![failure(FailureReason::FacesNotChecked)],
            ),
            assessment(Verdict::Inconclusive, Vec::new()),
        ];
        for a in &not_anonymous {
            for goal in Goal::ALL {
                for text in [
                    verdict_line(a),
                    verdict_note(*goal, a).to_owned(),
                    short_line(Some(a)),
                ] {
                    assert!(!says_anonymous(&text), "{text}");
                }
            }
            for c in Criterion::ALL {
                assert!(!says_anonymous(&criterion_line(a, c)));
            }
        }
        // Avec un verdict d'anonymat, l'objectif « Anonymiser définitivement »
        // annonce le suffixe `_anonyme`.
        let ok = assessment(Verdict::Anonymous, Vec::new());
        assert!(says_anonymous(verdict_note(Goal::Anonymize, &ok)));
    }

    #[test]
    fn criteria_in_one_line_each() {
        let report = |passed: [bool; 3]| SubjectReport {
            subject: SubjectRef::Person { value_id: 1 },
            clues: Vec::new(),
            group_estimate: None,
            passed,
        };
        let mut a = assessment(Verdict::Reidentifiable { subjects: 1 }, Vec::new());
        a.subjects = vec![report([true; 3]), report([false, true, false])];
        assert_eq!(
            criterion_line(&a, Criterion::Isolation),
            "1 personne protégée · 1 isolable"
        );
        assert_eq!(
            criterion_line(&a, Criterion::Linkage),
            "Aucun identifiant stable"
        );
        assert_eq!(
            criterion_line(&a, Criterion::Inference),
            "1 personne isolable liée à une donnée sensible"
        );
        a.subjects = vec![report([true; 3]); 22];
        assert_eq!(
            criterion_line(&a, Criterion::Isolation),
            "22 personnes protégées · aucune isolable"
        );
        a.subjects.clear();
        assert_eq!(
            criterion_line(&a, Criterion::Isolation),
            "Aucune personne à évaluer"
        );
    }

    #[test]
    fn kinds_in_lower_case_keep_their_acronyms() {
        assert_eq!(kind_word(EntityKind::Age), "âge");
        assert_eq!(kind_word(EntityKind::Fonction), "fonction");
        assert_eq!(kind_word(EntityKind::Iban), "IBAN");
        assert_eq!(kind_word(EntityKind::Ridet), "RIDET");
        assert_eq!(kind_word(EntityKind::Ip), "adresse IP");
    }

    /// Dossier fictif : une personne isolable par sa fonction, sa commune et
    /// son âge, laissés visibles.
    fn folder() -> (AnonymizeState, Session) {
        let text = "Le compte rendu cite Hélène Kaloï, infirmière à Farino, 62 ans, en arrêt \
                    maladie depuis mars. Contact : helene.kaloi@example.com.";
        let mut s = AnonymizeState::default();
        s.add_files(
            [FileEntry::new(PathBuf::from("C:/d/note.txt"), Some(1))],
            &["txt"],
        );
        let _ = s.begin_analysis();
        let input = InputFile::new("note.txt", text.as_bytes().to_vec());
        let session = Session::new();
        let a = engine().analyze(
            std::slice::from_ref(&input),
            &s.analyze_options(),
            &session,
            &NoProgress,
        );
        assert!(s.analysis_done(vec![input], Vec::new(), a, &session));
        for id in s.visible() {
            if s.candidate(id)
                .is_some_and(|c| c.kind.is_quasi_identifier())
            {
                s.set_selected(id, false, &session);
            }
        }
        (s, session)
    }

    #[test]
    fn a_subject_per_failure_with_clues_and_fixes() {
        let (mut s, _) = folder();
        let settings = AssessSettings::default();
        assess_now(&mut s, settings);
        let assessed = s.assessment(&settings).cloned().unwrap();
        assert!(!assessed.assessment.is_anonymous());
        let views = subjects(&s, &assessed);
        assert!(!views.is_empty());
        let person = views
            .iter()
            .find(|v| matches!(v.subject, SubjectRef::Person { .. }))
            .unwrap();
        // La personne est désignée par sa balise, jamais par son nom.
        assert!(person.label.starts_with("[PERSONNE_"), "{}", person.label);
        assert!(!person.criteria.is_empty() && !person.reasons.is_empty());
        let clues = person.clues.as_deref().unwrap_or_default();
        assert!(clues.contains("indices visibles"), "{clues}");
        assert!(
            !clues.contains("Farino"),
            "types et nombre, pas les valeurs"
        );
        assert!(!person.fixes.is_empty(), "correction conseillée");
        assert!(!format!("{person:?}").contains("Kaloï"));
        // Ce que l'IA pourrait deviner : les indices restés lisibles, à l'écran.
        let guesses = guesses(&s, &assessed);
        assert!(
            guesses
                .iter()
                .any(|g| g.contains("Farino") && g.contains("restent lisibles")),
            "{guesses:?}"
        );
    }
}
