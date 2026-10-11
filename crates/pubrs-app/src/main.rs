//! pubrs desktop app.

// No console window on Windows; a terminal that starts pubrs still gets its
// output through `platform::console::attach_parent_console`.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod adapter;
mod app;
mod autosave;
mod canvas;
mod checker;
mod clipboard;
mod dialogs;
mod find;
mod keymap;
mod mesa;
mod panels;
mod perf;
#[allow(unsafe_code)]
mod platform;
mod render_worker;
mod rulers;
mod selftest;
mod showcase;
mod spell;
mod tables;
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
    /// Draw the curated website screenshots into this folder and exit.
    pub showcase: Option<PathBuf>,
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
                "--showcase" => {
                    let dir = it
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--showcase needs an output folder"))?;
                    args.showcase = Some(PathBuf::from(dir));
                    args.headless_platform = true;
                }
                "--version" => {
                    println!("pubrs-app {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                "--help" | "-h" => {
                    println!(
                        "pubrs-app [FILE.pubrs...]\n\nOptions:\n  --smoke-test        open a window and exit (CI)\n  --self-test DIR     run the GUI end-to-end scenario, write results to DIR\n  --showcase DIR      draw the website screenshots into DIR\n  --version"
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
    pub showcase: Option<Result<String, String>>,
}

fn options(renderer: eframe::Renderer, showcase: bool) -> eframe::NativeOptions {
    let size = if showcase {
        showcase::WINDOW_PX
    } else {
        [1280.0, 860.0]
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("pubrs")
        .with_app_id("pubrs")
        .with_inner_size(size)
        .with_min_inner_size([720.0, 480.0]);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/pubrs-256.png"))
    {
        viewport = viewport.with_icon(icon);
    }
    let mut opts = eframe::NativeOptions {
        renderer,
        viewport,
        ..Default::default()
    };
    // Every backend, so a GPU that wgpu reaches only through OpenGL is found;
    // `adapter::select` takes hardware first (docs/adr/0011-gui-adapter.md).
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(c) = &mut opts.wgpu_options.wgpu_setup {
        c.instance_descriptor.backends =
            eframe::wgpu::Backends::PRIMARY | eframe::wgpu::Backends::GL;
        c.native_adapter_selector = Some(std::sync::Arc::new(adapter::select));
    }
    opts
}

fn run(args: &Args, renderer: eframe::Renderer, outcome: &Rc<RefCell<Outcome>>) -> eframe::Result {
    let args = args.clone();
    let outcome = outcome.clone();
    eframe::run_native(
        "pubrs",
        options(renderer, args.showcase.is_some()),
        Box::new(move |cc| Ok(Box::new(app::PubrsApp::new(cc, args, outcome)))),
    )
}

fn main() -> anyhow::Result<()> {
    let console = platform::console::attach_parent_console();
    let result = run_app();
    if let Err(e) = &result
        && !console
    {
        platform::console::show_fatal_error(&format!("{e:#}"));
    }
    result
}

fn run_app() -> anyhow::Result<()> {
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
    let mut result = run(&args, eframe::Renderer::Wgpu, &outcome);
    if result.is_err()
        && mesa::restart_requested()
        && let Some(exe) = mesa::bundled_exe()
    {
        tracing::info!(
            "no GPU driver: restarting with Mesa llvmpipe ({})",
            exe.display()
        );
        match mesa::run_bundled(&exe) {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => {
                tracing::warn!(
                    "cannot start {} ({e}); drawing with the system's software adapter",
                    exe.display()
                );
                result = run(&args, eframe::Renderer::Wgpu, &outcome);
            }
        }
    }
    if let Err(e) = result {
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
    if args.showcase.is_some() {
        match outcome.showcase {
            Some(Ok(report)) => println!("showcase ok\n{report}"),
            Some(Err(e)) => anyhow::bail!("showcase FAILED: {e}"),
            None => anyhow::bail!("showcase did not finish"),
        }
    }
    Ok(())
}
