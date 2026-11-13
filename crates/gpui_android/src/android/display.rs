//! Android display handling using `AndroidApp` configuration and `NativeWindow`.

use android_activity::AndroidApp;
use anyhow::Result;
use gpui::{Bounds, DisplayId, Pixels, PlatformDisplay, px, size};
use uuid::Uuid;

const DEFAULT_SCALE_FACTOR: f32 = 1.0;
const DEFAULT_WIDTH_DP: f32 = 412.0;
const DEFAULT_HEIGHT_DP: f32 = 915.0;
const DEFAULT_REFRESH_RATE_HZ: f64 = 60.0;

#[derive(Debug, Clone)]
pub(crate) struct AndroidDisplay {
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    refresh_rate_hz: f64,
}

impl AndroidDisplay {
    pub(crate) fn from_app(app: Option<&AndroidApp>) -> Self {
        let Some(app) = app else {
            return Self::fallback();
        };
        let config = app.config();
        let scale_factor = config
            .density()
            .map(|dpi| (dpi as f32 / 160.0).max(1.0))
            .unwrap_or(DEFAULT_SCALE_FACTOR);

        let (width_dp, height_dp) = if let Some(native_window) = app.native_window() {
            let width = (native_window.width().max(1) as f32) / scale_factor;
            let height = (native_window.height().max(1) as f32) / scale_factor;
            (width, height)
        } else {
            let width = config
                .screen_width_dp()
                .map(|w| w.max(1) as f32)
                .unwrap_or(DEFAULT_WIDTH_DP);
            let height = config
                .screen_height_dp()
                .map(|h| h.max(1) as f32)
                .unwrap_or(DEFAULT_HEIGHT_DP);
            (width, height)
        };

        Self {
            bounds: Bounds {
                origin: Default::default(),
                size: size(px(width_dp), px(height_dp)),
            },
            scale_factor,
            refresh_rate_hz: DEFAULT_REFRESH_RATE_HZ,
        }
    }

    pub(crate) fn fallback() -> Self {
        Self {
            bounds: Bounds {
                origin: Default::default(),
                size: size(px(DEFAULT_WIDTH_DP), px(DEFAULT_HEIGHT_DP)),
            },
            scale_factor: DEFAULT_SCALE_FACTOR,
            refresh_rate_hz: DEFAULT_REFRESH_RATE_HZ,
        }
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
}

impl PlatformDisplay for AndroidDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::new(1)
    }

    fn uuid(&self) -> Result<Uuid> {
        let bytes = format!(
            "android-display-{}-{}-{}",
            f32::from(self.bounds.size.width) as u32,
            f32::from(self.bounds.size.height) as u32,
            (self.scale_factor * 100.0) as u32
        );
        Ok(Uuid::new_v5(&Uuid::NAMESPACE_OID, bytes.as_bytes()))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }
}
