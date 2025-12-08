#![allow(dead_code)]
/// Viewport configuration for controlling page dimensions and device emulation.
#[derive(Debug, Clone)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
    pub is_mobile: bool,
    pub has_touch: bool,
    pub is_landscape: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            device_scale_factor: 1.0,
            is_mobile: false,
            has_touch: false,
            is_landscape: false,
        }
    }
}

impl Viewport {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    pub fn with_device_scale_factor(mut self, factor: f64) -> Self {
        self.device_scale_factor = factor;
        self
    }
}

/// Screenshot format options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFormat {
    #[default]
    Jpeg,
    Png,
    WebP,
}

impl ImageFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Png => "png",
            ImageFormat::WebP => "webp",
        }
    }
}

/// Defines a rectangular region for clipping screenshots.
#[derive(Debug, Clone, Copy)]
pub struct ClipRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// Configuration options for HTML screenshot capture.
#[derive(Debug, Clone, Default)]
pub struct CaptureOptions {
    pub(crate) format: ImageFormat,
    pub(crate) quality: Option<u8>,
    pub(crate) viewport: Option<Viewport>,
    pub(crate) full_page: bool,
    pub(crate) omit_background: bool,
    pub(crate) clip: Option<ClipRegion>,
}

impl CaptureOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_format(mut self, format: ImageFormat) -> Self {
        self.format = format;
        self
    }

    pub fn with_quality(mut self, quality: u8) -> Self {
        self.quality = Some(quality.min(100));
        self
    }

    pub fn with_viewport(mut self, viewport: Viewport) -> Self {
        self.viewport = Some(viewport);
        self
    }

    pub fn with_full_page(mut self, full_page: bool) -> Self {
        self.full_page = full_page;
        self
    }

    pub fn hidpi() -> Self {
        Self::new().with_viewport(Viewport::default().with_device_scale_factor(2.0))
    }
}
