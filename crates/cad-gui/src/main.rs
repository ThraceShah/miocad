#![forbid(unsafe_code)]
use cad_access::{Action as Startup, Config, HELP};
use cad_gui::bridge::Bridge;
use cad_ui_egui::{Action, Event, Failure, Workspace};
use std::{sync::Arc, time::Duration};
struct Desktop {
    workspace: Workspace,
    bridge: Bridge,
    scheduled: u64,
}
impl Desktop {
    fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        Workspace::theme(&cc.egui_ctx);
        if let Some(state) = &cc.wgpu_render_state {
            let info = state.adapter.get_info();
            eprintln!(
                "cad-gui display: {:?} / {} / {:?}",
                info.backend, info.name, info.device_type
            );
        }
        let ctx = cc.egui_ctx.clone();
        let bridge = Bridge::start(config, Arc::new(move || ctx.request_repaint()));
        Self {
            workspace: Workspace::default(),
            bridge,
            scheduled: u64::MAX,
        }
    }
}
impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.bridge.events.try_recv() {
            self.workspace.apply(event, ctx);
        }
        for action in self.workspace.show(ctx) {
            let (ticket, preview) = match &action {
                Action::Select(_) => (None, false),
                Action::NewDocument { ticket }
                | Action::Refresh { ticket }
                | Action::Reconnect { ticket }
                | Action::Execute { ticket, .. } => (Some(*ticket), false),
                Action::Preview { ticket, .. } => (Some(*ticket), true),
            };
            if let Err(message) = self.bridge.send(action) {
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
        ..Default::default()
    };
    eframe::run_native(
        "AI-CAD — Shared workspace",
        options,
        Box::new(move |cc| Ok(Box::new(Desktop::new(cc, config)))),
    )?;
    Ok(())
}
