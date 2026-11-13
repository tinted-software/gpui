//! Android Window implementation using `NativeWindow` and `WgpuRenderer`.

use super::{
    AndroidDisplay, CallbackSlot, dispatcher::AndroidDispatcher, platform::AndroidPlatformState,
};
use android_activity::AndroidApp;
use anyhow::anyhow;
use gpui::{
    AnyWindowHandle, Bounds, Capslock, DevicePixels, DispatchEventResult, Edges, GpuSpecs,
    Modifiers, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler,
    PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions, Scene, Size,
    TextInputStateChange, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
    WindowControlArea, WindowInsets, WindowParams, px, size,
};
use gpui_wgpu::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};
use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    rc::{Rc, Weak},
    sync::Arc,
};

#[derive(Clone, Debug)]
struct RawAndroidWindow(NativeWindow);

// Safety: `NativeWindow` wraps an `ANativeWindow*` with reference counting (`ANativeWindow_acquire`/`release`).
unsafe impl Send for RawAndroidWindow {}
unsafe impl Sync for RawAndroidWindow {}

impl HasWindowHandle for RawAndroidWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ptr = NonNull::new(self.0.ptr().as_ptr().cast()).ok_or(HandleError::Unavailable)?;
        let handle = AndroidNdkWindowHandle::new(ptr);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(handle)) })
    }
}

impl HasDisplayHandle for RawAndroidWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}

pub(crate) struct AndroidWindowState {
    handle: AnyWindowHandle,
    app: Option<AndroidApp>,
    dispatcher: Arc<AndroidDispatcher>,
    gpu_context: GpuContext,
    renderer: RefCell<Option<WgpuRenderer>>,
    native_window: RefCell<Option<NativeWindow>>,
    display: RefCell<AndroidDisplay>,
    scale_factor: Cell<f32>,
    bounds: Cell<Bounds<Pixels>>,
    insets: RefCell<WindowInsets>,
    appearance: Cell<WindowAppearance>,
    active: Cell<bool>,
    visible: Cell<bool>,
    drew_this_frame: Cell<bool>,
    input_handler: RefCell<Option<PlatformInputHandler>>,
    request_frame: CallbackSlot<Box<dyn FnMut(RequestFrameOptions)>>,
    input: CallbackSlot<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active_status_change: CallbackSlot<Box<dyn FnMut(bool)>>,
    hover_status_change: CallbackSlot<Box<dyn FnMut(bool)>>,
    resize: CallbackSlot<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: CallbackSlot<Box<dyn FnMut()>>,
    display_changed: CallbackSlot<Box<dyn FnMut()>>,
    should_close: CallbackSlot<Box<dyn FnMut() -> bool>>,
    close: CallbackSlot<Box<dyn FnOnce()>>,
    appearance_changed: CallbackSlot<Box<dyn FnMut()>>,
    insets_changed: CallbackSlot<Box<dyn FnMut(WindowInsets)>>,
    platform: Weak<AndroidPlatformState>,
}

pub(crate) struct AndroidWindow(Rc<AndroidWindowState>);

impl AndroidWindow {
    pub(crate) fn new(
        handle: AnyWindowHandle,
        _options: WindowParams,
        platform: &Rc<AndroidPlatformState>,
    ) -> anyhow::Result<Self> {
        let app = platform.application.android_app();
        let display = AndroidDisplay::from_app(app.as_ref());
        let scale_factor = display.scale_factor();
        let bounds = display.bounds();
        let appearance = platform.window_appearance_from_app();

        let native_window = app.as_ref().and_then(|app| app.native_window());
        let renderer = if let Some(ref native_window) = native_window {
            Some(Self::create_renderer(&platform.gpu_context, native_window)?)
        } else {
            return Err(anyhow!(
                "Cannot open GPUI Android window before NativeWindow is initialized"
            ));
        };

        let insets = app
            .as_ref()
            .zip(native_window.as_ref())
            .map(|(app, nw)| compute_insets(app, nw, scale_factor))
            .unwrap_or_default();

        let state = Rc::new(AndroidWindowState {
            handle,
            app,
            dispatcher: platform.dispatcher.clone(),
            gpu_context: platform.gpu_context.clone(),
            renderer: RefCell::new(renderer),
            native_window: RefCell::new(native_window),
            display: RefCell::new(display),
            scale_factor: Cell::new(scale_factor),
            bounds: Cell::new(bounds),
            insets: RefCell::new(insets),
            appearance: Cell::new(appearance),
            active: Cell::new(true),
            visible: Cell::new(true),
            drew_this_frame: Cell::new(false),
            input_handler: RefCell::new(None),
            request_frame: CallbackSlot::default(),
            input: CallbackSlot::default(),
            active_status_change: CallbackSlot::default(),
            hover_status_change: CallbackSlot::default(),
            resize: CallbackSlot::default(),
            moved: CallbackSlot::default(),
            display_changed: CallbackSlot::default(),
            should_close: CallbackSlot::default(),
            close: CallbackSlot::default(),
            appearance_changed: CallbackSlot::default(),
            insets_changed: CallbackSlot::default(),
            platform: Rc::downgrade(platform),
        });

        Ok(Self(state))
    }

    fn create_renderer(
        gpu_context: &GpuContext,
        native_window: &NativeWindow,
    ) -> anyhow::Result<WgpuRenderer> {
        let raw_window = RawAndroidWindow(native_window.clone());
        let config = WgpuSurfaceConfig {
            size: size(
                DevicePixels(native_window.width().max(1)),
                DevicePixels(native_window.height().max(1)),
            ),
            transparent: false,
            preferred_present_mode: Some(gpui_wgpu::wgpu::PresentMode::Mailbox),
        };
        WgpuRenderer::new(gpu_context.clone(), &raw_window, config, None, None)
    }

    pub(crate) fn register(&self) {
        if let Some(platform) = self.0.platform.upgrade() {
            platform.application.register_window(&self.0);
        }
    }
}

impl Drop for AndroidWindow {
    fn drop(&mut self) {
        if let Some(platform) = self.0.platform.upgrade() {
            platform.application.unregister_window(&self.0);
        }
        if let Some(callback) = self.0.close.take() {
            callback();
        }
        *self.0.renderer.borrow_mut() = None;
        *self.0.native_window.borrow_mut() = None;
    }
}

impl AndroidWindowState {
    pub(crate) fn handle(&self) -> AnyWindowHandle {
        self.handle
    }

    pub(crate) fn attach_native_window(&self, native_window: NativeWindow) {
        let raw_window = RawAndroidWindow(native_window.clone());
        let width = native_window.width().max(1);
        let height = native_window.height().max(1);
        let config = WgpuSurfaceConfig {
            size: size(DevicePixels(width), DevicePixels(height)),
            transparent: false,
            preferred_present_mode: Some(gpui_wgpu::wgpu::PresentMode::Mailbox),
        };

        let mut renderer_ref = self.renderer.borrow_mut();
        if let Some(renderer) = renderer_ref.as_mut() {
            let instance = self
                .gpu_context
                .borrow()
                .as_ref()
                .map(|ctx| ctx.instance.clone());
            if let Some(instance) = instance {
                if let Err(err) = renderer.replace_surface(&raw_window, config, &instance) {
                    log::error!("Failed to replace Android wgpu surface: {err:?}");
                }
            }
        } else {
            match WgpuRenderer::new(self.gpu_context.clone(), &raw_window, config, None, None) {
                Ok(renderer) => *renderer_ref = Some(renderer),
                Err(err) => log::error!("Failed to create Android wgpu renderer: {err:?}"),
            }
        }
        drop(renderer_ref);

        *self.native_window.borrow_mut() = Some(native_window);
        self.update_metrics();
        self.request_frame_async();
    }

    pub(crate) fn detach_native_window(&self) {
        if let Some(renderer) = self.renderer.borrow_mut().as_mut() {
            renderer.unconfigure_surface();
        }
        *self.native_window.borrow_mut() = None;
    }

    pub(crate) fn update_metrics(&self) {
        let display = AndroidDisplay::from_app(self.app.as_ref());
        let scale_factor = display.scale_factor();
        *self.display.borrow_mut() = display;
        self.scale_factor.set(scale_factor);

        let Some(native_window) = self.native_window.borrow().clone() else {
            return;
        };

        let physical_width = native_window.width().max(1);
        let physical_height = native_window.height().max(1);
        let logical_size = size(
            px(physical_width as f32 / scale_factor),
            px(physical_height as f32 / scale_factor),
        );
        let new_bounds = Bounds {
            origin: Default::default(),
            size: logical_size,
        };

        if let Some(renderer) = self.renderer.borrow_mut().as_mut() {
            renderer.update_drawable_size(size(
                DevicePixels(physical_width),
                DevicePixels(physical_height),
            ));
        }

        if self.bounds.get() != new_bounds {
            self.bounds.set(new_bounds);
            self.resize
                .with(|callback| callback(logical_size, scale_factor));
        }

        if let Some(app) = self.app.as_ref() {
            let new_insets = compute_insets(app, &native_window, scale_factor);
            if *self.insets.borrow() != new_insets {
                *self.insets.borrow_mut() = new_insets.clone();
                self.insets_changed.with(|callback| callback(new_insets));
            }
        }
    }

    pub(crate) fn did_draw_frame(&self) -> bool {
        self.drew_this_frame.get() && self.native_window.borrow().is_some()
    }

    pub(crate) fn request_frame(&self) {
        if self.native_window.borrow().is_none() {
            return;
        }
        self.drew_this_frame.set(false);
        self.request_frame
            .with(|callback| callback(RequestFrameOptions::default()));
    }

    fn request_frame_async(&self) {
        self.drew_this_frame.set(true);
        self.dispatcher.wake();
    }

    pub(crate) fn deliver_input(&self, input: PlatformInput) -> DispatchEventResult {
        self.input
            .with(|callback| callback(input))
            .unwrap_or_default()
    }

    pub(crate) fn notify_active_status_change(&self, active: bool) {
        self.active.set(active);
        self.active_status_change.with(|callback| callback(active));
    }

    pub(crate) fn notify_appearance_changed(&self, appearance: WindowAppearance) {
        if self.appearance.get() != appearance {
            self.appearance.set(appearance);
            self.appearance_changed.with(|callback| callback());
        }
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        self.scale_factor.get()
    }
}

fn compute_insets(
    app: &AndroidApp,
    native_window: &NativeWindow,
    scale_factor: f32,
) -> WindowInsets {
    let rect = app.content_rect();
    let width = native_window.width().max(1);
    let height = native_window.height().max(1);
    let scale = scale_factor.max(1.0);

    // If content_rect hasn't been populated yet (all zeros), insets are zero.
    if rect.left == 0 && rect.top == 0 && rect.right == 0 && rect.bottom == 0 {
        return WindowInsets::default();
    }

    WindowInsets {
        safe_area: Edges {
            top: px(rect.top.max(0) as f32 / scale),
            left: px(rect.left.max(0) as f32 / scale),
            bottom: px((height - rect.bottom).max(0) as f32 / scale),
            right: px((width - rect.right).max(0) as f32 / scale),
        },
        ..Default::default()
    }
}

impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let borrow = self.0.native_window.borrow();
        let Some(native_window) = borrow.as_ref() else {
            return Err(HandleError::Unavailable);
        };
        let ptr =
            NonNull::new(native_window.ptr().as_ptr().cast()).ok_or(HandleError::Unavailable)?;
        let handle = AndroidNdkWindowHandle::new(ptr);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(handle)) })
    }
}

impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}

impl PlatformWindow for AndroidWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.bounds.get()
    }

    fn is_maximized(&self) -> bool {
        true
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.0.bounds.get())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.bounds.get().size
    }

    fn resize(&mut self, _size: Size<Pixels>) {
        // Android windows are managed by the system and cannot be resized programmatically.
    }

    fn scale_factor(&self) -> f32 {
        self.0.scale_factor.get()
    }

    fn appearance(&self) -> WindowAppearance {
        self.0.appearance.get()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(self.0.display.borrow().clone()))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    fn modifiers(&self) -> Modifiers {
        Modifiers::default()
    }

    fn capslock(&self) -> Capslock {
        Capslock::default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        *self.0.input_handler.borrow_mut() = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.input_handler.borrow_mut().take()
    }

    fn text_input_state_changed(&self, change: TextInputStateChange) {
        let Some(app) = self.0.app.as_ref() else {
            return;
        };
        match change {
            TextInputStateChange::FocusGained => {
                app.show_soft_input(false);
            }
            TextInputStateChange::FocusLost => {
                app.hide_soft_input(false);
            }
            TextInputStateChange::SelectionChanged | TextInputStateChange::ContentChanged => {}
        }
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self, _activation_token: Option<&str>) -> bool {
        self.0.notify_active_status_change(true);
        true
    }

    fn is_active(&self) -> bool {
        self.0.active.get()
    }

    fn is_hovered(&self) -> bool {
        false
    }

    fn set_title(&mut self, _title: &str) {}

    fn set_background_appearance(&self, _background_appearance: WindowBackgroundAppearance) {}

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {}

    fn is_fullscreen(&self) -> bool {
        true
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn set_edited(&mut self, _edited: bool) {}

    fn show_character_palette(&self) {}

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.request_frame.set(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.input.set(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.active_status_change.set(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.hover_status_change.set(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.resize.set(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.moved.set(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.should_close.set(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.close.set(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.appearance_changed.set(callback);
    }

    fn insets(&self) -> WindowInsets {
        self.0.insets.borrow().clone()
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(WindowInsets)>) {
        self.0.insets_changed.set(callback);
    }

    fn draw(&self, scene: &Scene) {
        if let Some(renderer) = self.0.renderer.borrow_mut().as_mut() {
            if renderer.draw(scene) {
                self.0.drew_this_frame.set(true);
            }
        }
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0
            .renderer
            .borrow()
            .as_ref()
            .expect("Android window renderer not initialized")
            .sprite_atlas()
            .clone()
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.0
            .renderer
            .borrow()
            .as_ref()
            .and_then(|renderer| Some(renderer.gpu_specs()))
    }
}
