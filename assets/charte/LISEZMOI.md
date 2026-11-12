# Charte Skazy Formation (dossier généré)

Dossier généré par `tools/import_charte.py`, ne pas éditer à la main.
Source : design-skazy (lecture seule, voir invariant 7 de `AGENTS.md`).
Pour le régénérer : `uv run --project tools python tools/import_charte.py`.

## Contenu

- `tokens.json` : couleurs, rayons, espacements (px), ombres, tailles de texte (px).
- `fonts/Georama-400.ttf` à `Georama-800.ttf` : police de l'interface (converties du WOFF2).
- `logo-color.png`, `logo-white.png` : logos sans marge (fond clair, fond foncé).
- `icon.ico`, `icon-256.png`, `icon-1024.png` : icône de l'application.
- `icons/*.svg` : icônes Lucide 0.460.0 (licence ISC dans `icons/LICENSE`).

## Règles d'usage à respecter côté interface

Reprises du `readme.md` de la charte.

### Couleurs et contrastes
- Texte courant : `color-text-primary` (anthracite `#4a4a4a`). Texte secondaire : `color-text-secondary` (`#676f77`, AA).
- La couleur de marque `color-primary` (`#50967c`, 3,50:1 sur blanc) sert aux aplats, au logo, au décor et aux grands titres sur fond sombre. Jamais en texte courant, ni en fond de petit texte blanc.
- Liens, texte vert, fond des boutons primaires : `color-primary-dark` (`#3d7a62`, 5,05:1 avec du blanc). Survol : `color-primary-darker`. Les liens sont soulignés.
- `color-gray-500` est réservé au décor, jamais au texte.
- Texte d'erreur : `color-error`. `color-success`, `color-warning` et `color-info` servent de fonds ou d'icônes, jamais de texte.
- Bord des champs : `color-border-input` ; anneau de focus de 2 px en `color-border-focus`.
- Contraste minimal AA : 4,5:1 pour le texte courant, 3:1 pour les grands textes (24 px et plus, ou 18,66 px et plus en gras) et pour les éléments d'interface (bords de champ, icônes, focus).
- Une couleur de catégorie n'est jamais une couleur de texte : utiliser `-ink`. Petit texte sur une catégorie : couleur de base + `-on` anthracite (Data, Gestion de projet, Graphisme & UX, Marketing, IA, NoCode), ou fond `-ink` + texte blanc (Bureautique, Développement, Web).
- Fond de page : blanc ; fond sombre : `color-dark-3` (`#1a1a1a`).

### Formes
- Boutons en pilule (`radius-full`, 9999 px), toujours arrondis, jamais anguleux. Variantes : primaire (rempli), secondaire (contour), fantôme (texte seul), sombre. Primaire : fond `color-primary-dark` + texte blanc.
- Cartes : `radius-xl` (16 px), ombre `md`, fond blanc, marge intérieure de 20 px. Modales : ombre `xl`.
- Rayons : `radius-sm` 4, `md` 8, `lg` 12, `xl` 16, `2xl` 24, `3xl` 32 px.
- Transitions courtes : 150 ms (micro-interactions), 250 ms (survol). Pas d'animation décorative en boucle.

### Icônes
- Lucide uniquement, version 0.460.0, traits (non remplies). Tailles : 16 px (en ligne), 20 px (boutons), 24 px (seules).
- Aucun emoji, aucun symbole unicode en guise d'icône.

### Textes
- Titres : majuscule initiale uniquement (« Nos formations »).
- Badges : MAJUSCULES avec espacement des lettres (« DÉBUTANT »).
- Boutons : première lettre majuscule (« Voir les formations »).
- Vouvoiement : toujours « vous » dans l'interface.
- Français, ton direct, phrases courtes, verbes d'action.
