#[derive(Clone, Copy, PartialEq)]
enum Src {
    Original,
    Add,
}

struct Piece {
    src: Src,
    start: usize,
    len: usize,
    newlines: usize,
}

pub struct PieceTable {
    original: Box<str>,
    add: String,
    pieces: Vec<Piece>,
    len: usize,
}

fn count_nl(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

impl PieceTable {
    pub fn new(original: String) -> PieceTable {
        let original = original.into_boxed_str();
        let len = original.len();
        let mut pieces = Vec::new();
        if len > 0 {
            pieces.push(Piece { src: Src::Original, start: 0, len, newlines: count_nl(&original) });
        }
        PieceTable { original, add: String::new(), pieces, len }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    fn source(&self, src: Src) -> &str {
        match src {
            Src::Original => &self.original,
            Src::Add => &self.add,
        }
    }

    fn text_of(&self, p: &Piece) -> &str {
        &self.source(p.src)[p.start..p.start + p.len]
    }

    fn nl_in(&self, src: Src, start: usize, len: usize) -> usize {
        count_nl(&self.source(src)[start..start + len])
    }

    fn walk(&self) -> impl Iterator<Item = (usize, &Piece, &str)> + '_ {
        let mut off = 0;
        self.pieces.iter().map(move |p| {
            let start = off;
            off += p.len;
            (start, p, self.text_of(p))
        })
    }

    fn locate(&self, at: usize) -> (usize, usize) {
        let mut off = 0;
        for (i, p) in self.pieces.iter().enumerate() {
            if at < off + p.len {
                return (i, at - off);
            }
            off += p.len;
        }
        (self.pieces.len(), 0)
    }

    fn split(&mut self, i: usize, off: usize) {
        let (src, start, len, nls) = {
            let p = &self.pieces[i];
            (p.src, p.start, p.len, p.newlines)
        };
        let left_nl = self.nl_in(src, start, off);
        let left = Piece { src, start, len: off, newlines: left_nl };
        let right = Piece { src, start: start + off, len: len - off, newlines: nls - left_nl };
        self.pieces.splice(i..=i, [left, right]);
    }

    pub fn insert(&mut self, at: usize, s: &str) {
        if s.is_empty() {
            return;
        }
        let start = self.add.len();
        self.add.push_str(s);
        let piece = Piece { src: Src::Add, start, len: s.len(), newlines: count_nl(s) };
        let (i, off) = self.locate(at);
        if off == 0 {
            if i > 0 {
                let prev = &mut self.pieces[i - 1];
                if prev.src == Src::Add && prev.start + prev.len == start {
                    prev.len += piece.len;
                    prev.newlines += piece.newlines;
                    self.len += s.len();
                    return;
                }
            }
            self.pieces.insert(i, piece);
        } else {
            self.split(i, off);
            self.pieces.insert(i + 1, piece);
        }
        self.len += s.len();
    }

    pub fn delete(&mut self, start: usize, end: usize) -> String {
        let end = end.min(self.len);
        if start >= end {
            return String::new();
        }
        let removed = self.slice(start, end);
        let (mut i, off) = self.locate(start);
        if off > 0 {
            self.split(i, off);
            i += 1;
        }
        let mut remaining = end - start;
        while remaining > 0 {
            if self.pieces[i].len <= remaining {
                remaining -= self.pieces[i].len;
                self.pieces.remove(i);
            } else {
                let (src, pstart) = (self.pieces[i].src, self.pieces[i].start);
                let cut_nl = self.nl_in(src, pstart, remaining);
                let p = &mut self.pieces[i];
                p.start += remaining;
                p.len -= remaining;
                p.newlines -= cut_nl;
                remaining = 0;
            }
        }
        self.len -= end - start;
        removed
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.len);
        if start >= end {
            return String::new();
        }
        let mut out = String::with_capacity(end - start);
        for (off, p, s) in self.walk() {
            if off >= end {
                break;
            }
            if off + p.len > start {
                out.push_str(&s[start.saturating_sub(off)..(end - off).min(p.len)]);
            }
        }
        out
    }

    pub fn all(&self) -> String {
        self.slice(0, self.len)
    }

    pub fn next_boundary(&self, at: usize) -> usize {
        match self.char_at(at) {
            Some(c) => at + c.len_utf8(),
            None => self.len,
        }
    }

    pub fn prev_boundary(&self, at: usize) -> usize {
        if at == 0 {
            return 0;
        }
        let (i, off) = self.locate(at - 1);
        let s = self.text_of(&self.pieces[i]);
        let mut j = off;
        while !s.is_char_boundary(j) {
            j -= 1;
        }
        at - 1 - (off - j)
    }

    pub fn snap(&self, at: usize) -> usize {
        let at = at.min(self.len);
        if at == self.len {
            return at;
        }
        let (i, off) = self.locate(at);
        let s = self.text_of(&self.pieces[i]);
        let mut j = off;
        while !s.is_char_boundary(j) {
            j -= 1;
        }
        at - (off - j)
    }

    pub fn char_at(&self, at: usize) -> Option<char> {
        if at >= self.len {
            return None;
        }
        let (i, off) = self.locate(at);
        self.text_of(&self.pieces[i])[off..].chars().next()
    }

    pub fn line_count(&self) -> usize {
        self.pieces.iter().map(|p| p.newlines).sum::<usize>() + 1
    }

    pub fn line_of(&self, at: usize) -> usize {
        let mut nl = 0;
        for (off, p, s) in self.walk() {
            if off + p.len <= at {
                nl += p.newlines;
                continue;
            }
            if at > off && p.newlines > 0 {
                nl += count_nl(&s[..at - off]);
            }
            break;
        }
        nl
    }

    pub fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let mut need = line;
        for (off, p, s) in self.walk() {
            if p.newlines < need {
                need -= p.newlines;
                continue;
            }
            let mut i = 0;
            for _ in 0..need {
                i += s[i..].find('\n').unwrap() + 1;
            }
            return off + i;
        }
        self.len
    }

    pub fn next_newline(&self, from: usize) -> usize {
        for (off, p, s) in self.walk() {
            if off + p.len > from && p.newlines > 0 {
                let local = from.saturating_sub(off);
                if let Some(i) = s[local..].find('\n') {
                    return off + local + i;
                }
            }
        }
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::PieceTable;

    fn check(pt: &PieceTable, model: &str) {
        assert_eq!(pt.all(), model);
        assert_eq!(pt.len(), model.len());
        assert_eq!(pt.line_count(), model.matches('\n').count() + 1);
    }

    #[test]
    fn insert_delete_matches_string_model() {
        let mut pt = PieceTable::new("hello\nworld\n".into());
        let mut model = String::from("hello\nworld\n");
        let ops: &[(bool, usize, &str)] = &[
            (true, 0, "a"),
            (true, 6, "xy\n"),
            (false, 2, "5"),
            (true, 0, "δοκιμή "),
            (false, 0, "3"),
            (true, 10, "\n\n"),
            (false, 4, "9"),
        ];
        let floor = |m: &str, mut i: usize| {
            i = i.min(m.len());
            while !m.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        for &(ins, at, s) in ops {
            let at = floor(&model, at);
            if ins {
                pt.insert(at, s);
                model.insert_str(at, s);
            } else {
                let n: usize = s.parse().unwrap();
                let end = floor(&model, at + n);
                let removed = pt.delete(at, end);
                assert_eq!(removed, model[at..end].to_string());
                model.replace_range(at..end, "");
            }
            check(&pt, &model);
        }
    }

    #[test]
    fn typing_coalesces_pieces() {
        let mut pt = PieceTable::new(String::new());
        for (i, c) in "abcdef".char_indices() {
            pt.insert(i, &c.to_string());
        }
        assert_eq!(pt.pieces.len(), 1);
        assert_eq!(pt.all(), "abcdef");
    }

    #[test]
    fn lines() {
        let pt = PieceTable::new("ab\ncd\nef".into());
        assert_eq!(pt.line_count(), 3);
        assert_eq!(pt.line_start(0), 0);
        assert_eq!(pt.line_start(1), 3);
        assert_eq!(pt.line_start(2), 6);
        assert_eq!(pt.line_of(0), 0);
        assert_eq!(pt.line_of(2), 0);
        assert_eq!(pt.line_of(3), 1);
        assert_eq!(pt.line_of(8), 2);
        assert_eq!(pt.next_newline(0), 2);
        assert_eq!(pt.next_newline(3), 5);
        assert_eq!(pt.next_newline(6), 8);
    }

    #[test]
    fn boundaries() {
        let pt = PieceTable::new("aδb".into());
        assert_eq!(pt.next_boundary(0), 1);
        assert_eq!(pt.next_boundary(1), 3);
        assert_eq!(pt.prev_boundary(3), 1);
        assert_eq!(pt.prev_boundary(1), 0);
        assert_eq!(pt.char_at(1), Some('δ'));
        assert_eq!(pt.char_at(4), None);
    }

    #[test]
    fn delete_across_pieces() {
        let mut pt = PieceTable::new("abc".into());
        pt.insert(3, "def");
        pt.insert(0, "xyz");
        assert_eq!(pt.all(), "xyzabcdef");
        assert_eq!(pt.delete(2, 7), "zabcd");
        assert_eq!(pt.all(), "xyef");
    }
}
