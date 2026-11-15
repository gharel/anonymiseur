//! Session ONNX Runtime de YuNet (fonctionnalité `ia`).

use super::FaceError;
use super::post::{self, Candidate, Head};
use super::prep::Planes;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::{Session, SessionInputValue};
use ort::value::{Tensor, TensorElementType, ValueType};
use std::borrow::Cow;
use std::sync::Mutex;

/// Têtes de YuNet : pas et noms des sorties `cls`, `obj`, `bbox`, comme dans la
/// référence OpenCV (les sorties `kps_*`, points du visage, ne servent pas ici).
const HEADS: [(usize, &str, &str, &str); 3] = [
    (8, "cls_8", "obj_8", "bbox_8"),
    (16, "cls_16", "obj_16", "bbox_16"),
    (32, "cls_32", "obj_32", "bbox_32"),
];
/// Rang de l'entrée attendue : `1 × 3 × hauteur × largeur`.
const INPUT_RANK: usize = 4;

fn rt(e: ort::Error) -> FaceError {
    FaceError::Runtime(e.to_string())
}

/// Modèle chargé. `Session::run` demande `&mut` : l'inférence est sérialisée.
pub(crate) struct YuNet {
    session: Mutex<Session>,
    input: String,
}

impl std::fmt::Debug for YuNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuNet")
            .field("input", &self.input)
            .finish_non_exhaustive()
    }
}

impl YuNet {
    pub(crate) fn load(onnx: &[u8]) -> Result<Self, FaceError> {
        let env = crate::model::environment().map_err(|e| FaceError::Runtime(e.to_string()))?;
        let session = Session::builder(env)
            .map_err(rt)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| rt(e.into()))?
            .commit_from_memory(onnx)
            .map_err(rt)?;
        let [input] = session.inputs() else {
            return Err(FaceError::Model("une seule entrée attendue"));
        };
        let float_image = matches!(
            input.dtype(),
            ValueType::Tensor { ty: TensorElementType::Float32, shape, .. } if shape.len() == INPUT_RANK
        );
        if !float_image {
            return Err(FaceError::Model(
                "entrée attendue : image 1 × 3 × H × W en f32",
            ));
        }
        let outputs: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
        if HEADS
            .iter()
            .flat_map(|&(_, cls, obj, bbox)| [cls, obj, bbox])
            .any(|n| !outputs.contains(&n))
        {
            return Err(FaceError::Model("sorties cls, obj ou bbox absentes"));
        }
        let input = input.name().to_string();
        Ok(Self {
            session: Mutex::new(session),
            input,
        })
    }

    /// Visages candidats d'un niveau, en coordonnées de l'image d'origine
    /// (`scale` : facteurs du niveau vers l'origine), ajoutés à `out`.
    pub(crate) fn run(
        &self,
        level: Planes,
        score_threshold: f32,
        scale: (f32, f32),
        out: &mut Vec<Candidate>,
    ) -> Result<(), FaceError> {
        let (pad_w, pad_h) = (level.pad_w, level.pad_h);
        let tensor = Tensor::from_array(([1usize, 3, pad_h, pad_w], level.data)).map_err(rt)?;
        let feeds: Vec<(Cow<'_, str>, SessionInputValue<'_>)> =
            vec![(Cow::Borrowed(self.input.as_str()), tensor.into())];
        let mut session = self.session.lock().map_err(|_| FaceError::Poisoned)?;
        let outputs = session.run(feeds).map_err(rt)?;
        let extract = |name: &str| -> Result<&[f32], FaceError> {
            let value = outputs
                .get(name)
                .ok_or(FaceError::Model("sortie du modèle absente"))?;
            value
                .try_extract_tensor::<f32>()
                .map(|(_, data)| data)
                .map_err(rt)
        };
        let mut heads = Vec::with_capacity(HEADS.len());
        for (stride, cls, obj, bbox) in HEADS {
            heads.push(Head {
                stride,
                cls: extract(cls)?,
                obj: extract(obj)?,
                bbox: extract(bbox)?,
            });
        }
        post::decode(&heads, pad_w, pad_h, score_threshold, scale, out)
            .map_err(|_| FaceError::Model("dimensions de sortie inattendues"))
    }
}
