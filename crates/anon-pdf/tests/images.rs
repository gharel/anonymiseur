#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Images : masquage vérifié par ré-OCR, format conservé, EXIF absent.

mod common;

use std::io::Cursor;

use anon_core::{Handler, ImageProcessor, MaskRegion, PixelRect, TransformOptions};
use anon_pdf::{ImageHandler, MupdfImageProcessor};
use common::*;
use image::{DynamicImage, ImageFormat};

fn reencode(png: &[u8], format: ImageFormat) -> Vec<u8> {
    let img = image::load_from_memory(png).unwrap();
    let img = if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(img.to_rgb8())
    } else {
        DynamicImage::ImageRgba8(img.to_rgba8())
    };
    let img = if format == ImageFormat::WebP || format == ImageFormat::Gif {
        img
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    };
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format).unwrap();
    out.into_inner()
}

#[test]
fn masquage_tous_formats() {
    let png = exemple("carte-visite.png");
    let values = expected("carte-visite.png");
    let (s, _) = session(&values);
    let h = ImageHandler;
    let p = MupdfImageProcessor;
    for format in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Bmp,
        ImageFormat::Tiff,
        ImageFormat::WebP,
    ] {
        let input = if format == ImageFormat::Png {
            png.clone()
        } else {
            reencode(&png, format)
        };
        let before = joined(&h.extract(&input).unwrap().blocks);
        let (read, missed) = found(&values, &before);
        println!(
            "{format:?} : OCR {}/{} manquées {missed:?}",
            read.len(),
            values.len()
        );
        assert!(!read.is_empty(), "{format:?} : rien lu");
        let out = h
            .transform(&input, &replacer(&s), &TransformOptions::anonymize())
            .unwrap();
        assert_eq!(
            image::guess_format(&out.bytes).unwrap(),
            format,
            "format conservé"
        );
        let page = p.ocr(&out.bytes).unwrap();
        let (after, _) = page.text_with_spans();
        for (_, v) in &read {
            assert!(
                !norm(&after).contains(&norm(v)),
                "{format:?} : « {v} » encore lu"
            );
        }
    }
}

/// JPEG avec un segment EXIF (Artist = « Jean Dupont »).
fn jpeg_with_exif() -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        64,
        48,
        image::Rgb([200, 30, 30]),
    ));
    let mut jpeg = Cursor::new(Vec::new());
    img.write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
    let jpeg = jpeg.into_inner();
    let artist = b"Jean Dupont\0";
    // TIFF petit-boutiste : en-tête, 1 entrée IFD (Artist, ASCII), valeur après l'IFD.
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x013Bu16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&(artist.len() as u32).to_le_bytes());
    tiff.extend_from_slice(&26u32.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(artist);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

#[test]
fn exif_supprime() {
    let input = jpeg_with_exif();
    assert!(contains_bytes(&input, b"Exif"));
    assert!(contains_bytes(&input, b"Jean Dupont"));
    image::load_from_memory(&input).expect("JPEG de test valide");

    let p = MupdfImageProcessor;
    let masked = p
        .mask(
            &input,
            &[MaskRegion {
                rect: PixelRect {
                    x: 4,
                    y: 4,
                    w: 40,
                    h: 20,
                },
                label: Some("[PERSONNE_1]".into()),
            }],
        )
        .unwrap();
    assert_eq!(image::guess_format(&masked).unwrap(), ImageFormat::Jpeg);
    assert!(!contains_bytes(&masked, b"Exif"));
    assert!(!contains_bytes(&masked, b"Jean Dupont"));
    // Aplat effectivement posé.
    let img = image::load_from_memory(&masked).unwrap().to_rgb8();
    let px = img.get_pixel(30, 6).0;
    assert!(px[1] > 150, "aplat clair attendu, trouvé {px:?}");

    // Anonymisation sans aucune détection : l'EXIF part quand même.
    let (s, _) = session(&[]);
    let out = ImageHandler
        .transform(&input, &replacer(&s), &TransformOptions::anonymize())
        .unwrap();
    assert!(!contains_bytes(&out.bytes, b"Exif"));
    assert!(!contains_bytes(&out.bytes, b"Jean Dupont"));
}

#[test]
fn balise_ecrite_si_elle_tient() {
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        600,
        200,
        image::Rgb([255, 255, 255]),
    ));
    let mut png = Cursor::new(Vec::new());
    img.write_to(&mut png, ImageFormat::Png).unwrap();
    let png = png.into_inner();
    let p = MupdfImageProcessor;
    let out = p
        .mask(
            &png,
            &[
                MaskRegion {
                    rect: PixelRect {
                        x: 20,
                        y: 20,
                        w: 560,
                        h: 80,
                    },
                    label: Some("[PERSONNE_1]".into()),
                },
                // Trop petite : aplat seul.
                MaskRegion {
                    rect: PixelRect {
                        x: 20,
                        y: 150,
                        w: 10,
                        h: 5,
                    },
                    label: Some("[PERSONNE_2]".into()),
                },
            ],
        )
        .unwrap();
    let page = p.ocr(&out).unwrap();
    let (text, _) = page.text_with_spans();
    println!("balise relue : {text}");
    assert!(text.contains("PERSONNE"), "balise illisible : {text}");
    assert!(!text.contains("_2"));
}
