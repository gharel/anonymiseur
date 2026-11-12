//! Sonde de liaison et de taille : charge les modèles (inclus avec `embed`, sinon depuis
//! `assets/ner/`), analyse une phrase fictive et affiche les types trouvés (jamais le texte).
//!
//! `cargo run -p anon-ner --release --example sonde --features embed`

use anon_ner::{NerDetector, NerError};

fn detector() -> Result<NerDetector, NerError> {
    #[cfg(feature = "embed")]
    {
        Ok(NerDetector::embedded())
    }
    #[cfg(not(feature = "embed"))]
    {
        NerDetector::from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/ner"))
    }
}

fn main() -> std::process::ExitCode {
    let text = "Le contrat est signé par Jean Dupont pour Atelier Bougainville SAS, à Lyon.";
    let result = detector().and_then(|d| d.find(text));
    match result {
        Ok(spans) => {
            let kinds: Vec<_> = spans
                .iter()
                .map(|s| (s.kind.prefix(), s.start, s.end))
                .collect();
            #[allow(clippy::print_stdout)]
            {
                println!("ia disponible ; entités : {kinds:?}");
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("{e}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}
