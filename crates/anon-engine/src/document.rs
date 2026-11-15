//! Vue d'un document pour la vérification dans le texte : les blocs du fichier
//! (le contenu dans l'ordre du fichier, puis commentaires et notes, puis
//! propriétés), et dans chaque bloc les plages des valeurs trouvées, telles que
//! le `Replacer` les trouverait (mêmes frontières de mots, même casse, mêmes
//! variantes).
//!
//! Deux passes par bloc :
//! 1. les valeurs cochées, avec la session que l'anonymisation utiliserait
//!    (`apply_to` sur une copie) : ce sont exactement les occurrences remplacées ;
//! 2. toutes les valeurs, cochées ou non : les valeurs gardées visibles qui ne
//!    chevauchent aucune occurrence remplacée, pour pouvoir les masquer d'un clic.
//!
//! Sur un long document, une décision ne recalcule que les blocs où la valeur
//! peut apparaître ([`DocumentView::blocks_mentioning`], puis
//! [`Analysis::refresh_document`]).

use crate::analysis::Analysis;
use crate::mentions::{MaskedFinder, ValueFinder};
use anon_core::{BlockRole, EntityKind, Session};
use anon_replace::session_ext::match_key;
use anon_replace::{fold, has_variable_separators};
use std::collections::HashSet;
use std::fmt;

/// Occurrence d'une valeur dans un bloc. Offsets en octets, sur des frontières
/// de caractères.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// Identifiant du candidat (`Candidate::id`).
    pub candidate: usize,
    /// Vrai si l'occurrence sera remplacée avec la sélection courante ; faux
    /// pour une valeur gardée visible.
    pub masked: bool,
}

/// Bloc de texte logique du document (paragraphe, cellule, propriété…).
#[derive(Clone, PartialEq, Eq)]
pub struct DocBlock {
    pub text: String,
    pub role: BlockRole,
    /// Emplacement (`word/document.xml`, `page 3`), jamais de contenu.
    pub location: String,
    /// Plages triées par début, sans chevauchement.
    pub spans: Vec<Span>,
}

impl fmt::Debug for DocBlock {
    /// Ni le texte ni l'en-tête de colonne : l'emplacement et les plages seulement.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocBlock")
            .field("len", &self.text.len())
            .field("role", &self.role)
            .field("location", &self.location)
            .field("spans", &self.spans)
            .finish()
    }
}

/// Texte d'un document et occurrences de ses valeurs.
#[derive(Clone, PartialEq, Eq)]
pub struct DocumentView {
    /// Nom du fichier (celui donné à l'analyse).
    pub name: String,
    pub blocks: Vec<DocBlock>,
    /// Texte plié de chaque bloc, comme le `Replacer` le cherche.
    folded: Vec<String>,
}

impl fmt::Debug for DocumentView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentView")
            .field("blocks", &self.blocks.len())
            .field("spans", &self.span_count())
            .finish_non_exhaustive()
    }
}

impl DocumentView {
    /// Nombre total d'occurrences surlignées.
    pub fn span_count(&self) -> usize {
        self.blocks.iter().map(|b| b.spans.len()).sum()
    }

    /// Occurrences du candidat `id`, dans l'ordre de lecture : (bloc, plage).
    pub fn occurrences(&self, id: usize) -> impl Iterator<Item = (usize, Span)> + '_ {
        self.blocks.iter().enumerate().flat_map(move |(bi, b)| {
            b.spans
                .iter()
                .filter(move |s| s.candidate == id)
                .map(move |s| (bi, *s))
        })
    }

    /// Blocs où l'une des formes `forms` d'une valeur de type `kind` peut
    /// apparaître : ceux dont le texte plié contient sa clé. Les frontières de
    /// mots et la casse ne sont pas vérifiées : c'est un sur-ensemble des blocs
    /// où le `Replacer` la trouverait, donc de ceux qu'une décision sur cette
    /// valeur peut changer.
    pub fn blocks_mentioning(&self, kind: EntityKind, forms: &[&str]) -> Vec<usize> {
        let compact = has_variable_separators(kind);
        let needles: Vec<String> = forms
            .iter()
            .map(|f| match_key(kind, f))
            .filter(|k| !k.is_empty())
            .collect();
        if needles.is_empty() {
            return Vec::new();
        }
        self.folded
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                if compact {
                    // Séparateurs ignorés par ces types (voir `compact_key`).
                    let c: String = f
                        .chars()
                        .filter(|c| !matches!(c, ' ' | '.' | '-'))
                        .collect();
                    needles.iter().any(|n| c.contains(n.as_str()))
                } else {
                    needles.iter().any(|n| f.contains(n.as_str()))
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Candidats présents dans le document, dans l'ordre de leur première
    /// occurrence.
    pub fn candidates_in_order(&self) -> Vec<usize> {
        let mut seen = Vec::new();
        for b in &self.blocks {
            for s in &b.spans {
                if !seen.contains(&s.candidate) {
                    seen.push(s.candidate);
                }
            }
        }
        seen
    }
}

/// Rang d'affichage d'un bloc : contenu, commentaires et notes, propriétés.
fn reading_rank(role: BlockRole) -> u8 {
    match role {
        BlockRole::Comment | BlockRole::Note => 1,
        BlockRole::Metadata => 2,
        _ => 0,
    }
}

/// Les deux recherches d'une vue (module `mentions`) : valeurs remplacées,
/// toutes les valeurs.
struct Passes {
    masked: MaskedFinder,
    everything: ValueFinder,
    /// Valeurs cochées (identifiants de candidats).
    selected: HashSet<usize>,
}

impl Passes {
    /// Plages d'un texte, triées, sans chevauchement.
    fn spans(&self, text: &str) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .masked
            .matches(text)
            .into_iter()
            .filter_map(|m| {
                self.masked.owner(m.tag).map(|id| Span {
                    start: m.start,
                    end: m.end,
                    candidate: id,
                    masked: true,
                })
            })
            .collect();
        let kept: Vec<Span> = self
            .everything
            .find(text)
            .into_iter()
            .filter(|f| !self.selected.contains(&f.id))
            .map(|f| Span {
                start: f.start,
                end: f.end,
                candidate: f.id,
                masked: false,
            })
            .filter(|k| !spans.iter().any(|s| s.start < k.end && k.start < s.end))
            .collect();
        spans.extend(kept);
        spans.sort_by_key(|s| s.start);
        spans
    }
}

impl Analysis {
    /// Vue du fichier `file` (indice dans [`Analysis::files`]) : ses blocs et les
    /// occurrences des valeurs trouvées, remplacées ou gardées selon la
    /// sélection courante appliquée à `session`. La session n'est pas modifiée.
    /// `None` si l'indice n'existe pas.
    pub fn document(&self, file: usize, session: &Session) -> Option<DocumentView> {
        let data = self.store.get(file)?;
        let passes = self.passes(session);
        // Le contenu d'abord, puis commentaires et notes, puis propriétés du
        // fichier ; l'ordre du fichier est gardé à l'intérieur de chaque groupe.
        let mut ordered: Vec<&anon_core::TextBlock> = data.blocks.iter().collect();
        ordered.sort_by_key(|b| reading_rank(b.ctx.role));
        let blocks = ordered
            .iter()
            .map(|b| DocBlock {
                text: b.text.clone(),
                role: b.ctx.role,
                location: b.ctx.part.clone(),
                spans: passes.spans(&b.text),
            })
            .collect();
        Some(DocumentView {
            name: data.name.clone(),
            blocks,
            folded: ordered.iter().map(|b| fold(&b.text).text).collect(),
        })
    }

    /// Recalcule les plages des blocs `blocks` de `view` avec la sélection
    /// courante : après une décision, seuls les blocs où la valeur peut
    /// apparaître ([`DocumentView::blocks_mentioning`]) changent.
    pub fn refresh_document(&self, view: &mut DocumentView, session: &Session, blocks: &[usize]) {
        if blocks.is_empty() {
            return;
        }
        let passes = self.passes(session);
        for &bi in blocks {
            if let Some(b) = view.blocks.get_mut(bi) {
                b.spans = passes.spans(&b.text);
            }
        }
    }

    fn passes(&self, session: &Session) -> Passes {
        Passes {
            // Passe 1 : ce que l'anonymisation remplacerait.
            masked: MaskedFinder::new(self, session),
            // Passe 2 : toutes les valeurs, pour situer les valeurs gardées.
            everything: ValueFinder::new(self, |_| true),
            selected: self
                .candidates
                .iter()
                .filter(|c| c.selected)
                .map(|c| c.id)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalyzeOptions, Engine, InputFile, NoProgress};
    use anon_core::EntityKind;
    use anon_replace::{Replacer, SessionExt};

    /// Analyse de `text` comme un fichier texte, sans détection automatique
    /// gênante : seules les valeurs ajoutées à la main comptent.
    fn manual(text: &str, values: &[(EntityKind, &str)]) -> Analysis {
        let engine = Engine::standard();
        let input = InputFile::new("doc.txt", text.as_bytes().to_vec());
        let mut a = engine.analyze(
            std::slice::from_ref(&input),
            &AnalyzeOptions::default(),
            &Session::new(),
            &NoProgress,
        );
        a.candidates.clear();
        for (k, v) in values {
            assert!(a.add_manual(*k, v).is_some());
        }
        a
    }

    fn id_of(a: &Analysis, value: &str) -> usize {
        a.candidates
            .iter()
            .find(|c| c.value == value)
            .map(|c| c.id)
            .unwrap_or(usize::MAX)
    }

    fn surfaces(view: &DocumentView) -> Vec<(String, usize, bool)> {
        view.blocks
            .iter()
            .flat_map(|b| {
                b.spans.iter().map(|s| {
                    (
                        b.text.get(s.start..s.end).unwrap_or("").to_owned(),
                        s.candidate,
                        s.masked,
                    )
                })
            })
            .collect()
    }

    #[test]
    fn word_boundaries_like_the_replacer() {
        let a = manual(
            "Martin habite en Martinique, chez Martine et M. Martin.",
            &[(EntityKind::Nom, "Martin")],
        );
        let view = a.document(0, &Session::new()).unwrap();
        let s = surfaces(&view);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s.iter().all(|(t, _, masked)| t == "Martin" && *masked));
    }

    #[test]
    fn case_and_accents_follow_the_replacer() {
        let a = manual(
            "NAUTILUS PACIFIQUE SARL et nautilus pacifique sarl, puis Hélène et HELENE.",
            &[
                (EntityKind::Societe, "Nautilus Pacifique SARL"),
                (EntityKind::Prenom, "Hélène"),
            ],
        );
        let view = a.document(0, &Session::new()).unwrap();
        let s: Vec<String> = surfaces(&view).into_iter().map(|(t, _, _)| t).collect();
        assert_eq!(
            s,
            [
                "NAUTILUS PACIFIQUE SARL",
                "nautilus pacifique sarl",
                "Hélène",
                "HELENE"
            ]
        );
        // Une valeur courte respecte les majuscules : « Léo » mais pas « léo ».
        let a = manual("Léo dit léo.", &[(EntityKind::Prenom, "Léo")]);
        let view = a.document(0, &Session::new()).unwrap();
        let s: Vec<String> = surfaces(&view).into_iter().map(|(t, _, _)| t).collect();
        assert_eq!(s, ["Léo"]);
    }

    #[test]
    fn variants_point_to_the_same_value() {
        let engine = Engine::standard();
        let text = "Jean Dupont signe. DUPONT Jean relit. Jean Dupont paie. J. Dupont part.";
        let input = InputFile::new("doc.txt", text.as_bytes().to_vec());
        let mut a = engine.analyze(
            std::slice::from_ref(&input),
            &AnalyzeOptions::default(),
            &Session::new(),
            &NoProgress,
        );
        a.candidates.clear();
        let id = a.add_manual(EntityKind::Personne, "Jean Dupont").unwrap();
        if let Some(c) = a.candidates.iter_mut().find(|c| c.id == id) {
            c.variants = vec!["DUPONT Jean".into(), "J. Dupont".into()];
        }
        let view = a.document(0, &Session::new()).unwrap();
        let s = surfaces(&view);
        assert_eq!(s.len(), 4, "{s:?}");
        assert!(s.iter().all(|(_, c, _)| *c == id));
        assert_eq!(view.occurrences(id).count(), 4);
        assert_eq!(view.candidates_in_order(), [id]);
    }

    #[test]
    fn kept_values_are_located_but_not_masked() {
        let mut a = manual(
            "Pierre Garnier écrit à Nautilus Pacifique SARL.",
            &[
                (EntityKind::Personne, "Pierre Garnier"),
                (EntityKind::Societe, "Nautilus Pacifique SARL"),
            ],
        );
        let company = id_of(&a, "Nautilus Pacifique SARL");
        assert!(a.set_selected(company, false));
        let view = a.document(0, &Session::new()).unwrap();
        let s = surfaces(&view);
        assert_eq!(
            s,
            [
                (
                    "Pierre Garnier".to_owned(),
                    id_of(&a, "Pierre Garnier"),
                    true
                ),
                ("Nautilus Pacifique SARL".to_owned(), company, false),
            ]
        );
    }

    #[test]
    fn masked_spans_are_exactly_what_the_replacer_replaces() {
        // Une valeur gardée plus longue (« Jean Dupont ») ne cache pas la valeur
        // masquée qu'elle contient (« Dupont ») : l'anonymisation la remplacerait.
        let mut a = manual(
            "Jean Dupont et M. Dupont, Dupontel.",
            &[
                (EntityKind::Personne, "Jean Dupont"),
                (EntityKind::Nom, "Dupont"),
            ],
        );
        let full = id_of(&a, "Jean Dupont");
        assert!(a.set_selected(full, false));
        let session = Session::new();
        let view = a.document(0, &session).unwrap();
        let mut applied = session.clone();
        Engine::standard().apply_selection(&a, &mut applied);
        let r = Replacer::new(&applied);
        for b in &view.blocks {
            let expected: Vec<(usize, usize)> = r
                .matches(&b.text)
                .iter()
                .map(|m| (m.start, m.end))
                .collect();
            let got: Vec<(usize, usize)> = b
                .spans
                .iter()
                .filter(|s| s.masked)
                .map(|s| (s.start, s.end))
                .collect();
            assert_eq!(got, expected);
            // Triées, sans chevauchement, sur des frontières de caractères.
            for w in b.spans.windows(2) {
                assert!(w[0].end <= w[1].start);
            }
            for s in &b.spans {
                assert!(b.text.is_char_boundary(s.start) && b.text.is_char_boundary(s.end));
            }
        }
        let s = surfaces(&view);
        assert!(s.iter().any(|(t, _, m)| t == "Dupont" && *m));
        assert!(!s.iter().any(|(t, _, _)| t == "Dupontel"));
    }

    #[test]
    fn blocks_keep_their_order_and_location() {
        let a = manual(
            "Première ligne avec Hélène.\nDeuxième ligne.\nTroisième, Hélène encore.",
            &[(EntityKind::Prenom, "Hélène")],
        );
        let view = a.document(0, &Session::new()).unwrap();
        assert_eq!(view.name, "doc.txt");
        assert_eq!(view.blocks.len(), 3);
        assert!(view.blocks[0].text.starts_with("Première"));
        assert_eq!(view.blocks[1].spans.len(), 0);
        assert_eq!(view.span_count(), 2);
        assert!(view.blocks.iter().all(|b| !b.location.is_empty()));
        assert!(a.document(7, &Session::new()).is_none());
        // Aucun contenu dans les traces.
        let debug = format!("{view:?} {:?}", view.blocks[0]);
        assert!(!debug.contains("Hélène") && !debug.contains("Première"));
    }

    #[test]
    fn values_from_the_session_get_their_existing_tag() {
        // Valeur déjà dans la clé : elle reste masquée et pointe vers sa ligne.
        let engine = Engine::standard();
        let mut session = Session::new();
        session.intern(
            EntityKind::Societe,
            "Atelier Bougainville SAS",
            anon_core::Source::Manual,
        );
        let input = InputFile::new(
            "doc.txt",
            "Facture d'Atelier Bougainville SAS.".as_bytes().to_vec(),
        );
        let a = engine.analyze(
            std::slice::from_ref(&input),
            &AnalyzeOptions::default(),
            &session,
            &NoProgress,
        );
        let view = a.document(0, &session).unwrap();
        let s = surfaces(&view);
        let id = id_of(&a, "Atelier Bougainville SAS");
        assert!(
            s.contains(&("Atelier Bougainville SAS".to_owned(), id, true)),
            "{s:?}"
        );
    }

    #[test]
    fn properties_come_after_the_content() {
        assert!(reading_rank(BlockRole::Body) < reading_rank(BlockRole::Comment));
        assert!(reading_rank(BlockRole::Comment) < reading_rank(BlockRole::Metadata));
        assert_eq!(
            reading_rank(BlockRole::Header),
            reading_rank(BlockRole::Cell)
        );
    }

    #[test]
    fn partial_refresh_matches_a_full_view() {
        let mut a = manual(
            "Jean Dupont signe.\nRien ici.\nM. Dupont, Dupontel.\nTél. 06 12 34 56 78.",
            &[
                (EntityKind::Personne, "Jean Dupont"),
                (EntityKind::Nom, "Dupont"),
                (EntityKind::Tel, "06.12.34.56.78"),
            ],
        );
        let session = Session::new();
        let mut view = a.document(0, &session).unwrap();
        assert_eq!(view.span_count(), 3);
        for (value, kind) in [
            ("Jean Dupont", EntityKind::Personne),
            ("Dupont", EntityKind::Nom),
            ("06.12.34.56.78", EntityKind::Tel),
            ("Jean Dupont", EntityKind::Personne),
        ] {
            let id = id_of(&a, value);
            let selected = a.candidate(id).is_some_and(|c| c.selected);
            assert!(a.set_selected(id, !selected));
            let blocks = view.blocks_mentioning(kind, &[value]);
            a.refresh_document(&mut view, &session, &blocks);
            assert_eq!(view, a.document(0, &session).unwrap(), "{value}");
        }
        // Le bloc sans valeur n'est jamais recalculé ; le téléphone est trouvé
        // malgré d'autres séparateurs.
        assert_eq!(view.blocks_mentioning(EntityKind::Nom, &["Dupont"]), [0, 2]);
        assert_eq!(
            view.blocks_mentioning(EntityKind::Tel, &["06 12 34 56 78"]),
            [3]
        );
        assert!(view.blocks_mentioning(EntityKind::Nom, &["  "]).is_empty());
    }
}
