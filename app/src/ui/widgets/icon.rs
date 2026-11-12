//! Icônes Lucide 0.460.0 (`assets/charte/icons`). Les SVG sont rendus en blanc
//! puis teintés à la couleur voulue, comme le masque CSS de la charte.

use egui::{Color32, ImageSource, Rect, Response, Sense, Ui, Vec2};

/// Icônes disponibles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Check,
    ChevronDown,
    ChevronRight,
    CircleHelp,
    Clipboard,
    Copy,
    Download,
    EyeOff,
    Eye,
    FileSpreadsheet,
    FileText,
    File,
    FolderOpen,
    Image,
    Info,
    KeyRound,
    LockOpen,
    Lock,
    Plus,
    Presentation,
    RefreshCw,
    Search,
    Settings,
    ShieldCheck,
    Sparkles,
    Trash,
    TriangleAlert,
    Undo,
    Upload,
    X,
}

macro_rules! svg {
    ($name:literal) => {
        include_str!(concat!("../../../../assets/charte/icons/", $name, ".svg"))
    };
}

impl Icon {
    pub const ALL: [Icon; 30] = [
        Icon::Check,
        Icon::ChevronDown,
        Icon::ChevronRight,
        Icon::CircleHelp,
        Icon::Clipboard,
        Icon::Copy,
        Icon::Download,
        Icon::EyeOff,
        Icon::Eye,
        Icon::FileSpreadsheet,
        Icon::FileText,
        Icon::File,
        Icon::FolderOpen,
        Icon::Image,
        Icon::Info,
        Icon::KeyRound,
        Icon::LockOpen,
        Icon::Lock,
        Icon::Plus,
        Icon::Presentation,
        Icon::RefreshCw,
        Icon::Search,
        Icon::Settings,
        Icon::ShieldCheck,
        Icon::Sparkles,
        Icon::Trash,
        Icon::TriangleAlert,
        Icon::Undo,
        Icon::Upload,
        Icon::X,
    ];

    fn name(self) -> &'static str {
        match self {
            Icon::Check => "check",
            Icon::ChevronDown => "chevron-down",
            Icon::ChevronRight => "chevron-right",
            Icon::CircleHelp => "circle-help",
            Icon::Clipboard => "clipboard",
            Icon::Copy => "copy",
            Icon::Download => "download",
            Icon::EyeOff => "eye-off",
            Icon::Eye => "eye",
            Icon::FileSpreadsheet => "file-spreadsheet",
            Icon::FileText => "file-text",
            Icon::File => "file",
            Icon::FolderOpen => "folder-open",
            Icon::Image => "image",
            Icon::Info => "info",
            Icon::KeyRound => "key-round",
            Icon::LockOpen => "lock-open",
            Icon::Lock => "lock",
            Icon::Plus => "plus",
            Icon::Presentation => "presentation",
            Icon::RefreshCw => "refresh-cw",
            Icon::Search => "search",
            Icon::Settings => "settings",
            Icon::ShieldCheck => "shield-check",
            Icon::Sparkles => "sparkles",
            Icon::Trash => "trash-2",
            Icon::TriangleAlert => "triangle-alert",
            Icon::Undo => "undo-2",
            Icon::Upload => "upload",
            Icon::X => "x",
        }
    }

    fn svg(self) -> &'static str {
        match self {
            Icon::Check => svg!("check"),
            Icon::ChevronDown => svg!("chevron-down"),
            Icon::ChevronRight => svg!("chevron-right"),
            Icon::CircleHelp => svg!("circle-help"),
            Icon::Clipboard => svg!("clipboard"),
            Icon::Copy => svg!("copy"),
            Icon::Download => svg!("download"),
            Icon::EyeOff => svg!("eye-off"),
            Icon::Eye => svg!("eye"),
            Icon::FileSpreadsheet => svg!("file-spreadsheet"),
            Icon::FileText => svg!("file-text"),
            Icon::File => svg!("file"),
            Icon::FolderOpen => svg!("folder-open"),
            Icon::Image => svg!("image"),
            Icon::Info => svg!("info"),
            Icon::KeyRound => svg!("key-round"),
            Icon::LockOpen => svg!("lock-open"),
            Icon::Lock => svg!("lock"),
            Icon::Plus => svg!("plus"),
            Icon::Presentation => svg!("presentation"),
            Icon::RefreshCw => svg!("refresh-cw"),
            Icon::Search => svg!("search"),
            Icon::Settings => svg!("settings"),
            Icon::ShieldCheck => svg!("shield-check"),
            Icon::Sparkles => svg!("sparkles"),
            Icon::Trash => svg!("trash-2"),
            Icon::TriangleAlert => svg!("triangle-alert"),
            Icon::Undo => svg!("undo-2"),
            Icon::Upload => svg!("upload"),
            Icon::X => svg!("x"),
        }
    }

    fn uri(self) -> String {
        format!("bytes://icones/{}.svg", self.name())
    }

    fn source(self) -> ImageSource<'static> {
        ImageSource::Uri(self.uri().into())
    }

    /// Image egui de l'icône, teintée et dimensionnée.
    pub fn image(self, color: Color32, size: f32) -> egui::Image<'static> {
        egui::Image::new(self.source())
            .tint(color)
            .fit_to_exact_size(Vec2::splat(size))
    }

    /// Dessine l'icône centrée dans `rect`.
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        self.image(color, rect.width().min(rect.height()))
            .paint_at(ui, rect);
    }

    /// Ajoute l'icône seule (décorative, sans interaction).
    pub fn show(self, ui: &mut Ui, color: Color32, size: f32) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        self.paint(ui, rect, color);
        response
    }
}

/// Enregistre les SVG auprès du chargeur d'images, en blanc pour pouvoir les teinter.
pub fn install(ctx: &egui::Context) {
    for icon in Icon::ALL {
        let white = icon.svg().replace("currentColor", "#ffffff");
        ctx.include_bytes(icon.uri(), white.into_bytes());
    }
}
