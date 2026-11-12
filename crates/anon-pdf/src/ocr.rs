//! OCR Tesseract en mémoire : données de langue embarquées, verrou global,
//! préparation de l'image, regroupement des mots en paragraphes.

use std::fs;
use std::ops::Range;
use std::sync::{Mutex, MutexGuard};

use anon_core::{FormatError, OcrPage, OcrWord, PixelRect};
use image::GrayImage;
use image::imageops::FilterType;

use crate::ffi;

static FRA_TRAINEDDATA: &[u8] = include_bytes!("../../../assets/tessdata/fra.traineddata");
static ENG_TRAINEDDATA: &[u8] = include_bytes!("../../../assets/tessdata/eng.traineddata");

/// Langues Tesseract, dans l'ordre de priorité.
pub const OCR_LANGUAGES: &str = "fra+eng";
/// Résolution supposée d'une image isolée (sans information fiable).
pub const DEFAULT_IMAGE_DPI: u32 = 300;
/// En dessous de ce côté (en pixels), l'image est agrandie avant OCR.
pub const OCR_UPSCALE_BELOW_PX: u32 = 1600;
/// Facteur d'agrandissement des petites images.
pub const OCR_UPSCALE_FACTOR: u32 = 2;
/// Nombre maximal de pixels envoyés à Tesseract (au-delà : réduction).
pub const OCR_MAX_PIXELS: u64 = 40_000_000;
/// Confiance moyenne (0 à 1) en dessous de laquelle on avertit l'utilisateur.
pub const LOW_OCR_CONFIDENCE: f32 = 0.70;

/// Un seul OCR à la fois : Leptonica (dans MuPDF) a un allocateur global.
static OCR_LOCK: Mutex<()> = Mutex::new(());

fn ocr_error() -> FormatError {
    FormatError::Other("échec de l'OCR".into())
}

/// Mot reconnu, rectangle en pixels de l'image d'origine (flottants).
#[derive(Clone, PartialEq)]
pub(crate) struct OcrItem {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// 0 à 1.
    pub confidence: f32,
    pub para: i32,
    pub line: i32,
}

impl std::fmt::Debug for OcrItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrItem")
            .field("len", &self.text.len())
            .field("confidence", &self.confidence)
            .finish_non_exhaustive()
    }
}

/// Résultat d'OCR d'une image.
#[derive(Debug, Clone, Default)]
pub(crate) struct OcrResult {
    pub width: u32,
    pub height: u32,
    pub items: Vec<OcrItem>,
}

/// Paragraphe reconstitué : texte (mots séparés par une espace) et, pour chaque
/// mot, son intervalle d'octets et son indice dans `OcrResult::items`.
#[derive(Debug, Clone)]
pub(crate) struct OcrParagraph {
    pub text: String,
    pub words: Vec<(Range<usize>, usize)>,
}

impl OcrResult {
    pub fn mean_confidence(&self) -> f32 {
        if self.items.is_empty() {
            return 1.0;
        }
        self.items.iter().map(|w| w.confidence).sum::<f32>() / self.items.len() as f32
    }

    pub fn paragraphs(&self) -> Vec<OcrParagraph> {
        let mut out: Vec<OcrParagraph> = Vec::new();
        let mut current: Option<i32> = None;
        for (i, w) in self.items.iter().enumerate() {
            if current != Some(w.para) || out.is_empty() {
                out.push(OcrParagraph {
                    text: String::new(),
                    words: Vec::new(),
                });
                current = Some(w.para);
            }
            if let Some(p) = out.last_mut() {
                if !p.text.is_empty() {
                    p.text.push(' ');
                }
                let start = p.text.len();
                p.text.push_str(&w.text);
                p.words.push((start..p.text.len(), i));
            }
        }
        out
    }

    pub fn to_page(&self) -> OcrPage {
        OcrPage {
            width: self.width,
            height: self.height,
            words: self
                .items
                .iter()
                .map(|w| {
                    let x = w.x0.max(0.0).floor() as u32;
                    let y = w.y0.max(0.0).floor() as u32;
                    let x1 = (w.x1.max(0.0).ceil() as u32).min(self.width);
                    let y1 = (w.y1.max(0.0).ceil() as u32).min(self.height);
                    OcrWord {
                        text: w.text.clone(),
                        rect: PixelRect {
                            x,
                            y,
                            w: x1.saturating_sub(x),
                            h: y1.saturating_sub(y),
                        },
                        confidence: w.confidence,
                    }
                })
                .collect(),
        }
    }
}

/// Session d'OCR : détient le verrou global et le dossier privé des données de
/// langue (supprimé à la fin). Ne jamais en ouvrir deux dans le même fil.
pub(crate) struct OcrSession {
    _dir: tempfile::TempDir,
    datadir: String,
    _guard: MutexGuard<'static, ()>,
}

impl std::fmt::Debug for OcrSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrSession").finish_non_exhaustive()
    }
}

impl OcrSession {
    pub fn begin() -> Result<Self, FormatError> {
        let guard = OCR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Données de langue (pas de contenu utilisateur) dans un dossier privé.
        let dir = tempfile::Builder::new()
            .prefix("anon-tessdata-")
            .tempdir()?;
        fs::write(dir.path().join("fra.traineddata"), FRA_TRAINEDDATA)?;
        fs::write(dir.path().join("eng.traineddata"), ENG_TRAINEDDATA)?;
        let datadir = dir
            .path()
            .to_str()
            .ok_or_else(|| FormatError::Other("dossier temporaire non UTF-8".into()))?
            .replace('\\', "/");
        Ok(Self {
            _dir: dir,
            datadir,
            _guard: guard,
        })
    }

    /// OCR d'une image en niveaux de gris. `dpi` : résolution réelle si connue.
    pub fn recognize(&self, img: &GrayImage, dpi: u32) -> Result<OcrResult, FormatError> {
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return Ok(OcrResult {
                width: w,
                height: h,
                items: Vec::new(),
            });
        }
        // Mise à l'échelle : agrandir les petites images, réduire les énormes.
        let pixels = u64::from(w) * u64::from(h);
        let mut scale = 1.0f32;
        if w.max(h) < OCR_UPSCALE_BELOW_PX {
            scale = OCR_UPSCALE_FACTOR as f32;
        }
        let scaled_pixels = pixels as f64 * f64::from(scale) * f64::from(scale);
        if scaled_pixels > OCR_MAX_PIXELS as f64 {
            scale = (OCR_MAX_PIXELS as f64 / pixels as f64).sqrt() as f32;
        }
        let sw = ((w as f32 * scale).round() as u32).max(1);
        let sh = ((h as f32 * scale).round() as u32).max(1);
        let scaled = if sw == w && sh == h {
            img.clone()
        } else {
            image::imageops::resize(img, sw, sh, FilterType::CatmullRom)
        };
        // Largeur multiple de 4, complétée en blanc (exigence de la cale).
        let pw = sw.div_ceil(4) * 4;
        let mut samples = vec![255u8; pw as usize * sh as usize];
        for (y, row) in scaled.rows().enumerate() {
            let start = y * pw as usize;
            if let Some(dst) = samples.get_mut(start..start + sw as usize) {
                for (d, p) in dst.iter_mut().zip(row) {
                    *d = p.0[0];
                }
            }
        }
        let eff_dpi = ((dpi as f32) * scale).round().max(1.0) as u32;
        let raw = ffi::ocr_gray(&samples, pw, sh, eff_dpi, OCR_LANGUAGES, &self.datadir, "")
            .map_err(|_| ocr_error())?;
        let inv = 1.0 / scale;
        let items = raw
            .into_iter()
            .filter(|r| !r.text.trim().is_empty())
            .map(|r| OcrItem {
                text: r.text.trim().to_string(),
                x0: (r.left as f32 * inv).clamp(0.0, w as f32),
                y0: (r.top as f32 * inv).clamp(0.0, h as f32),
                x1: (r.right as f32 * inv).clamp(0.0, w as f32),
                y1: (r.bottom as f32 * inv).clamp(0.0, h as f32),
                confidence: (r.confidence / 100.0).clamp(0.0, 1.0),
                para: r.block.saturating_mul(10_000).saturating_add(r.para),
                line: r.line,
            })
            .collect();
        Ok(OcrResult {
            width: w,
            height: h,
            items,
        })
    }

    #[cfg(test)]
    pub fn datadir(&self) -> &std::path::Path {
        self._dir.path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn donnees_tesseract_supprimees_a_la_fin() {
        let s = OcrSession::begin().unwrap();
        let dir = s.datadir().to_path_buf();
        assert!(dir.join("fra.traineddata").is_file());
        assert!(dir.join("eng.traineddata").is_file());
        drop(s);
        assert!(!dir.exists(), "dossier des données de langue non supprimé");
    }

    #[test]
    fn image_blanche_sans_mot() {
        let s = OcrSession::begin().unwrap();
        let img = GrayImage::from_pixel(201, 77, image::Luma([255]));
        let r = s.recognize(&img, 300).unwrap();
        assert!(r.items.is_empty());
        assert_eq!((r.width, r.height), (201, 77));
    }

    fn item(text: &str, para: i32, line: i32) -> OcrItem {
        OcrItem {
            text: text.into(),
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
            confidence: 0.5,
            para,
            line,
        }
    }

    #[test]
    fn paragraphes_et_intervalles() {
        let r = OcrResult {
            width: 10,
            height: 10,
            items: vec![item("Jean", 0, 0), item("Dupont", 0, 1), item("Lyon", 1, 2)],
        };
        let p = r.paragraphs();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].text, "Jean Dupont");
        assert_eq!(&p[0].text[p[0].words[1].0.clone()], "Dupont");
        assert_eq!(p[1].words[0].1, 2);
        assert!((r.mean_confidence() - 0.5).abs() < 1e-6);
        assert_eq!(r.to_page().words.len(), 3);
    }
}
