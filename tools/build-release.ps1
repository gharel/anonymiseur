# Construit les deux exécutables Windows livrés, en CRT statique :
#     dist\Anonymiseur.exe       édition complète (IA de noms, ONNX Runtime lié statiquement)
#     dist\Anonymiseur-Lite.exe  édition Lite (--no-default-features)
# Affiche taille, SHA-256 et DLL importées de chacun ; échoue si une DLL hors système apparaît
# (CRT Visual C++, DirectML, ONNX Runtime…). Écrit dist\SHA256SUMS.
#
# Usage, depuis la racine du dépôt :
#     .\tools\build-release.ps1
#     .\tools\build-release.ps1 -Edition Lite          # Lite seule (pas besoin d'ONNX Runtime ni des modèles)
#     .\tools\build-release.ps1 -CheckOnly             # vérifie les exécutables déjà dans dist\
#
# Prérequis : ceux de crates/anon-pdf/BUILD.md (libclang dans tools\.venv), et pour l'édition
# complète les modèles dans assets\ner\ (voir assets\ner\LISEZMOI.md) et ONNX Runtime compilé
# par tools\build-ort.ps1 (lancé ici s'il manque ; ORT_LIB_PATH déjà posée est respectée).

param(
    [ValidateSet("Toutes", "Complete", "Lite")]
    [string]$Edition = "Toutes",
    [string]$TargetDir,
    [string]$DistDir,
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $TargetDir) { $TargetDir = Join-Path $root "target-dist" }
if (-not $DistDir) { $DistDir = Join-Path $root "dist" }

$artefacts = [ordered]@{}
if ($Edition -in @("Toutes", "Complete")) { $artefacts["Anonymiseur.exe"] = "Complete" }
if ($Edition -in @("Toutes", "Lite")) { $artefacts["Anonymiseur-Lite.exe"] = "Lite" }

function Invoke-Checked {
    param([string]$Exe, [string[]]$Arguments)
    Write-Host "> $Exe $($Arguments -join ' ')"
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Échec ($LASTEXITCODE) : $Exe $($Arguments -join ' ')" }
}

function Find-Dumpbin {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) { throw "vswhere.exe introuvable : Visual Studio 2022 (toolset C++) est requis." }
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw "Toolset C++ de Visual Studio introuvable." }
    $candidats = Get-ChildItem (Join-Path $vs "VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe") -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending
    if (-not $candidats) { throw "dumpbin.exe introuvable sous $vs." }
    return $candidats[0].FullName
}

# DLL importées par un exécutable (section « Image has the following dependencies »).
function Get-Imports {
    param([string]$Dumpbin, [string]$Exe)
    $sortie = & $Dumpbin /nologo /dependents $Exe
    if ($LASTEXITCODE -ne 0) { throw "dumpbin a échoué sur $Exe" }
    $dans = $false
    $dlls = @()
    foreach ($ligne in $sortie) {
        $t = $ligne.Trim()
        if ($t -like "Image has the following*dependencies*") { $dans = $true; continue }
        if ($dans) {
            if ($t -eq "") { if ($dlls.Count -gt 0) { $dans = $false }; continue }
            if ($t -like "Summary*") { break }
            if ($t -match '\.dll$') { $dlls += $t }
        }
    }
    return $dlls
}

# Une DLL est admise si elle fait partie de Windows (System32 ou jeu d'API) et n'est pas
# un composant redistribuable (CRT Visual C++, ONNX Runtime, DirectML) : ceux-là peuvent
# se trouver dans System32 sur un poste de développement sans y être sur celui de l'utilisateur.
$Refusees = '^(vcruntime|msvcp|ucrtbase|api-ms-win-crt-|concrt|vccorlib|vcomp|libomp|directml|onnxruntime|libiomp)'
function Test-SystemDll {
    param([string]$Name)
    $n = $Name.ToLowerInvariant()
    if ($n -match $Refusees) { return $false }
    if ($n -match '^(api-ms-win-|ext-ms-win-)') { return $true }
    return (Test-Path (Join-Path $env:SystemRoot "System32\$Name"))
}

function Test-Models {
    $ner = Join-Path $root "assets\ner"
    $sums = Join-Path $ner "SHA256SUMS"
    foreach ($ligne in Get-Content $sums) {
        if ($ligne -notmatch '^([0-9a-f]{64})\s+\*?(.+)$') { continue }
        $attendu = $Matches[1]; $nom = $Matches[2].Trim()
        $fichier = Join-Path $ner $nom
        if (-not (Test-Path $fichier)) {
            throw "Modèle manquant : assets\ner\$nom. Récupérez-le (release modeles-v1) ou préparez-le (assets\ner\LISEZMOI.md)."
        }
        $obtenu = (Get-FileHash $fichier -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($obtenu -ne $attendu) { throw "Empreinte inattendue pour assets\ner\$nom : $obtenu (attendu $attendu)." }
        Write-Host "Modèle vérifié : $nom"
    }
}

if (-not $CheckOnly) {
    # libclang, bindgen, _CL_=/MT et RUSTFLAGS=+crt-static.
    . (Join-Path $PSScriptRoot "dev-env.ps1") -StaticCrt

    # _CL_ n'est pas suivie par Cargo : on purge mupdf-sys une fois pour ce dossier de build,
    # au cas où il aurait été compilé en /MD. Le témoin évite de recompiler MuPDF à chaque fois.
    $temoin = Join-Path $TargetDir "release\.crt-static"
    if (-not (Test-Path $temoin)) {
        if (Test-Path (Join-Path $TargetDir "release")) {
            Invoke-Checked cargo @("clean", "-p", "mupdf-sys", "--release", "--target-dir", $TargetDir)
        }
        New-Item -ItemType Directory -Force (Split-Path $temoin) | Out-Null
        Set-Content $temoin "MuPDF compilé avec _CL_=/MT"
    }

    $commun = @("build", "-p", "anonymiseur", "--release", "--locked", "--target-dir", $TargetDir)
    New-Item -ItemType Directory -Force $DistDir | Out-Null
    $exeCargo = Join-Path $TargetDir "release\anonymiseur.exe"

    foreach ($nom in $artefacts.Keys) {
        if ($artefacts[$nom] -eq "Complete") {
            Test-Models
            if (-not $env:ORT_LIB_PATH) {
                & (Join-Path $PSScriptRoot "build-ort.ps1")
                if (-not $?) { throw "tools\build-ort.ps1 a échoué." }
                $env:ORT_LIB_PATH = Join-Path $root "target-ort\lib"
                # build-ort.ps1 retire _CL_ et RUSTFLAGS pour son propre build : on les remet.
                . (Join-Path $PSScriptRoot "dev-env.ps1") -StaticCrt
            }
            Write-Host "ORT_LIB_PATH=$env:ORT_LIB_PATH"
            Invoke-Checked cargo $commun
        } else {
            Invoke-Checked cargo ($commun + @("--no-default-features"))
        }
        Copy-Item $exeCargo (Join-Path $DistDir $nom) -Force
    }
}

# Vérification et rapport.
$dumpbin = Find-Dumpbin
$echec = $false
$empreintes = @()
foreach ($nom in $artefacts.Keys) {
    $exe = Join-Path $DistDir $nom
    if (-not (Test-Path $exe)) { throw "$exe absent." }
    $taille = (Get-Item $exe).Length
    $hash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    $empreintes += "$hash  $nom"
    Write-Host ""
    Write-Host "== $nom"
    Write-Host ("Taille  : {0:N1} Mo ({1} octets)" -f ($taille / 1MB), $taille)
    Write-Host "SHA-256 : $hash"
    Write-Host "DLL importées :"
    foreach ($dll in Get-Imports -Dumpbin $dumpbin -Exe $exe) {
        if (Test-SystemDll $dll) {
            Write-Host "  $dll"
        } else {
            Write-Host "  $dll  <-- HORS SYSTÈME" -ForegroundColor Red
            $echec = $true
        }
    }
}

if ($Edition -eq "Toutes") {
    [IO.File]::WriteAllText((Join-Path $DistDir "SHA256SUMS"), (($empreintes -join "`n") + "`n"))
}

if ($echec) { throw "Au moins un exécutable dépend d'une DLL hors système : il ne serait pas autonome." }
Write-Host ""
Write-Host "Exécutables autonomes dans $DistDir"
