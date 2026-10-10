//! pubrs desktop app.

use std::cell::Cell;
use std::rc::Rc;

use eframe::egui;

/// Command-line options for the GUI.
#[derive(Debug, Clone, Default)]
struct Args {
    /// Open a window, draw a few frames, then exit 0. Used by CI.
    smoke_test: bool,
}

impl Args {
    fn parse() -> anyhow::Result<Self> {
        let mut args = Args::default();
        for a in std::env::args().skip(1) {
            match a.as_str() {
                "--smoke-test" => args.smoke_test = true,
                "--version" => {
                    println!("pubrs-app {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                other => anyhow::bail!("unknown argument {other:?}"),
            }
        }
        Ok(args)
    }
}

struct App {
    args: Args,
    frames: u64,
    painted: Rc<Cell<bool>>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("pubrs");
            ui.label("Desktop publishing");
        });
        self.frames += 1;
        if self.args.smoke_test {
            if self.frames >= 3 {
                self.painted.set(true);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
    }
}

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("pubrs")
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    }
}

fn run(args: &Args, renderer: eframe::Renderer, painted: &Rc<Cell<bool>>) -> eframe::Result {
    let args = args.clone();
    let painted = painted.clone();
    eframe::run_native(
        "pubrs",
        options(renderer),
        Box::new(move |_cc| {
            Ok(Box::new(App {
                args,
                frames: 0,
                painted,
            }))
        }),
    )
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu=warn,naga=warn,eframe=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse()?;
    let painted = Rc::new(Cell::new(false));
    tracing::info!("starting GUI with the wgpu renderer");
    if let Err(e) = run(&args, eframe::Renderer::Wgpu, &painted) {
        tracing::warn!("wgpu renderer unavailable ({e}); falling back to OpenGL");
        run(&args, eframe::Renderer::Glow, &painted)
            .map_err(|e| anyhow::anyhow!("no usable renderer: {e}"))?;
    }
    if args.smoke_test {
        anyhow::ensure!(painted.get(), "smoke test: window closed before painting");
        println!("smoke test ok");
    }
    Ok(())
}
