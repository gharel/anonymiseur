//! Détection des visages sur des images de test du domaine public (sources et licences :
//! `tests/fixtures/SOURCES.md`). Le modèle est versionné dans `assets/faces/` : ces
//! tests ne dépendent d'aucun téléchargement.
#![cfg(feature = "ia")]
// Fichier de test : les assistants hors `#[test]` peuvent paniquer ; les mesures
// affichent leur détail avec `--nocapture`.
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use anon_core::format::PixelRect;
use anon_ner::faces::{FaceDetector, FaceError};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Part minimale de la boîte annotée à la main couverte par le rectangle trouvé.
const MIN_COVERAGE: f64 = 0.95;
/// Recouvrement minimal (IoU) entre le rectangle trouvé, marges comprises, et la
/// boîte annotée : le masque reste centré sur le visage.
const MIN_IOU: f64 = 0.3;
/// Temps maximal de détection d'une image de 1 000 pixels de haut, en release.
const MAX_DETECT_TIME: Duration = Duration::from_millis(100);
/// Nombre de mesures du temps de détection (médiane).
const TIMING_RUNS: usize = 9;

/// Boîtes annotées à la main (front, joues, menton ; sans les cheveux).
const MONA_LISA_FACE: PixelRect = PixelRect {
    x: 242,
    y: 137,
    w: 153,
    h: 218,
};
const PEARL_FACE: PixelRect = PixelRect {
    x: 211,
    y: 227,
    w: 200,
    h: 245,
};

fn detector() -> &'static FaceDetector {
    static D: OnceLock<FaceDetector> = OnceLock::new();
    D.get_or_init(|| FaceDetector::embedded().unwrap_or_else(|e| panic!("modèle : {e}")))
}

struct Image {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl Image {
    fn from_rgba(img: image::RgbaImage) -> Self {
        let (width, height) = img.dimensions();
        Self {
            rgba: img.into_raw(),
            width,
            height,
        }
    }

    fn rgba(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone()).unwrap()
    }

    fn detect(&self) -> Vec<PixelRect> {
        detector()
            .detect(&self.rgba, self.width, self.height)
            .unwrap_or_else(|e| panic!("détection : {e}"))
    }
}

fn fixture(name: &str) -> Image {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let img = image::open(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()));
    Image::from_rgba(img.to_rgba8())
}

fn area(r: &PixelRect) -> f64 {
    f64::from(r.w) * f64::from(r.h)
}

fn intersection(a: &PixelRect, b: &PixelRect) -> f64 {
    let w = (a.x + a.w).min(b.x + b.w).saturating_sub(a.x.max(b.x));
    let h = (a.y + a.h).min(b.y + b.h).saturating_sub(a.y.max(b.y));
    f64::from(w) * f64::from(h)
}

fn shift(r: PixelRect, dx: u32, dy: u32) -> PixelRect {
    PixelRect {
        x: r.x + dx,
        y: r.y + dy,
        ..r
    }
}

/// Un seul visage, qui couvre la boîte annotée et reste centré dessus.
fn assert_one_face(found: &[PixelRect], truth: PixelRect, what: &str) {
    assert_eq!(found.len(), 1, "{what} : {found:?}");
    let r = &found[0];
    let inter = intersection(r, &truth);
    let coverage = inter / area(&truth);
    let iou = inter / (area(r) + area(&truth) - inter);
    println!("{what} : {r:?}, couverture {coverage:.3}, IoU {iou:.3}");
    assert!(
        coverage >= MIN_COVERAGE,
        "{what} : couverture {coverage:.3} < {MIN_COVERAGE} ({r:?} contre {truth:?})"
    );
    assert!(
        iou >= MIN_IOU,
        "{what} : IoU {iou:.3} < {MIN_IOU} ({r:?} contre {truth:?})"
    );
}

#[test]
fn finds_mona_lisa_face_where_annotated() {
    let img = fixture("portrait_mona_lisa.jpg");
    assert_one_face(&img.detect(), MONA_LISA_FACE, "Joconde");
}

#[test]
fn finds_turned_face_where_annotated() {
    let img = fixture("portrait_jeune_fille_perle.jpg");
    assert_one_face(&img.detect(), PEARL_FACE, "Jeune Fille à la perle");
}

#[test]
fn no_face_on_landscape_or_text_page() {
    for name in ["paysage_nuit_etoilee.jpg", "page_texte.png"] {
        let found = fixture(name).detect();
        assert!(found.is_empty(), "{name} : {found:?}");
    }
}

/// Grande image (12 mégapixels) où le visage n'occupe que 4 % de la largeur : le
/// niveau de 1 280 pixels le trouve.
#[test]
fn small_face_in_a_large_image() {
    let portrait = fixture("portrait_mona_lisa.jpg").rgba();
    let (dx, dy) = (2500, 1500);
    let mut canvas = image::RgbaImage::from_pixel(4000, 3000, image::Rgba([255; 4]));
    image::imageops::overlay(&mut canvas, &portrait, i64::from(dx), i64::from(dy));
    let img = Image::from_rgba(canvas);
    let t = Instant::now();
    let found = img.detect();
    println!("détection 4 000 × 3 000 : {:?}", t.elapsed());
    assert_one_face(&found, shift(MONA_LISA_FACE, dx, dy), "grande image");
}

/// Visage qui remplit l'image (photo d'identité agrandie) : trop grand pour le modèle à
/// pleine taille, trouvé au niveau de 320 pixels.
#[test]
fn face_filling_the_image() {
    let portrait = fixture("portrait_mona_lisa.jpg").rgba();
    let (x0, y0, w, h) = (200, 100, 240, 300);
    let crop = image::imageops::crop_imm(&portrait, x0, y0, w, h).to_image();
    let factor = 4;
    let big = image::imageops::resize(
        &crop,
        w * factor,
        h * factor,
        image::imageops::FilterType::Triangle,
    );
    let img = Image::from_rgba(big);
    let truth = PixelRect {
        x: (MONA_LISA_FACE.x - x0) * factor,
        y: (MONA_LISA_FACE.y - y0) * factor,
        w: MONA_LISA_FACE.w * factor,
        h: MONA_LISA_FACE.h * factor,
    };
    assert_one_face(&img.detect(), truth, "visage pleine image");
}

/// Images minuscules ou de proportions extrêmes : un résultat, jamais de panique.
#[test]
fn tiny_and_extreme_images() {
    let portrait = fixture("portrait_mona_lisa.jpg").rgba();
    let thumb = image::imageops::thumbnail(&portrait, 67, 100);
    let found = Image::from_rgba(thumb).detect();
    println!("vignette 67 × 100 : {found:?}");
    for f in &found {
        assert!(f.x + f.w <= 67 && f.y + f.h <= 100, "{f:?}");
    }
    for (w, h) in [(1, 1), (2, 1), (1, 5000), (5000, 1), (31, 33)] {
        let rgba = vec![128u8; (w * h * 4) as usize];
        let found = detector().detect(&rgba, w, h).unwrap();
        assert!(found.is_empty(), "{w} × {h} : {found:?}");
    }
}

/// Pixels quelconques (tampon de la bonne taille, contenu aléatoire, transparence
/// partielle) : un résultat sans panique.
#[test]
fn noise_and_flat_images_do_not_panic() {
    let (w, h) = (300u32, 200u32);
    let mut state: u32 = 0x2545_f491;
    let noise: Vec<u8> = (0..w * h * 4)
        .map(|_| {
            // Générateur congruentiel : reproductible, sans dépendance.
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    let found = detector().detect(&noise, w, h).unwrap();
    println!("bruit : {} visage(s)", found.len());
    for flat in [0u8, 255] {
        let rgba = vec![flat; (w * h * 4) as usize];
        assert!(detector().detect(&rgba, w, h).unwrap().is_empty());
    }
}

#[test]
fn empty_or_inconsistent_buffers_are_errors() {
    let d = detector();
    assert_eq!(
        d.detect(&[], 0, 0),
        Err(FaceError::EmptyImage {
            width: 0,
            height: 0
        })
    );
    assert!(matches!(
        d.detect(&[0; 4], 1, 0),
        Err(FaceError::EmptyImage { .. })
    ));
    let img = fixture("portrait_mona_lisa.jpg");
    let truncated = &img.rgba[..img.rgba.len() - 1];
    assert!(matches!(
        d.detect(truncated, img.width, img.height),
        Err(FaceError::BufferSize { .. })
    ));
    assert!(matches!(
        d.detect(&img.rgba, img.width + 1, img.height),
        Err(FaceError::BufferSize { .. })
    ));
    // Dimensions dont le produit déborde : erreur, pas de dépassement.
    assert!(matches!(
        d.detect(&[0; 16], u32::MAX, u32::MAX),
        Err(FaceError::BufferSize { .. })
    ));
    // Les messages ne contiennent que des dimensions.
    let msg = d.detect(truncated, 671, 1000).unwrap_err().to_string();
    assert!(msg.contains("671") && msg.contains("1000"), "{msg}");
}

#[test]
fn corrupted_model_is_an_error() {
    assert!(FaceDetector::from_bytes(&[]).is_err());
    assert!(FaceDetector::from_bytes(b"pas un modele onnx").is_err());
    let model = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/faces")
            .join(anon_ner::faces::MODEL_FILE),
    )
    .unwrap();
    assert!(FaceDetector::from_bytes(&model[..model.len() / 2]).is_err());
    assert!(FaceDetector::from_bytes(&model).is_ok());
}

#[test]
fn detector_is_shareable_and_debug_is_short() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<FaceDetector>();
    assert!(FaceDetector::is_available());
    let debug = format!("{:?}", detector());
    assert!(debug.len() < 200, "{debug}");
}

/// Temps de détection d'une image de 671 × 1 000 pixels (deux niveaux : 671 × 1 000 et
/// 215 × 320). La borne n'est vérifiée qu'en release : en debug, la préparation de
/// l'image n'est pas optimisée.
#[test]
fn detection_time_for_a_1000_px_image() {
    let img = fixture("portrait_mona_lisa.jpg");
    img.detect();
    let mut times: Vec<Duration> = (0..TIMING_RUNS)
        .map(|_| {
            let t = Instant::now();
            img.detect();
            t.elapsed()
        })
        .collect();
    times.sort();
    let median = times[TIMING_RUNS / 2];
    println!(
        "détection {} × {} : médiane {median:?}, min {:?}, max {:?}",
        img.width,
        img.height,
        times[0],
        times[TIMING_RUNS - 1]
    );
    if !cfg!(debug_assertions) {
        assert!(
            median < MAX_DETECT_TIME,
            "médiane {median:?} ≥ {MAX_DETECT_TIME:?}"
        );
    }
}
