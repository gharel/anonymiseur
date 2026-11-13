//! Aperçu des images analysées et zones de masquage tracées à la main :
//! conversion entre les points de l'écran et les pixels de l'image, liste des
//! zones par image. Sans egui.

use anon_core::PixelRect;
use anon_engine::ManualMask;

/// Côté minimal d'une zone tracée, en pixels de l'image : en dessous, c'est
/// un clic, pas un tracé.
pub const MIN_MASK_PX: u32 = 4;
/// Agrandissement maximal d'une petite image à l'écran.
pub const MAX_UPSCALE: f32 = 2.0;

/// Placement d'une image à l'écran : coin haut gauche (points) et échelle
/// (points par pixel de l'image).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    pub width: u32,
    pub height: u32,
}

impl Fit {
    /// Image `width` × `height` pixels posée en (`left`, `top`), réduite pour
    /// tenir dans `max_w` × `max_h` points, agrandie au plus de [`MAX_UPSCALE`].
    /// `None` pour une image vide ou une place nulle.
    pub fn new(
        left: f32,
        top: f32,
        max_w: f32,
        max_h: f32,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        if width == 0 || height == 0 || max_w <= 0.0 || max_h <= 0.0 {
            return None;
        }
        let scale = (max_w / width as f32)
            .min(max_h / height as f32)
            .min(MAX_UPSCALE);
        Some(Self {
            left,
            top,
            scale,
            width,
            height,
        })
    }

    /// Taille affichée, en points.
    pub fn size(&self) -> (f32, f32) {
        (
            self.width as f32 * self.scale,
            self.height as f32 * self.scale,
        )
    }

    /// Même échelle, autre coin (la mise en page place l'image après coup).
    pub fn at(self, left: f32, top: f32) -> Self {
        Self { left, top, ..self }
    }

    /// Rectangle en pixels → rectangle à l'écran `[x0, y0, x1, y1]`.
    pub fn to_screen(&self, r: PixelRect) -> [f32; 4] {
        [
            self.left + r.x as f32 * self.scale,
            self.top + r.y as f32 * self.scale,
            self.left + (r.x.saturating_add(r.w)) as f32 * self.scale,
            self.top + (r.y.saturating_add(r.h)) as f32 * self.scale,
        ]
    }

    fn axis(&self, v: f32, origin: f32, max: u32, round_up: bool) -> u32 {
        let px = (v - origin) / self.scale;
        let px = if round_up { px.ceil() } else { px.floor() };
        if px.is_nan() || px <= 0.0 {
            0
        } else if px >= max as f32 {
            max
        } else {
            px as u32
        }
    }

    /// Point de l'écran → pixel de l'image, ramené dans l'image.
    pub fn to_pixel(&self, x: f32, y: f32) -> (u32, u32) {
        (
            self.axis(x, self.left, self.width, false),
            self.axis(y, self.top, self.height, false),
        )
    }

    /// Rectangle tracé de `a` à `b` (points de l'écran, dans n'importe quel
    /// sens) → zone en pixels, coupée aux bords de l'image. `None` si la zone
    /// est trop petite (simple clic) ou hors de l'image.
    pub fn drag_to_rect(&self, a: (f32, f32), b: (f32, f32)) -> Option<PixelRect> {
        let x0 = self.axis(a.0.min(b.0), self.left, self.width, false);
        let y0 = self.axis(a.1.min(b.1), self.top, self.height, false);
        let x1 = self.axis(a.0.max(b.0), self.left, self.width, true);
        let y1 = self.axis(a.1.max(b.1), self.top, self.height, true);
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        (w >= MIN_MASK_PX && h >= MIN_MASK_PX).then_some(PixelRect { x: x0, y: y0, w, h })
    }
}

/// Zone tracée à la main sur une image d'un fichier du lot.
#[derive(Clone, PartialEq, Eq)]
pub struct FileMask {
    /// Fichier d'origine (deux fichiers peuvent avoir une image du même nom).
    pub file: String,
    pub mask: ManualMask,
}

impl std::fmt::Debug for FileMask {
    /// Sans le nom du fichier (il peut désigner une personne).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileMask")
            .field("media", &self.mask.media)
            .field("rect", &self.mask.rect)
            .finish_non_exhaustive()
    }
}

/// Zones manuelles du lot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManualMasks {
    pub masks: Vec<FileMask>,
}

impl ManualMasks {
    pub fn add(&mut self, file: &str, media: &str, rect: PixelRect) {
        let m = FileMask {
            file: file.to_owned(),
            mask: ManualMask {
                media: media.to_owned(),
                rect,
            },
        };
        if !self.masks.contains(&m) {
            self.masks.push(m);
        }
    }

    /// Zones d'une image, dans l'ordre du tracé.
    pub fn of_image(&self, file: &str, media: &str) -> Vec<PixelRect> {
        self.masks
            .iter()
            .filter(|m| m.file == file && m.mask.media == media)
            .map(|m| m.mask.rect)
            .collect()
    }

    /// Retire la `index`-ième zone de cette image.
    pub fn remove(&mut self, file: &str, media: &str, index: usize) {
        let pos = self
            .masks
            .iter()
            .enumerate()
            .filter(|(_, m)| m.file == file && m.mask.media == media)
            .nth(index)
            .map(|(i, _)| i);
        if let Some(i) = pos {
            self.masks.remove(i);
        }
    }

    pub fn clear_image(&mut self, file: &str, media: &str) {
        self.masks
            .retain(|m| !(m.file == file && m.mask.media == media));
    }

    /// Zones à transmettre au moteur pour le fichier `file`.
    pub fn for_file(&self, file: &str) -> Vec<ManualMask> {
        self.masks
            .iter()
            .filter(|m| m.file == file)
            .map(|m| m.mask.clone())
            .collect()
    }

    /// Garde seulement les zones des images encore présentes.
    pub fn retain_images(&mut self, keep: impl Fn(&str, &str) -> bool) {
        self.masks.retain(|m| keep(&m.file, &m.mask.media));
    }

    pub fn len(&self) -> usize {
        self.masks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u32, y: u32, w: u32, h: u32) -> PixelRect {
        PixelRect { x, y, w, h }
    }

    #[test]
    fn fit_reduces_and_caps_upscale() {
        let f = Fit::new(10.0, 20.0, 500.0, 400.0, 1500, 860);
        let f = f.unwrap_or_else(|| unreachable!());
        assert!((f.scale - 500.0 / 1500.0).abs() < 1e-6);
        let (w, h) = f.size();
        assert!((w - 500.0).abs() < 1e-3 && h <= 400.0);
        let small = Fit::new(0.0, 0.0, 1000.0, 1000.0, 100, 50);
        assert_eq!(small.map(|f| f.scale), Some(MAX_UPSCALE));
        assert_eq!(Fit::new(0.0, 0.0, 100.0, 100.0, 0, 10), None);
        assert_eq!(Fit::new(0.0, 0.0, 0.0, 100.0, 10, 10), None);
    }

    #[test]
    fn screen_and_pixels_round_trip() {
        let f = Fit {
            left: 100.0,
            top: 50.0,
            scale: 0.5,
            width: 1000,
            height: 600,
        };
        let r = rect(200, 100, 300, 40);
        let [x0, y0, x1, y1] = f.to_screen(r);
        assert_eq!([x0, y0, x1, y1], [200.0, 100.0, 350.0, 120.0]);
        assert_eq!(f.drag_to_rect((x0, y0), (x1, y1)), Some(r));
        // Tracé de bas en haut et de droite à gauche : même zone.
        assert_eq!(f.drag_to_rect((x1, y1), (x0, y0)), Some(r));
        assert_eq!(f.to_pixel(100.0, 50.0), (0, 0));
        assert_eq!(f.to_pixel(600.0, 350.0), (1000, 600));
    }

    #[test]
    fn drag_is_clamped_and_small_drags_ignored() {
        let f = Fit {
            left: 0.0,
            top: 0.0,
            scale: 2.0,
            width: 100,
            height: 50,
        };
        // Débordement : coupé aux bords.
        assert_eq!(
            f.drag_to_rect((-40.0, -10.0), (500.0, 20.0)),
            Some(rect(0, 0, 100, 10))
        );
        // Simple clic, ou zone de moins de MIN_MASK_PX pixels.
        assert_eq!(f.drag_to_rect((20.0, 20.0), (20.5, 20.5)), None);
        assert_eq!(f.drag_to_rect((20.0, 20.0), (25.0, 80.0)), None);
        // Entièrement hors de l'image.
        assert_eq!(f.drag_to_rect((300.0, 0.0), (400.0, 50.0)), None);
        // Valeurs non finies : rien de tracé, pas de panique.
        assert_eq!(f.drag_to_rect((f32::NAN, 0.0), (f32::NAN, 10.0)), None);
    }

    #[test]
    fn masks_per_image_and_file() {
        let mut m = ManualMasks::default();
        m.add("a.png", "image", rect(1, 1, 10, 10));
        m.add("a.png", "image", rect(1, 1, 10, 10));
        m.add("a.png", "image", rect(20, 20, 10, 10));
        m.add("b.png", "image", rect(5, 5, 10, 10));
        m.add("c.docx", "word/media/image1.png", rect(0, 0, 8, 8));
        assert_eq!(m.len(), 4, "doublon ignoré");
        assert_eq!(m.of_image("a.png", "image").len(), 2);
        assert_eq!(m.for_file("b.png").len(), 1);
        assert_eq!(m.for_file("b.png")[0].media, "image");
        m.remove("a.png", "image", 1);
        assert_eq!(m.of_image("a.png", "image"), vec![rect(1, 1, 10, 10)]);
        m.remove("a.png", "image", 5);
        assert_eq!(m.len(), 3);
        m.clear_image("b.png", "image");
        assert!(m.for_file("b.png").is_empty());
        m.retain_images(|f, _| f != "c.docx");
        assert_eq!(m.len(), 1);
        assert!(!format!("{:?}", m).contains("a.png"));
    }
}
