# Anonymiseur — règles pour les agents et les développeurs

Application de bureau **locale** (Windows, macOS) qui remplace les données sensibles d'un document par des balises (`[PERSONNE_1]`), puis remet les vraies valeurs dans la réponse d'une IA. Exécutable unique, aucun réseau. Projet de Skazy Formation, licence AGPL-3.0.

Lire ce fichier en entier avant de modifier le code.

## Commandes

```bash
cargo fmt --all                                          # formater
cargo clippy --workspace --all-targets -- -D warnings    # lint, zéro avertissement
cargo test --workspace                                   # tous les tests
cargo test -p anon-core                                  # un seul crate
cargo run -p anonymiseur                                 # lancer l'application
uv run --project tools tools/make_examples.py            # régénérer les faux fichiers d'exemple
uv run --project tools tools/import_charte.py            # régénérer assets/charte depuis design-skazy
```

Sous Windows, `mupdf-sys` a besoin de libclang : `$env:LIBCLANG_PATH = "<racine>\tools\.venv\Lib\site-packages\clang\native"` (créé par `uv venv tools/.venv` puis `uv pip install --python tools/.venv libclang`).

## Architecture

Espace de travail Cargo. Les dépendances ne vont que vers le bas de cette liste.

| Crate | Rôle |
|---|---|
| `app` (`anonymiseur`) | Interface egui à la charte Skazy. Aucune logique métier. |
| `crates/anon-engine` | Orchestration : analyse, anonymisation, restauration, contrôle de fuite. Tests d'intégration pilotés par `exemples/attendu.json`. |
| `crates/anon-office` | Gestionnaires Word/Excel/PowerPoint (OOXML), ODF, texte brut. Pur Rust. |
| `crates/anon-pdf` | PDF, images, OCR (MuPDF + Tesseract). Seul crate autorisé à utiliser `unsafe` (FFI). |
| `crates/anon-ner` | Détection de noms par modèle ONNX. Optionnel (fonctionnalité `ia`). |
| `crates/anon-detect` | Règles à clé, colonnes de tableur, quasi-identifiants, listes, fusion des détections. |
| `crates/anon-dict` | Dictionnaires embarqués (prénoms, noms, communes) et leur détecteur. |
| `crates/anon-replace` | Normalisation, `Replacer`, `Restorer`, enregistrement des valeurs dans la session. |
| `crates/anon-vault` | Fichier `.anonmap` : session sérialisée et chiffrée par mot de passe. |
| `crates/anon-core` | **Contrats** partagés, rien d'autre. |

`spikes/` contient des prototypes jetables, hors espace de travail.

### Contrats (`anon-core`)

- `entities` : `EntityKind` (le préfixe de balise est figé), `Tag`, `Detection`, `Source`.
- `text` : `TextBlock`, `BlockContext`, `Replacement`, trait `TextTransform`.
- `format` : trait `Handler` (`extract`, `deep_scan`, `transform`), `MediaTransform`, `ImageProcessor`, `Warning`.
- `detect` : trait `Detector`, `DetectOptions`, `Profile`.
- `session` : `Session`, `Entity` — sérialisés dans `.anonmap`.

Changer un contrat = mettre à jour tous les crates consommateurs, ce fichier et, pour `session`, `FORMAT_VERSION` avec un test de relecture de l'ancien format.

### Modèle de fonctionnement

1. `Handler::extract` donne des **blocs de texte logiques** (un paragraphe entier, une cellule).
2. Les `Detector` proposent des `Detection` ; l'utilisateur valide ; les valeurs entrent dans la `Session`.
3. `Replacer` (un `TextTransform`) remplace **toutes** les occurrences des valeurs validées, en une passe. Ce n'est pas un remplacement par position.
4. `Handler::transform` réécrit le fichier ; ce qui n'est pas touché est recopié à l'identique.
5. Contrôle de fuite : `Handler::deep_scan` sur le fichier produit, puis recherche de toute valeur validée.
6. Restauration : même `Handler::transform` avec `Restorer`.

## Invariants — ne jamais les casser

1. **Aucun réseau.** Aucune dépendance réseau à l'exécution (`reqwest`, `hyper`, `ureq`, `tokio`, sockets). `deny.toml` les interdit ; la CI échoue sinon. Les téléchargements n'existent que dans `tools/` (scripts de préparation).
2. **Contrôle de fuite bloquant.** Un fichier où subsiste une valeur validée n'est jamais livré à l'utilisateur.
3. **Aucun contenu utilisateur dans les journaux, erreurs, avertissements ou `Debug`.** Seulement des emplacements (`word/document.xml`, `page 3`) et des balises.
4. **Aucune panique sur un fichier corrompu.** `unwrap`, `expect`, `panic!` et l'indexation non vérifiée sont interdits hors tests ; on renvoie `FormatError`.
5. **Exemples uniquement fictifs.** Aucune donnée réelle dans `exemples/`, les tests ou les instantanés. Emails en `example.com`.
6. **La table de correspondance ne touche le disque que chiffrée**, à l'endroit choisi par l'utilisateur. Aucun fichier temporaire contenant des données en clair.
7. **`C:\Users\gharel\www\design-skazy` est la source de la charte.** `assets/charte/` est une copie générée : ne pas l'éditer à la main, ne rien écrire dans `design-skazy`.
8. **Textes de l'interface** : français, vouvoiement, aucun emoji, titres en majuscule initiale.

## Définition de « terminé »

Un changement est terminé quand :

- toute fonctionnalité nouvelle a ses tests ; tout bug corrigé a un test qui échouait avant la correction ;
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` et `cargo test --workspace` passent ;
- aucun test existant n'a été affaibli, supprimé ou marqué `#[ignore]` pour passer ; si un test doit changer, la raison est écrite dans le message de commit ;
- les seuils de `tests/baseline.json` (rappel et précision de détection) ne baissent pas ;
- un nouveau format ou un nouveau cas piège a son faux fichier dans `exemples/` et son entrée dans `exemples/attendu.json` ;
- `CHANGELOG.md` est à jour (section « Non publié »).

## Tests

- **Unitaires** : dans chaque module (`#[cfg(test)]`).
- **Propriétés** (`proptest`) : anonymiser puis restaurer redonne le texte d'origine ; une balise n'est jamais re-remplacée.
- **Intégration** : `crates/anon-engine/tests`, pilotés par `exemples/attendu.json`.
- **Instantanés** (`insta`) : `cargo insta review` pour accepter un écart, jamais d'acceptation en bloc sans lecture.
- **Robustesse** : fichiers tronqués ou corrompus → `Err`, pas de panique.

Vérifier un comportement en lançant le test, pas en le supposant. Un rapport « ça marche » sans sortie de test ne vaut rien.

## Conventions

- Identifiants en anglais, commentaires et messages utilisateur en français.
- Erreurs typées avec `thiserror` ; pas de `String` comme type d'erreur public.
- Commits conventionnels en français (`feat:`, `fix:`, `test:`, `docs:`, `chore:`), versions sémantiques.
- Une dépendance nouvelle doit être justifiée (taille, licence compatible AGPL, pas de réseau) et ajoutée avec `cargo add -p <crate>`.
- Les nombres magiques de détection (seuils, longueurs) sont des constantes nommées.
