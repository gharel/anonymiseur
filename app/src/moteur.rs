//! Le moteur tel qu'il est livré : tous les formats (bureautique, PDF, images),
//! l'OCR, et tous les détecteurs — dont l'IA de noms et la détection des
//! visages dans l'édition complète.
//!
//! C'est le seul endroit où les crates sont assemblés. L'interface et les tests
//! de bout en bout passent par [`build`] pour mesurer exactement ce qui est livré.

use anon_engine::Engine;

/// État de l'IA de noms dans cette édition de l'application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IaStatus {
    /// Modèles chargés : la détection de noms par IA est active.
    Active,
    /// Édition Lite (ou macOS Intel) : l'application est compilée sans IA.
    Absente,
    /// Édition complète, mais les modèles n'ont pas pu être chargés ; la
    /// détection continue avec les règles et les dictionnaires.
    Indisponible,
}

impl IaStatus {
    /// Phrase affichée dans l'interface.
    pub fn label(self) -> &'static str {
        match self {
            IaStatus::Active => "Détection de noms par IA : active",
            IaStatus::Absente => "Édition Lite : détection par règles et dictionnaires",
            IaStatus::Indisponible => {
                "IA de noms indisponible : détection par règles et dictionnaires"
            }
        }
    }
}

/// Moteur assemblé et état de l'IA.
#[derive(Debug)]
pub struct Moteur {
    pub engine: Engine,
    pub ia: IaStatus,
}

/// Détection des visages de l'édition complète, branchée dans le moteur.
///
/// L'image est décodée avec les mêmes plafonds et la même orientation EXIF que
/// l'OCR d'`anon-pdf` : les rectangles sont dans le repère des zones masquées.
/// Aucun pixel n'est gardé ; une image illisible donne « visages non vérifiés ».
#[cfg(feature = "ia")]
mod visages {
    use anon_core::PixelRect;
    use anon_engine::FaceFinder;
    use anon_ner::faces::FaceDetector;
    use anon_pdf::{MAX_DECODE_ALLOC, MAX_IMAGE_PIXELS, MAX_IMAGE_SIDE};
    use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
    use std::io::Cursor;

    pub(super) struct Visages(pub(super) FaceDetector);

    impl std::fmt::Debug for Visages {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Visages")
        }
    }

    /// Image décodée en RGBA 8 bits, orientée ; `None` si illisible ou trop grande.
    fn rgba(bytes: &[u8]) -> Option<image::RgbaImage> {
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .ok()?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_IMAGE_SIDE);
        limits.max_image_height = Some(MAX_IMAGE_SIDE);
        limits.max_alloc = Some(MAX_DECODE_ALLOC);
        reader.limits(limits);
        let mut decoder = reader.into_decoder().ok()?;
        let (w, h) = decoder.dimensions();
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS {
            return None;
        }
        let orientation = decoder.orientation().ok();
        let mut image = DynamicImage::from_decoder(decoder).ok()?;
        if let Some(o) = orientation {
            image.apply_orientation(o);
        }
        Some(image.to_rgba8())
    }

    impl FaceFinder for Visages {
        fn faces(&self, image: &[u8]) -> Option<Vec<PixelRect>> {
            let rgba = rgba(image)?;
            let (w, h) = rgba.dimensions();
            self.0.detect(rgba.as_raw(), w, h).ok()
        }
    }
}

/// Assemble le moteur. Le chargement des modèles d'IA prend environ une seconde :
/// à appeler hors du fil de l'interface.
pub fn build() -> Moteur {
    let builder = Engine::standard_builder()
        .handlers_from(anon_pdf::handler_for)
        .image_processor(Box::new(anon_pdf::MupdfImageProcessor::new()));

    #[cfg(feature = "ia")]
    let (builder, ia) = {
        let detector = anon_ner::NerDetector::embedded();
        let (builder, ia) = match detector.warm_up() {
            Ok(()) => (builder.detector(Box::new(detector)), IaStatus::Active),
            Err(_) => (builder, IaStatus::Indisponible),
        };
        // Modèle de visages inclus dans le binaire ; sans lui, « visages non vérifiés ».
        let builder = match anon_ner::faces::FaceDetector::embedded() {
            Ok(faces) => builder.face_finder(Box::new(visages::Visages(faces))),
            Err(_) => builder,
        };
        (builder, ia)
    };
    #[cfg(not(feature = "ia"))]
    let ia = IaStatus::Absente;

    Moteur {
        engine: builder.build(),
        ia,
    }
}

#[cfg(all(test, feature = "ia"))]
mod tests {
    use super::visages::Visages;
    use anon_engine::FaceFinder;

    #[test]
    fn visages_d_une_image_illisible_non_verifies() {
        let Ok(d) = anon_ner::faces::FaceDetector::embedded() else {
            return;
        };
        let v = Visages(d);
        assert!(v.faces(b"pas une image").is_none());
        // Image blanche : vérifiée, aucun visage.
        let mut png = Vec::new();
        let white = image::RgbaImage::from_pixel(64, 48, image::Rgba([255, 255, 255, 255]));
        let encoded = image::DynamicImage::ImageRgba8(white)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
        assert!(encoded.is_ok());
        assert_eq!(v.faces(&png), Some(Vec::new()));
    }
}
