//! Images (traitement factice) et contrôle de fuite (gestionnaire factice).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anon_core::{
    BlockContext, BlockRole, EntityKind, Extraction, FormatError, Handler, ImageProcessor,
    MaskRegion, MediaItem, OcrPage, OcrWord, Output, PixelRect, Session, TextBlock, TextTransform,
    TransformOptions,
};
use anon_detect::RuleDetector;
use anon_dict::DictionaryDetector;
use anon_engine::{
    AnalyzeOptions, AnonymizeOptions, Engine, EngineError, InputFile, MASK_MARGIN_PX, ManualMask,
    NoProgress, sha256_hex,
};
use std::sync::{Arc, Mutex};

// --- Image factice : une ligne « x y w h mot » par mot, « MASK x y w h » par zone masquée.

fn image(words: &[(u32, u32, u32, u32, &str)]) -> Vec<u8> {
    let mut s = String::from("FAKEIMG\n");
    for (x, y, w, h, t) in words {
        s.push_str(&format!("{x} {y} {w} {h} {t}\n"));
    }
    s.into_bytes()
}

fn rect(parts: &[&str]) -> Option<PixelRect> {
    let n: Vec<u32> = parts
        .iter()
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    Some(PixelRect {
        x: *n.first()?,
        y: *n.get(1)?,
        w: *n.get(2)?,
        h: *n.get(3)?,
    })
}

fn intersects(a: PixelRect, b: PixelRect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

#[derive(Default)]
struct Calls {
    ocr: usize,
    masks: Vec<Vec<MaskRegion>>,
}

/// OCR et masquage factices. `lazy` : le masquage ne fait rien (fuite simulée).
struct FakeProcessor {
    calls: Arc<Mutex<Calls>>,
    lazy: bool,
}

impl ImageProcessor for FakeProcessor {
    fn ocr(&self, image: &[u8]) -> Result<OcrPage, FormatError> {
        self.calls.lock().unwrap().ocr += 1;
        let text = std::str::from_utf8(image).map_err(|_| FormatError::Other("image".into()))?;
        let mut lines = text.lines();
        if lines.next() != Some("FAKEIMG") {
            return Err(FormatError::corrupt("image", "entête"));
        }
        let mut masks = Vec::new();
        let mut words = Vec::new();
        for l in lines {
            let parts: Vec<&str> = l.split(' ').collect();
            if parts.first() == Some(&"MASK") {
                masks.extend(rect(&parts[1..5]));
            } else if parts.len() >= 5 {
                words.push(OcrWord {
                    text: parts[4..].join(" "),
                    rect: rect(&parts[..4]).unwrap(),
                    confidence: 0.95,
                });
            }
        }
        words.retain(|w| !masks.iter().any(|m| intersects(*m, w.rect)));
        Ok(OcrPage {
            width: 400,
            height: 100,
            words,
        })
    }

    fn mask(&self, image: &[u8], regions: &[MaskRegion]) -> Result<Vec<u8>, FormatError> {
        self.calls.lock().unwrap().masks.push(regions.to_vec());
        let mut out = image.to_vec();
        if !self.lazy {
            for r in regions {
                out.extend(
                    format!("MASK {} {} {} {}\n", r.rect.x, r.rect.y, r.rect.w, r.rect.h)
                        .into_bytes(),
                );
            }
        }
        Ok(out)
    }
}

// --- Paquet factice : corps (une ligne par bloc) puis images, séparés par « \n===\n ».

const SEP: &str = "\n===\n";

struct PackHandler;
static PACK: PackHandler = PackHandler;

fn split(input: &[u8]) -> Result<(String, Vec<Vec<u8>>), FormatError> {
    let s = std::str::from_utf8(input).map_err(|_| FormatError::corrupt("pack", "utf-8"))?;
    let mut parts = s.split(SEP);
    let body = parts.next().unwrap_or("").to_string();
    Ok((body, parts.map(|p| p.as_bytes().to_vec()).collect()))
}

fn join(body: &str, images: &[Vec<u8>]) -> Vec<u8> {
    let mut out = body.as_bytes().to_vec();
    for i in images {
        out.extend_from_slice(SEP.as_bytes());
        out.extend_from_slice(i);
    }
    out
}

fn media_name(i: usize) -> String {
    format!("media/image{}.png", i + 1)
}

fn ctx(i: usize) -> BlockContext {
    BlockContext::new(format!("ligne {}", i + 1), BlockRole::Body)
}

impl Handler for PackHandler {
    fn extensions(&self) -> &'static [&'static str] {
        &["pack"]
    }

    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        let (body, images) = split(input)?;
        Ok(Extraction {
            blocks: body
                .lines()
                .enumerate()
                .map(|(i, l)| TextBlock {
                    text: l.into(),
                    ctx: ctx(i),
                })
                .collect(),
            media: images
                .into_iter()
                .enumerate()
                .map(|(i, bytes)| MediaItem {
                    name: media_name(i),
                    bytes,
                })
                .collect(),
            warnings: Vec::new(),
        })
    }

    fn transform(
        &self,
        input: &[u8],
        tr: &dyn TextTransform,
        opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        let (body, images) = split(input)?;
        let mut count = 0;
        let lines: Vec<String> = body
            .lines()
            .enumerate()
            .map(|(i, l)| {
                let reps = tr.replacements(l, &ctx(i));
                count += reps.len();
                anon_core::text::apply_replacements(l, &reps).into_owned()
            })
            .collect();
        let images: Vec<Vec<u8>> = images
            .into_iter()
            .enumerate()
            .map(|(i, b)| {
                opts.media
                    .and_then(|m| m.apply(&media_name(i), &b))
                    .unwrap_or(b)
            })
            .collect();
        Ok(Output {
            bytes: join(&lines.join("\n"), &images),
            replacements: count,
            warnings: Vec::new(),
        })
    }
}

/// Gestionnaire qui ne remplace rien : toute valeur fuit.
struct NoopHandler;
static NOOP: NoopHandler = NoopHandler;

impl Handler for NoopHandler {
    fn extensions(&self) -> &'static [&'static str] {
        &["noop"]
    }
    fn extract(&self, input: &[u8]) -> Result<Extraction, FormatError> {
        Ok(Extraction {
            blocks: vec![TextBlock {
                text: String::from_utf8_lossy(input).into_owned(),
                ctx: BlockContext::new("corps", BlockRole::Body),
            }],
            ..Default::default()
        })
    }
    fn transform(
        &self,
        input: &[u8],
        _tr: &dyn TextTransform,
        _opts: &TransformOptions<'_>,
    ) -> Result<Output, FormatError> {
        Ok(Output {
            bytes: input.to_vec(),
            replacements: 0,
            warnings: Vec::new(),
        })
    }
}

fn engine(lazy: bool) -> (Engine, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let e = Engine::builder()
        .handler("pack", &PACK)
        .handler("noop", &NOOP)
        .detector(Box::new(RuleDetector::new()))
        .detector(Box::new(DictionaryDetector::new()))
        .image_processor(Box::new(FakeProcessor {
            calls: calls.clone(),
            lazy,
        }))
        .build();
    (e, calls)
}

const WORDS: &[(u32, u32, u32, u32, &str)] = &[
    (0, 0, 50, 10, "Contact"),
    (60, 0, 50, 10, "Olivier"),
    (120, 0, 50, 10, "Nguyen"),
    (0, 20, 200, 10, "olivier.nguyen@example.org"),
];

fn pack() -> (InputFile, Vec<u8>) {
    let img = image(WORDS);
    (
        InputFile::new(
            "rapport.pack",
            join(
                "Rapport de suivi.\nAucun nom ici.",
                std::slice::from_ref(&img),
            ),
        ),
        img,
    )
}

fn widened(x: u32, y: u32, w: u32, h: u32) -> PixelRect {
    let m = MASK_MARGIN_PX;
    let (x0, y0) = (x.saturating_sub(m), y.saturating_sub(m));
    PixelRect {
        x: x0,
        y: y0,
        w: x + w + m - x0,
        h: y + h + m - y0,
    }
}

/// 7. Zones masquées calculées à partir des mots, original rangé puis remis.
#[test]
fn images_masquees_puis_restaurees() {
    let (engine, calls) = engine(false);
    assert!(engine.supported_extensions().contains(&"pack"));
    let (file, original_image) = pack();
    let analysis = engine.analyze(
        std::slice::from_ref(&file),
        &AnalyzeOptions::default(),
        &Session::new(),
        &NoProgress,
    );
    assert_eq!(
        analysis.files[0].images,
        vec!["media/image1.png".to_string()]
    );
    assert_eq!(calls.lock().unwrap().ocr, 1);
    let person = analysis
        .candidates
        .iter()
        .find(|c| c.kind == EntityKind::Personne)
        .expect("personne lue par OCR");
    assert_eq!(person.value, "Olivier Nguyen");
    assert_eq!(person.occurrences, 1);
    assert!(
        analysis
            .candidates
            .iter()
            .any(|c| c.kind == EntityKind::Email)
    );

    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let manual = PixelRect {
        x: 300,
        y: 50,
        w: 40,
        h: 20,
    };
    let opts = AnonymizeOptions {
        manual_masks: vec![ManualMask {
            media: "media/image1.png".into(),
            rect: manual,
        }],
    };
    let out = engine
        .anonymize(&file, &analysis, &mut session, &opts)
        .unwrap();
    assert_eq!(out.name, "rapport_anonymise.pack");

    // L'OCR de l'analyse est réutilisé : un seul nouvel appel (contrôle de fuite).
    let c = calls.lock().unwrap();
    assert_eq!(c.ocr, 2);
    assert_eq!(c.masks.len(), 1);
    let regions = &c.masks[0];
    let rects: Vec<PixelRect> = regions.iter().map(|r| r.rect).collect();
    assert!(rects.contains(&widened(60, 0, 50, 10)), "{rects:?}");
    assert!(rects.contains(&widened(120, 0, 50, 10)));
    assert!(rects.contains(&widened(0, 20, 200, 10)));
    assert!(rects.contains(&manual));
    assert!(
        !rects.contains(&widened(0, 0, 50, 10)),
        "« Contact » masqué"
    );
    // Balise écrite sur le premier mot de chaque remplacement seulement.
    let olivier = regions
        .iter()
        .find(|r| r.rect == widened(60, 0, 50, 10))
        .unwrap();
    assert_eq!(olivier.label.as_deref(), Some("[PERSONNE_1]"));
    let nguyen = regions
        .iter()
        .find(|r| r.rect == widened(120, 0, 50, 10))
        .unwrap();
    assert_eq!(nguyen.label, None);
    drop(c);
    assert_eq!(out.report.replacements, 4);

    // Original rangé sous le SHA-256 de l'image masquée.
    let (_, images) = split(&out.bytes).unwrap();
    assert_eq!(session.media.len(), 1);
    assert_eq!(session.media[0].masked_sha256, sha256_hex(&images[0]));
    assert_eq!(session.media[0].original, original_image);

    // Restauration : l'image d'origine revient.
    let restored = engine
        .restore(
            &InputFile::new(out.name.clone(), out.bytes.clone()),
            &session,
        )
        .unwrap();
    assert_eq!(restored.name, "rapport_restaure.pack");
    assert_eq!(restored.bytes, file.bytes);
}

#[test]
fn images_mode_irreversible() {
    let (engine, _) = engine(false);
    let (file, _) = pack();
    let analysis = engine.analyze(
        std::slice::from_ref(&file),
        &AnalyzeOptions::default(),
        &Session::new(),
        &NoProgress,
    );
    let mut session = Session {
        irreversible: true,
        ..Session::new()
    };
    engine.apply_selection(&analysis, &mut session);
    let out = engine
        .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
        .unwrap();
    assert!(session.media.is_empty());
    // Restauration : le texte revient, pas l'image.
    let restored = engine
        .restore(&InputFile::new(out.name, out.bytes.clone()), &session)
        .unwrap();
    assert_eq!(restored.bytes, out.bytes);
}

/// 6. Gestionnaire qui ne remplace rien : fuite, aucun octet rendu.
#[test]
fn fuite_simulee_dans_le_texte() {
    let (engine, _) = engine(false);
    let file = InputFile::new("lettre.noop", b"Jean Dupont signe le contrat.".to_vec());
    let analysis = engine.analyze(
        std::slice::from_ref(&file),
        &AnalyzeOptions::default(),
        &Session::new(),
        &NoProgress,
    );
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    assert!(!session.entities.is_empty());
    let result = engine.anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default());
    let Err(EngineError::Leak { leaks }) = result else {
        panic!("fuite non détectée");
    };
    assert_eq!(leaks.len(), 1);
    assert_eq!(leaks[0].tag.to_string(), "[PERSONNE_1]");
    assert_eq!(leaks[0].part, "corps");
    let msg = EngineError::Leak {
        leaks: leaks.clone(),
    }
    .to_string();
    assert!(!msg.contains("Jean") && !msg.contains("Dupont"), "{msg}");
    assert!(!format!("{leaks:?}").contains("Dupont"));

    // Le repli en texte, lui, est anonymisé.
    let text = engine.export_text(&file, &session).unwrap();
    assert_eq!(text, "[PERSONNE_1] signe le contrat.");
}

/// 6 (images). Masquage sans effet : la ré-lecture de l'image trouve la fuite.
#[test]
fn fuite_simulee_dans_une_image() {
    let (engine, _) = engine(true);
    let (file, _) = pack();
    let analysis = engine.analyze(
        std::slice::from_ref(&file),
        &AnalyzeOptions::default(),
        &Session::new(),
        &NoProgress,
    );
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let result = engine.anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default());
    let Err(EngineError::Leak { leaks }) = result else {
        panic!("fuite d'image non détectée");
    };
    assert!(leaks.iter().all(|l| l.part == "media/image1.png"));
    assert!(leaks.iter().any(|l| l.tag.kind == EntityKind::Personne));
    // Rien n'est gardé quand le fichier n'est pas rendu.
    assert!(session.media.is_empty());
}
