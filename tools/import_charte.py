#!/usr/bin/env python3
"""Importe la charte graphique Skazy Formation dans assets/charte/.

Source en lecture seule : le dépôt design-skazy (AGENTS.md, invariant 7).
Chemin : --source, sinon variable DESIGN_SKAZY, sinon ../design-skazy.

Sorties (déterministes) : tokens.json, fonts/Georama-*.ttf, logos, icônes
d'application, icons/*.svg (Lucide 0.460.0), LISEZMOI.md.

    uv run --project tools python tools/import_charte.py [--source CHEMIN] [--date AAAA-MM-JJ]
    uv run --project tools python tools/import_charte.py --check
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import sys
import urllib.request
from pathlib import Path

from fontTools.ttLib import TTFont
from PIL import Image, ImageChops, ImageDraw

RACINE = Path(__file__).resolve().parent.parent
SORTIE = RACINE / "assets" / "charte"

LUCIDE_VERSION = "0.460.0"
LUCIDE_URL = f"https://unpkg.com/lucide-static@{LUCIDE_VERSION}"
ICONES = [
    "upload", "file-text", "file-spreadsheet", "presentation", "image", "file",
    "shield-check", "lock", "lock-open", "eye", "eye-off", "check", "x", "plus",
    "trash-2", "copy", "clipboard", "folder-open", "triangle-alert", "info",
    "search", "settings", "refresh-cw", "download", "key-round", "chevron-down",
    "chevron-right", "circle-help", "sparkles", "undo-2",
    # Thème clair, sombre ou système.
    "sun", "moon", "monitor",
    # Raccourcis, tri, presse-papiers, états des fichiers, états vides.
    "keyboard", "chevron-up", "chevrons-up-down", "clipboard-paste", "circle-check",
    "circle-x", "clock", "loader-circle", "inbox", "arrow-down",
]
POIDS = [400, 500, 600, 700, 800]
GLYPHES_REQUIS = "éèêàçœÉ’"

LICENCE_ISC = """ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
"""

LISEZMOI = """# Charte Skazy Formation (dossier généré)

Dossier généré par `tools/import_charte.py`, ne pas éditer à la main.
Source : design-skazy (lecture seule, voir invariant 7 de `AGENTS.md`).
Pour le régénérer : `uv run --project tools python tools/import_charte.py`.

## Contenu

- `tokens.json` : couleurs, rayons, espacements (px), ombres, tailles de texte (px).
- `fonts/Georama-400.ttf` à `Georama-800.ttf` : police de l'interface (converties du WOFF2).
- `logo-color.png`, `logo-white.png` : logos sans marge (fond clair, fond foncé).
- `logo-white-color.png` : logo sans marge pour le thème sombre (« skazy » blanc, « formation » vert).
- `icon.ico`, `icon-256.png`, `icon-1024.png` : icône de l'application.
- `icons/*.svg` : icônes Lucide 0.460.0 (licence ISC dans `icons/LICENSE`).

## Règles d'usage à respecter côté interface

Reprises du `readme.md` de la charte.

### Couleurs et contrastes
- Texte courant : `color-text-primary` (anthracite `#4a4a4a`). Texte secondaire : `color-text-secondary` (`#676f77`, AA).
- La couleur de marque `color-primary` (`#50967c`, 3,50:1 sur blanc) sert aux aplats, au logo, au décor et aux grands titres sur fond sombre. Jamais en texte courant, ni en fond de petit texte blanc.
- Liens, texte vert, fond des boutons primaires : `color-primary-dark` (`#3d7a62`, 5,05:1 avec du blanc). Survol : `color-primary-darker`. Les liens sont soulignés.
- `color-gray-500` est réservé au décor, jamais au texte.
- Texte d'erreur : `color-error`. `color-success`, `color-warning` et `color-info` servent de fonds ou d'icônes, jamais de texte.
- Bord des champs : `color-border-input` ; anneau de focus de 2 px en `color-border-focus`.
- Contraste minimal AA : 4,5:1 pour le texte courant, 3:1 pour les grands textes (24 px et plus, ou 18,66 px et plus en gras) et pour les éléments d'interface (bords de champ, icônes, focus).
- Une couleur de catégorie n'est jamais une couleur de texte : utiliser `-ink`. Petit texte sur une catégorie : couleur de base + `-on` anthracite (Data, Gestion de projet, Graphisme & UX, Marketing, IA, NoCode), ou fond `-ink` + texte blanc (Bureautique, Développement, Web).
- Fond de page : blanc ; fond sombre : `color-dark-3` (`#1a1a1a`).

### Formes
- Boutons en pilule (`radius-full`, 9999 px), toujours arrondis, jamais anguleux. Variantes : primaire (rempli), secondaire (contour), fantôme (texte seul), sombre. Primaire : fond `color-primary-dark` + texte blanc.
- Cartes : `radius-xl` (16 px), ombre `md`, fond blanc, marge intérieure de 20 px. Modales : ombre `xl`.
- Rayons : `radius-sm` 4, `md` 8, `lg` 12, `xl` 16, `2xl` 24, `3xl` 32 px.
- Transitions courtes : 150 ms (micro-interactions), 250 ms (survol). Pas d'animation décorative en boucle.

### Icônes
- Lucide uniquement, version 0.460.0, traits (non remplies). Tailles : 16 px (en ligne), 20 px (boutons), 24 px (seules).
- Aucun emoji, aucun symbole unicode en guise d'icône.

### Textes
- Titres : majuscule initiale uniquement (« Nos formations »).
- Badges : MAJUSCULES avec espacement des lettres (« DÉBUTANT »).
- Boutons : première lettre majuscule (« Voir les formations »).
- Vouvoiement : toujours « vous » dans l'interface.
- Français, ton direct, phrases courtes, verbes d'action.
"""


# --------------------------------------------------------------------------
# Jetons CSS
# --------------------------------------------------------------------------

def lire_variables(chemin: Path) -> dict[str, str]:
    """Retourne {nom sans --: valeur brute} dans l'ordre du fichier."""
    texte = re.sub(r"/\*.*?\*/", "", chemin.read_text(encoding="utf-8"), flags=re.S)
    return {m.group(1): m.group(2).strip()
            for m in re.finditer(r"--([\w-]+)\s*:\s*([^;]+);", texte)}


def resoudre(nom: str, variables: dict[str, str], pile: tuple[str, ...] = ()) -> str:
    if nom in pile:
        raise ValueError(f"référence circulaire : {nom}")
    valeur = variables[nom]
    return re.sub(r"var\(--([\w-]+)\)",
                  lambda m: resoudre(m.group(1), variables, pile + (nom,)), valeur)


def px(valeur: str) -> int | float:
    v = valeur.strip()
    if v == "0":
        return 0
    m = re.fullmatch(r"(-?[\d.]+)(px|rem)", v)
    if not m:
        raise ValueError(f"dimension inattendue : {valeur}")
    n = float(m.group(1)) * (16 if m.group(2) == "rem" else 1)
    return int(n) if n == int(n) else n


def couleur_rgba(texte: str) -> str:
    m = re.fullmatch(r"rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)", texte.strip())
    if not m:
        raise ValueError(f"couleur d'ombre inattendue : {texte}")
    r, g, b = (int(m.group(i)) for i in (1, 2, 3))
    return f"#{r:02x}{g:02x}{b:02x}{round(float(m.group(4)) * 255):02x}"


def ombre(valeur: str) -> list[dict]:
    couches = []
    for couche in re.split(r",\s*(?![^()]*\))", valeur):
        m = re.fullmatch(r"\s*(-?[\d.]+)(?:px)?\s+(-?[\d.]+)(?:px)?\s+([\d.]+)(?:px)?\s+(rgba\([^)]*\))\s*", couche)
        if not m:
            raise ValueError(f"ombre inattendue : {couche}")
        x, y, flou = (px(f"{m.group(i)}px") for i in (1, 2, 3))
        r = re.findall(r"[\d.]+", m.group(4))
        couches.append({
            "x": x, "y": y, "flou": flou,
            "couleur": couleur_rgba(m.group(4)),
            "rgba": [int(r[0]), int(r[1]), int(r[2]), float(r[3])],
        })
    return couches


def construire_tokens(source: Path, date: str | None) -> dict:
    t = source / "tokens"
    couleurs_v = lire_variables(t / "colors.css")
    typo_v = lire_variables(t / "typography.css")
    esp_v = lire_variables(t / "spacing.css")

    couleurs = {}
    for nom in couleurs_v:
        val = resoudre(nom, couleurs_v).strip()
        if not re.fullmatch(r"#[0-9a-fA-F]{6}", val):
            raise ValueError(f"couleur non hexadécimale : {nom} = {val}")
        couleurs[nom] = val.lower()

    rayons = {n.removeprefix("radius-"): px(resoudre(n, esp_v))
              for n in esp_v if n.startswith("radius-")}
    espacements = {n.removeprefix("space-"): px(resoudre(n, esp_v))
                   for n in esp_v if n.startswith("space-")}
    for n in esp_v:
        if n.startswith("container-") or n == "header-height":
            espacements[n] = px(resoudre(n, esp_v))
    ombres = {n.removeprefix("shadow-"): ombre(resoudre(n, esp_v))
              for n in esp_v if n.startswith("shadow-")}
    texte = {n.removeprefix("text-"): px(resoudre(n, typo_v))
             for n in typo_v if n.startswith("text-") and n.endswith(("xs", "sm", "base", "md", "lg", "xl", "2xl", "3xl", "4xl", "5xl", "size"))
             and re.fullmatch(r"-?[\d.]+(px|rem)", resoudre(n, typo_v).strip())}

    tokens = {"couleurs": couleurs, "rayons": rayons, "espacements": espacements,
              "ombres": ombres, "texte": texte, "source": "design-skazy"}
    if date:
        tokens["genere_le"] = date
    return tokens


# --------------------------------------------------------------------------
# Polices
# --------------------------------------------------------------------------

def manquants(font: TTFont) -> list[str]:
    cmap = font.getBestCmap() or {}
    return [c for c in GLYPHES_REQUIS if ord(c) not in cmap]


def convertir_polices(source: Path) -> None:
    dest = SORTIE / "fonts"
    dest.mkdir(parents=True, exist_ok=True)
    for p in POIDS:
        src = source / "assets" / "fonts" / f"georama-{p}.woff2"
        # Horodatage de la source conservé : sans cela, chaque import changerait les TTF.
        police = TTFont(src, recalcTimestamp=False)
        absents = manquants(police)
        if absents:
            raise SystemExit(f"{src.name} : glyphes absents {absents}")
        police.flavor = None
        police.save(dest / f"Georama-{p}.ttf")


# --------------------------------------------------------------------------
# Images
# --------------------------------------------------------------------------

def sous_chemins(d: str) -> list[list[tuple[float, float]]]:
    resultat = []
    for seg in re.findall(r"M[^MZ]*Z?", d):
        nombres = [float(n) for n in re.findall(r"-?\d+(?:\.\d+)?", seg)]
        pts = list(zip(nombres[0::2], nombres[1::2]))
        if len(pts) >= 3:
            resultat.append(pts)
    return resultat


def rendre_icone(svg: Path, taille: int) -> Image.Image:
    """Rend favicon.svg (carré arrondi vert + glyphe blanc, chemin M/L/Z) sans moteur SVG."""
    texte = svg.read_text(encoding="utf-8")
    d = re.search(r'\sd="([^"]+)"', texte).group(1)
    tx, ty, ech = (float(v) for v in re.search(
        r"translate\(([\d.]+) ([\d.]+)\) scale\(([\d.]+)\)", texte).groups())
    rx = float(re.search(r'rx="([\d.]+)"', texte).group(1))
    fond = re.search(r'<rect[^>]*fill="(#[0-9a-fA-F]{6})"', texte).group(1)
    ss = 4
    n = taille * ss
    k = n / 512
    masque_fond = Image.new("L", (n, n), 0)
    ImageDraw.Draw(masque_fond).rounded_rectangle((0, 0, n - 1, n - 1), radius=rx * k, fill=255)
    glyphe = Image.new("L", (n, n), 0)
    for pts in sous_chemins(d):  # règle pair-impair : XOR des sous-chemins
        m = Image.new("L", (n, n), 0)
        ImageDraw.Draw(m).polygon([((tx + x * ech) * k, (ty + y * ech) * k) for x, y in pts], fill=255)
        glyphe = ImageChops.difference(glyphe, m)
    img = Image.new("RGBA", (n, n), fond)
    img.paste((255, 255, 255, 255), mask=glyphe)
    img.putalpha(masque_fond)
    return img.resize((taille, taille), Image.LANCZOS)


def copier_images(source: Path) -> None:
    a = source / "assets"
    SORTIE.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(a / "logo-color-crop.png", SORTIE / "logo-color.png")
    shutil.copyfile(a / "logo-white-crop.png", SORTIE / "logo-white.png")
    shutil.copyfile(a / "logo-white-color-crop.png", SORTIE / "logo-white-color.png")
    shutil.copyfile(a / "favicon" / "favicon.ico", SORTIE / "icon.ico")
    for t in (256, 1024):
        rendre_icone(a / "favicon" / "favicon.svg", t).save(SORTIE / f"icon-{t}.png", optimize=True)


def telecharger(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "import-charte"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def icones_lucide() -> None:
    dest = SORTIE / "icons"
    dest.mkdir(parents=True, exist_ok=True)
    for nom in ICONES:
        f = dest / f"{nom}.svg"
        if not f.exists():
            f.write_bytes(telecharger(f"{LUCIDE_URL}/icons/{nom}.svg"))
    licence = dest / "LICENSE"
    try:
        # Lucide publie sa licence dans le paquet ; sinon texte ISC embarqué.
        brut = telecharger(f"{LUCIDE_URL}/LICENSE").decode("utf-8")
        licence.write_text(brut.replace("\r\n", "\n"), encoding="utf-8", newline="\n")
    except Exception:
        licence.write_text(LICENCE_ISC, encoding="utf-8", newline="\n")


# --------------------------------------------------------------------------
# Vérification
# --------------------------------------------------------------------------

def verifier() -> int:
    erreurs: list[str] = []
    ok: list[str] = []

    def ko(msg: str) -> None:
        erreurs.append(msg)

    try:
        tokens = json.loads((SORTIE / "tokens.json").read_text(encoding="utf-8"))
    except Exception as e:
        print(f"ECHEC tokens.json illisible : {e}")
        return 1
    for cle in ("couleurs", "rayons", "espacements", "ombres", "texte", "source"):
        if cle not in tokens or not tokens[cle]:
            ko(f"tokens.json : section « {cle} » absente ou vide")
    couleurs = tokens.get("couleurs", {})
    mauvaises = [k for k, v in couleurs.items() if not re.fullmatch(r"#[0-9a-f]{6}", str(v))]
    if mauvaises:
        ko(f"couleurs hors format #rrggbb : {mauvaises}")
    if "var(" in (SORTIE / "tokens.json").read_text(encoding="utf-8"):
        ko("référence var( restante dans tokens.json")
    ok.append(f"tokens.json : {len(couleurs)} couleurs, {len(tokens.get('rayons', {}))} rayons, "
              f"{len(tokens.get('espacements', {}))} espacements, {len(tokens.get('ombres', {}))} ombres, "
              f"{len(tokens.get('texte', {}))} tailles")

    for p in POIDS:
        f = SORTIE / "fonts" / f"Georama-{p}.ttf"
        try:
            police = TTFont(f)
            absents = manquants(police)
            if absents:
                ko(f"{f.name} : glyphes absents {absents}")
        except Exception as e:
            ko(f"{f.name} illisible : {e}")
    ok.append(f"polices : {len(POIDS)} TTF ouvrables, glyphes {GLYPHES_REQUIS} présents")

    for nom, taille in (("logo-color.png", None), ("logo-white.png", None),
                        ("logo-white-color.png", None),
                        ("icon-256.png", (256, 256)), ("icon-1024.png", (1024, 1024)),
                        ("icon.ico", None)):
        try:
            with Image.open(SORTIE / nom) as im:
                im.load()
                if taille and im.size != taille:
                    ko(f"{nom} : taille {im.size}, attendu {taille}")
        except Exception as e:
            ko(f"{nom} illisible : {e}")
    ok.append("images : logos, icon.ico, icon-256.png, icon-1024.png ouvrables")

    for nom in ICONES:
        f = SORTIE / "icons" / f"{nom}.svg"
        try:
            import xml.etree.ElementTree as ET
            racine = ET.parse(f).getroot()
            if not racine.tag.endswith("svg") or "viewBox" not in racine.attrib:
                ko(f"icons/{nom}.svg : racine svg ou viewBox absente")
        except Exception as e:
            ko(f"icons/{nom}.svg absente ou mal formée : {e}")
    if not (SORTIE / "icons" / "LICENSE").is_file():
        ko("icons/LICENSE absent")
    if not (SORTIE / "LISEZMOI.md").is_file():
        ko("LISEZMOI.md absent")
    ok.append(f"icônes : {len(ICONES)} SVG bien formés + LICENSE + LISEZMOI.md")

    for ligne in ok:
        print("OK    " + ligne)
    for e in erreurs:
        print("ECHEC " + e)
    print("check : " + ("réussi" if not erreurs else f"{len(erreurs)} erreur(s)"))
    return 1 if erreurs else 0


# --------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--source", help="dossier design-skazy (sinon $DESIGN_SKAZY, sinon ../design-skazy)")
    ap.add_argument("--date", help="valeur fixe de genere_le (omise par défaut)")
    ap.add_argument("--check", action="store_true", help="valide les sorties sans rien générer")
    args = ap.parse_args()

    if args.check:
        return verifier()

    source = Path(args.source or os.environ.get("DESIGN_SKAZY") or RACINE.parent / "design-skazy").resolve()
    if not (source / "tokens" / "colors.css").is_file():
        print(f"Source introuvable : {source}", file=sys.stderr)
        return 2

    SORTIE.mkdir(parents=True, exist_ok=True)
    tokens = construire_tokens(source, args.date)
    (SORTIE / "tokens.json").write_text(
        json.dumps(tokens, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    convertir_polices(source)
    copier_images(source)
    icones_lucide()
    (SORTIE / "LISEZMOI.md").write_text(LISEZMOI, encoding="utf-8", newline="\n")
    print(f"Charte importée depuis {source} vers {SORTIE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
