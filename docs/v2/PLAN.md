# Anonymiseur V2 : plan

Statut : proposé et validé le 15 novembre 2026. Développement en cours.
Maquettes des écrans : [`maquettes.html`](maquettes.html).

## 1. En bref

- **Le constat.** La V1 fait de la pseudonymisation réversible : chaque valeur sensible devient une balise, et vous gardez la clé. Pour vous, le document reste une donnée personnelle. Le rapport de risque de la V1 compte des indices et ne dit pas si une personne reste reconnaissable.
- **La V2 propose deux objectifs nommés sans ambiguïté.** « Masquer pour l'IA » est réversible (pseudonymisation). « Anonymiser définitivement » est irréversible : le document sort du champ du RGPD si l'évaluation le confirme.
- **Une évaluation alignée sur le projet de lignes directrices 02/2026 du CEPD.** L'outil teste les trois critères (isolement, recoupement, déduction), personne par personne, pour un destinataire donné. Il propose une correction en un clic pour chaque échec et produit une fiche d'évaluation à conserver.
- **L'interface tient en une seule boucle.** Préparer, envoyer à l'IA, restaurer la réponse : tout se passe dans un même dossier de travail, sans changer d'onglet ni retaper le mot de passe. La vérification se fait dans le document, sur le texte surligné, au clavier.
- **Des objectifs chiffrés et des tests avec de vrais utilisateurs.** Score SUS (System Usability Scale) d'au moins 85, 95 % des tâches réussies sans aide, et aucun participant qui envoie un document où subsiste un identifiant direct.

## 2. Ce que dit le droit en novembre 2026

| Source | Ce qu'elle établit | Conséquence pour l'outil |
|---|---|---|
| RGPD, art. 4(1), 4(5), considérant 26 | Une donnée pseudonymisée reste personnelle pour qui détient la clé. Une donnée anonyme sort du champ du RGPD. | Deux objectifs distincts, deux libellés distincts. |
| CJUE, C-413/23 P, CEPD c. CRU, 4 septembre 2025 | Des données pseudonymisées peuvent être anonymes pour un destinataire qui n'a pas de moyen raisonnable de réidentifier. Elles restent personnelles pour l'expéditeur. | L'évaluation se fait « pour un destinataire » : l'outil demande à qui part le document. |
| CEPD, lignes directrices 02/2026 sur l'anonymisation, version 1.0 adoptée le 7 juillet 2026 pour consultation (close le 30 octobre 2026) | Trois critères : pas d'isolement d'un enregistrement, pas de recoupement, pas de déduction précise et significative. Deux approches : contextuelle et simplifiée. Un jeu mixte (anonyme pour certains, pas pour d'autres) est entièrement personnel (§ 36). L'anonymisation est un traitement qui demande une base légale (§ 38). Ne pas qualifier d'« anonyme » ce qui reste identifiable (§ 40). Documenter et conserver l'évaluation (§ 41). Réévaluer avec le temps (§ 35). Un contrat seul ne suffit pas (§ 34). Interroger un modèle d'IA est une forme de déduction (§ 82), et les agents d'IA abaissent le coût de la réidentification (§ 83). | Moteur d'évaluation des trois critères, verdict par personne, fiche d'évaluation, date de réévaluation, prise en compte de l'IA comme attaquant. |
| CEPD, lignes directrices 01/2025 sur la pseudonymisation (version soumise à consultation en janvier 2025) | Les informations supplémentaires (la clé) se conservent séparément et protégées. Notion de domaine de pseudonymisation. | La clé ne s'enregistre plus à côté des fichiers produits. Avertissement quand une même clé sert pour plusieurs destinataires. |
| Omnibus numérique (en négociation) | La position du Conseil du 21 octobre 2026 garde la définition actuelle de la donnée personnelle et ajoute un article sur les données pseudonymisées. Le Parlement n'a pas fini ses travaux. | Aucune dépendance de conception. Point à surveiller avant la sortie. |
| Staab et al., ICLR 2024 ; Rocher et al., Nature Communications, 2019 | Un grand modèle de langage déduit le lieu, l'âge ou la profession d'un auteur à partir d'indices du texte. Avec 15 attributs démographiques, 99,98 % des Américains se réidentifient dans n'importe quel jeu de données. | Le destinataire « IA » est traité comme un attaquant capable de déduction. L'outil estime la taille du groupe auquel chaque personne appartient. |
| Pilán et al., Text Anonymization Benchmark, 2022 | Mesures dédiées au texte : rappel sur les identifiants directs et indirects, précision pondérée. | Nouveaux seuils dans `tests/baseline.json`, séparés par type d'identifiant. |

Le texte final des lignes directrices 02/2026 peut encore changer. Le moteur garde donc ses seuils et ses règles dans des constantes nommées et documentées, faciles à ajuster.

## 3. Exigences RGPD de la V2

- **R1. Libellés exacts.** Aucun écran, aucun nom de fichier, aucune fiche ne qualifie un résultat d'« anonyme » sans évaluation réussie des trois critères. Le verdict affiché précise toujours sa portée : « Anonyme pour un assistant IA grand public, évaluation du 15/11/2026, approche simplifiée ».
- **R2. Destinataire explicite.** L'outil demande où part le document (assistant IA grand public, IA de votre organisation sous contrat, partage externe, publication) et retient la réponse, sans aucun contenu.
- **R3. Trois critères évalués**, sur les tableaux comme sur les textes, avec le détail de chaque échec.
- **R4. Verdict par personne.** Un document est anonyme seulement si chaque personne qu'il mentionne passe les trois critères (§ 36).
- **R5. Fiche d'évaluation** exportable et conservée par l'utilisateur, sans aucune donnée personnelle (§ 41).
- **R6. Clé séparée.** La clé de restauration s'enregistre par défaut dans un dossier distinct des fichiers produits. L'outil avertit si vous choisissez le même dossier.
- **R7. Pas de recoupement entre envois** en mode « Anonymiser définitivement » : numérotation des balises propre à chaque envoi, aucun identifiant stable (numéro client, matricule) conservé.
- **R8. Données sensibles repérées** (art. 9 et 10 : santé, opinions, religion, syndicat, orientation sexuelle, origine, données génétiques ou biométriques, condamnations). Liées à une personne isolable, elles font échouer le critère de déduction.
- **R9. L'IA comme attaquant.** Le critère de déduction tient compte d'un destinataire capable d'interroger le texte et de croiser des indices.
- **R10. Aide à la décision, responsabilité inchangée.** L'outil documente une évaluation. Le responsable de traitement décide et répond de sa décision. La fiche le rappelle, avec un champ pour la base légale.

## 4. Moteur : ce qui change

### 4.1 Deux objectifs

| | Masquer pour l'IA | Anonymiser définitivement |
|---|---|---|
| Terme juridique | Pseudonymisation | Anonymisation |
| Retour aux vraies valeurs | Oui, avec la clé | Non, aucune clé créée |
| Indices (dates, lieux, âges, fonctions, montants) | Masqués ou rendus moins précis au choix | Rendus moins précis ou supprimés jusqu'à réussir l'évaluation |
| Évaluation des trois critères | Calculée pour le destinataire, à titre d'information | Requise avant d'écrire le mot « anonyme » |
| Libellé des fichiers produits | `_masque` | `_anonyme` si l'évaluation réussit, sinon `_masque_sans_cle` |
| Usage type | Faire travailler une IA puis restaurer sa réponse | Partager, publier, archiver, entraîner un modèle |

### 4.2 Évaluation des trois critères

Nouveau crate `anon-assess`, entre `anon-detect` et `anon-engine`. Par défaut il applique l'approche simplifiée du CEPD : toute voie de réidentification compte, quel que soit l'attaquant. Elle est plus prudente et le CEPD admet qu'un responsable s'en serve pour démontrer l'anonymat. Trois questions sur le destinataire permettent ensuite l'approche contextuelle : « Le destinataire connaît-il les personnes ? », « Le document sera-t-il publié ? », « D'autres documents du même dossier lui seront-ils envoyés ? ». Chaque réponse qui écarte un risque est consignée dans la fiche avec sa justification.

**Tableaux (Excel, Calc, CSV).** Un enregistrement est une ligne.

- Les colonnes sont classées en identifiants directs, indices, attributs sensibles et autres, à partir de l'en-tête (logique de colonnes existante) et des valeurs.
- **Isolement** : k-anonymat sur les colonnes d'indices. Le critère échoue si un groupe de lignes identiques compte moins de `K_MIN` lignes (5 par défaut, réglable de 2 à 20).
- **Recoupement** : échec si une colonne garde un identifiant stable (numéro client, matricule, email haché) ou si une combinaison d'indices reste unique.
- **Déduction** : l-diversité sur les attributs sensibles. Le critère échoue si tous les membres d'un groupe partagent la même valeur sensible (cas de l'exemple 12 des lignes directrices : tous les hommes nés en 1946 dans le 54 ont la sclérose).
- **Correction** : généralisation par hiérarchies (date, mois, année, décennie ; âge, tranche de 5 puis de 10 ans ; commune, département ou province, région ; code postal à 2 chiffres ; montant en tranche), choisie par un algorithme glouton de type Datafly : l'outil élargit d'abord la colonne la plus variée, jusqu'à atteindre `K_MIN`. Les lignes restantes hors norme sont supprimées dans la limite de `MAX_SUPPRESSION_RATE` (5 % par défaut), sinon l'outil demande votre arbitrage.

**Textes (Word, PDF, PowerPoint, OpenDocument, texte).** Un enregistrement est une personne mentionnée.

- **Profil d'indices par personne** : l'outil regroupe les mentions d'une même personne (regroupement des variantes de la V1), puis rattache les indices trouvés dans la même phrase ou le même paragraphe : fonction, employeur, lieu, âge, date d'un événement, donnée sensible.
- **Isolement** : estimation de la taille du groupe auquel la personne appartient, à partir de données de population embarquées (populations légales de l'INSEE pour la France, recensement de l'ISEE pour la Nouvelle-Calédonie, répartition par âge et par catégorie professionnelle). Le critère échoue sous `K_MIN`. Deux exemples : une fonction rattachée à une organisation nommée (« responsable technique de Nautilus Pacifique SARL ») désigne une seule personne ; une profession, une commune de moins de 1 000 habitants et un âge exact (« infirmière », « Farino », « 62 ans ») désignent un groupe estimé à moins de 5 personnes.
- **Recoupement** : échec si un identifiant stable subsiste (numéro de dossier, de contrat, de parcelle, pseudonyme en ligne, lien vers un profil).
- **Déduction** : échec si une donnée sensible ou un événement marquant (licenciement, arrêt maladie, procédure) reste rattaché à une personne isolable.
- **Ce que l'IA pourrait deviner** : liste, par personne, des indices restants qu'un modèle de langage peut croiser.

Ces estimations donnent un ordre de grandeur. Elles sont testées sur des cas fictifs annotés, et l'outil vous laisse confirmer ou corriger chaque verdict.

**Images et pages scannées.** Le texte lu par OCR suit la même évaluation. La V2 ajoute la détection des visages (petit modèle ONNX de type YuNet, moins de 400 Ko, licence à vérifier, édition complète). L'édition Lite indique « Visages non vérifiés » et ne peut pas conclure à l'anonymat d'une image qui contient une photo.

### 4.3 Corrections proposées

Chaque échec arrive avec sa correction, appliquée en un clic et annulable (Ctrl+Z) :

| Échec | Correction proposée |
|---|---|
| Personne isolable par sa fonction dans une organisation nommée | Masquer l'organisation (`[SOCIETE_1]`), ou élargir la fonction (« responsable technique » devient « salarié ») |
| Personne isolable par profession, commune et âge | « infirmière » devient « professionnelle de santé », « Farino » devient « province Sud », « 62 ans » devient « 60-69 ans » |
| Donnée sensible liée à une personne | Masquer la mention (`[SANTE_1]`) ou la phrase |
| Identifiant stable | Remplacer par un numéro aléatoire propre à cet envoi |
| Groupe trop petit dans un tableau | Généraliser la colonne proposée, ou supprimer n lignes |
| Visage | Flouter ou masquer la zone |

Le moteur recalcule l'évaluation après chaque correction, en moins d'une seconde sur les exemples.

### 4.4 Données sensibles

Nouveaux types d'entités (le préfixe des types existants reste figé) : `SANTE`, `OPINION`, `RELIGION`, `SYNDICAT`, `ORIENTATION`, `ORIGINE`, `GENETIQUE`, `BIOMETRIE`, `JUDICIAIRE`. Détection par lexiques et règles de contexte en français et en anglais, enrichie par l'IA de noms quand elle est présente. Ces valeurs sont cochées d'office dans les deux objectifs.

### 4.5 Fiche d'évaluation

Fichier Markdown et PDF, enregistré là où vous le choisissez. Il ne contient que des balises, des catégories et des nombres :

- date, version de l'outil, objectif, destinataire, approche (simplifiée ou contextuelle) ;
- documents traités (nombre et type ; le nom des fichiers seulement si vous le cochez) ;
- techniques appliquées : masquage, généralisation (avec les niveaux), suppression de lignes ;
- résultat de chaque critère, personne par personne, sous forme de balises ;
- risques résiduels acceptés et votre justification ;
- base légale et finalité (champs à remplir par vous) ;
- date de réévaluation conseillée (12 mois par défaut) et rappel des événements qui imposent de réévaluer plus tôt : incident de sécurité, nouvelle publication de données, diffusion plus large.

### 4.6 Clé de restauration

- La « table de correspondance » devient la **clé de restauration** (fichier `.anonmap`, chiffrement inchangé : Argon2id, AES-256-GCM).
- Emplacement par défaut : `Documents\Anonymiseur\Clés`, distinct du dossier des fichiers produits.
- Option « Retenir le mot de passe sur cet ordinateur » dans le trousseau du système (Gestionnaire d'identification de Windows, Trousseau de macOS) : la restauration ne demande plus de mot de passe sur ce poste. Dépendance à justifier : crate `keyring`, sans réseau, licence MIT ou Apache-2.0.
- Durée de conservation choisie à la création, avec rappel, et action « Détruire la clé » (l'outil explique que vos originaux restent, eux, des données personnelles).
- Une clé réutilisée pour plusieurs conversations permet à l'IA de relier ces conversations : l'écran le signale.
- `FORMAT_VERSION` passe à 2 (objectif, destinataire, date de réévaluation), avec un test de relecture des clés de la V1.

### 4.7 Ce que la V2 ne promet pas

- Une garantie d'anonymat pour un texte libre : l'outil documente une évaluation prudente, la décision vous revient.
- La détection d'indices de style (tournures, fautes récurrentes) qui trahissent un auteur.
- Le recoupement avec des données externes que l'outil ne connaît pas : la fiche le mentionne comme risque résiduel.

## 5. UX : audit de la V1

Mené sur les 64 captures de référence (`app/tests/snapshots`) et le code de l'interface, avec les heuristiques de Nielsen.

| # | Constat | Gravité | Correction V2 |
|---|---|---|---|
| 1 | La boucle de travail (anonymiser, envoyer, restaurer) est coupée en trois onglets. Pour restaurer, il faut rouvrir la table et retaper le mot de passe. | Majeure | Un dossier de travail unique qui couvre toute la boucle |
| 2 | Le premier écran empile trois intentions : bandeau d'accueil, zone de dépôt, formulaire « Toujours anonymiser ». | Moyenne | Une seule action principale. Les listes passent dans les réglages et dans la sélection de texte. |
| 3 | À l'étape Vérifier, deux bandeaux et le niveau de protection repoussent la première valeur sous la ligne de flottaison (en 1280×800, l'en-tête du tableau est à 735 px). | Majeure | La première décision est visible sans défiler |
| 4 | La colonne Contexte coupe la phrase à une trentaine de caractères. Vous décidez sans voir le document. | Majeure | Vérification dans le document, texte surligné |
| 5 | Vocabulaire technique : Casse, Occ., Balise, Table de correspondance, Généralisation, catégorie « Contexte ». | Moyenne | Glossaire de la section 7.4 |
| 6 | Le niveau de protection se choisit après l'analyse. Le profil Renforcé ajoute 14 valeurs à revoir sans dire pourquoi elles comptent. | Moyenne | Objectif et destinataire choisis une fois. Les indices apparaissent avec leur effet sur le verdict. |
| 7 | La clé s'enregistre à côté des fichiers produits, contrairement à la recommandation du CEPD de conserver la clé séparément. | Majeure (RGPD) | Dossier de clés distinct, trousseau du système |
| 8 | Le rapport final affiche « Risque élevé » sans proposer de correction. Les mêmes trois conseils se répètent sous chaque fichier. Le conseil « utilisez le profil Renforcé » s'affiche alors que ce profil est déjà actif. | Majeure | Bilan avant export, chaque échec avec sa correction |
| 9 | Le risque compte des occurrences d'indices (`RISK_HIGH_TOTAL = 10`) : un long document est « élevé » par sa seule longueur. | Majeure | Évaluation des trois critères, personne par personne |
| 10 | Texte libre : deux zones de texte brut, sans surlignage. Impossible de voir ce qui a été détecté ou de corriger sur place. | Moyenne | Même vue de vérification que pour les documents |
| 11 | Désanonymiser : le résultat commence en bas de l'écran (vers 720 px sur 800). | Mineure | Résultat côte à côte avec la réponse collée |
| 12 | Confirmation du mot de passe à saisir deux fois, pour chaque dossier. | Mineure | Phrase de passe générée à copier, ou trousseau du système |

**À garder** : le pied de page « Tout reste sur votre ordinateur », le bouton « Essayer avec des exemples », les raccourcis clavier et le panneau F1, le mode sombre, les contrastes AA vérifiés par test, l'avancement détaillé par fichier, les notifications, l'action désactivée qui dit pourquoi.

## 6. UX V2 : principes

Repères : les meilleurs outils de leur catégorie. Grammarly pour la correction dans le texte, Linear et Superhuman pour la vitesse au clavier et la palette de commandes, 1Password Watchtower pour un bilan de sécurité qui propose chaque correction, Things pour le calme visuel.

1. **Une seule boucle visible.** Préparer, envoyer, restaurer, dans un même dossier de travail.
2. **Vérifier dans le document.** Le texte surligné remplace le tableau comme vue principale. Le tableau reste disponible pour le tri et les traitements en masse.
3. **Montrer seulement ce qui demande une décision.** Les valeurs sûres sont masquées d'office et repliées. Vous traitez la file « À décider ».
4. **Chaque alerte arrive avec sa correction**, appliquée en un clic et annulable.
5. **Le clavier d'abord.** Tout se fait sans souris : file de décisions, palette Ctrl+K, raccourcis affichés dans les infobulles.
6. **Annuler plutôt que confirmer.** Historique d'annulation sur toutes les décisions. Une confirmation seulement pour l'irréversible.
7. **Dire juste.** Les mots « anonyme », « protégé » et « sûr » n'apparaissent que si l'évaluation les justifie.
8. **Un seul message à la fois.** Plus de bandeaux empilés. L'aide apparaît à l'endroit et au moment où elle sert.
9. **Rapidité perçue.** Les surlignages du premier fichier s'affichent pendant l'analyse des suivants.
10. **Accessible à tous.** WCAG 2.2 AA, lecteurs d'écran, zoom du texte (Ctrl+plus), réduction des animations.

## 7. UX V2 : parcours et écrans

### 7.1 Architecture

```
Accueil ── déposer des fichiers ou coller un texte ──► Dossier de travail
                                                       ├─ Vérifier   (dans le document, file « À décider »)
                                                       ├─ Bilan      (trois critères, corrections)
                                                       ├─ Envoyer    (fichiers, texte + consigne)
                                                       └─ Réponse    (coller, restaurer, enregistrer)
Accueil ── ouvrir une clé (.anonmap) ────────────────► Dossier de travail, à l'étape Réponse
```

Le dossier de travail remplace les trois onglets. Le texte collé devient un document du dossier comme un autre. La clé reste ouverte en mémoire pendant toute la session : la restauration ne redemande rien.

### 7.2 Écrans

**A. Accueil.** Une action principale, deux objectifs, aucun formulaire.

```
┌──────────────────────────────────────────────────────────────────────────┐
│ skazy                                            Ctrl+K Commandes    ☾   │
├──────────────────────────────────────────────────────────────────────────┤
│  Préparer des documents pour une IA                                       │
│  Tout reste sur votre ordinateur.                                         │
│                                                                           │
│  ┌─────────────────────────────────────────────────────────────────────┐ │
│  │        Déposez des fichiers, ou collez un texte (Ctrl+V)            │ │
│  │   Word, Excel, PowerPoint, PDF, OpenDocument, images, texte          │ │
│  └─────────────────────────────────────────────────────────────────────┘ │
│                                                                           │
│  Objectif   (●) Masquer pour l'IA       ( ) Anonymiser définitivement     │
│                 Vous récupérez la réponse    Aucun retour possible        │
│                 avec les vraies valeurs      Sortie du RGPD si validée    │
│                                                                           │
│  Restaurer une réponse avec une clé     Essayer avec des exemples         │
└──────────────────────────────────────────────────────────────────────────┘
```

**B. Vérifier dans le document.** Trois colonnes : documents du dossier, texte surligné, file de décisions.

```
┌ Dossier · 4 documents ─────────────────── Pour : assistant IA grand public ▾ ┐
│ DOCUMENTS        │ contrat-prestation.docx              │ À DÉCIDER  2 / 5    │
│ ● contrat     2  │                                      │                     │
│ ○ clients     3  │ Entre ▓Nautilus Pacifique SARL▓,     │ « gérant »          │
│ ○ notes       0  │ RIDET ▓1 234 567.001▓, dont le siège │ Fonction · 2 fois   │
│ ○ courrier    0  │ est situé ▓14 rue des Frangipaniers▓,│ …représentée par    │
│                  │ représentée par ▓Jean Dupont▓,       │ Jean Dupont, ░gérant│
│ CATÉGORIES       │ ░gérant░, ci-après « le Client »…    │ ░, ci-après…        │
│ Personnes    23  │                                      │                     │
│ Organisations 3  │ ▓ masqué   ░ à décider               │ [M] Masquer         │
│ Lieux        13  │ Sélectionnez du texte pour le        │ [G] Garder          │
│ Sensibles     1  │ masquer partout.                     │ [J] Suivante        │
│                  │                                      │ ─────────────────── │
│                  │                                      │ Masquées · 90  ▸    │
├──────────────────┴──────────────────────────────────────┴─────────────────────┤
│ Bilan : 1 personne reste reconnaissable                     Continuer  Ctrl+↵ │
└───────────────────────────────────────────────────────────────────────────────┘
```

- Survol d'un surlignage : la balise qui le remplacera (`[PERSONNE_1]`) et le nombre d'occurrences.
- Clic : bascule masqué ou gardé, dans tout le dossier.
- Sélection de texte : « Masquer partout » ou « Toujours masquer » (remplace le formulaire « Toujours anonymiser »).
- PDF et images : la page rendue, avec les zones surlignées et le tracé de zones de la V1.
- Le tableau de la V1 reste accessible par un bouton « Vue tableau » (tri, filtres, traitements en masse).

**C. Bilan.** Le verdict d'abord, puis les échecs avec leur correction.

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Bilan avant envoi                      Pour : assistant IA grand public ▾ │
│                                                                           │
│  ▌ 1 personne reste reconnaissable. Corrigez-la pour un envoi anonyme.     │
│                                                                           │
│   Isolement     22 personnes protégées · 1 isolable                       │
│   Recoupement   Aucun identifiant stable                                  │
│   Déduction     Aucune donnée sensible liée à une personne isolable       │
│                                                                           │
│   [PERSONNE_2]  responsable technique · Nautilus Pacifique SARL · 62 ans   │
│                 groupe estimé : 1 personne                                 │
│                 Correction : masquer la société     [ Appliquer ]  Autre ▾ │
│                                                                           │
│   [ Tout corriger ]        Ce que l'IA pourrait deviner ▸                  │
│                                                                           │
│   Fiche d'évaluation : enregistrée avec le dossier      [ Ouvrir ]         │
└──────────────────────────────────────────────────────────────────────────┘
```

**D. Envoyer.** Deux façons d'envoyer, la consigne incluse.

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Prêt à envoyer · vérification finale réussie : aucune valeur masquée     │
│  ne subsiste dans les 4 fichiers.                                          │
│                                                                           │
│  [ Ouvrir le dossier des fichiers ]   [ Copier le texte et la consigne ]   │
│                                                                           │
│  Quand l'IA a répondu : copiez sa réponse, puis revenez ici (Ctrl+Maj+V).  │
│  Clé de restauration : Documents\Anonymiseur\Clés\dossier-15-11.anonmap   │
└──────────────────────────────────────────────────────────────────────────┘
```

**E. Réponse.** Réponse collée et réponse restaurée côte à côte.

```
┌───────────────────────────────────┬──────────────────────────────────────┐
│ Réponse de l'IA         [Coller]  │ Réponse restaurée     [Copier] [.docx]│
│                                   │                                      │
│ Le contrat lie [SOCIETE_1] et     │ Le contrat lie ▓Nautilus Pacifique   │
│ [SOCIETE_2]. [PERSONNE_1]         │ SARL▓ et ▓Atelier Bougainville SAS▓. │
│ représente…                       │ ▓Jean Dupont▓ représente…            │
│                                   │                                      │
│                                   │ 1 balise inconnue : [PERSONNE_9]      │
└───────────────────────────────────┴──────────────────────────────────────┘
```

**F. Presse-papiers express (après le cœur de la V2).** Deux raccourcis globaux, actifs tant que l'application tourne : Ctrl+Alt+M masque le texte copié avec la clé du dossier ouvert, Ctrl+Alt+R restaure le texte copié. Une notification indique le nombre de valeurs traitées. S'il reste des valeurs à décider, une petite fenêtre de vérification s'ouvre avant de remplacer le presse-papiers. Sous macOS, l'outil guide l'autorisation d'accessibilité.

### 7.3 Clavier et palette de commandes

| Touche | Action |
|---|---|
| Ctrl+K | Palette : toutes les actions, documents et réglages, par recherche |
| J / K | Décision suivante, précédente |
| M / G | Masquer, garder |
| Ctrl+Z, Ctrl+Maj+Z | Annuler, rétablir |
| / | Rechercher dans le document |
| Ctrl+Entrée | Étape suivante |
| Ctrl+Maj+V | Coller la réponse de l'IA et la restaurer |
| F1 | Liste des raccourcis (conservée) |

### 7.4 Vocabulaire

| V1 | V2 |
|---|---|
| Anonymiser (mode réversible) | Masquer pour l'IA |
| Irréversible | Anonymiser définitivement |
| Table de correspondance | Clé de restauration |
| Contrôle de fuite | Vérification finale |
| Quasi-identifiants, « Contexte » | Indices |
| Généralisation | Rendre moins précis |
| Casse | Respecter les majuscules (dans le détail d'une valeur) |
| Occ. | « 3 fois » |
| Risque élevé | « 1 personne reste reconnaissable » |

Règles inchangées : français, vouvoiement, aucun emoji, titres en majuscule initiale.

### 7.5 Système visuel

- Charte Skazy inchangée (couleurs, Georama, formes). La V2 ajoute une couleur sémantique « à décider » (jaune de la catégorie data) et une couleur « masqué » neutre, vérifiées par le test de contraste AA dans les deux thèmes.
- Densité réglable : confortable par défaut, compacte pour les longs dossiers.
- Mouvement : transitions de 150 à 200 ms, désactivées si le système demande moins d'animations.
- Mise en page en trois colonnes à partir de 1280 px, deux colonnes en dessous (la file de décisions passe en panneau), une colonne sous 900 px.

### 7.6 Accessibilité

- WCAG 2.2 AA, dont la taille des cibles (24 px minimum) et le focus toujours visible.
- Surlignages annoncés par les lecteurs d'écran (« Jean Dupont, personne, masqué, 10 fois »).
- Parcours complet testé au clavier seul, avec NVDA sous Windows et VoiceOver sous macOS.
- Zoom du texte de 80 % à 200 % sans perte de fonction.

## 8. Objectifs mesurables et tests utilisateurs

| Mesure | Cible V2 | Comment |
|---|---|---|
| Score SUS | 85 ou plus (la moyenne de l'industrie est 68) | Questionnaire après chaque séance de test |
| Tâches réussies sans aide | 95 % | 5 tâches du protocole ci-dessous |
| Temps jusqu'au premier fichier prêt à envoyer, nouvel utilisateur, avec les exemples | 2 minutes au plus | Chronométré en séance |
| Boucle complète au clavier, utilisateur habitué (texte copié, masqué, réponse restaurée) | 15 secondes au plus | Chronométré |
| Envois avec un identifiant direct visible | 0 participant | Contrôle des fichiers produits en séance |
| Décisions demandées par document d'exemple | 10 au plus (95 valeurs listées en V1) | Mesuré sur `exemples/` |
| Affichage des premiers surlignages après dépôt (docx de 10 pages, édition Lite) | moins d'une seconde | Test de performance automatisé |

**Protocole.** Aucune télémétrie (invariant 1) : les mesures viennent de séances observées.

1. Séance 0, sur la V1 : 5 participants, pour mesurer le point de départ.
2. Séances 1 et 2, sur les maquettes puis sur un prototype : 5 participants chacune, à voix haute.
3. Séance 3, sur la bêta : 6 participants, mesures complètes.

Participants : apprenants de Skazy Formation sans profil technique, un délégué à la protection des données ou un juriste, une personne qui utilise un lecteur d'écran. Tâches, toutes sur les exemples fictifs : préparer le contrat pour une IA ; corriger une personne reconnaissable ; restaurer une réponse ; anonymiser définitivement le fichier clients pour un partage ; retrouver une clé d'un dossier précédent.

## 9. Lots de travail

Rôles inchangés : le modèle principal fixe les contrats, relit chaque diff et lance les tests ; les sous-agents écrivent le code (Sonnet pour les tâches bien spécifiées, Opus pour les tâches délicates). Chaque lot respecte la définition de « terminé » d'`AGENTS.md` et ajoute ses faux fichiers à `exemples/` et à `exemples/attendu.json`.

| Lot | Contenu | Crates | Agent | Taille |
|---|---|---|---|---|
| 0 | Séance de test V1 ; prototype jetable de la vue document dans egui (surlignage d'un docx de 200 pages, image à moins de 16 ms) | `spikes/` | Opus | S |
| 1 | Contrats : `Goal`, `Recipient`, `Criterion`, `Verdict`, `Assessment`, `Fix` ; nouveaux types sensibles ; `FORMAT_VERSION` 2 et relecture de la V1 ; nouvel invariant dans `AGENTS.md` | `anon-core`, `anon-vault` | principal | S |
| 2 | Données de population (INSEE, ISEE) et hiérarchies de généralisation, préparées par un script de `tools/` | `anon-dict`, `tools/` | Sonnet | M |
| 3 | Évaluation des tableaux : k-anonymat, l-diversité, généralisation de type Datafly, suppression bornée | `anon-assess` | Sonnet | M |
| 4 | Évaluation des textes : profils d'indices par personne, estimation de groupe, déduction, données sensibles | `anon-assess`, `anon-detect` | Opus | L |
| 5 | Fiche d'évaluation (Markdown, PDF) | `anon-engine`, `anon-pdf` | Sonnet | S |
| 6 | Clé de restauration : dossier séparé, trousseau du système, durée de conservation, balises propres à chaque envoi | `anon-vault`, `anon-replace`, `app` | Sonnet | M |
| 7 | Interface : dossier de travail, vue document, file de décisions, palette, historique d'annulation | `app` | Opus | L |
| 8 | Interface : bilan et corrections, envoi, réponse côte à côte | `app` | Opus | M |
| 9 | Visages dans les images (édition complète) | `anon-pdf`, `anon-ner` | Opus | M |
| 10 | Presse-papiers express | `app` | Sonnet | M |
| 11 | Séances de test 2 et 3, finitions, `README.md`, `CONFIDENTIALITE.md`, site, version 2.0.0 | tous | principal | M |

Ordre : 0 et 1 d'abord ; 2, 3, 5 et 6 en parallèle (fichiers disjoints) ; 4 après 2 ; 7 en parallèle du moteur, branché sur des évaluations factices ; 8 après 4 et 7 ; 9 et 10 ensuite.

**Nouveaux tests.**

- Unitaires : k-anonymat et l-diversité sur des tableaux construits à la main, dont l'exemple 12 des lignes directrices ; estimation de groupe sur des profils fictifs.
- Propriétés (`proptest`) : après correction, le k minimal atteint `K_MIN` ; une correction annulée redonne l'état précédent.
- Intégration : nouvelle section `evaluation` dans `exemples/attendu.json` (verdict attendu par personne et par critère).
- Instantanés : fiche d'évaluation et écrans de la V2, en clair et en sombre.
- Seuils : rappel séparé sur identifiants directs et indices dans `tests/baseline.json`, aucune baisse admise.
- Interface : tout le parcours au clavier, et pour chaque écran les tests de hauteur des contrôles et de contraste.

**Nouveaux faux fichiers d'exemple.** Un tableau de patients où un groupe partage le même diagnostic ; un compte rendu où une personne reste reconnaissable par sa fonction et sa commune ; un courrier qui mentionne un arrêt maladie ; une image avec un visage de synthèse (à fournir sous une licence qui permet la diffusion).

## 10. Risques

| Risque | Parade |
|---|---|
| Le texte final des lignes directrices 02/2026 change | Seuils et règles dans des constantes nommées ; revue du texte final avant la version 2.0.0 |
| L'estimation de groupe se trompe sur un cas réel | Verdict présenté comme une estimation, confirmable ; fiche qui consigne l'approche ; tests sur des cas annotés |
| egui peine à afficher un long document surligné | Prototype du lot 0 avant tout développement de l'interface ; rendu par page visible |
| Raccourcis globaux bloqués sous macOS sans autorisation | Lot 10 indépendant, livrable plus tard |
| Taille de l'exécutable | Données de population compressées (`fst`), modèle de visages de 300 Ko ; mesure à chaque lot |
| Licence du modèle de visages ou des données ISEE incompatible | Vérification avant le lot 9 et le lot 2 ; repli : zones à tracer à la main |

## 11. Décisions

Prises à la validation du plan, le 15 novembre 2026 :

1. Libellés retenus : « Masquer pour l'IA » et « Anonymiser définitivement ».
2. `K_MIN` vaut 5 par défaut, réglable de 2 à 20.
3. Données de population : France et Nouvelle-Calédonie.
4. Le presse-papiers express entre dans la version 2.0.0 (lot 10).
5. Les noms fictifs à la place des balises sont reportés après la version 2.0.0.

Les séances de test avec des utilisateurs (lots 0 et 11) restent à organiser par l'équipe Skazy : l'outil ne collecte aucune mesure.

## Sources

- RGPD, règlement (UE) 2016/679, art. 4, 9, 10 et considérant 26.
- CJUE, 4 septembre 2025, C-413/23 P, CEPD c. CRU, ECLI:EU:C:2025:645 : [EUR-Lex](https://eur-lex.europa.eu/eli/C/2025/5551/oj/eng).
- CEPD, lignes directrices 02/2026 sur l'anonymisation, version 1.0 du 7 juillet 2026 : [consultation publique](https://www.edpb.europa.eu/public-consultations/guidelines-022026-on-anonymisation_en), [texte](https://www.edpb.europa.eu/system/files/2026-07/edpb_guidelines_202602_anonymisation_v1_en_0.pdf).
- CEPD, lignes directrices 01/2025 sur la pseudonymisation : [annonce](https://edpb.europa.eu/news/news/2025/edpb-adopts-pseudonymisation-guidelines-and-paves-way-improve-cooperation_en).
- Omnibus numérique, position du Conseil du 21 octobre 2026 : [synthèse](https://www.technology.org/2026/11/10/gdpr-changes-digital-omnibus-council-position/) (source secondaire).
- Analyses des lignes directrices 02/2026 : [Timelex](https://www.timelex.eu/en/blog/when-data-really-anonymous-edpbs-2026-anonymisation-guidelines-practice), [DLA Piper](https://privacymatters.dlapiper.com/2026/08/eu-edpb-publishes-draft-guidelines-on-anonymisation/).
- R. Staab, M. Vero, M. Balunović, M. Vechev, « Beyond Memorization: Violating Privacy via Inference with Large Language Models », ICLR 2024.
- L. Rocher, J. M. Hendrickx, Y.-A. de Montjoye, « Estimating the success of re-identifications in incomplete datasets using generative models », Nature Communications 10, 3069, 2019.
- I. Pilán et al., « The Text Anonymization Benchmark (TAB) », Computational Linguistics 48(4), 2022.
- L. Sweeney, « k-anonymity: a model for protecting privacy », 2002 ; A. Machanavajjhala et al., « l-diversity », 2007.
