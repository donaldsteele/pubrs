//! Linux: printing through CUPS (`lpstat` to list printers, `lpr` to print
//! the exported PDF). Linux has no single system print dialog, so the app
//! shows its own printer chooser.

use std::path::Path;
use std::process::Command;

use super::{PrintBackend, PrintOptions, PrintOutcome, PrinterInfo};

pub struct CupsPrint;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `lpstat -e` (one destination per line) and `lpstat -d`
/// ("system default destination: NAME").
pub fn parse_printers(list: &str, default: &str) -> Vec<PrinterInfo> {
    let default_name = default
        .rsplit_once(':')
        .map(|(_, n)| n.trim().to_string())
        .unwrap_or_default();
    let mut v: Vec<PrinterInfo> = list
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|name| PrinterInfo {
            name: name.to_string(),
            is_default: name == default_name,
        })
        .collect();
    v.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
    v
}

impl PrintBackend for CupsPrint {
    fn name(&self) -> &'static str {
        "CUPS (lpr)"
    }

    fn has_system_dialog(&self) -> bool {
        false
    }

    fn printers(&self) -> Vec<PrinterInfo> {
        let list = run("lpstat", &["-e"]).unwrap_or_default();
        let default = run("lpstat", &["-d"]).unwrap_or_default();
        parse_printers(&list, &default)
    }

    fn print_pdf(
        &self,
        pdf: &Path,
        job_name: &str,
        options: &PrintOptions,
    ) -> Result<PrintOutcome, String> {
        let mut cmd = Command::new("lpr");
        if let Some(p) = &options.printer {
            cmd.arg("-P").arg(p);
        }
        cmd.arg("-#").arg(options.copies.clamp(1, 999).to_string());
        cmd.arg("-T").arg(job_name);
        cmd.arg(pdf);
        let out = cmd
            .output()
            .map_err(|e| format!("cannot run lpr (is CUPS installed?): {e}"))?;
        if out.status.success() {
            let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Ok(PrintOutcome::Queued(if msg.is_empty() {
                "sent to the printer".into()
            } else {
                msg
            }))
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lpstat_output() {
        let p = parse_printers(
            "Office_Laser\nHome_Inkjet\n",
            "system default destination: Home_Inkjet\n",
        );
        assert_eq!(p[0].name, "Home_Inkjet");
        assert!(p[0].is_default);
        assert_eq!(p[1].name, "Office_Laser");
        assert!(parse_printers("", "no system default destination").is_empty());
    }
}
