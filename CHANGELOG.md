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
- **Interface** à la charte Skazy Formation, en français, avec trois onglets : Anonymiser, Désanonymiser, Texte libre. Bouton « Essayer avec des exemples », reprise d'une table existante pour garder les mêmes balises, bouton « Nouvelle session ».
- **Images et pages scannées** : aperçu des zones qui seront masquées, et tracé à la souris de zones supplémentaires quand la lecture a manqué quelque chose (signature, tampon, écriture manuscrite). L'image d'origine revient à la restauration.
- **Généralisation** en profil Renforcé : une date remplacée par son année, un âge par sa tranche de dix ans, un montant par son ordre de grandeur. Irréversible.
- **Exécutables Windows autonomes** : `Anonymiseur.exe` (complet) et `Anonymiseur-Lite.exe`, sans aucune bibliothèque à installer. Paquet macOS et publication automatique préparés.
- **Documents** : `README.md`, `CONFIDENTIALITE.md`, `THIRD-PARTY.md`, `LICENSE` (AGPL-3.0), `CONTRIBUTING.md` ; faux fichiers d'exemple dans `exemples/` avec un scénario guidé.
- **Garde-fous** : aucune dépendance réseau, vérifiée automatiquement à chaque modification du code.
- **Mode sombre** : thème Système (par défaut, suit le réglage de l'ordinateur), Clair ou Sombre, depuis un bouton de l'en-tête. Palette sombre dérivée de la charte (fond `#1a1a1a`, vert de marque éclairci, badges de catégorie adaptés, logo clair) ; chaque couple texte / fond et chaque élément d'interface est vérifié par un test de contraste WCAG AA dans les deux thèmes.
- **Préférences retenues** d'une session à l'autre : thème, niveau de protection, taille de la fenêtre. Petit fichier `preferences.json` dans le dossier de configuration du système, sans aucun chemin de fichier ni contenu.
- **Sélecteur à la charte** (bord, rayon, chevron, anneau de focus) à la place du sélecteur natif d'egui.

### Modifié
- **Contrôles alignés** : un champ, un sélecteur, un bouton ou un sélecteur segmenté posés sur une même ligne ont la même hauteur (32, 40 ou 48 px, définies dans le thème) et le même centre ; un test le mesure sur chaque écran.
- **Zones de texte plus lisibles** (texte libre, réponse collée, résultat de la désanonymisation) : police de 15 px, interligne plus large, zones qui occupent la hauteur de la fenêtre et défilent à l'intérieur.
- **« Essayer avec des exemples »** : un seul clic, sans dialogue. Les exemples sont placés dans `Documents\Anonymiseur\Exemples`, toujours le même dossier : seuls les fichiers d'exemple manquants sont recréés, aucun fichier existant n'est remplacé, et seuls les documents d'exemple sont chargés.
- **Fichiers produits par l'outil écartés** : un fichier `_anonymise`, `_restaure` ou une table `.anonmap` ajouté à l'onglet Anonymiser n'est jamais anonymisé une seconde fois ; un message indique combien ont été ignorés.
- **Dossier de sortie par défaut** : sous-dossier `Anonymisés` à côté du premier fichier, créé à l'écriture, pour ne plus mélanger sources et résultats.

### Limites connues
- Pseudonymisation réversible, pas anonymisation au sens du RGPD ; aucune détection n'est exhaustive, la relecture de l'écran de validation est nécessaire.
- L'OCR lit mal l'écriture manuscrite.
- Une variante de casse est restaurée sous sa forme canonique.
- Les noms de feuilles de tableur ne sont pas modifiés.
- Mesure sur les exemples fictifs, PDF et images compris, sans correction humaine : en profil Standard, 0 valeur encore lisible sur 220 ; en profil Renforcé, 1 sur 327 en édition Lite (une ville espagnole) et 0 en édition complète. Ces fichiers ont servi à régler les détecteurs : le chiffre garantit la non-régression, pas le résultat sur vos documents.
- Sur un jeu de contrôle indépendant de 109 phrases, l'IA de noms fait passer le rappel des personnes de 85,5 % à 96,4 % en français, et de 71-78 % à 100 % dans les autres langues. Elle ajoute des faux positifs, surtout des lieux.
- Un titre de tableau comme « Responsable technique » peut être pris pour une fonction, et l'en-tête de colonne « Ville » pour un lieu : ils se décochent à l'écran de validation.
- Non vérifié : fonctionnement sur macOS, publication par GitHub Actions, ouverture des fichiers produits dans Word, Excel et PowerPoint (relus seulement avec des bibliothèques tierces), glisser-déposer et tracé de zones dans la vraie fenêtre (testés par événements simulés), lecteurs d'écran, pages PDF tournées et champs de formulaire PDF.
- L'exécutable n'est pas signé : le système affiche un avertissement au premier lancement.
- macOS avec processeur Intel : édition Lite seulement.
