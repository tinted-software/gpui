//! Native Android platform support for GPUI.
//!
//! This crate connects GPUI to Android's `NativeActivity` (via `android-activity`)
//! and renders through `gpui_wgpu` (Vulkan/GLES) and `CosmicTextSystem`. It owns
//! Android application lifecycle, windowing, safe-area and keyboard insets,
//! soft-keyboard visibility, and touch/key input delivery.

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "android")]
pub use android::{
    AndroidApp, AndroidPlatform, current_platform, init_android_app, run_android_app,
};
