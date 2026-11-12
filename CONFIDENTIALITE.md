# Confidentialité

## Tout reste sur votre ordinateur

Vos documents, les données détectées et les résultats ne quittent jamais votre machine. L'application ne se connecte pas à Internet. L'IA de noms est un modèle embarqué dans l'exécutable : elle tourne localement.

**Ce qui le garantit.** Aucune bibliothèque réseau n'est liée à l'application. À chaque modification du code, deux contrôles automatiques le vérifient. L'outil `cargo deny` refuse les dépendances réseau (par exemple `reqwest`, `hyper`, `tokio`). Une tâche dédiée examine en plus l'arbre des dépendances de l'application sur Windows et sur macOS. Les téléchargements (modèles, dictionnaires, polices) n'existent que dans les scripts de préparation du dépôt, jamais dans l'application. Ces contrôles portent sur le code : si vous voulez une preuve de votre côté, un pare-feu qui bloque l'application suffit à la constater.

## Ce qui est écrit sur le disque

- **Le fichier anonymisé**, à l'endroit que vous choisissez.
- **La table de correspondance `.anonmap`**, à l'endroit que vous choisissez. Elle contient les vraies valeurs et leurs balises. Elle est chiffrée avec une clé dérivée de votre mot de passe (Argon2id) et l'algorithme AES-256-GCM. **Le mot de passe n'est pas récupérable** : personne, y compris Skazy Formation, ne peut ouvrir la table sans lui. Un mot de passe oublié signifie une table perdue. Un mauvais mot de passe et un fichier altéré donnent le même message d'erreur.

## Ce qui n'est jamais écrit

- La table de correspondance en clair.
- Un journal contenant du contenu de vos documents. Les messages d'erreur et d'avertissement ne citent que des emplacements (par exemple `word/document.xml`, `page 3`) et des balises.
- Un fichier temporaire contenant du contenu de vos documents. Les images, le texte reconnu par l'OCR et les documents sont traités en mémoire.

**Une exception.** Pour l'OCR, les données de langue (français et anglais) sont extraites dans un dossier temporaire privé le temps du traitement, puis supprimées. Elles sont embarquées dans l'exécutable et ne contiennent rien qui vienne de vous.

## Mode irréversible

Si vous cochez « Mode irréversible : ne pas créer de table de correspondance », aucune table n'est enregistrée. Vous ne pourrez plus remettre les vraies valeurs. Choisissez-le quand vous n'avez pas besoin de revenir au document d'origine.

## Contrôle de fuite avant livraison

Après l'anonymisation, l'application relit le fichier produit (commentaires, métadonnées, révisions et liens compris) et cherche toute valeur que vous avez validée. Si elle en retrouve une, le fichier n'est pas créé. L'application indique alors les balises et les emplacements concernés, jamais les valeurs.

## Rapport de risque

Certaines informations, prises ensemble, peuvent suffire à reconnaître quelqu'un : une date, un âge, une fonction rare, une petite commune, un montant. Le rapport (Faible, Modéré, Élevé) compte celles qui restent visibles dans le fichier produit et propose des conseils. Le profil Renforcé les masque.

## Ce que l'outil ne garantit pas

- **Une détection complète.** Elle peut manquer des données. La relecture de l'écran de validation vous revient.
- **Une anonymisation au sens du RGPD.** Il s'agit d'une pseudonymisation : avec la table et le mot de passe, tout est réversible, et un document peut rester identifiable par recoupement.
- **La protection de votre ordinateur.** Si une personne accède à votre session ou à votre mot de passe, elle accède aux fichiers. Choisissez un mot de passe solide.
- **Ce que devient le fichier une fois envoyé à une IA.** Les balises protègent les données remplacées, pas le reste du contenu. Ne partagez que ce que vous avez relu.
- **Une lecture correcte des scans.** L'OCR peut manquer du texte, surtout manuscrit.
