//! Session ONNX Runtime et tokeniseur d'un modèle (fonctionnalité `ia`).

use crate::NerError;
use crate::aggregate::{ProbAccumulator, Span, aggregate, windows};
use crate::labels::{Label, labels_from_config};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;
use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};
use tokenizers::Tokenizer;

/// Environnement ONNX Runtime partagé, créé une seule fois, télémétrie coupée.
static ENVIRONMENT: OnceLock<Result<ort::environment::Environment, NerError>> = OnceLock::new();

fn environment() -> Result<&'static ort::environment::Environment, NerError> {
    ENVIRONMENT
        .get_or_init(|| {
            ort::init()
                .with_name("anon-ner")
                .with_telemetry(false)
                .build()
                .map_err(|e| NerError::Runtime(e.to_string()))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Un modèle chargé. `Session::run` demande `&mut` : l'inférence est sérialisée par un
/// verrou ; ONNX Runtime parallélise lui-même chaque passe sur les cœurs disponibles.
pub(crate) struct Model {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    labels: Vec<Label>,
    cls: u32,
    sep: u32,
    inputs: Vec<String>,
    output: String,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("labels", &self.labels.len())
            .field("inputs", &self.inputs)
            .finish_non_exhaustive()
    }
}

fn rt(e: ort::Error) -> NerError {
    NerError::Runtime(e.to_string())
}

impl Model {
    pub(crate) fn load(
        onnx: &[u8],
        tokenizer_json: &[u8],
        config: &[u8],
    ) -> Result<Self, NerError> {
        let labels = labels_from_config(config)?;
        let mut tokenizer = Tokenizer::from_bytes(tokenizer_json)
            .map_err(|_| NerError::Config("tokenizer.json illisible"))?;
        tokenizer
            .with_truncation(None)
            .map_err(|_| NerError::Config("tokenizer.json : troncature"))?;
        tokenizer.with_padding(None);
        let specials = tokenizer
            .encode("", true)
            .map_err(|_| NerError::Config("tokenizer.json : jetons spéciaux"))?;
        let (cls, sep) = match specials.get_ids() {
            [cls, sep] => (*cls, *sep),
            _ => {
                return Err(NerError::Config(
                    "tokenizer.json : jetons spéciaux inattendus",
                ));
            }
        };

        let session = Session::builder(environment()?)
            .map_err(rt)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| rt(e.into()))?
            .commit_from_memory(onnx)
            .map_err(rt)?;
        let inputs: Vec<String> = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        if !inputs.iter().any(|n| n == "input_ids") {
            return Err(NerError::Config("modèle sans entrée input_ids"));
        }
        let output = session
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .ok_or(NerError::Config("modèle sans sortie"))?;
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            labels,
            cls,
            sep,
            inputs,
            output,
        })
    }

    /// Entités du texte, offsets en octets.
    pub(crate) fn predict(&self, text: &str) -> Result<Vec<Span>, NerError> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let enc = self
            .tokenizer
            .encode(text, false)
            .map_err(|_| NerError::Tokenize)?;
        let ids = enc.get_ids();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let n_labels = self.labels.len();
        let mut acc = ProbAccumulator::new(ids.len(), n_labels);
        for range in windows(ids.len()) {
            let content = ids.get(range.clone()).ok_or(NerError::Tokenize)?;
            let mut win: Vec<i64> = Vec::with_capacity(content.len() + 2);
            win.push(i64::from(self.cls));
            win.extend(content.iter().map(|&i| i64::from(i)));
            win.push(i64::from(self.sep));
            let logits = self.run_window(win)?;
            let inner = logits
                .get(n_labels..logits.len().saturating_sub(n_labels))
                .ok_or(NerError::Runtime("sortie du modèle trop courte".into()))?;
            acc.add_window(range, inner);
        }
        Ok(aggregate(
            text,
            enc.get_offsets(),
            &self.labels,
            &acc.argmax(),
        ))
    }

    fn run_window(&self, ids: Vec<i64>) -> Result<Vec<f32>, NerError> {
        let len = ids.len();
        let mut feeds: Vec<(Cow<'_, str>, SessionInputValue<'_>)> = Vec::new();
        for name in &self.inputs {
            let data = match name.as_str() {
                "input_ids" => ids.clone(),
                "attention_mask" => vec![1i64; len],
                _ => vec![0i64; len],
            };
            let t = Tensor::from_array(([1usize, len], data)).map_err(rt)?;
            feeds.push((Cow::Owned(name.clone()), t.into()));
        }
        let mut session = self.session.lock().map_err(|_| NerError::Poisoned)?;
        let outputs = session.run(feeds).map_err(rt)?;
        let value = outputs
            .get(&self.output)
            .ok_or(NerError::Runtime("sortie logits absente".into()))?;
        let (shape, data) = value.try_extract_tensor::<f32>().map_err(rt)?;
        let expected = len * self.labels.len();
        if data.len() != expected || shape.iter().product::<i64>() != expected as i64 {
            return Err(NerError::Runtime(
                "dimensions de sortie inattendues (nombre d'étiquettes ?)".into(),
            ));
        }
        Ok(data.to_vec())
    }
}
