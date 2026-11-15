# Sources des dictionnaires embarqués

Généré par `tools/prepare_dicts.py` le 2026-11-12. Ne pas éditer à la main.
Les listes sont pliées (minuscules, sans accents, apostrophes et tirets normalisés)
et portent un palier de fréquence (1 = rare … 3 = très courant).

## prenoms.txt

- Source : Insee, « Fichier des prénoms », édition 2025 (naissances 1900-2025, France hors Mayotte).
- Page : https://www.insee.fr/fr/statistiques/8595130 (parue le 16/07/2026)
- Fichier utilisé : https://www.insee.fr/fr/statistiques/fichier/8595130/prenoms-2025-nat_csv.zip
- Licence : Licence Ouverte / Open Licence 2.0 (Etalab), données publiques de l'Insee.
- Traitement : effectifs cumulés tous sexes et toutes années, prénoms d'au moins
  300 naissances ; « _PRENOMS_RARES » écarté. Paliers :
  3 si >= 20000, 2 si >= 3000, 1 sinon.

## noms.txt

- Source : Insee, « Fichier des noms de famille » (noms attribués au moins 30 fois de 1891 à 2000,
  France métropolitaine et DOM), édition parue le 22/05/2018. Aucune édition plus récente
  n'est publiée par l'Insee.
- Page : https://www.insee.fr/fr/statistiques/3536630
- Fichier utilisé : https://www.insee.fr/fr/statistiques/fichier/3536630/noms2008nat_txt.zip
- Licence : Licence Ouverte / Open Licence 2.0 (Etalab).
- Traitement : somme des effectifs par décennie. Paliers : 3 si >= 20000,
  2 si >= 3000, 1 sinon.

## communes.txt

- Source : Insee, Code officiel géographique (COG), millésime 2026 (situation au 01/01/2026),
  diffusé aussi sur https://www.data.gouv.fr/datasets/code-officiel-geographique-cog.
- Fichiers utilisés : https://www.insee.fr/fr/statistiques/fichier/8740222/v_commune_2026.csv (communes, communes déléguées et associées ;
  arrondissements municipaux écartés) et https://www.insee.fr/fr/statistiques/fichier/8740222/v_commune_comer_2026.csv (communes des collectivités
  d'outre-mer, dont les 33 communes de Nouvelle-Calédonie).
- Licence : Licence Ouverte / Open Licence 2.0 (Etalab).
- Traitement : variantes NCCENR (« Mont-Dore ») et LIBELLE (« Le Mont-Dore »). Palier 3 =
  commune, 2 = commune déléguée ou associée. Chefs-lieux d'outre-mer ajoutés s'ils manquent :
  Paris, Lyon, Marseille, Fort-de-France, Cayenne, Saint-Denis, Mamoudzou, Basse-Terre, Pointe-à-Pitre, Papeete, Mata-Utu, Nouméa, Saint-Pierre, Gustavia, Marigot.

## mots_courants.txt

- Source principale : Dicollecte / Grammalecte, « Lexique des formes fléchies du français »
  v7.7, https://grammalecte.net/ ; fichier utilisé : https://www.grammalecte.net/dic/lexique-grammalecte-fr-v7.7.zip.
- Licence : Mozilla Public License 2.0 (compatible AGPL-3.0 ; les données sont réutilisées
  en l'état, sous forme de formes pliées).
- Traitement : formes à initiale minuscule, hors noms propres (étiquettes npr, prn, patr),
  d'indice de fréquence >= 6 (échelle 0-9 du lexique), soit environ
  30 000 formes (noms communs, adjectifs, verbes conjugués, adverbes). Un prénom ou nom
  de palier 3 dont l'indice est 6 (« jean », « marie »)
  n'est pas retenu : il reste détectable. Palier 3 = indice >= 7, 2 sinon.
- Ajouts écrits à la main (contenu du projet, AGPL-3.0) : mots courants qui sont aussi des
  prénoms ou noms (pierre, rose, marin, blanc, petit…), jours, mois, titres, mots de
  courrier, pays, régions et langues (palier 3).

<!-- prepare_population.py -->
# Données de population

Généré par `tools/prepare_population.py` le 2026-11-15. Ne pas éditer à la main.
Toutes ces sources sont publiées sous la Licence Ouverte / Open Licence 2.0 (Etalab),
compatible AGPL-3.0 (réutilisation libre avec mention de la source, faite ici).

## population_communes.txt, population_zones.txt, population_territoires.txt

- France : Insee, populations légales 2023 (millésime 2023, en vigueur au 1er janvier 2026,
  limites territoriales au 1er janvier 2025), parues le 18/12/2025.
  Page : https://www.insee.fr/fr/statistiques/8680726 ; fichier : https://www.insee.fr/fr/statistiques/fichier/8680726/ensemble.zip.
  Population municipale (PMUN) des 34904 communes de France métropolitaine, des DROM et de
  Saint-Pierre-et-Miquelon, Saint-Barthélemy et Saint-Martin ; zone = département
  (ou collectivité), région = région (ou collectivité). Paris, Lyon et Marseille : somme de
  leurs arrondissements municipaux (qui restent aussi présents sous leur propre nom).
- Nouvelle-Calédonie : Insee et ISEE, populations de référence du recensement de 2025
  (décret n° 2025-966 du 18 septembre 2025), parues le 17/10/2025.
  Page : https://www.insee.fr/fr/statistiques/8658741 ; fichier : https://www.insee.fr/fr/statistiques/fichier/8658741/NC2025_Annexe_TableauxDecret_pour%20Insee.fr.xlsx.
  Population municipale des 33 communes (arrondie à l'unité).
  Noms : ceux de ce tableau, du jeu « Communes de la Nouvelle-Calédonie » ci-dessous et du
  Code officiel géographique 2026 (Insee, https://www.insee.fr/fr/statistiques/fichier/8740222/v_commune_comer_2026.csv).
- Rattachement des communes calédoniennes aux provinces : Gouvernement de la Nouvelle-Calédonie,
  jeu « Communes de la Nouvelle-Calédonie » (mis à jour le 29/07/2020), https://data.gouv.nc/explore/dataset/communes-nc/.
  Poya, partagée entre les provinces Nord et Sud, y est rattachée à la province Nord.
- Licence : Licence Ouverte / Open Licence 2.0 (Insee : https://www.insee.fr/fr/information/2008466 ;
  data.gouv.nc : licence indiquée sur chaque jeu).
- Traitement : noms pliés comme les dictionnaires, avec et sans article initial ;
  homonymes conservés. Population totale : France 68141890, Nouvelle-Calédonie 264596.

## population_ages.txt

- France : Insee, bilan démographique 2025, tableau POP3 « Population au 1er janvier par sexe
  et âge détaillé », feuille 2026 (estimation provisoire au 1er janvier 2026, France
  hors Mayotte et DROM compris), parue le 13/01/2026. Page : https://www.insee.fr/fr/statistiques/8721185 ; fichier : https://www.insee.fr/fr/statistiques/fichier/8721185/3_Pop1janv_age.xlsx.
  Âge en années révolues ; « 105 ou plus » ramené à 105.
- Nouvelle-Calédonie : ISEE, recensement de la population 2019, fichier détail « Individus »
  (271 407 personnes), variable AGER (âge révolu), agrégée par l'API du portail.
  Page : https://data.gouv.nc/explore/dataset/recensement-de-la-population-2019-individus-nc/. Aucune répartition par âge du recensement de 2025 n'est encore publiée.

## metiers.txt

- Insee, Base Tous salariés, catalogue Melodi :
  « Salaires dans le secteur privé par catégorie socioprofessionnelle détaillée »
  (DS_DERA_PRIVE_ANNUEL, année 2024), https://api.insee.fr/melodi/file/DS_DERA_PRIVE_ANNUEL/DS_DERA_PRIVE_ANNUEL_2024_CSV_FR ;
  « Salaires dans la fonction publique par catégorie socioprofessionnelle détaillée »
  (DS_DERA_PUBLIC_ANNUEL, année 2023), https://api.insee.fr/melodi/file/DS_DERA_PUBLIC_ANNUEL/DS_DERA_PUBLIC_ANNUEL_2023_CSV_FR.
  Effectifs en équivalent temps plein (EQTP), par profession PCS-ESE (4 positions),
  France ; salariés seulement (ni non-salariés, ni salariés agricoles ou des particuliers
  employeurs, ni militaires).
- DREES, « La démographie des professionnels de santé depuis 2012 », https://data.drees.solidarites-sante.gouv.fr/explore/dataset/la-demographie-des-professionnels-de-sante-depuis-2012/ :
  médecins, chirurgiens-dentistes, pharmaciens, sages-femmes et masseurs-kinésithérapeutes
  (RPPS, au 1er janvier 2026, tous modes d'exercice) ; infirmières
  salariées (BTS) et libérales (SNDS), année 2023.
- Licence : Licence Ouverte / Open Licence 2.0.
- Traitement : chaque intitulé courant (masculin et féminin) est rattaché, dans le script,
  à un ou plusieurs codes PCS-ESE ou à une profession de la DREES ; l'effectif est la somme
  arrondie des effectifs de ces codes. La catégorie plus large est un regroupement écrit
  dans le script (contenu du projet, AGPL-3.0). Les métiers exercés surtout en indépendant
  hors santé (avocat, notaire, architecte, agriculteur, chauffeur de taxi…) sont écartés :
  aucune source ouverte ne les compte par profession. Pour les autres, l'effectif salarié
  en EQTP sous-estime le nombre de personnes : l'estimation de groupe qui en découle est
  prudente (groupes plus petits).
