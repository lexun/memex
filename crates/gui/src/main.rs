//! Memex GUI - A desktop interface for the memex daemon
//!
//! Built with GPUI (Zed's GPU-accelerated UI framework)

use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window, WindowBounds,
    WindowOptions,
};

/// Daemon connection status
struct DaemonStatus {
    connected: bool,
    socket_path: PathBuf,
    version: Option<String>,
    pid: Option<u32>,
}

impl Default for DaemonStatus {
    fn default() -> Self {
        Self {
            connected: false,
            socket_path: default_socket_path(),
            version: None,
            pid: None,
        }
    }
}

fn default_socket_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/memex/memex.sock"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/memex.sock"))
}

/// Main application view
struct MemexApp {
    status: DaemonStatus,
}

impl Render for MemexApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let status_color = if self.status.connected {
            rgb(0x4ade80) // green
        } else {
            rgb(0xf87171) // red
        };

        let status_text = if self.status.connected {
            "Connected"
        } else {
            "Disconnected"
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e)) // dark background
            .text_color(rgb(0xcdd6f4)) // light text
            .child(
                // Header
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x313244))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Memex"),
                    )
                    .child(
                        // Status indicator
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(8.0)).rounded_full().bg(status_color))
                            .child(status_text),
                    ),
            )
            .child(
                // Main content
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .p_4()
                    .gap_4()
                    .child(
                        // Daemon status card
                        div()
                            .flex()
                            .flex_col()
                            .p_4()
                            .rounded_lg()
                            .bg(rgb(0x313244))
                            .gap_3()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xa6adc8))
                                    .child("Daemon Status"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(status_row(
                                        "Socket",
                                        &self.status.socket_path.display().to_string(),
                                    ))
                                    .child(status_row(
                                        "Status",
                                        if self.status.connected {
                                            "Running"
                                        } else {
                                            "Stopped"
                                        },
                                    ))
                                    .child(status_row(
                                        "PID",
                                        &self
                                            .status
                                            .pid
                                            .map(|p| p.to_string())
                                            .unwrap_or_else(|| "-".to_string()),
                                    ))
                                    .child(status_row(
                                        "Version",
                                        self.status.version.as_deref().unwrap_or("-"),
                                    )),
                            ),
                    ),
            )
    }
}

fn status_row(label: &str, value: &str) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .child(
            div()
                .text_sm()
                .text_color(rgb(0x6c7086))
                .child(label.to_string()),
        )
        .child(div().text_sm().child(value.to_string()))
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(400.), px(300.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| MemexApp {
                    status: DaemonStatus::default(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
