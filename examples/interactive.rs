//! Run with `cargo run -p egui_log_viewer --example interactive`.

use egui::{Color32, RichText, TextEdit};
use egui_log_viewer::LogViewer;

const LOG_EDITOR_BACKGROUND: Color32 = Color32::from_rgb(12, 13, 16);
const LOG_SYNTAX_RED: Color32 = Color32::from_rgb(224, 108, 117);
const LOG_SYNTAX_GREEN: Color32 = Color32::from_rgb(152, 195, 121);
const LOG_SYNTAX_YELLOW: Color32 = Color32::from_rgb(229, 192, 123);
const LOG_SYNTAX_BLUE: Color32 = Color32::from_rgb(97, 175, 239);
const LOG_SYNTAX_PURPLE: Color32 = Color32::from_rgb(198, 120, 221);
const LOG_SYNTAX_CYAN: Color32 = Color32::from_rgb(86, 182, 194);
const LOG_SYNTAX_FOREGROUND: Color32 = Color32::from_rgb(171, 178, 191);
const LOG_SYNTAX_MUTED: Color32 = Color32::from_rgb(92, 99, 112);

fn log_ansi_theme() -> &'static egui_sgr::EguiAnsiTheme {
    static THEME: std::sync::OnceLock<egui_sgr::EguiAnsiTheme> = std::sync::OnceLock::new();
    THEME.get_or_init(|| {
        let mut theme = egui_sgr::EguiAnsiTheme::default();
        theme.default_foreground = LOG_SYNTAX_FOREGROUND;
        theme.default_format.color = LOG_SYNTAX_FOREGROUND;
        theme.default_background = LOG_EDITOR_BACKGROUND;
        theme.palette[..16].copy_from_slice(&[
            LOG_EDITOR_BACKGROUND,
            LOG_SYNTAX_RED,
            LOG_SYNTAX_GREEN,
            LOG_SYNTAX_YELLOW,
            LOG_SYNTAX_BLUE,
            LOG_SYNTAX_PURPLE,
            LOG_SYNTAX_CYAN,
            LOG_SYNTAX_FOREGROUND,
            LOG_SYNTAX_MUTED,
            LOG_SYNTAX_RED,
            LOG_SYNTAX_GREEN,
            LOG_SYNTAX_YELLOW,
            LOG_SYNTAX_BLUE,
            LOG_SYNTAX_PURPLE,
            LOG_SYNTAX_CYAN,
            Color32::WHITE,
        ]);
        theme
    })
}

#[derive(Clone, Copy)]
enum LogSource {
    Serve,
    Up,
    RootDaemon,
    Pid(u32),
}

impl LogSource {
    fn badge(self) -> (&'static str, Color32) {
        match self {
            Self::Serve => ("serve", LOG_SYNTAX_GREEN),
            Self::Up => ("up", LOG_SYNTAX_BLUE),
            Self::RootDaemon => ("daemon", LOG_SYNTAX_PURPLE),
            Self::Pid(_) => ("pid", LOG_SYNTAX_YELLOW),
        }
    }

    fn copy_label(self) -> String {
        match self {
            Self::Serve => "serve".to_owned(),
            Self::Up => "up".to_owned(),
            Self::RootDaemon => "daemon".to_owned(),
            Self::Pid(pid) => format!("pid({pid})"),
        }
    }
}

struct LogEntry {
    source: LogSource,
    level: &'static str,
    target: &'static str,
    message: &'static str,
    fields: Vec<(&'static str, &'static str)>,
    connection_id: Option<&'static str>,
}

impl LogEntry {
    fn new(
        source: LogSource,
        level: &'static str,
        target: &'static str,
        message: &'static str,
    ) -> Self {
        Self {
            source,
            level,
            target,
            message,
            fields: Vec::new(),
            connection_id: None,
        }
    }

    fn field(mut self, name: &'static str, value: &'static str) -> Self {
        self.fields.push((name, value));
        self
    }

    fn connection(mut self, id: &'static str) -> Self {
        self.connection_id = Some(id);
        self
    }

    fn plain_text(&self) -> String {
        let message = egui_sgr::ansi_to_layout_job(self.message, log_ansi_theme()).text;
        let mut text = format!(
            "{} {} [{}] {}",
            self.source.badge().0,
            self.level,
            self.target,
            message
        );
        for (name, value) in &self.fields {
            text.push_str(&format!("\n{name}={value}"));
        }
        text
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        query.is_empty()
            || [
                Some(self.source.copy_label()),
                Some(self.level.to_owned()),
                Some(self.target.to_owned()),
                Some(egui_sgr::ansi_to_layout_job(self.message, log_ansi_theme()).text),
                self.connection_id.map(str::to_owned),
            ]
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(&query))
            || self.fields.iter().any(|(name, value)| {
                name.to_lowercase().contains(&query) || value.to_lowercase().contains(&query)
            })
    }
}

fn level_color(level: &str) -> Color32 {
    match level {
        "ERROR" => LOG_SYNTAX_RED,
        "WARN" => LOG_SYNTAX_YELLOW,
        "INFO" => LOG_SYNTAX_CYAN,
        "DEBUG" => LOG_SYNTAX_GREEN,
        "TRACE" => LOG_SYNTAX_MUTED,
        _ => LOG_SYNTAX_FOREGROUND,
    }
}

struct DemoApp {
    rows: Vec<LogEntry>,
    plain_text: String,
    query: String,
    show_fields: bool,
    last_action: Option<String>,
}

impl DemoApp {
    fn new() -> Self {
        let rows = sample_logs();
        let plain_text = rows
            .iter()
            .map(LogEntry::plain_text)
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            rows,
            plain_text,
            query: String::new(),
            show_fields: true,
            last_action: None,
        }
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        egui::Panel::top("demo_header").show(ui, |ui| {
            ui.vertical(|ui| {
                ui.heading(RichText::new("egui_log_viewer").size(24.0).strong());
                ui.label(
                    RichText::new(
                        "nsproxy-style source badges, levels, targets, ANSI messages, and fields.",
                    )
                    .color(LOG_SYNTAX_MUTED),
                );
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FILTER").strong());
                    ui.add(
                        TextEdit::singleline(&mut self.query)
                            .hint_text("Search source, target, message, or field…")
                            .desired_width(360.0),
                    );
                    ui.checkbox(&mut self.show_fields, "Fields");
                    if ui.button("Clear").clicked() {
                        self.query.clear();
                    }
                });
            });
            ui.add_space(12.0);

            let query = self.query.trim();
            let visible_rows: Vec<&LogEntry> =
                self.rows.iter().filter(|row| row.matches(query)).collect();

            ui.columns(2, |columns| {
                let (left, right) = columns.split_at_mut(1);
                let log_ui = &mut left[0];
                let text_ui = &mut right[0];

                log_ui.group(|log_ui| {
                    log_ui.horizontal(|ui| {
                        ui.heading("Structured events");
                        ui.label(
                            RichText::new(format!(
                                "{} visible · {} total",
                                visible_rows.len(),
                                self.rows.len()
                            ))
                            .color(LOG_SYNTAX_FOREGROUND),
                        );
                    });
                    log_ui.label(
                        RichText::new(
                            "Drag or Shift-click rows; Ctrl+C copies the selection as plain text.",
                        )
                        .size(14.0)
                        .color(LOG_SYNTAX_MUTED),
                    );
                    log_ui.add_space(8.0);

                    let mut row_action = None;
                    let row_height = log_ui.text_style_height(&egui::TextStyle::Body);
                    let output = egui::Frame::NONE
                        .fill(LOG_EDITOR_BACKGROUND)
                        .inner_margin(egui::Margin::same(6))
                        .show(log_ui, |log_ui| {
                            LogViewer::show_rows(
                                log_ui,
                                "interactive-example",
                                &visible_rows,
                                row_height,
                                620.0,
                                |entry| entry.plain_text(),
                                |ui, _, row, _selected| {
                                    ui.horizontal_wrapped(|ui| {
                                        let (source, source_color) = row.source.badge();
                                        ui.colored_label(source_color, source);
                                        ui.colored_label(level_color(row.level), row.level);
                                        ui.colored_label(
                                            LOG_SYNTAX_BLUE,
                                            format!("[{}]", row.target),
                                        );
                                        ui.label(egui_sgr::ansi_to_layout_job(
                                            row.message,
                                            log_ansi_theme(),
                                        ));

                                        if self.show_fields {
                                            for (name, value) in &row.fields {
                                                ui.colored_label(
                                                    LOG_SYNTAX_CYAN,
                                                    format!("{name}="),
                                                );
                                                ui.label(
                                                    RichText::new(*value)
                                                        .monospace()
                                                        .color(LOG_SYNTAX_GREEN),
                                                );
                                            }
                                        }

                                        if let Some(connection_id) = row.connection_id {
                                            if ui.button("Copy ID").clicked() {
                                                ui.ctx().copy_text(connection_id.to_owned());
                                                row_action = Some(format!(
                                                    "Copied connection {connection_id}"
                                                ));
                                            }
                                            if ui.button("Inspect").clicked() {
                                                row_action = Some(format!(
                                                    "Inspecting connection {connection_id}"
                                                ));
                                            }
                                        }
                                    });
                                },
                            )
                        })
                        .inner;
                    if let Some(action) = row_action {
                        self.last_action = Some(action);
                    }

                    let selection_status = output
                        .selected_rows
                        .as_ref()
                        .map(|range| {
                            format!(
                                "Rows {}–{} selected · Ctrl+C copies",
                                range.start() + 1,
                                range.end() + 1
                            )
                        })
                        .unwrap_or_else(|| "No row selection".to_owned());
                    log_ui.label(
                        RichText::new(selection_status)
                            .size(14.0)
                            .color(LOG_SYNTAX_MUTED),
                    );
                });

                text_ui.group(|text_ui| {
                    text_ui.heading("Plain text editor");
                    text_ui.label(
                        RichText::new("The same records flattened for character-level selection.")
                            .size(14.0)
                            .color(LOG_SYNTAX_MUTED),
                    );
                    text_ui.add_space(8.0);
                    text_ui.add(
                        TextEdit::multiline(&mut self.plain_text)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .desired_rows(28)
                            .desired_width(f32::INFINITY),
                    );
                });
            });

            if let Some(action) = self.last_action.as_deref() {
                ui.add_space(6.0);
                ui.label(RichText::new(action).color(LOG_SYNTAX_GREEN));
            }
        });
    }
}

impl Default for DemoApp {
    fn default() -> Self {
        Self::new()
    }
}

fn sample_logs() -> Vec<LogEntry> {
    use LogSource::{Serve, Up};

    let entry = |source, level, target, message| LogEntry::new(source, level, target, message);

    vec![
        entry(
            Up,
            "WARN",
            "supervisor",
            "sp up error: failed to connect to \x1b[36m/nsp3/basic/up.sock\x1b[0m: up daemon protocol version mismatch: build hash mismatch: \x1b[36mlocal\x1b[0m=\x1b[33m2fea16b279d12c0a9d944fe0e830d63e4557800f0862a5f3c9a8f74a79a57005\x1b[0m, \x1b[36mremote\x1b[0m=\x1b[33m73fe6b54f8071293056f87684c8aac5ffa22e550842434a0e170b460fa993cd4\x1b[0m",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_ipstack",
            "stream dead, recreating \x1b[35mNetworkTuple\x1b[0m { \x1b[36msrc\x1b[0m: \x1b[32m198.18.0.1:38240\x1b[0m, \x1b[36mdst\x1b[0m: \x1b[32m100.68.0.1:53\x1b[0m, \x1b[36mtcp\x1b[0m: \x1b[33mfalse\x1b[0m }",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::proxy_adapters",
            "\x1b[35mSOCKS5\x1b[0m TCP connection to proxy \x1b[32m127.0.0.1:9909\x1b[0m",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_ipstack",
            "stream dead, recreating \x1b[35mNetworkTuple\x1b[0m { \x1b[36msrc\x1b[0m: \x1b[32m198.18.0.1:53088\x1b[0m, \x1b[36mdst\x1b[0m: \x1b[32m100.68.0.1:53\x1b[0m, \x1b[36mtcp\x1b[0m: \x1b[33mfalse\x1b[0m }",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::proxy_adapters",
            "\x1b[35mSOCKS5\x1b[0m TCP connection to proxy \x1b[32m127.0.0.1:9909\x1b[0m",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::router",
            "proxy \x1b[33mbdcbc740\x1b[0m relay failed for \x1b[32m198.18.0.15:443\x1b[0m: \x1b[35mIo\x1b[0m { flow: \x1b[32m\"proxy->tun\"\x1b[0m, from: Right, to: Left, op: Copy, source: Custom { kind: \x1b[31mBrokenPipe\x1b[0m, error: \x1b[32m\"TCP connection closed\"\x1b[0m } }, retrying",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_core::uplink::router",
            "\x1b[32m198.18.0.15:443\x1b[0m: \x1b[31mMaxRetry\x1b[0m",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::state_blueprint",
            "load_state_read \x1b[32m\"/nsp3/uplink/stats.json\"\x1b[0m",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::state_blueprint",
            "save_state \x1b[32m\"/nsp3/uplink/stats.json\"\x1b[0m",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_ipstack",
            "stream dead, recreating \x1b[35mNetworkTuple\x1b[0m { \x1b[36msrc\x1b[0m: \x1b[32m198.18.0.1:36164\x1b[0m, \x1b[36mdst\x1b[0m: \x1b[32m100.68.0.1:53\x1b[0m, \x1b[36mtcp\x1b[0m: \x1b[33mfalse\x1b[0m }",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::proxy_adapters",
            "\x1b[35mSOCKS5\x1b[0m TCP connection to proxy \x1b[32m127.0.0.1:9909\x1b[0m",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::router",
            "proxy \x1b[33mbdcbc740\x1b[0m relay failed for \x1b[32m198.18.0.15:443\x1b[0m: \x1b[35mIo\x1b[0m { flow: \x1b[32m\"proxy->tun\"\x1b[0m, from: Right, to: Left, op: Copy, source: Custom { kind: \x1b[31mBrokenPipe\x1b[0m, error: \x1b[32m\"TCP connection closed\"\x1b[0m } }, retrying",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_core::uplink::router",
            "\x1b[32m198.18.0.15:443\x1b[0m: \x1b[31mMaxRetry\x1b[0m",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_ipstack",
            "stream dead, recreating \x1b[35mNetworkTuple\x1b[0m { \x1b[36msrc\x1b[0m: \x1b[32m198.18.0.1:60536\x1b[0m, \x1b[36mdst\x1b[0m: \x1b[32m100.68.0.1:53\x1b[0m, \x1b[36mtcp\x1b[0m: \x1b[33mfalse\x1b[0m }",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_ipstack",
            "stream dead, recreating \x1b[35mNetworkTuple\x1b[0m { \x1b[36msrc\x1b[0m: \x1b[32m198.18.0.1:60536\x1b[0m, \x1b[36mdst\x1b[0m: \x1b[32m100.68.0.1:53\x1b[0m, \x1b[36mtcp\x1b[0m: \x1b[33mfalse\x1b[0m }",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::proxy_adapters",
            "\x1b[35mSOCKS5\x1b[0m TCP connection to proxy \x1b[32m127.0.0.1:9909\x1b[0m",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::router",
            "proxy \x1b[33mbdcbc740\x1b[0m relay failed for \x1b[32m198.18.0.9:443\x1b[0m: \x1b[35mIo\x1b[0m { flow: \x1b[32m\"proxy->tun\"\x1b[0m, from: Right, to: Left, op: Copy, source: Custom { kind: \x1b[31mBrokenPipe\x1b[0m, error: \x1b[32m\"TCP connection closed\"\x1b[0m } }, retrying",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_core::uplink::router",
            "\x1b[32m198.18.0.9:443\x1b[0m: \x1b[31mMaxRetry\x1b[0m",
        ),
        entry(
            Serve,
            "INFO",
            "nsproxy_core::uplink::proxy_adapters",
            "\x1b[35mSOCKS5\x1b[0m TCP connection to proxy \x1b[32m127.0.0.1:9909\x1b[0m",
        ),
        entry(
            Serve,
            "WARN",
            "nsproxy_ipstack",
            "stream dead, recreating \x1b[35mNetworkTuple\x1b[0m { \x1b[36msrc\x1b[0m: \x1b[32m198.18.0.1:33454\x1b[0m, \x1b[36mdst\x1b[0m: \x1b[32m100.68.0.1:53\x1b[0m, \x1b[36mtcp\x1b[0m: \x1b[33mfalse\x1b[0m }",
        ),
    ]
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1000.0, 680.0]),
        ..Default::default()
    };

    let mut app = DemoApp::default();
    let mut set_initial_zoom = true;
    eframe::run_ui_native("egui_log_viewer example", options, move |ui, _frame| {
        if set_initial_zoom {
            ui.ctx().set_zoom_factor(1.);
            set_initial_zoom = false;
        }
        app.show(ui);
    })
}
