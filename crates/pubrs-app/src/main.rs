//! pubrs desktop app.

mod app;
mod autosave;
mod canvas;
mod dialogs;
mod keymap;
mod panels;
mod platform;
mod selftest;
mod textedit;
mod view;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui;

/// Command-line options.
#[derive(Debug, Clone, Default)]
pub struct Args {
    /// Open a window, draw a few frames, then exit 0 (CI).
    pub smoke_test: bool,
    /// Drive the GUI through the end-to-end scenario and exit (CI).
    pub self_test: Option<PathBuf>,
    /// Use only bundled fonts and no native dialogs/clipboard (tests).
    pub headless_platform: bool,
    /// Files to open.
    pub files: Vec<PathBuf>,
}

impl Args {
    fn parse() -> anyhow::Result<Self> {
        let mut args = Args::default();
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--smoke-test" => args.smoke_test = true,
                "--self-test" => {
                    let dir = it
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--self-test needs an output folder"))?;
                    args.self_test = Some(PathBuf::from(dir));
                    args.headless_platform = true;
                }
                "--version" => {
                    println!("pubrs-app {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                "--help" | "-h" => {
                    println!(
                        "pubrs-app [FILE.pubrs...]\n\nOptions:\n  --smoke-test        open a window and exit (CI)\n  --self-test DIR     run the GUI end-to-end scenario, write results to DIR\n  --version"
                    );
                    std::process::exit(0);
                }
                other if other.starts_with('-') => anyhow::bail!("unknown option {other:?}"),
                path => args.files.push(PathBuf::from(path)),
            }
        }
        Ok(args)
    }
}

/// Result reported by the app after the window closes.
#[derive(Debug, Default, Clone)]
pub struct Outcome {
    pub painted: bool,
    pub self_test: Option<Result<String, String>>,
}

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    let mut opts = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("pubrs")
            .with_app_id("pubrs")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    // egui draws with Vulkan/Metal/DX12 here and leaves GL to the glow fallback.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(c) = &mut opts.wgpu_options.wgpu_setup {
        c.instance_descriptor.backends = eframe::wgpu::Backends::PRIMARY;
    }
    opts
}

fn run(args: &Args, renderer: eframe::Renderer, outcome: &Rc<RefCell<Outcome>>) -> eframe::Result {
    let args = args.clone();
    let outcome = outcome.clone();
    eframe::run_native(
        "pubrs",
        options(renderer),
        Box::new(move |cc| Ok(Box::new(app::PubrsApp::new(cc, args, outcome)))),
    )
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "info,wgpu=warn,wgpu_hal=error,naga=warn,eframe=warn,egui_wgpu=warn,zbus=warn"
                    .into()
            }),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse()?;
    let outcome = Rc::new(RefCell::new(Outcome::default()));
    tracing::info!("starting GUI with the wgpu renderer");
    if let Err(e) = run(&args, eframe::Renderer::Wgpu, &outcome) {
        tracing::warn!("wgpu renderer unavailable ({e}); falling back to OpenGL");
        run(&args, eframe::Renderer::Glow, &outcome)
            .map_err(|e| anyhow::anyhow!("no usable renderer: {e}"))?;
    }
    let outcome = outcome.borrow().clone();
    if args.smoke_test {
        anyhow::ensure!(outcome.painted, "smoke test: window closed before painting");
        println!("smoke test ok");
    }
    if args.self_test.is_some() {
        match outcome.self_test {
            Some(Ok(report)) => println!("self-test ok\n{report}"),
            Some(Err(e)) => anyhow::bail!("self-test FAILED: {e}"),
            None => anyhow::bail!("self-test did not finish"),
        }
    }
    Ok(())
}
