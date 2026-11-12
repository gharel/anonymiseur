//! Images : masquage à l'anonymisation, remise des originaux à la restauration.

use anon_core::{
    BlockContext, BlockRole, FormatError, ImageProcessor, MaskRegion, MediaOriginal,
    MediaTransform, OcrPage, PixelRect, Session, TextTransform, Warning, WarningCode,
};
use anon_replace::Replacer;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// Marge ajoutée autour de chaque mot masqué, en pixels.
pub const MASK_MARGIN_PX: u32 = 2;
/// Confiance moyenne d'OCR en dessous de laquelle l'image est signalée.
pub const LOW_OCR_CONFIDENCE: f32 = 0.6;

/// Zone à masquer saisie par l'utilisateur sur une image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualMask {
    /// Nom de l'image, tel que donné par `FileAnalysis::images`.
    pub media: String,
    pub rect: PixelRect,
}

/// SHA-256 en hexadécimal minuscule.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn widen(rect: PixelRect, page: &OcrPage) -> PixelRect {
    let x = rect.x.saturating_sub(MASK_MARGIN_PX);
    let y = rect.y.saturating_sub(MASK_MARGIN_PX);
    let mut right = rect.x.saturating_add(rect.w).saturating_add(MASK_MARGIN_PX);
    let mut bottom = rect.y.saturating_add(rect.h).saturating_add(MASK_MARGIN_PX);
    if page.width > 0 {
        right = right.min(page.width);
    }
    if page.height > 0 {
        bottom = bottom.min(page.height);
    }
    PixelRect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

/// Zones à masquer : les mots touchés par un remplacement sur le texte OCR.
/// La balise est écrite sur le premier mot de chaque remplacement.
pub(crate) fn regions_for(page: &OcrPage, replacer: &Replacer) -> Vec<MaskRegion> {
    let (text, spans) = page.text_with_spans();
    let ctx = BlockContext::new("image", BlockRole::Ocr);
    let mut out = Vec::new();
    for r in replacer.replacements(&text, &ctx) {
        let mut first = true;
        for (i, s) in spans.iter().enumerate() {
            if s.start < r.end
                && r.start < s.end
                && let Some(w) = page.words.get(i)
            {
                out.push(MaskRegion {
                    rect: widen(w.rect, page),
                    label: first.then(|| r.with.clone()),
                });
                first = false;
            }
        }
    }
    out
}

#[derive(Default)]
struct MaskState {
    originals: Vec<MediaOriginal>,
    error: Option<FormatError>,
    warnings: Vec<Warning>,
    masked_regions: usize,
}

/// `MediaTransform` d'anonymisation : OCR (repris de l'analyse si possible),
/// zones calculées, masquage, original gardé.
pub(crate) struct Masker<'a> {
    processor: &'a dyn ImageProcessor,
    replacer: &'a Replacer,
    ocr: HashMap<&'a str, &'a OcrPage>,
    manual: &'a [ManualMask],
    keep_originals: bool,
    state: Mutex<MaskState>,
}

impl<'a> Masker<'a> {
    pub fn new(
        processor: &'a dyn ImageProcessor,
        replacer: &'a Replacer,
        ocr: HashMap<&'a str, &'a OcrPage>,
        manual: &'a [ManualMask],
        keep_originals: bool,
    ) -> Self {
        Self {
            processor,
            replacer,
            ocr,
            manual,
            keep_originals,
            state: Mutex::new(MaskState::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, MaskState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Erreur de masquage survenue pendant la transformation.
    pub fn take_error(&self) -> Option<FormatError> {
        self.state().error.take()
    }

    pub fn finish(self) -> (Vec<MediaOriginal>, Vec<Warning>, usize) {
        let st = self.state.into_inner().unwrap_or_else(|p| p.into_inner());
        (st.originals, st.warnings, st.masked_regions)
    }
}

impl MediaTransform for Masker<'_> {
    fn apply(&self, name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        let sha = sha256_hex(bytes);
        let page: Cow<'_, OcrPage> = match self.ocr.get(sha.as_str()) {
            Some(p) => Cow::Borrowed(*p),
            None => match self.processor.ocr(bytes) {
                Ok(p) => Cow::Owned(p),
                Err(_) => {
                    self.state().warnings.push(Warning::new(
                        WarningCode::ImageNotProcessed,
                        name,
                        "Image illisible par l'OCR, laissée telle quelle : vérifiez-la à l'œil.",
                    ));
                    // Les zones manuelles s'appliquent quand même.
                    Cow::Owned(OcrPage::default())
                }
            },
        };
        let mut regions = regions_for(&page, self.replacer);
        regions.extend(
            self.manual
                .iter()
                .filter(|m| m.media == name)
                .map(|m| MaskRegion {
                    rect: m.rect,
                    label: None,
                }),
        );
        if regions.is_empty() {
            return None;
        }
        match self.processor.mask(bytes, &regions) {
            Ok(masked) => {
                let mut st = self.state();
                st.masked_regions = st.masked_regions.saturating_add(regions.len());
                if self.keep_originals {
                    st.originals.push(MediaOriginal {
                        masked_sha256: sha256_hex(&masked),
                        original: bytes.to_vec(),
                    });
                }
                Some(masked)
            }
            Err(e) => {
                let mut st = self.state();
                if st.error.is_none() {
                    st.error = Some(e);
                }
                None
            }
        }
    }
}

/// `MediaTransform` de restauration : remet l'original d'une image masquée.
pub(crate) struct Unmasker<'a> {
    by_sha: HashMap<&'a str, &'a [u8]>,
}

impl<'a> Unmasker<'a> {
    pub fn new(session: &'a Session) -> Self {
        Self {
            by_sha: session
                .media
                .iter()
                .map(|m| (m.masked_sha256.as_str(), m.original.as_slice()))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_sha.is_empty()
    }
}

impl MediaTransform for Unmasker<'_> {
    fn apply(&self, _name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        self.by_sha
            .get(sha256_hex(bytes).as_str())
            .map(|o| o.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn widen_is_clamped() {
        let page = OcrPage {
            width: 10,
            height: 10,
            words: Vec::new(),
        };
        let r = widen(
            PixelRect {
                x: 1,
                y: 0,
                w: 8,
                h: 9,
            },
            &page,
        );
        assert_eq!(
            r,
            PixelRect {
                x: 0,
                y: 0,
                w: 10,
                h: 10
            }
        );
    }
}
