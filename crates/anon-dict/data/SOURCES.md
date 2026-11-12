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
