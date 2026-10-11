//! Minimal text markup accepted by `text_set` and `frame_add_text`:
//!
//! - one line per paragraph;
//! - `# ` starts a "Heading 1" paragraph, `## ` a "Heading 2" paragraph;
//! - `**bold**`, `*italic*` and `__underline__` inline;
//! - a backslash escapes the next character (`\*`).

use pubrs_core::{CharProps, ParaProps, Paragraph, Run, StyleKind, StyleSheet};

fn toggles(text: &str) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let (mut bold, mut italic, mut underline) = (false, false, false);
    let mut cur = String::new();
    let flush = |cur: &mut String, runs: &mut Vec<Run>, b: bool, i: bool, u: bool| {
        if !cur.is_empty() {
            runs.push(Run {
                text: std::mem::take(cur),
                style: None,
                props: CharProps {
                    bold: b.then_some(true),
                    italic: i.then_some(true),
                    underline: u.then_some(true),
                    ..Default::default()
                },
                field: None,
            });
        }
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match (c, next) {
            ('\\', Some(n)) => {
                cur.push(n);
                i += 2;
            }
            ('*', Some('*')) => {
                flush(&mut cur, &mut runs, bold, italic, underline);
                bold = !bold;
                i += 2;
            }
            ('_', Some('_')) => {
                flush(&mut cur, &mut runs, bold, italic, underline);
                underline = !underline;
                i += 2;
            }
            ('*', _) => {
                flush(&mut cur, &mut runs, bold, italic, underline);
                italic = !italic;
                i += 1;
            }
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }
    flush(&mut cur, &mut runs, bold, italic, underline);
    if runs.is_empty() {
        runs.push(Run::default());
    }
    runs
}

/// Parse markup into paragraphs. Heading prefixes use the style sheet's
/// "Heading 1"/"Heading 2" paragraph styles when they exist.
pub fn parse(text: &str, styles: &StyleSheet) -> Vec<Paragraph> {
    let text = pubrs_core::text::normalize_newlines(text);
    let style = |name: &str| {
        styles
            .by_name(name)
            .filter(|s| s.kind == StyleKind::Paragraph)
            .map(|s| s.id)
    };
    text.split('\n')
        .map(|line| {
            let (style, body) = if let Some(rest) = line.strip_prefix("## ") {
                (style("Heading 2"), rest)
            } else if let Some(rest) = line.strip_prefix("# ") {
                (style("Heading 1"), rest)
            } else {
                (None, line)
            };
            Paragraph {
                style,
                props: ParaProps::default(),
                runs: toggles(body),
            }
        })
        .collect()
}

/// True if `text` contains any markup syntax.
pub fn looks_like_markup(text: &str) -> bool {
    text.contains("**")
        || text.contains("__")
        || text
            .lines()
            .any(|l| l.starts_with("# ") || l.starts_with("## "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubrs_core::SeqIds;

    #[test]
    fn headings_and_inline() {
        let sheet = StyleSheet::with_defaults(&mut SeqIds::new(1));
        let p = parse("# Title\nPlain **bold** and *it* __u__ \\*star\\*", &sheet);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].style, sheet.by_name("Heading 1").map(|s| s.id));
        assert_eq!(p[0].text(), "Title");
        assert_eq!(p[1].text(), "Plain bold and it u *star*");
        let bold = p[1].runs.iter().find(|r| r.text == "bold").unwrap();
        assert_eq!(bold.props.bold, Some(true));
        let it = p[1].runs.iter().find(|r| r.text == "it").unwrap();
        assert_eq!(it.props.italic, Some(true));
        let u = p[1].runs.iter().find(|r| r.text == "u").unwrap();
        assert_eq!(u.props.underline, Some(true));
    }
}
