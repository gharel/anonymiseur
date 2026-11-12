//! Lancement des détecteurs et fusion des détections.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use anon_core::{DetectOptions, Detection, Detector, Source, TextBlock};

/// Rang d'une source en cas d'égalité : plus petit = préféré.
fn source_rank(source: Source) -> u8 {
    match source {
        Source::Manual => 0,
        Source::List => 1,
        Source::Rule => 2,
        Source::Column => 3,
        Source::Dictionary => 4,
        Source::Ner => 5,
    }
}

fn is_keyed(d: &Detection) -> bool {
    d.confidence >= 1.0
}

fn len(d: &Detection) -> usize {
    d.end.saturating_sub(d.start)
}

/// `Less` si `a` est préférée à `b` : confiance 1.0 d'abord, puis intervalle le
/// plus long, puis confiance, puis source ; la position et le type départagent
/// pour que le résultat ne dépende pas de l'ordre d'entrée.
fn preference(a: &Detection, b: &Detection) -> Ordering {
    is_keyed(b)
        .cmp(&is_keyed(a))
        .then_with(|| len(b).cmp(&len(a)))
        .then_with(|| b.confidence.total_cmp(&a.confidence))
        .then_with(|| source_rank(a.source).cmp(&source_rank(b.source)))
        .then_with(|| a.start.cmp(&b.start))
        .then_with(|| a.end.cmp(&b.end))
        .then_with(|| a.kind.cmp(&b.kind))
}

/// Résout les chevauchements. Sortie triée par position, sans chevauchement
/// (deux intervalles qui se touchent sont conservés). Les détections vides ou
/// inversées sont ignorées.
pub fn merge(detections: Vec<Detection>) -> Vec<Detection> {
    let mut candidates: Vec<Detection> =
        detections.into_iter().filter(|d| d.start < d.end).collect();
    candidates.sort_by(preference);

    // début -> fin des intervalles retenus (disjoints).
    let mut kept: BTreeMap<usize, usize> = BTreeMap::new();
    let mut out = Vec::with_capacity(candidates.len());
    for d in candidates {
        // Les intervalles retenus sont disjoints : seul celui dont le début est le
        // plus grand parmi ceux < d.end peut déborder sur d.
        let clash = kept
            .range(..d.end)
            .next_back()
            .is_some_and(|(_, &end)| end > d.start);
        if clash {
            continue;
        }
        kept.insert(d.start, d.end);
        out.push(d);
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    out
}

/// Ensemble de détecteurs lancés ensemble sur un bloc.
#[derive(Default)]
pub struct Pipeline {
    detectors: Vec<Box<dyn Detector + Send>>,
}

impl fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipeline")
            .field(
                "detectors",
                &self.detectors.iter().map(|d| d.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Pipeline {
    pub fn new(detectors: Vec<Box<dyn Detector + Send>>) -> Self {
        Self { detectors }
    }

    pub fn push(&mut self, detector: Box<dyn Detector + Send>) {
        self.detectors.push(detector);
    }

    /// Lance tous les détecteurs, retire les types non voulus et les offsets
    /// invalides, puis fusionne.
    pub fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        let text = block.text.as_str();
        let all: Vec<Detection> = self
            .detectors
            .iter()
            .flat_map(|d| d.detect(block, opts))
            .filter(|d| {
                opts.wants(d.kind)
                    && d.start < d.end
                    && d.end <= text.len()
                    && text.is_char_boundary(d.start)
                    && text.is_char_boundary(d.end)
            })
            .collect();
        merge(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleDetector;
    use anon_core::{BlockContext, BlockRole, EntityKind, Profile};
    use proptest::prelude::*;

    fn det(
        kind: EntityKind,
        start: usize,
        end: usize,
        source: Source,
        confidence: f32,
    ) -> Detection {
        Detection {
            kind,
            start,
            end,
            source,
            confidence,
        }
    }

    fn spans(v: &[Detection]) -> Vec<(usize, usize, EntityKind)> {
        v.iter().map(|d| (d.start, d.end, d.kind)).collect()
    }

    #[test]
    fn empty_input() {
        assert!(merge(Vec::new()).is_empty());
    }

    #[test]
    fn drops_empty_and_inverted() {
        let v = vec![
            det(EntityKind::Nom, 3, 3, Source::Rule, 1.0),
            det(EntityKind::Nom, 5, 2, Source::Rule, 1.0),
        ];
        assert!(merge(v).is_empty());
    }

    #[test]
    fn disjoint_and_touching_are_kept_sorted() {
        let v = vec![
            det(EntityKind::Email, 10, 20, Source::Rule, 0.9),
            det(EntityKind::Tel, 0, 5, Source::Rule, 0.9),
            det(EntityKind::Ip, 5, 10, Source::Rule, 0.9),
        ];
        assert_eq!(
            spans(&merge(v)),
            vec![
                (0, 5, EntityKind::Tel),
                (5, 10, EntityKind::Ip),
                (10, 20, EntityKind::Email)
            ]
        );
    }

    #[test]
    fn keyed_confidence_beats_longer_interval() {
        let v = vec![
            det(EntityKind::Tel, 0, 20, Source::Rule, 0.9),
            det(EntityKind::Nir, 5, 10, Source::Rule, 1.0),
        ];
        assert_eq!(spans(&merge(v)), vec![(5, 10, EntityKind::Nir)]);
    }

    #[test]
    fn longer_beats_shorter_without_key() {
        let v = vec![
            det(EntityKind::Prenom, 0, 4, Source::Ner, 0.99),
            det(EntityKind::Personne, 0, 11, Source::Dictionary, 0.5),
        ];
        assert_eq!(spans(&merge(v)), vec![(0, 11, EntityKind::Personne)]);
    }

    #[test]
    fn both_keyed_longer_wins() {
        let v = vec![
            det(EntityKind::Cb, 0, 16, Source::Rule, 1.0),
            det(EntityKind::Iban, 0, 27, Source::Rule, 1.0),
        ];
        assert_eq!(spans(&merge(v)), vec![(0, 27, EntityKind::Iban)]);
    }

    #[test]
    fn equal_length_higher_confidence_wins() {
        let v = vec![
            det(EntityKind::Nom, 0, 5, Source::Rule, 0.6),
            det(EntityKind::Prenom, 0, 5, Source::Ner, 0.8),
        ];
        assert_eq!(spans(&merge(v)), vec![(0, 5, EntityKind::Prenom)]);
    }

    #[test]
    fn equal_everything_but_source_uses_source_order() {
        let order = [
            Source::Manual,
            Source::List,
            Source::Rule,
            Source::Column,
            Source::Dictionary,
            Source::Ner,
        ];
        for (i, best) in order.iter().enumerate() {
            let mut v: Vec<Detection> = order
                .iter()
                .skip(i)
                .map(|s| det(EntityKind::Nom, 0, 4, *s, 0.7))
                .collect();
            v.reverse();
            let out = merge(v);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].source, *best);
        }
    }

    #[test]
    fn chain_of_overlaps_keeps_best_first() {
        // A (0..10) chevauche B (8..20) qui chevauche C (18..26) ; B est le plus long.
        let v = vec![
            det(EntityKind::Nom, 0, 10, Source::Rule, 0.5),
            det(EntityKind::Prenom, 8, 20, Source::Rule, 0.5),
            det(EntityKind::Lieu, 18, 26, Source::Rule, 0.5),
        ];
        assert_eq!(spans(&merge(v)), vec![(8, 20, EntityKind::Prenom)]);
    }

    #[test]
    fn contained_interval_is_dropped() {
        let v = vec![
            det(EntityKind::Url, 0, 30, Source::Rule, 0.95),
            det(EntityKind::Ip, 7, 18, Source::Rule, 0.9),
            det(EntityKind::Email, 40, 50, Source::Rule, 0.95),
        ];
        assert_eq!(
            spans(&merge(v)),
            vec![(0, 30, EntityKind::Url), (40, 50, EntityKind::Email)]
        );
    }

    #[test]
    fn exact_duplicates_collapse() {
        let d = det(EntityKind::Email, 0, 10, Source::Rule, 0.95);
        assert_eq!(merge(vec![d.clone(), d.clone(), d]).len(), 1);
    }

    #[test]
    fn nan_confidence_does_not_panic() {
        let v = vec![
            det(EntityKind::Nom, 0, 5, Source::Rule, f32::NAN),
            det(EntityKind::Prenom, 0, 5, Source::Rule, 0.5),
        ];
        assert_eq!(merge(v).len(), 1);
    }

    struct Fixed(Vec<Detection>);

    impl Detector for Fixed {
        fn name(&self) -> &'static str {
            "fixed"
        }
        fn detect(&self, _: &TextBlock, _: &DetectOptions) -> Vec<Detection> {
            self.0.clone()
        }
    }

    fn block(text: &str) -> TextBlock {
        TextBlock {
            text: text.to_string(),
            ctx: BlockContext::new("test", BlockRole::Body),
        }
    }

    #[test]
    fn pipeline_filters_unwanted_kinds_and_bad_offsets() {
        let text = "é abcdef";
        let pipeline = Pipeline::new(vec![Box::new(Fixed(vec![
            // quasi-identifiant, retiré en profil Standard
            det(EntityKind::Date, 3, 5, Source::Rule, 0.9),
            // coupe le « é »
            det(EntityKind::Nom, 1, 3, Source::Rule, 0.9),
            // hors bornes
            det(EntityKind::Nom, 3, 99, Source::Rule, 0.9),
            // valide
            det(EntityKind::Email, 3, 5, Source::Rule, 0.9),
        ]))]);
        let out = pipeline.detect(&block(text), &DetectOptions::default());
        assert_eq!(spans(&out), vec![(3, 5, EntityKind::Email)]);
        let renforce = DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        };
        // Date (3..5) et Email (3..5) se chevauchent : un seul reste.
        assert_eq!(pipeline.detect(&block(text), &renforce).len(), 1);
    }

    #[test]
    fn pipeline_runs_all_detectors_and_merges() {
        let text = "Écrire à jean@example.com";
        let manual = Fixed(vec![det(EntityKind::Personne, 11, 15, Source::Manual, 1.0)]);
        let mut p = Pipeline::default();
        p.push(Box::new(RuleDetector));
        p.push(Box::new(manual));
        let out = p.detect(&block(text), &DetectOptions::default());
        // 1.0 (manuel) l'emporte sur l'email à 0.95 qui le recouvre.
        assert_eq!(spans(&out), vec![(11, 15, EntityKind::Personne)]);
        assert!(format!("{p:?}").contains("rules"));
    }

    #[test]
    fn pipeline_with_rules_finds_mixed_values() {
        let text = "Mail jean@example.com, tél 06 12 34 56 78.";
        let p = Pipeline::new(vec![Box::new(RuleDetector)]);
        let out = p.detect(&block(text), &DetectOptions::default());
        let vals: Vec<&str> = out.iter().map(|d| &text[d.start..d.end]).collect();
        assert_eq!(vals, vec!["jean@example.com", "06 12 34 56 78"]);
    }

    fn arb_detection() -> impl Strategy<Value = Detection> {
        (0usize..40, 0usize..40, 0usize..6, 0u8..3, 0u8..=10).prop_map(|(a, b, k, s, c)| {
            Detection {
                kind: EntityKind::ALL[k],
                start: a.min(b),
                end: a.max(b),
                source: [Source::Rule, Source::Ner, Source::Manual][usize::from(s)],
                confidence: f32::from(c) / 10.0,
            }
        })
    }

    proptest! {
        #[test]
        fn merge_output_is_sorted_and_disjoint(v in proptest::collection::vec(arb_detection(), 0..30)) {
            let out = merge(v.clone());
            for w in out.windows(2) {
                prop_assert!(w[0].end <= w[1].start, "{:?}", w);
            }
            for d in &out {
                prop_assert!(d.start < d.end);
            }
            // idempotent et indépendant de l'ordre d'entrée
            prop_assert_eq!(merge(out.clone()), out.clone());
            let mut rev = v;
            rev.reverse();
            prop_assert_eq!(merge(rev), out);
        }

        #[test]
        fn pipeline_never_panics_and_offsets_valid(s in "\\PC{0,200}") {
            let p = Pipeline::new(vec![Box::new(RuleDetector)]);
            let opts = DetectOptions { profile: Profile::Renforce, ..Default::default() };
            let out = p.detect(&block(&s), &opts);
            for w in out.windows(2) {
                prop_assert!(w[0].end <= w[1].start);
            }
            for d in &out {
                prop_assert!(d.start < d.end && d.end <= s.len());
                prop_assert!(s.is_char_boundary(d.start) && s.is_char_boundary(d.end));
            }
        }
    }
}
