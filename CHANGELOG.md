# Journal des modifications

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions [sémantiques](https://semver.org/lang/fr/).

## Non publié

### Ajouté
- **Formats** : Word, Excel et PowerPoint (`.docx`, `.xlsx`, `.pptx`), OpenDocument (`.odt`, `.ods`, `.odp`), PDF avec texte, PDF scannés et images (lecture par OCR en français et en anglais), texte, Markdown et CSV. Seuls les passages touchés sont modifiés, le reste du fichier est recopié à l'identique.
- **Détection** : personnes, sociétés, lieux, adresses, emails, téléphones, IBAN, numéros de sécurité sociale, SIRET, RIDET, cartes bancaires, adresses IP, liens, plaques, matricules. Reconnaissance des variantes d'un même nom (`Jean Dupont`, `DUPONT Jean`, `J. Dupont`) sous une seule balise.
- **Colonnes de tableur** : le type de donnée est déduit de l'en-tête de colonne.
- **Profil Renforcé** : en plus du profil Standard, dates, âges, fonctions, lieux, montants et matricules.
- **Listes personnelles** : termes à toujours anonymiser ou à ne jamais anonymiser.
- **IA de noms** (édition complète) : deux modèles embarqués, un pour le français, un pour les autres langues, choisis selon la langue du texte. L'édition Lite fonctionne sans.
- **Contrôle de fuite** : un fichier où subsiste une valeur validée n'est pas livré.
- **Table de correspondance `.anonmap`** chiffrée par mot de passe (Argon2id, AES-256-GCM), et mode irréversible sans table.
- **Désanonymisation** de la réponse d'une IA, y compris quand les balises sont légèrement altérées ; les balises inconnues sont signalées et laissées intactes.
- **Rapport de risque** (Faible, Modéré, Élevé) et conseils, à partir des informations indirectes restées visibles.
- **Consigne à donner à l'IA** pour qu'elle conserve les balises.
- **Messages utiles** pour les anciens formats Office (`.doc`, `.xls`, `.ppt`) et les raccourcis Google (`.gdoc`, `.gsheet`, `.gslides`).
- **Interface** à la charte Skazy Formation, en français, avec trois onglets.
- **Documents** : `README.md`, `CONFIDENTIALITE.md`, `THIRD-PARTY.md`, `LICENSE` (AGPL-3.0), `CONTRIBUTING.md` ; faux fichiers d'exemple dans `exemples/` avec un scénario guidé.
- **Garde-fous** : aucune dépendance réseau, vérifiée automatiquement à chaque modification du code.

### Limites connues
- Pseudonymisation réversible, pas anonymisation au sens du RGPD ; aucune détection n'est exhaustive, la relecture de l'écran de validation est nécessaire.
- L'OCR lit mal l'écriture manuscrite.
- Une variante de casse est restaurée sous sa forme canonique.
- Les noms de feuilles de tableur ne sont pas modifiés.
- Mesure sur les exemples fictifs, sans correction humaine : en profil Standard, 0 valeur encore lisible sur 188 ; en profil Renforcé, 1 sur 281 (une ville espagnole). Un titre de tableau comme « Responsable technique » peut être pris pour une fonction. Les noms seuls dans une cellule sont parfois manqués par l'IA de noms.
- Non vérifié : fonctionnement sur macOS, ouverture des fichiers produits dans Word, Excel et PowerPoint (relus seulement avec des bibliothèques tierces), glisser-déposer réel, lecteurs d'écran, pages PDF tournées et champs de formulaire PDF.
- L'exécutable n'est pas signé : le système affiche un avertissement au premier lancement.
- macOS avec processeur Intel : édition Lite seulement.
