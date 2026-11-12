# Réglages de build Windows (MSVC) pour mupdf-sys, sans LLVM installé sur le système.
#
# Usage, depuis la racine du dépôt :
#     . .\tools\dev-env.ps1            # CRT dynamique (développement)
#     . .\tools\dev-env.ps1 -StaticCrt # CRT statique (exécutable livré)
#
# Prérequis : `uv venv tools/.venv` puis `uv pip install --python tools/.venv libclang`.
# Voir crates/anon-pdf/BUILD.md.

param(
    [switch]$StaticCrt
)

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$libclang = Join-Path $root "tools\.venv\Lib\site-packages\clang\native"
if (-not (Test-Path (Join-Path $libclang "libclang.dll"))) {
    Write-Warning "libclang.dll introuvable dans $libclang : lancez 'uv venv tools/.venv' puis 'uv pip install --python tools/.venv libclang'."
}

# bindgen (mupdf-sys) trouve libclang ici.
$env:LIBCLANG_PATH = $libclang

# Le libclang du paquet pip n'a pas les en-têtes de ressources de clang : max_align_t
# manque dans les liaisons générées (erreur E0425 dans mupdf/src/device/native.rs).
$maxAlign = (Join-Path $root "tools\bindgen\max_align_t.h") -replace '\\', '/'
$env:BINDGEN_EXTRA_CLANG_ARGS = "-include $maxAlign"

if ($StaticCrt) {
    # Les .vcxproj de MuPDF compilent en /MD ; cl.exe ajoute le contenu de _CL_ après
    # les options du projet, donc /MT l'emporte. Rust doit suivre (+crt-static).
    # _CL_ n'est pas suivie par cargo : `cargo clean -p mupdf-sys --release` une fois.
    $env:_CL_ = "/MT"
    $env:RUSTFLAGS = "-C target-feature=+crt-static"
} else {
    Remove-Item Env:_CL_ -ErrorAction SilentlyContinue
}

Write-Host "LIBCLANG_PATH=$env:LIBCLANG_PATH"
Write-Host "BINDGEN_EXTRA_CLANG_ARGS=$env:BINDGEN_EXTRA_CLANG_ARGS"
if ($StaticCrt) { Write-Host "_CL_=/MT, RUSTFLAGS=$env:RUSTFLAGS" }
