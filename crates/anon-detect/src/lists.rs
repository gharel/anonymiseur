//! Termes « toujours anonymiser » fournis par l'utilisateur.

use std::collections::HashMap;

use anon_core::{DetectOptions, Detection, Detector, EntityKind, Source, TextBlock};

use crate::fold::fold_char;

/// Les termes de l'utilisateur sont sûrs : confiance maximale.
const CONF_LIST: f32 = 1.0;

/// Détecteur de liste (`name() == "list"`) : mots entiers, insensible à la casse
/// et aux accents.
#[derive(Debug, Default, Clone)]
pub struct ListDetector {
    terms: Vec<(Vec<char>, EntityKind)>,
    by_first: HashMap<char, Vec<usize>>,
}

impl ListDetector {
    pub fn new(terms: Vec<(String, EntityKind)>) -> Self {
        let mut kept: Vec<(Vec<char>, EntityKind)> = Vec::new();
        for (term, kind) in terms {
            let folded: Vec<char> = term.trim().chars().flat_map(fold_char).collect();
            if !folded.is_empty() {
                kept.push((folded, kind));
            }
        }
        let mut by_first: HashMap<char, Vec<usize>> = HashMap::new();
        for (i, (chars, _)) in kept.iter().enumerate() {
            if let Some(first) = chars.first() {
                by_first.entry(*first).or_default().push(i);
            }
        }
        Self {
            terms: kept,
            by_first,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

impl Detector for ListDetector {
    fn name(&self) -> &'static str {
        "list"
    }

    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        if self.terms.is_empty() {
            return Vec::new();
        }
        let text = block.text.as_str();
        // Texte plié, avec pour chaque caractère l'intervalle d'origine (octets).
        let mut folded: Vec<(char, usize, usize)> = Vec::with_capacity(text.len());
        for (i, c) in text.char_indices() {
            let end = i + c.len_utf8();
            folded.extend(fold_char(c).into_iter().map(|f| (f, i, end)));
        }
        let mut out = Vec::new();
        for (i, (c, start, _)) in folded.iter().enumerate() {
            let Some(candidates) = self.by_first.get(c) else {
                continue;
            };
            for &t in candidates {
                let Some((chars, kind)) = self.terms.get(t) else {
                    continue;
                };
                if opts.disabled.contains(kind) {
                    continue;
                }
                let j = i + chars.len();
                let Some(window) = folded.get(i..j) else {
                    continue;
                };
                if !window.iter().map(|(f, _, _)| f).eq(chars.iter()) {
                    continue;
                }
                let first_alnum = chars.first().is_some_and(|f| f.is_alphanumeric());
                let last_alnum = chars.last().is_some_and(|f| f.is_alphanumeric());
                let before_ok = !first_alnum
                    || i == 0
                    || folded
                        .get(i - 1)
                        .is_none_or(|(f, _, _)| !f.is_alphanumeric());
                let after_ok =
                    !last_alnum || folded.get(j).is_none_or(|(f, _, _)| !f.is_alphanumeric());
                let Some((_, _, end)) = window.last() else {
                    continue;
                };
                if before_ok && after_ok && start < end {
                    out.push(Detection {
                        kind: *kind,
                        start: *start,
                        end: *end,
                        source: Source::List,
                        confidence: CONF_LIST,
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::{BlockContext, BlockRole};
    use proptest::prelude::*;

    fn block(text: &str) -> TextBlock {
        TextBlock {
            text: text.to_string(),
            ctx: BlockContext::new("test", BlockRole::Body),
        }
    }

    fn list(terms: &[(&str, EntityKind)]) -> ListDetector {
        ListDetector::new(terms.iter().map(|(t, k)| (t.to_string(), *k)).collect())
    }

    fn found(l: &ListDetector, text: &str) -> Vec<(EntityKind, String)> {
        l.detect(&block(text), &DetectOptions::default())
            .into_iter()
            .map(|d| {
                assert_eq!(d.source, Source::List);
                assert!((d.confidence - 1.0).abs() < f32::EPSILON);
                (d.kind, text[d.start..d.end].to_string())
            })
            .collect()
    }

    #[test]
    fn case_and_accent_insensitive() {
        let l = list(&[("Éric Dupont", EntityKind::Personne)]);
        for t in [
            "Éric Dupont",
            "eric dupont",
            "ÉRIC DUPONT",
            "Eric DUPONT",
            "éric\u{a0}dupont",
        ] {
            assert_eq!(
                found(&l, t),
                vec![(EntityKind::Personne, t.to_string())],
                "{t}"
            );
        }
        let l = list(&[("eric", EntityKind::Prenom)]);
        assert_eq!(
            found(&l, "Bonjour Éric, ça va ?"),
            vec![(EntityKind::Prenom, "Éric".to_string())]
        );
    }

    #[test]
    fn whole_words_only() {
        let l = list(&[
            ("Dupont", EntityKind::Nom),
            ("Nautilus", EntityKind::Societe),
        ]);
        assert!(found(&l, "Dupontel").is_empty());
        assert!(found(&l, "Madupont").is_empty());
        assert!(found(&l, "Dupont2").is_empty());
        assert_eq!(found(&l, "(Dupont)").len(), 1);
        assert_eq!(found(&l, "Dupont.").len(), 1);
        assert_eq!(found(&l, "Dupont").len(), 1);
        assert_eq!(found(&l, "M. Dupont, Nautilus et Dupont").len(), 3);
    }

    #[test]
    fn offsets_are_original_bytes() {
        let l = list(&[("été", EntityKind::Autre)]);
        let t = "L'ÉTÉ 😀 été";
        let d = l.detect(&block(t), &DetectOptions::default());
        let vals: Vec<&str> = d.iter().map(|d| &t[d.start..d.end]).collect();
        assert_eq!(vals, vec!["ÉTÉ", "été"]);
    }

    #[test]
    fn punctuation_terms_and_empty_terms() {
        let l = list(&[
            ("", EntityKind::Nom),
            ("   ", EntityKind::Nom),
            ("M. Dupont", EntityKind::Personne),
        ]);
        assert!(!l.is_empty());
        assert_eq!(
            found(&l, "Voir M. Dupont."),
            vec![(EntityKind::Personne, "M. Dupont".to_string())]
        );
        assert!(ListDetector::new(vec![]).is_empty());
        assert!(found(&ListDetector::new(vec![]), "texte").is_empty());
        assert!(found(&list(&[("", EntityKind::Nom)]), "texte").is_empty());
    }

    #[test]
    fn several_kinds_and_disabled() {
        let l = list(&[
            ("Dupont", EntityKind::Nom),
            ("Dupont SARL", EntityKind::Societe),
        ]);
        let t = "Dupont SARL";
        let mut kinds: Vec<_> = found(&l, t).into_iter().map(|(k, _)| k).collect();
        kinds.sort();
        assert_eq!(kinds, vec![EntityKind::Nom, EntityKind::Societe]);
        let mut opts = DetectOptions::default();
        opts.disabled.insert(EntityKind::Nom);
        assert_eq!(l.detect(&block(t), &opts).len(), 1);
        assert_eq!(l.name(), "list");
    }

    proptest! {
        #[test]
        fn never_panics_and_offsets_valid(text in "\\PC{0,120}", term in "\\PC{0,10}") {
            let l = ListDetector::new(vec![(term.clone(), EntityKind::Autre), ("é a".to_string(), EntityKind::Nom)]);
            for d in l.detect(&block(&text), &DetectOptions::default()) {
                prop_assert!(d.start < d.end && d.end <= text.len());
                prop_assert!(text.is_char_boundary(d.start) && text.is_char_boundary(d.end));
            }
        }
    }
}
