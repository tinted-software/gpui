use super::{
    CallbackSlot, events::convert_input_event, platform::AndroidPlatformState,
    window::AndroidWindowState,
};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use gpui::AppLifecyclePhase;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

static CURRENT_ANDROID_APP: parking_lot::Mutex<Option<AndroidApp>> = parking_lot::Mutex::new(None);

/// Registers the [`AndroidApp`] handle so that [`gpui_platform::application()`] /
/// [`crate::current_platform`] can drive the Android `NativeActivity` event loop.
pub fn init_android_app(app: AndroidApp) {
    *CURRENT_ANDROID_APP.lock() = Some(app);
}

/// Initializes the Android platform handle and runs a GPUI application entry point.
pub fn run_android_app(app: AndroidApp, entry: impl FnOnce()) {
    init_android_app(app);
    entry();
}

pub(super) fn current_android_app() -> Option<AndroidApp> {
    CURRENT_ANDROID_APP.lock().clone()
}

#[derive(Default)]
pub(super) struct AndroidApplicationState {
    app: RefCell<Option<AndroidApp>>,
    running: Cell<bool>,
    should_quit: Cell<bool>,
    finish_launching: CallbackSlot<Box<dyn FnOnce()>>,
    pub(super) quit: CallbackSlot<Box<dyn FnMut()>>,
    pub(super) open_urls: CallbackSlot<Box<dyn FnMut(Vec<String>)>>,
    pub(super) app_lifecycle: CallbackSlot<Box<dyn FnMut(AppLifecyclePhase)>>,
    pub(super) memory_warning: CallbackSlot<Box<dyn FnMut()>>,
    pub(super) displays_changed: CallbackSlot<Box<dyn FnMut()>>,
    windows: RefCell<Vec<Weak<AndroidWindowState>>>,
}

impl AndroidApplicationState {
    pub(super) fn new(app: Option<AndroidApp>) -> Self {
        Self {
            app: RefCell::new(app),
            ..Default::default()
        }
    }

    pub(super) fn android_app(&self) -> Option<AndroidApp> {
        self.app.borrow().clone()
    }

    pub(super) fn register_window(&self, window: &Rc<AndroidWindowState>) {
        self.windows.borrow_mut().push(Rc::downgrade(window));
    }

    pub(super) fn unregister_window(&self, window: &Rc<AndroidWindowState>) {
        let window = Rc::downgrade(window);
        self.windows
            .borrow_mut()
            .retain(|entry| !entry.ptr_eq(&window));
    }

    pub(super) fn with_windows(&self, mut callback: impl FnMut(&AndroidWindowState)) {
        // Native callbacks may open or close windows. Do not hold a registry borrow
        // across them, and skip snapshot entries that have since been unregistered.
        let windows = self.windows.borrow().clone();
        for window in windows {
            if self
                .windows
                .borrow()
                .iter()
                .any(|entry| entry.ptr_eq(&window))
            {
                if let Some(window) = window.upgrade() {
                    callback(&window);
                }
            }
        }
    }

    pub(super) fn request_quit(&self) {
        self.should_quit.set(true);
    }

    fn finish_launching(&self) -> bool {
        let Some(callback) = self.finish_launching.take() else {
            return false;
        };
        callback();
        true
    }

    fn notify_memory_warning(&self) {
        self.memory_warning.with(|callback| callback());
    }

    fn notify_app_lifecycle(&self, phase: AppLifecyclePhase) {
        match phase {
            AppLifecyclePhase::Foreground => {}
            AppLifecyclePhase::Active => {
                self.with_windows(|window| window.notify_active_status_change(true));
            }
            AppLifecyclePhase::Inactive => {
                self.with_windows(|window| window.notify_active_status_change(false));
            }
            AppLifecyclePhase::Background => {
                self.with_windows(|window| window.notify_active_status_change(false));
            }
        }
        self.app_lifecycle.with(|callback| callback(phase));
    }
}

pub(super) fn run(platform: &Rc<AndroidPlatformState>, on_finish_launching: Box<dyn FnOnce()>) {
    let app = platform
        .application
        .android_app()
        .expect("init_android_app must be called before Application::run on Android");

    assert!(
        !platform.application.running.replace(true),
        "the Android application is already running"
    );

    platform
        .application
        .finish_launching
        .set(on_finish_launching);

    // If the NativeWindow is already available before entering the loop, launch immediately.
    if app.native_window().is_some() {
        platform.application.finish_launching();
    }

    while !platform.application.should_quit.get() {
        let mut any_drew_frame = false;
        platform.application.with_windows(|window| {
            if window.did_draw_frame() {
                any_drew_frame = true;
            }
        });
        let timeout = if any_drew_frame {
            Some(Duration::from_millis(16))
        } else {
            None
        };

        app.poll_events(timeout, |event| match event {
            PollEvent::Main(main_event) => match main_event {
                MainEvent::InitWindow { .. } => {
                    if let Some(native_window) = app.native_window() {
                        let launched = platform.application.finish_launching();
                        if !launched {
                            platform.application.with_windows(|window| {
                                window.attach_native_window(native_window.clone());
                            });
                        }
                    }
                }
                MainEvent::TerminateWindow { .. } => {
                    platform
                        .application
                        .with_windows(AndroidWindowState::detach_native_window);
                }
                MainEvent::WindowResized { .. } | MainEvent::ContentRectChanged { .. } => {
                    platform
                        .application
                        .with_windows(AndroidWindowState::update_metrics);
                    platform
                        .application
                        .displays_changed
                        .with(|callback| callback());
                }
                MainEvent::ConfigChanged { .. } => {
                    let appearance = platform.window_appearance_from_app();
                    platform.application.with_windows(|window| {
                        window.update_metrics();
                        window.notify_appearance_changed(appearance);
                    });
                    platform
                        .application
                        .displays_changed
                        .with(|callback| callback());
                }
                MainEvent::GainedFocus => {
                    platform
                        .application
                        .notify_app_lifecycle(AppLifecyclePhase::Active);
                }
                MainEvent::LostFocus => {
                    platform
                        .application
                        .notify_app_lifecycle(AppLifecyclePhase::Inactive);
                }
                MainEvent::Resume { .. } => {
                    platform
                        .application
                        .notify_app_lifecycle(AppLifecyclePhase::Foreground);
                }
                MainEvent::Pause => {
                    platform
                        .application
                        .notify_app_lifecycle(AppLifecyclePhase::Background);
                }
                MainEvent::LowMemory => {
                    platform.application.notify_memory_warning();
                }
                MainEvent::Destroy => {
                    platform
                        .application
                        .notify_app_lifecycle(AppLifecyclePhase::Background);
                    if let Some(mut callback) = platform.application.quit.take() {
                        callback();
                    }
                    platform.application.should_quit.set(true);
                }
                MainEvent::InputAvailable => {
                    drain_input_events(&app, platform);
                }
                _ => {}
            },
            PollEvent::Wake | PollEvent::Timeout => {}
            _ => {}
        });

        drain_input_events(&app, platform);

        while platform.dispatcher.run_ready_main_tasks() {}

        platform
            .application
            .with_windows(AndroidWindowState::request_frame);

        while platform.dispatcher.run_ready_main_tasks() {}
    }
}

fn drain_input_events(app: &AndroidApp, platform: &AndroidPlatformState) {
    let Ok(mut iter) = app.input_events_iter() else {
        return;
    };
    let mut scale_factor = 1.0_f32;
    platform.application.with_windows(|window| {
        scale_factor = window.scale_factor();
    });

    loop {
        if !iter.next(|event| {
            let key_map = match event {
                android_activity::input::InputEvent::KeyEvent(key_event) => {
                    app.device_key_character_map(key_event.device_id()).ok()
                }
                _ => None,
            };
            let inputs = convert_input_event(event, scale_factor, key_map.as_ref());
            if inputs.is_empty() {
                return InputStatus::Unhandled;
            }
            let mut handled = false;
            for input in inputs {
                platform.application.with_windows(|window| {
                    let result = window.deliver_input(input.clone());
                    if !result.propagate {
                        handled = true;
                    }
                });
            }
            if handled {
                InputStatus::Handled
            } else {
                InputStatus::Unhandled
            }
        }) {
            break;
        }
    }
}
