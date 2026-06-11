use crate::editor::{Editor, Mode};
use crate::width;
use std::io::{self, Write};

pub fn render(ed: &Editor, out: &mut impl Write) -> io::Result<()> {
    let (rows, cols) = (ed.rows, ed.cols);
    if rows < 3 || cols < 8 {
        return Ok(());
    }
    let trows = rows - 2;

    let b = ed.buf();
    let line = b.line();
    let ccol = width::display_col(&b.text, b.text.line_start(line), b.cursor);
    let (top, left) = (b.top, b.left);

    let mut f = String::with_capacity(rows * cols * 2);
    f.push_str("\x1b[?2026h\x1b[?25l\x1b[H");
    let sel = b.selection();
    let nlines = b.text.line_count();
    for r in 0..trows {
        f.push_str("\x1b[K");
        let li = top + r;
        if li < nlines {
            let ls = b.text.line_start(li);
            let le = b.text.next_newline(ls);
            let s = b.text.slice(ls, le);
            let (line_sel, nl_sel) = match sel {
                Some((a, z)) => {
                    let s0 = a.max(ls);
                    let s1 = z.min(le);
                    (
                        if s0 < s1 { Some((s0 - ls, s1 - ls)) } else { None },
                        a <= le && z > le,
                    )
                }
                None => (None, false),
            };
            draw_line(&mut f, &s, left, cols, line_sel, nl_sel);
        }
        f.push_str("\r\n");
    }
    modeline(ed, &mut f, cols, line, ccol, nlines);
    f.push_str("\r\n\x1b[K");

    let echo_cursor = echo_line(ed, &mut f, cols);
    let (crow, ccol_screen) = match echo_cursor {
        Some(c) => (rows, c + 1),
        None => (line - top + 1, ccol - left + 1),
    };
    f.push_str(&format!("\x1b[{};{}H\x1b[?25h\x1b[?2026l", crow, ccol_screen));
    out.write_all(f.as_bytes())?;
    out.flush()
}

fn draw_line(f: &mut String, s: &str, left: usize, cols: usize, sel: Option<(usize, usize)>, nl_sel: bool) {
    let total = width::str_width(s);
    let truncated = total > left + cols;
    let avail = if truncated { cols - 1 } else { cols };
    let mut col = 0;
    let mut vw = 0;
    let mut bi = 0;
    let mut rev = false;
    for c in s.chars() {
        if col >= left + avail {
            break;
        }
        let w = width::step(c, col);
        if col + w > left + avail {
            break;
        }
        if col >= left {
            let want = sel.is_some_and(|(a, z)| bi >= a && bi < z);
            if want != rev {
                f.push_str(if want { "\x1b[7m" } else { "\x1b[m" });
                rev = want;
            }
            match c {
                '\t' => {
                    for _ in 0..w {
                        f.push(' ');
                    }
                }
                c if (c as u32) < 0x20 => {
                    f.push('^');
                    f.push((c as u8 + 0x40) as char);
                }
                _ => f.push(c),
            }
            vw += w;
        } else if col + w > left {
            for _ in 0..col + w - left {
                f.push(' ');
            }
            vw += col + w - left;
        }
        col += w;
        bi += c.len_utf8();
    }
    if rev {
        f.push_str("\x1b[m");
    }
    if truncated {
        while vw < cols - 1 {
            f.push(' ');
            vw += 1;
        }
        f.push('$');
    } else if nl_sel && vw < cols {
        f.push_str("\x1b[7m \x1b[m");
    }
}

fn clip(s: &str, cols: usize) -> (String, usize) {
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = width::step(c, w);
        if w + cw > cols {
            break;
        }
        out.push(c);
        w += cw;
    }
    (out, w)
}

fn modeline(ed: &Editor, f: &mut String, cols: usize, line: usize, ccol: usize, nlines: usize) {
    let b = ed.buf();
    let flag = if b.modified { "**" } else { "--" };
    let pct = (line + 1) * 100 / nlines;
    let s = format!(
        "-{}- {}   L{}/{} C{}   {}%   {}",
        flag,
        b.name,
        line + 1,
        nlines,
        ccol,
        pct,
        b.path.as_deref().map(|p| p.display().to_string()).unwrap_or_default()
    );
    let (mut clipped, w) = clip(&s, cols);
    clipped.extend(std::iter::repeat_n(' ', cols - w));
    f.push_str("\x1b[7m");
    f.push_str(&clipped);
    f.push_str("\x1b[m");
}

fn echo_line(ed: &Editor, f: &mut String, cols: usize) -> Option<usize> {
    match &ed.mode {
        Mode::Prompt(p) => {
            let s = if p.matches.is_empty() {
                format!("{}{}", p.label, p.input)
            } else {
                format!("{}{}  {{{}}}", p.label, p.input, p.matches)
            };
            let (clipped, _) = clip(&s, cols);
            f.push_str(&clipped);
            let c = width::str_width(&p.label) + width::str_width(&p.input[..p.cur]);
            Some(c.min(cols - 1))
        }
        Mode::Search(s) => {
            let msg = format!(
                "{}I-search{}: {}",
                if s.found { "" } else { "Failing " },
                if s.forward { "" } else { " backward" },
                s.needle
            );
            let (clipped, _) = clip(&msg, cols);
            f.push_str(&clipped);
            None
        }
        Mode::Replace(r) => {
            let msg = format!("Query replacing {} with {}: (y, n, !, q)", r.from, r.to);
            let (clipped, _) = clip(&msg, cols);
            f.push_str(&clipped);
            None
        }
        Mode::Edit => {
            let (clipped, _) = clip(&ed.echo, cols);
            f.push_str(&clipped);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(s: &str, left: usize, cols: usize) -> String {
        let mut f = String::new();
        draw_line(&mut f, s, left, cols, None, false);
        f
    }

    #[test]
    fn selection_reverses_span() {
        let mut f = String::new();
        draw_line(&mut f, "abcde", 0, 10, Some((1, 3)), false);
        assert_eq!(f, "a\x1b[7mbc\x1b[mde");
    }

    #[test]
    fn selection_to_line_end_marks_newline() {
        let mut f = String::new();
        draw_line(&mut f, "ab", 0, 10, Some((0, 2)), true);
        assert_eq!(f, "\x1b[7mab\x1b[m\x1b[7m \x1b[m");
    }

    #[test]
    fn selection_on_empty_line_shows_space() {
        let mut f = String::new();
        draw_line(&mut f, "", 0, 10, None, true);
        assert_eq!(f, "\x1b[7m \x1b[m");
    }

    #[test]
    fn plain_line() {
        assert_eq!(drawn("hello", 0, 10), "hello");
    }

    #[test]
    fn truncated_line_marks_dollar() {
        assert_eq!(drawn("abcdefghij", 0, 8), "abcdefg$");
    }

    #[test]
    fn horizontal_scroll() {
        assert_eq!(drawn("abcdefghij", 2, 8), "cdefghij");
        assert_eq!(drawn("abcdefghijklm", 2, 8), "cdefghi$");
    }

    #[test]
    fn tabs_expand() {
        assert_eq!(drawn("\ta", 0, 12), "        a");
    }

    #[test]
    fn control_chars_caret() {
        assert_eq!(drawn("a\u{1}b", 0, 10), "a^Ab");
    }

    #[test]
    fn wide_chars() {
        assert_eq!(drawn("語ab", 0, 10), "語ab");
        assert_eq!(drawn("語ab", 1, 10), " ab");
    }

    #[test]
    fn wide_char_at_right_edge_dropped() {
        assert_eq!(drawn("ab語cd", 0, 4), "ab $");
    }
}
