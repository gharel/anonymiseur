# Compiler `anon-pdf`

`anon-pdf` embarque MuPDF 1.27 et Tesseract 5 (via `mupdf-sys` 0.8, sources incluses dans le
paquet crates.io) et compile une petite cale C (`csrc/ocr_shim.c`) pour faire l'OCR en mémoire.

## Windows (MSVC)

Prérequis :

- Visual Studio 2022 (ou Build Tools) avec le toolset C++ v143 : `mupdf-sys` compile MuPDF avec MSBuild.
- Rust 1.99 MSVC (fixé par `rust-toolchain.toml`).
- **libclang** pour bindgen, sans installer LLVM :
  `uv venv tools/.venv` puis `uv pip install --python tools/.venv libclang`.

Variables (posées par `tools/dev-env.ps1`, à sourcer dans le shell) :

```powershell
. .\tools\dev-env.ps1             # LIBCLANG_PATH + BINDGEN_EXTRA_CLANG_ARGS
. .\tools\dev-env.ps1 -StaticCrt  # en plus : _CL_=/MT et RUSTFLAGS=+crt-static
```

| Variable | Valeur | Pourquoi |
|---|---|---|
| `LIBCLANG_PATH` | `<racine>\tools\.venv\Lib\site-packages\clang\native` | bindgen charge `libclang.dll`. |
| `BINDGEN_EXTRA_CLANG_ARGS` | `-include <racine>/tools/bindgen/max_align_t.h` | le libclang de pip n'a pas les en-têtes de ressources de clang ; sans ce fichier, `max_align_t` manque (erreur E0425 dans `mupdf/src/device/native.rs`). |
| `_CL_` | `/MT` | CRT statique : les `.vcxproj` de MuPDF compilent en `/MD` ; `cl.exe` ajoute `_CL_` après les options du projet. |
| `RUSTFLAGS` | `-C target-feature=+crt-static` | CRT statique côté Rust (et pour la cale C, via le crate `cc`). |

Pour l'espace de travail, ces réglages peuvent aller dans `.cargo/config.toml` à la racine :

```toml
[env]
LIBCLANG_PATH = { value = "tools/.venv/Lib/site-packages/clang/native", relative = true }
BINDGEN_EXTRA_CLANG_ARGS = "-include tools/bindgen/max_align_t.h"   # chemin à rendre absolu si bindgen ne le trouve pas
_CL_ = "/MT"

[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

Remarques :

- `RUSTFLAGS=+crt-static` **seul** échoue à l'édition de liens (`LNK2001 __imp_unlink…`) : il faut `_CL_=/MT` en même temps.
- `_CL_` n'est pas suivie par Cargo : après l'avoir changée, `cargo clean -p mupdf-sys` (profil concerné).
- Le script de `mupdf-sys` recopie et recompile toutes les sources à chaque relance : ~3 min.
  Tout changement de variable suivie (`BINDGEN_EXTRA_CLANG_ARGS`, fonctions) coûte ce temps.
- En profil dev, MuPDF est compilé en Debug : avertissement `LNK4098 msvcrt` bénin.
- Sous MSVC, les fonctions Cargo de `mupdf` ne réduisent pas le code compilé (les `.vcxproj` incluent toujours Tesseract, Leptonica…).

## macOS (non vérifié)

- Xcode Command Line Tools (`clang`, `make`) ; `mupdf-sys` compile MuPDF avec `make`.
- libclang est fourni par les outils Xcode : en général rien à poser. Sinon
  `LIBCLANG_PATH=$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib`.
- Pas de `max_align_t.h` ni de `_CL_` : le CRT est celui du système (libc++ et libSystem dynamiques, normal sur macOS).
- La fonction `tesseract` de `mupdf` est **indispensable** : sous `make`, elle décide si Tesseract
  et Leptonica sont compilés ; la cale appelle `ocr_init`, `ocr_recognise` et l'API C de Tesseract
  (`capi.cpp`, listée dans `Makelists`).

## Cale OCR (`build.rs`)

`mupdf-sys` ne publie pas son dossier d'en-têtes. `build.rs` le cherche dans l'ordre :

1. `ANON_MUPDF_INCLUDE` (dossier contenant `mupdf/fitz.h`) ;
2. `<target>/<profil>/build/mupdf-sys-*/out/build/include` (copie faite par le script de `mupdf-sys`) ;
3. `$CARGO_HOME/registry/src/*/mupdf-sys-0.8.0/mupdf/include`.

En cas de dépôt vendu (`cargo vendor`) ou de cache Cargo déplacé, poser `ANON_MUPDF_INCLUDE`.
La version de `mupdf-sys` est épinglée par le `Cargo.lock` ; la cale dépend de fonctions internes
de MuPDF (`ocr_init`, `ocr_recognise`, `ocr_fin`) : à revérifier à chaque montée de version.

## Données Tesseract

`assets/tessdata/{fra,eng}.traineddata` sont embarquées par `include_bytes!` (5,2 Mo).
À chaque traitement qui fait de l'OCR, elles sont écrites dans un dossier temporaire privé
(`anon-tessdata-*`, créé par `tempfile`), supprimé à la fin du traitement. Aucune image ni aucun
texte reconnu ne touche le disque.

## Commandes

```powershell
. .\tools\dev-env.ps1
cargo test -p anon-pdf --target-dir target-pdf
cargo clippy -p anon-pdf --all-targets --target-dir target-pdf -- -D warnings
```
