//! PDF, images et OCR (MuPDF + Tesseract embarqués).
//!
//! - [`PdfHandler`] : PDF texte et scannés, caviardage réel et balises.
//! - [`ImageHandler`] : images isolées (OCR, masquage).
//! - [`MupdfImageProcessor`] : OCR et masquage pour les autres crates.
//!
//! Seul crate autorisé à utiliser `unsafe`, confiné dans le module `ffi`.
//! L'OCR se fait entièrement en mémoire ; seules les données de langue de
//! Tesseract (pas de contenu utilisateur) sont écrites dans un dossier privé
//! temporaire, supprimé à la fin de chaque traitement.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

mod ffi;
mod image_handler;
mod image_proc;
mod ocr;
mod pdf;

use anon_core::Handler;

pub use image_handler::{ImageHandler, OCR_MASK_MARGIN_PX, OCR_MASK_MARGIN_RATIO};
pub use image_proc::{
    JPEG_QUALITY, MAX_DECODE_ALLOC, MAX_IMAGE_PIXELS, MAX_IMAGE_SIDE, MIN_LABEL_PX,
    MupdfImageProcessor,
};
pub use ocr::{DEFAULT_IMAGE_DPI, LOW_OCR_CONFIDENCE, OCR_LANGUAGES};
pub use pdf::{
    DEEP_SCAN_MIN_IMAGE_SIDE_PT, MAX_PDF_BYTES, MAX_PDF_PAGES, MIN_TAG_FONT_SIZE,
    OCR_IMAGE_MIN_PAGE_FRACTION, OCR_RENDER_DPI, PdfHandler, SCANNED_IMAGE_COVERAGE,
    decompressed_bytes,
};

static PDF: PdfHandler = PdfHandler;
static IMAGE: ImageHandler = ImageHandler;

/// Gestionnaire de l'extension donnée (sans point, casse indifférente).
pub fn handler_for(extension: &str) -> Option<&'static dyn Handler> {
    let ext = extension.trim_start_matches('.').to_ascii_lowercase();
    if pdf::PDF_EXTENSIONS.contains(&ext.as_str()) {
        return Some(&PDF);
    }
    if image_handler::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Some(&IMAGE);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_lookup() {
        assert!(handler_for("pdf").is_some());
        assert!(handler_for(".PDF").is_some());
        for e in ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"] {
            assert!(handler_for(e).is_some(), "{e}");
        }
        assert!(handler_for("docx").is_none());
    }

    #[test]
    fn handlers_are_sync() {
        fn is_sync<T: Sync>() {}
        is_sync::<PdfHandler>();
        is_sync::<ImageHandler>();
        is_sync::<MupdfImageProcessor>();
    }
}
