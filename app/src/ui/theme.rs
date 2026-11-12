//! Thème Skazy Formation : couleurs, rayons, espacements et tailles de texte lus
//! dans `assets/charte/tokens.json` (copie générée de la charte, voir `AGENTS.md`).
//!
//! Le fichier est inclus à la compilation. Une clé absente ne fait pas paniquer
//! l'application : elle donne une couleur de repli voyante, et un test vérifie
//! qu'aucune clé n'y tombe.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle, Visuals,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

const TOKENS_JSON: &str = include_str!("../../../assets/charte/tokens.json");

/// Couleur de repli quand une clé manque : magenta, impossible à rater.
const MISSING: Color32 = Color32::from_rgb(255, 0, 255);

/// Grille d'espacement de la charte : multiples de 4 px.
pub const GRID: f32 = 4.0;

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

/// Les jetons utilisés par l'interface.
#[derive(Debug, Clone)]
pub struct Palette {
    pub primary: Color32,
    pub primary_dark: Color32,
    pub primary_darker: Color32,
    pub primary_light: Color32,
    pub primary_subtle: Color32,
    pub white: Color32,
    pub dark: Color32,
    pub gray_50: Color32,
    pub gray_100: Color32,
    pub gray_200: Color32,
    pub gray_300: Color32,
    pub gray_400: Color32,
    pub gray_500: Color32,
    pub gray_700: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub bg_page: Color32,
    pub bg_subtle: Color32,
    pub bg_muted: Color32,
    pub border: Color32,
    pub border_focus: Color32,
    pub border_input: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub info: Color32,
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

impl Palette {
    /// Lit les jetons inclus à la compilation.
    pub fn from_embedded() -> Result<Self, TokenError> {
        Self::from_json(TOKENS_JSON)
    }

    pub fn from_json(json: &str) -> Result<Self, TokenError> {
        let raw: RawTokens = serde_json::from_str(json).map_err(TokenError::Json)?;
        let mut missing = Vec::new();
        let mut colors = BTreeMap::new();
        for (k, v) in &raw.couleurs {
            let c = parse_hex(v).ok_or_else(|| TokenError::BadColor(k.clone()))?;
            colors.insert(k.as_str(), c);
        }
        let mut c = |key: &str| -> Color32 {
            match colors.get(format!("color-{key}").as_str()) {
                Some(c) => *c,
                None => {
                    missing.push(format!("color-{key}"));
                    MISSING
                }
            }
        };
        let mut cat = |slug: &str| Tinted {
            fill: c(&format!("cat-{slug}-tint")),
            ink: c(&format!("cat-{slug}-ink")),
            edge: c(&format!("cat-{slug}")),
        };
        let bureautique = cat("bureautique");
        let dev = cat("dev");
        let graphisme = cat("graphisme");
        let ia = cat("ia");
        let gestion = cat("gestion");
        let data = cat("data");

        let primary = c("primary");
        let primary_dark = c("primary-dark");
        let primary_darker = c("primary-darker");
        let primary_light = c("primary-light");
        let primary_subtle = c("primary-subtle");
        let white = c("white");
        let dark = c("dark");
        let gray_50 = c("gray-50");
        let gray_100 = c("gray-100");
        let gray_200 = c("gray-200");
        let gray_300 = c("gray-300");
        let gray_400 = c("gray-400");
        let gray_500 = c("gray-500");
        let gray_700 = c("gray-700");
        let text = c("text-primary");
        let text_secondary = c("text-secondary");
        let bg_page = c("bg-page");
        let bg_subtle = c("bg-subtle");
        let bg_muted = c("bg-muted");
        let border = c("border");
        let border_focus = c("border-focus");
        let border_input = c("border-input");
        let success = c("success");
        let warning = c("warning");
        let error = c("error");
        let info = c("info");

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
                        to_u8(s.rgba[3] * 255.0),
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

        Ok(Self {
            primary,
            primary_dark,
            primary_darker,
            primary_light,
            primary_subtle,
            white,
            dark,
            gray_50,
            gray_100,
            gray_200,
            gray_300,
            gray_400,
            gray_500,
            gray_700,
            text,
            text_secondary,
            bg_page,
            bg_subtle,
            bg_muted,
            border,
            border_focus,
            border_input,
            success,
            warning,
            error,
            info,
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

    /// Fond teinté et encre pour l'erreur (la charte ne fournit pas de teinte).
    pub fn error_tint(&self) -> Color32 {
        mix(self.white, self.error, 0.08)
    }
}

/// Mélange linéaire en sRGB : `t` = part de `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| to_u8(f32::from(x) + (f32::from(y) - f32::from(x)) * t);
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

static PALETTE: OnceLock<Palette> = OnceLock::new();

/// Palette globale, lue une fois.
pub fn palette() -> &'static Palette {
    PALETTE.get_or_init(|| match Palette::from_embedded() {
        Ok(p) => p,
        // Fichier illisible : on garde une interface utilisable, en couleurs de repli.
        Err(_) => Palette::fallback(),
    })
}

impl Palette {
    fn fallback() -> Self {
        let t = Tinted {
            fill: Color32::WHITE,
            ink: Color32::BLACK,
            edge: Color32::GRAY,
        };
        Self {
            primary: MISSING,
            primary_dark: Color32::BLACK,
            primary_darker: Color32::BLACK,
            primary_light: MISSING,
            primary_subtle: Color32::WHITE,
            white: Color32::WHITE,
            dark: Color32::BLACK,
            gray_50: Color32::WHITE,
            gray_100: Color32::WHITE,
            gray_200: Color32::LIGHT_GRAY,
            gray_300: Color32::LIGHT_GRAY,
            gray_400: Color32::GRAY,
            gray_500: Color32::GRAY,
            gray_700: Color32::DARK_GRAY,
            text: Color32::BLACK,
            text_secondary: Color32::DARK_GRAY,
            bg_page: Color32::WHITE,
            bg_subtle: Color32::WHITE,
            bg_muted: Color32::WHITE,
            border: Color32::LIGHT_GRAY,
            border_focus: Color32::BLACK,
            border_input: Color32::GRAY,
            success: Color32::DARK_GREEN,
            warning: Color32::YELLOW,
            error: Color32::RED,
            info: Color32::BLUE,
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

/// Applique la charte au contexte : polices, couleurs, rayons, espacements.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Light);
    let p = palette();
    ctx.all_styles_mut(|style| apply(style, p));
    egui_extras::install_image_loaders(ctx);
    super::widgets::icon::install(ctx);
}

/// Réglages du style egui (thème clair uniquement).
pub fn apply(style: &mut egui::Style, p: &Palette) {
    use TextStyle as T;
    style.text_styles = [
        (T::Small, font(Weight::Regular, p.text_xs)),
        (T::Body, font(Weight::Regular, p.text_sm)),
        (T::Button, font(Weight::Semibold, p.text_sm)),
        (T::Heading, font(Weight::Bold, p.text_xl)),
        (T::Monospace, FontId::monospace(p.text_sm - 1.0)),
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

    let mut v = Visuals::light();
    v.override_text_color = None;
    v.hyperlink_color = p.primary_dark;
    v.faint_bg_color = p.gray_50;
    v.extreme_bg_color = p.white;
    v.text_edit_bg_color = Some(p.white);
    v.code_bg_color = p.bg_muted;
    v.warn_fg_color = p.data.ink;
    v.error_fg_color = p.error;
    v.panel_fill = p.bg_subtle;
    v.window_fill = p.white;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = radius(p.radius_xl);
    v.window_shadow = p.shadow_xl;
    v.popup_shadow = p.shadow_md;
    v.menu_corner_radius = radius(p.radius_md);
    v.selection.bg_fill = p.primary_subtle;
    v.selection.stroke = Stroke::new(2.0, p.border_focus);
    v.text_cursor.stroke = Stroke::new(2.0, p.primary_dark);

    let w = &mut v.widgets;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.bg_fill = p.white;
    w.noninteractive.weak_bg_fill = p.white;
    w.noninteractive.corner_radius = radius(p.radius_md);
    for (state, stroke) in [
        (&mut w.inactive, p.border_input),
        (&mut w.hovered, p.primary_dark),
        (&mut w.active, p.primary_darker),
        (&mut w.open, p.primary_dark),
    ] {
        state.fg_stroke = Stroke::new(1.5, p.text);
        state.bg_stroke = Stroke::new(1.5, stroke);
        state.bg_fill = p.white;
        state.weak_bg_fill = p.white;
        state.corner_radius = radius(p.radius_md);
        state.expansion = 0.0;
    }
    w.hovered.weak_bg_fill = p.gray_50;
    w.active.weak_bg_fill = p.gray_100;
    style.visuals = v;
    // Durées de la charte : 150 ms pour les micro-interactions.
    style.animation_time = 0.15;
    style.interaction.selectable_labels = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_tokens_parse_without_missing_keys() {
        let p = Palette::from_embedded();
        assert!(p.is_ok());
        if let Ok(p) = p {
            assert!(p.missing.is_empty(), "clés absentes : {:?}", p.missing);
            assert_eq!(p.primary_dark, Color32::from_rgb(0x3d, 0x7a, 0x62));
            assert_eq!(p.radius_xl, 16.0);
            assert_eq!(p.space(4), 16.0);
            assert_eq!(p.text_base, 16.0);
            assert_eq!(p.shadow_md.blur, 12);
        }
    }

    #[test]
    fn spacing_is_on_the_4px_grid() {
        let p = palette();
        for step in [1u8, 2, 3, 4, 5, 6, 8, 10, 12] {
            assert_eq!(p.space(step) % GRID, 0.0, "pas {step}");
        }
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#ffffff"), Some(Color32::WHITE));
        assert_eq!(parse_hex("#00000014").map(|c| c.a()), Some(0x14));
        assert_eq!(parse_hex("ffffff"), None);
        assert_eq!(parse_hex("#fff"), None);
    }

    /// Contraste WCAG entre deux couleurs opaques.
    fn contrast(a: Color32, b: Color32) -> f32 {
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

    #[test]
    fn text_pairs_meet_aa() {
        let p = palette();
        let pairs = [
            (p.text, p.white),
            (p.text, p.bg_subtle),
            (p.text_secondary, p.white),
            (p.text_secondary, p.bg_subtle),
            (p.primary_dark, p.white),
            (p.white, p.primary_dark),
            (p.white, p.primary_darker),
            (p.error, p.white),
            (p.error, p.error_tint()),
            (p.primary_dark, p.primary_subtle),
            (p.bureautique.ink, p.bureautique.fill),
            (p.dev.ink, p.dev.fill),
            (p.graphisme.ink, p.graphisme.fill),
            (p.ia.ink, p.ia.fill),
            (p.gestion.ink, p.gestion.fill),
            (p.data.ink, p.data.fill),
            (p.gray_700, p.gray_100),
        ];
        for (fg, bg) in pairs {
            assert!(contrast(fg, bg) >= 4.5, "{fg:?} sur {bg:?}");
        }
        // Le vert de marque n'atteint pas l'AA : il ne doit jamais servir de texte.
        assert!(contrast(p.primary, p.white) < 4.5);
        // Bord des champs et anneau de focus : 3:1 minimum.
        assert!(contrast(p.border_input, p.white) >= 3.0);
        assert!(contrast(p.border_focus, p.white) >= 3.0);
    }
}
