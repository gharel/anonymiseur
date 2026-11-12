# Spike PDF — MuPDF via `mupdf` 0.8 (notes pour l'auteur de `anon-pdf`)

Testé le 2026-11-12 sur Windows 11, Rust 1.99.1 MSVC, VS 2022 Community (toolset v143).
MuPDF embarqué : 1.27.2 ; Tesseract embarqué : 5.0.0-alpha (sources MuPDF).
**macOS non testé.**

Lancer : `python gen_fixtures.py` (via `uv run --project ../../tools`), puis `cargo run`.
Fichiers produits dans `out/` (PDF caviardés + rendus PNG pour contrôle visuel).

## Résultat (sortie réelle de `cargo run --release`)

```
OK     1. Compilation / liaison MuPDF
OK     2. Caviardage PDF texte + balises + métadonnées
OK     3. Cas piège (', ", TJ, hexa, coupure de ligne)
OK     4. OCR Tesseract (PDF image, PNG)
OK     5. Caviardage de pixels + ré-OCR
```

Le détail de chaque vérification est dans `run_debug.log`.

## Cargo

```toml
mupdf = { version = "0.8", default-features = false, features = ["base14-fonts", "img", "tesseract"] }
```

- `base14-fonts` : Helvetica intégrée (pour écrire les balises et rendre les PDF simples).
- `tesseract` : nécessaire côté Rust, `DocumentWriter::with_ocr` est sous `#[cfg(feature = "tesseract")]`.
- `img` : active `FZ_ENABLE_IMG` (ouverture de PNG/JPEG comme documents). Non testé sans.
- Exclus : `js`, `xps`, `svg`, `cbz`, `html`, `epub`, `system-fonts` (tire `font-kit`), `brotli`, `docx-output`.
- **Sous MSVC, les fonctions Cargo ne réduisent pas le code compilé** : `libmupdf.vcxproj` référence
  toujours libtesseract, libleptonica, libextract, libmubarcode, libharfbuzz (et définit
  `HAVE_TESSERACT`). Les fonctions ne changent que les `FZ_ENABLE_*`. La taille ne baissera pas
  en retirant `tesseract`/`docx-output` sous Windows (sous macOS, `make` respecte ces options).

## Build Windows : obstacles et solutions

| Obstacle | Solution |
|---|---|
| Sous-modules | Inutile depuis crates.io : le paquet `mupdf-sys` 0.8.0 contient les sources MuPDF. |
| `libclang` absent | `LIBCLANG_PATH=<racine>\tools\.venv\Lib\site-packages\clang\native` |
| `error[E0425]: cannot find type max_align_t` dans `mupdf/src/device/native.rs:599` | Le libclang du paquet pip n'a pas les en-têtes de ressources de clang ; les en-têtes MSVC ne définissent pas `max_align_t` en C. Solution : `BINDGEN_EXTRA_CLANG_ARGS="-include <chemin>/bindgen/max_align_t.h"` avec `typedef double max_align_t;` (même définition que clang pour MSVC). |
| Avertissement `LNK4098 msvcrt` en profil dev | Bénin : le profil dev compile MuPDF en configuration Debug. Disparaît en release. |
| Variables d'environnement | Mises dans `.cargo/config.toml` `[env]` (chemins absolus de cette machine). |

Temps mesurés (machine de dev, MSBuild `/MP`) :
- premier build complet (debug) : ~205 s, dont presque tout pour `mupdf-sys` ;
- rebuild complet de `mupdf-sys` après changement de variable : ~208 s ;
- `cargo build --release` (profil ci-dessous) : 151 s ; avec CRT statique : 180 s ;
- recompilation du seul code Rust : ~2 s.
- Le script de build **efface et recopie** les sources à chaque relance : tout changement de
  variable suivie (`BINDGEN_EXTRA_CLANG_ARGS`, fonctions…) coûte ~3 min.

## Taille et CRT statique (point 6)

Profil : `opt-level=3, lto="thin", codegen-units=1, strip=true, panic="abort"`.

| Variante | Taille `.exe` | DLL hors système |
|---|---|---|
| release, CRT dynamique | 9 272 320 o (8,8 Mio) | MSVCP140, VCRUNTIME140, VCRUNTIME140_1 (+ api-ms-win-crt-*) |
| release, `+crt-static` + `_CL_=/MT` | 9 667 072 o (9,2 Mio) | aucune (KERNEL32, ntdll, bcryptprimitives) |

Données Tesseract en plus : `fra` 1,1 Mo + `eng` 4,1 Mo (tessdata_fast).

CRT statique :
- `RUSTFLAGS="-C target-feature=+crt-static"` **seul échoue** : `LNK2001 __imp_unlink, __imp_realloc,
  __imp_fwrite…` (les .vcxproj de MuPDF compilent en `/MD`).
- **Solution qui marche** : ajouter la variable d'environnement `_CL_=/MT` (MSBuild la transmet à
  `cl.exe`, qui l'ajoute *après* les options du projet, donc `/MT` remplace `/MD`). Il faut forcer
  la relance du script : `cargo clean -p mupdf-sys --release`. Binaire testé : 5/5 OK.
- `_CL_` n'est pas suivie par `cargo:rerun-if-env-changed` : la mettre dans `.cargo/config.toml`
  `[env]` ET nettoyer `mupdf-sys` une fois. Pour la CI, la poser avant le premier build.

## Appels d'API qui marchent (signatures exactes, crate `mupdf` 0.8.0)

Aucun `unsafe` ni FFI n'a été nécessaire dans le spike.

Extraction des caractères avec positions :
- `Page::to_text_page(&self, flags: TextPageFlags) -> Result<TextPage, Error>`
- `TextPage::structured(&self) -> StructuredText` → `blocks[].content: TextBlockContent::Text { lines }`,
  `lines[].chars[]: StructuredTextChar { ch: char, origin: Point, quad: Quad, size: f32, .. }`
  (`Quad` n'est pas `Copy` : `.clone()`).
- `TextPage::words(&self) -> Vec<TextWord>` (`text`, `bounds: Rect`, `block`, `line`, `word`).
- `Page::search(&self, needle: &str, hit_max: u32) -> Result<FzArray<Quad>, Error>`.
- Coordonnées : espace MuPDF (origine en haut à gauche, y vers le bas), identiques pour
  l'annotation, `Shape` et la recherche.

Caviardage :
- `PdfDocument::load_pdf_page(&self, page_no: i32) -> Result<PdfPage, Error>` (`PdfPage: Deref<Target = Page>`)
- `PdfPage::add_redact_annotation(&mut self, area: impl Into<AnnotationArea>) -> Result<PdfAnnotation, Error>`
  (`Rect`, `Quad`, `Vec<Quad>`, `&[Quad]` acceptés).
- `PdfPage::apply_redactions_with_options(&mut self, options: PdfRedactOptions) -> Result<bool, Error>`
- `PdfRedactOptions { black_boxes: bool, image_method: PdfRedactImageMethod, line_art: PdfRedactLineArtMethod, text: PdfRedactTextMethod }`
  - texte : `PdfRedactOptions::mupdf_default()` (= pas de boîte noire, `Pixels`, `LineArt::None`, `Text::Remove`) ;
  - scan : `image_method: PdfRedactImageMethod::Pixels` → les pixels sont réellement effacés dans l'image
    (vérifié en extrayant l'image seule et en la ré-OCRisant).
  - `apply_redactions()` sans options = `pymupdf_default()` (supprime aussi le dessin vectoriel couvert).

Écriture de la balise :
- `Shape::new(page: &mut PdfPage) -> Result<Shape, Error>`
- `Shape::insert_text(&mut self, point: Point, text: &str, opts: &TextOptions) -> Result<&mut Self, Error>`
  (`point` = début de ligne de base ; `TextOptions { fontsize, fontname: "helv".into(), fill: Some(PdfColor::rgb(0.,0.,0.)), ..Default::default() }`)
- `Shape::draw_rect(&mut self, &Rect)?.finish(&FinishOptions { color: None, fill: Some(blanc), .. })?` (fond blanc sur scan)
- `Shape::commit(&mut self, doc: &mut PdfDocument, overlay: bool) -> Result<(), Error>`
- Mesure : `Font::new("Helvetica")`, `font.encode_character(c as i32)`, `font.advance_glyph(g)` (en em) ;
  taille = `min(taille d'origine, largeur zone / largeur à 1 pt)`.

Métadonnées et enregistrement :
- `doc.trailer()?.dict_delete("Info")`, `doc.catalog()?.dict_delete("Metadata")` (XMP).
- `PdfWriteOptions::default()` + `.set_garbage_level(4).set_clean(true).set_compress(true)` puis
  `doc.save_with_options(path: &str, opts)`. Le ramasse-miettes supprime les objets orphelins (ancien
  Info, flux XMP) : vérifié, aucun octet du nom dans le fichier réécrit décompressé.
- Contrôle de fuite brut : `doc.write_to_with_options(&mut Vec<u8>, opts.set_decompress(true))` puis
  recherche UTF-8 / Latin-1 / UTF-16BE.

OCR (point 4) — **l'API sûre n'expose pas le périphérique OCR**, seulement l'écrivain « pdfocr » :
```rust
let mut w = DocumentWriter::with_ocr(tmp_path, "ocr-language=fra+eng,ocr-datadir=C:/…/tessdata")?;
let dev = w.begin_page(page.bounds()?)?;
page.run(&dev, &Matrix::IDENTITY)?;
w.end_page(dev)?;
drop(w);                                   // ferme le fichier
let words = Document::open(tmp_path)?.load_page(0)?.words(TextExtractOptions::default())?;
```
- Marche pour une page PDF image seule et pour un PNG ouvert par `Document::open("x.png")`.
- Les boîtes de mots sont dans l'espace de la page source (réutilisables pour le caviardage).
- 0,4–0,5 s par page A4 à 200 dpi en profil dev ; exécution complète des 5 points en release : 1,3 s.
- `ocr-datadir` : dossier contenant `fra.traineddata`/`eng.traineddata` (barres obliques OK).

## Besoins `unsafe` / FFI pour le vrai crate

- **Fichier temporaire de l'OCR** : `with_ocr` n'écrit que vers un chemin. Le PDF temporaire contient
  l'image du scan en clair et le texte reconnu → contraire à l'esprit de l'invariant 6.
  `fz_new_pdfocr_writer_with_output` (vers un `fz_output`/buffer) et `fz_new_ocr_device` existent dans
  `mupdf-sys`, mais **inutilisables depuis Rust avec le contexte du crate `mupdf`** : `context()`,
  `Device::from_raw` et `FFIWrapper::as_ptr` sont `pub(crate)`. Options :
  1. petite cale C (compilée avec `cc` contre les en-têtes de `mupdf-sys`) qui crée **son propre**
     `fz_context`, fait `fz_new_stext_page` + `fz_new_stext_device` + `fz_new_ocr_device(ctx, stext_dev, ctm, mediabox, 1, "fra+eng", datadir, NULL, NULL)` + `fz_run_page`, le tout sous `fz_try`, et rend les mots/boîtes ;
  2. proposer en amont un accès public (ou `with_ocr_to_buffer`) ;
  3. appeler `fz_*` directement depuis Rust : **à proscrire**, une exception MuPDF fait un `longjmp`
     à travers des cadres Rust (comportement indéfini) ou termine le processus.
- Tout le reste du spike passe par l'API sûre.

## Pièges observés

- **Ordre de lecture** : la balise est écrite dans un flux de contenu ajouté en fin de page ; à
  l'extraction elle sort après le reste (« Entre , demeurant… » puis « [PERSONNE_1] »). Position
  vérifiée correcte (centre de la balise dans la zone d'origine), mais un contrôle « texte suivi »
  doit se faire par position, pas par ordre du flux.
- **Nom coupé sur deux lignes** : la balise ajustée au seul premier morceau (« Jean ») tombe à
  ~4 pt (illisible à l'œil, extractible). Prévoir une taille minimale qui déborde, ou une balise
  par morceau.
- Recherche : `Page::search` ne suffit pas pour les noms coupés ; le spike reconstruit le texte
  depuis les caractères (lignes jointes par une espace) et retrouve 5/5 occurrences dans le PDF
  piège (`'`, `"`, `TJ` crénelé, chaîne hexa, coupure de ligne). Césure (« Müller-\nLefèvre ») non testée.
- Boîtes Tesseract un peu justes : sans marge, un bout du dernier glyphe restait visible. Marge de
  2 pt (`OCR_MARGIN`).
- Ré-OCR de la balise : lue « [PERSONNE _2] » (espace parasite) — ne pas compter sur un OCR exact
  des balises.
- Le caviardage MuPDF supprime le glyphe entier dès qu'il touche la zone ; les voisins (« Entre »,
  « , demeurant ») ont été conservés, mais un crénage serré pourrait mordre sur un caractère voisin.
- `Document::metadata()` renvoie `""` (pas d'erreur) quand `/Info` est absent.

## Non vérifié

- macOS (build `make`, taille, signature).
- PDF chiffrés, formulaires, annotations existantes, polices Type3/CID sans ToUnicode, texte en
  rotation, images en masque (`ImageMask`), pages multiples, PDF corrompus (pas de panique ?).
- Fils d'exécution : le contexte MuPDF du crate est local au fil ; usage multi-fils non testé.
- Build sans `img`, et réduction de taille possible en retirant libextract/libmubarcode du .vcxproj.
