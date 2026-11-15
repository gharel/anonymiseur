# Anonymiseur

Anonymiseur remplace les données sensibles d'un document (noms, adresses, numéros, montants, indices) par des balises comme `[PERSONNE_1]`. Vous pouvez alors envoyer le document à une IA. Quand l'IA répond, l'outil remet les vraies valeurs à la place des balises. Avant l'envoi, un bilan vérifie si quelqu'un reste reconnaissable, selon les trois critères du Comité européen de la protection des données. Tout se passe sur votre ordinateur, dans un seul exécutable, sans connexion à Internet.

Il s'adresse aux apprenants et aux formateurs qui veulent utiliser une IA sur des documents contenant des données personnelles. C'est un projet de Skazy Formation, libre sous licence AGPL-3.0.

**[Télécharger pour Windows ou macOS](https://gharel.github.io/anonymiseur/)** : page de présentation, empreintes SHA-256 et lien d'analyse VirusTotal pour chaque fichier. Les exécutables sont aussi dans les [publications du dépôt](https://github.com/gharel/anonymiseur/releases/latest).

## Deux objectifs

- **Masquer pour l'IA** : vous récupérez la réponse de l'IA avec les vraies valeurs. L'outil crée une clé de restauration chiffrée. C'est une pseudonymisation : les fichiers produits restent des données personnelles.
- **Anonymiser définitivement** : aucune clé, aucun retour possible. Si le bilan conclut que personne n'est reconnaissable pour le destinataire choisi, le fichier sort du champ du RGPD et prend le suffixe `_anonyme`.

## Le parcours

1. **Déposer.** Glissez vos fichiers dans la fenêtre, ou collez un texte (Ctrl+V) : il devient un document du dossier de travail.
2. **Vérifier.** Le texte du document s'affiche, chaque valeur surlignée : en gris ce qui sera masqué, en jaune ce qui reste à décider. La file « À décider » présente les cas douteux un par un (M masque, G garde, J et K passent au suivant ou au précédent). Sélectionnez un passage pour le masquer partout. Le tableau de la version 1 reste accessible par « Vue tableau ».
3. **Bilan.** Pour le destinataire choisi (assistant IA grand public, IA de votre organisation, partage externe, publication), l'outil évalue l'isolement, le recoupement et la déduction, personne par personne. Chaque échec vient avec sa correction : masquer, rendre moins précis (une commune devient un département, un âge une tranche), supprimer quelques lignes d'un tableau, masquer un visage. « Tout corriger » applique les corrections conseillées ; Ctrl+Z les annule.
4. **Envoyer.** L'outil produit les fichiers, vérifie qu'aucune valeur masquée n'y subsiste (sinon il ne les crée pas), puis propose de copier le texte avec la consigne pour l'IA. La clé de restauration est enregistrée dans `Documents\Anonymiseur\Clés`, protégée par une phrase de passe générée ou la vôtre. La fiche d'évaluation (Markdown ou PDF) garde la trace de ce qui a été fait.
5. **Réponse.** Collez la réponse de l'IA (Ctrl+Maj+V) : elle s'affiche à côté de sa version restaurée. Les balises que l'IA a légèrement abîmées (`[Personne 1]`, `[personne_2]`) sont reconnues ; une balise inconnue est signalée et laissée intacte. Copiez le résultat ou enregistrez-le en `.md` ou `.docx`.

Un fichier déjà produit par l'outil (`_masque`, `_anonyme`, `_restaure`, `.anonmap`) n'est jamais traité une seconde fois.

## Presse-papiers express

Pour un échange rapide avec une IA, sans passer par les étapes, l'application garde deux raccourcis actifs même quand sa fenêtre est en arrière-plan :

- **Ctrl+Alt+M** masque le texte copié, avec les mêmes balises que le dossier ouvert, et le remet dans le presse-papiers : il ne reste qu'à le coller dans l'IA. S'il reste des valeurs à décider, une petite fenêtre de vérification s'ouvre d'abord (M masque, G garde, Entrée remplace le presse-papiers, Échap annule).
- **Ctrl+Alt+R** restaure le texte copié, par exemple la réponse de l'IA, avec la clé ouverte.

Une notification en bas à droite de l'écran donne le résultat (« 12 valeurs masquées, presse-papiers prêt »), sans jamais afficher le texte copié. Sans dossier ouvert, le texte masqué devient le document « presse-papiers.txt » d'un dossier gardé en mémoire jusqu'à la fermeture, pour pouvoir restaurer ensuite. Si une autre application utilise déjà l'un de ces raccourcis, l'application le signale ; les actions « Masquer le presse-papiers » et « Restaurer le presse-papiers » restent disponibles dans la palette (Ctrl+K), où les raccourcis globaux se désactivent aussi.

## Formats pris en charge

| Type | Extensions |
|---|---|
| Word | `.docx` |
| Excel | `.xlsx` |
| PowerPoint | `.pptx` |
| OpenDocument | `.odt`, `.ods`, `.odp` |
| PDF avec texte | `.pdf` |
| PDF scanné | `.pdf` (lu par OCR) |
| Images | `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.tif`, `.tiff`, `.webp` (lues par OCR) |
| Texte | `.txt`, `.md`, `.csv` |

- **Google Docs, Sheets, Slides** : dans Google, choisissez Fichier, Télécharger, puis le format Microsoft Office ou OpenDocument. Un raccourci `.gdoc` ou `.gsheet` ne contient pas le document.
- **Anciens formats `.doc`, `.xls`, `.ppt`** : ouvrez le fichier dans Word, Excel ou PowerPoint et réenregistrez-le au format moderne. L'application affiche ce conseil si vous en déposez un.

## Deux éditions

- **Complète** : règles, dictionnaires, IA de noms (deux modèles embarqués, un pour le français, un pour les autres langues) et détection des visages dans les images.
- **Lite** : règles et dictionnaires seulement. Les visages ne sont pas vérifiés : le bilan d'un dossier qui contient des images est alors « non concluant ».

Sur macOS avec processeur Intel, seule l'édition Lite est prévue.

## Données reconnues

- **Identifiants directs** : personnes (noms et prénoms), sociétés, adresses, emails, téléphones, IBAN, numéros de sécurité sociale, SIRET et SIREN, RIDET, cartes bancaires, adresses IP, liens, plaques d'immatriculation, matricules et identifiants.
- **Indices** : dates, âges, fonctions, lieux, codes postaux, montants, événements personnels (licenciement, divorce, arrêt de travail). Pris ensemble, ils peuvent suffire à reconnaître quelqu'un ; le bilan estime combien de personnes partagent ces indices, d'après les chiffres de population de l'Insee et de l'ISEE (France et Nouvelle-Calédonie).
- **Données sensibles** (RGPD, articles 9 et 10) : santé, opinion politique ou philosophique, religion, appartenance syndicale, orientation sexuelle, origine, données génétiques et biométriques, condamnations. Elles ne sont retenues que rattachées à une personne (« suivi pour une dépression », « déléguée syndicale »).
- **Colonnes de tableur** : le type de donnée est déduit de l'en-tête (« Nom », « Diagnostic », « Code postal », « N° dossier »…).

## Premier lancement

L'exécutable n'est pas signé. Votre système l'affichera avec un avertissement : c'est attendu.

- **Windows** : à l'avertissement SmartScreen, cliquez sur « Informations complémentaires », puis sur « Exécuter quand même ». Si Smart App Control bloque le fichier, faites un clic droit sur le fichier, Propriétés, cochez « Débloquer », puis validez.
- **macOS 15 et suivants** : ouvrez Réglages Système, Confidentialité et sécurité, puis cliquez sur « Ouvrir quand même » à côté du message concernant l'application.

## Limites à connaître

- **« Masquer pour l'IA » est une pseudonymisation.** Tant que la clé existe, les données peuvent être retrouvées.
- **Le bilan est une estimation.** Il suit l'approche simplifiée des lignes directrices 02/2026 du CEPD : les tailles de groupe viennent de statistiques nationales, pas de ce que le destinataire sait déjà. La décision d'anonymat et sa justification restent celles du responsable de traitement.
- **Aucune détection n'est exhaustive.** Un nom rare, une faute de frappe ou un format inhabituel peuvent passer. C'est pourquoi l'étape Vérifier existe : relisez-la toujours.
- **L'OCR lit mal l'écriture manuscrite**, et le détecteur de visages peut manquer un visage de profil ou caché. Sur un scan ou une photo, vérifiez le résultat à l'œil.
- **Casse des variantes.** Si une valeur apparaît sous une variante de casse, la restauration la remet sous sa forme canonique, pas sous la casse d'origine.
- **Noms de feuilles de tableur.** Ils ne sont pas modifiés.
- **Ce qui n'a pas été vérifié.** L'application n'a pas été testée sur macOS. Les fichiers Word, Excel et PowerPoint produits n'ont pas été ouverts dans Microsoft Office lui-même : ils ont été relus avec des bibliothèques tierces (python-docx, openpyxl, python-pptx). Le glisser-déposer réel et les lecteurs d'écran n'ont pas non plus été vérifiés.

Détails sur les données et leur protection : [CONFIDENTIALITE.md](CONFIDENTIALITE.md).

## Fichiers d'exemple

Le bouton « Essayer avec des exemples » charge en un clic de faux fichiers, entièrement fictifs, dans chaque format. Ils sont placés dans `Documents\Anonymiseur\Exemples` (sous macOS, `Documents/Anonymiseur/Exemples`) : le même dossier est réutilisé à chaque clic, seuls les fichiers d'exemple manquants sont recréés et aucun fichier existant n'est remplacé. Le fichier `LISEZMOI.md` de ce dossier propose un scénario guidé ; `reponse-ia-exemple.md` simule la réponse d'une IA pour l'étape Réponse ; `patients.xlsx`, `compte-rendu-commission.docx` et `courrier-arret-maladie.txt` montrent le bilan à l'œuvre. Les mêmes fichiers sont dans le dossier `exemples/` du dépôt.

## Raccourcis clavier

| Touches | Action |
|---|---|
| Ctrl+K | Palette de commandes : toutes les actions, par recherche |
| Ctrl+O | Ajouter des fichiers au dossier de travail |
| Ctrl+V | Coller un texte : il devient un document du dossier |
| Ctrl+Entrée | Étape suivante |
| Échap | Annuler le traitement en cours, sinon revenir |
| Ctrl+Z, Ctrl+Maj+Z | Annuler, rétablir une décision ou une correction |
| Ctrl+1 à Ctrl+4 | Aller à l'étape Vérifier, Bilan, Envoyer ou Réponse |
| Ctrl+Maj+V | Coller la réponse de l'IA et la restaurer |
| Ctrl+Alt+M | Masquer le texte copié, même application en arrière-plan |
| Ctrl+Alt+R | Restaurer le texte copié, même application en arrière-plan |
| Ctrl+F, / | Rechercher dans le document ou le tableau |
| J, K | Valeur à décider suivante, précédente |
| M, G | Masquer, garder la valeur à décider |
| ← → ↑ ↓, Espace | Parcourir les valeurs du texte ou du tableau, les masquer ou les garder |
| Maj+← → | Étendre la sélection d'un mot, pour la masquer partout |
| F1 | Afficher les raccourcis (aussi en pied de page) |

Sous macOS, Cmd remplace Ctrl, et Option remplace Alt.

## Thème et préférences

Le bouton en haut à droite choisit le thème : Système (par défaut, suit le réglage de l'ordinateur), Clair ou Sombre. Le thème, le dernier destinataire, la taille de la fenêtre et l'activation des raccourcis du presse-papiers sont retenus d'une session à l'autre dans un petit fichier `preferences.json` du dossier de configuration (`%APPDATA%\Anonymiseur` sous Windows, `~/Library/Application Support/Anonymiseur` sous macOS). Ce fichier ne contient aucun chemin de fichier ni aucun contenu de vos documents.

## Compiler depuis les sources

Voir [AGENTS.md](AGENTS.md) pour les commandes et l'architecture, et [crates/anon-pdf/BUILD.md](crates/anon-pdf/BUILD.md) pour les prérequis du PDF et de l'OCR (Windows et macOS). L'édition Lite se compile avec `--no-default-features` sur le paquet `anonymiseur`.

## Licence

GNU AGPL version 3 : voir [LICENSE](LICENSE). Les logos et la marque Skazy Formation ne sont pas couverts. Les composants tiers sont listés dans [THIRD-PARTY.md](THIRD-PARTY.md).
