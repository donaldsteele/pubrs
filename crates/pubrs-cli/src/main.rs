//! `pubrs`: headless command line over `pubrs-api`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use pubrs_api::tools::doc::{DocOpen, DocRef};
use pubrs_api::tools::export::{ExportPdf, ExportPng, FontEmbeddingIn};
use pubrs_api::{ScriptFile, Session, SessionConfig, run_script};
use serde_json::json;

/// `println!` that exits quietly when stdout is closed (e.g. `pubrs tools | head`).
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if writeln!(std::io::stdout(), $($arg)*).is_err() {
            std::process::exit(0);
        }
    }};
}

#[derive(Parser)]
#[command(
    name = "pubrs",
    version,
    about = "pubrs desktop publishing — headless tools"
)]
struct Cli {
    /// Use only the bundled fonts (reproducible output on every machine).
    #[arg(long, global = true)]
    no_system_fonts: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Render pages of a .pubrs file to PNG.
    Render {
        file: PathBuf,
        /// Page number (1-based). Default: every page.
        #[arg(long)]
        page: Option<usize>,
        #[arg(long, default_value_t = 150.0)]
        dpi: f64,
        /// Include the bleed area.
        #[arg(long)]
        bleed: bool,
        /// Output file (one page) or directory (several pages).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export a .pubrs file to PDF.
    Export {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Leave out the bleed even if the document has one.
        #[arg(long)]
        no_bleed: bool,
        /// Add crop marks.
        #[arg(long)]
        crop_marks: bool,
        /// Convert text to outlines instead of embedding fonts.
        #[arg(long)]
        outlines: bool,
    },
    /// Check a .pubrs file for print problems. Exit code 1 if there are warnings.
    Validate {
        file: PathBuf,
        /// Print the warnings as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Replay a JSON list of tool calls (the same calls the MCP server takes).
    Script {
        file: PathBuf,
        /// Directory for outputs, available as ${out_dir} (default: current directory).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Extra variables: --var name=value (available as ${name}).
        #[arg(long = "var", value_parser = parse_var)]
        vars: Vec<(String, String)>,
        /// Print every step's full result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List the available tools.
    Tools {
        /// Include input schemas.
        #[arg(long)]
        json: bool,
    },
}

fn parse_var(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| format!("expected name=value, got {s:?}"))
}

fn session(cli: &Cli) -> Session {
    Session::new(SessionConfig {
        system_fonts: !cli.no_system_fonts,
        ..Default::default()
    })
}

fn open(s: &mut Session, file: &Path) -> Result<()> {
    s.doc_open(DocOpen {
        path: file.display().to_string(),
    })
    .map_err(|e| anyhow::anyhow!("{} ({})", e.message, e.hint))?;
    Ok(())
}

fn api<T>(r: pubrs_api::ApiResult<T>) -> Result<T> {
    r.map_err(|e| anyhow::anyhow!("{}: {} — {}", e.code, e.message, e.hint))
}

fn run(cli: Cli) -> Result<ExitCode> {
    let mut s = session(&cli);
    match &cli.command {
        Cmd::Render {
            file,
            page,
            dpi,
            bleed,
            output,
        } => {
            open(&mut s, file)?;
            let pages = api(s.page_list(DocRef::default()))?.pages;
            let selected: Vec<_> = match page {
                Some(n) => {
                    let p = pages.get(n.saturating_sub(1)).with_context(|| {
                        format!("page {n} does not exist ({} pages)", pages.len())
                    })?;
                    vec![p.clone()]
                }
                None => pages,
            };
            let stem = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            for p in &selected {
                let path = match (output, selected.len()) {
                    (Some(o), 1) if o.extension().is_some() => o.clone(),
                    (Some(dir), _) => dir.join(format!("{stem}-p{}.png", p.index + 1)),
                    (None, _) => PathBuf::from(format!("{stem}-p{}.png", p.index + 1)),
                };
                let out = api(s.export_png(ExportPng {
                    page_id: Some(p.page_id),
                    dpi: Some(*dpi),
                    include_bleed: *bleed,
                    path: Some(path.display().to_string()),
                    ..Default::default()
                }))?;
                say!("{} ({}x{})", path.display(), out.width, out.height);
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Export {
            file,
            output,
            no_bleed,
            crop_marks,
            outlines,
        } => {
            open(&mut s, file)?;
            let out = output.clone().unwrap_or_else(|| file.with_extension("pdf"));
            let r = api(s.export_pdf(ExportPdf {
                path: Some(out.display().to_string()),
                bleed: no_bleed.then_some(false),
                crop_marks: *crop_marks,
                font_embedding: if *outlines {
                    FontEmbeddingIn::Outlines
                } else {
                    FontEmbeddingIn::Subset
                },
                ..Default::default()
            }))?;
            say!(
                "{} ({} pages, {} bytes)",
                out.display(),
                r.page_count,
                r.bytes
            );
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Validate { file, json } => {
            open(&mut s, file)?;
            let v = api(s.validate(DocRef::default()))?;
            if *json {
                say!("{}", serde_json::to_string_pretty(&v)?);
            } else if v.ok {
                say!("{}: no problems found", file.display());
            } else {
                for w in &v.warnings {
                    say!(
                        "{:?}: {}{}\n  hint: {}",
                        w.code,
                        w.message,
                        w.frame.map(|f| format!(" (frame {f})")).unwrap_or_default(),
                        w.hint
                    );
                }
                say!("{} warning(s)", v.warnings.len());
            }
            Ok(if v.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Cmd::Script {
            file,
            out,
            vars,
            json,
        } => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            let script: ScriptFile = serde_json::from_str(&text).context("invalid script JSON")?;
            let out_dir = out.clone().unwrap_or_else(|| PathBuf::from("."));
            std::fs::create_dir_all(&out_dir)?;
            let script_dir = file
                .parent()
                .map(|p| {
                    if p.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        p
                    }
                })
                .unwrap_or(Path::new("."));
            let mut v = BTreeMap::new();
            v.insert("script_dir".into(), json!(script_dir.display().to_string()));
            v.insert("out_dir".into(), json!(out_dir.display().to_string()));
            for (k, val) in vars {
                v.insert(k.clone(), json!(val));
            }
            let report = run_script(&mut s, &script.steps(), v);
            if *json {
                say!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for r in &report.results {
                    match (&r.error, r.ok) {
                        (None, _) => say!("ok    {:>3} {}", r.index, r.tool),
                        (Some(e), true) => {
                            say!("ok    {:>3} {} (expected {})", r.index, r.tool, e.code)
                        }
                        (Some(e), false) => {
                            say!(
                                "FAIL  {:>3} {}: {} — {}",
                                r.index,
                                r.tool,
                                e.message,
                                e.hint
                            )
                        }
                    }
                }
            }
            if !report.ok {
                bail!("script failed");
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Tools { json } => {
            for t in pubrs_api::all_tools() {
                if *json {
                    say!(
                        "{}",
                        json!({"name": t.name, "description": t.description, "input_schema": (t.input_schema)()})
                    );
                } else {
                    say!("{:<24} {}", t.name, t.description);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}
