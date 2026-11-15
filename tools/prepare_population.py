#!/usr/bin/env python3
"""Prépare les données de population embarquées (crates/anon-dict/data).

Usage : uv run --project tools python tools/prepare_population.py [--refresh]

Seul script autorisé à utiliser le réseau pour ces données (jamais embarqué dans
l'application). Les sources sont téléchargées dans un cache hors du dépôt ;
`--refresh` force un nouveau téléchargement. Aucun chiffre n'est écrit à la main :
tout effectif vient d'un fichier public cité dans SOURCES.md.

Sorties (texte UTF-8, une entrée par ligne, champs séparés par une tabulation) :
- population_communes.txt   : nom plié, population municipale, code de zone ;
- population_zones.txt      : code de zone, territoire, libellé de la zone
                              (département, province, collectivité), libellé de la région ;
- population_territoires.txt: code du territoire, libellé, population ;
- population_ages.txt       : territoire, âge en années révolues, effectif ;
- metiers.txt               : intitulé plié, effectif France entière, catégorie plus large ;
- la section « Données de population » de SOURCES.md.

Le pliage vient de tools/prepare_dicts.py (identique à crates/anon-dict/src/fold.rs).
"""

from __future__ import annotations

import csv
import datetime
import io
import json
import re
import sys
import tempfile
import time
import urllib.request
import zipfile
from collections import defaultdict
from pathlib import Path

from prepare_dicts import POPULATION_MARKER, fold

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "crates" / "anon-dict" / "data"
CACHE = Path(tempfile.gettempdir()) / "anon-population-cache"

USER_AGENT = "Mozilla/5.0 (anon-prepare-population; offline-anonymizer data preparation)"
SOCKET_TIMEOUT_S = 120
MAX_ATTEMPTS = 8

# --- Sources (URL exactes vérifiées) ---------------------------------------

LEGAL_PAGE = "https://www.insee.fr/fr/statistiques/8680726"
LEGAL_URL = "https://www.insee.fr/fr/statistiques/fichier/8680726/ensemble.zip"
NC_PAGE = "https://www.insee.fr/fr/statistiques/8658741"
NC_URL = (
    "https://www.insee.fr/fr/statistiques/fichier/8658741/"
    "NC2025_Annexe_TableauxDecret_pour%20Insee.fr.xlsx"
)
NC_COMMUNES_PAGE = "https://data.gouv.nc/explore/dataset/communes-nc/"
# Noms officiels des communes calédoniennes (COG 2026, comme tools/prepare_dicts.py).
COG_COMER_URL = (
    "https://www.insee.fr/fr/statistiques/fichier/8740222/v_commune_comer_2026.csv"
)
NC_COMMUNES_URL = (
    "https://data.gouv.nc/api/explore/v2.1/catalog/datasets/communes-nc/exports/csv?delimiter=%3B"
)
AGES_FR_PAGE = "https://www.insee.fr/fr/statistiques/8721185"
AGES_FR_URL = "https://www.insee.fr/fr/statistiques/fichier/8721185/3_Pop1janv_age.xlsx"
AGES_FR_SHEET = "2026"
AGES_NC_PAGE = "https://data.gouv.nc/explore/dataset/recensement-de-la-population-2019-individus-nc/"
AGES_NC_URL = (
    "https://data.gouv.nc/api/explore/v2.1/catalog/datasets/"
    "recensement-de-la-population-2019-individus-nc/records"
    "?select=ager%2Ccount(*)%20as%20n&group_by=ager&limit=200"
)
PRIVATE_PAGE = "https://catalogue-donnees.insee.fr/fr/catalogue/recherche/DS_DERA_PRIVE_ANNUEL"
PRIVATE_URL = "https://api.insee.fr/melodi/file/DS_DERA_PRIVE_ANNUEL/DS_DERA_PRIVE_ANNUEL_2024_CSV_FR"
PUBLIC_PAGE = "https://catalogue-donnees.insee.fr/fr/catalogue/recherche/DS_DERA_PUBLIC_ANNUEL"
PUBLIC_URL = "https://api.insee.fr/melodi/file/DS_DERA_PUBLIC_ANNUEL/DS_DERA_PUBLIC_ANNUEL_2023_CSV_FR"
DREES_PAGE = (
    "https://data.drees.solidarites-sante.gouv.fr/explore/dataset/"
    "la-demographie-des-professionnels-de-sante-depuis-2012/"
)
DREES_BASE = (
    "https://data.drees.solidarites-sante.gouv.fr/api/explore/v2.1/catalog/datasets/"
    "la-demographie-des-professionnels-de-sante-depuis-2012/attachments/"
)
DREES_FILES = {
    "medecins": "medecins_rpps_2012_2026_xlsx",
    "dentistes": "chirurgiens_dentistes_rpps_2012_2026_xlsx",
    "pharmaciens": "pharmaciens_rpps_2012_2026_xlsx",
    "sages_femmes": "sages_femmes_rpps_2012_2026_xlsx",
    "kines": "kinesitherapeutes_rpps_2017_2026_xlsx",
    "infirmieres_liberales": "infirmieres_liberales_snds_2012_2023_xlsx",
    "infirmieres_salariees": "infirmieres_salariees_bts_2013_2023_xlsx",
}

# --- Constantes ---------------------------------------------------------------

NEW_CALEDONIA_COMMUNES = 33
# Âge ouvert « 105 ou plus » de l'Insee, ramené à cet âge.
AGE_OPEN_FR = 105
# Population municipale de la Nouvelle-Calédonie au recensement de 2019 (contrôle
# de cohérence de l'agrégat par âge : c'est le nombre d'enregistrements du fichier).
NC_2019_RECORDS = 271_407
# Nombre minimal de métiers distincts retenus.
MIN_OCCUPATIONS = 100
LEADING_ARTICLES = ("le ", "la ", "les ", "l'")
# Arrondissements municipaux de Paris, Lyon et Marseille : additionnés pour la commune.
MUNICIPAL_DISTRICT = re.compile(r"(Paris|Lyon|Marseille) \d+(?:er|e) Arrondissement")
FR = "FR"
NC = "NC"

# Libellés des provinces tels qu'ils apparaissent dans le jeu communes-nc.
NC_PROVINCE_CODES = {
    "province nord": "NC-NORD",
    "province sud": "NC-SUD",
    "province des iles loyaute": "NC-ILES",
}

# --- Métiers ------------------------------------------------------------------
# Catégories plus larges (masculin, féminin). Le niveau suivant est « professionnel »
# ou « professionnelle » (anon-dict, module hierarchy).
DOMAINS = {
    "sante": ("professionnel de santé", "professionnelle de santé"),
    "social": ("professionnel du social", "professionnelle du social"),
    "enseignement": ("professionnel de l'enseignement", "professionnelle de l'enseignement"),
    "administration": ("professionnel de l'administration", "professionnelle de l'administration"),
    "gestion": ("professionnel de la gestion", "professionnelle de la gestion"),
    "droit": ("professionnel du droit", "professionnelle du droit"),
    "commerce": ("professionnel du commerce", "professionnelle du commerce"),
    "hotellerie": (
        "professionnel de l'hôtellerie-restauration",
        "professionnelle de l'hôtellerie-restauration",
    ),
    "alimentation": ("artisan de l'alimentation", "artisane de l'alimentation"),
    "batiment": ("professionnel du bâtiment", "professionnelle du bâtiment"),
    "industrie": ("professionnel de l'industrie", "professionnelle de l'industrie"),
    "ingenierie": ("professionnel de l'ingénierie", "professionnelle de l'ingénierie"),
    "logistique": (
        "professionnel du transport et de la logistique",
        "professionnelle du transport et de la logistique",
    ),
    "informatique": ("professionnel de l'informatique", "professionnelle de l'informatique"),
    "securite": ("professionnel de la sécurité", "professionnelle de la sécurité"),
    "banque": (
        "professionnel de la banque et de l'assurance",
        "professionnelle de la banque et de l'assurance",
    ),
    "communication": (
        "professionnel de la communication",
        "professionnelle de la communication",
    ),
    "services": (
        "professionnel des services à la personne",
        "professionnelle des services à la personne",
    ),
    "proprete": ("professionnel de la propreté", "professionnelle de la propreté"),
    "direction": ("dirigeant d'entreprise", "dirigeante d'entreprise"),
    "nature": ("professionnel des espaces verts et de l'agriculture",
               "professionnelle des espaces verts et de l'agriculture"),
    "sport": ("professionnel du sport", "professionnelle du sport"),
}

# (intitulés masculins, intitulés féminins, catégorie, sources)
# Intitulé épicène : présent dans les deux listes (la catégorie masculine s'applique).
# Source « pcs:XXXX » : effectif en équivalent temps plein de la Base Tous salariés
# (privé 2024 + fonction publique 2023), code PCS-ESE ; « drees:clé » : effectif DREES.
# Métiers exercés surtout en indépendant hors santé (avocat, notaire, architecte,
# agriculteur, taxi, photographe…) : écartés, faute de source ouverte qui les compte.
TEACHERS = ["pcs:421A", "pcs:421B", "pcs:341A", "pcs:342B", "pcs:342C", "pcs:342D",
            "pcs:422A", "pcs:422B", "pcs:422C"]
SECONDARY = ["pcs:341A", "pcs:422A", "pcs:422B", "pcs:422C"]
SELLERS = ["pcs:553B", "pcs:553C", "pcs:554A", "pcs:554B", "pcs:554C", "pcs:554E",
           "pcs:554F", "pcs:554G", "pcs:554H", "pcs:556A"]
ENGINEERS = ["pcs:380A", "pcs:381B", "pcs:381C", "pcs:382A", "pcs:382C", "pcs:382D",
             "pcs:383A", "pcs:383B", "pcs:383C", "pcs:384A", "pcs:384B", "pcs:384C",
             "pcs:385A", "pcs:385B", "pcs:385C", "pcs:386B", "pcs:386C", "pcs:386D",
             "pcs:386E", "pcs:387A", "pcs:387B", "pcs:387C", "pcs:387D", "pcs:387E",
             "pcs:387F", "pcs:388A", "pcs:388B", "pcs:388C", "pcs:388D", "pcs:388E",
             "pcs:389A", "pcs:332A", "pcs:332B"]
TECHNICIANS = ["pcs:471A", "pcs:471B", "pcs:472A", "pcs:472B", "pcs:472C", "pcs:472D",
               "pcs:473A", "pcs:473B", "pcs:473C", "pcs:474A", "pcs:474B", "pcs:474C",
               "pcs:475A", "pcs:475B", "pcs:476A", "pcs:476B", "pcs:477A", "pcs:477B",
               "pcs:477C", "pcs:477D", "pcs:478A", "pcs:478B", "pcs:478C", "pcs:478D",
               "pcs:479A", "pcs:479B"]
IT = ["pcs:388A", "pcs:388B", "pcs:388C", "pcs:388D", "pcs:388E",
      "pcs:478A", "pcs:478B", "pcs:478C", "pcs:478D"]
BUSINESS_HEADS = ["pcs:210X", "pcs:220X", "pcs:231A", "pcs:232A", "pcs:233A", "pcs:233B",
                  "pcs:233C", "pcs:233D"]
NURSES = ["drees:infirmieres_salariees", "drees:infirmieres_liberales"]

METIERS: list[tuple[list[str], list[str], str, list[str]]] = [
    # Santé
    (["médecin"], ["médecin"], "sante", ["drees:medecins"]),
    (["chirurgien-dentiste", "dentiste"], ["chirurgienne-dentiste", "dentiste"], "sante",
     ["drees:dentistes"]),
    (["pharmacien"], ["pharmacienne"], "sante", ["drees:pharmaciens"]),
    (["sage-femme", "maïeuticien"], ["sage-femme"], "sante", ["drees:sages_femmes"]),
    (["masseur-kinésithérapeute", "kinésithérapeute", "kiné"],
     ["masseuse-kinésithérapeute", "kinésithérapeute", "kiné"], "sante", ["drees:kines"]),
    (["infirmier", "infirmier diplômé d'état"], ["infirmière", "infirmière diplômée d'état"],
     "sante", NURSES),
    (["aide-soignant"], ["aide-soignante"], "sante", ["pcs:526A"]),
    (["auxiliaire de puériculture"], ["auxiliaire de puériculture"], "sante", ["pcs:526C"]),
    (["puériculteur"], ["puéricultrice"], "sante", ["pcs:431C"]),
    (["cadre de santé", "cadre infirmier"], ["cadre de santé", "cadre infirmière"], "sante",
     ["pcs:431A"]),
    (["préparateur en pharmacie"], ["préparatrice en pharmacie"], "sante", ["pcs:433D"]),
    (["ambulancier"], ["ambulancière"], "sante", ["pcs:526E"]),
    (["psychologue"], ["psychologue"], "sante", ["pcs:311D", "pcs:343A"]),
    (["opticien", "opticien lunetier"], ["opticienne", "opticienne lunetière"], "sante",
     ["pcs:433B"]),
    (["technicien médical", "manipulateur radio"], ["technicienne médicale", "manipulatrice radio"],
     "sante", ["pcs:433A"]),
    (["assistant dentaire", "assistant médical"], ["assistante dentaire", "assistante médicale"],
     "sante", ["pcs:526B"]),
    (["aide médico-psychologique", "accompagnant éducatif et social"],
     ["aide médico-psychologique", "accompagnante éducative et sociale"], "sante", ["pcs:526D"]),
    (["agent de service hospitalier", "ash"], ["agente de service hospitalier", "ash"], "sante",
     ["pcs:525D"]),
    (["médecin hospitalier", "praticien hospitalier"], ["médecin hospitalière",
                                                         "praticienne hospitalière"],
     "sante", ["pcs:344A"]),
    # Social
    (["éducateur spécialisé"], ["éducatrice spécialisée"], "social", ["pcs:434D"]),
    (["moniteur éducateur"], ["monitrice éducatrice"], "social", ["pcs:434E"]),
    (["assistant de service social", "assistant social"],
     ["assistante de service social", "assistante sociale"], "social", ["pcs:434B"]),
    (["éducateur de jeunes enfants"], ["éducatrice de jeunes enfants"], "social", ["pcs:434G"]),
    (["animateur", "animateur socioculturel"], ["animatrice", "animatrice socioculturelle"],
     "social", ["pcs:435B"]),
    (["aide à domicile", "auxiliaire de vie"], ["aide à domicile", "auxiliaire de vie"], "social",
     ["pcs:563B"]),
    (["conseiller en économie sociale familiale"], ["conseillère en économie sociale familiale"],
     "social", ["pcs:434C"]),
    (["directeur de centre socioculturel"], ["directrice de centre socioculturel"], "social",
     ["pcs:435A"]),
    (["éducateur technique spécialisé", "moniteur d'atelier"],
     ["éducatrice technique spécialisée", "monitrice d'atelier"], "social", ["pcs:434F"]),
    # Enseignement
    (["enseignant"], ["enseignante"], "enseignement", TEACHERS),
    (["professeur des écoles", "instituteur"], ["professeure des écoles", "institutrice"],
     "enseignement", ["pcs:421A", "pcs:421B"]),
    (["professeur", "professeur de collège", "professeur de lycée"],
     ["professeure", "professeure de collège", "professeure de lycée"], "enseignement", SECONDARY),
    (["professeur de lycée professionnel"], ["professeure de lycée professionnel"],
     "enseignement", ["pcs:422B"]),
    (["enseignant-chercheur", "maître de conférences", "professeur des universités"],
     ["enseignante-chercheuse", "maîtresse de conférences", "professeure des universités"],
     "enseignement", ["pcs:342B"]),
    (["conseiller principal d'éducation", "cpe"], ["conseillère principale d'éducation", "cpe"],
     "enseignement", ["pcs:422D"]),
    (["surveillant", "assistant d'éducation"], ["surveillante", "assistante d'éducation"],
     "enseignement", ["pcs:422E"]),
    (["formateur"], ["formatrice"], "enseignement", ["pcs:423B"]),
    (["moniteur d'auto-école", "moniteur d'école de conduite"],
     ["monitrice d'auto-école", "monitrice d'école de conduite"], "enseignement", ["pcs:423A"]),
    (["chercheur", "chargé de recherche", "directeur de recherche"],
     ["chercheuse", "chargée de recherche", "directrice de recherche"], "enseignement",
     ["pcs:342F"]),
    (["ingénieur d'études", "ingénieur de recherche"],
     ["ingénieure d'études", "ingénieure de recherche"], "enseignement", ["pcs:342G"]),
    (["doctorant"], ["doctorante"], "enseignement", ["pcs:342H"]),
    (["chef d'établissement", "proviseur", "principal de collège"],
     ["cheffe d'établissement", "proviseure", "principale de collège"], "enseignement",
     ["pcs:341B"]),
    (["bibliothécaire", "archiviste"], ["bibliothécaire", "archiviste"], "communication",
     ["pcs:351A", "pcs:425A"]),
    (["atsem", "agent territorial spécialisé des écoles maternelles"],
     ["atsem", "agente territoriale spécialisée des écoles maternelles"], "enseignement",
     ["pcs:525A"]),
    # Administration, gestion, droit
    (["secrétaire"], ["secrétaire"], "administration", ["pcs:542A", "pcs:461C"]),
    (["assistant de direction", "secrétaire de direction"],
     ["assistante de direction", "secrétaire de direction"], "administration", ["pcs:461B"]),
    (["employé administratif"], ["employée administrative"], "administration",
     ["pcs:543G", "pcs:543H"]),
    (["agent administratif"], ["agente administrative"], "administration",
     ["pcs:524A", "pcs:524B", "pcs:524C"]),
    (["adjoint administratif"], ["adjointe administrative"], "administration",
     ["pcs:523B", "pcs:523C", "pcs:523D"]),
    (["agent d'accueil", "hôte d'accueil"], ["agente d'accueil", "hôtesse d'accueil"],
     "administration", ["pcs:541B", "pcs:541C"]),
    (["standardiste"], ["standardiste"], "administration", ["pcs:541D"]),
    (["comptable"], ["comptable"], "gestion", ["pcs:543B", "pcs:461D"]),
    (["aide-comptable"], ["aide-comptable"], "gestion", ["pcs:543C"]),
    (["contrôleur de gestion"], ["contrôleuse de gestion"], "gestion", ["pcs:372B"]),
    (["directeur financier", "responsable financier"],
     ["directrice financière", "responsable financière"], "gestion", ["pcs:373A", "pcs:373C"]),
    (["chargé d'études", "chargé d'études économiques"],
     ["chargée d'études", "chargée d'études économiques"], "gestion", ["pcs:372A"]),
    (["responsable des ressources humaines", "responsable rh", "chargé de recrutement"],
     ["responsable des ressources humaines", "responsable rh", "chargée de recrutement"],
     "gestion", ["pcs:372C"]),
    (["gestionnaire de paie", "gestionnaire rh"], ["gestionnaire de paie", "gestionnaire rh"],
     "gestion", ["pcs:543E"]),
    (["responsable de formation"], ["responsable de formation"], "gestion", ["pcs:372D"]),
    (["juriste", "juriste d'entreprise"], ["juriste", "juriste d'entreprise"], "droit",
     ["pcs:372E"]),
    (["magistrat", "juge"], ["magistrate", "juge"], "droit", ["pcs:333A"]),
    # Commerce
    (["vendeur"], ["vendeuse"], "commerce", SELLERS),
    (["caissier"], ["caissière"], "commerce", ["pcs:552A"]),
    (["employé de libre-service", "employé de rayon", "employé de commerce"],
     ["employée de libre-service", "employée de rayon", "employée de commerce"], "commerce",
     ["pcs:551A"]),
    (["chef de rayon"], ["cheffe de rayon"], "commerce", ["pcs:462B"]),
    (["directeur de magasin"], ["directrice de magasin"], "commerce", ["pcs:374A"]),
    (["commercial", "technico-commercial", "représentant"],
     ["commerciale", "technico-commerciale", "représentante"], "commerce",
     ["pcs:463A", "pcs:463B", "pcs:463C", "pcs:463D", "pcs:463E"]),
    (["responsable commercial", "directeur commercial"],
     ["responsable commerciale", "directrice commerciale"], "commerce",
     ["pcs:374C", "pcs:374D"]),
    (["acheteur", "chef de produit"], ["acheteuse", "cheffe de produit"], "commerce",
     ["pcs:462C", "pcs:374B"]),
    (["téléconseiller", "télévendeur"], ["téléconseillère", "télévendeuse"], "commerce",
     ["pcs:555A"]),
    (["assistant commercial"], ["assistante commerciale"], "commerce", ["pcs:543F"]),
    (["fleuriste"], ["fleuriste"], "commerce", ["pcs:554D"]),
    (["agent de voyages"], ["agente de voyages"], "commerce", ["pcs:546B"]),
    # Hôtellerie, restauration, alimentation
    (["serveur"], ["serveuse"], "hotellerie", ["pcs:561B", "pcs:561C"]),
    (["cuisinier", "commis de cuisine"], ["cuisinière", "commis de cuisine"], "hotellerie",
     ["pcs:636D"]),
    (["chef de cuisine", "chef cuisinier"], ["cheffe de cuisine", "cheffe cuisinière"],
     "hotellerie", ["pcs:488A"]),
    (["plongeur", "aide de cuisine", "employé polyvalent de restauration"],
     ["plongeuse", "aide de cuisine", "employée polyvalente de restauration"], "hotellerie",
     ["pcs:561D"]),
    (["réceptionniste"], ["réceptionniste"], "hotellerie", ["pcs:561E"]),
    (["valet de chambre", "employé d'étage"], ["femme de chambre", "employée d'étage"],
     "hotellerie", ["pcs:561F"]),
    (["maître d'hôtel"], ["maîtresse d'hôtel"], "hotellerie", ["pcs:468A"]),
    (["boulanger", "pâtissier"], ["boulangère", "pâtissière"], "alimentation", ["pcs:636C"]),
    (["boucher"], ["bouchère"], "alimentation", ["pcs:636A"]),
    (["charcutier"], ["charcutière"], "alimentation", ["pcs:636B"]),
    # Bâtiment
    (["maçon"], ["maçonne"], "batiment", ["pcs:632A"]),
    (["plombier", "chauffagiste"], ["plombière", "chauffagiste"], "batiment", ["pcs:632F"]),
    (["électricien"], ["électricienne"], "batiment", ["pcs:633A"]),
    (["menuisier"], ["menuisière"], "batiment", ["pcs:632D"]),
    (["charpentier"], ["charpentière"], "batiment", ["pcs:632C"]),
    (["couvreur"], ["couvreuse"], "batiment", ["pcs:632E"]),
    (["peintre en bâtiment"], ["peintre en bâtiment"], "batiment", ["pcs:632G"]),
    (["conducteur de travaux"], ["conductrice de travaux"], "batiment", ["pcs:481A", "pcs:382C"]),
    (["chef de chantier"], ["cheffe de chantier"], "batiment", ["pcs:481B"]),
    (["géomètre", "topographe"], ["géomètre", "topographe"], "batiment", ["pcs:472B"]),
    (["dessinateur", "dessinateur projeteur"], ["dessinatrice", "dessinatrice projeteuse"],
     "batiment", ["pcs:472A", "pcs:473A", "pcs:474A"]),
    (["serrurier", "métallier"], ["serrurière", "métallière"], "batiment", ["pcs:634B"]),
    (["conducteur d'engins"], ["conductrice d'engins"], "batiment", ["pcs:621C"]),
    (["agent d'entretien des bâtiments", "agent de maintenance des bâtiments"],
     ["agente d'entretien des bâtiments", "agente de maintenance des bâtiments"], "batiment",
     ["pcs:632K"]),
    # Industrie, ingénierie
    (["soudeur"], ["soudeuse"], "industrie", ["pcs:623E", "pcs:623D"]),
    (["chaudronnier"], ["chaudronnière"], "industrie", ["pcs:623A"]),
    (["mécanicien", "mécanicien automobile"], ["mécanicienne", "mécanicienne automobile"],
     "industrie", ["pcs:634C"]),
    (["carrossier"], ["carrossière"], "industrie", ["pcs:634A"]),
    (["mécanicien de maintenance"], ["mécanicienne de maintenance"], "industrie",
     ["pcs:628A", "pcs:634D"]),
    (["électromécanicien"], ["électromécanicienne"], "industrie", ["pcs:628B"]),
    (["technicien de maintenance"], ["technicienne de maintenance"], "industrie",
     ["pcs:477B", "pcs:477C"]),
    (["ingénieur"], ["ingénieure"], "ingenierie", ENGINEERS),
    (["technicien"], ["technicienne"], "ingenierie", TECHNICIANS),
    (["responsable qualité", "ingénieur qualité"], ["responsable qualité", "ingénieure qualité"],
     "ingenierie", ["pcs:387D"]),
    (["ingénieur commercial", "ingénieur d'affaires"],
     ["ingénieure commerciale", "ingénieure d'affaires"], "ingenierie",
     ["pcs:382D", "pcs:383C", "pcs:384C", "pcs:385C", "pcs:388D"]),
    (["ingénieur réseau", "administrateur réseau", "administrateur systèmes"],
     ["ingénieure réseau", "administratrice réseau", "administratrice systèmes"], "informatique",
     ["pcs:388B", "pcs:388E"]),
    (["directeur technique"], ["directrice technique"], "ingenierie", ["pcs:380A"]),
    (["ingénieur environnement"], ["ingénieure environnement"], "ingenierie", ["pcs:387F"]),
    # Transport, logistique
    (["chauffeur routier", "conducteur routier", "routier"],
     ["chauffeuse routière", "conductrice routière", "routière"], "logistique", ["pcs:641A"]),
    (["chauffeur de bus", "conducteur de bus", "conducteur de car"],
     ["chauffeuse de bus", "conductrice de bus", "conductrice de car"], "logistique",
     ["pcs:641B"]),
    (["livreur", "chauffeur-livreur", "coursier"], ["livreuse", "chauffeuse-livreuse", "coursière"],
     "logistique", ["pcs:643A"]),
    (["magasinier"], ["magasinière"], "logistique", ["pcs:653A"]),
    (["cariste"], ["cariste"], "logistique", ["pcs:652A"]),
    (["préparateur de commandes"], ["préparatrice de commandes"], "logistique", ["pcs:676C"]),
    (["manutentionnaire"], ["manutentionnaire"], "logistique", ["pcs:676A"]),
    (["responsable logistique", "logisticien"], ["responsable logistique", "logisticienne"],
     "logistique", ["pcs:387B"]),
    (["responsable d'entrepôt"], ["responsable d'entrepôt"], "logistique", ["pcs:487A"]),
    (["pilote de ligne"], ["pilote de ligne"], "logistique", ["pcs:389B"]),
    (["steward"], ["hôtesse de l'air"], "logistique", ["pcs:546D"]),
    (["conducteur de train", "conducteur de tramway", "conducteur de métro"],
     ["conductrice de train", "conductrice de tramway", "conductrice de métro"], "logistique",
     ["pcs:654B"]),
    (["contrôleur de train", "contrôleur des transports"],
     ["contrôleuse de train", "contrôleuse des transports"], "logistique", ["pcs:546A"]),
    (["docker"], ["docker"], "logistique", ["pcs:652B"]),
    # Informatique
    (["développeur", "développeur informatique"], ["développeuse", "développeuse informatique"],
     "informatique", ["pcs:388A", "pcs:478A"]),
    (["informaticien"], ["informaticienne"], "informatique", IT),
    (["chef de projet informatique", "responsable informatique"],
     ["cheffe de projet informatique", "responsable informatique"], "informatique", ["pcs:388C"]),
    (["technicien informatique", "technicien support"],
     ["technicienne informatique", "technicienne support"], "informatique",
     ["pcs:478B", "pcs:478C", "pcs:478D"]),
    # Sécurité
    (["policier", "gardien de la paix"], ["policière", "gardienne de la paix"], "securite",
     ["pcs:531A", "pcs:531B", "pcs:452A"]),
    (["policier municipal"], ["policière municipale"], "securite", ["pcs:531B"]),
    (["pompier", "sapeur-pompier"], ["pompière", "sapeuse-pompière"], "securite", ["pcs:533A"]),
    (["agent de sécurité", "vigile"], ["agente de sécurité", "vigile"], "securite", ["pcs:534A"]),
    (["surveillant pénitentiaire"], ["surveillante pénitentiaire"], "securite", ["pcs:531C"]),
    # Banque, assurance
    (["conseiller bancaire", "chargé de clientèle bancaire"],
     ["conseillère bancaire", "chargée de clientèle bancaire"], "banque", ["pcs:467A"]),
    (["conseiller en assurance", "gestionnaire de sinistres"],
     ["conseillère en assurance", "gestionnaire de sinistres"], "banque", ["pcs:467C"]),
    (["directeur d'agence bancaire"], ["directrice d'agence bancaire"], "banque", ["pcs:376D"]),
    # Communication, culture
    (["journaliste"], ["journaliste"], "communication", ["pcs:352A"]),
    (["graphiste"], ["graphiste"], "communication", ["pcs:465A"]),
    (["chargé de communication", "responsable de la communication"],
     ["chargée de communication", "responsable de la communication"], "communication",
     ["pcs:375B"]),
    (["technicien du spectacle", "régisseur"], ["technicienne du spectacle", "régisseuse"],
     "communication", ["pcs:465B", "pcs:637C"]),
    # Services à la personne, propreté
    (["coiffeur"], ["coiffeuse"], "services", ["pcs:562B"]),
    (["esthéticien"], ["esthéticienne"], "services", ["pcs:562A"]),
    (["gardien d'immeuble", "concierge"], ["gardienne d'immeuble", "concierge"], "services",
     ["pcs:564A"]),
    (["agent de propreté", "agent de nettoyage", "nettoyeur"],
     ["agente de propreté", "agente de nettoyage", "nettoyeuse"], "proprete", ["pcs:684A"]),
    (["agent d'entretien"], ["agente d'entretien"], "proprete", ["pcs:684A", "pcs:632K"]),
    # Direction
    (["gérant", "chef d'entreprise"], ["gérante", "cheffe d'entreprise"], "direction",
     BUSINESS_HEADS),
    (["directeur général", "pdg"], ["directrice générale", "pdg"], "direction",
     ["pcs:231A", "pcs:232A"]),
    # Espaces verts, agriculture, sport
    (["jardinier", "paysagiste"], ["jardinière", "paysagiste"], "nature", ["pcs:631A"]),
    (["ouvrier agricole"], ["ouvrière agricole"], "nature",
     ["pcs:691A", "pcs:691B", "pcs:691C", "pcs:691D", "pcs:691E", "pcs:691F"]),
    (["sportif professionnel"], ["sportive professionnelle"], "sport", ["pcs:424A"]),
]


# --- Téléchargement ----------------------------------------------------------


def download(url: str, refresh: bool) -> bytes:
    """Télécharge `url` (cache local), avec reprises."""
    CACHE.mkdir(parents=True, exist_ok=True)
    cached = CACHE / re.sub(r"[^A-Za-z0-9._-]", "_", url.split("//", 1)[1])[:180]
    if cached.exists() and not refresh and cached.stat().st_size > 0:
        return cached.read_bytes()
    last: Exception | None = None
    for attempt in range(1, MAX_ATTEMPTS + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            buf = io.BytesIO()
            with urllib.request.urlopen(req, timeout=SOCKET_TIMEOUT_S) as resp:
                while chunk := resp.read(65536):
                    buf.write(chunk)
                expected = resp.headers.get("Content-Length")
            data = buf.getvalue()
            if expected and int(expected) != len(data):
                raise OSError(f"téléchargement tronqué ({len(data)}/{expected} octets)")
            if not data:
                raise OSError("réponse vide")
            cached.write_bytes(data)
            print(f"  {url} : {len(data)} octets", file=sys.stderr)
            return data
        except Exception as exc:  # noqa: BLE001 - on réessaie quoi qu'il arrive
            last = exc
            print(f"  tentative {attempt}/{MAX_ATTEMPTS} échouée : {exc}", file=sys.stderr)
            time.sleep(min(2 * attempt, 15))
    raise SystemExit(f"échec du téléchargement de {url} : {last}")


def decode(raw: bytes) -> str:
    for enc in ("utf-8-sig", "cp1252"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            continue
    return raw.decode("latin-1", errors="replace")


def csv_rows(text: str, delimiter: str = ";") -> list[dict[str, str]]:
    return list(csv.DictReader(io.StringIO(text), delimiter=delimiter))


def zip_member(raw: bytes, suffix: str, exclude: str = "\0") -> str:
    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        names = [n for n in z.namelist() if n.lower().endswith(suffix) and exclude not in n]
        if len(names) != 1:
            raise SystemExit(f"entrée {suffix!r} introuvable ou ambiguë : {z.namelist()}")
        return decode(z.read(names[0]))


def xlsx(raw: bytes):
    from openpyxl import load_workbook

    return load_workbook(io.BytesIO(raw), read_only=True, data_only=True)


# --- Communes ------------------------------------------------------------------


def name_variants(name: str) -> set[str]:
    """Formes pliées d'un nom de commune : telle quelle, et sans article initial."""
    f = fold(name)
    out = {f}
    for art in LEADING_ARTICLES:
        if f.startswith(art) and len(f) > len(art):
            out.add(f[len(art):].strip())
    # « Mont-Dore (Le) », « Île-des-Pins (L') » : article rejeté en fin.
    m = re.fullmatch(r"(.+?) \((le|la|les|l')\)", f)
    if m:
        base, art = m.group(1), m.group(2)
        out.discard(f)
        out.add(base)
        out.add(f"{art}{base}" if art.endswith("'") else f"{art} {base}")
    return {v for v in out if v}


def build_communes(refresh: bool):
    """Communes françaises (Insee) et calédoniennes (Insee, ISEE, Gouvernement de la NC)."""
    raw = download(LEGAL_URL, refresh)
    zones: dict[str, tuple[str, str, str]] = {}  # code -> (territoire, zone, région)
    entries: set[tuple[str, int, str]] = set()
    totals = defaultdict(int)

    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        deps = csv_rows(decode(z.read("donnees_departements.csv")))
        communes = csv_rows(decode(z.read("donnees_communes.csv")))
        cols = csv_rows(decode(z.read("donnees_collectivites.csv")))
    dep_name = {d["DEP"]: d["Département"] for d in deps}
    cities: dict[tuple[str, str], int] = defaultdict(int)
    for c in communes:
        dep, pop = c["DEP"], int(c["PMUN"])
        zones.setdefault(dep, (FR, dep_name[dep], c["Région"]))
        totals[FR] += pop
        district = MUNICIPAL_DISTRICT.fullmatch(c["Commune"])
        if district:
            cities[(district.group(1), dep)] += pop
        for v in name_variants(c["Commune"]):
            entries.add((v, pop, dep))
    for (city, dep), pop in cities.items():
        entries.add((fold(city), pop, dep))
    for c in cols:
        code, label, pop = c["COL"], c["Collectivité"], int(c["PMUN"])
        zones.setdefault(code, (FR, label, label))
        totals[FR] += pop
        for v in name_variants(c["Commune"]):
            entries.add((v, pop, code))
    n_fr = len(communes) + len(cols)

    # Nouvelle-Calédonie : populations 2025 (Insee), provinces (Gouvernement de la NC).
    provinces = {}
    nc_names = {}
    for r in csv_rows(decode(download(NC_COMMUNES_URL, refresh))):
        code = r["code_com"].strip()
        # Poya est partagée (« province Nord,province Sud ») : première province citée.
        prov = r["province"].split(",")[0].strip()
        key = NC_PROVINCE_CODES.get(fold(prov))
        if key is None:
            raise SystemExit(f"province inconnue : {prov!r}")
        provinces[code] = key
        zones.setdefault(key, (NC, prov, "Nouvelle-Calédonie"))
        nc_names[code] = {r["nom_commune"], r["nom_minus"]}
    comer = csv.reader(io.StringIO(decode(download(COG_COMER_URL, refresh))))
    for row in comer:
        # COM, TNCC, NCC, NCCENR, LIBELLE, TYPECOM, COMER, LIBELLE_COMER
        if len(row) >= 5 and row[0] in nc_names:
            nc_names[row[0]].update({row[3], row[4]})
    wb = xlsx(download(NC_URL, refresh))
    ws = wb["Communes"]
    seen = 0
    for row in ws.iter_rows(values_only=True):
        label = row[0] if row else None
        m = re.fullmatch(r"(\d\d)\.\s+(.+)", str(label or "").strip())
        if not m:
            continue
        code = f"988{m.group(1)}"
        pop = round(float(row[1]))
        prov = provinces.get(code)
        if prov is None:
            raise SystemExit(f"commune {code} sans province")
        totals[NC] += pop
        seen += 1
        for name in {m.group(2)} | nc_names.get(code, set()):
            for v in name_variants(name):
                entries.add((v, pop, prov))
    if seen != NEW_CALEDONIA_COMMUNES:
        raise SystemExit(f"{seen} communes calédoniennes lues, {NEW_CALEDONIA_COMMUNES} attendues")
    return zones, sorted(entries), dict(totals), n_fr


# --- Âges ----------------------------------------------------------------------


def build_ages(refresh: bool) -> dict[str, list[tuple[int, int]]]:
    wb = xlsx(download(AGES_FR_URL, refresh))
    ws = wb[AGES_FR_SHEET]
    fr: list[tuple[int, int]] = []
    header_seen = False
    for row in ws.iter_rows(values_only=True):
        if not row:
            continue
        if row[0] == "Année de naissance":
            header_seen = True
            continue
        if not header_seen or row[1] is None:
            continue
        m = re.match(r"\s*(\d+)", str(row[1]))
        if not m or row[5] is None:
            continue
        # Colonnes : année, âge, France métropolitaine (ensemble, H, F), France (ensemble, H, F).
        fr.append((int(m.group(1)), int(row[5])))
    if not fr or fr[-1][0] != AGE_OPEN_FR:
        raise SystemExit("pyramide des âges France illisible")

    data = json.loads(decode(download(AGES_NC_URL, refresh)))
    nc = sorted((int(r["ager"]), int(r["n"])) for r in data["results"] if r["ager"] is not None)
    if sum(n for _, n in nc) != NC_2019_RECORDS:
        raise SystemExit(f"âges NC : {sum(n for _, n in nc)} enregistrements, {NC_2019_RECORDS} attendus")
    return {FR: fr, NC: nc}


# --- Métiers -------------------------------------------------------------------


def melodi_counts(raw: bytes) -> tuple[dict[str, float], str]:
    """Effectifs EQTP par code PCS-ESE (toutes autres dimensions à « _T »)."""
    text = zip_member(raw, ".csv", exclude="metadata")
    rows = csv_rows(text)
    if not rows:
        raise SystemExit("fichier Melodi vide")
    dims = [k for k in rows[0] if k not in ("PCS_ESE", "OBS_VALUE", "DERA_MEASURE",
                                           "UNIT_MEASURE", "TIME_PERIOD", "GEO",
                                           "GEO_OBJECT", "FREQ", "QUANTILE", "OBS_STATUS")]
    out: dict[str, float] = {}
    year = ""
    for r in rows:
        if r["DERA_MEASURE"] != "EFFECTIFS_EQTP" or any(r[d] != "_T" for d in dims):
            continue
        if r["OBS_VALUE"]:
            out[r["PCS_ESE"]] = float(r["OBS_VALUE"])
            year = r["TIME_PERIOD"]
    return out, year


def drees_count(raw: bytes) -> tuple[int, str]:
    """Effectif national, toutes catégories (première ligne de données), dernière année."""
    wb = xlsx(raw)
    ws = wb["Effectifs"]
    rows = ws.iter_rows(values_only=True)
    header = next(rows)
    first = next(rows)
    best: tuple[int, str] | None = None
    for h, v in zip(header, first):
        m = re.fullmatch(r"effectif_(\d{4})", str(h or ""))
        if m and isinstance(v, (int, float)):
            best = (int(v), m.group(1))
    labels = [str(v) for v in first if isinstance(v, str)]
    if best is None or not all("nsemble" in s or "France" in s for s in labels):
        raise SystemExit("fichier DREES : ligne nationale introuvable")
    return best


def build_occupations(refresh: bool):
    private, private_year = melodi_counts(download(PRIVATE_URL, refresh))
    public, public_year = melodi_counts(download(PUBLIC_URL, refresh))
    drees: dict[str, tuple[int, str]] = {}
    for key, att in DREES_FILES.items():
        drees[key] = drees_count(download(DREES_BASE + att, refresh))
    out: dict[str, tuple[int, str]] = {}
    for masc, fem, domain, sources in METIERS:
        total = 0.0
        for s in sources:
            kind, code = s.split(":", 1)
            if kind == "pcs":
                if code not in private and code not in public:
                    raise SystemExit(f"code PCS absent des deux fichiers : {code}")
                total += private.get(code, 0.0) + public.get(code, 0.0)
            elif kind == "drees":
                total += drees[code][0]
            else:
                raise SystemExit(f"source inconnue : {s}")
        count = round(total)
        if count <= 0:
            raise SystemExit(f"effectif nul pour {masc[0]}")
        broad_m, broad_f = DOMAINS[domain]
        seen_here: set[str] = set()
        for forms, broad in ((masc, broad_m), (fem, broad_f)):
            for form in forms:
                key = fold(form)
                if key in seen_here:
                    continue  # intitulé épicène : la catégorie masculine prime
                if key in out:
                    raise SystemExit(f"intitulé présent dans deux métiers : {form}")
                seen_here.add(key)
                out[key] = (count, broad)
    years = {"prive": private_year, "public": public_year}
    years.update({f"drees:{k}": v[1] for k, v in drees.items()})
    return out, years


# --- Écriture ------------------------------------------------------------------


def write(name: str, header: str, lines: list[str]) -> None:
    body = "".join(f"# {h}\n" for h in header.strip().splitlines()) + "".join(f"{l}\n" for l in lines)
    (DATA / name).write_text(body, encoding="utf-8", newline="\n")
    print(f"{name} : {len(lines)} lignes, {len(body.encode('utf-8'))} octets")


def write_sources(today: str, totals: dict[str, int], n_fr: int, years: dict[str, str]) -> None:
    section = f"""{POPULATION_MARKER}
# Données de population

Généré par `tools/prepare_population.py` le {today}. Ne pas éditer à la main.
Toutes ces sources sont publiées sous la Licence Ouverte / Open Licence 2.0 (Etalab),
compatible AGPL-3.0 (réutilisation libre avec mention de la source, faite ici).

## population_communes.txt, population_zones.txt, population_territoires.txt

- France : Insee, populations légales 2023 (millésime 2023, en vigueur au 1er janvier 2026,
  limites territoriales au 1er janvier 2025), parues le 18/12/2025.
  Page : {LEGAL_PAGE} ; fichier : {LEGAL_URL}.
  Population municipale (PMUN) des {n_fr} communes de France métropolitaine, des DROM et de
  Saint-Pierre-et-Miquelon, Saint-Barthélemy et Saint-Martin ; zone = département
  (ou collectivité), région = région (ou collectivité). Paris, Lyon et Marseille : somme de
  leurs arrondissements municipaux (qui restent aussi présents sous leur propre nom).
- Nouvelle-Calédonie : Insee et ISEE, populations de référence du recensement de 2025
  (décret n° 2025-966 du 18 septembre 2025), parues le 17/10/2025.
  Page : {NC_PAGE} ; fichier : {NC_URL}.
  Population municipale des {NEW_CALEDONIA_COMMUNES} communes (arrondie à l'unité).
  Noms : ceux de ce tableau, du jeu « Communes de la Nouvelle-Calédonie » ci-dessous et du
  Code officiel géographique 2026 (Insee, {COG_COMER_URL}).
- Rattachement des communes calédoniennes aux provinces : Gouvernement de la Nouvelle-Calédonie,
  jeu « Communes de la Nouvelle-Calédonie » (mis à jour le 29/07/2020), {NC_COMMUNES_PAGE}.
  Poya, partagée entre les provinces Nord et Sud, y est rattachée à la province Nord.
- Licence : Licence Ouverte / Open Licence 2.0 (Insee : https://www.insee.fr/fr/information/2008466 ;
  data.gouv.nc : licence indiquée sur chaque jeu).
- Traitement : noms pliés comme les dictionnaires, avec et sans article initial ;
  homonymes conservés. Population totale : France {totals[FR]}, Nouvelle-Calédonie {totals[NC]}.

## population_ages.txt

- France : Insee, bilan démographique 2025, tableau POP3 « Population au 1er janvier par sexe
  et âge détaillé », feuille {AGES_FR_SHEET} (estimation provisoire au 1er janvier 2026, France
  hors Mayotte et DROM compris), parue le 13/01/2026. Page : {AGES_FR_PAGE} ; fichier : {AGES_FR_URL}.
  Âge en années révolues ; « 105 ou plus » ramené à 105.
- Nouvelle-Calédonie : ISEE, recensement de la population 2019, fichier détail « Individus »
  (271 407 personnes), variable AGER (âge révolu), agrégée par l'API du portail.
  Page : {AGES_NC_PAGE}. Aucune répartition par âge du recensement de 2025 n'est encore publiée.

## metiers.txt

- Insee, Base Tous salariés, catalogue Melodi :
  « Salaires dans le secteur privé par catégorie socioprofessionnelle détaillée »
  (DS_DERA_PRIVE_ANNUEL, année {years['prive']}), {PRIVATE_URL} ;
  « Salaires dans la fonction publique par catégorie socioprofessionnelle détaillée »
  (DS_DERA_PUBLIC_ANNUEL, année {years['public']}), {PUBLIC_URL}.
  Effectifs en équivalent temps plein (EQTP), par profession PCS-ESE (4 positions),
  France ; salariés seulement (ni non-salariés, ni salariés agricoles ou des particuliers
  employeurs, ni militaires).
- DREES, « La démographie des professionnels de santé depuis 2012 », {DREES_PAGE} :
  médecins, chirurgiens-dentistes, pharmaciens, sages-femmes et masseurs-kinésithérapeutes
  (RPPS, au 1er janvier {years['drees:medecins']}, tous modes d'exercice) ; infirmières
  salariées (BTS) et libérales (SNDS), année {years['drees:infirmieres_liberales']}.
- Licence : Licence Ouverte / Open Licence 2.0.
- Traitement : chaque intitulé courant (masculin et féminin) est rattaché, dans le script,
  à un ou plusieurs codes PCS-ESE ou à une profession de la DREES ; l'effectif est la somme
  arrondie des effectifs de ces codes. La catégorie plus large est un regroupement écrit
  dans le script (contenu du projet, AGPL-3.0). Les métiers exercés surtout en indépendant
  hors santé (avocat, notaire, architecte, agriculteur, chauffeur de taxi…) sont écartés :
  aucune source ouverte ne les compte par profession. Pour les autres, l'effectif salarié
  en EQTP sous-estime le nombre de personnes : l'estimation de groupe qui en découle est
  prudente (groupes plus petits).
"""
    path = DATA / "SOURCES.md"
    old = path.read_text(encoding="utf-8") if path.exists() else ""
    pos = old.find(POPULATION_MARKER)
    head = (old[:pos] if pos >= 0 else old).rstrip("\n")
    text = (head + "\n\n" if head else "") + section
    path.write_text(text, encoding="utf-8", newline="\n")


def main() -> None:
    refresh = "--refresh" in sys.argv[1:]
    DATA.mkdir(parents=True, exist_ok=True)
    zones, entries, totals, n_fr = build_communes(refresh)
    ages = build_ages(refresh)
    occupations, years = build_occupations(refresh)
    distinct = len({m[0][0] for m in METIERS})
    if distinct < MIN_OCCUPATIONS:
        raise SystemExit(f"{distinct} métiers, {MIN_OCCUPATIONS} attendus au moins")

    used = {z for _, _, z in entries}
    zone_lines = [f"{c}\t{t}\t{label}\t{reg}" for c, (t, label, reg) in sorted(zones.items()) if c in used]
    write(
        "population_zones.txt",
        "Zones (département, province, collectivité). Généré par tools/prepare_population.py.\n"
        "code<TAB>territoire<TAB>zone<TAB>région",
        zone_lines,
    )
    write(
        "population_communes.txt",
        "Population municipale des communes. Généré par tools/prepare_population.py.\n"
        "nom plié<TAB>population<TAB>code de zone (homonymes sur plusieurs lignes)",
        [f"{n}\t{p}\t{z}" for n, p, z in entries],
    )
    write(
        "population_territoires.txt",
        "Population municipale totale. Généré par tools/prepare_population.py.\n"
        "code<TAB>territoire<TAB>population",
        [f"{FR}\tFrance\t{totals[FR]}", f"{NC}\tNouvelle-Calédonie\t{totals[NC]}"],
    )
    write(
        "population_ages.txt",
        "Population par âge en années révolues. Généré par tools/prepare_population.py.\n"
        "territoire<TAB>âge<TAB>effectif (le dernier âge de chaque territoire est ouvert)",
        [f"{t}\t{a}\t{n}" for t in (FR, NC) for a, n in ages[t]],
    )
    write(
        "metiers.txt",
        f"Effectifs par métier ({distinct} métiers). Généré par tools/prepare_population.py.\n"
        "intitulé plié<TAB>effectif France entière<TAB>catégorie plus large",
        [f"{k}\t{c}\t{b}" for k, (c, b) in sorted(occupations.items())],
    )
    write_sources(datetime.date.today().isoformat(), totals, n_fr, years)
    print(f"métiers distincts : {distinct} ; intitulés : {len(occupations)}")


if __name__ == "__main__":
    main()
