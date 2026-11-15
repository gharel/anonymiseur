# Confidentialité

## Tout reste sur votre ordinateur

Vos documents, les données détectées et les résultats ne quittent jamais votre machine. L'application ne se connecte pas à Internet. L'IA de noms et le détecteur de visages sont des modèles embarqués dans l'exécutable : ils tournent localement. Les chiffres de population qui servent à l'évaluation (Insee, ISEE) sont eux aussi embarqués.

**Ce qui le garantit.** Aucune bibliothèque réseau n'est liée à l'application. À chaque modification du code, deux contrôles automatiques le vérifient. L'outil `cargo deny` refuse les dépendances réseau (par exemple `reqwest`, `hyper`, `tokio`). Une tâche dédiée examine en plus l'arbre des dépendances de l'application sur Windows et sur macOS. Les téléchargements (modèles, dictionnaires, données de population, polices) n'existent que dans les scripts de préparation du dépôt, jamais dans l'application. Ces contrôles portent sur le code : si vous voulez une preuve de votre côté, un pare-feu qui bloque l'application suffit à la constater.

## Deux objectifs

- **Masquer pour l'IA** : les valeurs sont remplacées par des balises et une clé de restauration est créée. C'est une pseudonymisation : avec la clé et son mot de passe, tout est réversible. Les fichiers produits restent des données personnelles au sens du RGPD.
- **Anonymiser définitivement** : aucune clé n'est créée, aucune image d'origine n'est gardée et la numérotation des balises est propre à cet envoi. Le fichier ne prend le suffixe `_anonyme` que si l'évaluation des trois critères conclut que personne n'y est reconnaissable ; sinon il s'appelle `_masque_sans_cle`.

## Ce qui est écrit sur le disque

- **Les fichiers produits**, à l'endroit que vous choisissez.
- **La clé de restauration `.anonmap`** (objectif « Masquer pour l'IA »), par défaut dans `Documents\Anonymiseur\Clés`, un dossier distinct de celui des fichiers produits. Elle contient les vraies valeurs, leurs balises, l'objectif, le destinataire, sa date de création et la date jusqu'à laquelle la garder. Elle est chiffrée avec une clé dérivée de votre mot de passe (Argon2id) et l'algorithme AES-256-GCM. **Le mot de passe n'est pas récupérable** : personne, y compris Skazy Formation, ne peut ouvrir la clé sans lui. Un mot de passe oublié signifie une clé perdue. Un mauvais mot de passe et un fichier altéré donnent le même message d'erreur.
- **Le mot de passe d'une clé, si vous cochez « Retenir le mot de passe sur cet ordinateur »** : il est confié au Gestionnaire d'identification de Windows ou au Trousseau de macOS, sous un identifiant tiré du fichier de la clé, sans aucun contenu de vos documents. Toute personne qui ouvre votre session peut alors restaurer sans mot de passe. « Détruire la clé » efface le fichier et cette entrée.
- **La fiche d'évaluation**, si vous l'enregistrez (Markdown ou PDF). Elle ne contient que des balises, des catégories et des nombres ; le nom des fichiers seulement si vous le demandez.
- **Les préférences** (`preferences.json`) : thème, taille de la fenêtre, dernier destinataire, activation des raccourcis du presse-papiers. Aucun chemin de fichier, aucun contenu.

## Ce qui n'est jamais écrit

- La clé de restauration en clair.
- Un journal contenant du contenu de vos documents. Les messages d'erreur et d'avertissement ne citent que des emplacements (par exemple `word/document.xml`, `page 3`) et des balises.
- Un fichier temporaire contenant du contenu de vos documents. Les images, le texte reconnu par l'OCR et les documents sont traités en mémoire.

**Une exception.** Pour l'OCR, les données de langue (français et anglais) sont extraites dans un dossier temporaire privé le temps du traitement, puis supprimées. Elles sont embarquées dans l'exécutable et ne contiennent rien qui vienne de vous.

## Presse-papiers express

Les raccourcis Ctrl+Alt+M et Ctrl+Alt+R lisent le presse-papiers seulement quand vous les pressez. Le texte copié est traité en mémoire, jamais écrit sur le disque. Sans dossier ouvert, la clé qui permet de le restaurer reste en mémoire jusqu'à la fermeture de l'application. La notification n'affiche que des nombres et des balises. Le texte restauré est placé dans le presse-papiers sans entrer dans l'historique du presse-papiers ni dans sa synchronisation entre appareils (Windows) ; sous macOS, il est marqué pour que les gestionnaires d'historique l'ignorent. Le texte masqué, lui, n'est remplacé dans le presse-papiers que si aucune valeur masquée n'y subsiste.

## Durée de conservation

À la création d'une clé, vous choisissez combien de temps la garder (12 mois par défaut). Une fois la date passée, l'application vous le rappelle à l'ouverture de la clé et propose de la détruire. Détruire la clé ne détruit pas vos documents d'origine : ils restent des données personnelles, à protéger comme avant.

Une même clé réutilisée pour plusieurs conversations permet à l'IA de relier ces conversations entre elles : l'application le signale.

## Vérification finale avant livraison

Après le traitement, l'application relit le fichier produit (commentaires, métadonnées, révisions et liens compris) et cherche toute valeur masquée. Si elle en retrouve une, le fichier n'est pas créé. L'application indique alors les balises et les emplacements concernés, jamais les valeurs.

## Évaluation des trois critères

Avant l'envoi, le bilan applique les trois critères des lignes directrices 02/2026 du Comité européen de la protection des données :

- **Isolement** : peut-on distinguer une personne ? Un nom resté visible, ou un groupe estimé de moins de 5 personnes (réglable de 2 à 20) d'après le lieu, l'âge et le métier.
- **Recoupement** : un identifiant stable (matricule, numéro de dossier, lien, SIRET) permet-il de relier le document à une autre source ?
- **Déduction** : une donnée sensible (santé, opinion, religion, syndicat, condamnation…) est-elle liée à une personne qu'on peut isoler ?

L'évaluation vaut pour le destinataire choisi et pour l'état du document au moment du bilan. Les tailles de groupe sont des estimations tirées des chiffres de population ; elles ne tiennent pas compte de ce que le destinataire sait déjà.

## Ce que l'outil ne garantit pas

- **Une détection complète.** Elle peut manquer des données. La relecture de l'étape Vérifier vous revient.
- **Une anonymisation certaine.** Le verdict du bilan est une évaluation selon une approche simplifiée. La décision et sa justification restent celles du responsable de traitement, et l'évaluation est à refaire si le contexte change : incident de sécurité, nouvelle publication de données, diffusion plus large.
- **La protection de votre ordinateur.** Si une personne accède à votre session ou à votre mot de passe, elle accède aux fichiers. Choisissez un mot de passe solide, ou gardez la phrase de passe proposée.
- **Ce que devient le fichier une fois envoyé à une IA.** Les balises protègent les données remplacées, pas le reste du contenu. Ne partagez que ce que vous avez relu.
- **Une lecture correcte des scans.** L'OCR peut manquer du texte, surtout manuscrit. Le détecteur de visages peut en manquer un de profil ou partiellement caché.
