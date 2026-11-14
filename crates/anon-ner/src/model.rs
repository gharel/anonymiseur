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

/// Nombre maximal de fenêtres passées ensemble au modèle. La mémoire d'une passe croît avec
/// le nombre de fenêtres, et ONNX Runtime conserve le pic atteint.
pub(crate) const MAX_BATCH_WINDOWS: usize = 8;

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
    /// Jeton de remplissage des lots (masqué).
    pad: u32,
    inputs: Vec<String>,
    output: String,
    /// Plus grand lot passé au modèle (tests seulement).
    #[cfg(test)]
    max_rows: std::sync::atomic::AtomicUsize,
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
        let pad_id = ["<pad>", "[PAD]"]
            .iter()
            .find_map(|t| tokenizer.token_to_id(t));
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
            pad: pad_id.unwrap_or(sep),
            inputs,
            output,
            #[cfg(test)]
            max_rows: std::sync::atomic::AtomicUsize::new(0),
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
        let ranges = windows(ids.len());
        let mut batch: Vec<Vec<i64>> = Vec::with_capacity(ranges.len());
        for range in &ranges {
            let content = ids.get(range.clone()).ok_or(NerError::Tokenize)?;
            let mut win: Vec<i64> = Vec::with_capacity(content.len() + 2);
            win.push(i64::from(self.cls));
            win.extend(content.iter().map(|&i| i64::from(i)));
            win.push(i64::from(self.sep));
            batch.push(win);
        }
        // Les fenêtres d'un bloc passent par lots de MAX_BATCH_WINDOWS : un lot unique pour un
        // long bloc gonfle la réserve mémoire d'ONNX Runtime, qui la garde ensuite.
        let mut logits = Vec::with_capacity(batch.len());
        for chunk in batch.chunks(MAX_BATCH_WINDOWS) {
            logits.extend(self.run_batch(chunk)?);
        }
        for (range, (win, out)) in ranges.into_iter().zip(batch.iter().zip(&logits)) {
            let inner = out
                .get(n_labels..(win.len().saturating_sub(1)) * n_labels)
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

    /// Passe un lot de fenêtres, complétées à droite par `pad` (masque d'attention à 0).
    /// Renvoie, par fenêtre, les logits de toutes ses positions (`len × n_labels`).
    fn run_batch(&self, batch: &[Vec<i64>]) -> Result<Vec<Vec<f32>>, NerError> {
        let rows = batch.len();
        let width = batch.iter().map(Vec::len).max().unwrap_or(0);
        if rows == 0 || width == 0 {
            return Ok(Vec::new());
        }
        #[cfg(test)]
        self.max_rows
            .fetch_max(rows, std::sync::atomic::Ordering::Relaxed);
        let mut ids = vec![i64::from(self.pad); rows * width];
        let mut mask = vec![0i64; rows * width];
        for (r, win) in batch.iter().enumerate() {
            for (c, &id) in win.iter().enumerate() {
                if let (Some(i), Some(m)) =
                    (ids.get_mut(r * width + c), mask.get_mut(r * width + c))
                {
                    *i = id;
                    *m = 1;
                }
            }
        }
        let mut feeds: Vec<(Cow<'_, str>, SessionInputValue<'_>)> = Vec::new();
        for name in &self.inputs {
            let data = match name.as_str() {
                "input_ids" => ids.clone(),
                "attention_mask" => mask.clone(),
                _ => vec![0i64; rows * width],
            };
            let t = Tensor::from_array(([rows, width], data)).map_err(rt)?;
            feeds.push((Cow::Owned(name.clone()), t.into()));
        }
        let nl = self.labels.len();
        let mut session = self.session.lock().map_err(|_| NerError::Poisoned)?;
        let outputs = session.run(feeds).map_err(rt)?;
        let value = outputs
            .get(&self.output)
            .ok_or(NerError::Runtime("sortie logits absente".into()))?;
        let (shape, data) = value.try_extract_tensor::<f32>().map_err(rt)?;
        let expected = rows * width * nl;
        if data.len() != expected || shape.iter().product::<i64>() != expected as i64 {
            return Err(NerError::Runtime(
                "dimensions de sortie inattendues (nombre d'étiquettes ?)".into(),
            ));
        }
        batch
            .iter()
            .enumerate()
            .map(|(r, win)| {
                data.get(r * width * nl..(r * width + win.len()) * nl)
                    .map(<[f32]>::to_vec)
                    .ok_or(NerError::Runtime("sortie du modèle trop courte".into()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    fn french_model() -> Model {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/ner");
        let read = |f: &str| std::fs::read(dir.join(f)).expect(f);
        Model::load(
            &read("fr.onnx"),
            &read("fr.tokenizer.json"),
            &read("fr.config.json"),
        )
        .expect("modèle français")
    }

    /// Un long bloc passe en plusieurs lots bornés : un lot unique faisait monter la
    /// mémoire d'ONNX Runtime à plus de 4 Go sur les exemples. Les noms du début et de
    /// la fin du bloc restent trouvés, à leur place.
    #[test]
    fn long_block_runs_in_bounded_batches() {
        let model = french_model();
        let filler = "Le comité a examiné le budget annuel et le calendrier des travaux. ";
        let mut text = String::from("Jean Dupont ouvre la séance. ");
        let tokens = |t: &str| model.tokenizer.encode(t, false).map(|e| e.len()).unwrap();
        while tokens(&text) < 3 * MAX_BATCH_WINDOWS * crate::aggregate::WINDOW_TOKENS {
            text.push_str(filler);
        }
        text.push_str("Marie Lefèvre clôt la séance.");

        let spans = model.predict(&text).unwrap();

        let max_rows = model.max_rows.load(Ordering::Relaxed);
        assert!(
            max_rows <= MAX_BATCH_WINDOWS,
            "lot de {max_rows} fenêtres (maximum {MAX_BATCH_WINDOWS})"
        );
        let found = |name: &str| {
            spans
                .iter()
                .any(|s| text.get(s.start..s.end).is_some_and(|v| v.contains(name)))
        };
        assert!(found("Jean Dupont"), "nom du début");
        assert!(found("Marie Lefèvre"), "nom de la fin");
    }
}
