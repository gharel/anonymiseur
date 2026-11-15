# Journal des modifications

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), versions [sémantiques](https://semver.org/lang/fr/).

## Non publié

### Ajouté
- **Contrats de la V2** (`anon-core`, module `assess`) : objectif (« Masquer pour l'IA », « Anonymiser définitivement »), destinataire, approche simplifiée ou contextuelle, entrées et résultat de l'évaluation des trois critères du CEPD (isolement, recoupement, déduction), corrections proposées. Aucun calcul encore.
- **Catégories particulières de données** (RGPD, art. 9 et 10) : santé, opinion politique ou philosophique, religion, appartenance syndicale, orientation sexuelle, origine ethnique, données génétiques et biométriques, condamnations ou infractions, dans une nouvelle famille « Sensibles », cherchée dans les deux profils. Nouveau type « Événement personnel » (licenciement, divorce, procédure), un indice du profil Renforcé. Pas encore de détection automatique : ces types se choisissent à la main.
- **Clé de restauration, format 2** : le fichier `.anonmap` retient l'objectif, le destinataire, la date de création et la date jusqu'à laquelle garder la clé. Les tables de la version 0.1 se relisent sans changement.
- **Lecture des tableaux** : contrat `Handler::tables` (une entrée par feuille), à implémenter pour Excel, Calc et CSV.
- **Interface V2, dossier de travail** : les trois onglets (Anonymiser, Désanonymiser, Texte libre) laissent place à un accueil et à un dossier de travail en quatre étapes, Vérifier, Bilan, Envoyer, Réponse, avec l'indicateur d'étapes dans l'en-tête. L'accueil reçoit les fichiers déposés ou un texte collé (Ctrl+V), qui devient un document du dossier, et propose deux objectifs : « Masquer pour l'IA » (clé de restauration) ou « Anonymiser définitivement » (aucune clé). Une clé `.anonmap` ouverte depuis l'accueil mène à l'étape Réponse. La barre du dossier montre l'objectif, le destinataire (retenu d'une session à l'autre) et le bouton Annuler ; la barre d'action ne porte qu'un message et l'action principale.
- **Réponse sans mot de passe** : la clé créée à l'étape Envoyer reste ouverte en mémoire ; la réponse de l'IA, collée (Ctrl+Maj+V), s'affiche à côté de sa version restaurée, et les balises inconnues sont signalées.
- **Palette de commandes** (Ctrl+K) : toutes les actions, trouvées sans tenir compte des accents ni des majuscules, choisies aux flèches, lancées par Entrée ; chaque action affiche son raccourci, ou la raison pour laquelle elle est impossible.
- **Annuler, rétablir** (Ctrl+Z, Ctrl+Maj+Z) chaque décision de vérification : cocher, décocher, type, majuscules, forme moins précise, ajout ou retrait d'une valeur, zone tracée sur une image.
- **Raccourcis** : Ctrl+Entrée pour l'étape suivante, Ctrl+1 à Ctrl+4 pour aller à une étape, Ctrl+Maj+V pour coller la réponse de l'IA et la restaurer ; panneau F1 mis à jour.
- **Copier le texte et la consigne** : à l'étape Envoyer, le texte masqué des documents, précédé de la consigne pour l'IA, en un clic.
- **Bilan provisoire** avant l'envoi : valeurs masquées, valeurs à confirmer, indices encore visibles et estimation du risque de la version 1, en attendant l'évaluation des trois critères.

### Modifié
- **Détection toujours en profil Renforcé** : le choix du niveau de protection disparaît ; les indices (dates, âges, fonctions, lieux, montants) sont toujours cherchés.
- **Vocabulaire** : « clé de restauration » (table de correspondance), « vérification finale » (contrôle de fuite), « indices » (catégorie Contexte), « rendre moins précis » (généralisation), « 3 fois » (occurrences), « respecter les majuscules » (casse).
- **Un seul message à la fois** : plus de bandeaux empilés à l'étape Vérifier ; un ajout de fichiers donne une seule notification. Le bandeau d'accueil et la liste « Toujours anonymiser » de l'écran de dépôt disparaissent ; la liste « Toujours masquer » reste à l'étape Vérifier.

### Corrigé
- **Conseil du rapport de risque** : en profil Renforcé, un risque élevé invite à cocher dans la liste les informations indirectes restées visibles, ou à les retirer du document. Le rapport recommandait jusqu'ici le profil Renforcé, déjà actif.

## [0.1.1] - 2026-11-14

### Corrigé
- **Mémoire de l'édition complète** : l'IA de noms traite un long bloc de texte par lots de huit fenêtres au lieu d'un seul. Sur les fichiers d'exemple, le pic de mémoire passe de 4,4 Go à 0,7 Go, pour la même durée. La version 0.1.0 pouvait saturer un ordinateur de 8 Go.

## [0.1.0] - 2026-11-14

Première version publique.

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
- **Notifications** discrètes en bas à droite (« Consigne copiée », « 3 fichiers ajoutés », « Exemples prêts »…), lues par les lecteurs d'écran ; une erreur reste affichée jusqu'à ce qu'on la ferme. Les boutons Copier passent brièvement à « Copié ».
- **Raccourcis clavier** : Ctrl+O (ajouter des fichiers, ou ouvrir une table), Ctrl+Entrée (action principale de l'étape), Échap (annuler le traitement, sinon revenir), Ctrl+1, 2, 3 (onglets), Ctrl+F (recherche dans le tableau), F1 (liste des raccourcis, aussi en pied de page). Cmd au lieu de Ctrl sous macOS.
- **Tableau de vérification** : recherche dans la valeur et le contexte, tri par type, valeur ou nombre d'occurrences, résumé « 42 valeurs cochées sur 57 · 3 à confirmer », bouton « Suivante à confirmer », cocher ou décocher les lignes affichées, ↑ ↓ et Espace pour parcourir et cocher.
- **Dépôt sur toute la fenêtre** : un voile indique ce qui va se passer ; dans Désanonymiser, une table déposée s'ouvre (mot de passe demandé) et les documents s'ajoutent ; dans Anonymiser, une table déposée est proposée à la reprise.
- **Désanonymiser** : « Coller depuis le presse-papiers » colle et restaure en un geste ; les balises inconnues sont surlignées dans le résultat, avec leur nombre.
- **Avancement détaillé** pendant l'analyse, l'anonymisation et la restauration : état de chaque fichier (en attente, en cours, terminé, erreur), temps écoulé, bouton Annuler.
- **Bandeau d'accueil** au premier lancement (« Anonymisez · Envoyez à l'IA avec la consigne · Désanonymisez la réponse »), refermable et retenu.

### Modifié
- **Une action désactivée dit pourquoi** : infobulle et ligne d'aide sous le bouton principal (« Ouvrez d'abord une table de correspondance », « Cochez au moins une valeur à anonymiser », « Les deux mots de passe diffèrent »).
- **Finitions** : survol et appui visibles sur tout ce qui se clique, curseur main, anneau de focus au clavier, nom lu par les lecteurs d'écran sur chaque bouton d'icône (vérifié par un test), textes tronqués avec leur version entière en infobulle, états vides avec une icône et une action.
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
