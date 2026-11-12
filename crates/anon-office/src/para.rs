//! Recollage des runs d'un paragraphe et répartition des remplacements.
//!
//! Un paragraphe (`w:p`, `a:p`, `si`, `text:p` en ODF…) est découpé en
//! segments : morceaux de texte situés dans la source (modifiables) ou
//! caractères virtuels (tabulation, saut de ligne) non modifiables. Le texte
//! du bloc est leur concaténation. Un remplacement est réparti ainsi : son
//! texte va dans le premier segment modifiable touché, les suivants sont vidés
//! pour la partie couverte. Chaque segment reste dans son run d'origine, donc
//! la mise en forme est conservée.

use crate::xml::Patch;
use anon_core::Replacement;
use std::ops::Range;

/// Attribut posé sur un élément texte modifié pour conserver les espaces.
pub(crate) const PRESERVE_ATTR: &str = " xml:space=\"preserve\"";

#[derive(Debug, Clone)]
struct Seg {
    /// Intervalle dans le texte du paragraphe.
    at: Range<usize>,
    /// Intervalle dans la source ; `None` pour un caractère virtuel.
    src: Option<Range<usize>>,
    /// Élément porteur (pour `xml:space`).
    holder: Option<usize>,
}

/// Paragraphe en cours de lecture.
#[derive(Debug, Clone, Default)]
pub(crate) struct Para {
    pub text: String,
    segs: Vec<Seg>,
    /// Pour chaque élément porteur, correctif `xml:space="preserve"` à poser
    /// s'il est modifié.
    holders: Vec<Option<Patch>>,
}

/// Résultat de la répartition.
#[derive(Debug, Default)]
pub(crate) struct Edits {
    pub patches: Vec<Patch>,
    /// Remplacements effectivement placés.
    pub applied: usize,
}

impl Para {
    pub fn new() -> Self {
        Self::default()
    }

    /// Déclare un élément porteur de texte ; `preserve` est le correctif à
    /// appliquer s'il est modifié (`None` : inutile).
    pub fn add_holder(&mut self, preserve: Option<Patch>) -> usize {
        self.holders.push(preserve);
        self.holders.len() - 1
    }

    /// Ajoute un morceau de texte lu dans la source.
    pub fn push_text(&mut self, text: &str, src: Range<usize>, holder: Option<usize>) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        self.segs.push(Seg {
            at: start..self.text.len(),
            src: Some(src),
            holder,
        });
    }

    /// Ajoute un caractère virtuel (tabulation, saut de ligne).
    pub fn push_virtual(&mut self, text: &str) {
        let start = self.text.len();
        self.text.push_str(text);
        self.segs.push(Seg {
            at: start..self.text.len(),
            src: None,
            holder: None,
        });
    }

    /// Répartit des remplacements (triés, sans chevauchement) sur les segments.
    /// `encode` transforme le nouveau texte d'un segment en texte brut à écrire.
    pub fn edits(&self, reps: &[Replacement], encode: &dyn Fn(&str) -> String) -> Edits {
        let mut local: Vec<Vec<(usize, usize, String)>> = vec![Vec::new(); self.segs.len()];
        let mut applied = 0usize;
        let mut last_end = 0usize;
        for r in reps {
            if r.start > r.end || r.end > self.text.len() || r.start < last_end {
                continue;
            }
            let mut placed = false;
            for (k, s) in self.segs.iter().enumerate() {
                if s.src.is_none() {
                    continue;
                }
                let overlap = s.at.start < r.end && r.start < s.at.end;
                let insertion = r.start == r.end && s.at.start <= r.start && r.start < s.at.end;
                if !overlap && !insertion {
                    continue;
                }
                let ls = r.start.max(s.at.start) - s.at.start;
                let le = r.end.min(s.at.end) - s.at.start;
                let with = if placed {
                    String::new()
                } else {
                    placed = true;
                    r.with.clone()
                };
                if let Some(l) = local.get_mut(k) {
                    l.push((ls, le, with));
                }
            }
            if placed {
                applied += 1;
                last_end = r.end;
            }
        }
        let mut patches = Vec::new();
        let mut touched = vec![false; self.holders.len()];
        for (s, edits) in self.segs.iter().zip(local.iter()) {
            if edits.is_empty() {
                continue;
            }
            let (Some(src), Some(old)) = (&s.src, self.text.get(s.at.clone())) else {
                continue;
            };
            let mut new = String::with_capacity(old.len());
            let mut cursor = 0usize;
            for (ls, le, with) in edits {
                new.push_str(old.get(cursor..*ls).unwrap_or(""));
                new.push_str(with);
                cursor = *le;
            }
            new.push_str(old.get(cursor..).unwrap_or(""));
            if new == old {
                continue;
            }
            patches.push(Patch::new(src.clone(), encode(&new)));
            if let Some(t) = s.holder.and_then(|h| touched.get_mut(h)) {
                *t = true;
            }
        }
        for (h, t) in self.holders.iter().zip(touched) {
            if let (Some(p), true) = (h, t) {
                patches.push(p.clone());
            }
        }
        Edits { patches, applied }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::apply_patches;

    fn rep(start: usize, end: usize, with: &str) -> Replacement {
        Replacement {
            start,
            end,
            with: with.into(),
        }
    }

    #[test]
    fn split_value_goes_to_first_run() {
        // <b>Jean </b><n>Dupont</n>, gérant
        let src = "<b>Jean </b><n>Dupont</n><n>, gérant</n>";
        let mut p = Para::new();
        let h1 = p.add_holder(Some(Patch::new(2..2, PRESERVE_ATTR)));
        p.push_text("Jean ", 3..8, Some(h1));
        let h2 = p.add_holder(None);
        p.push_text("Dupont", 15..21, Some(h2));
        let h3 = p.add_holder(None);
        p.push_text(", gérant", 28..37, Some(h3));
        assert_eq!(p.text, "Jean Dupont, gérant");
        let e = p.edits(&[rep(0, 11, "[PERSONNE_1]")], &|s| s.to_string());
        assert_eq!(e.applied, 1);
        let out = apply_patches(src, e.patches);
        assert_eq!(
            out,
            "<b xml:space=\"preserve\">[PERSONNE_1]</b><n></n><n>, gérant</n>"
        );
    }

    #[test]
    fn virtual_segments_are_kept() {
        let mut p = Para::new();
        p.push_text("A", 0..1, None);
        p.push_virtual(" ");
        p.push_text("B", 5..6, None);
        assert_eq!(p.text, "A B");
        let e = p.edits(&[rep(0, 3, "X")], &|s| s.to_string());
        assert_eq!(e.patches.len(), 2);
        assert_eq!(apply_patches("A....B", e.patches), "X....");
    }

    #[test]
    fn replacement_only_on_virtual_is_not_applied() {
        let mut p = Para::new();
        p.push_virtual("\t");
        let e = p.edits(&[rep(0, 1, "X")], &|s| s.to_string());
        assert_eq!(e.applied, 0);
        assert!(e.patches.is_empty());
    }
}
