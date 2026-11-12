# Jeu de fichiers d'exemple

DONNÉES FICTIVES — exemple de test

Tous les fichiers de ce dossier sont **entièrement fictifs** et générés par `tools/make_examples.py`
(`uv run --project tools python tools/make_examples.py`). Les emails sont en `example.com` ou `example.org`.
La vérité terrain lisible par les tests est dans `attendu.json` : toute valeur sensible d'un fichier y figure.

## Contenu

| Fichier | Ce qu'il permet de vérifier |
|---|---|
| `contrat-prestation.docx` | nom coupé entre deux mises en forme, en-tête, pied de page, commentaire, révision suivie, lien `mailto:`, propriétés |
| `fichier-clients.xlsx` | tableau Excel, téléphones et montants saisis en nombres, formule, commentaire de cellule, graphique, 2e feuille |
| `presentation-projet.pptx` | diapositives, notes du présentateur, graphique, image contenant du texte |
| `facture.pdf` | PDF avec texte, métadonnées |
| `courrier-scanne.pdf`, `carte-visite.png` | images seules : aucun texte extractible, OCR nécessaire |
| `compte-rendu.odt`, `planning.ods`, `support.odp` | formats OpenDocument |
| `notes.txt`, `export.csv`, `brief.md` | texte brut, CSV à séparateur `;`, Markdown |
| `report-en.docx`, `informe-es.txt` | anglais et espagnol, téléphones internationaux |
| `reponse-ia-exemple.md` | réponse d'IA avec balises correctes, altérées et inventée |

Les mêmes sociétés et les mêmes sept personnes reviennent d'un fichier à l'autre : Jean Dupont (écrit aussi
`DUPONT Jean` et `J. Dupont`), Marie-Claire de La Fontaine, Hélène Kaloï, Martin Lefèvre, Pierre Garnier,
Anaïs Vaitiare, Olivier Nguyen.

## Scénario de test guidé

### 1. Anonymiser le contrat

1. Ouvrez l'application et déposez `contrat-prestation.docx`.
2. Vérifiez à l'écran que la liste des détections contient : les deux sociétés, les personnes, le RIDET, le
   SIRET, l'IBAN, les adresses (dont la boîte postale), les emails, les téléphones, les dates et les montants.
3. Contrôlez les emplacements difficiles : le nom « Jean Dupont » écrit en deux mises en forme (prénom en gras),
   l'en-tête, le pied de page, le commentaire de Hélène Kaloï, la révision suivie (Pierre Garnier supprimé,
   Olivier Nguyen inséré), le lien `mailto:` et les propriétés du document (auteur, société).
4. Contrôlez aussi que **ne sont pas détectés** : « Martinique », « pierre angulaire », « 1.2.3.4 » et
   « 123456789 ».
5. Validez, anonymisez, puis ouvrez le fichier produit : plus aucune valeur sensible ne doit y subsister, y compris
   dans le commentaire, la révision, le lien et les propriétés. Le contrôle de fuite doit passer sans alerte.

### 2. Cohérence des balises

Dans la même session, ajoutez `facture.pdf`, `notes.txt` et `brief.md`. Une même personne ou société doit
recevoir la même balise partout, y compris sous ses variantes (`Jean Dupont`, `DUPONT Jean`, `J. Dupont`).

### 3. Tableur

Déposez `fichier-clients.xlsx` : les colonnes Nom, Prénom, Email, Téléphone, Adresse, RIDET et Montant doivent être
reconnues. Vérifiez les téléphones saisis en nombres (par exemple `33612345678`), la formule (le nom dans le texte
entre guillemets), le commentaire de la cellule A2 et la feuille « Notes libres ». Le graphique doit survivre à
l'export. Aucun tableau croisé dynamique n'est fourni : Excel n'était pas pilotable par COM sur la machine de
génération.

### 4. Images et OCR

Déposez `courrier-scanne.pdf` et `carte-visite.png` : aucun texte n'est extractible, l'application doit passer par
l'OCR et proposer les valeurs (marquées `ocr: true` dans `attendu.json`). Dans `presentation-projet.pptx`, l'image de
la quatrième diapositive contient un contact (Olivier Nguyen) que seul l'OCR peut trouver ; les notes du
présentateur contiennent un nom et un téléphone.

### 5. Profil renforcé

Relancez l'analyse d'un fichier en profil renforcé : les dates, âges, fonctions (« directrice financière »),
montants et communes (Païta, Dumbéa, Koné) apparaissent en plus. Ils sont marqués `renforce` dans `attendu.json`.

### 6. Restauration

Après avoir anonymisé le contrat, collez `reponse-ia-exemple.md` dans la zone de restauration. Les balises
exactes sont remplacées par les vraies valeurs ; les formes altérées (`[Personne 1]`, `[PERSONNE\_1]`,
`**[SOCIETE_1]**`, `[personne_2]`) sont reconnues ; `[PERSONNE_99]` est signalée comme inconnue et reste intacte.
Si votre numérotation diffère de celle du fichier (ordre de détection), adaptez les numéros avant de comparer.

### 7. Autres formats et langues

Déposez `compte-rendu.odt`, `planning.ods`, `support.odp` (auteur en métadonnée, notes du présentateur dans
l'ODP), `export.csv` (séparateur `;`), `report-en.docx` et `informe-es.txt` (téléphones et IBAN internationaux).

## Contrôle automatique

`cargo test -p anon-engine` rejoue ces fichiers contre `attendu.json`. Chaque nouveau cas piège doit recevoir son
fichier ici et son entrée dans `attendu.json`.
