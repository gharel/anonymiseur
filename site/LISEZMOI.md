# Site de présentation (GitHub Pages)

Page unique, HTML et CSS écrits à la main, sans framework. Elle ne charge aucune ressource extérieure (ni CDN, ni police distante, ni traceur). Servie à `https://gharel.github.io/anonymiseur/`.

Déploiement : `.github/workflows/pages.yml` (push sur `main` touchant `site/**`, publication d'une release, ou lancement manuel). Réglage à faire une fois : Settings, Pages, Source « GitHub Actions ».

## Origine des fichiers

| Fichier | Origine |
|---|---|
| `fonts/georama-*.woff2` | `design-skazy/assets/fonts/` (400, 600, 700, 800) |
| `img/logo-color.png`, `img/logo-white-color.png`, `img/icon-256.png` | `assets/charte/` |
| `img/LICENSE-lucide.txt` | `assets/charte/icons/LICENSE` (ISC) |
| icônes | Lucide 0.460.0 (`assets/charte/icons/*.svg`), intégrées en sprite SVG dans `index.html` |
| `captures/*.png` | `app/tests/snapshots/<nom>_1280x800.png`, réduites à 128 couleurs |
| `empreintes.json` | **généré au déploiement** à partir du `SHA256SUMS` de la dernière release ; absent du dépôt |

## Remplacer les captures

Garder les mêmes noms de fichier. Recompression :

```bash
python -I -c "from PIL import Image; import sys; [Image.open(f).convert('RGB').quantize(128).save(f, optimize=True) for f in sys.argv[1:]]" site/captures/*.png
```

## Essai local

```bash
cd site && python -m http.server
```

Pour voir les empreintes en local, créer à la main un `empreintes.json` (nom de fichier vers SHA-256) et ne pas le commiter.
