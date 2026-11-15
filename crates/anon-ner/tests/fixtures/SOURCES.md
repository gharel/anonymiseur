# Images de test de la détection des visages

Aucune donnée personnelle : trois peintures du domaine public, d'artistes morts depuis plus de 70 ans (reproductions photographiques fidèles d'œuvres en deux dimensions, mention PD-Art sur Wikimedia Commons), et une page de texte fictive générée par le script.

Regénérer : `uv run --project tools tools/prepare_faces.py --fixtures`. Le script vérifie l'empreinte SHA-1 de l'original sur Commons, télécharge la vignette de 1 280 pixels de large, la réduit (Lanczos) et l'enregistre en JPEG de qualité 85.

| Fichier | Œuvre | Source | Licence | Taille |
|---|---|---|---|---|
| `portrait_mona_lisa.jpg` | Léonard de Vinci (1452-1519), *La Joconde*, 1503-1506, musée du Louvre | [Commons](https://commons.wikimedia.org/wiki/File:Mona_Lisa,_by_Leonardo_da_Vinci,_from_C2RMF_retouched.jpg), original SHA-1 `4ba2f28c16b265bcde00336a8e0b1251e4a258da` | Domaine public (`PD-art`, `PD-old-100-expired` ; retouche versée au domaine public par son auteur, `PD-retouched-user`) | 671 × 1 000 |
| `portrait_jeune_fille_perle.jpg` | Johannes Vermeer (1632-1675), *La Jeune Fille à la perle*, vers 1665, Mauritshuis | [Commons](https://commons.wikimedia.org/wiki/File:1665_Girl_with_a_Pearl_Earring.jpg), original SHA-1 `863e60ca490e9e58214f2ecc5931d2d9b837f6d8` | Domaine public (`PD-Art-two-auto`, 1675) | 675 × 800 |
| `paysage_nuit_etoilee.jpg` | Vincent van Gogh (1853-1890), *La Nuit étoilée*, 1889, MoMA | [Commons](https://commons.wikimedia.org/wiki/File:Van_Gogh_-_Starry_Night_-_Google_Art_Project.jpg), original SHA-1 `b3e5f8291b4137ff50ba876c84a1f7248186104a` | Domaine public (`PD-Art`, `PD-old-100-expired`) | 800 × 634 |
| `page_texte.png` | Compte rendu de réunion fictif, sans nom, écrit en Georama (police de la charte, OFL-1.1) | `tools/prepare_faces.py` | Créée pour le projet, AGPL-3.0 | 850 × 1 100, niveaux de gris |

## Visages annotés à la main

Boîtes du front au menton et d'une joue à l'autre, sans les cheveux, relevées sur une grille de 20 pixels (constantes de `tests/faces.rs`) :

| Fichier | x | y | largeur | hauteur |
|---|---|---|---|---|
| `portrait_mona_lisa.jpg` | 242 | 137 | 153 | 218 |
| `portrait_jeune_fille_perle.jpg` | 211 | 227 | 200 | 245 |

`paysage_nuit_etoilee.jpg` et `page_texte.png` ne contiennent aucun visage.
