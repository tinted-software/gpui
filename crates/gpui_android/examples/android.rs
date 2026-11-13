//! Native Android smoke application for GPUI.

#![cfg_attr(target_os = "android", no_main)]

#[cfg(target_os = "android")]
use gpui::{Context, Window, WindowOptions, div, prelude::*, px, rgb};

#[cfg(target_os = "android")]
struct AndroidExample {
    tap_count: usize,
}

#[cfg(target_os = "android")]
impl Render for AndroidExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(rgb(0x101622))
            .text_color(rgb(0xf5f7ff))
            .child(div().text_3xl().child("GPUI on Android"))
            .child(
                div()
                    .id("tap-counter")
                    .px_6()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(0x246bfd))
                    .text_xl()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tap_count += 1;
                        cx.notify();
                    }))
                    .child(format!("Taps: {}", self.tap_count)),
            )
            .child(
                div()
                    .max_w(px(320.))
                    .text_center()
                    .child("Rust app, native AndroidActivity loop, GPUI wgpu rendering"),
            )
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: gpui_android::AndroidApp) {
    gpui_android::run_android_app(app, || {
        gpui_platform::application().run(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| AndroidExample { tap_count: 0 })
            })
            .unwrap();
            cx.activate(true);
        });
    });
}

#[cfg(not(target_os = "android"))]
fn main() {
    eprintln!("Build this example for an Android target (e.g. aarch64-linux-android).");
}
