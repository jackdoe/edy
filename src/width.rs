use crate::text::PieceTable;

pub fn char_width(c: char) -> usize {
    match c as u32 {
        0x0300..=0x036f
        | 0x0483..=0x0489
        | 0x0591..=0x05c7
        | 0x0610..=0x061a
        | 0x064b..=0x065f
        | 0x0670
        | 0x06d6..=0x06dc
        | 0x06df..=0x06e4
        | 0x0711
        | 0x0730..=0x074a
        | 0x07a6..=0x07b0
        | 0x0900..=0x0902
        | 0x093c
        | 0x0941..=0x0948
        | 0x094d
        | 0x0951..=0x0954
        | 0x0e31
        | 0x0e34..=0x0e3a
        | 0x0e47..=0x0e4e
        | 0x1ab0..=0x1aff
        | 0x1dc0..=0x1dff
        | 0x200b..=0x200f
        | 0x20d0..=0x20ff
        | 0xfe00..=0xfe0f
        | 0xfe20..=0xfe2f => 0,
        0x1100..=0x115f
        | 0x2e80..=0x303e
        | 0x3041..=0x33ff
        | 0x3400..=0x4dbf
        | 0x4e00..=0x9fff
        | 0xa000..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6
        | 0x1f300..=0x1f64f
        | 0x1f680..=0x1f6ff
        | 0x1f900..=0x1f9ff
        | 0x20000..=0x2fffd
        | 0x30000..=0x3fffd => 2,
        _ => 1,
    }
}

pub fn step(c: char, col: usize) -> usize {
    match c {
        '\t' => 8 - col % 8,
        c if (c as u32) < 0x20 => 2,
        _ => char_width(c),
    }
}

pub fn str_width(s: &str) -> usize {
    s.chars().fold(0, |w, c| w + step(c, w))
}

pub fn display_col(t: &PieceTable, line_start: usize, at: usize) -> usize {
    str_width(&t.slice(line_start, at))
}

pub fn offset_at_col(t: &PieceTable, line_start: usize, goal: usize) -> usize {
    let end = t.next_newline(line_start);
    let mut col = 0;
    let mut off = line_start;
    for c in t.slice(line_start, end).chars() {
        if col >= goal {
            return off;
        }
        col += step(c, col);
        off += c.len_utf8();
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('語'), 2);
        assert_eq!(char_width('\u{0301}'), 0);
        assert_eq!(str_width("a語b"), 4);
        assert_eq!(str_width("\tx"), 9);
        assert_eq!(str_width("ab\tx"), 9);
    }

    #[test]
    fn columns() {
        let t = PieceTable::new("ab語\ncd".into());
        assert_eq!(display_col(&t, 0, 2), 2);
        assert_eq!(display_col(&t, 0, 5), 4);
        assert_eq!(offset_at_col(&t, 0, 2), 2);
        assert_eq!(offset_at_col(&t, 0, 99), 5);
        assert_eq!(offset_at_col(&t, 6, 1), 7);
    }
}
