# Contribuer

Merci de votre intérêt pour Anonymiseur. Les règles complètes (architecture, invariants, tests, conventions) sont dans [AGENTS.md](AGENTS.md) : lisez-le en entier avant de modifier le code.

## Un changement est terminé quand

- toute fonctionnalité nouvelle a ses tests, et tout bug corrigé a un test qui échouait avant la correction ;
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` et `cargo test --workspace` passent ;
- aucun test existant n'a été affaibli, supprimé ou ignoré pour passer ;
- les seuils de `tests/baseline.json` ne baissent pas ;
- un nouveau format ou un nouveau cas piège a son faux fichier dans `exemples/` et son entrée dans `exemples/attendu.json` ;
- la section « Non publié » de `CHANGELOG.md` est à jour.

## Données réelles interdites

Aucune donnée réelle dans `exemples/`, dans les tests ou dans les instantanés. Les exemples sont fictifs, les emails en `example.com`. Les fichiers d'exemple se régénèrent avec `tools/make_examples.py`.

## Autres règles à ne pas oublier

- Aucune dépendance réseau à l'exécution : la CI échoue sinon.
- Textes de l'interface en français, au vouvoiement, sans emoji.
- Commits conventionnels en français (`feat:`, `fix:`, `test:`, `docs:`, `chore:`).

Le projet est publié sous licence AGPL-3.0 : votre contribution l'est aussi.
