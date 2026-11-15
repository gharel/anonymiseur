# Sources des lexiques de `anon-detect`

## sensibles.txt

Lexiques des catégories particulières de données (RGPD, art. 9 et 10) et des
événements personnels, en français et en anglais. Écrits à la main pour ce projet
(contenu du projet, licence AGPL-3.0). Ce sont des listes de termes courants
(maladies, infractions, partis, syndicats, religions…), des faits qui ne relèvent
d'aucun droit d'auteur ; aucune base de données n'a été recopiée. Les références
ci-dessous ont servi à choisir les catégories et à vérifier les noms.

- Catégories : règlement (UE) 2016/679 (RGPD), art. 9 et 10 ; CNIL, « Données
  sensibles », https://www.cnil.fr/fr/definition/donnee-sensible ; CNIL,
  « Qu'est-ce qu'une donnée de santé ? »,
  https://www.cnil.fr/fr/quest-ce-ce-quune-donnee-de-sante.
- Santé : noms usuels des maladies et traitements, contrôlés sur la Classification
  internationale des maladies de l'OMS (CIM-10, https://icd.who.int/browse10/2019/en)
  sans reprise de ses libellés ; maladies d'importance en Nouvelle-Calédonie
  (leptospirose, dengue, rhumatisme articulaire aigu) d'après la DASS de
  Nouvelle-Calédonie (https://dass.gouv.nc).
- Infractions : intitulés courants du code pénal français (Légifrance,
  https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006070719).
- Partis politiques : noms publics des partis français (Commission nationale des
  comptes de campagne et des financements politiques, https://www.cnccfp.fr) et
  calédoniens (Congrès de la Nouvelle-Calédonie, https://www.congres.nc) ; partis
  britanniques et américains les plus connus.
- Syndicats : organisations représentatives au niveau national (ministère du
  Travail, mesure de l'audience syndicale, https://travail-emploi.gouv.fr) et
  centrales calédoniennes (USTKE, USOENC, CSTNC, COGETRA, CSTC-FO, FSAO,
  UT CFE-CGC) ; grands syndicats britanniques et américains.
- Origine : communautés d'appartenance du recensement de la Nouvelle-Calédonie
  (ISEE, https://www.isee.nc) et adjectifs usuels d'origine géographique.
- Religions, orientation sexuelle, données génétiques et biométriques,
  événements personnels : vocabulaire courant.

Les règles de contexte qui exploitent ces lexiques sont dans
`src/sensitive.rs` : un terme n'est détecté que rattaché à une personne
(« atteinte d'un », « condamné pour », « adhérente de », possessif, verbe d'état).
