//! Post-traitement de YuNet, repris de `FaceDetectorYNImpl::postProcess` d'OpenCV
//! (`modules/objdetect/src/face_detect.cpp`, branche 4.x) et de `dnn::NMSBoxes`
//! (`modules/dnn/src/nms.inl.hpp`) :
//!
//! - pour chaque pas (8, 16, 32) et chaque ancre `(r, c)` de la grille complétée :
//!   score `sqrt(clamp(cls) × clamp(obj))`, ancre écartée si `score < seuil` ;
//! - boîte : centre `((c + dx) × pas, (r + dy) × pas)`, taille `(exp(dw) × pas,
//!   exp(dh) × pas)` ;
//! - s'il reste plus d'une boîte : suppression des non-maxima sur des rectangles
//!   entiers (troncature `int(float)`), scores strictement supérieurs au seuil, tri
//!   stable par score décroissant, au plus `top_k`, boîte gardée si son IoU avec
//!   chaque boîte déjà gardée est inférieur ou égal au seuil.
//!
//! Écarts assumés : les cinq points du visage (yeux, nez, bouche) ne sont pas
//! décodés (inutiles pour masquer) ; une ancre dont le score ou la boîte n'est pas un
//! nombre fini est écartée ; la suppression des non-maxima se fait en coordonnées de
//! l'image d'origine, sur les boîtes de tous les niveaux de la pyramide réunies.

use super::{MARGIN_BOTTOM, MARGIN_SIDE, MARGIN_TOP};
use anon_core::format::PixelRect;

/// Sorties du modèle pour un pas : `cls` et `obj` (une valeur par ancre), `bbox`
/// (quatre valeurs par ancre : dx, dy, dw, dh).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Head<'a> {
    pub stride: usize,
    pub cls: &'a [f32],
    pub obj: &'a [f32],
    pub bbox: &'a [f32],
}

/// Visage candidat, en pixels de l'image d'origine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Candidate {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

/// Sorties du modèle de taille inattendue (modèle différent ou corrompu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShapeMismatch;

/// Décode les ancres d'un niveau complété à `pad_w × pad_h`. `scale_x` et `scale_y`
/// ramènent les coordonnées du niveau à celles de l'image d'origine.
pub(crate) fn decode(
    heads: &[Head<'_>],
    pad_w: usize,
    pad_h: usize,
    score_threshold: f32,
    (scale_x, scale_y): (f32, f32),
    out: &mut Vec<Candidate>,
) -> Result<(), ShapeMismatch> {
    for head in heads {
        let stride = head.stride.max(1);
        let cols = pad_w / stride;
        let rows = pad_h / stride;
        let anchors = cols * rows;
        if head.cls.len() != anchors || head.obj.len() != anchors || head.bbox.len() != 4 * anchors
        {
            return Err(ShapeMismatch);
        }
        let s = stride as f32;
        let cells = head
            .cls
            .iter()
            .zip(head.obj)
            .zip(head.bbox.as_chunks::<4>().0);
        for (idx, ((&cls, &obj), bbox)) in cells.enumerate() {
            let score = (cls.clamp(0.0, 1.0) * obj.clamp(0.0, 1.0)).sqrt();
            if score.is_nan() || score < score_threshold {
                continue;
            }
            let &[dx, dy, dw, dh] = bbox;
            let (r, c) = ((idx / cols) as f32, (idx % cols) as f32);
            let cx = (c + dx) * s;
            let cy = (r + dy) * s;
            let w = dw.exp() * s;
            let h = dh.exp() * s;
            let candidate = Candidate {
                x: (cx - w / 2.0) * scale_x,
                y: (cy - h / 2.0) * scale_y,
                w: w * scale_x,
                h: h * scale_y,
                score,
            };
            let finite = [candidate.x, candidate.y, candidate.w, candidate.h]
                .iter()
                .all(|v| v.is_finite());
            if finite && candidate.w > 0.0 && candidate.h > 0.0 {
                out.push(candidate);
            }
        }
    }
    Ok(())
}

/// Rectangle entier d'OpenCV (`Rect2i`), obtenu par troncature comme `int(float)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect2i {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

impl Rect2i {
    fn from(c: &Candidate) -> Self {
        // `as i32` tronque vers zéro et sature, comme une conversion C++ dans les bornes.
        Self {
            x: i64::from(c.x as i32),
            y: i64::from(c.y as i32),
            w: i64::from(c.w as i32),
            h: i64::from(c.h as i32),
        }
    }

    fn area(&self) -> i64 {
        self.w * self.h
    }

    /// Recouvrement `1 - jaccardDistance(a, b)` d'OpenCV.
    fn overlap(&self, other: &Self) -> f64 {
        let (aa, ab) = (self.area(), other.area());
        if aa + ab <= 0 {
            return 1.0;
        }
        let empty = |r: &Self| r.w <= 0 || r.h <= 0;
        let inter = if empty(self) || empty(other) {
            0
        } else {
            let w = (self.x + self.w).min(other.x + other.w) - self.x.max(other.x);
            let h = (self.y + self.h).min(other.y + other.h) - self.y.max(other.y);
            if w <= 0 || h <= 0 { 0 } else { w * h }
        };
        let inter = inter as f64;
        inter / (aa as f64 + ab as f64 - inter)
    }
}

/// Indices des candidats gardés par la suppression des non-maxima, par score
/// décroissant. Avec un seul candidat, la référence le garde sans ce filtre.
pub(crate) fn nms(
    candidates: &[Candidate],
    score_threshold: f32,
    nms_threshold: f32,
    top_k: usize,
) -> Vec<usize> {
    if candidates.len() <= 1 {
        return (0..candidates.len()).collect();
    }
    let mut order: Vec<usize> = (0..candidates.len())
        .filter(|&i| candidates.get(i).is_some_and(|c| c.score > score_threshold))
        .collect();
    // Tri stable par score décroissant (`std::stable_sort` de la référence).
    order.sort_by(|&a, &b| {
        let score = |i: usize| candidates.get(i).map_or(f32::MIN, |c| c.score);
        score(b).total_cmp(&score(a))
    });
    if top_k > 0 {
        order.truncate(top_k);
    }
    let rects: Vec<Rect2i> = candidates.iter().map(Rect2i::from).collect();
    let threshold = f64::from(nms_threshold);
    let mut kept: Vec<usize> = Vec::new();
    for idx in order {
        let Some(rect) = rects.get(idx) else {
            continue;
        };
        let keep = kept
            .iter()
            .filter_map(|&k| rects.get(k))
            .all(|other| rect.overlap(other) <= threshold);
        if keep {
            kept.push(idx);
        }
    }
    kept
}

/// Rectangle à masquer : boîte du modèle agrandie de [`MARGIN_SIDE`] à gauche et à
/// droite, [`MARGIN_TOP`] en haut (cheveux), [`MARGIN_BOTTOM`] en bas (menton, cou),
/// arrondie vers l'extérieur et bornée à l'image. `None` si rien n'en reste.
pub(crate) fn mask_rect(c: &Candidate, width: u32, height: u32) -> Option<PixelRect> {
    let (iw, ih) = (f64::from(width), f64::from(height));
    let (x, y, w, h) = (
        f64::from(c.x),
        f64::from(c.y),
        f64::from(c.w),
        f64::from(c.h),
    );
    let left = (x - w * MARGIN_SIDE).floor().clamp(0.0, iw);
    let right = (x + w + w * MARGIN_SIDE).ceil().clamp(0.0, iw);
    let top = (y - h * MARGIN_TOP).floor().clamp(0.0, ih);
    let bottom = (y + h + h * MARGIN_BOTTOM).ceil().clamp(0.0, ih);
    if !(right > left && bottom > top) {
        return None;
    }
    // Valeurs entières bornées par la taille de l'image : la conversion est exacte.
    Some(PixelRect {
        x: left as u32,
        y: top as u32,
        w: (right - left) as u32,
        h: (bottom - top) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pas des trois têtes du modèle.
    const STRIDES: [usize; 3] = [8, 16, 32];

    fn cand(x: f32, y: f32, w: f32, h: f32, score: f32) -> Candidate {
        Candidate { x, y, w, h, score }
    }

    /// Une grille 32 × 32 : 16 ancres au pas 8, 4 au pas 16, 1 au pas 32.
    struct Outputs {
        cls: [Vec<f32>; 3],
        obj: [Vec<f32>; 3],
        bbox: [Vec<f32>; 3],
    }

    impl Outputs {
        fn empty() -> Self {
            let n = |s: usize| (32 / s) * (32 / s);
            let make = |k: usize| STRIDES.map(|s| vec![0f32; k * n(s)]);
            Self {
                cls: make(1),
                obj: make(1),
                bbox: make(4),
            }
        }

        fn heads(&self) -> Vec<Head<'_>> {
            (0..3)
                .map(|i| Head {
                    stride: STRIDES[i],
                    cls: &self.cls[i],
                    obj: &self.obj[i],
                    bbox: &self.bbox[i],
                })
                .collect()
        }
    }

    #[test]
    fn decodes_like_opencv() {
        let mut o = Outputs::empty();
        // Pas 8, ancre (r = 1, c = 2) → idx 6 ; score sqrt(0,81 × 1) = 0,9.
        o.cls[0][6] = 0.81;
        o.obj[0][6] = 1.5; // borné à 1
        o.bbox[0][24..28].copy_from_slice(&[0.5, 0.25, 0.0, 1f32.ln()]);
        let mut out = Vec::new();
        decode(&o.heads(), 32, 32, 0.6, (1.0, 1.0), &mut out).unwrap();
        assert_eq!(out.len(), 1);
        let c = out[0];
        assert!((c.score - 0.9).abs() < 1e-6);
        // centre ((2 + 0,5) × 8, (1 + 0,25) × 8) = (20, 10) ; taille (8, 8).
        assert!((c.x - 16.0).abs() < 1e-4 && (c.y - 6.0).abs() < 1e-4);
        assert!((c.w - 8.0).abs() < 1e-4 && (c.h - 8.0).abs() < 1e-4);

        // Mise à l'échelle vers l'image d'origine.
        let mut scaled = Vec::new();
        decode(&o.heads(), 32, 32, 0.6, (2.0, 3.0), &mut scaled).unwrap();
        assert!((scaled[0].x - 32.0).abs() < 1e-4 && (scaled[0].h - 24.0).abs() < 1e-4);
    }

    #[test]
    fn score_below_threshold_or_nan_is_skipped() {
        let mut o = Outputs::empty();
        o.cls[1][0] = 0.25;
        o.obj[1][0] = 1.0; // score 0,5
        o.cls[2][0] = f32::NAN;
        o.obj[2][0] = 1.0;
        o.cls[0][0] = 1.0;
        o.obj[0][0] = 1.0;
        o.bbox[0][2] = f32::INFINITY; // largeur infinie
        let mut out = Vec::new();
        decode(&o.heads(), 32, 32, 0.6, (1.0, 1.0), &mut out).unwrap();
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn unexpected_output_sizes_are_an_error() {
        let o = Outputs::empty();
        let mut heads = o.heads();
        heads[1].bbox = &o.bbox[1][..4];
        assert_eq!(
            decode(&heads, 32, 32, 0.6, (1.0, 1.0), &mut Vec::new()),
            Err(ShapeMismatch)
        );
        assert_eq!(
            decode(&o.heads(), 64, 32, 0.6, (1.0, 1.0), &mut Vec::new()),
            Err(ShapeMismatch)
        );
    }

    #[test]
    fn nms_keeps_best_of_overlapping_boxes() {
        let c = [
            cand(10.0, 10.0, 100.0, 100.0, 0.8),
            cand(12.0, 12.0, 100.0, 100.0, 0.95),
            cand(300.0, 300.0, 50.0, 50.0, 0.7),
        ];
        assert_eq!(nms(&c, 0.6, 0.3, 5000), vec![1, 2]);
    }

    #[test]
    fn nms_threshold_is_inclusive_and_scores_strict() {
        // La seconde boîte est la moitié basse de la première : IoU de 0,5 exactement.
        let c = [
            cand(0.0, 0.0, 100.0, 100.0, 0.9),
            cand(0.0, 50.0, 100.0, 50.0, 0.8),
        ];
        assert_eq!(nms(&c, 0.6, 0.3, 5000), vec![0]);
        assert_eq!(
            nms(&c, 0.6, 0.5, 5000),
            vec![0, 1],
            "IoU égal au seuil : gardée"
        );
        // Score égal au seuil : écarté par la suppression (comparaison stricte)…
        let c = [
            cand(0.0, 0.0, 10.0, 10.0, 0.6),
            cand(100.0, 0.0, 10.0, 10.0, 0.9),
        ];
        assert_eq!(nms(&c, 0.6, 0.3, 5000), vec![1]);
        // …mais un candidat seul est gardé tel quel, comme dans la référence.
        assert_eq!(nms(&c[..1], 0.6, 0.3, 5000), vec![0]);
    }

    #[test]
    fn nms_truncates_like_rect2i_and_honours_top_k() {
        // 0,9 et 0,99 tronqués à 0 : rectangles identiques, IoU 1.
        let c = [
            cand(0.9, 0.99, 10.0, 10.0, 0.7),
            cand(0.0, 0.0, 10.9, 10.9, 0.8),
        ];
        assert_eq!(nms(&c, 0.6, 0.3, 5000), vec![1]);
        let c = [
            cand(0.0, 0.0, 10.0, 10.0, 0.7),
            cand(100.0, 0.0, 10.0, 10.0, 0.8),
            cand(200.0, 0.0, 10.0, 10.0, 0.9),
        ];
        assert_eq!(nms(&c, 0.6, 0.3, 2), vec![2, 1]);
    }

    #[test]
    fn mask_rect_adds_margins_and_stays_inside() {
        let c = cand(100.0, 100.0, 100.0, 100.0, 0.9);
        let r = mask_rect(&c, 1000, 1000).unwrap();
        let near = |got: u32, want: f64| (f64::from(got) - want).abs() <= 1.0;
        assert!(near(r.x, 100.0 - 100.0 * MARGIN_SIDE), "{r:?}");
        assert!(near(r.y, 100.0 - 100.0 * MARGIN_TOP), "{r:?}");
        assert!(near(r.w, 100.0 * (1.0 + 2.0 * MARGIN_SIDE)), "{r:?}");
        assert!(
            near(r.h, 100.0 * (1.0 + MARGIN_TOP + MARGIN_BOTTOM)),
            "{r:?}"
        );
        assert!(r.w > 100 && r.h > 100);

        let edge = cand(-20.0, 950.0, 60.0, 80.0, 0.9);
        let r = mask_rect(&edge, 1000, 1000).unwrap();
        assert_eq!((r.x, r.y + r.h), (0, 1000), "{r:?}");
        assert!(r.x + r.w <= 1000);
        assert!(mask_rect(&cand(2000.0, 0.0, 10.0, 10.0, 0.9), 1000, 1000).is_none());
    }
}
