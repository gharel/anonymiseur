//! Thème Skazy Formation : couleurs, rayons, espacements et tailles de texte lus
//! dans `assets/charte/tokens.json` (copie générée de la charte, voir `AGENTS.md`).
//!
//! Deux palettes, claire et sombre, exposent les mêmes rôles (surface, texte,
//! accent…) : les composants ne connaissent que ces rôles et appellent
//! [`palette`]. La palette sombre est dérivée des jetons de la charte (fond
//! `color-dark-3`, vert de marque éclairci pour rester lisible).
//!
//! Le thème actif est lu sur le contexte egui au début de chaque image
//! ([`sync`]) et gardé dans une variable propre au fil d'exécution : deux
//! fenêtres de test rendues en parallèle sur deux fils ne se gênent pas.
//!
//! Le fichier est inclus à la compilation. Une clé absente ne fait pas paniquer
//! l'application : elle donne une couleur de repli voyante, et un test vérifie
//! qu'aucune clé n'y tombe.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle, Theme, Visuals,
};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

const TOKENS_JSON: &str = include_str!("../../../assets/charte/tokens.json");

/// Couleur de repli quand une clé manque : magenta, impossible à rater.
const MISSING: Color32 = Color32::from_rgb(255, 0, 255);

/// Grille d'espacement de la charte : multiples de 4 px.
pub const GRID: f32 = 4.0;

/// Hauteurs des contrôles posés sur une même ligne (champ, sélecteur, bouton,
/// sélecteur segmenté) : petite, moyenne (par défaut), grande.
pub const CONTROL_SM: f32 = 32.0;
pub const CONTROL_MD: f32 = 40.0;
pub const CONTROL_LG: f32 = 48.0;

/// Rayon des champs et des sélecteurs (8 px).
pub const FIELD_RADIUS: f32 = 8.0;
/// Marge horizontale intérieure des champs.
pub const FIELD_PAD_X: f32 = 14.0;
/// Épaisseur du bord des champs (1,5 px de la charte).
pub const FIELD_BORDER: f32 = 1.5;

/// Taille du texte des zones multilignes (texte collé, résultat).
pub const TEXT_AREA_SIZE: f32 = 15.0;
/// Interligne ajouté aux zones multilignes, en px.
pub const TEXT_AREA_LEADING: f32 = 7.0;

/// Choix du thème par l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    /// Suit le réglage du système.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Système",
            ThemeChoice::Light => "Clair",
            ThemeChoice::Dark => "Sombre",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Graisses de Georama embarquées.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
    Extrabold,
}

impl Weight {
    fn family_name(self) -> &'static str {
        match self {
            Weight::Regular => "georama-400",
            Weight::Medium => "georama-500",
            Weight::Semibold => "georama-600",
            Weight::Bold => "georama-700",
            Weight::Extrabold => "georama-800",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Weight::Regular => include_bytes!("../../../assets/charte/fonts/Georama-400.ttf"),
            Weight::Medium => include_bytes!("../../../assets/charte/fonts/Georama-500.ttf"),
            Weight::Semibold => include_bytes!("../../../assets/charte/fonts/Georama-600.ttf"),
            Weight::Bold => include_bytes!("../../../assets/charte/fonts/Georama-700.ttf"),
            Weight::Extrabold => include_bytes!("../../../assets/charte/fonts/Georama-800.ttf"),
        }
    }

    const ALL: [Weight; 5] = [
        Weight::Regular,
        Weight::Medium,
        Weight::Semibold,
        Weight::Bold,
        Weight::Extrabold,
    ];
}

/// Police Georama d'une graisse et d'une taille données.
pub fn font(weight: Weight, size: f32) -> FontId {
    let family = match weight {
        Weight::Regular => FontFamily::Proportional,
        other => FontFamily::Name(other.family_name().into()),
    };
    FontId::new(size, family)
}

#[derive(Debug, Deserialize)]
struct RawShadow {
    x: f32,
    y: f32,
    flou: f32,
    rgba: [f32; 4],
}

#[derive(Debug, Deserialize)]
struct RawTokens {
    couleurs: BTreeMap<String, String>,
    rayons: BTreeMap<String, f32>,
    espacements: BTreeMap<String, f32>,
    ombres: BTreeMap<String, Vec<RawShadow>>,
    texte: BTreeMap<String, f32>,
}

/// Erreur de lecture des jetons de la charte.
#[derive(Debug)]
pub enum TokenError {
    Json(serde_json::Error),
    BadColor(String),
}

/// Paire fond / texte pour un badge teinté.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tinted {
    pub fill: Color32,
    pub ink: Color32,
    /// Couleur de base : contour décoratif.
    pub edge: Color32,
}

/// Rôles de couleur de l'interface, et dimensions de la charte.
#[derive(Debug, Clone)]
pub struct Palette {
    pub dark_mode: bool,
    // --- Marque ---
    /// Vert de marque : aplats décoratifs (barre d'accent, jauge forte).
    pub primary: Color32,
    /// Fond des boutons primaires, des cases cochées, de l'étape courante.
    pub primary_dark: Color32,
    /// Survol du bouton primaire.
    pub primary_darker: Color32,
    /// Fond teinté de marque (onglet actif, zone de dépôt survolée).
    pub primary_subtle: Color32,
    /// Texte, icônes et liens verts.
    pub accent: Color32,
    /// Survol d'un texte vert.
    pub accent_hover: Color32,
    /// Texte posé sur `primary_dark` ou `primary_darker`.
    pub on_primary: Color32,
    // --- Surfaces ---
    /// Fond de la fenêtre.
    pub page: Color32,
    /// Cartes, en-tête, pied, champs, menus.
    pub surface: Color32,
    /// Creux dans une carte (zone de dépôt, résultat, en-tête de tableau).
    pub sunken: Color32,
    /// Piste du sélecteur segmenté, pastille neutre.
    pub muted: Color32,
    /// Survol d'une ligne ou d'un élément de menu.
    pub hover: Color32,
    /// Survol appuyé (option du sélecteur segmenté, appui).
    pub hover_strong: Color32,
    // --- Texte ---
    pub heading: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    // --- Traits ---
    pub border: Color32,
    pub border_input: Color32,
    pub border_focus: Color32,
    /// Filet décoratif (liaison d'étapes, contour de puce).
    pub line: Color32,
    /// Contour décoratif appuyé (pastille d'étape à venir).
    pub line_strong: Color32,
    /// Piste des jauges.
    pub track: Color32,
    /// Remplissage des jauges (progression), contrasté sur la piste.
    pub meter: Color32,
    // --- États ---
    pub success: Color32,
    pub warning: Color32,
    /// Texte d'erreur.
    pub error: Color32,
    /// Fond d'erreur.
    pub error_tint: Color32,
    pub info: Color32,
    /// Fond d'une ligne à confirmer.
    pub attention_row: Color32,
    /// Bouton « sombre » et son survol.
    pub dark_button: Color32,
    pub dark_button_hover: Color32,
    // --- Teintes ---
    pub brand: Tinted,
    pub neutral: Tinted,
    pub bureautique: Tinted,
    pub dev: Tinted,
    pub graphisme: Tinted,
    pub ia: Tinted,
    pub gestion: Tinted,
    pub data: Tinted,
    /// Rayons en px.
    pub radius_sm: f32,
    pub radius_md: f32,
    pub radius_lg: f32,
    pub radius_xl: f32,
    pub radius_full: f32,
    /// Espacements en px (clé de la charte : 1 = 4 px…).
    pub space: BTreeMap<String, f32>,
    /// Ombres « sm », « md », « xl ».
    pub shadow_sm: Shadow,
    pub shadow_md: Shadow,
    pub shadow_xl: Shadow,
    /// Tailles de texte en px.
    pub text_xs: f32,
    pub text_sm: f32,
    pub text_base: f32,
    pub text_md: f32,
    pub text_lg: f32,
    pub text_xl: f32,
    pub text_2xl: f32,
    /// Clés absentes du fichier (doit rester vide).
    pub missing: Vec<String>,
}

fn parse_hex(hex: &str) -> Option<Color32> {
    let h = hex.strip_prefix('#')?;
    let byte = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    match h.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

fn to_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn to_i8(v: f32) -> i8 {
    v.round().clamp(-128.0, 127.0) as i8
}

/// Couleurs brutes de la charte, lues une fois pour les deux palettes.
struct Tokens {
    colors: BTreeMap<String, Color32>,
    missing: Vec<String>,
}

impl Tokens {
    fn c(&mut self, key: &str) -> Color32 {
        let k = format!("color-{key}");
        match self.colors.get(&k) {
            Some(c) => *c,
            None => {
                self.missing.push(k);
                MISSING
            }
        }
    }

    /// Couleurs de base, encre et teinte d'une catégorie.
    fn cat(&mut self, slug: &str) -> (Color32, Color32, Color32) {
        (
            self.c(&format!("cat-{slug}")),
            self.c(&format!("cat-{slug}-ink")),
            self.c(&format!("cat-{slug}-tint")),
        )
    }
}

/// Part de la couleur de catégorie dans le fond d'un badge sombre.
const DARK_TINT_FILL: f32 = 0.2;
/// Part de blanc dans l'encre d'un badge sombre.
const DARK_TINT_INK: f32 = 0.5;

impl Palette {
    /// Lit les jetons inclus à la compilation (thème clair).
    pub fn from_embedded() -> Result<Self, TokenError> {
        Self::from_json(TOKENS_JSON, false)
    }

    /// Palette sombre dérivée des jetons inclus.
    pub fn dark_from_embedded() -> Result<Self, TokenError> {
        Self::from_json(TOKENS_JSON, true)
    }

    pub fn from_json(json: &str, is_dark: bool) -> Result<Self, TokenError> {
        let raw: RawTokens = serde_json::from_str(json).map_err(TokenError::Json)?;
        let mut colors = BTreeMap::new();
        for (k, v) in &raw.couleurs {
            let c = parse_hex(v).ok_or_else(|| TokenError::BadColor(k.clone()))?;
            colors.insert(k.clone(), c);
        }
        let mut t = Tokens {
            colors,
            missing: Vec::new(),
        };
        let cats = [
            t.cat("bureautique"),
            t.cat("dev"),
            t.cat("graphisme"),
            t.cat("ia"),
            t.cat("gestion"),
            t.cat("data"),
        ];
        let primary = t.c("primary");
        let primary_dark = t.c("primary-dark");
        let primary_darker = t.c("primary-darker");
        let primary_light = t.c("primary-light");
        let primary_subtle = t.c("primary-subtle");
        let white = t.c("white");
        let dark = t.c("dark");
        let dark_2 = t.c("dark-2");
        let dark_3 = t.c("dark-3");
        let gray_50 = t.c("gray-50");
        let gray_100 = t.c("gray-100");
        let gray_200 = t.c("gray-200");
        let gray_300 = t.c("gray-300");
        let gray_400 = t.c("gray-400");
        let gray_500 = t.c("gray-500");
        let gray_700 = t.c("gray-700");
        let text_primary = t.c("text-primary");
        let text_secondary = t.c("text-secondary");
        let bg_subtle = t.c("bg-subtle");
        let border = t.c("border");
        let border_focus = t.c("border-focus");
        let border_input = t.c("border-input");
        let success = t.c("success");
        let warning = t.c("warning");
        let error = t.c("error");
        let info = t.c("info");
        let mut missing = t.missing;

        let mut num = |map: &BTreeMap<String, f32>, group: &str, key: &str| -> f32 {
            match map.get(key) {
                Some(v) => *v,
                None => {
                    missing.push(format!("{group}.{key}"));
                    0.0
                }
            }
        };
        let radius_sm = num(&raw.rayons, "rayons", "sm");
        let radius_md = num(&raw.rayons, "rayons", "md");
        let radius_lg = num(&raw.rayons, "rayons", "lg");
        let radius_xl = num(&raw.rayons, "rayons", "xl");
        let radius_full = num(&raw.rayons, "rayons", "full");
        let text_xs = num(&raw.texte, "texte", "xs");
        let text_sm = num(&raw.texte, "texte", "sm");
        let text_base = num(&raw.texte, "texte", "base");
        let text_md = num(&raw.texte, "texte", "md");
        let text_lg = num(&raw.texte, "texte", "lg");
        let text_xl = num(&raw.texte, "texte", "xl");
        let text_2xl = num(&raw.texte, "texte", "2xl");

        // egui ne sait dessiner qu'une ombre par forme : on garde la plus diffuse.
        // En sombre, l'ombre est plus dense pour rester visible.
        let alpha_boost = if is_dark { 3.0 } else { 1.0 };
        let mut shadow = |key: &str| -> Shadow {
            match raw.ombres.get(key).and_then(|layers| layers.first()) {
                Some(s) => Shadow {
                    offset: [to_i8(s.x), to_i8(s.y)],
                    blur: to_u8(s.flou),
                    spread: 0,
                    color: Color32::from_rgba_unmultiplied(
                        to_u8(s.rgba[0]),
                        to_u8(s.rgba[1]),
                        to_u8(s.rgba[2]),
                        to_u8((s.rgba[3] * alpha_boost).min(1.0) * 255.0),
                    ),
                },
                None => {
                    missing.push(format!("ombres.{key}"));
                    Shadow::NONE
                }
            }
        };
        let shadow_sm = shadow("sm");
        let shadow_md = shadow("md");
        let shadow_xl = shadow("xl");

        let [bureautique, dev, graphisme, ia, gestion, data] = if is_dark {
            let surface = mix(dark_3, white, DARK_SURFACE);
            cats.map(|(base, _, _)| Tinted {
                fill: mix(surface, base, DARK_TINT_FILL),
                ink: mix(base, white, DARK_TINT_INK),
                edge: base,
            })
        } else {
            cats.map(|(base, ink, tint)| Tinted {
                fill: tint,
                ink,
                edge: base,
            })
        };

        let colors = if is_dark {
            let surface = mix(dark_3, white, DARK_SURFACE);
            let accent = primary_light;
            let subtle = mix(surface, primary, 0.2);
            let error_text = mix(error, white, 0.45);
            let neutral_fill = mix(dark_3, white, 0.1);
            Roles {
                primary,
                primary_dark,
                primary_darker,
                primary_subtle: subtle,
                accent,
                accent_hover: mix(accent, white, 0.3),
                on_primary: white,
                page: dark_3,
                surface,
                sunken: mix(dark_3, white, 0.02),
                muted: dark_2,
                hover: mix(dark_3, white, 0.08),
                hover_strong: mix(dark_3, white, 0.15),
                heading: mix(white, dark_3, 0.04),
                text: mix(white, dark_3, 0.12),
                text_secondary: gray_500,
                border: mix(dark_3, white, 0.14),
                border_input,
                border_focus: accent,
                line: mix(dark_3, white, 0.22),
                line_strong: mix(dark_3, white, 0.32),
                track: mix(dark_3, white, 0.16),
                meter: accent,
                success,
                warning,
                error: error_text,
                error_tint: mix(surface, error, 0.16),
                info,
                attention_row: mix(surface, warning, 0.1),
                dark_button: mix(dark_3, white, 0.2),
                dark_button_hover: mix(dark_3, white, 0.28),
                brand: Tinted {
                    fill: subtle,
                    ink: accent,
                    edge: primary,
                },
                neutral: Tinted {
                    fill: neutral_fill,
                    ink: gray_300,
                    edge: mix(dark_3, white, 0.32),
                },
            }
        } else {
            Roles {
                primary,
                primary_dark,
                primary_darker,
                primary_subtle,
                accent: primary_dark,
                accent_hover: primary_darker,
                on_primary: white,
                page: bg_subtle,
                surface: white,
                sunken: gray_50,
                muted: gray_100,
                hover: gray_50,
                hover_strong: gray_200,
                heading: dark,
                text: text_primary,
                text_secondary,
                border,
                border_input,
                border_focus,
                line: gray_300,
                line_strong: gray_400,
                track: gray_200,
                meter: primary_dark,
                success,
                warning,
                error,
                error_tint: mix(white, error, 0.08),
                info,
                attention_row: mix(white, data.fill, 0.85),
                dark_button: dark,
                dark_button_hover: mix(dark, Color32::BLACK, 0.3),
                brand: Tinted {
                    fill: primary_subtle,
                    ink: primary_dark,
                    edge: primary,
                },
                neutral: Tinted {
                    fill: gray_100,
                    ink: gray_700,
                    edge: gray_400,
                },
            }
        };

        Ok(Self {
            dark_mode: is_dark,
            primary: colors.primary,
            primary_dark: colors.primary_dark,
            primary_darker: colors.primary_darker,
            primary_subtle: colors.primary_subtle,
            accent: colors.accent,
            accent_hover: colors.accent_hover,
            on_primary: colors.on_primary,
            page: colors.page,
            surface: colors.surface,
            sunken: colors.sunken,
            muted: colors.muted,
            hover: colors.hover,
            hover_strong: colors.hover_strong,
            heading: colors.heading,
            text: colors.text,
            text_secondary: colors.text_secondary,
            border: colors.border,
            border_input: colors.border_input,
            border_focus: colors.border_focus,
            line: colors.line,
            line_strong: colors.line_strong,
            track: colors.track,
            meter: colors.meter,
            success: colors.success,
            warning: colors.warning,
            error: colors.error,
            error_tint: colors.error_tint,
            info: colors.info,
            attention_row: colors.attention_row,
            dark_button: colors.dark_button,
            dark_button_hover: colors.dark_button_hover,
            brand: colors.brand,
            neutral: colors.neutral,
            bureautique,
            dev,
            graphisme,
            ia,
            gestion,
            data,
            radius_sm,
            radius_md,
            radius_lg,
            radius_xl,
            radius_full,
            space: raw.espacements,
            shadow_sm,
            shadow_md,
            shadow_xl,
            text_xs,
            text_sm,
            text_base,
            text_md,
            text_lg,
            text_xl,
            text_2xl,
            missing,
        })
    }

    /// Espacement de la charte (`space(4)` = 16 px). Repli sur la grille de 4 px.
    pub fn space(&self, step: u8) -> f32 {
        self.space
            .get(&step.to_string())
            .copied()
            .unwrap_or(f32::from(step) * GRID)
    }
}

/// Part de blanc dans la surface sombre (cartes) par rapport au fond `dark-3`.
const DARK_SURFACE: f32 = 0.05;

/// Couleurs par rôle, avant assemblage de la palette.
struct Roles {
    primary: Color32,
    primary_dark: Color32,
    primary_darker: Color32,
    primary_subtle: Color32,
    accent: Color32,
    accent_hover: Color32,
    on_primary: Color32,
    page: Color32,
    surface: Color32,
    sunken: Color32,
    muted: Color32,
    hover: Color32,
    hover_strong: Color32,
    heading: Color32,
    text: Color32,
    text_secondary: Color32,
    border: Color32,
    border_input: Color32,
    border_focus: Color32,
    line: Color32,
    line_strong: Color32,
    track: Color32,
    meter: Color32,
    success: Color32,
    warning: Color32,
    error: Color32,
    error_tint: Color32,
    info: Color32,
    attention_row: Color32,
    dark_button: Color32,
    dark_button_hover: Color32,
    brand: Tinted,
    neutral: Tinted,
}

/// Mélange linéaire en sRGB : `t` = part de `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| to_u8(f32::from(x) + (f32::from(y) - f32::from(x)) * t);
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Même couleur, opacité `alpha` (0 à 1).
pub fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), to_u8(alpha * 255.0))
}

static LIGHT: OnceLock<Palette> = OnceLock::new();
static DARK: OnceLock<Palette> = OnceLock::new();

thread_local! {
    /// Thème de l'image en cours sur ce fil (voir [`sync`]).
    static DARK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Palette claire, lue une fois.
pub fn light() -> &'static Palette {
    LIGHT.get_or_init(|| match Palette::from_embedded() {
        Ok(p) => p,
        // Fichier illisible : on garde une interface utilisable, en couleurs de repli.
        Err(_) => Palette::fallback(false),
    })
}

/// Palette sombre, lue une fois.
pub fn dark() -> &'static Palette {
    DARK.get_or_init(|| match Palette::dark_from_embedded() {
        Ok(p) => p,
        Err(_) => Palette::fallback(true),
    })
}

/// Palette du thème actif sur ce fil.
pub fn palette() -> &'static Palette {
    if DARK_ACTIVE.with(Cell::get) {
        dark()
    } else {
        light()
    }
}

/// Aligne la palette de ce fil sur le style de `ui` (à appeler au début de chaque image).
pub fn sync(ui: &egui::Ui) {
    set_dark(ui.visuals().dark_mode);
}

/// Choisit la palette de ce fil (galerie, tests).
pub fn set_dark(dark: bool) {
    DARK_ACTIVE.with(|d| d.set(dark));
}

impl Palette {
    fn fallback(dark: bool) -> Self {
        let (bg, fg) = if dark {
            (Color32::BLACK, Color32::WHITE)
        } else {
            (Color32::WHITE, Color32::BLACK)
        };
        let t = Tinted {
            fill: bg,
            ink: fg,
            edge: Color32::GRAY,
        };
        Self {
            dark_mode: dark,
            primary: MISSING,
            primary_dark: Color32::DARK_GREEN,
            primary_darker: Color32::DARK_GREEN,
            primary_subtle: bg,
            accent: fg,
            accent_hover: fg,
            on_primary: Color32::WHITE,
            page: bg,
            surface: bg,
            sunken: bg,
            muted: bg,
            hover: Color32::GRAY,
            hover_strong: Color32::GRAY,
            heading: fg,
            text: fg,
            text_secondary: fg,
            border: Color32::GRAY,
            border_input: Color32::GRAY,
            border_focus: fg,
            line: Color32::GRAY,
            line_strong: Color32::GRAY,
            track: Color32::GRAY,
            meter: Color32::DARK_GREEN,
            success: Color32::DARK_GREEN,
            warning: Color32::YELLOW,
            error: Color32::RED,
            error_tint: bg,
            info: Color32::BLUE,
            attention_row: bg,
            dark_button: Color32::DARK_GRAY,
            dark_button_hover: Color32::BLACK,
            brand: t,
            neutral: t,
            bureautique: t,
            dev: t,
            graphisme: t,
            ia: t,
            gestion: t,
            data: t,
            radius_sm: 4.0,
            radius_md: 8.0,
            radius_lg: 12.0,
            radius_xl: 16.0,
            radius_full: 9999.0,
            space: BTreeMap::new(),
            shadow_sm: Shadow::NONE,
            shadow_md: Shadow::NONE,
            shadow_xl: Shadow::NONE,
            text_xs: 12.0,
            text_sm: 14.0,
            text_base: 16.0,
            text_md: 18.0,
            text_lg: 20.0,
            text_xl: 24.0,
            text_2xl: 30.0,
            missing: vec!["tokens.json illisible".to_owned()],
        }
    }
}

/// Rayon egui depuis une valeur en px (plafonné à 255, ce qui suffit pour une pilule).
pub fn radius(px: f32) -> CornerRadius {
    CornerRadius::same(to_u8(px.min(255.0)))
}

/// Marge egui depuis des px.
pub fn margin(x: f32, y: f32) -> Margin {
    Margin::symmetric(to_i8(x), to_i8(y))
}

/// Polices : Georama dans toutes ses graisses, puis les polices par défaut
/// d'egui en repli pour les caractères absents de Georama.
pub fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    // Hack (police à chasse fixe d'egui) couvre les flèches et symboles absents de Georama.
    let mut fallback = fallback;
    if defs.font_data.contains_key("Hack") {
        fallback.insert(fallback.len().saturating_sub(1), "Hack".to_owned());
    }
    for w in Weight::ALL {
        let name = w.family_name().to_owned();
        defs.font_data
            .insert(name.clone(), Arc::new(FontData::from_static(w.bytes())));
        let mut chain = vec![name];
        chain.extend(fallback.iter().cloned());
        match w {
            Weight::Regular => {
                defs.families.insert(FontFamily::Proportional, chain);
            }
            other => {
                defs.families
                    .insert(FontFamily::Name(other.family_name().into()), chain);
            }
        }
    }
    defs
}

/// Applique la charte au contexte : polices, styles clair et sombre, thème choisi.
pub fn install(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_fonts(fonts());
    // Sans réglage du système connu (tests, certaines sessions), le thème clair.
    ctx.options_mut(|o| o.fallback_theme = Theme::Light);
    ctx.style_mut_of(Theme::Light, |style| apply(style, light()));
    ctx.style_mut_of(Theme::Dark, |style| apply(style, dark()));
    ctx.set_theme(choice.preference());
    egui_extras::install_image_loaders(ctx);
    super::widgets::icon::install(ctx);
}

/// Réglages du style egui pour une palette.
pub fn apply(style: &mut egui::Style, p: &Palette) {
    use TextStyle as T;
    style.text_styles = [
        (T::Small, font(Weight::Regular, p.text_xs)),
        (T::Body, font(Weight::Regular, p.text_sm)),
        (T::Button, font(Weight::Semibold, p.text_sm)),
        (T::Heading, font(Weight::Bold, p.text_xl)),
        (T::Monospace, FontId::monospace(TEXT_AREA_SIZE)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(p.space(2), p.space(2));
    s.button_padding = egui::vec2(p.space(4), p.space(2));
    s.interact_size = egui::vec2(p.space(10), p.space(8));
    s.window_margin = margin(p.space(5), p.space(5));
    s.menu_margin = margin(p.space(2), p.space(2));
    s.indent = p.space(4);
    s.icon_width = 18.0;
    s.icon_width_inner = 10.0;
    s.icon_spacing = p.space(2);
    s.combo_height = 280.0;

    let mut v = if p.dark_mode {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.override_text_color = None;
    v.hyperlink_color = p.accent;
    v.faint_bg_color = p.sunken;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.code_bg_color = p.muted;
    v.warn_fg_color = p.data.ink;
    v.error_fg_color = p.error;
    v.panel_fill = p.page;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = radius(p.radius_xl);
    v.window_shadow = p.shadow_xl;
    v.popup_shadow = p.shadow_md;
    v.menu_corner_radius = radius(p.radius_md);
    v.selection.bg_fill = mix(p.surface, p.primary, if p.dark_mode { 0.45 } else { 0.25 });
    v.selection.stroke = Stroke::new(2.0, p.border_focus);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);

    let w = &mut v.widgets;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.corner_radius = radius(p.radius_md);
    for (state, stroke) in [
        (&mut w.inactive, p.border_input),
        (&mut w.hovered, p.accent),
        (&mut w.active, p.accent_hover),
        (&mut w.open, p.accent),
    ] {
        state.fg_stroke = Stroke::new(1.5, p.text);
        state.bg_stroke = Stroke::new(1.5, stroke);
        state.bg_fill = p.surface;
        state.weak_bg_fill = p.surface;
        state.corner_radius = radius(p.radius_md);
        state.expansion = 0.0;
    }
    w.hovered.weak_bg_fill = p.hover;
    w.active.weak_bg_fill = p.hover_strong;
    style.visuals = v;
    // Durées de la charte : 150 ms pour les micro-interactions.
    style.animation_time = 0.15;
    style.interaction.selectable_labels = false;
}

/// Contraste WCAG entre deux couleurs opaques.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn lum(c: Color32) -> f32 {
        let ch = |v: u8| {
            let s = f32::from(v) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Seuils WCAG AA.
pub const AA_TEXT: f32 = 4.5;
pub const AA_LARGE: f32 = 3.0;

/// Couples texte / fond utilisés par l'interface : (nom, texte, fond).
/// Exigence : 4,5:1 (texte courant).
pub fn text_pairs(p: &Palette) -> Vec<(&'static str, Color32, Color32)> {
    let mut v = vec![
        ("texte sur surface", p.text, p.surface),
        ("texte sur page", p.text, p.page),
        ("texte sur creux", p.text, p.sunken),
        ("texte sur piste", p.text, p.muted),
        ("texte sur survol", p.text, p.hover),
        ("texte sur survol appuyé", p.text, p.hover_strong),
        ("texte sur ligne à confirmer", p.text, p.attention_row),
        ("titre sur surface", p.heading, p.surface),
        ("titre sur page", p.heading, p.page),
        ("titre sur ligne à confirmer", p.heading, p.attention_row),
        ("secondaire sur surface", p.text_secondary, p.surface),
        ("secondaire sur page", p.text_secondary, p.page),
        ("secondaire sur creux", p.text_secondary, p.sunken),
        ("secondaire sur survol", p.text_secondary, p.hover),
        (
            "secondaire sur ligne à confirmer",
            p.text_secondary,
            p.attention_row,
        ),
        ("vert sur surface", p.accent, p.surface),
        ("vert sur page", p.accent, p.page),
        ("vert sur creux", p.accent, p.sunken),
        ("vert sur survol", p.accent, p.hover),
        ("vert sur fond de marque", p.accent, p.primary_subtle),
        (
            "vert survolé sur fond de marque",
            p.accent_hover,
            p.primary_subtle,
        ),
        ("vert sur ligne à confirmer", p.accent, p.attention_row),
        ("blanc sur bouton primaire", p.on_primary, p.primary_dark),
        (
            "blanc sur bouton primaire survolé",
            p.on_primary,
            p.primary_darker,
        ),
        ("blanc sur bouton sombre", p.on_primary, p.dark_button),
        (
            "blanc sur bouton sombre survolé",
            p.on_primary,
            p.dark_button_hover,
        ),
        ("erreur sur surface", p.error, p.surface),
        ("erreur sur page", p.error, p.page),
        ("erreur sur fond d'erreur", p.error, p.error_tint),
        ("texte sur fond d'erreur", p.text, p.error_tint),
        ("texte sur bandeau info", p.text, p.dev.fill),
        ("titre sur bandeau info", p.heading, p.dev.fill),
        ("texte sur bandeau succès", p.text, p.primary_subtle),
        ("texte sur bandeau avertissement", p.text, p.data.fill),
        ("valeur généralisée sur surface", p.data.ink, p.surface),
        (
            "valeur généralisée sur ligne à confirmer",
            p.data.ink,
            p.attention_row,
        ),
    ];
    for (name, t) in [
        ("badge marque", p.brand),
        ("badge neutre", p.neutral),
        ("badge Personnes", p.bureautique),
        ("badge Organisations", p.dev),
        ("badge Lieux", p.graphisme),
        ("badge Contacts", p.ia),
        ("badge Identifiants", p.gestion),
        ("badge Finances", p.data),
    ] {
        v.push((name, t.ink, t.fill));
    }
    v
}

/// Éléments d'interface (bords de champ, anneau de focus, icônes, cases,
/// jauges) : (nom, élément, fond). Exigence : 3:1.
pub fn ui_pairs(p: &Palette) -> Vec<(&'static str, Color32, Color32)> {
    vec![
        // Les champs sont toujours posés dans une carte (surface).
        ("bord de champ sur surface", p.border_input, p.surface),
        ("focus sur surface", p.border_focus, p.surface),
        ("focus sur page", p.border_focus, p.page),
        ("focus sur creux", p.border_focus, p.sunken),
        ("icône verte sur surface", p.accent, p.surface),
        ("icône secondaire sur surface", p.text_secondary, p.surface),
        ("icône d'erreur sur fond d'erreur", p.error, p.error_tint),
        ("icône d'avertissement sur bandeau", p.data.ink, p.data.fill),
        ("icône d'information sur bandeau", p.dev.ink, p.dev.fill),
        ("icône de succès sur bandeau", p.accent, p.primary_subtle),
        ("case cochée sur surface", p.primary_dark, p.surface),
        ("étape courante sur page", p.primary_dark, p.page),
        ("jauge de progression sur piste", p.meter, p.track),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_tokens_parse_without_missing_keys() {
        for dark in [false, true] {
            let p = Palette::from_json(TOKENS_JSON, dark);
            assert!(p.is_ok());
            if let Ok(p) = p {
                assert!(p.missing.is_empty(), "clés absentes : {:?}", p.missing);
                assert_eq!(p.primary_dark, Color32::from_rgb(0x3d, 0x7a, 0x62));
                assert_eq!(p.radius_xl, 16.0);
                assert_eq!(p.space(4), 16.0);
                assert_eq!(p.text_base, 16.0);
                assert_eq!(p.shadow_md.blur, 12);
                assert_eq!(p.dark_mode, dark);
            }
        }
        // Fond sombre de la charte : `color-dark-3`.
        assert_eq!(dark().page, Color32::from_rgb(0x1a, 0x1a, 0x1a));
        assert_eq!(light().surface, Color32::WHITE);
    }

    #[test]
    fn spacing_and_controls_are_on_the_4px_grid() {
        let p = light();
        for step in [1u8, 2, 3, 4, 5, 6, 8, 10, 12] {
            assert_eq!(p.space(step) % GRID, 0.0, "pas {step}");
        }
        for h in [CONTROL_SM, CONTROL_MD, CONTROL_LG] {
            assert_eq!(h % GRID, 0.0, "hauteur {h}");
        }
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#ffffff"), Some(Color32::WHITE));
        assert_eq!(parse_hex("#00000014").map(|c| c.a()), Some(0x14));
        assert_eq!(parse_hex("ffffff"), None);
        assert_eq!(parse_hex("#fff"), None);
    }

    /// Couple le plus faible d'une liste.
    fn weakest(pairs: &[(&'static str, Color32, Color32)]) -> (&'static str, f32) {
        pairs
            .iter()
            .map(|(n, a, b)| (*n, contrast(*a, *b)))
            .fold(
                ("", f32::INFINITY),
                |acc, x| if x.1 < acc.1 { x } else { acc },
            )
    }

    #[test]
    fn every_text_and_ui_pair_meets_aa_in_both_themes() {
        for p in [light(), dark()] {
            let theme = if p.dark_mode { "sombre" } else { "clair" };
            let mut failures = Vec::new();
            for (name, fg, bg) in text_pairs(p) {
                let c = contrast(fg, bg);
                if c < AA_TEXT {
                    failures.push(format!("{theme} : {name} = {c:.2}:1 (texte, 4,5 requis)"));
                }
            }
            for (name, fg, bg) in ui_pairs(p) {
                let c = contrast(fg, bg);
                if c < AA_LARGE {
                    failures.push(format!("{theme} : {name} = {c:.2}:1 (interface, 3 requis)"));
                }
            }
            assert!(failures.is_empty(), "{failures:#?}");
            let (tn, tc) = weakest(&text_pairs(p));
            let (un, uc) = weakest(&ui_pairs(p));
            println!(
                "{theme} : texte le plus faible « {tn} » {tc:.2}:1 ; interface « {un} » {uc:.2}:1"
            );
        }
        // Le vert de marque n'atteint pas l'AA en clair : il ne sert jamais de texte.
        assert!(contrast(light().primary, light().surface) < AA_TEXT);
    }

    #[test]
    fn thread_local_theme_does_not_leak_between_threads() {
        set_dark(true);
        assert!(palette().dark_mode);
        let other = std::thread::spawn(|| palette().dark_mode).join();
        assert_eq!(other.ok(), Some(false), "un autre fil garde le thème clair");
        set_dark(false);
        assert!(!palette().dark_mode);
    }

    #[test]
    fn theme_choice_maps_to_egui() {
        assert_eq!(
            ThemeChoice::System.preference(),
            egui::ThemePreference::System
        );
        assert_eq!(ThemeChoice::Dark.preference(), egui::ThemePreference::Dark);
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }
}
