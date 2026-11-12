//! Résultat d'une analyse : lignes de l'écran de validation (`Candidate`),
//! bilan par fichier, et données gardées en mémoire (blocs extraits, OCR) pour
//! recompter et masquer sans relire les fichiers.

use crate::error::EngineError;
use anon_core::{
    DetectOptions, EntityKind, OcrPage, Session, Source, Tag, TextBlock, Treatment, Warning,
};
use anon_replace::session_ext::match_key;
use anon_replace::{CASE_SENSITIVE_MAX_CHARS, Replacer, SessionExt, key};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

/// Confiance à partir de laquelle une ligne est cochée par défaut.
pub const SELECT_MIN_CONFIDENCE: f32 = 0.6;
/// Nombre de caractères gardés de part et d'autre d'une occurrence dans `Candidate::context`.
pub const CONTEXT_CHARS: usize = 30;

/// Réglages d'une analyse.
#[derive(Clone, Default)]
pub struct AnalyzeOptions {
    pub detect: DetectOptions,
    /// Liste « toujours anonymiser » : terme et type.
    pub always: Vec<(String, EntityKind)>,
    /// Liste « ne jamais anonymiser ».
    pub never: Vec<String>,
}

impl fmt::Debug for AnalyzeOptions {
    /// N'affiche pas les termes des listes (contenu utilisateur).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalyzeOptions")
            .field("detect", &self.detect)
            .field("always", &self.always.len())
            .field("never", &self.never.len())
            .finish()
    }
}

/// Bilan de lecture d'un fichier. Un fichier illisible n'interrompt pas les autres.
#[derive(Debug)]
pub struct FileAnalysis {
    pub name: String,
    /// Nombre de blocs de texte extraits (hors OCR).
    pub blocks: usize,
    /// Noms des images lues par OCR (à utiliser dans `ManualMask::media`).
    pub images: Vec<String>,
    pub warnings: Vec<Warning>,
    pub error: Option<EngineError>,
}

/// Une ligne de l'écran de validation : une valeur, agrégée sur tous les
/// fichiers par type et clé normalisée.
#[derive(Clone, PartialEq)]
pub struct Candidate {
    pub id: usize,
    pub kind: EntityKind,
    /// Forme de surface la plus fréquente.
    pub value: String,
    /// Autres formes de surface remplacées par la même balise.
    pub variants: Vec<String>,
    /// Nombre d'endroits que le `Replacer` remplacerait.
    pub occurrences: usize,
    /// Fichiers où la valeur apparaît.
    pub files: Vec<String>,
    /// Extrait autour de la première occurrence. Affichage seulement, jamais journalisé.
    pub context: String,
    pub source: Source,
    pub confidence: f32,
    pub selected: bool,
    pub case_sensitive: bool,
    /// Valeur courte ou mot courant : risque de faux positif, casse exacte exigée.
    pub ambiguous: bool,
    /// Balise déjà attribuée dans la session.
    pub existing_tag: Option<Tag>,
    pub treatment: Treatment,
    /// Nombre de détections agrégées (sert à départager les formes).
    pub(crate) hits: usize,
    file_ids: BTreeSet<usize>,
}

impl fmt::Debug for Candidate {
    /// N'affiche ni la valeur, ni les variantes, ni le contexte.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Candidate")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("variants", &self.variants.len())
            .field("occurrences", &self.occurrences)
            .field("source", &self.source)
            .field("confidence", &self.confidence)
            .field("selected", &self.selected)
            .field("existing_tag", &self.existing_tag.map(|t| t.to_string()))
            .finish_non_exhaustive()
    }
}

/// Image lue par OCR à l'analyse.
pub(crate) struct ImageData {
    pub name: String,
    /// SHA-256 de l'image d'origine (clé de recherche à l'anonymisation).
    pub sha256: String,
    pub ocr: OcrPage,
}

/// Données d'un fichier gardées en mémoire.
pub(crate) struct FileData {
    pub name: String,
    pub blocks: Vec<TextBlock>,
    pub images: Vec<ImageData>,
}

impl FileData {
    pub fn empty(name: &str) -> Self {
        Self {
            name: name.to_string(),
            blocks: Vec::new(),
            images: Vec::new(),
        }
    }

    /// Tous les textes où chercher des occurrences : blocs puis OCR.
    fn texts(&self) -> impl Iterator<Item = String> + '_ {
        self.blocks
            .iter()
            .map(|b| b.text.clone())
            .chain(self.images.iter().map(|i| i.ocr.text_with_spans().0))
    }
}

/// Résultat d'une analyse.
pub struct Analysis {
    pub files: Vec<FileAnalysis>,
    pub candidates: Vec<Candidate>,
    /// Vrai si l'analyse a été interrompue par l'utilisateur.
    pub cancelled: bool,
    pub(crate) never: Vec<String>,
    pub(crate) store: Vec<FileData>,
    next_id: usize,
}

impl fmt::Debug for Analysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analysis")
            .field("files", &self.files)
            .field("candidates", &self.candidates)
            .field("cancelled", &self.cancelled)
            .finish_non_exhaustive()
    }
}

/// Extrait d'environ [`CONTEXT_CHARS`] caractères de part et d'autre de `start..end`.
pub(crate) fn excerpt(text: &str, start: usize, end: usize) -> String {
    let before = text.get(..start).unwrap_or("");
    let middle = text.get(start..end).unwrap_or("");
    let after = text.get(end..).unwrap_or("");
    let skip = before.chars().count().saturating_sub(CONTEXT_CHARS);
    let mut out = String::new();
    if skip > 0 {
        out.push('…');
    }
    out.extend(before.chars().skip(skip));
    out.push_str(middle);
    out.extend(after.chars().take(CONTEXT_CHARS));
    if after.chars().count() > CONTEXT_CHARS {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Vrai pour une valeur courte ou un mot courant (« Pierre », « Rose »).
pub fn is_ambiguous(value: &str) -> bool {
    let k = key(value);
    let letters = k.chars().filter(|c| !c.is_whitespace()).count();
    letters <= CASE_SENSITIVE_MAX_CHARS || (!k.contains(' ') && anon_dict::is_common_word(value))
}

/// Ordre d'enregistrement : personnes complètes, puis les autres types, puis
/// noms et prénoms isolés, pour que les balises se lient.
fn person_rank(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Personne => 0,
        EntityKind::Nom | EntityKind::Prenom => 2,
        _ => 1,
    }
}

/// Collecte des détections, groupées par type et clé.
pub(crate) struct Aggregator {
    never: Vec<String>,
    index: HashMap<(EntityKind, String), usize>,
    groups: Vec<Group>,
}

struct Group {
    kind: EntityKind,
    forms: Vec<(String, usize)>,
    source: Source,
    confidence: f32,
    files: BTreeSet<usize>,
    context: String,
}

impl Aggregator {
    pub fn new(never: &[String]) -> Self {
        Self {
            never: never
                .iter()
                .map(|n| key(n))
                .filter(|k| !k.is_empty())
                .collect(),
            index: HashMap::new(),
            groups: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        kind: EntityKind,
        text: &str,
        start: usize,
        end: usize,
        source: Source,
        confidence: f32,
        file: usize,
    ) {
        let Some(surface) = text.get(start..end) else {
            return;
        };
        let surface = surface.trim();
        let k = key(surface);
        if k.is_empty() || self.never.contains(&k) {
            return;
        }
        let mk = match_key(kind, surface);
        let gi = match self.index.get(&(kind, mk.clone())) {
            Some(&gi) => gi,
            None => {
                self.groups.push(Group {
                    kind,
                    forms: Vec::new(),
                    source,
                    confidence,
                    files: BTreeSet::new(),
                    context: excerpt(text, start, end),
                });
                let gi = self.groups.len().saturating_sub(1);
                self.index.insert((kind, mk), gi);
                gi
            }
        };
        let Some(g) = self.groups.get_mut(gi) else {
            return;
        };
        match g.forms.iter_mut().find(|(f, _)| f == surface) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => g.forms.push((surface.to_string(), 1)),
        }
        if confidence > g.confidence {
            g.confidence = confidence;
            g.source = source;
        }
        g.files.insert(file);
    }
}

impl Analysis {
    /// Assemble l'analyse : candidats agrégés, valeurs déjà connues de la
    /// session, variantes de personnes, comptage des occurrences.
    pub(crate) fn finish(
        files: Vec<FileAnalysis>,
        store: Vec<FileData>,
        agg: Aggregator,
        never: Vec<String>,
        session: &Session,
        cancelled: bool,
    ) -> Self {
        let mut a = Analysis {
            files,
            candidates: Vec::new(),
            cancelled,
            never,
            store,
            next_id: 1,
        };
        for g in agg.groups {
            let mut best = 0usize;
            for (i, (_, n)) in g.forms.iter().enumerate() {
                if g.forms.get(best).is_some_and(|(_, b)| n > b) {
                    best = i;
                }
            }
            let Some((value, _)) = g.forms.get(best).cloned() else {
                continue;
            };
            let variants = g
                .forms
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != best)
                .map(|(_, (f, _))| f.clone())
                .collect();
            let hits = g.forms.iter().map(|(_, n)| n).sum();
            a.push_candidate(
                g.kind,
                value,
                variants,
                g.source,
                g.confidence,
                g.context,
                g.files,
                hits,
                session,
            );
        }
        a.add_session_entities(session);
        a.merge_person_variants();
        a.recount();
        a
    }

    #[allow(clippy::too_many_arguments)]
    fn push_candidate(
        &mut self,
        kind: EntityKind,
        value: String,
        variants: Vec<String>,
        source: Source,
        confidence: f32,
        context: String,
        file_ids: BTreeSet<usize>,
        hits: usize,
        session: &Session,
    ) -> usize {
        let ambiguous = is_ambiguous(&value);
        let existing = session
            .tag_for(kind, &value)
            .and_then(|t| session.entity(t));
        let (case_sensitive, treatment) = match existing {
            Some(e) => (e.case_sensitive, e.treatment.clone()),
            None => (ambiguous, Treatment::Tag),
        };
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.candidates.push(Candidate {
            id,
            kind,
            value,
            variants,
            occurrences: 0,
            files: Vec::new(),
            context,
            source,
            confidence,
            selected: existing.is_some() || confidence >= SELECT_MIN_CONFIDENCE,
            case_sensitive,
            ambiguous,
            existing_tag: existing.map(|e| e.tag),
            treatment,
            hits,
            file_ids,
        });
        id
    }

    /// Valeurs déjà dans la session, présentes dans les fichiers mais non
    /// détectées cette fois : elles seront remplacées de toute façon, on les montre.
    fn add_session_entities(&mut self, session: &Session) {
        if session.entities.is_empty() {
            return;
        }
        let replacer = Replacer::new(session);
        let mut found: Vec<(Tag, String, usize)> = Vec::new();
        for (fi, fd) in self.store.iter().enumerate() {
            for text in fd.texts() {
                for m in replacer.matches(&text) {
                    if !found.iter().any(|(t, _, _)| *t == m.tag) {
                        found.push((m.tag, excerpt(&text, m.start, m.end), fi));
                    }
                }
            }
        }
        for (tag, context, fi) in found {
            if self.candidates.iter().any(|c| c.existing_tag == Some(tag)) {
                continue;
            }
            let Some(e) = session.entity(tag) else {
                continue;
            };
            let (value, variants, source) = (e.value.clone(), e.variants.clone(), e.source);
            self.push_candidate(
                tag.kind,
                value,
                variants,
                source,
                1.0,
                context,
                BTreeSet::from([fi]),
                1,
                session,
            );
        }
    }

    /// Regroupe les formes d'une même personne (`DUPONT Jean`, `J. Dupont`)
    /// sous la forme complète, quand le rapprochement est sans ambiguïté.
    fn merge_person_variants(&mut self) {
        while let Some((from, into)) = self.find_person_merge() {
            let Some(src) = self.candidates.get(from).cloned() else {
                return;
            };
            if let Some(dst) = self.candidates.get_mut(into) {
                for v in std::iter::once(&src.value).chain(src.variants.iter()) {
                    if dst.value != *v && !dst.variants.contains(v) {
                        dst.variants.push(v.clone());
                    }
                }
                dst.hits = dst.hits.saturating_add(src.hits);
                if src.confidence > dst.confidence {
                    dst.confidence = src.confidence;
                }
                dst.selected |= src.selected;
                dst.file_ids.extend(src.file_ids.iter().copied());
                if dst.existing_tag.is_none() {
                    dst.existing_tag = src.existing_tag;
                }
            }
            self.candidates.remove(from);
        }
    }

    fn find_person_merge(&self) -> Option<(usize, usize)> {
        let persons: Vec<(usize, Vec<String>)> = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == EntityKind::Personne)
            .map(|(i, c)| (i, name_tokens(&c.value)))
            .collect();
        for (i, short) in &persons {
            let Some(ci) = self.candidates.get(*i) else {
                continue;
            };
            let targets: Vec<usize> = persons
                .iter()
                .filter(|(j, full)| {
                    j != i && !has_initial(full) && same_person(short, full) && {
                        // Permutation : la forme la plus fréquente garde la valeur.
                        has_initial(short)
                            || self.candidates.get(*j).is_some_and(|cj| {
                                cj.hits > ci.hits || (cj.hits == ci.hits && j < i)
                            })
                    }
                })
                .map(|(j, _)| *j)
                .collect();
            if let [only] = targets.as_slice() {
                return Some((*i, *only));
            }
        }
        None
    }

    /// Recalcule `occurrences` et `files` avec un `Replacer` construit sur
    /// tous les candidats, dans l'ordre d'enregistrement de `apply_selection`.
    pub(crate) fn recount(&mut self) {
        let mut s = Session::new();
        for n in &self.never {
            s.add_never(n);
        }
        let mut owner: HashMap<Tag, usize> = HashMap::new();
        for i in self.ordered_indices() {
            let Some(c) = self.candidates.get(i) else {
                continue;
            };
            let Some(tag) = s.intern_with_case(c.kind, &c.value, c.source, c.case_sensitive) else {
                continue;
            };
            for v in &c.variants {
                s.add_variant(tag, v);
            }
            owner.entry(tag).or_insert(i);
        }
        let replacer = Replacer::new(&s);
        let mut counts = vec![0usize; self.candidates.len()];
        let mut files: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); self.candidates.len()];
        for (fi, fd) in self.store.iter().enumerate() {
            for text in fd.texts() {
                for m in replacer.matches(&text) {
                    if let Some(&i) = owner.get(&m.tag) {
                        if let Some(n) = counts.get_mut(i) {
                            *n = n.saturating_add(1);
                        }
                        if let Some(f) = files.get_mut(i) {
                            f.insert(fi);
                        }
                    }
                }
            }
        }
        for (i, c) in self.candidates.iter_mut().enumerate() {
            c.occurrences = counts.get(i).copied().unwrap_or(0);
            let mut ids = files.get(i).cloned().unwrap_or_default();
            ids.extend(c.file_ids.iter().copied());
            c.files = ids
                .iter()
                .filter_map(|fi| self.store.get(*fi).map(|f| f.name.clone()))
                .collect();
        }
    }

    /// Indices des candidats dans l'ordre d'enregistrement.
    fn ordered_indices(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.candidates.len()).collect();
        idx.sort_by_key(|&i| {
            self.candidates
                .get(i)
                .map_or(u8::MAX, |c| person_rank(c.kind))
        });
        idx
    }

    fn position(&self, id: usize) -> Option<usize> {
        self.candidates.iter().position(|c| c.id == id)
    }

    pub fn candidate(&self, id: usize) -> Option<&Candidate> {
        self.candidates.iter().find(|c| c.id == id)
    }

    /// Candidats cochés.
    pub fn selected(&self) -> impl Iterator<Item = &Candidate> {
        self.candidates.iter().filter(|c| c.selected)
    }

    /// Ajout manuel par l'utilisateur. Renvoie l'identifiant de la ligne (une
    /// ligne existante de même type et même valeur est cochée et réutilisée),
    /// `None` si la valeur est vide.
    pub fn add_manual(&mut self, kind: EntityKind, value: &str) -> Option<usize> {
        let value = value.trim();
        let mk = match_key(kind, value);
        if mk.is_empty() {
            return None;
        }
        if let Some(c) = self.candidates.iter_mut().find(|c| {
            c.kind == kind
                && std::iter::once(&c.value)
                    .chain(c.variants.iter())
                    .any(|v| match_key(kind, v) == mk)
        }) {
            c.selected = true;
            return Some(c.id);
        }
        // Premier endroit où la valeur apparaît, pour le contexte.
        let mut probe = Session::new();
        let ambiguous = is_ambiguous(value);
        probe.intern_with_case(kind, value, Source::Manual, ambiguous);
        let r = Replacer::new(&probe);
        let mut context = String::new();
        let mut file_ids = BTreeSet::new();
        'outer: for (fi, fd) in self.store.iter().enumerate() {
            for text in fd.texts() {
                if let Some(m) = r.matches(&text).first() {
                    context = excerpt(&text, m.start, m.end);
                    file_ids.insert(fi);
                    break 'outer;
                }
            }
        }
        let empty = Session::new();
        let id = self.push_candidate(
            kind,
            value.to_string(),
            Vec::new(),
            Source::Manual,
            1.0,
            context,
            file_ids,
            1,
            &empty,
        );
        if let Some(c) = self.candidates.last_mut() {
            c.selected = true;
        }
        self.recount();
        Some(id)
    }

    pub fn set_selected(&mut self, id: usize, selected: bool) -> bool {
        match self.candidates.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                c.selected = selected;
                true
            }
            None => false,
        }
    }

    pub fn set_kind(&mut self, id: usize, kind: EntityKind) -> bool {
        let Some(i) = self.position(id) else {
            return false;
        };
        if let Some(c) = self.candidates.get_mut(i) {
            c.kind = kind;
            c.existing_tag = None;
        }
        self.recount();
        true
    }

    pub fn set_treatment(&mut self, id: usize, treatment: Treatment) -> bool {
        match self.candidates.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                c.treatment = treatment;
                true
            }
            None => false,
        }
    }

    pub fn set_case_sensitive(&mut self, id: usize, case_sensitive: bool) -> bool {
        let Some(i) = self.position(id) else {
            return false;
        };
        if let Some(c) = self.candidates.get_mut(i) {
            c.case_sensitive = case_sensitive;
        }
        self.recount();
        true
    }

    pub fn remove(&mut self, id: usize) -> bool {
        let Some(i) = self.position(id) else {
            return false;
        };
        self.candidates.remove(i);
        self.recount();
        true
    }

    /// Enregistre les candidats cochés dans la session (voir `Engine::apply_selection`).
    pub(crate) fn apply_to(&self, session: &mut Session) {
        for n in &self.never {
            session.add_never(n);
        }
        for i in self.ordered_indices() {
            let Some(c) = self.candidates.get(i) else {
                continue;
            };
            if !c.selected {
                if let Some(tag) = c.existing_tag {
                    session.set_enabled(tag, false);
                }
                continue;
            }
            let Some(tag) = session.intern_with_case(c.kind, &c.value, c.source, c.case_sensitive)
            else {
                continue;
            };
            for v in &c.variants {
                session.add_variant(tag, v);
            }
            session.set_treatment(tag, c.treatment.clone());
            session.set_enabled(tag, true);
        }
    }

    /// OCR gardé en mémoire, par SHA-256 de l'image d'origine.
    pub(crate) fn ocr_by_sha(&self) -> HashMap<&str, &OcrPage> {
        self.store
            .iter()
            .flat_map(|f| f.images.iter())
            .map(|i| (i.sha256.as_str(), &i.ocr))
            .collect()
    }

    pub(crate) fn file(&self, name: &str) -> Option<&FileAnalysis> {
        self.files.iter().find(|f| f.name == name)
    }
}

/// Mots d'un nom, sans les points des initiales.
fn name_tokens(value: &str) -> Vec<String> {
    key(value)
        .split(' ')
        .map(|t| t.trim_matches('.').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn has_initial(tokens: &[String]) -> bool {
    tokens.iter().any(|t| t.chars().count() == 1)
}

/// Vrai si `short` désigne la même personne que `full` : mêmes mots dans un
/// autre ordre, ou initiales à la place de certains mots.
fn same_person(short: &[String], full: &[String]) -> bool {
    if short.len() != full.len() || short.len() < 2 || short == full {
        return false;
    }
    let mut used = vec![false; full.len()];
    let mut pending = Vec::new();
    for s in short {
        match full
            .iter()
            .enumerate()
            .find(|(j, f)| !used.get(*j).copied().unwrap_or(true) && *f == s)
        {
            Some((j, _)) => {
                if let Some(u) = used.get_mut(j) {
                    *u = true;
                }
            }
            None => pending.push(s),
        }
    }
    for s in pending {
        let Some(initial) = s.chars().next().filter(|_| s.chars().count() == 1) else {
            return false;
        };
        match full
            .iter()
            .enumerate()
            .find(|(j, f)| !used.get(*j).copied().unwrap_or(true) && f.starts_with(initial))
        {
            Some((j, _)) => {
                if let Some(u) = used.get_mut(j) {
                    *u = true;
                }
            }
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<String> {
        name_tokens(s)
    }

    #[test]
    fn person_forms() {
        assert!(same_person(&toks("DUPONT Jean"), &toks("Jean Dupont")));
        assert!(same_person(&toks("J. Dupont"), &toks("Jean Dupont")));
        assert!(!same_person(&toks("J. Durand"), &toks("Jean Dupont")));
        assert!(!same_person(&toks("Jean Dupont"), &toks("Jean Dupont")));
        assert!(!same_person(&toks("Dupont"), &toks("Jean Dupont")));
    }

    #[test]
    fn excerpt_is_bounded_and_char_safe() {
        let text = format!("{}Élodie{}", "é".repeat(50), "à".repeat(50));
        let start = "é".repeat(50).len();
        let e = excerpt(&text, start, start + "Élodie".len());
        assert!(e.contains("Élodie"));
        assert!(e.chars().count() <= 2 * CONTEXT_CHARS + "Élodie".len() + 2);
        assert_eq!(excerpt("a\n b", 0, 1), "a b");
    }

    #[test]
    fn ambiguity() {
        assert!(is_ambiguous("Pierre"));
        assert!(is_ambiguous("Léo"));
        assert!(!is_ambiguous("Dupont"));
        assert!(!is_ambiguous("Pierre Garnier"));
    }
}
