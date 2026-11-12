//! Gestionnaire des images isolées (PNG, JPEG, GIF, BMP, TIFF, WebP).

use anon_core::{
    BlockContext, BlockRole, Direction, Extraction, FormatError, Handler, MaskRegion, Output,
    PixelRect, TextBlock, TextTransform, TransformOptions, Warning, WarningCode,
};

use crate::image_proc::{decode, encode, ocr_decoded, paint_regions};
use crate::ocr::{LOW_OCR_CONFIDENCE, OcrResult, OcrSession};

/// Emplacement des blocs d'une image isolée.
pub const IMAGE_PART: &str = "image";
/// Marge minimale (pixels) autour d'un mot OCR masqué : les boîtes Tesseract sont justes.
pub const OCR_MASK_MARGIN_PX: f32 = 3.0;
/// Marge relative à la hauteur du mot.
pub const OCR_MASK_MARGIN_RATIO: f32 = 0.15;

/// Zone à masquer issue de l'OCR : un rectangle par ligne, balise sur le plus large.
#[derive(Debug, Clone)]
pub(crate) struct OcrHit {
    /// Rectangles `[x0, y0, x1, y1]`, un par ligne, dans le repère de l'OCR.
    pub rects: Vec<[f32; 4]>,
    pub label: String,
}

impl OcrHit {
    /// Indice du rectangle le plus large (où écrire la balise).
    pub fn widest(&self) -> Option<usize> {
        self.rects
            .iter()
            .enumerate()
            .max_by(|a, b| (a.1[2] - a.1[0]).total_cmp(&(b.1[2] - b.1[0])))
            .map(|(i, _)| i)
    }
}

/// Blocs de texte d'un résultat d'OCR, un par paragraphe.
pub(crate) fn ocr_blocks(result: &OcrResult, part: &str) -> Vec<TextBlock> {
    result
        .paragraphs()
        .into_iter()
        .filter(|p| !p.text.trim().is_empty())
        .map(|p| TextBlock {
            text: p.text,
            ctx: BlockContext::new(part, BlockRole::Ocr),
        })
        .collect()
}

/// Remplacements demandés par `tr` sur les paragraphes OCR, convertis en zones.
pub(crate) fn ocr_hits(result: &OcrResult, tr: &dyn TextTransform, part: &str) -> Vec<OcrHit> {
    let ctx = BlockContext::new(part, BlockRole::Ocr);
    let mut hits = Vec::new();
    for para in result.paragraphs() {
        for rep in tr.replacements(&para.text, &ctx) {
            let mut rects: Vec<(i32, [f32; 4])> = Vec::new();
            for (span, idx) in &para.words {
                if span.start >= rep.end || span.end <= rep.start {
                    continue;
                }
                let Some(w) = result.items.get(*idx) else {
                    continue;
                };
                let r = [w.x0, w.y0, w.x1, w.y1];
                match rects.last_mut() {
                    Some((line, acc)) if *line == w.line => {
                        acc[0] = acc[0].min(r[0]);
                        acc[1] = acc[1].min(r[1]);
                        acc[2] = acc[2].max(r[2]);
                        acc[3] = acc[3].max(r[3]);
                    }
                    _ => rects.push((w.line, r)),
                }
            }
            if !rects.is_empty() {
                hits.push(OcrHit {
                    rects: rects.into_iter().map(|(_, r)| r).collect(),
                    label: rep.with,
                });
            }
        }
    }
    hits
}

fn low_confidence_warning(result: &OcrResult, part: &str) -> Option<Warning> {
    (!result.items.is_empty() && result.mean_confidence() < LOW_OCR_CONFIDENCE).then(|| {
        Warning::new(
            WarningCode::LowOcrConfidence,
            part,
            "Texte lu par OCR avec une confiance faible : vérifiez le résultat à l'œil.",
        )
    })
}

pub(crate) fn hits_to_regions(hits: &[OcrHit], width: u32, height: u32) -> Vec<MaskRegion> {
    let mut regions = Vec::new();
    for hit in hits {
        let widest = hit.widest();
        for (i, r) in hit.rects.iter().enumerate() {
            let h = (r[3] - r[1]).max(0.0);
            let m = (h * OCR_MASK_MARGIN_RATIO).max(OCR_MASK_MARGIN_PX);
            let x0 = (r[0] - m).max(0.0).floor() as u32;
            let y0 = (r[1] - m).max(0.0).floor() as u32;
            let x1 = ((r[2] + m).max(0.0).ceil() as u32).min(width);
            let y1 = ((r[3] + m).max(0.0).ceil() as u32).min(height);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            regions.push(MaskRegion {
                rect: PixelRect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                },
                label: (Some(i) == widest).then(|| hit.label.clone()),
            });
        }
    }
    regions
}

/// Images isolées : OCR à l'extraction, masquage des zones à l'anonymisation.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImageHandler;

pub(crate) const IMAGE_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"];

impl Handler for ImageHandler {
    fn extensions(&self) -> &'static [&'static str] {
        IMAGE_EXTENSIONS
    }

    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        let decoded = decode(input)?;
        let session = OcrSession::begin()?;
        let result = ocr_decoded(&session, &decoded.image)?;
        drop(session);
        let mut warnings = Vec::new();
        warnings.extend(low_confidence_warning(&result, IMAGE_PART));
        Ok(Extraction {
            blocks: ocr_blocks(&result, IMAGE_PART),
            media: Vec::new(),
            warnings,
        })
    }

    fn transform(
        &self,
        input: &[u8],
        tr: &dyn TextTransform,
        opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        match opts.direction {
            Direction::Restore => {
                let bytes = opts
                    .media
                    .and_then(|m| m.apply(IMAGE_PART, input))
                    .unwrap_or_else(|| input.to_vec());
                Ok(Output {
                    bytes,
                    replacements: 0,
                    warnings: Vec::new(),
                })
            }
            Direction::Anonymize => {
                let decoded = decode(input)?;
                let session = OcrSession::begin()?;
                let result = ocr_decoded(&session, &decoded.image)?;
                drop(session);
                let hits = ocr_hits(&result, tr, IMAGE_PART);
                let (w, h) = (decoded.image.width(), decoded.image.height());
                let regions = hits_to_regions(&hits, w, h);
                let mut warnings: Vec<Warning> = low_confidence_warning(&result, IMAGE_PART)
                    .into_iter()
                    .collect();
                let missing_labels = regions
                    .iter()
                    .filter_map(|r| r.label.as_deref().map(|l| (l, r.rect)))
                    .filter(|(l, r)| {
                        crate::image_proc::label_size(l, r.w as f32, r.h as f32).is_none()
                    })
                    .count();
                if missing_labels > 0 {
                    warnings.push(Warning::new(
                        WarningCode::TagDidNotFit,
                        IMAGE_PART,
                        format!("{missing_labels} balise(s) trop longue(s) pour la zone masquée : zone masquée sans balise."),
                    ));
                }
                if regions.is_empty() && !opts.scrub_metadata {
                    return Ok(Output {
                        bytes: input.to_vec(),
                        replacements: 0,
                        warnings,
                    });
                }
                // Toujours ré-encodé quand on nettoie les métadonnées : l'EXIF disparaît.
                let painted = paint_regions(&decoded.image, &regions);
                Ok(Output {
                    bytes: encode(&painted, decoded.format)?,
                    replacements: hits.len(),
                    warnings,
                })
            }
        }
    }
}
