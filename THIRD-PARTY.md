# Composants tiers

Anonymiseur est sous licence AGPL-3.0. Il embarque les composants suivants, chacun sous sa propre licence.

## Moteurs de lecture

| Composant | Licence | Source |
|---|---|---|
| MuPDF 1.27 (via `mupdf-sys`) | AGPL-3.0 | https://mupdf.com |
| Tesseract 5 (OCR, intégré à MuPDF) | Apache-2.0 | https://github.com/tesseract-ocr/tesseract |
| Leptonica (intégré à MuPDF) | BSD-2-Clause | https://github.com/DanBloomberg/leptonica |
| Données de langue `fra` et `eng` (tessdata_fast) | Apache-2.0 | https://github.com/tesseract-ocr/tessdata_fast |
| ONNX Runtime (via `ort`, édition complète) | MIT | https://onnxruntime.ai |

Les licences de Tesseract, de Leptonica et d'ONNX Runtime sont celles publiées par leurs projets ; elles ne sont pas reprises d'un fichier du dépôt, sauf les données tessdata (`assets/tessdata/LISEZMOI.md`).

## Modèles d'IA de noms (édition complète)

| Modèle | Auteur | Licence | Source |
|---|---|---|---|
| DistilCamemBERT-NER (`cmarkea/distilcamembert-base-ner`), requantifié en 4 bits par le projet ; français | cmarkea | MIT | https://huggingface.co/cmarkea/distilcamembert-base-ner |
| pii-medium v1.1.0 (`newmean-ai/pii-medium`), 54 langues | newmean.ai | Apache-2.0 | https://huggingface.co/newmean-ai/pii-medium |

Le modèle pii-medium s'appuie sur mMiniLMv2 (Microsoft, MIT) et le tokeniseur XLM-R (MIT). Powered by newmean.ai. Son fichier NOTICE est reproduit tel quel dans `licences/NOTICE-pii-medium.txt`. Révisions exactes et empreintes SHA-256 : `assets/ner/LISEZMOI.md`.

## Données de détection

| Donnée | Licence | Source |
|---|---|---|
| Fichier des prénoms, fichier des noms de famille, code officiel géographique (communes), Insee | Licence Ouverte 2.0 (Etalab) | https://www.insee.fr |
| Lexique des formes fléchies du français v7.7, Dicollecte / Grammalecte | MPL-2.0 | https://grammalecte.net |

Éditions et traitements exacts : `crates/anon-dict/data/SOURCES.md`.

## Interface

| Composant | Licence | Source |
|---|---|---|
| egui, eframe | MIT ou Apache-2.0 | https://github.com/emilk/egui |
| Polices par défaut d'egui (repli pour les glyphes absents de Georama) | licences de polices libres admises par `deny.toml` (OFL-1.1, Ubuntu-font-1.0) | https://github.com/emilk/egui |
| Lucide 0.460.0 (icônes) | ISC, texte dans `assets/charte/icons/LICENSE` | https://lucide.dev |
| Georama (police de la charte, graisses 400 à 800), © 2020 The Georama Project Authors | SIL Open Font License 1.1, texte dans `licences/OFL-1.1-Georama.txt` | https://github.com/productiontype/Georama |

Les logos et la marque Skazy Formation ne sont pas des composants tiers libres : voir `LICENSE`.

## Bibliothèques Rust

Pour la liste complète et à jour des crates et de leurs licences, lancez `cargo deny list` à la racine du dépôt. Les licences admises sont définies dans `deny.toml`.
