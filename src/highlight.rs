//! Fast line-based Markdown syntax highlighting for the plain-text editor.
//! Hand-rolled scanner (no regex): headings, fences, quotes, lists, hr,
//! inline code/bold/italic/links. Cached per frame keyed on (text, theme).

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, TextStyle};

pub const EDITOR_FONT_SIZE: f32 = 15.0;

struct Palette {
    text: Color32,
    heading: Color32,
    marker: Color32, // #, >, list bullets, fence backticks
    code: Color32,
    emphasis: Color32,
    link: Color32,
    url: Color32,
    quote: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            text: Color32::from_gray(220),
            heading: Color32::from_rgb(120, 180, 255),
            marker: Color32::from_gray(130),
            code: Color32::from_rgb(214, 157, 102),
            emphasis: Color32::from_rgb(235, 219, 178),
            link: Color32::from_rgb(100, 170, 240),
            url: Color32::from_gray(120),
            quote: Color32::from_rgb(140, 170, 140),
        }
    } else {
        Palette {
            text: Color32::from_gray(40),
            heading: Color32::from_rgb(0, 90, 200),
            marker: Color32::from_gray(150),
            code: Color32::from_rgb(160, 80, 20),
            emphasis: Color32::from_rgb(110, 80, 30),
            link: Color32::from_rgb(20, 100, 210),
            url: Color32::from_gray(150),
            quote: Color32::from_rgb(80, 130, 80),
        }
    }
}

fn font() -> FontId {
    FontId::monospace(EDITOR_FONT_SIZE)
}

fn fmt(color: Color32) -> TextFormat {
    TextFormat { font_id: font(), color, ..Default::default() }
}

fn fmt_italic(color: Color32) -> TextFormat {
    TextFormat { font_id: font(), color, italics: true, ..Default::default() }
}

pub fn layout_job(text: &str, dark: bool) -> LayoutJob {
    let p = palette(dark);
    let mut job = LayoutJob::default();
    job.text = String::with_capacity(text.len());
    let mut in_fence = false;

    // Iterate lines keeping the trailing newline attached so offsets stay exact.
    let mut rest = text;
    loop {
        let (line, remainder, had_newline) = match rest.find('\n') {
            Some(i) => (&rest[..i], &rest[i + 1..], true),
            None => (rest, "", false),
        };
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            job.append(line, 0.0, fmt(p.marker));
            in_fence = !in_fence;
        } else if in_fence {
            job.append(line, 0.0, fmt(p.code));
        } else if trimmed.starts_with('#') {
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            if hashes <= 6 && trimmed[hashes..].starts_with(' ') {
                let mut f = fmt(p.heading);
                f.font_id = FontId::monospace(EDITOR_FONT_SIZE + 1.0);
                job.append(line, 0.0, f);
            } else {
                append_inline(&mut job, line, &p);
            }
        } else if trimmed.starts_with('>') {
            job.append(line, 0.0, fmt_italic(p.quote));
        } else if is_hr(trimmed) {
            job.append(line, 0.0, fmt(p.marker));
        } else if let Some(marker_len) = list_marker_len(line) {
            job.append(&line[..marker_len], 0.0, fmt(p.marker));
            append_inline(&mut job, &line[marker_len..], &p);
        } else {
            append_inline(&mut job, line, &p);
        }

        if had_newline {
            job.append("\n", 0.0, fmt(p.text));
        }
        if remainder.is_empty() && !had_newline {
            break;
        }
        if remainder.is_empty() {
            break;
        }
        rest = remainder;
    }
    job
}

fn is_hr(t: &str) -> bool {
    let t = t.trim_end();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// Length of a leading list marker ("- ", "* ", "+ ", "12. ", including indent
/// and task-list checkbox), or None.
fn list_marker_len(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start().len();
    let t = &line[indent..];
    let after = if let Some(r) = t.strip_prefix("- ").or(t.strip_prefix("* ")).or(t.strip_prefix("+ ")) {
        r
    } else {
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        t[digits..].strip_prefix(". ")?
    };
    let mut len = line.len() - after.len();
    // task list checkbox
    for cb in ["[ ] ", "[x] ", "[X] "] {
        if after.starts_with(cb) {
            len += cb.len();
            break;
        }
    }
    Some(len)
}

/// Inline spans: `code`, **bold**, *italic*, [text](url). Non-nested,
/// first-match; plenty for an editor view.
fn append_inline(job: &mut LayoutJob, line: &str, p: &Palette) {
    let bytes = line.as_bytes();
    let mut plain_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let matched = match bytes[i] {
            b'`' => find_close(line, i + 1, "`").map(|end| {
                (end + 1, fmt(p.code))
            }),
            b'*' if line[i..].starts_with("**") => find_close(line, i + 2, "**").map(|end| {
                let mut f = fmt(p.emphasis);
                f.color = p.emphasis;
                (end + 2, f)
            }),
            b'*' => find_close(line, i + 1, "*").map(|end| (end + 1, fmt_italic(p.emphasis))),
            b'[' => line[i..].find("](").and_then(|mid| {
                line[i + mid + 2..].find(')').map(|close| {
                    let text_end = i + mid + 1; // after ']'
                    let url_end = i + mid + 2 + close + 1;
                    (text_end, url_end, fmt(p.link), fmt(p.url))
                })
            }).map(|(text_end, url_end, link_fmt, url_fmt)| {
                if plain_start < i {
                    job.append(&line[plain_start..i], 0.0, fmt(p.text));
                }
                job.append(&line[i..text_end], 0.0, link_fmt);
                job.append(&line[text_end..url_end], 0.0, url_fmt);
                (url_end, fmt(p.text)) // dummy fmt; span already appended
            }).map(|(end, _)| (end, TextFormat::default())),
            _ => None,
        };
        match matched {
            Some((end, format)) if bytes[i] == b'[' => {
                // link already fully appended above
                let _ = format;
                plain_start = end;
                i = end;
            }
            Some((end, format)) => {
                if plain_start < i {
                    job.append(&line[plain_start..i], 0.0, fmt(p.text));
                }
                job.append(&line[i..end], 0.0, format);
                plain_start = end;
                i = end;
            }
            None => {
                // advance one full char (not byte) to stay on UTF-8 boundaries
                i += line[i..].chars().next().map_or(1, |c| c.len_utf8());
            }
        }
    }
    if plain_start < line.len() {
        job.append(&line[plain_start..], 0.0, fmt(p.text));
    }
}

/// Find closing delimiter at or after `from`; returns byte index of delimiter start.
fn find_close(line: &str, from: usize, delim: &str) -> Option<usize> {
    if from >= line.len() {
        return None;
    }
    line[from..].find(delim).filter(|&off| off > 0).map(|off| from + off)
}

/// Set the preview/reading text styles on a ui (slightly larger, comfy).
pub fn reading_style(ui: &mut egui::Ui) {
    let styles = &mut ui.style_mut().text_styles;
    if let Some(f) = styles.get_mut(&TextStyle::Body) {
        f.size = 16.0;
    }
    if let Some(f) = styles.get_mut(&TextStyle::Monospace) {
        f.size = 14.5;
    }
    ui.style_mut().spacing.item_spacing.y = 8.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_text_matches_input_exactly() {
        // The layout job must reproduce the source byte-for-byte or the
        // editor's cursor mapping breaks.
        let src = "# Head\n\n- item **bold** and `code`\n> quote\n```rs\nlet x = 1;\n```\ntext [a](http://b) *it*\n日本語 **太字**\n";
        let job = layout_job(src, true);
        assert_eq!(job.text, src);
        let job = layout_job(src, false);
        assert_eq!(job.text, src);
    }

    #[test]
    fn list_markers() {
        assert_eq!(list_marker_len("- hi"), Some(2));
        assert_eq!(list_marker_len("  * hi"), Some(4));
        assert_eq!(list_marker_len("3. hi"), Some(3));
        assert_eq!(list_marker_len("- [x] done"), Some(6));
        assert_eq!(list_marker_len("-nope"), None);
        assert_eq!(list_marker_len("word - nope"), None);
    }
}
