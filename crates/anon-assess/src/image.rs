//! Évaluation des images : un visage visible permet d'isoler une personne.
//! L'édition Lite ne détecte pas les visages (`faces: None`) : l'évaluation ne
//! peut alors pas conclure.

use anon_core::{Criterion, Failure, FailureReason, Fix, ImageInput, SubjectRef, SubjectReport};

/// Évalue les images : (bilans, échecs). Une image sans visage n'est pas un sujet.
pub(crate) fn assess_images(images: &[ImageInput]) -> (Vec<SubjectReport>, Vec<Failure>) {
    let mut subjects = Vec::new();
    let mut failures = Vec::new();
    for img in images {
        let subject = SubjectRef::Image {
            file: img.file,
            media: img.media.clone(),
        };
        let (reason, fixes) = match &img.faces {
            None => (FailureReason::FacesNotChecked, Vec::new()),
            Some(faces) if faces.is_empty() => continue,
            Some(faces) => (
                FailureReason::FaceVisible,
                faces
                    .iter()
                    .map(|rect| Fix::MaskRegion {
                        file: img.file,
                        media: img.media.clone(),
                        rect: *rect,
                    })
                    .collect(),
            ),
        };
        failures.push(Failure {
            criterion: Criterion::Isolation,
            subject: subject.clone(),
            reason,
            fixes,
        });
        let mut passed = [true; 3];
        if let Some(p) = passed.get_mut(Criterion::Isolation.index()) {
            *p = false;
        }
        subjects.push(SubjectReport {
            subject,
            clues: Vec::new(),
            group_estimate: None,
            passed,
        });
    }
    (subjects, failures)
}
