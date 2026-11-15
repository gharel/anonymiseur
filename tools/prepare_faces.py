#!/usr/bin/env python3
"""Prépare le modèle de détection des visages (`assets/faces/`) et les images de test
de `crates/anon-ner/tests/fixtures/`.

Usage :
    uv run --project tools tools/prepare_faces.py              # modèle seulement
    uv run --project tools tools/prepare_faces.py --fixtures   # modèle et images de test

Seul endroit où le réseau sert (jamais dans l'application). Les sources sont figées :
commit d'OpenCV Zoo et empreinte SHA-256 du modèle ; révision Wikimedia Commons
(empreinte SHA-1 de l'original) pour les peintures du domaine public.

Le modèle et sa licence sont versionnés (230 Ko) : ce script ne sert qu'à les
regénérer ou à vérifier leur provenance.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets" / "faces"
FIXTURES = ROOT / "crates" / "anon-ner" / "tests" / "fixtures"

# Client générique : aucune information sur le poste ni sur le projet.
USER_AGENT = "prepare-faces-script/1.0 (offline tool, test fixtures)"
SOCKET_TIMEOUT_S = 60
MAX_ATTEMPTS = 4
RETRY_DELAY_S = 3

# --- Modèle : YuNet, OpenCV Zoo (licence MIT) --------------------------------------
# `face_detection_yunet_2026may.onnx` : mêmes poids que `2023mar`, réexporté avec une
# entrée de taille libre (dimensions symboliques `height` et `width`).
ZOO_COMMIT = "eb9f4f2d4b79b58e5513b5d25d5f0243a8357bf4"
ZOO_DIR = "models/face_detection_yunet"
MODEL_NAME = "face_detection_yunet_2026may.onnx"
MODEL_URL = f"https://media.githubusercontent.com/media/opencv/opencv_zoo/{ZOO_COMMIT}/{ZOO_DIR}/{MODEL_NAME}"
MODEL_SHA256 = "ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0"
MODEL_SIZE = 229_738
LICENSE_URL = f"https://raw.githubusercontent.com/opencv/opencv_zoo/{ZOO_COMMIT}/{ZOO_DIR}/LICENSE"
LICENSE_SHA256 = "c83b8120c50ccbd4c4f96edf53141bdd566ebb8f8e9227e415326aa1b1aba958"

# --- Images de test : peintures du domaine public (Wikimedia Commons, PD-Art) -----------
COMMONS_API = "https://commons.wikimedia.org/w/api.php"
# Largeur de vignette standard servie par Wikimedia (les autres largeurs sont refusées).
THUMB_WIDTH = 1280
JPEG_QUALITY = 85

# nom du fixture -> (fichier Commons, SHA-1 de l'original, plus grand côté en px)
PAINTINGS: dict[str, tuple[str, str, int]] = {
    "portrait_mona_lisa.jpg": (
        "Mona_Lisa,_by_Leonardo_da_Vinci,_from_C2RMF_retouched.jpg",
        "4ba2f28c16b265bcde00336a8e0b1251e4a258da",
        1000,
    ),
    "portrait_jeune_fille_perle.jpg": (
        "1665_Girl_with_a_Pearl_Earring.jpg",
        "863e60ca490e9e58214f2ecc5931d2d9b837f6d8",
        800,
    ),
    "paysage_nuit_etoilee.jpg": (
        "Van_Gogh_-_Starry_Night_-_Google_Art_Project.jpg",
        "b3e5f8291b4137ff50ba876c84a1f7248186104a",
        800,
    ),
}

# Page de texte générée (aucune source extérieure, texte fictif sans nom), en Georama
# (police de la charte, OFL-1.1) : la police par défaut de Pillow n'a pas les accents.
TEXT_PAGE = "page_texte.png"
TEXT_PAGE_FONT = ROOT / "assets" / "charte" / "fonts" / "Georama-400.ttf"
TEXT_PAGE_SIZE = (850, 1100)
TEXT_PAGE_MARGIN = 70
TEXT_PAGE_FONT_SIZE = 22
TEXT_PAGE_LINE_HEIGHT = 34
TEXT_PAGE_LINES = [
    "Compte rendu de la réunion du comité technique",
    "",
    "Le comité s'est réuni pour examiner le budget annuel et le",
    "calendrier des travaux de rénovation du bâtiment principal.",
    "Les membres présents ont approuvé le plan de financement,",
    "sous réserve d'une étude complémentaire sur l'isolation des",
    "toitures et le remplacement des menuiseries extérieures.",
    "",
    "Points examinés :",
    "1. Bilan des dépenses du premier semestre.",
    "2. Choix du prestataire pour la maintenance des ascenseurs.",
    "3. Mise à jour du règlement intérieur de la salle commune.",
    "4. Questions diverses relatives à la signalétique.",
    "",
    "La prochaine séance aura lieu au début du mois prochain.",
    "Le présent compte rendu est diffusé aux membres du comité",
    "et affiché dans le hall d'entrée pendant quinze jours.",
]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def download(url: str) -> bytes:
    last: Exception | None = None
    for attempt in range(1, MAX_ATTEMPTS + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=SOCKET_TIMEOUT_S) as resp:
                data = resp.read()
                expected = resp.headers.get("Content-Length")
            if expected and int(expected) != len(data):
                raise OSError(f"téléchargement tronqué ({len(data)}/{expected} octets)")
            return data
        except OSError as e:  # URLError, HTTPError, délai dépassé
            last = e
            print(f"  essai {attempt}/{MAX_ATTEMPTS} échoué : {e}", file=sys.stderr)
            time.sleep(RETRY_DELAY_S)
    raise SystemExit(f"échec du téléchargement de {url} : {last}")


def prepare_model() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    model = download(MODEL_URL)
    if len(model) != MODEL_SIZE or sha256(model) != MODEL_SHA256:
        raise SystemExit(f"{MODEL_NAME} : empreinte inattendue ({len(model)} octets, {sha256(model)})")
    license_text = download(LICENSE_URL)
    if sha256(license_text) != LICENSE_SHA256:
        raise SystemExit(f"LICENSE : empreinte inattendue ({sha256(license_text)})")
    (ASSETS / MODEL_NAME).write_bytes(model)
    (ASSETS / "LICENSE").write_bytes(license_text)
    sums = f"{MODEL_SHA256}  {MODEL_NAME}\n{LICENSE_SHA256}  LICENSE\n"
    (ASSETS / "SHA256SUMS").write_text(sums, encoding="ascii", newline="\n")
    print(f"{MODEL_NAME}  {len(model)} octets  {MODEL_SHA256}")


def commons_thumb(title: str, original_sha1: str) -> bytes:
    query = urllib.parse.urlencode(
        {
            "action": "query",
            "format": "json",
            "prop": "imageinfo",
            "iiprop": "url|sha1",
            "iiurlwidth": THUMB_WIDTH,
            "titles": f"File:{title}",
        }
    )
    info = json.loads(download(f"{COMMONS_API}?{query}"))
    pages = list(info["query"]["pages"].values())
    ii = pages[0]["imageinfo"][0]
    if ii["sha1"] != original_sha1:
        raise SystemExit(f"{title} : l'original a changé sur Commons (SHA-1 {ii['sha1']})")
    return download(ii["thumburl"])


def prepare_fixtures() -> None:
    FIXTURES.mkdir(parents=True, exist_ok=True)
    for name, (title, sha1, longest) in PAINTINGS.items():
        img = Image.open(io.BytesIO(commons_thumb(title, sha1))).convert("RGB")
        img.thumbnail((longest, longest), Image.Resampling.LANCZOS)
        out = FIXTURES / name
        img.save(out, "JPEG", quality=JPEG_QUALITY, optimize=True)
        print(f"{name}  {img.width}x{img.height}  {out.stat().st_size} octets")

    page = Image.new("L", TEXT_PAGE_SIZE, 255)
    draw = ImageDraw.Draw(page)
    font = ImageFont.truetype(str(TEXT_PAGE_FONT), TEXT_PAGE_FONT_SIZE)
    for i, line in enumerate(TEXT_PAGE_LINES):
        y = TEXT_PAGE_MARGIN + i * TEXT_PAGE_LINE_HEIGHT
        draw.text((TEXT_PAGE_MARGIN, y), line, fill=0, font=font)
    out = FIXTURES / TEXT_PAGE
    page.save(out, "PNG", optimize=True)
    print(f"{TEXT_PAGE}  {page.width}x{page.height}  {out.stat().st_size} octets")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fixtures", action="store_true", help="regénère aussi les images de test")
    args = parser.parse_args()
    prepare_model()
    if args.fixtures:
        prepare_fixtures()


if __name__ == "__main__":
    main()
