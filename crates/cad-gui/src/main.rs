#![forbid(unsafe_code)]
use cad_access::{Action as Startup, Config, HELP};
use cad_gui::bridge::Bridge;
use cad_ui_egui::{Action, Event, Failure, Workspace};
use std::{sync::Arc, time::Duration};
struct Desktop {
    workspace: Workspace,
    bridge: Bridge,
    scheduled: u64,
    probe_path: Option<std::path::PathBuf>,
    probe_previous: String,
}
impl Desktop {
    fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        Workspace::theme(&cc.egui_ctx);
        let mut workspace = Workspace::default();
        if let Some(state) = &cc.wgpu_render_state {
            cad_ui_egui::local_view::install(state);
            workspace.local_backend = Some(format!("{:?}", state.adapter.get_info().backend));
            let info = state.adapter.get_info();
            eprintln!(
                "cad-gui display: {:?} / {} / {:?}",
                info.backend, info.name, info.device_type
            );
        }
        let ctx = cc.egui_ctx.clone();
        let bridge = Bridge::start(config, Arc::new(move || ctx.request_repaint()));
        Self {
            workspace,
            bridge,
            scheduled: u64::MAX,
            probe_path: std::env::var_os("CAD_GUI_PROBE_FILE").map(Into::into),
            probe_previous: String::new(),
        }
    }
}
impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.bridge.events.try_recv() {
            if matches!(&event,Event::AgentReply{result:Ok(state),..} if state.done) {
                if let Some(active) = self.workspace.model.active.clone() {
                    let _ = self.bridge.send(Action::Select(active));
                }
            }
            self.workspace.apply(event, ctx);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut paths = Vec::new();
        let mut files = Vec::new();
        for file in dropped {
            if let Some(bytes) = file.bytes {
                match cad_agent::Attachment::from_bytes(file.name, &bytes) {
                    Ok(a) => files.push(a),
                    Err(error) => self.workspace.apply(Event::AttachmentError(error), ctx),
                }
            } else if let Some(path) = file.path {
                paths.push(path);
            }
        }
        if !files.is_empty() {
            self.workspace.apply(Event::Attachments(files), ctx);
        }
        if !paths.is_empty() {
            let _ = self.bridge.send(Action::LoadAttachments(paths));
        }
        for action in self.workspace.show(ctx) {
            let agent_id = match &action {
                Action::AskAgent { id, .. } | Action::UndoAgent { id, .. } => Some(*id),
                _ => None,
            };
            let (ticket, preview) = match &action {
                Action::Select(_)
                | Action::AskAgent { .. }
                | Action::UndoAgent { .. }
                | Action::ChooseAttachments
                | Action::LoadAttachments(_) => (None, false),
                Action::NewDocument { ticket }
                | Action::Refresh { ticket }
                | Action::Reconnect { ticket }
                | Action::Execute { ticket, .. }
                | Action::DownloadNative { ticket, .. } => (Some(*ticket), false),
                Action::Preview { ticket, .. } => (Some(*ticket), true),
            };
            if let Err(message) = self.bridge.send(action) {
                if let Some(id) = agent_id {
                    self.workspace.apply(
                        Event::AgentReply {
                            id,
                            result: Err(message),
                        },
                        ctx,
                    );
                    continue;
                }
                self.workspace.apply(
                    Event::Failed {
                        ticket,
                        preview,
                        failure: Failure {
                            kind: "queue_full".into(),
                            message,
                            request: None,
                        },
                    },
                    ctx,
                );
            }
        }
        if let Some(path) = &self.probe_path {
            if let Ok(value) = serde_json::to_string(&self.workspace.geometry) {
                if value != self.probe_previous {
                    let temporary = path.with_extension("tmp");
                    if std::fs::write(&temporary, &value)
                        .and_then(|_| std::fs::rename(&temporary, path))
                        .is_ok()
                    {
                        self.probe_previous = value;
                    }
                }
            }
        }
        if self.scheduled != self.workspace.model.view_epoch {
            self.scheduled = self.workspace.model.view_epoch;
            if let Some(request) = self.workspace.render_request() {
                if let Err(message) = self.bridge.render(request) {
                    self.workspace.apply(
                        Event::Failed {
                            ticket: None,
                            preview: false,
                            failure: Failure {
                                kind: "render_failed".into(),
                                message,
                                request: None,
                            },
                        },
                        ctx,
                    );
                }
            }
        }
        if self.workspace.model.busy || self.workspace.model.connecting {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}
use eframe::egui;
fn main() {
    if let Err(error) = run() {
        eprintln!("cad-gui: {error}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = match Config::parse()? {
        Startup::Help => {
            print!("{HELP}");
            println!("Desktop: local orbit/selection, temporary preview, explicit confirm/cancel. --command is not supported.");
            return Ok(());
        }
        Startup::Version => {
            println!("cad-gui {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Startup::Run(config) => config,
    };
    if config.command.is_some() {
        return Err("--command is only supported by cad-cli".into());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280., 820.])
            .with_min_inner_size([1050., 680.]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 24,
        ..Default::default()
    };
    eframe::run_native(
        "AI-CAD — Shared workspace",
        options,
        Box::new(move |cc| Ok(Box::new(Desktop::new(cc, config)))),
    )?;
    Ok(())
}
