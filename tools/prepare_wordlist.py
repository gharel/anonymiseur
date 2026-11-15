#!/usr/bin/env python3
"""Prépare la liste de mots des phrases de passe générées (crates/anon-vault/data).

Usage : python tools/prepare_wordlist.py

Bibliothèque standard uniquement, aucun réseau. Part de
crates/anon-dict/data/mots_courants.txt (Dicollecte / Grammalecte, MPL-2.0),
déjà plié (minuscules, sans accents). Sélection : palier de fréquence 3,
4 à 8 lettres a-z, hors liste d'exclusion (grossier, sensible, ambigu), puis
au plus MAX_WORDS mots choisis dans un ordre déterministe (empreinte SHA-256).
Sortie : mots.txt (un mot par ligne, trié) et SOURCES.md.
"""

import hashlib
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "crates/anon-dict/data/mots_courants.txt"
OUT = ROOT / "crates/anon-vault/data"
MIN_LEN, MAX_LEN, MAX_WORDS, TIER = 4, 8, 4096, 3

# Mots grossiers, violents, sexuels, discriminatoires ou liés à la maladie et à la mort.
EXCLUDED_STEMS = (
    "merd", "con", "cul", "pute", "putain", "salo", "salop", "bite", "couill",
    "baise", "nique", "foutr", "fout", "chier", "chie", "pisse", "pisser",
    "enculer", "encule", "bordel", "batard", "negre", "nazi", "viol", "sexe",
    "sexu", "porno", "nu", "nus", "nue", "seins", "sein", "fesse", "tuer",
    "tue", "mort", "meurt", "cadavre", "suicid", "sida", "cancer", "tueur",
    "meurtr", "assass", "haine", "hair", "guerre", "bombe", "arme", "armes",
    "drogue", "pedo", "inceste", "juif", "arabe", "tare", "idiot", "debile",
    "crev", "cass", "pete", "peter", "chatt", "pine", "zob", "foutu", "bande",
)


def excluded(word: str) -> bool:
    return any(word == s or (len(s) >= 4 and word.startswith(s)) for s in EXCLUDED_STEMS)


def main() -> None:
    words = set()
    for line in SRC.read_text(encoding="utf-8").splitlines():
        word, _, tier = line.partition("\t")
        if (
            tier == str(TIER)
            and MIN_LEN <= len(word) <= MAX_LEN
            and re.fullmatch(r"[a-z]+", word)
            and not excluded(word)
        ):
            words.add(word)
    ranked = sorted(words, key=lambda w: hashlib.sha256(w.encode()).hexdigest())
    chosen = sorted(ranked[:MAX_WORDS])
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "mots.txt").write_text("\n".join(chosen) + "\n", encoding="utf-8")
    (OUT / "SOURCES.md").write_text(
        f"""# Liste de mots des phrases de passe

Généré par `tools/prepare_wordlist.py`. Ne pas éditer à la main.

## mots.txt

- {len(chosen)} mots français courants, de {MIN_LEN} à {MAX_LEN} lettres, sans accents, un par ligne.
- Source : `crates/anon-dict/data/mots_courants.txt`, lui-même tiré de Dicollecte / Grammalecte,
  « Lexique des formes fléchies du français » v7.7 (https://grammalecte.net/).
- Licence : Mozilla Public License 2.0 (compatible AGPL-3.0), données réutilisées sous forme
  de formes pliées.
- Traitement : palier de fréquence 3 (les plus courants), lettres a-z seulement, liste
  d'exclusion de termes grossiers, violents, sexuels ou liés à la maladie et à la mort,
  puis sélection déterministe d'au plus {MAX_WORDS} mots (ordre des empreintes SHA-256).
""",
        encoding="utf-8",
    )
    print(f"{len(chosen)} mots")


if __name__ == "__main__":
    main()
