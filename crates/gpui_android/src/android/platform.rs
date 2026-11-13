//! Android Platform implementation.

use super::{
    AndroidDisplay, AndroidWindow,
    application::{AndroidApplicationState, current_android_app},
    dispatcher::AndroidDispatcher,
};
use anyhow::anyhow;
use futures::channel::oneshot;
use gpui::{
    Action, AnyWindowHandle, AppLifecyclePhase, BackgroundExecutor, ClipboardItem, CursorStyle,
    DummyKeyboardMapper, ForegroundExecutor, Keymap, Menu, MenuItem, PathPromptOptions, Platform,
    PlatformDisplay, PlatformKeyboardLayout, PlatformKeyboardMapper, PlatformTextSystem,
    PlatformWindow, Result, Task, ThermalState, WindowAppearance, WindowParams,
};
use gpui_parley::ParleyTextSystem;
use gpui_render::SwashGlyphRasterizer;
use gpui_wgpu::GpuContext;
use ndk::configuration::UiModeNight;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

pub struct AndroidPlatform(Rc<AndroidPlatformState>);

pub(crate) struct AndroidPlatformState {
    pub(super) application: AndroidApplicationState,
    pub(super) dispatcher: Arc<AndroidDispatcher>,
    pub(super) gpu_context: GpuContext,
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
}

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidPlatform {
    pub fn new() -> Self {
        let app = current_android_app();
        let waker = app.as_ref().map(|app| app.create_waker());
        Self::with_app_and_waker(app, waker)
    }

    pub fn new_headless() -> Self {
        Self::with_app_and_waker(None, None)
    }

    fn with_app_and_waker(
        app: Option<android_activity::AndroidApp>,
        waker: Option<android_activity::AndroidAppWaker>,
    ) -> Self {
        let dispatcher = Arc::new(AndroidDispatcher::new(waker));
        let text_system: Arc<dyn PlatformTextSystem> = Arc::new(
            ParleyTextSystem::new(SwashGlyphRasterizer::default())
                .with_system_fallback_family("Roboto"),
        );
        let state = Rc::new(AndroidPlatformState {
            application: AndroidApplicationState::new(app),
            dispatcher: dispatcher.clone(),
            gpu_context: Rc::new(RefCell::new(None)),
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
        });
        Self(state)
    }
}

impl AndroidPlatformState {
    pub(super) fn window_appearance_from_app(&self) -> WindowAppearance {
        let Some(app) = self.application.android_app() else {
            return WindowAppearance::Light;
        };
        match app.config().ui_mode_night() {
            UiModeNight::Yes => WindowAppearance::Dark,
            _ => WindowAppearance::Light,
        }
    }
}

struct AndroidKeyboardLayout;

impl PlatformKeyboardLayout for AndroidKeyboardLayout {
    fn id(&self) -> &str {
        "android-default"
    }

    fn name(&self) -> &str {
        "Android Default"
    }
}

impl Platform for AndroidPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.0.background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.0.foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        super::application::run(&self.0, on_finish_launching);
    }

    fn quit(&self) {
        self.0.application.request_quit();
        self.0.dispatcher.wake();
    }

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {
        log::warn!("Android apps cannot restart themselves");
    }

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![Rc::new(AndroidDisplay::from_app(
            self.0.application.android_app().as_ref(),
        ))]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(AndroidDisplay::from_app(
            self.0.application.android_app().as_ref(),
        )))
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        let mut active = None;
        self.0.application.with_windows(|window| {
            if active.is_none() {
                active = Some(window.handle());
            }
        });
        active
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        let window = Box::new(AndroidWindow::new(handle, options, &self.0)?);
        window.register();
        Ok(window)
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.0.window_appearance_from_app()
    }

    fn open_url(&self, url: &str) {
        log::warn!("open_url is not yet implemented on Android: {url}");
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.0.application.open_urls.set(callback);
    }

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        if tx
            .send(Err(anyhow!("File picker not yet implemented for Android")))
            .is_err()
        {
            log::debug!("GPUI Android: File picker receiver was dropped");
        }
        rx
    }

    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        if tx
            .send(Err(anyhow!("Save dialog not yet implemented for Android")))
            .is_err()
        {
            log::debug!("GPUI Android: Save dialog receiver was dropped");
        }
        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, _path: &Path) {}

    fn open_with_system(&self, _path: &Path) {}

    fn on_quit(&self, mut callback: Box<dyn FnMut() -> bool>) {
        self.0.application.quit.set(Box::new(move || {
            callback();
        }));
    }

    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        self.0.application.app_lifecycle.set(callback);
    }

    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        self.0.application.memory_warning.set(callback);
    }

    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn app_path(&self) -> Result<PathBuf> {
        self.0
            .application
            .android_app()
            .and_then(|app| app.internal_data_path())
            .ok_or_else(|| anyhow!("Android internal data path is unavailable"))
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        Ok(self.app_path()?.join(name))
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn hide_cursor_until_mouse_moves(&self) {}

    fn is_cursor_visible(&self) -> bool {
        false
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn write_to_clipboard(&self, _item: ClipboardItem) {}

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        None
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "Credential storage is not implemented for Android"
        )))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Err(anyhow!(
            "Credential storage is not implemented for Android"
        )))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "Credential storage is not implemented for Android"
        )))
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}

    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }

    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(AndroidKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }
}
