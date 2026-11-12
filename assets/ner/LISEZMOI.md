# Modèles de reconnaissance de noms (`anon-ner`)

Préparation (réseau uniquement ici, révisions Hugging Face figées) :

```
uv run --project tools/model tools/model/prepare_assets.py
```

Les `.onnx` ne sont pas versionnés (`.gitignore`). `tokenizer.json` et `config.json` le sont.

## Fichiers livrés

| Fichier | Source (révision) | Licence | Octets | SHA-256 |
|---|---|---|---|---|
| `fr.onnx` | `cmarkea/distilcamembert-base-ner` (`e539d952`), `model.onnx` requantifié : MatMul 4 bits (MatMulNBits, bloc 32) + plongements `Gather` int8 (`tools/model/prepare_camembert.py`) | MIT | 51 847 114 | `53da0cab5dc74d9fb6e20d48db484b1b8daca705d7cf29ba73314c93471c2012` |
| `fr.tokenizer.json` | généré depuis `sentencepiece.bpe.model` (vérifié contre sentencepiece) | MIT | 2 420 903 | `9ba012bda95411a9a6509be2d55ca53b36ba82f47ec34ca634a2649a4d385662` |
| `fr.config.json` | `config.json` du dépôt | MIT | 945 | `103f36a11079994f128a20c46cb388912dc9d481fded4ce12fbfe2d9f63d2662` |
| `multi.onnx` | `newmean-ai/pii-medium` (`aff3cd7f`, v1.1.0), `onnx/model_q4.onnx` | Apache-2.0 | 13 962 065 | `dec417ab148ab203b1cc9051dadf1117a6590ce6bc02585f434c5095c6a37ba5` |
| `multi.tokenizer.json` | `tokenizer.json` du dépôt | Apache-2.0 (tokeniseur XLM-R, MIT) | 2 554 906 | `547c77a3f8f9ef1373006affb044bc8d487868a11e3cd199e72e6ba2266969fa` |
| `multi.config.json` | `config.json` du dépôt | Apache-2.0 | 1 277 | `713f4d16da5cb6ac3aa42cb55d286b6b47a2c35d9e8edceab95b0830d65dc4f7` |

Attribution demandée (non obligatoire) par newmean.ai : « Powered by newmean.ai » dans l'écran « À propos ».
Le fichier `NOTICE` de pii-medium (mMiniLMv2 Microsoft MIT, tokeniseur XLM-R MIT) doit figurer dans les mentions de licence.

## Banc d'essai (`tools/model/bench.py`)

Données : `exemples/` (blocs extraits, entités non OCR) et jeu de contrôle indépendant
`tools/model/controle/controle.jsonl` (66 phrases FR, 21 EN, 22 autres langues : es, de, it, pt, nl, vi, ar, pl ;
noms calédoniens, polynésiens, vietnamiens, maghrébins, composés, à particule ; 22 phrases sans nom).
Rappel **strict** : tous les caractères alphanumériques de l'occurrence couverts. Précision : la prédiction chevauche
une valeur attendue ou en est une partie. Poste : Intel Core Ultra 9 285H, ONNX Runtime 1.31 CPU.
Les temps (texte de 1 000 mots, médiane de 5) ont été pris pendant que d'autres compilations tournaient : écarts
jusqu'à ×3 entre deux mesures, à ne lire que comme ordre de grandeur.

| Candidat | Licence | Mo | ms/1 000 mots | Pers. FR contrôle R/P | Pers. FR exemples R/P | Pers. EN R/P | Pers. autres R/P | Sociétés FR R/P | Lieux FR R/P |
|---|---|---|---|---|---|---|---|---|---|
| pii-nano q4e | Apache-2.0 | 2.1 | 545 | 87.3 / 94.4 | 85.6 / 67.6 | 90.9 / 91.3 | 85.0 / 90.0 | 70.0 / 88.9 | 7.7 / 100 |
| pii-small q4e | Apache-2.0 | 6.2 | 605 | 89.1 / 93.2 | 85.6 / 74.2 | 95.5 / 100 | 90.0 / 87.0 | 70.0 / 87.5 | 7.7 / 100 |
| pii-medium q4e | Apache-2.0 | 11.3 | 1 048 | 92.7 / 96.4 | 85.6 / 69.6 | 100 / 95.7 | 90.0 / 87.0 | 70.0 / 100 | 7.7 / 100 |
| **pii-medium q4** | Apache-2.0 | 14.0 | 820–2 700 | 92.7 / 98.1 | 88.7 / 71.8 | 100 / 95.7 | 95.0 / 87.0 | 70.0 / 100 | 7.7 / 100 |
| pii-large q4e | Apache-2.0 | 15.5 | 850–3 500 | 92.7 / 91.5 | 87.6 / 68.9 | 95.5 / 95.5 | 95.0 / 90.9 | 60.0 / 100 | 7.7 / 100 |
| distilcamembert fp32 (référence) | MIT | 270.1 | 1 300–2 900 | 100 / 98.2 | 87.6 / 76.0 | 81.8 / 100 | 65.0 / 85.7 | 90.0 / 100 | 92.3 / 93.3 |
| distilcamembert int8 HF (`model_quantized.onnx`) | MIT | 142.7 | 1 360 | **0.0** | 0.0 | 0.0 | 0.0 | 0.0 | 7.7 |
| distilcamembert int8 + Gather (symétrique) | MIT | 67.8 | 994 | **0.0** | 0.0 | 0.0 | 0.0 | 0.0 | 15.4 |
| distilcamembert int8 par canal + Gather | MIT | 68.0 | 505–3 100 | 98.2 / 98.2 | 82.5 / 74.0 | 81.8 / 100 | 60.0 / 80.0 | 60.0 / 100 | 84.6 / 92.9 |
| distilcamembert MatMul 8 bits + Gather | MIT | 73.1 | 2 600–4 800 | 100 / 98.2 | 85.6 / 74.7 | 81.8 / 100 | 65.0 / 85.7 | 90.0 / 100 | 92.3 / 93.3 |
| **distilcamembert MatMul 4 bits + Gather** | MIT | 51.8 | 1 240–3 600 | 100 / 96.5 | 80.4 / 76.4 | 86.4 / 95.2 | 65.0 / 85.7 | 60.0 / 100 | 92.3 / 83.3 |
| distilbert-multilingual-ner-hrl int8 (Xenova) | AFL-3.0, **incompatible** | 135.4 | 3 169 | 90.9 / 96.4 | 81.4 / 87.7 | 95.5 / 100 | 100 / 90.9 | 80.0 / 68.8 | 84.6 / 100 |

Quantification de DistilCamemBERT plongements compris : l'int8 dynamique symétrique (et le fichier int8 publié sur
Hugging Face) donne un modèle muet (rappel 0) ; l'int8 par canal à plage réduite fonctionne (68 Mo, −1,8 point sur le
contrôle, −5 sur les exemples) ; MatMul 4 bits + Gather int8 (52 Mo) garde 100 % sur le contrôle, −7 points sur les
exemples (uniquement des noms seuls dans une cellule).

## Décision

Règle : le plus petit modèle dont le rappel personnes en français est à moins de 2 points du meilleur ; si seul le modèle
français y parvient, l'embarquer avec un multilingue. Mesure de décision : le **jeu de contrôle** (indépendant ; les
exemples ont servi à régler d'autres détecteurs).

- Meilleur rappel FR : 100,0 (DistilCamemBERT). À moins de 2 points : seulement les variantes DistilCamemBERT ; les
  modèles pii plafonnent à 92,7. Le plus petit : **MatMul 4 bits + Gather int8, 51,8 Mo** → `fr.onnx`.
- Compagnon multilingue (même règle sur EN + autres langues, contrôle + exemples) : meilleur compatible 97,6
  (pii-medium q4) ; à moins de 2 points : lui seul (medium q4e et large à 95,2) → **pii-medium q4, 14,0 Mo** → `multi.onnx`.
- Variante écartée : distilbert-hrl, licence AFL-3.0 hors liste `deny.toml`.

Aiguillage par bloc (`whatlang`) : français fiable → `fr`, autre langue fiable → `multi`, détection non fiable
(cellule, texte court) → les deux, union. Détecteur livré, mesuré en Rust (`cargo test -p anon-ner`) : contrôle FR
100 % / 96,2 %, autres langues 100 % / 96,2 % ; exemples 92,3 % / 74,2 %.

## Limites observées

- Noms seuls dans une cellule (« Garnier », « Lefèvre », « de La Fontaine ») souvent manqués : relève des règles de colonne.
- Particules et noms composés partiellement couverts par pii (« Anne de Villeneuve », « Louis d'Harcourt du Plessis »).
- Faux positifs : titres en majuscules (« DONNÉES FICTIVES »), en-têtes (« Prénom », « Ville »), communes calédoniennes
  prises pour des personnes (« Païta », « Dumbéa »), parties locales d'emails (« jean.dupont », recouvertes par la règle email).
- DistilCamemBERT faible hors français (65 % sur les autres langues), d'où l'aiguillage.
