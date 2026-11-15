//! Préparation de l'image pour YuNet : composition sur fond blanc, réduction par
//! moyenne de surface, plans B, G, R en `f32` (0 à 255) complétés de zéros en bas et
//! à droite jusqu'à un multiple de [`PAD_DIVISOR`] (`padWithDivisor` puis
//! `blobFromImage` de la référence OpenCV : ordre BGR, aucune normalisation).
//!
//! Tout accès aux pixels passe par `get` ou des itérateurs : un tampon incohérent
//! donne `None`, jamais une panique.

use super::LEVEL_SIDES;
use std::fmt;

/// Multiple auquel la référence complète l'entrée du modèle (`divisor` d'OpenCV).
pub(crate) const PAD_DIVISOR: usize = 32;
/// Canaux du tenseur, dans l'ordre d'OpenCV : bleu, vert, rouge.
const CHANNELS: usize = 3;
/// Octets d'un pixel RGBA.
const RGBA_BYTES: usize = 4;
/// Valeur d'un canal blanc, fond sur lequel les pixels transparents sont composés.
const WHITE: f32 = 255.0;

/// Taille complétée au multiple de [`PAD_DIVISOR`] (formule de la référence).
pub(crate) fn padded(n: usize) -> usize {
    (n.saturating_sub(1) / PAD_DIVISOR + 1) * PAD_DIVISOR
}

/// Image prête pour le modèle : `data` est le tenseur NCHW (`1 × 3 × pad_h × pad_w`).
/// Les pixels sont du contenu utilisateur : `Debug` n'affiche que les dimensions.
#[derive(Clone, PartialEq)]
pub(crate) struct Planes {
    /// Taille utile, sans le complément.
    pub width: usize,
    pub height: usize,
    /// Taille complétée (multiple de [`PAD_DIVISOR`]).
    pub pad_w: usize,
    pub pad_h: usize,
    pub data: Vec<f32>,
}

impl fmt::Debug for Planes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planes")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pad_w", &self.pad_w)
            .field("pad_h", &self.pad_h)
            .finish_non_exhaustive()
    }
}

/// Source de lignes de pixels, rendues entrelacées B, G, R (`3 × width` valeurs).
pub(crate) trait RowSource {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    /// Remplit `out` avec la ligne `y`. `None` si la ligne n'existe pas.
    fn read_row(&self, y: usize, out: &mut [f32]) -> Option<()>;
}

/// Pixels RGBA 8 bits, ligne après ligne, sans remplissage.
#[derive(Clone, Copy)]
pub(crate) struct Rgba<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl fmt::Debug for Rgba<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rgba")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl RowSource for Rgba<'_> {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    /// Composition sur fond blanc : un pixel transparent se lit comme le voit
    /// l'utilisateur sur une page.
    fn read_row(&self, y: usize, out: &mut [f32]) -> Option<()> {
        let stride = self.width.checked_mul(RGBA_BYTES)?;
        let start = y.checked_mul(stride)?;
        let row = self.data.get(start..start.checked_add(stride)?)?;
        let (pixels, _) = row.as_chunks::<RGBA_BYTES>();
        for (o, &[r, g, b, a]) in out.as_chunks_mut::<CHANNELS>().0.iter_mut().zip(pixels) {
            let alpha = f32::from(a) / 255.0;
            let blend = |c: u8| f32::from(c) * alpha + WHITE * (1.0 - alpha);
            *o = [blend(b), blend(g), blend(r)];
        }
        Some(())
    }
}

impl RowSource for Planes {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn read_row(&self, y: usize, out: &mut [f32]) -> Option<()> {
        let plane = self.pad_w.checked_mul(self.pad_h)?;
        let row = |c: usize| {
            let start = c * plane + y * self.pad_w;
            self.data.get(start..start + self.width)
        };
        let (b, g, r) = (row(0)?, row(1)?, row(2)?);
        let outs = out.as_chunks_mut::<CHANNELS>().0.iter_mut();
        for (((o, b), g), r) in outs.zip(b).zip(g).zip(r) {
            *o = [*b, *g, *r];
        }
        Some(())
    }
}

/// Contribution des pixels sources `start..start + weights.len()` à un pixel produit.
#[derive(Debug, Clone, PartialEq)]
struct Taps {
    start: usize,
    weights: Vec<f32>,
}

/// Poids de la moyenne de surface sur un axe (`INTER_AREA` d'OpenCV) : chaque pixel
/// produit couvre `src / dst` pixels sources, pondérés par la part couverte.
fn area_taps(src: usize, dst: usize) -> Vec<Taps> {
    let ratio = src as f64 / dst as f64;
    (0..dst)
        .map(|o| {
            let a = o as f64 * ratio;
            let b = (o + 1) as f64 * ratio;
            let start = (a.floor() as usize).min(src.saturating_sub(1));
            let end = (b.ceil() as usize).clamp(start + 1, src);
            let weights = (start..end)
                .map(|i| {
                    let covered = (b.min((i + 1) as f64) - a.max(i as f64)).max(0.0);
                    (covered / ratio) as f32
                })
                .collect();
            Taps { start, weights }
        })
        .collect()
}

/// Réduit une ligne entrelacée selon `taps`.
fn resample_row(src: &[f32], taps: &[Taps], out: &mut [f32]) -> Option<()> {
    for (o, tap) in out.as_chunks_mut::<CHANNELS>().0.iter_mut().zip(taps) {
        let start = tap.start.checked_mul(CHANNELS)?;
        let end = start.checked_add(tap.weights.len().checked_mul(CHANNELS)?)?;
        let (pixels, _) = src.get(start..end)?.as_chunks::<CHANNELS>();
        let mut sum = [0f32; CHANNELS];
        for (px, &w) in pixels.iter().zip(&tap.weights) {
            for (s, &v) in sum.iter_mut().zip(px) {
                *s += v * w;
            }
        }
        *o = sum;
    }
    Some(())
}

/// Image de `width × height` pixels tirée de `src` (moyenne de surface, ou copie si
/// la taille est la même), complétée de zéros. `None` si une dimension est nulle ou
/// si la source est incohérente.
pub(crate) fn resample(src: &impl RowSource, width: usize, height: usize) -> Option<Planes> {
    let (src_w, src_h) = (src.width(), src.height());
    if width == 0 || height == 0 || src_w == 0 || src_h == 0 {
        return None;
    }
    let (pad_w, pad_h) = (padded(width), padded(height));
    let plane = pad_w.checked_mul(pad_h)?;
    let mut data = vec![0f32; plane.checked_mul(CHANNELS)?];
    let mut src_row = vec![0f32; src_w.checked_mul(CHANNELS)?];
    let mut row = vec![0f32; width.checked_mul(CHANNELS)?];

    let same_width = width == src_w;
    let x_taps = if same_width {
        Vec::new()
    } else {
        area_taps(src_w, width)
    };
    let y_taps = area_taps(src_h, height);
    // Ligne source déjà réduite en largeur : deux lignes produites voisines partagent
    // au plus une ligne source.
    let mut cached: Option<usize> = None;
    let mut reduced = vec![0f32; row.len()];
    for (y, tap) in y_taps.iter().enumerate() {
        row.fill(0.0);
        for (k, &wy) in tap.weights.iter().enumerate() {
            let sy = tap.start + k;
            if cached != Some(sy) {
                src.read_row(sy, &mut src_row)?;
                if same_width {
                    for (d, s) in reduced.iter_mut().zip(&src_row) {
                        *d = *s;
                    }
                } else {
                    resample_row(&src_row, &x_taps, &mut reduced)?;
                }
                cached = Some(sy);
            }
            for (d, s) in row.iter_mut().zip(&reduced) {
                *d += wy * s;
            }
        }
        for (c, channel) in data.chunks_exact_mut(plane).enumerate() {
            let start = y.checked_mul(pad_w)?;
            let dst = channel.get_mut(start..start.checked_add(width)?)?;
            for (d, px) in dst.iter_mut().zip(row.as_chunks::<CHANNELS>().0) {
                *d = px.get(c).copied()?;
            }
        }
    }
    Some(Planes {
        width,
        height,
        pad_w,
        pad_h,
        data,
    })
}

/// Tailles des niveaux de la pyramide pour une image `width × height`, du plus grand
/// au plus petit : plus grand côté ramené à chaque valeur de [`LEVEL_SIDES`], jamais
/// agrandi, au moins un pixel, sans doublon.
pub(crate) fn level_sizes(width: usize, height: usize) -> Vec<(usize, usize)> {
    let longest = width.max(height).max(1) as f64;
    let mut sizes: Vec<(usize, usize)> = Vec::with_capacity(LEVEL_SIDES.len());
    for side in LEVEL_SIDES {
        let scale = (f64::from(side) / longest).min(1.0);
        let size = (
            ((width as f64 * scale).round() as usize).max(1),
            ((height as f64 * scale).round() as usize).max(1),
        );
        if sizes.last() != Some(&size) {
            sizes.push(size);
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(width: usize, height: usize, px: [u8; 4]) -> Vec<u8> {
        px.repeat(width * height)
    }

    #[test]
    fn pads_like_the_reference() {
        assert_eq!(padded(1), 32);
        assert_eq!(padded(32), 32);
        assert_eq!(padded(33), 64);
        assert_eq!(padded(671), 672);
        assert_eq!(padded(1000), 1024);
    }

    #[test]
    fn copies_bgr_order_and_pads_with_zeros() {
        let data = rgba(3, 2, [10, 20, 30, 255]);
        let src = Rgba {
            data: &data,
            width: 3,
            height: 2,
        };
        let p = resample(&src, 3, 2).unwrap();
        assert_eq!((p.pad_w, p.pad_h), (32, 32));
        assert_eq!(p.data.len(), 3 * 32 * 32);
        let plane = 32 * 32;
        assert_eq!(p.data[0], 30.0, "bleu d'abord");
        assert_eq!(p.data[plane], 20.0, "puis vert");
        assert_eq!(p.data[2 * plane], 10.0, "puis rouge");
        assert_eq!(p.data[2 * plane + 32 + 2], 10.0, "dernier pixel utile");
        assert_eq!(p.data[3], 0.0, "complément à droite");
        assert_eq!(p.data[2 * 32], 0.0, "complément en bas");
        let total: f32 = p.data.iter().sum();
        assert_eq!(total, 6.0 * 60.0);
    }

    #[test]
    fn transparent_pixels_are_white() {
        let data = [0, 0, 0, 0, 200, 100, 50, 255];
        let src = Rgba {
            data: &data,
            width: 2,
            height: 1,
        };
        let p = resample(&src, 2, 1).unwrap();
        let plane = p.pad_w * p.pad_h;
        assert_eq!(
            [p.data[0], p.data[plane], p.data[2 * plane]],
            [255.0; 3],
            "transparent → blanc"
        );
        assert_eq!(
            [p.data[1], p.data[plane + 1], p.data[2 * plane + 1]],
            [50.0, 100.0, 200.0]
        );
    }

    #[test]
    fn area_average_keeps_mean_and_uniform_colour() {
        // Damier noir et blanc 4 × 4 réduit en 2 × 2 : gris moyen partout.
        let mut data = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                data.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let src = Rgba {
            data: &data,
            width: 4,
            height: 4,
        };
        let p = resample(&src, 2, 2).unwrap();
        for (y, x) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            assert!((p.data[y * p.pad_w + x] - 127.5).abs() < 1e-3);
        }
        // Rapport non entier (7 → 3) : une couleur uniforme reste uniforme.
        let data = rgba(7, 5, [40, 80, 120, 255]);
        let src = Rgba {
            data: &data,
            width: 7,
            height: 5,
        };
        let p = resample(&src, 3, 2).unwrap();
        let plane = p.pad_w * p.pad_h;
        for y in 0..2 {
            for x in 0..3 {
                let i = y * p.pad_w + x;
                assert!((p.data[i] - 120.0).abs() < 1e-3);
                assert!((p.data[plane + i] - 80.0).abs() < 1e-3);
                assert!((p.data[2 * plane + i] - 40.0).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn cascade_from_planes() {
        let data = rgba(64, 48, [1, 2, 3, 255]);
        let src = Rgba {
            data: &data,
            width: 64,
            height: 48,
        };
        let big = resample(&src, 64, 48).unwrap();
        let small = resample(&big, 16, 12).unwrap();
        assert_eq!((small.width, small.height), (16, 12));
        assert!((small.data[0] - 3.0).abs() < 1e-4);
    }

    #[test]
    fn taps_cover_each_source_pixel_once() {
        for (src, dst) in [(10, 3), (1000, 320), (7, 7), (5, 1), (1, 1)] {
            let taps = area_taps(src, dst);
            let mut coverage = vec![0f64; src];
            for t in &taps {
                let sum: f32 = t.weights.iter().sum();
                assert!((sum - 1.0).abs() < 1e-4, "{src}→{dst} : somme {sum}");
                for (k, w) in t.weights.iter().enumerate() {
                    coverage[t.start + k] += f64::from(*w) * src as f64 / dst as f64;
                }
            }
            for c in coverage {
                assert!((c - 1.0).abs() < 1e-3, "{src}→{dst} : couverture {c}");
            }
        }
    }

    #[test]
    fn inconsistent_sources_give_none() {
        let data = rgba(2, 2, [0, 0, 0, 255]);
        let short = Rgba {
            data: &data[..10],
            width: 2,
            height: 2,
        };
        assert!(resample(&short, 2, 2).is_none());
        let src = Rgba {
            data: &data,
            width: 2,
            height: 2,
        };
        assert!(resample(&src, 0, 2).is_none());
        let empty = Rgba {
            data: &[],
            width: 0,
            height: 0,
        };
        assert!(resample(&empty, 1, 1).is_none());
    }

    #[test]
    fn levels_never_upscale_and_keep_aspect() {
        assert_eq!(level_sizes(4000, 3000), vec![(1280, 960), (320, 240)]);
        assert_eq!(level_sizes(671, 1000), vec![(671, 1000), (215, 320)]);
        assert_eq!(level_sizes(200, 100), vec![(200, 100)]);
        assert_eq!(level_sizes(1, 1), vec![(1, 1)]);
        assert_eq!(level_sizes(100_000, 1), vec![(1280, 1), (320, 1)]);
    }

    #[test]
    fn debug_shows_no_pixel() {
        let data = rgba(1, 1, [9, 9, 9, 255]);
        let src = Rgba {
            data: &data,
            width: 1,
            height: 1,
        };
        let p = resample(&src, 1, 1).unwrap();
        assert!(!format!("{p:?}").contains('9'));
        assert!(!format!("{src:?}").contains('9'));
    }
}
