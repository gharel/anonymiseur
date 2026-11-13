# Anonymiseur

Anonymiseur remplace les données sensibles d'un document (noms, adresses, numéros, montants) par des balises comme `[PERSONNE_1]`. Vous pouvez alors envoyer le document à une IA. Quand l'IA répond, l'outil remet les vraies valeurs à la place des balises. Tout se passe sur votre ordinateur, dans un seul exécutable, sans connexion à Internet.

Il s'adresse aux apprenants et aux formateurs qui veulent utiliser une IA sur des documents contenant des données personnelles. C'est un projet de Skazy Formation, libre sous licence AGPL-3.0.

<!-- À COMPLÉTER : lien de téléchargement -->

## Le parcours en trois étapes

1. **Anonymiser.** Déposez votre fichier dans l'application. Elle propose une liste de données à masquer. Vous la relisez, vous corrigez, vous validez. L'application produit un fichier anonymisé et, sauf si vous choisissez le mode irréversible, une table de correspondance `.anonmap` protégée par un mot de passe que vous choisissez. Par défaut, ces fichiers sont écrits dans un dossier `Anonymisés` créé à côté de vos fichiers, pour ne pas mélanger sources et résultats. Un fichier déjà produit par l'outil (`_anonymise`, `_restaure`, `.anonmap`) n'est jamais anonymisé une seconde fois.
2. **Envoyer à l'IA.** Donnez à l'IA le fichier anonymisé avec la consigne fournie par l'application. Cette consigne demande à l'IA de conserver les balises telles quelles (texte dans `assets/consigne-ia.md`).
3. **Désanonymiser.** Collez la réponse de l'IA dans l'application et ouvrez votre table `.anonmap` avec son mot de passe : les balises sont remplacées par les vraies valeurs. Les balises que l'IA a légèrement abîmées (`[Personne 1]`, `[personne_2]`) sont reconnues. Une balise inconnue est signalée et laissée intacte.

Avant de livrer le fichier, l'application vérifie qu'aucune valeur validée n'y subsiste. Si elle en trouve une, elle ne crée pas le fichier.

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

- **Complète** : règles, dictionnaires et IA de noms (deux modèles embarqués, un pour le français, un pour les autres langues).
- **Lite** : règles et dictionnaires seulement, sans IA de noms.

Sur macOS avec processeur Intel, seule l'édition Lite est prévue.

## Données reconnues

Personnes (noms et prénoms), sociétés, lieux, adresses, emails, téléphones, IBAN, numéros de sécurité sociale, SIRET et SIREN, RIDET, cartes bancaires, adresses IP, liens, dates de naissance, plaques d'immatriculation, matricules et identifiants.

Deux profils :

- **Standard** : les identifiants directs (nom, email, téléphone, IBAN, etc.).
- **Renforcé** : le profil Standard, plus les informations indirectes qui peuvent désigner une personne par recoupement : dates, âges, fonctions, lieux, montants, matricules.

À la fin du traitement, l'application affiche un rapport de risque (Faible, Modéré, Élevé) calculé d'après les informations indirectes restées visibles, avec des conseils.

## Premier lancement

L'exécutable n'est pas signé. Votre système l'affichera avec un avertissement : c'est attendu.

- **Windows** : à l'avertissement SmartScreen, cliquez sur « Informations complémentaires », puis sur « Exécuter quand même ». Si Smart App Control bloque le fichier, faites un clic droit sur le fichier, Propriétés, cochez « Débloquer », puis validez.
- **macOS 15 et suivants** : ouvrez Réglages Système, Confidentialité et sécurité, puis cliquez sur « Ouvrir quand même » à côté du message concernant l'application.

## Limites à connaître

- **C'est une pseudonymisation réversible, pas une anonymisation au sens du RGPD.** Tant que la table `.anonmap` existe, les données peuvent être retrouvées. Même sans elle, un document peut rester identifiable par recoupement.
- **Aucune détection n'est exhaustive.** Un nom rare, une faute de frappe ou un format inhabituel peuvent passer. C'est pourquoi l'écran de validation existe : relisez-le toujours.
- **L'OCR lit mal l'écriture manuscrite.** Sur un scan, vérifiez le résultat à l'œil.
- **Casse des variantes.** Si une valeur apparaît sous une variante de casse, la restauration la remet sous sa forme canonique, pas sous la casse d'origine.
- **Noms de feuilles de tableur.** Ils ne sont pas modifiés.
- **Ce qui n'a pas été vérifié.** L'application n'a pas été testée sur macOS. Les fichiers Word, Excel et PowerPoint produits n'ont pas été ouverts dans Microsoft Office lui-même : ils ont été relus avec des bibliothèques tierces (python-docx, openpyxl, python-pptx). Le glisser-déposer réel et les lecteurs d'écran n'ont pas non plus été vérifiés.

Détails sur les données et leur protection : [CONFIDENTIALITE.md](CONFIDENTIALITE.md).

## Fichiers d'exemple

Le bouton « Essayer avec des exemples » charge en un clic de faux fichiers, entièrement fictifs, dans chaque format. Ils sont placés dans `Documents\Anonymiseur\Exemples` (sous macOS, `Documents/Anonymiseur/Exemples`) : le même dossier est réutilisé à chaque clic, seuls les fichiers d'exemple manquants sont recréés et aucun fichier existant n'est remplacé. Le fichier `LISEZMOI.md` de ce dossier propose un scénario guidé ; `reponse-ia-exemple.md` simule la réponse d'une IA pour l'onglet Désanonymiser. Les mêmes fichiers sont dans le dossier `exemples/` du dépôt.

## Thème et préférences

Le bouton en haut à droite choisit le thème : Système (par défaut, suit le réglage de l'ordinateur), Clair ou Sombre. Le thème, le dernier niveau de protection (Standard ou Renforcé) et la taille de la fenêtre sont retenus d'une session à l'autre dans un petit fichier `preferences.json` du dossier de configuration (`%APPDATA%\Anonymiseur` sous Windows, `~/Library/Application Support/Anonymiseur` sous macOS). Ce fichier ne contient aucun chemin de fichier ni aucun contenu de vos documents.

## Compiler depuis les sources

Voir [AGENTS.md](AGENTS.md) pour les commandes et l'architecture, et [crates/anon-pdf/BUILD.md](crates/anon-pdf/BUILD.md) pour les prérequis du PDF et de l'OCR (Windows et macOS). L'édition Lite se compile avec `--no-default-features` sur le paquet `anonymiseur`.

## Licence

GNU AGPL version 3 : voir [LICENSE](LICENSE). Les logos et la marque Skazy Formation ne sont pas couverts. Les composants tiers sont listés dans [THIRD-PARTY.md](THIRD-PARTY.md).
