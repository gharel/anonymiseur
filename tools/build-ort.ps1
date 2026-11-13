# Compile ONNX Runtime depuis les sources en bibliothèques statiques, CRT statique (/MT),
# CPU seul, sans DirectML : l'édition complète se lie alors avec +crt-static et ne dépend
# d'aucune DLL hors système.
#
# Pourquoi : les binaires précompilés téléchargés par ort-sys (2.0.0-rc.15, table
# build/download/dist.tsv) n'existent sous Windows qu'en /MD et avec DirectML ; aucune
# variante ni fonctionnalité ne sélectionne un CRT statique.
#
# Usage, depuis la racine du dépôt :
#     .\tools\build-ort.ps1                 # compile (rien à faire si déjà compilé)
#     .\tools\build-ort.ps1 -Force          # recompile
#     .\tools\build-ort.ps1 -Prune          # supprime sources et build, ne garde que lib\ (cache CI)
# Puis : $env:ORT_LIB_PATH = "<sortie>\lib" (affiché à la fin).
#
# Sortie : une seule bibliothèque <sortie>\lib\onnxruntime.lib, fusion (lib.exe) des
# bibliothèques d'ONNX Runtime et de ses dépendances. ort-sys la lie telle quelle quand elle
# existe ; sa recherche « build complet » ne suit pas la disposition MSBuild de la 1.31
# (model_package\, libutf8_validity.lib) et l'édition de liens échouerait.
#
# Réseau : clonage d'ONNX Runtime et de ses dépendances (FetchContent), Python et cmake
# par uv. Uniquement ici, à la préparation ; jamais à l'exécution de l'application.
#
# Prérequis : Visual Studio 2022 (toolset C++ v143), git, uv. Durée : 30 à 60 min.

param(
    # Version attendue par ort 2.0.0-rc.15 (fonctionnalité api-31, binaires ms@1.31.0).
    [string]$Version = "1.31.0",
    # Dossier de sortie : ignoré par git (/target-*/).
    [string]$OutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")).Path "target-ort"),
    [switch]$Force,
    [switch]$Prune
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# cmake 4 refuse les cmake_minimum_required < 3.5 de certaines dépendances : on reste en 3.x.
$CmakeVersion = "3.31.6"
$PythonVersion = "3.12"

$OutDir = [System.IO.Path]::GetFullPath($OutDir)
$src = Join-Path $OutDir "onnxruntime-$Version"
$buildRoot = Join-Path $OutDir "build"
$cfgDir = Join-Path $buildRoot "Release"
$venv = Join-Path $OutDir "venv"
$libDir = Join-Path $OutDir "lib"                 # valeur de ORT_LIB_PATH
$fused = Join-Path $libDir "onnxruntime.lib"
$stamp = Join-Path $libDir "VERSION"

function Invoke-Checked {
    param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Échec ($LASTEXITCODE) : $Exe $($Arguments -join ' ')" }
}

function Find-MsvcTool {
    param([string]$Name)
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw "Toolset C++ de Visual Studio introuvable." }
    $t = Get-ChildItem (Join-Path $vs "VC\Tools\MSVC\*\bin\Hostx64\x64\$Name") -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $t) { throw "$Name introuvable sous $vs." }
    return $t.FullName
}

function Remove-Intermediates {
    Write-Host "Nettoyage : seul $libDir est conservé."
    foreach ($d in @($buildRoot, $src, $venv)) {
        if (Test-Path $d) { Remove-Item $d -Recurse -Force }
    }
}

if ((Test-Path $fused) -and (Test-Path $stamp) -and ((Get-Content $stamp -Raw).Trim() -eq $Version) -and -not $Force) {
    Write-Host "ONNX Runtime $Version déjà prêt : $fused"
    if ($Prune) { Remove-Intermediates }
    Write-Host "ORT_LIB_PATH=$libDir"
    return
}

New-Item -ItemType Directory -Force $OutDir | Out-Null

# Variables qui fausseraient le build CMake (posées par dev-env.ps1 -StaticCrt).
Remove-Item Env:_CL_ -ErrorAction SilentlyContinue
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue

$jobs = [Environment]::ProcessorCount
$common = Join-Path $cfgDir "Release\onnxruntime_common.lib"
$re2 = Join-Path $cfgDir "_deps\re2-build\Release\re2.lib"

if ($Force -or -not (Test-Path $common) -or -not (Test-Path $re2)) {
    # Python et cmake dans un environnement privé (aucun droit administrateur requis).
    if (-not (Test-Path (Join-Path $venv "Scripts\python.exe"))) {
        Invoke-Checked uv @("venv", "--python", $PythonVersion, $venv)
    }
    $python = Join-Path $venv "Scripts\python.exe"
    Invoke-Checked uv @("pip", "install", "--python", $python, "cmake==$CmakeVersion")
    $env:PATH = "$(Join-Path $venv 'Scripts');$env:PATH"
    Invoke-Checked cmake @("--version")

    if (-not (Test-Path (Join-Path $src ".git"))) {
        # core.autocrlf=false : les correctifs cmake/patches/*.patch doivent garder leurs fins de ligne
        # Unix, comme les sources téléchargées qu'ils modifient ; convertis en CRLF, patch les rejette.
        Invoke-Checked git @("-c", "advice.detachedHead=false", "-c", "core.autocrlf=false", "clone", "--depth", "1", "--branch", "v$Version",
            "--recurse-submodules", "--shallow-submodules", "https://github.com/microsoft/onnxruntime", $src)
    }
    if ($Force -and (Test-Path $buildRoot)) { Remove-Item $buildRoot -Recurse -Force }

    # Le générateur « Visual Studio 17 2022 » exige Visual Studio 2022 ; cmake 3.x ne connaît pas
    # Visual Studio 2026 (seul présent sur windows-latest de GitHub Actions : prendre windows-2022).
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    $vs2022 = & $vswhere -version "[17.0,18.0)" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs2022) {
        throw "Visual Studio 2022 avec le toolset C++ est requis (introuvable). Sur GitHub Actions : runs-on: windows-2022."
    }

    # Les correctifs d'ONNX Runtime sur ses dépendances (onnx…) s'appliquent avec le patch.exe de Git.
    # Celui de Strawberry Perl, trouvé en premier sur les machines GitHub Actions, en rejette des
    # morceaux (scoped_resource.h). On impose donc celui de Git.
    # Git for Windows dans Program Files (là où CMake le cherche aussi), sinon le git du PATH.
    $candidats = @(
        (Join-Path $env:ProgramFiles "Git\usr\bin\patch.exe"),
        (Join-Path (Split-Path (Split-Path (Split-Path (git --exec-path)))) "usr\bin\patch.exe")
    )
    $patchExe = $candidats | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $patchExe) { throw "patch.exe de Git for Windows introuvable : $($candidats -join ', ')." }
    $patchExe = $patchExe -replace '\\', '/'
    Write-Host "patch : $patchExe"

    $buildArgs = @(
        (Join-Path $src "tools\ci_build\build.py"),
        "--build_dir", $buildRoot,
        "--config", "Release",
        "--update", "--build",
        "--parallel", "$jobs",
        "--skip_tests",
        "--skip_submodule_sync",
        "--cmake_generator", "Visual Studio 17 2022",
        # CRT statique pour ONNX Runtime et toutes ses dépendances FetchContent.
        "--enable_msvc_static_runtime",
        # Opérateurs ML classiques (arbres, SVM…) inutiles pour les modèles de noms.
        # Les opérateurs contrib (com.microsoft, dont MatMulNBits) restent : fr.onnx en a besoin.
        "--disable_ml_ops",
        "--compile_no_warning_as_error",
        # Télémétrie 1DS (envoi réseau) activée par défaut depuis 1.30 : interdite (invariant 1).
        "--no_telemetry",
        "--cmake_extra_defines",
        "onnxruntime_BUILD_UNIT_TESTS=OFF",
        # Pas de détection GPU/NPU (DXCore, d3d12, dxgi, setupapi) : seul le CPU est utilisé.
        # Sans cela, device_discovery.cc exige le SDK Windows 10.0.22621+ (DXCoreHardwareIDParts).
        "onnxruntime_DISABLE_DEVICE_DISCOVERY=ON",
        "onnxruntime_USE_TELEMETRY=OFF",
        "onnxruntime_BUILD_BENCHMARKS=OFF",
        "Patch_EXECUTABLE=$patchExe"
    )
    Push-Location $src
    try {
        Invoke-Checked $python $buildArgs
        # re2 (opérateur RegexFullMatch) n'est pas dans ALL_BUILD : on le compile à part.
        Invoke-Checked cmake @("--build", (Join-Path $cfgDir "_deps\re2-build"), "--config", "Release",
            "--target", "re2", "--parallel", "$jobs")
    } finally {
        Pop-Location
    }
}

# Fusion en une seule onnxruntime.lib.
$parts = @()
$parts += Get-ChildItem (Join-Path $cfgDir "Release\onnxruntime_*.lib") |
    Where-Object { $_.Name -ne "onnxruntime_providers_shared.lib" }   # fournisseurs en DLL : non utilisé
$deps = Join-Path $cfgDir "_deps"
$parts += Get-Item (Join-Path $cfgDir "model_package\Release\model_package.lib")
$parts += Get-ChildItem (Join-Path $deps "abseil_cpp-build") -Recurse -Filter "absl_*.lib"
$parts += Get-Item @(
    (Join-Path $deps "onnx-build\Release\onnx.lib"),
    (Join-Path $deps "onnx-build\Release\onnx_proto.lib"),
    (Join-Path $deps "protobuf-build\Release\libprotobuf-lite.lib"),
    (Join-Path $deps "protobuf-build\third_party\utf8_range\Release\libutf8_validity.lib"),
    (Join-Path $deps "protobuf-build\third_party\utf8_range\Release\libutf8_range.lib"),
    (Join-Path $deps "pytorch_cpuinfo-build\Release\cpuinfo.lib"),
    (Join-Path $deps "re2-build\Release\re2.lib"),
    (Join-Path $deps "flatbuffers-build\Release\flatbuffers.lib")
)
New-Item -ItemType Directory -Force $libDir | Out-Null
$rsp = Join-Path $libDir "onnxruntime.rsp"
$lines = @("/NOLOGO", "/MACHINE:X64", "/IGNORE:4006,4221", "/OUT:`"$fused`"") + ($parts | ForEach-Object { "`"$($_.FullName)`"" })
Set-Content -Path $rsp -Value $lines -Encoding ascii
Write-Host "Fusion de $($parts.Count) bibliothèques dans $fused"
Invoke-Checked (Find-MsvcTool "lib.exe") @("@$rsp")
Remove-Item $rsp
Set-Content -Path $stamp -Value $Version -Encoding ascii

if ($Prune) { Remove-Intermediates }

Write-Host ""
Write-Host ("ONNX Runtime $Version (statique, /MT, CPU seul) : {0:N0} Mo" -f ((Get-Item $fused).Length / 1MB))
Write-Host "ORT_LIB_PATH=$libDir"