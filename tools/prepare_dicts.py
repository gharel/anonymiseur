#!/usr/bin/env python3
"""Prépare les dictionnaires embarqués de l'anonymiseur (crates/anon-dict/data).

Usage : uv run --project tools python tools/prepare_dicts.py [--refresh]

Bibliothèque standard uniquement. Seul script autorisé à utiliser le réseau
(jamais embarqué dans l'application). Les sources sont téléchargées dans un
cache hors du dépôt ; `--refresh` force un nouveau téléchargement.

Sortie : listes texte triées, une entrée par ligne `forme_pliée<TAB>palier`
(palier de fréquence 1 = rare … 3 = très courant), plus SOURCES.md.

Le pliage (`fold`) DOIT rester identique à `fold()` de crates/anon-dict/src/fold.rs ;
un test Rust relit chaque liste et vérifie que toute entrée est déjà pliée.
"""

from __future__ import annotations

import csv
import datetime
import io
import re
import sys
import tempfile
import time
import unicodedata
import urllib.request
import zipfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "crates" / "anon-dict" / "data"
CACHE = Path(tempfile.gettempdir()) / "anon-dicts-cache"

USER_AGENT = "Mozilla/5.0 (anon-prepare-dicts; offline-anonymizer data preparation)"
SOCKET_TIMEOUT_S = 90
MAX_ATTEMPTS = 12

# --- Sources (URL exactes vérifiées) ---------------------------------------

FIRST_NAMES_PAGE = "https://www.insee.fr/fr/statistiques/8595130"
FIRST_NAMES_URL = (
    "https://www.insee.fr/fr/statistiques/fichier/8595130/prenoms-2025-nat_csv.zip"
)
LEXICON_PAGE = "https://grammalecte.net/"
LEXICON_URL = "https://www.grammalecte.net/dic/lexique-grammalecte-fr-v7.7.zip"
SURNAMES_PAGE = "https://www.insee.fr/fr/statistiques/3536630"
SURNAMES_URL = (
    "https://www.insee.fr/fr/statistiques/fichier/3536630/noms2008nat_txt.zip"
)
COG_PAGE = "https://www.data.gouv.fr/datasets/code-officiel-geographique-cog"
COG_COMMUNES_URL = (
    "https://www.insee.fr/fr/statistiques/fichier/8740222/v_commune_2026.csv"
)
COG_COMER_URL = (
    "https://www.insee.fr/fr/statistiques/fichier/8740222/v_commune_comer_2026.csv"
)

# --- Seuils (nombres magiques nommés) --------------------------------------

MIN_FIRST_NAME_BIRTHS = 300  # naissances cumulées 1900-2025 pour être retenu
FIRST_NAME_TIER3 = 20_000
FIRST_NAME_TIER2 = 3_000
SURNAME_TIER3 = 20_000  # naissances cumulées 1891-2000
SURNAME_TIER2 = 3_000
MIN_SURNAME_BIRTHS = 30  # seuil de publication du fichier INSEE lui-même
NEW_CALEDONIA_COMMUNES = 33
MIN_ENTRY_LEN = 2
# Indice de fréquence du lexique Grammalecte (0-9) à partir duquel une forme
# minuscule est un « mot courant » (~30 000 formes).
LEXICON_MIN_FREQUENCY = 6
# À cet indice limite, un prénom ou nom très courant (« jean », « marie ») reste
# un nom : au-dessus, le mot ordinaire l'emporte (« pierre », « rose », « petit »).
LEXICON_NAME_OVERRIDE_FREQUENCY = 6
LEXICON_PROPER_TAGS = {"npr", "prn", "patr"}

# Communes d'outre-mer ajoutées si le COG ne les contient pas (chefs-lieux).
EXTRA_COMMUNES = [
    "Paris",
    "Lyon",
    "Marseille",
    "Fort-de-France",
    "Cayenne",
    "Saint-Denis",
    "Mamoudzou",
    "Basse-Terre",
    "Pointe-à-Pitre",
    "Papeete",
    "Mata-Utu",
    "Nouméa",
    "Saint-Pierre",
    "Gustavia",
    "Marigot",
]

# --- Pliage -----------------------------------------------------------------

_SPECIAL = {
    "œ": "oe",
    "æ": "ae",
    "ß": "ss",
    "ø": "o",
    "đ": "d",
    "ł": "l",
    "’": "'",
    "‘": "'",
    "ʼ": "'",
    "`": "'",
    "´": "'",
    "-": " ",
    "‐": " ",
    "‑": " ",
    "‒": " ",
    "–": " ",
    "—": " ",
    "―": " ",
    "−": " ",
}
_KEEP = re.compile(r"^[a-z][a-z' ]*$")


def fold(text: str) -> str:
    """Minuscules, sans accents, apostrophes unifiées, tirets et espaces -> espace unique."""
    out: list[str] = []
    for ch in unicodedata.normalize("NFD", text):
        if unicodedata.combining(ch) != 0:
            continue
        low = ch.lower()  # par caractère, comme char::to_lowercase en Rust
        for c in low:
            c = _SPECIAL.get(c, c)
            if c.isspace():
                c = " "
            out.append(c)
    return " ".join(p for p in "".join(out).split(" ") if p)


def clean_entry(form: str) -> str | None:
    f = fold(form)
    if len(f) < MIN_ENTRY_LEN or not _KEEP.match(f):
        return None
    return f


# --- Téléchargement ----------------------------------------------------------


def download(url: str, refresh: bool) -> bytes:
    """Télécharge `url` (cache local). Le serveur INSEE est lent : on réessaie."""
    CACHE.mkdir(parents=True, exist_ok=True)
    cached = CACHE / re.sub(r"[^A-Za-z0-9._-]", "_", url.split("//", 1)[1])
    if cached.exists() and not refresh:
        data = cached.read_bytes()
        if _complete(url, data) and (url.endswith(".zip") or data.endswith(b"\n")):
            return data
    last: Exception | None = None
    for attempt in range(1, MAX_ATTEMPTS + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            buf = io.BytesIO()
            with urllib.request.urlopen(req, timeout=SOCKET_TIMEOUT_S) as resp:
                while True:
                    chunk = resp.read(65536)
                    if not chunk:
                        break
                    buf.write(chunk)
            data = buf.getvalue()
            expected = resp.headers.get("Content-Length")
            if expected and int(expected) != len(data):
                raise OSError(f"téléchargement tronqué ({len(data)}/{expected} octets)")
            if not _complete(url, data):
                raise OSError("téléchargement incomplet")
            cached.write_bytes(data)
            print(f"  {url} : {len(data)} octets", file=sys.stderr)
            return data
        except Exception as exc:  # noqa: BLE001 - on réessaie quoi qu'il arrive
            last = exc
            print(f"  tentative {attempt}/{MAX_ATTEMPTS} échouée : {exc}", file=sys.stderr)
            time.sleep(min(2 * attempt, 15))
    raise SystemExit(f"échec du téléchargement de {url} : {last}")


def _complete(url: str, data: bytes) -> bool:
    if url.endswith(".zip"):
        try:
            with zipfile.ZipFile(io.BytesIO(data)) as z:
                return z.testzip() is None
        except zipfile.BadZipFile:
            return False
    return len(data) > 0


def decode(raw: bytes) -> str:
    for enc in ("utf-8-sig", "cp1252", "latin-1"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            continue
    return raw.decode("latin-1", errors="replace")


# --- Prénoms -----------------------------------------------------------------


def tier(count: int, t3: int, t2: int) -> int:
    return 3 if count >= t3 else 2 if count >= t2 else 1


def build_first_names(refresh: bool) -> dict[str, int]:
    raw = download(FIRST_NAMES_URL, refresh)
    totals: Counter[str] = Counter()
    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        member = next(n for n in z.namelist() if n.lower().endswith(".csv"))
        text = decode(z.read(member))
    reader = csv.reader(io.StringIO(text), delimiter=";")
    header = [h.strip().lower() for h in next(reader)]

    def col(*names: str) -> int:
        for n in names:
            if n in header:
                return header.index(n)
        raise SystemExit(f"colonne introuvable parmi {names} dans {header}")

    c_name = col("preusuel", "prenom", "prénom")
    c_count = col("nombre", "valeur")
    c_year = col("annais", "periode", "année")
    for row in reader:
        if len(row) <= max(c_name, c_count, c_year):
            continue
        name = row[c_name].strip()
        if not name or name.startswith("_") or "XXXX" in row[c_year]:
            continue
        try:
            n = int(float(row[c_count]))
        except ValueError:
            continue
        totals[name] += n
    folded: Counter[str] = Counter()
    for name, n in totals.items():
        f = clean_entry(name)
        if f:
            folded[f] += n
    return {
        f: tier(n, FIRST_NAME_TIER3, FIRST_NAME_TIER2)
        for f, n in folded.items()
        if n >= MIN_FIRST_NAME_BIRTHS
    }


# --- Noms de famille ---------------------------------------------------------


def build_surnames(refresh: bool) -> dict[str, int]:
    raw = download(SURNAMES_URL, refresh)
    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        member = next(n for n in z.namelist() if n.lower().endswith(".txt"))
        text = decode(z.read(member))
    lines = text.splitlines()
    header = re.split(r"\t|;", lines[0].strip())
    # Colonnes attendues : NOM, puis un effectif par décennie (et éventuellement un total).
    totals: Counter[str] = Counter()
    for line in lines[1:]:
        parts = re.split(r"\t|;", line.rstrip("\r\n"))
        if len(parts) < 2:
            continue
        name = parts[0].strip()
        # « AUTRES NOMS » agrège les noms de moins de 30 occurrences : pas un nom.
        if not name or name.startswith("_") or name.upper() == "AUTRES NOMS":
            continue
        totals[name] += sum(int(p) for p in parts[1:] if p.strip().isdigit())
    if not totals:
        raise SystemExit(f"fichier des noms vide ou format inattendu : {header}")
    folded: Counter[str] = Counter()
    for name, n in totals.items():
        f = clean_entry(name)
        if f:
            folded[f] += n
    return {
        f: tier(n, SURNAME_TIER3, SURNAME_TIER2)
        for f, n in folded.items()
        if n >= MIN_SURNAME_BIRTHS
    }


# --- Communes ---------------------------------------------------------------


def build_communes(refresh: bool) -> dict[str, int]:
    out: dict[str, int] = {}

    def add(name: str | None, level: int) -> None:
        f = clean_entry(name or "")
        if f:
            out[f] = max(out.get(f, 0), level)

    text = decode(download(COG_COMMUNES_URL, refresh))
    for row in csv.DictReader(io.StringIO(text)):
        kind = row.get("TYPECOM", "")
        if kind == "COM":
            level = 3
        elif kind in ("COMA", "COMD"):
            level = 2
        else:  # ARM : arrondissements municipaux, pas des noms de lieu
            continue
        add(row.get("NCCENR", ""), level)
        add(row.get("LIBELLE", ""), level)

    text = decode(download(COG_COMER_URL, refresh))
    nc = 0
    for row in csv.DictReader(io.StringIO(text)):
        add(row.get("NCCENR", ""), 3)
        add(row.get("LIBELLE", ""), 3)
        if row.get("COMER") == "988" or row.get("COM", "").startswith("988"):
            nc += 1
    if nc != NEW_CALEDONIA_COMMUNES:
        print(
            f"ATTENTION : {nc} communes de Nouvelle-Calédonie trouvées "
            f"(attendu {NEW_CALEDONIA_COMMUNES})",
            file=sys.stderr,
        )
    for name in EXTRA_COMMUNES:
        f = clean_entry(name)
        if f and f not in out:
            print(f"  commune ajoutée (absente du COG) : {name}", file=sys.stderr)
            out[f] = 3
    return out


# --- Mots courants -----------------------------------------------------------

# Aucune liste de fréquence à licence clairement compatible (Licence Ouverte, CC0,
# CC-BY, MIT) n'a été retenue : Lexique 3 est sous CC BY-SA, les listes dérivées de
# sous-titres sont sous CC BY-SA ou sans licence claire. Liste raisonnée, écrite à la
# main : mots français courants qui sont aussi des prénoms ou des noms de famille
# (ou qui s'écrivent avec une majuscule en début de phrase et prêtent à confusion).
# « martin » n'en fait volontairement pas partie (le nom propre domine).
COMMON_WORDS = """
guadeloupe martinique guyane réunion mayotte calédonie polynésie france corse
marin blanc abbé agneau aimé aimée amour ange appel arbre argent aube aurore avenir bailly bal banc baron
barre bas bateau beau bel belle bergère berger besson bœuf bois bon bonne boucher boulanger
bourgeois bref brun bureau cadet camp carré chance chanoine chapelle charpentier chasseur
chat chaud chef chêne chevalier chien clair clerc colombe comte constant constance content
cordier cour courtois court cousin couturier cuisinier cure curé dame danse dauphin devoir
dimanche doux droit duc dupré ecole ermite espoir étoile été eugène fabre fidèle fier fils
fleur fontaine forêt forestier forge forgeron fort fortune fournier franc français franche
froid gai gaillard garçon gardien garnier gentil gérard gilles grâce grand grande gras gris
gros guerre guide guillaume hardi heureux honoré hôtel hugo humble innocent iris jardin
jardinier jaune jeune joli joly joyeux juge jour justice laborde lacroix lafont laine laurier
lebrun leclerc lecomte legrand lejeune lemaire lemaître lenoir leroux leroy léger lévêque
liberté lièvre long loup lourd lys madame maigre main maire maître marchand marais marcel
marchal mardi mars marteau martel masson matin mauvais mercier mercredi merci mer messager
meunier michel miel moine monde monsieur montagne mont moreau moulin muet muller neuf
neveu noble noël noir noire nom normand nouveau nuit olive or orange page paix papillon
parent parfait pascal pasteur pâques patient pauvre paysan pêcheur pelletier père perle
petit petite pierre pigeon pilote plaine poète poirier pont port portier poulain pré prévost
prieur prince princesse prudence prudent puits rapide renard renaud reine rémi rémy rené
rivière robin roche roi roman rose rossignol rouge rousseau roux roy ruisseau sable sage
saint sainte salomon sang sauvage savoie savant seigneur sergent serre simple soleil soldat
sourd sourire souris source sec tailleur taillandier tellier terre tête thierry tigre
toussaint tour tourneur tranquille tricot truc vaillant valentin vallée vallet vendredi
verdier vert verte viel vieux vigne vigneron villain ville violet violette vivier voisin
voyer vrai victoire victor
janvier février mars avril mai juin juillet août septembre octobre novembre décembre
lundi mardi mercredi jeudi vendredi samedi dimanche
avocat avocate directeur directrice docteur président présidente professeur
ministre notaire médecin infirmier infirmière juge greffier huissier expert
client cliente patient patiente locataire propriétaire employeur salarié
contrat dossier facture devis courrier adresse numéro société entreprise
agence cabinet groupe banque mairie tribunal préfecture commune
rue avenue boulevard place chemin impasse route allée quai cours
bonjour bonsoir merci cordialement madame monsieur
""".split()


# Pays, régions et langues courants (français et formes locales), pour que
# « France », « España » ou « Deutschland » ne passent jamais pour des noms.
GEO_WORDS = """
afghanistan afrique albanie algérie allemagne amérique andorre angleterre angola
antilles argentine arménie asie australie autriche azerbaïdjan bahamas belgique bénin
biélorussie birmanie bolivie bosnie brésil bretagne bulgarie burkina cambodge cameroun
canada chili chine chypre colombie congo corée corse croatie cuba danemark djibouti
écosse égypte emirats équateur érythrée espagne estonie états-unis éthiopie europe
finlande france gabon galles géorgie ghana grèce groenland guadeloupe guatemala guinée
guyane haïti honduras hongrie inde indonésie irak iran irlande islande israël italie
jamaïque japon jordanie kazakhstan kenya koweït laos lettonie liban libye lituanie
luxembourg macédoine madagascar malaisie mali malte maroc martinique maurice mauritanie
mayotte mexique moldavie monaco mongolie monténégro mozambique namibie népal nicaragua
niger nigeria norvège océanie ouganda pakistan palestine panama paraguay pays-bas pérou
philippines pologne polynésie portugal qatar réunion roumanie royaume-uni russie rwanda
sénégal serbie singapour slovaquie slovénie somalie soudan suède suisse syrie tahiti
taïwan tchad tchéquie thaïlande togo tunisie turquie ukraine uruguay vanuatu venezuela
vietnam yémen zambie zimbabwe calédonie wallis futuna
españa méxico francia alemania italia deutschland österreich schweiz belgië nederland
polska česko magyarország românia ελλάδα england spain germany switzerland netherlands
ireland scotland wales brasil perú chile colombia argentina portugal
français française anglais anglaise allemand espagnol espagnole italien italienne
arabe chinois japonais russe portugais portugaise néerlandais polonais turc turque
canaque kanak caldoche tahitien
""".split()


def build_common_words(
    first: dict[str, int], last: dict[str, int], refresh: bool
) -> dict[str, int]:
    """Mots courants : lexique Grammalecte (formes minuscules fréquentes) + listes écrites à la main."""
    out: dict[str, int] = {}
    raw = download(LEXICON_URL, refresh)
    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        member = next(n for n in z.namelist() if n.endswith(".txt") and "lexique" in n)
        text = decode(z.read(member))
    word_re = re.compile(r"^[a-zàâäçéèêëîïôöùûüÿœæ'-]+$")
    kept = 0
    for line in text.splitlines():
        cols = line.split("	")
        if len(cols) < 20 or not cols[0].isdigit():
            continue
        form, tags, freq = cols[2], cols[4], cols[-1]
        if not freq.isdigit() or not form[:1].islower() or not word_re.match(form):
            continue
        freq_i = int(freq)
        if freq_i < LEXICON_MIN_FREQUENCY or tags.split()[0] in LEXICON_PROPER_TAGS:
            continue
        f = clean_entry(form)
        if not f:
            continue
        is_name = first.get(f) == 3 or last.get(f) == 3
        if is_name and freq_i <= LEXICON_NAME_OVERRIDE_FREQUENCY:
            continue
        out[f] = max(out.get(f, 0), 3 if freq_i >= 7 else 2)
        kept += 1
    print(f"  lexique : {len(out)} formes retenues", file=sys.stderr)
    for w in COMMON_WORDS + GEO_WORDS:
        f = clean_entry(w)
        if f:
            out[f] = 3
    return out


# --- Écriture -----------------------------------------------------------------


def write_list(name: str, entries: dict[str, int]) -> None:
    DATA.mkdir(parents=True, exist_ok=True)
    path = DATA / name
    lines = [f"{k}\t{v}" for k, v in sorted(entries.items(), key=lambda kv: kv[0].encode())]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    size = path.stat().st_size
    print(f"{name}: {len(entries)} entrées, {size} octets")


def write_sources(today: str) -> None:
    text = f"""# Sources des dictionnaires embarqués

Généré par `tools/prepare_dicts.py` le {today}. Ne pas éditer à la main.
Les listes sont pliées (minuscules, sans accents, apostrophes et tirets normalisés)
et portent un palier de fréquence (1 = rare … 3 = très courant).

## prenoms.txt

- Source : Insee, « Fichier des prénoms », édition 2025 (naissances 1900-2025, France hors Mayotte).
- Page : {FIRST_NAMES_PAGE} (parue le 16/07/2026)
- Fichier utilisé : {FIRST_NAMES_URL}
- Licence : Licence Ouverte / Open Licence 2.0 (Etalab), données publiques de l'Insee.
- Traitement : effectifs cumulés tous sexes et toutes années, prénoms d'au moins
  {MIN_FIRST_NAME_BIRTHS} naissances ; « _PRENOMS_RARES » écarté. Paliers :
  3 si >= {FIRST_NAME_TIER3}, 2 si >= {FIRST_NAME_TIER2}, 1 sinon.

## noms.txt

- Source : Insee, « Fichier des noms de famille » (noms attribués au moins 30 fois de 1891 à 2000,
  France métropolitaine et DOM), édition parue le 22/05/2018. Aucune édition plus récente
  n'est publiée par l'Insee.
- Page : {SURNAMES_PAGE}
- Fichier utilisé : {SURNAMES_URL}
- Licence : Licence Ouverte / Open Licence 2.0 (Etalab).
- Traitement : somme des effectifs par décennie. Paliers : 3 si >= {SURNAME_TIER3},
  2 si >= {SURNAME_TIER2}, 1 sinon.

## communes.txt

- Source : Insee, Code officiel géographique (COG), millésime 2026 (situation au 01/01/2026),
  diffusé aussi sur {COG_PAGE}.
- Fichiers utilisés : {COG_COMMUNES_URL} (communes, communes déléguées et associées ;
  arrondissements municipaux écartés) et {COG_COMER_URL} (communes des collectivités
  d'outre-mer, dont les {NEW_CALEDONIA_COMMUNES} communes de Nouvelle-Calédonie).
- Licence : Licence Ouverte / Open Licence 2.0 (Etalab).
- Traitement : variantes NCCENR (« Mont-Dore ») et LIBELLE (« Le Mont-Dore »). Palier 3 =
  commune, 2 = commune déléguée ou associée. Chefs-lieux d'outre-mer ajoutés s'ils manquent :
  {", ".join(EXTRA_COMMUNES)}.

## mots_courants.txt

- Source principale : Dicollecte / Grammalecte, « Lexique des formes fléchies du français »
  v7.7, {LEXICON_PAGE} ; fichier utilisé : {LEXICON_URL}.
- Licence : Mozilla Public License 2.0 (compatible AGPL-3.0 ; les données sont réutilisées
  en l'état, sous forme de formes pliées).
- Traitement : formes à initiale minuscule, hors noms propres (étiquettes npr, prn, patr),
  d'indice de fréquence >= {LEXICON_MIN_FREQUENCY} (échelle 0-9 du lexique), soit environ
  30 000 formes (noms communs, adjectifs, verbes conjugués, adverbes). Un prénom ou nom
  de palier 3 dont l'indice est {LEXICON_NAME_OVERRIDE_FREQUENCY} (« jean », « marie »)
  n'est pas retenu : il reste détectable. Palier 3 = indice >= 7, 2 sinon.
- Ajouts écrits à la main (contenu du projet, AGPL-3.0) : mots courants qui sont aussi des
  prénoms ou noms (pierre, rose, marin, blanc, petit…), jours, mois, titres, mots de
  courrier, pays, régions et langues (palier 3).
"""
    (DATA / "SOURCES.md").write_text(text, encoding="utf-8", newline="\n")


def main() -> None:
    refresh = "--refresh" in sys.argv[1:]
    print("prénoms…", file=sys.stderr)
    first = build_first_names(refresh)
    print("noms de famille…", file=sys.stderr)
    last = build_surnames(refresh)
    print("communes…", file=sys.stderr)
    communes = build_communes(refresh)
    words = build_common_words(first, last, refresh)
    write_list("prenoms.txt", first)
    write_list("noms.txt", last)
    write_list("communes.txt", communes)
    write_list("mots_courants.txt", words)
    write_sources(datetime.date.today().isoformat())


if __name__ == "__main__":
    main()
