#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Entrées tronquées, corrompues ou aléatoires : jamais de panique.

mod common;

use anon_core::{Handler, ImageProcessor, MaskRegion, PixelRect, TransformOptions};
use anon_pdf::{ImageHandler, MupdfImageProcessor, PdfHandler};
use common::*;
use proptest::prelude::*;

fn all_ops(h: &dyn Handler, bytes: &[u8]) {
    let (s, _) = session(&[(anon_core::EntityKind::Personne, "Jean Dupont".into())]);
    let _ = h.extract(bytes);
    let _ = h.deep_scan(bytes);
    let _ = h.transform(bytes, &replacer(&s), &TransformOptions::anonymize());
}

#[test]
fn pdf_tronques() {
    let input = exemple("facture.pdf");
    let mut cuts: Vec<usize> = (1..40).map(|i| input.len() * i / 40).collect();
    cuts.extend([0, 1, 5, 9, 100, input.len() - 1]);
    for cut in cuts {
        all_ops(&PdfHandler, &input[..cut]);
    }
    // Scanné tronqué (OCR sur image partielle) : extraction seulement.
    let scan = exemple("courrier-scanne.pdf");
    for i in [1, 3, 5, 7, 9] {
        let _ = PdfHandler.extract(&scan[..scan.len() * i / 10]);
    }
}

#[test]
fn pdf_octets_alteres() {
    let input = fixture("piege.pdf");
    for step in [7usize, 13, 31, 97] {
        let mut b = input.clone();
        for i in (0..b.len()).step_by(step) {
            b[i] ^= 0x5A;
        }
        all_ops(&PdfHandler, &b);
    }
}

#[test]
fn images_tronquees_et_enormes() {
    let input = exemple("carte-visite.png");
    for i in [0usize, 1, 8, 16, 33, 100, input.len() / 2, input.len() - 1] {
        let r = ImageHandler.extract(&input[..i]);
        assert!(r.is_err() || r.is_ok());
    }
    // En-tête PNG annonçant 100 000 × 100 000 pixels.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend_from_slice(&100_000u32.to_be_bytes());
    ihdr.extend_from_slice(&100_000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(&ihdr);
    png.extend_from_slice(&[0, 0, 0, 0]);
    assert!(ImageHandler.extract(&png).is_err());
    let p = MupdfImageProcessor;
    assert!(p.ocr(&png).is_err());
    assert!(
        p.mask(
            &png,
            &[MaskRegion {
                rect: PixelRect {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1
                },
                label: None
            }]
        )
        .is_err()
    );
    // Zone hors image : ignorée, pas de panique.
    let small = p
        .mask(
            &input,
            &[MaskRegion {
                rect: PixelRect {
                    x: u32::MAX - 2,
                    y: u32::MAX - 2,
                    w: u32::MAX,
                    h: 9,
                },
                label: Some("[X_1]".into()),
            }],
        )
        .unwrap();
    assert!(!small.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn pdf_aleatoire(body in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut b = b"%PDF-1.4\n".to_vec();
        b.extend_from_slice(&body);
        all_ops(&PdfHandler, &b);
        all_ops(&PdfHandler, &body);
    }

    #[test]
    fn image_aleatoire(body in proptest::collection::vec(any::<u8>(), 0..2048)) {
        for magic in [&b"\x89PNG\r\n\x1a\n"[..], b"\xFF\xD8\xFF", b"GIF89a", b"BM", b"II*\0", b"RIFF"] {
            let mut b = magic.to_vec();
            b.extend_from_slice(&body);
            let _ = ImageHandler.extract(&b);
            let _ = MupdfImageProcessor.mask(&b, &[]);
        }
    }
}
