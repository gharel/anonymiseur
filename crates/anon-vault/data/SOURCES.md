# Liste de mots des phrases de passe

Généré par `tools/prepare_wordlist.py`. Ne pas éditer à la main.

## mots.txt

- 4096 mots français courants, de 4 à 8 lettres, sans accents, un par ligne.
- Source : `crates/anon-dict/data/mots_courants.txt`, lui-même tiré de Dicollecte / Grammalecte,
  « Lexique des formes fléchies du français » v7.7 (https://grammalecte.net/).
- Licence : Mozilla Public License 2.0 (compatible AGPL-3.0), données réutilisées sous forme
  de formes pliées.
- Traitement : palier de fréquence 3 (les plus courants), lettres a-z seulement, liste
  d'exclusion de termes grossiers, violents, sexuels ou liés à la maladie et à la mort,
  puis sélection déterministe d'au plus 4096 mots (ordre des empreintes SHA-256).
