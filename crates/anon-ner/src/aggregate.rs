//! Partie pure (sans ONNX Runtime) : plan des fenêtres, moyenne des probabilités,
//! agrégation des sous-mots en entités, ajustement des offsets.
//!
//! Transcription exacte de `tools/model/pipeline.py` : toute modification doit être
//! reportée des deux côtés (le test `python_concordance` le vérifie).

use crate::labels::{Bio, Label};
use anon_core::entities::EntityKind;
use std::ops::Range;

/// Longueur totale d'une fenêtre envoyée au modèle, jetons spéciaux compris.
pub const WINDOW_TOKENS: usize = 256;
/// Chevauchement entre deux fenêtres consécutives, en jetons.
pub const WINDOW_OVERLAP: usize = 64;
/// Confiance moyenne minimale d'une entité (moyenne des probabilités de ses jetons).
pub const MIN_CONFIDENCE: f32 = 0.5;

/// Jetons de contenu par fenêtre (sans `<s>` / `</s>`).
const WINDOW_CONTENT: usize = WINDOW_TOKENS - 2;
/// Pas entre deux débuts de fenêtre.
const WINDOW_STEP: usize = WINDOW_CONTENT - WINDOW_OVERLAP;

/// Découpe `n` jetons en fenêtres chevauchantes (intervalles de jetons de contenu).
pub fn windows(n: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    let mut start = 0;
    loop {
        let end = (start + WINDOW_CONTENT).min(n);
        out.push(start..end);
        if end >= n {
            return out;
        }
        start += WINDOW_STEP;
    }
}

/// Accumulateur des probabilités par jeton sur plusieurs fenêtres.
#[derive(Debug)]
pub struct ProbAccumulator {
    n_labels: usize,
    sums: Vec<f64>,
    counts: Vec<u32>,
}

impl ProbAccumulator {
    pub fn new(n_tokens: usize, n_labels: usize) -> Self {
        Self {
            n_labels,
            sums: vec![0.0; n_tokens * n_labels],
            counts: vec![0; n_tokens],
        }
    }

    /// Ajoute les logits d'une fenêtre (`logits.len() == range.len() * n_labels`,
    /// jetons spéciaux déjà retirés) après softmax.
    pub fn add_window(&mut self, range: Range<usize>, logits: &[f32]) {
        let nl = self.n_labels;
        for (k, tok) in range.enumerate() {
            let Some(row) = logits.get(k * nl..(k + 1) * nl) else {
                return;
            };
            let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
            let exps: Vec<f64> = row.iter().map(|&x| (x as f64 - max).exp()).collect();
            let total: f64 = exps.iter().sum();
            if let Some(dst) = self.sums.get_mut(tok * nl..(tok + 1) * nl) {
                for (d, e) in dst.iter_mut().zip(&exps) {
                    *d += e / total;
                }
            }
            if let Some(c) = self.counts.get_mut(tok) {
                *c += 1;
            }
        }
    }

    /// Étiquette retenue (argmax de la moyenne) et sa probabilité, par jeton.
    pub fn argmax(&self) -> Vec<(usize, f32)> {
        let nl = self.n_labels;
        self.counts
            .iter()
            .enumerate()
            .map(|(tok, &c)| {
                let row = self.sums.get(tok * nl..(tok + 1) * nl).unwrap_or(&[]);
                let mut best = (0usize, f64::NEG_INFINITY);
                for (i, &v) in row.iter().enumerate() {
                    if v > best.1 {
                        best = (i, v);
                    }
                }
                let p = if c == 0 { 0.0 } else { best.1 / c as f64 };
                (best.0, p as f32)
            })
            .collect()
    }
}

/// Entité trouvée dans un texte : offsets en octets, sur frontières de caractères.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub kind: EntityKind,
    pub start: usize,
    pub end: usize,
    pub confidence: f32,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Agrège les étiquettes par jeton en entités.
///
/// - `offsets` : intervalle d'octets de chaque jeton dans `text` ;
/// - `labels` : table des étiquettes du modèle ;
/// - `preds` : (indice d'étiquette, probabilité) par jeton.
pub fn aggregate(
    text: &str,
    offsets: &[(usize, usize)],
    labels: &[Label],
    preds: &[(usize, f32)],
) -> Vec<Span> {
    struct Cur {
        kind: EntityKind,
        start: usize,
        end: usize,
        scores: Vec<f32>,
    }
    let mut raw: Vec<Cur> = Vec::new();
    let mut open = false;
    for (&(s, e), &(li, score)) in offsets.iter().zip(preds) {
        if e <= s {
            continue;
        }
        let Some(label) = labels.get(li) else {
            open = false;
            continue;
        };
        let Some(kind) = label.kind else {
            open = false;
            continue;
        };
        let last = if open { raw.last_mut() } else { None };
        match last {
            Some(cur) if cur.kind == kind && (label.bio != Bio::B || s <= cur.end) => {
                cur.end = e;
                cur.scores.push(score);
            }
            _ => {
                raw.push(Cur {
                    kind,
                    start: s,
                    end: e,
                    scores: vec![score],
                });
                open = true;
            }
        }
    }

    let mut out: Vec<Span> = Vec::new();
    for cur in raw {
        let Some((start, end)) = snap_to_words(text, cur.start, cur.end) else {
            continue;
        };
        let conf = cur.scores.iter().map(|&x| x as f64).sum::<f64>() / cur.scores.len() as f64;
        let conf = conf as f32;
        if conf < MIN_CONFIDENCE {
            continue;
        }
        if let Some(prev) = out.last_mut()
            && prev.kind == cur.kind
            && prev.end >= start
        {
            prev.end = prev.end.max(end);
            prev.confidence = prev.confidence.min(conf);
            continue;
        }
        out.push(Span {
            kind: cur.kind,
            start,
            end,
            confidence: conf,
        });
    }
    out
}

/// Étend `start..end` aux frontières de mot puis retire ponctuation et espaces aux bords.
/// Renvoie `None` si rien ne reste. Les offsets renvoyés sont sur des frontières de caractères.
fn snap_to_words(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let mut s = floor_char_boundary(text, start.min(text.len()));
    let mut e = ceil_char_boundary(text, end.min(text.len()));
    // Un jeton coupé au milieu d'un mot : on étend jusqu'aux frontières du mot.
    if text
        .get(s..)
        .and_then(|t| t.chars().next())
        .is_some_and(is_word_char)
    {
        while let Some(c) = text.get(..s).and_then(|t| t.chars().next_back()) {
            if !is_word_char(c) {
                break;
            }
            s -= c.len_utf8();
        }
    }
    if text
        .get(..e)
        .and_then(|t| t.chars().next_back())
        .is_some_and(is_word_char)
    {
        while let Some(c) = text.get(e..).and_then(|t| t.chars().next()) {
            if !is_word_char(c) {
                break;
            }
            e += c.len_utf8();
        }
    }
    while s < e {
        let c = text.get(s..e)?.chars().next()?;
        if is_word_char(c) {
            break;
        }
        s += c.len_utf8();
    }
    while e > s {
        let c = text.get(s..e)?.chars().next_back()?;
        if is_word_char(c) {
            break;
        }
        e -= c.len_utf8();
    }
    (s < e).then_some((s, e))
}

fn floor_char_boundary(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(text: &str, mut i: usize) -> usize {
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::parse_labels;

    fn labels() -> Vec<Label> {
        parse_labels(&["O", "B-PER", "I-PER", "B-ORG", "I-ORG"])
    }

    #[test]
    fn windows_cover_all_tokens_with_overlap() {
        assert!(windows(0).is_empty());
        assert_eq!(windows(10), vec![0..10]);
        assert_eq!(windows(WINDOW_CONTENT), vec![0..WINDOW_CONTENT]);
        let w = windows(1000);
        assert_eq!(w.first().map(|r| r.start), Some(0));
        assert_eq!(w.last().map(|r| r.end), Some(1000));
        for pair in w.windows(2) {
            assert_eq!(pair[0].end - pair[1].start, WINDOW_OVERLAP);
        }
    }

    #[test]
    fn accumulator_averages_overlapping_windows() {
        let mut acc = ProbAccumulator::new(2, 2);
        acc.add_window(0..2, &[10.0, 0.0, 0.0, 10.0]);
        acc.add_window(1..2, &[10.0, 0.0]);
        let p = acc.argmax();
        assert_eq!(p[0].0, 0);
        assert!((p[1].1 - 0.5).abs() < 0.01);
    }

    #[test]
    fn subwords_merge_and_snap_to_word() {
        // « Mme Kaloï est là » : jetons « Ka », « lo », « ï » (sous-mots), le dernier mal étiqueté.
        let text = "Mme Kaloï est là";
        let offsets = [(0, 3), (4, 6), (6, 8), (8, 10), (11, 14), (15, 18)];
        let preds = [(0, 0.9), (1, 0.9), (2, 0.9), (0, 0.6), (0, 0.9), (0, 0.9)];
        let spans = aggregate(text, &offsets, &labels(), &preds);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].start..spans[0].end], "Kaloï");
        assert_eq!(spans[0].kind, EntityKind::Personne);
    }

    #[test]
    fn b_label_starts_new_entity_after_space() {
        let text = "Jean Paul";
        let spans = aggregate(text, &[(0, 4), (5, 9)], &labels(), &[(1, 0.9), (1, 0.9)]);
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn low_confidence_is_dropped_and_punctuation_trimmed() {
        let text = "« Dupont », dit-il";
        let off = [(0, 2), (3, 9), (10, 12)];
        let spans = aggregate(text, &off, &labels(), &[(1, 0.4), (2, 0.4), (0, 0.9)]);
        assert!(spans.is_empty());
        let spans = aggregate(text, &off, &labels(), &[(1, 0.9), (2, 0.9), (2, 0.9)]);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].start..spans[0].end], "Dupont");
    }

    #[test]
    fn snap_handles_multibyte_and_bad_offsets() {
        let text = "é😀Lê";
        assert_eq!(snap_to_words(text, 1, 100), Some((0, text.len())));
        assert_eq!(snap_to_words(text, 2, 100), Some((6, text.len())));
        // Le jeton « 😀 » seul : rien à garder ; « é😀 » : on garde « é » sans déborder sur « Lê ».
        assert_eq!(snap_to_words(text, 2, 6), None);
        assert_eq!(snap_to_words(text, 0, 6), Some((0, 2)));
        assert_eq!(snap_to_words("", 0, 3), None);
    }
}
