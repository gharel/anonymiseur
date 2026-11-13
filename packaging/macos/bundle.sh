#!/usr/bin/env bash
# Construit Anonymiseur.app autour d'un exécutable déjà compilé, puis l'archive en .zip avec ditto.
#
# Usage, depuis la racine du dépôt (macOS seulement : sips, iconutil, codesign, ditto) :
#     packaging/macos/bundle.sh <exécutable> <dossier de sortie> <nom de l'archive sans .zip> [Lite]
# Exemple :
#     packaging/macos/bundle.sh target/release/anonymiseur dist Anonymiseur-macos-arm64
#     packaging/macos/bundle.sh target/release/anonymiseur dist Anonymiseur-Lite-macos-x64 Lite
#
# Variables : VERSION (sinon lue dans Cargo.toml), MACOSX_DEPLOYMENT_TARGET (défaut 13.4).
# Non vérifié sur un Mac au moment de l'écriture : exécuté par .github/workflows/release.yml.
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage : $0 <exécutable> <dossier de sortie> <nom de l'archive> [Lite]" >&2
  exit 2
fi

binaire="$1"
sortie="$2"
archive="$3"
edition="${4:-}"

racine="$(cd "$(dirname "$0")/../.." && pwd)"
plist_modele="$racine/packaging/macos/Info.plist"
icone_source="$racine/assets/charte/icon-1024.png"

if [ -z "${VERSION:-}" ]; then
  # Première clé version = du [workspace.package] de Cargo.toml.
  VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$racine/Cargo.toml" | head -n 1)"
fi
min_macos="${MACOSX_DEPLOYMENT_TARGET:-13.4}"

if [ "$edition" = "Lite" ]; then
  nom="Anonymiseur Lite"
  bundle_id="fr.skazy-formation.anonymiseur-lite"
else
  nom="Anonymiseur"
  bundle_id="fr.skazy-formation.anonymiseur"
fi

[ -f "$binaire" ] || { echo "Exécutable introuvable : $binaire" >&2; exit 1; }
[ -f "$icone_source" ] || { echo "Icône introuvable : $icone_source" >&2; exit 1; }

mkdir -p "$sortie"
app="$sortie/$nom.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# Exécutable.
cp "$binaire" "$app/Contents/MacOS/Anonymiseur"
chmod 755 "$app/Contents/MacOS/Anonymiseur"

# Info.plist à partir du modèle.
sed -e "s/__NAME__/$nom/g" \
    -e "s/__BUNDLE_ID__/$bundle_id/g" \
    -e "s/__VERSION__/$VERSION/g" \
    -e "s/__MIN_MACOS__/$min_macos/g" \
    "$plist_modele" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Icône .icns depuis le PNG 1024 px de la charte.
travail="$(mktemp -d)"
trap 'rm -rf "$travail"' EXIT
iconset="$travail/Anonymiseur.iconset"
mkdir -p "$iconset"
for taille in 16 32 128 256 512; do
  double=$((taille * 2))
  sips -z "$taille" "$taille" "$icone_source" --out "$iconset/icon_${taille}x${taille}.png" > /dev/null
  sips -z "$double" "$double" "$icone_source" --out "$iconset/icon_${taille}x${taille}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Anonymiseur.icns"

# Signature ad hoc du paquet entier (pas de certificat Apple) : obligatoire sur Apple Silicon
# une fois le paquet modifié. Gatekeeper affichera tout de même l'avertissement (README).
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# Archive : ditto garde les attributs étendus et la structure du paquet.
rm -f "$sortie/$archive.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$sortie/$archive.zip"

echo "Paquet : $app"
echo "Archive : $sortie/$archive.zip"
echo "Dépendances dynamiques :"
otool -L "$app/Contents/MacOS/Anonymiseur"
# Autonomie : seules les bibliothèques du système (/usr/lib, /System) sont admises.
hors_systeme="$(otool -L "$app/Contents/MacOS/Anonymiseur" | tail -n +2 | awk '{print $1}' \
  | grep -Ev '^(/usr/lib/|/System/Library/)' || true)"
if [ -n "$hors_systeme" ]; then
  echo "Dépendances hors système :" >&2
  echo "$hors_systeme" >&2
  exit 1
fi
