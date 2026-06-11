use crate::file;
use crate::forth;
use crate::term::Key;
use crate::text::PieceTable;
use crate::width;
use std::path::{Path, PathBuf};

#[derive(Clone)]
enum Op {
    Ins { at: usize, len: usize },
    Del { at: usize, text: String },
}

struct Undo {
    op: Op,
    cursor: usize,
    group: u64,
}

pub struct Buffer {
    pub text: PieceTable,
    pub name: String,
    pub path: Option<PathBuf>,
    pub cursor: usize,
    pub mark: Option<usize>,
    pub mark_active: bool,
    pub goal: Option<usize>,
    pub top: usize,
    pub left: usize,
    pub modified: bool,
    mode_bits: Option<u32>,
    undo: Vec<Undo>,
    undo_pos: usize,
    group: u64,
    in_undo: bool,
}

impl Buffer {
    pub fn new(name: String, path: Option<PathBuf>, content: String, mode_bits: Option<u32>) -> Buffer {
        Buffer {
            text: PieceTable::new(content),
            name,
            path,
            cursor: 0,
            mark: None,
            mark_active: false,
            goal: None,
            top: 0,
            left: 0,
            modified: false,
            mode_bits,
            undo: Vec::new(),
            undo_pos: 0,
            group: 0,
            in_undo: false,
        }
    }

    fn record(&mut self, op: Op) {
        self.undo.push(Undo { op, cursor: self.cursor, group: self.group });
        if !self.in_undo {
            self.undo_pos = self.undo.len();
        }
    }

    pub fn new_group(&mut self) {
        self.group += 1;
    }

    pub fn break_undo_chain(&mut self) {
        self.undo_pos = self.undo.len();
    }

    pub fn insert(&mut self, at: usize, s: &str) {
        if s.is_empty() {
            return;
        }
        self.text.insert(at, s);
        self.record(Op::Ins { at, len: s.len() });
        self.modified = true;
        self.mark_active = false;
        if let Some(m) = self.mark {
            if m > at {
                self.mark = Some(m + s.len());
            }
        }
        if self.cursor > at {
            self.cursor += s.len();
        }
    }

    pub fn delete(&mut self, start: usize, end: usize) -> String {
        if start >= end {
            return String::new();
        }
        let s = self.text.delete(start, end);
        self.record(Op::Del { at: start, text: s.clone() });
        self.modified = true;
        self.mark_active = false;
        let n = end - start;
        if let Some(m) = self.mark {
            self.mark = Some(if m <= start {
                m
            } else if m >= end {
                m - n
            } else {
                start
            });
        }
        if self.cursor >= end {
            self.cursor -= n;
        } else if self.cursor > start {
            self.cursor = start;
        }
        s
    }

    pub fn undo(&mut self) -> bool {
        if self.undo_pos == 0 {
            return false;
        }
        let gid = self.undo[self.undo_pos - 1].group;
        self.group += 1;
        self.in_undo = true;
        while self.undo_pos > 0 && self.undo[self.undo_pos - 1].group == gid {
            self.undo_pos -= 1;
            let cur = self.undo[self.undo_pos].cursor;
            let op = self.undo[self.undo_pos].op.clone();
            match op {
                Op::Ins { at, len } => {
                    self.delete(at, at + len);
                }
                Op::Del { at, text } => {
                    self.insert(at, &text);
                }
            }
            self.cursor = cur.min(self.text.len());
        }
        self.in_undo = false;
        true
    }

    pub fn line(&self) -> usize {
        self.text.line_of(self.cursor)
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        if !self.mark_active {
            return None;
        }
        self.mark
            .map(|m| (m.min(self.cursor), m.max(self.cursor)))
            .filter(|(a, z)| a != z)
    }

    pub fn reframe(&mut self, rows: usize, cols: usize) {
        let line = self.line();
        if line < self.top {
            self.top = line;
        }
        if line >= self.top + rows {
            self.top = line - rows + 1;
        }
        let col = width::display_col(&self.text, self.text.line_start(line), self.cursor);
        if col < self.left {
            self.left = col;
        }
        if col >= self.left + cols {
            self.left = col + 1 - cols;
        }
    }

    pub fn forward(&mut self) {
        self.cursor = self.text.next_boundary(self.cursor);
    }

    pub fn backward(&mut self) {
        self.cursor = self.text.prev_boundary(self.cursor);
    }

    pub fn vertical(&mut self, down: bool) {
        let line = self.line();
        let goal = match self.goal {
            Some(g) => g,
            None => {
                let g = width::display_col(&self.text, self.text.line_start(line), self.cursor);
                self.goal = Some(g);
                g
            }
        };
        let target = if down {
            if line + 1 >= self.text.line_count() {
                self.cursor = self.text.len();
                return;
            }
            line + 1
        } else {
            if line == 0 {
                self.cursor = 0;
                return;
            }
            line - 1
        };
        self.cursor = width::offset_at_col(&self.text, self.text.line_start(target), goal);
    }

    fn word_end(&self) -> usize {
        let mut at = self.cursor;
        while let Some(c) = self.text.char_at(at) {
            if c.is_alphanumeric() {
                break;
            }
            at += c.len_utf8();
        }
        while let Some(c) = self.text.char_at(at) {
            if !c.is_alphanumeric() {
                break;
            }
            at += c.len_utf8();
        }
        at
    }

    fn word_start(&self) -> usize {
        let mut at = self.cursor;
        loop {
            if at == 0 {
                return 0;
            }
            let p = self.text.prev_boundary(at);
            if self.text.char_at(p).is_some_and(|c| c.is_alphanumeric()) {
                break;
            }
            at = p;
        }
        loop {
            if at == 0 {
                return 0;
            }
            let p = self.text.prev_boundary(at);
            if !self.text.char_at(p).is_some_and(|c| c.is_alphanumeric()) {
                break;
            }
            at = p;
        }
        at
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cmd {
    Insert(char),
    Newline,
    Forward,
    Backward,
    Next,
    Prev,
    WordForward,
    WordBackward,
    LineStart,
    LineEnd,
    BufStart,
    BufEnd,
    PageDown,
    PageUp,
    Recenter,
    DeleteForward,
    DeleteBackward,
    KillLine,
    KillWordForward,
    KillWordBackward,
    SetMark,
    ExchangeMark,
    KillRegion,
    CopyRegion,
    Yank,
    YankPop,
    SearchForward,
    SearchBackward,
    QueryReplace,
    Undo,
    Save,
    WriteAs,
    FindFile,
    SwitchBuffer,
    KillBuffer,
    ListBuffers,
    QuitEditor,
    Repl,
    EvalPoint,
}

pub fn keymap(cx: bool, key: Key) -> Option<Cmd> {
    if cx {
        return match key {
            Key::Ctrl('s') => Some(Cmd::Save),
            Key::Ctrl('w') => Some(Cmd::WriteAs),
            Key::Ctrl('f') => Some(Cmd::FindFile),
            Key::Ctrl('c') => Some(Cmd::QuitEditor),
            Key::Ctrl('x') => Some(Cmd::ExchangeMark),
            Key::Ctrl('b') => Some(Cmd::ListBuffers),
            Key::Char('b') => Some(Cmd::SwitchBuffer),
            Key::Char('k') => Some(Cmd::KillBuffer),
            _ => None,
        };
    }
    match key {
        Key::Char(c) => Some(Cmd::Insert(c)),
        Key::Enter => Some(Cmd::Newline),
        Key::Tab => Some(Cmd::Insert('\t')),
        Key::Ctrl('f') | Key::Right => Some(Cmd::Forward),
        Key::Ctrl('b') | Key::Left => Some(Cmd::Backward),
        Key::Ctrl('n') | Key::Down => Some(Cmd::Next),
        Key::Ctrl('p') | Key::Up => Some(Cmd::Prev),
        Key::Meta('f') => Some(Cmd::WordForward),
        Key::Meta('b') => Some(Cmd::WordBackward),
        Key::Ctrl('a') | Key::Home => Some(Cmd::LineStart),
        Key::Ctrl('e') | Key::End => Some(Cmd::LineEnd),
        Key::Meta('<') => Some(Cmd::BufStart),
        Key::Meta('>') => Some(Cmd::BufEnd),
        Key::Ctrl('v') | Key::PageDown => Some(Cmd::PageDown),
        Key::Meta('v') | Key::PageUp => Some(Cmd::PageUp),
        Key::Ctrl('l') => Some(Cmd::Recenter),
        Key::Ctrl('d') | Key::Delete => Some(Cmd::DeleteForward),
        Key::Backspace => Some(Cmd::DeleteBackward),
        Key::Ctrl('k') => Some(Cmd::KillLine),
        Key::Meta('d') => Some(Cmd::KillWordForward),
        Key::MetaBackspace => Some(Cmd::KillWordBackward),
        Key::Ctrl(' ') => Some(Cmd::SetMark),
        Key::Ctrl('w') => Some(Cmd::KillRegion),
        Key::Meta('w') => Some(Cmd::CopyRegion),
        Key::Ctrl('y') => Some(Cmd::Yank),
        Key::Meta('y') => Some(Cmd::YankPop),
        Key::Ctrl('s') => Some(Cmd::SearchForward),
        Key::Ctrl('r') => Some(Cmd::SearchBackward),
        Key::Meta('%') => Some(Cmd::QueryReplace),
        Key::Ctrl('_') => Some(Cmd::Undo),
        Key::Meta('x') => Some(Cmd::Repl),
        Key::Meta(';') => Some(Cmd::EvalPoint),
        _ => None,
    }
}

pub struct Prompt {
    pub label: String,
    pub input: String,
    pub cur: usize,
    pub matches: String,
    pub yes_no: bool,
    action: PromptAction,
}

enum PromptAction {
    FindFile,
    WriteAs,
    GotoLine,
    SwitchBuffer,
    KillBufferConfirm,
    QuitConfirm,
    ReplaceFrom,
    ReplaceTo { from: String },
    Eval,
}

pub struct Search {
    pub forward: bool,
    pub needle: String,
    pub found: bool,
    origin: usize,
    at: usize,
}

pub struct Replace {
    pub from: String,
    pub to: String,
    at: usize,
    count: usize,
}

pub enum Mode {
    Edit,
    Prompt(Prompt),
    Search(Search),
    Replace(Replace),
}

#[derive(Clone, Copy, PartialEq)]
enum Last {
    Insert,
    Kill,
    Yank,
    LineMove,
    Other,
}

pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub current: usize,
    pub mode: Mode,
    pub echo: String,
    pub rows: usize,
    pub cols: usize,
    pub quit: bool,
    last_buffer: usize,
    kills: Vec<String>,
    kill_idx: usize,
    yank: Option<(usize, usize)>,
    cx: bool,
    mg: bool,
    arg: Option<usize>,
    arg_digits: bool,
    last: Last,
    last_needle: String,
    pub vm: forth::Vm,
}

struct EvalCtx<'a> {
    buf: &'a mut Buffer,
    echo: &'a mut String,
}

impl EvalCtx<'_> {
    fn at(&self, n: i64) -> usize {
        let n = n.clamp(0, self.buf.text.len() as i64) as usize;
        self.buf.text.snap(n)
    }

    fn range(&self, a: i64, b: i64) -> (usize, usize) {
        let (a, b) = (self.at(a), self.at(b));
        (a.min(b), a.max(b))
    }
}

impl forth::Host for EvalCtx<'_> {
    fn len(&self) -> i64 {
        self.buf.text.len() as i64
    }

    fn cursor(&self) -> i64 {
        self.buf.cursor as i64
    }

    fn set_cursor(&mut self, at: i64) {
        self.buf.cursor = self.at(at);
    }

    fn mark(&self) -> i64 {
        self.buf.mark.map_or(-1, |m| m as i64)
    }

    fn set_mark(&mut self, at: i64) {
        if at < 0 {
            self.buf.mark = None;
            self.buf.mark_active = false;
        } else {
            self.buf.mark = Some(self.at(at));
            self.buf.mark_active = true;
        }
    }

    fn selection(&self) -> (i64, i64) {
        match self.buf.selection() {
            Some((a, b)) => (a as i64, b as i64),
            None => (self.buf.cursor as i64, self.buf.cursor as i64),
        }
    }

    fn slice(&self, a: i64, b: i64) -> String {
        let (a, b) = self.range(a, b);
        self.buf.text.slice(a, b)
    }

    fn insert(&mut self, s: &str) {
        let at = self.buf.cursor;
        self.buf.insert(at, s);
        self.buf.cursor = at + s.len();
    }

    fn delete(&mut self, a: i64, b: i64) {
        let (a, b) = self.range(a, b);
        self.buf.delete(a, b);
    }

    fn line(&self) -> i64 {
        self.buf.line() as i64
    }

    fn lines(&self) -> i64 {
        self.buf.text.line_count() as i64
    }

    fn line_start(&self, l: i64) -> i64 {
        let l = l.clamp(0, self.lines() - 1) as usize;
        self.buf.text.line_start(l) as i64
    }

    fn line_end(&self, l: i64) -> i64 {
        let ls = self.line_start(l) as usize;
        self.buf.text.next_newline(ls) as i64
    }

    fn search(&self, s: &str, fwd: bool) -> i64 {
        let hay = self.buf.text.all();
        let c = self.buf.cursor;
        if fwd {
            hay[c..].find(s).map_or(-1, |i| (c + i) as i64)
        } else {
            hay[..c].rfind(s).map_or(-1, |i| i as i64)
        }
    }

    fn message(&mut self, s: &str) {
        *self.echo = s.to_string();
    }
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn form_at(text: &str, cur: usize) -> Option<String> {
    let toks = forth::lex(text).ok()?;
    let mut i = 0;
    while i < toks.len() {
        if toks[i].1 == ":" {
            let start = toks[i].0;
            let mut j = i + 1;
            while j < toks.len() && toks[j].1 != ";" {
                j += 1;
            }
            if j < toks.len() {
                let end = toks[j].0 + 1;
                if cur >= start && cur <= end {
                    return Some(text[start..end].to_string());
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    let mut before: Option<&(usize, String)> = None;
    for t in &toks {
        let end = t.0 + t.1.len();
        if cur >= t.0 && cur <= end {
            return Some(t.1.clone());
        }
        if end <= cur {
            before = Some(t);
        }
    }
    before.map(|t| t.1.clone())
}

fn common_prefix(items: &[String]) -> String {
    let mut p = items[0].clone();
    for s in &items[1..] {
        while !s.starts_with(p.as_str()) {
            p.pop();
        }
    }
    p
}

fn buffer_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl Editor {
    pub fn new() -> Editor {
        Editor {
            buffers: Vec::new(),
            current: 0,
            mode: Mode::Edit,
            echo: String::new(),
            rows: 24,
            cols: 80,
            quit: false,
            last_buffer: 0,
            kills: Vec::new(),
            kill_idx: 0,
            yank: None,
            cx: false,
            mg: false,
            arg: None,
            arg_digits: false,
            last: Last::Other,
            last_needle: String::new(),
            vm: forth::Vm::new(),
        }
    }

    pub fn eval_forth(&mut self, src: &str) {
        let b = &mut self.buffers[self.current];
        b.new_group();
        b.break_undo_chain();
        let mut echo = String::new();
        let r = self.vm.run(src, &mut EvalCtx { buf: &mut self.buffers[self.current], echo: &mut echo });
        self.echo = match r {
            Ok(()) => {
                if echo.is_empty() {
                    self.vm.show()
                } else {
                    echo
                }
            }
            Err(e) => e,
        };
    }

    fn eval_at_point(&mut self) {
        let b = self.buf();
        match form_at(&b.text.all(), b.cursor) {
            Some(src) => self.eval_forth(&src),
            None => self.echo = "Nothing to evaluate here".into(),
        }
    }

    pub fn ensure_buffer(&mut self) {
        if self.buffers.is_empty() {
            self.buffers.push(Buffer::new("*scratch*".into(), None, String::new(), None));
            self.current = 0;
            self.last_buffer = 0;
        }
    }

    pub fn buf(&self) -> &Buffer {
        &self.buffers[self.current]
    }

    pub fn buf_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.current]
    }

    pub fn reframe(&mut self) {
        if self.rows >= 3 && self.cols >= 8 {
            let (rows, cols) = (self.rows - 2, self.cols);
            self.buf_mut().reframe(rows, cols);
        }
    }

    fn add_buffer(&mut self, b: Buffer) {
        if !self.buffers.is_empty() {
            self.last_buffer = self.current;
        }
        self.buffers.push(b);
        self.current = self.buffers.len() - 1;
    }

    pub fn find_file(&mut self, path: &Path) {
        let canon = file::canonical(path);
        if let Some(i) = self.buffers.iter().position(|b| b.path.as_deref() == Some(&canon)) {
            self.last_buffer = self.current;
            self.current = i;
            return;
        }
        match file::load(path) {
            Ok(l) => {
                let name = buffer_name(&l.path);
                self.add_buffer(Buffer::new(name, Some(l.path), l.content, l.mode));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.add_buffer(Buffer::new(buffer_name(&canon), Some(canon), String::new(), None));
                self.echo = "(New file)".into();
            }
            Err(e) => self.echo = format!("{}: {}", path.display(), e),
        }
    }

    pub fn handle_key(&mut self, key: Key) {
        self.echo.clear();
        match std::mem::replace(&mut self.mode, Mode::Edit) {
            Mode::Edit => self.edit_key(key),
            Mode::Prompt(p) => self.prompt_key(p, key),
            Mode::Search(s) => self.search_key(s, key),
            Mode::Replace(r) => self.replace_key(r, key),
        }
    }

    fn edit_key(&mut self, key: Key) {
        if key == Key::Ctrl('g') {
            self.cx = false;
            self.mg = false;
            self.arg = None;
            self.last = Last::Other;
            self.buf_mut().mark_active = false;
            self.echo = "Quit".into();
            return;
        }
        if self.mg {
            self.mg = false;
            if matches!(key, Key::Char('g') | Key::Meta('g')) {
                self.start_prompt(PromptAction::GotoLine, "Goto line: ");
            } else {
                self.echo = "M-g is undefined here".into();
            }
            return;
        }
        if !self.cx {
            if key == Key::Ctrl('u') {
                self.arg = Some(self.arg.map_or(4, |n| n.saturating_mul(4)));
                self.arg_digits = false;
                return;
            }
            if let (Some(n), Key::Char(c @ '0'..='9')) = (self.arg, key) {
                let d = c as usize - '0' as usize;
                self.arg = Some(if self.arg_digits { n.saturating_mul(10) + d } else { d });
                self.arg_digits = true;
                return;
            }
            if key == Key::Ctrl('x') {
                self.cx = true;
                return;
            }
            if key == Key::Meta('g') {
                self.mg = true;
                return;
            }
        }
        let cx = std::mem::take(&mut self.cx);
        match keymap(cx, key) {
            Some(cmd) => self.run(cmd),
            None => {
                self.arg = None;
                self.last = Last::Other;
                self.echo = format!("{:?} is undefined", key);
            }
        }
    }

    fn run(&mut self, cmd: Cmd) {
        let n = self.arg.take().unwrap_or(1).max(1);
        self.arg_digits = false;
        let coalesce = matches!(cmd, Cmd::Insert(_)) && self.last == Last::Insert;
        if !coalesce {
            self.buf_mut().new_group();
        }
        if cmd != Cmd::Undo {
            self.buf_mut().break_undo_chain();
        }
        match cmd {
            Cmd::Insert(c) => self.insert_char(c, n),
            Cmd::Newline => self.insert_char('\n', n),
            Cmd::Forward => self.repeat(n, |b| b.forward()),
            Cmd::Backward => self.repeat(n, |b| b.backward()),
            Cmd::Next => self.repeat(n, |b| b.vertical(true)),
            Cmd::Prev => self.repeat(n, |b| b.vertical(false)),
            Cmd::WordForward => self.repeat(n, |b| b.cursor = b.word_end()),
            Cmd::WordBackward => self.repeat(n, |b| b.cursor = b.word_start()),
            Cmd::LineStart => {
                let b = self.buf_mut();
                b.cursor = b.text.line_start(b.line());
            }
            Cmd::LineEnd => {
                let b = self.buf_mut();
                b.cursor = b.text.next_newline(b.cursor);
            }
            Cmd::BufStart => self.buf_mut().cursor = 0,
            Cmd::BufEnd => {
                let b = self.buf_mut();
                b.cursor = b.text.len();
            }
            Cmd::PageDown => self.page(n, true),
            Cmd::PageUp => self.page(n, false),
            Cmd::Recenter => {
                let half = self.rows.saturating_sub(2) / 2;
                let b = self.buf_mut();
                b.top = b.line().saturating_sub(half);
            }
            Cmd::DeleteForward => self.repeat(n, |b| {
                let end = b.text.next_boundary(b.cursor);
                b.delete(b.cursor, end);
            }),
            Cmd::DeleteBackward => self.repeat(n, |b| {
                let start = b.text.prev_boundary(b.cursor);
                b.delete(start, b.cursor);
            }),
            Cmd::KillLine => self.kill_line(n),
            Cmd::KillWordForward => self.kill_words(n, true),
            Cmd::KillWordBackward => self.kill_words(n, false),
            Cmd::SetMark => {
                let b = self.buf_mut();
                b.mark = Some(b.cursor);
                b.mark_active = true;
                self.echo = "Mark set".into();
            }
            Cmd::ExchangeMark => {
                let b = self.buf_mut();
                match b.mark {
                    Some(m) => {
                        b.mark = Some(b.cursor);
                        b.mark_active = true;
                        b.cursor = m.min(b.text.len());
                    }
                    None => self.echo = "No mark set in this buffer".into(),
                }
            }
            Cmd::KillRegion => match self.region() {
                Some((a, b)) => self.kill(a, b, true),
                None => self.echo = "The mark is not set now".into(),
            },
            Cmd::CopyRegion => match self.region() {
                Some((a, b)) => {
                    let s = self.buf().text.slice(a, b);
                    self.push_kill(s, true);
                    self.buf_mut().mark_active = false;
                }
                None => self.echo = "The mark is not set now".into(),
            },
            Cmd::Yank => self.yank_cmd(),
            Cmd::YankPop => self.yank_pop(),
            Cmd::SearchForward => self.start_search(true),
            Cmd::SearchBackward => self.start_search(false),
            Cmd::QueryReplace => self.start_prompt(PromptAction::ReplaceFrom, "Query replace: "),
            Cmd::Undo => {
                for _ in 0..n {
                    if !self.buf_mut().undo() {
                        self.echo = "No further undo information".into();
                        break;
                    }
                }
                if self.echo.is_empty() {
                    self.echo = "Undo".into();
                }
            }
            Cmd::Save => self.save(),
            Cmd::WriteAs => self.start_prompt(PromptAction::WriteAs, "Write file: "),
            Cmd::FindFile => self.start_prompt(PromptAction::FindFile, "Find file: "),
            Cmd::SwitchBuffer => {
                let default = self.buffers[self.last_buffer.min(self.buffers.len() - 1)].name.clone();
                self.start_prompt(PromptAction::SwitchBuffer, format!("Switch to buffer (default {}): ", default));
            }
            Cmd::KillBuffer => {
                if self.buf().modified {
                    let label = format!("Buffer {} modified; kill anyway? (y or n) ", self.buf().name);
                    self.start_yes_no(PromptAction::KillBufferConfirm, label);
                } else {
                    self.kill_current_buffer();
                }
            }
            Cmd::Repl => self.start_prompt(PromptAction::Eval, "cmd: "),
            Cmd::EvalPoint => self.eval_at_point(),
            Cmd::ListBuffers => {
                self.echo = self
                    .buffers
                    .iter()
                    .map(|b| format!("{}{}", b.name, if b.modified { "*" } else { "" }))
                    .collect::<Vec<_>>()
                    .join("  ");
            }
            Cmd::QuitEditor => {
                if self.buffers.iter().any(|b| b.modified) {
                    self.start_yes_no(PromptAction::QuitConfirm, "Modified buffers exist; really exit? (y or n) ".into());
                } else {
                    self.quit = true;
                }
            }
        }
        self.last = match cmd {
            Cmd::Insert(_) | Cmd::Newline => Last::Insert,
            Cmd::KillLine | Cmd::KillRegion | Cmd::KillWordForward | Cmd::KillWordBackward => Last::Kill,
            Cmd::Yank | Cmd::YankPop => Last::Yank,
            Cmd::Next | Cmd::Prev | Cmd::PageDown | Cmd::PageUp => Last::LineMove,
            _ => Last::Other,
        };
        if self.last != Last::LineMove {
            self.buf_mut().goal = None;
        }
    }

    fn repeat(&mut self, n: usize, f: impl Fn(&mut Buffer)) {
        let b = self.buf_mut();
        for _ in 0..n {
            f(b);
        }
    }

    fn insert_char(&mut self, c: char, n: usize) {
        let s: String = std::iter::repeat_n(c, n).collect();
        let b = self.buf_mut();
        let at = b.cursor;
        b.insert(at, &s);
        b.cursor = at + s.len();
    }

    fn page(&mut self, n: usize, down: bool) {
        let step = self.rows.saturating_sub(4).max(1);
        self.repeat(step * n, |b| b.vertical(down));
    }

    fn region(&self) -> Option<(usize, usize)> {
        let b = self.buf();
        b.mark.map(|m| {
            let m = m.min(b.text.len());
            (m.min(b.cursor), m.max(b.cursor))
        })
    }

    fn kill(&mut self, start: usize, end: usize, forward: bool) {
        let s = self.buf_mut().delete(start, end);
        if !s.is_empty() {
            self.push_kill(s, forward);
        }
    }

    fn push_kill(&mut self, s: String, forward: bool) {
        if self.last == Last::Kill && !self.kills.is_empty() {
            if forward {
                self.kills[0].push_str(&s);
            } else {
                self.kills[0].insert_str(0, &s);
            }
        } else {
            self.kills.insert(0, s);
            self.kills.truncate(16);
        }
        self.kill_idx = 0;
    }

    fn kill_line(&mut self, n: usize) {
        for _ in 0..n {
            let b = self.buf();
            let cur = b.cursor;
            let le = b.text.next_newline(cur);
            let end = if n > 1 || cur == le { (le + 1).min(b.text.len()) } else { le };
            if end <= cur {
                break;
            }
            self.kill(cur, end, true);
            self.last = Last::Kill;
        }
    }

    fn kill_words(&mut self, n: usize, forward: bool) {
        for _ in 0..n {
            let b = self.buf();
            let (start, end) = if forward { (b.cursor, b.word_end()) } else { (b.word_start(), b.cursor) };
            if start == end {
                break;
            }
            self.kill(start, end, forward);
            self.last = Last::Kill;
        }
    }

    fn yank_cmd(&mut self) {
        if self.kills.is_empty() {
            self.echo = "Kill ring is empty".into();
            return;
        }
        self.kill_idx = 0;
        let s = self.kills[0].clone();
        let b = self.buf_mut();
        let at = b.cursor;
        b.mark = Some(at);
        b.insert(at, &s);
        b.cursor = at + s.len();
        self.yank = Some((at, at + s.len()));
    }

    fn yank_pop(&mut self) {
        let Some((a, e)) = self.yank.filter(|_| self.last == Last::Yank && !self.kills.is_empty()) else {
            self.echo = "Previous command was not a yank".into();
            return;
        };
        self.kill_idx = (self.kill_idx + 1) % self.kills.len();
        let s = self.kills[self.kill_idx].clone();
        let b = self.buf_mut();
        b.delete(a, e);
        b.insert(a, &s);
        b.cursor = a + s.len();
        self.yank = Some((a, a + s.len()));
    }

    fn start_search(&mut self, forward: bool) {
        let cursor = self.buf().cursor;
        self.mode = Mode::Search(Search { forward, needle: String::new(), found: true, origin: cursor, at: cursor });
    }

    fn search_key(&mut self, mut s: Search, key: Key) {
        match key {
            Key::Char(c) => {
                s.needle.push(c);
                self.search_move(&mut s, false);
                self.mode = Mode::Search(s);
            }
            Key::Ctrl('s') | Key::Ctrl('r') => {
                s.forward = key == Key::Ctrl('s');
                if s.needle.is_empty() {
                    s.needle = self.last_needle.clone();
                    self.search_move(&mut s, false);
                } else {
                    self.search_move(&mut s, true);
                }
                self.mode = Mode::Search(s);
            }
            Key::Backspace => {
                s.needle.pop();
                s.at = s.origin;
                if s.needle.is_empty() {
                    self.buf_mut().cursor = s.origin;
                    s.found = true;
                } else {
                    self.buf_mut().cursor = s.origin;
                    self.search_move(&mut s, false);
                }
                self.mode = Mode::Search(s);
            }
            Key::Ctrl('g') => {
                self.buf_mut().cursor = s.origin;
                self.echo = "Quit".into();
            }
            Key::Enter => self.finish_search(s),
            _ => {
                self.finish_search(s);
                self.edit_key(key);
            }
        }
    }

    fn finish_search(&mut self, s: Search) {
        if !s.needle.is_empty() {
            self.last_needle = s.needle;
        }
    }

    fn search_move(&mut self, s: &mut Search, advance: bool) {
        if s.needle.is_empty() {
            return;
        }
        let hay = self.buf().text.all();
        let pos = if s.forward {
            let from = if advance { ceil_boundary(&hay, s.at + 1) } else { s.at.min(hay.len()) };
            hay.get(from..).and_then(|h| h.find(&s.needle)).map(|i| from + i)
        } else {
            let limit = if advance {
                if s.at == 0 {
                    s.found = false;
                    return;
                }
                floor_boundary(&hay, s.at - 1)
            } else {
                s.at.min(hay.len())
            };
            let end = ceil_boundary(&hay, limit + s.needle.len());
            let p = hay[..end].rfind(&s.needle);
            if advance {
                p.filter(|&p| p < s.at)
            } else {
                p
            }
        };
        match pos {
            Some(p) => {
                s.at = p;
                s.found = true;
                self.buf_mut().cursor = if s.forward { p + s.needle.len() } else { p };
            }
            None => s.found = false,
        }
    }

    fn start_prompt(&mut self, action: PromptAction, label: impl Into<String>) {
        self.mode = Mode::Prompt(Prompt {
            label: label.into(),
            input: String::new(),
            cur: 0,
            matches: String::new(),
            yes_no: false,
            action,
        });
    }

    fn start_yes_no(&mut self, action: PromptAction, label: String) {
        self.mode = Mode::Prompt(Prompt {
            label,
            input: String::new(),
            cur: 0,
            matches: String::new(),
            yes_no: true,
            action,
        });
    }

    fn prompt_key(&mut self, mut p: Prompt, key: Key) {
        if p.yes_no {
            match key {
                Key::Char('y') => self.prompt_submit(p.action, "y".into()),
                Key::Char('n') | Key::Ctrl('g') => self.echo = "Quit".into(),
                _ => self.mode = Mode::Prompt(p),
            }
            return;
        }
        p.matches.clear();
        match key {
            Key::Enter => {
                self.prompt_submit(p.action, p.input);
                return;
            }
            Key::Ctrl('g') => {
                self.echo = "Quit".into();
                return;
            }
            Key::Tab => self.complete_prompt(&mut p),
            Key::Char(c) => {
                p.input.insert(p.cur, c);
                p.cur += c.len_utf8();
            }
            Key::Backspace => {
                if let Some(c) = p.input[..p.cur].chars().last() {
                    p.cur -= c.len_utf8();
                    p.input.remove(p.cur);
                }
            }
            Key::Ctrl('d') | Key::Delete => {
                if p.cur < p.input.len() {
                    p.input.remove(p.cur);
                }
            }
            Key::Ctrl('a') | Key::Home => p.cur = 0,
            Key::Ctrl('e') | Key::End => p.cur = p.input.len(),
            Key::Ctrl('f') | Key::Right => {
                if let Some(c) = p.input[p.cur..].chars().next() {
                    p.cur += c.len_utf8();
                }
            }
            Key::Ctrl('b') | Key::Left => {
                if let Some(c) = p.input[..p.cur].chars().last() {
                    p.cur -= c.len_utf8();
                }
            }
            Key::Ctrl('k') => p.input.truncate(p.cur),
            _ => {}
        }
        self.mode = Mode::Prompt(p);
    }

    fn complete_prompt(&mut self, p: &mut Prompt) {
        let (cands, strip) = match &p.action {
            PromptAction::SwitchBuffer => (
                self.buffers
                    .iter()
                    .map(|b| b.name.clone())
                    .filter(|n| n.starts_with(&p.input))
                    .collect::<Vec<_>>(),
                0,
            ),
            PromptAction::FindFile | PromptAction::WriteAs => {
                let strip = p.input.rfind('/').map_or(0, |i| i + 1);
                (file::complete(&p.input), strip)
            }
            PromptAction::Eval => {
                let strip = p.input.rfind(char::is_whitespace).map_or(0, |i| i + 1);
                let prefix = &p.input[strip..];
                let mut cands: Vec<String> = forth::PRIMS
                    .iter()
                    .copied()
                    .chain(self.vm.names())
                    .filter(|n| n.starts_with(prefix))
                    .map(|n| format!("{}{}", &p.input[..strip], n))
                    .collect();
                cands.sort();
                cands.dedup();
                (cands, strip)
            }
            _ => {
                p.matches = "[No completion]".into();
                return;
            }
        };
        if cands.is_empty() {
            p.matches = "[No match]".into();
            return;
        }
        let lcp = common_prefix(&cands);
        if lcp.len() > p.input.len() {
            p.input = lcp;
            p.cur = p.input.len();
        } else if cands.len() == 1 {
            p.matches = "[Sole completion]".into();
        } else {
            p.matches = cands.iter().map(|c| &c[strip..]).collect::<Vec<_>>().join("  ");
        }
    }

    fn prompt_submit(&mut self, action: PromptAction, input: String) {
        match action {
            PromptAction::FindFile => {
                if input.is_empty() {
                    self.echo = "Quit".into();
                } else {
                    self.find_file(Path::new(&input));
                }
            }
            PromptAction::WriteAs => {
                if input.is_empty() {
                    self.echo = "Quit".into();
                } else {
                    self.write_as(PathBuf::from(input));
                }
            }
            PromptAction::GotoLine => match input.trim().parse::<usize>() {
                Ok(line) if line >= 1 => {
                    let b = self.buf_mut();
                    let target = (line - 1).min(b.text.line_count() - 1);
                    b.cursor = b.text.line_start(target);
                }
                _ => self.echo = "Invalid line number".into(),
            },
            PromptAction::SwitchBuffer => {
                let idx = if input.is_empty() {
                    Some(self.last_buffer.min(self.buffers.len() - 1))
                } else {
                    self.buffers.iter().position(|b| b.name == input)
                };
                match idx {
                    Some(i) => {
                        self.last_buffer = self.current;
                        self.current = i;
                    }
                    None => self.echo = format!("No buffer named {}", input),
                }
            }
            PromptAction::KillBufferConfirm => self.kill_current_buffer(),
            PromptAction::QuitConfirm => self.quit = true,
            PromptAction::ReplaceFrom => {
                if input.is_empty() {
                    self.echo = "Quit".into();
                } else {
                    let label = format!("Query replace {} with: ", input);
                    self.start_prompt(PromptAction::ReplaceTo { from: input }, label);
                }
            }
            PromptAction::ReplaceTo { from } => self.start_replace(from, input),
            PromptAction::Eval => {
                if !input.is_empty() {
                    self.eval_forth(&input);
                }
            }
        }
    }

    fn save(&mut self) {
        let b = self.buf();
        match b.path.clone() {
            None => self.start_prompt(PromptAction::WriteAs, "Write file: "),
            Some(p) => {
                if !b.modified {
                    self.echo = "(No changes need to be saved)".into();
                    return;
                }
                match file::save(&p, &b.text.all(), b.mode_bits) {
                    Ok(()) => {
                        self.buf_mut().modified = false;
                        self.echo = format!("Wrote {}", p.display());
                    }
                    Err(e) => self.echo = format!("Save failed: {}", e),
                }
            }
        }
    }

    fn write_as(&mut self, path: PathBuf) {
        let canon = file::canonical(&path);
        let b = self.buf();
        match file::save(&canon, &b.text.all(), b.mode_bits) {
            Ok(()) => {
                let name = buffer_name(&canon);
                let b = self.buf_mut();
                b.path = Some(canon.clone());
                b.name = name;
                b.modified = false;
                self.echo = format!("Wrote {}", canon.display());
            }
            Err(e) => self.echo = format!("Save failed: {}", e),
        }
    }

    fn kill_current_buffer(&mut self) {
        let idx = self.current;
        self.buffers.remove(idx);
        if self.last_buffer > idx {
            self.last_buffer -= 1;
        }
        self.ensure_buffer();
        if self.last_buffer >= self.buffers.len() {
            self.last_buffer = 0;
        }
        self.current = self.last_buffer;
    }

    fn start_replace(&mut self, from: String, to: String) {
        if from.is_empty() {
            self.echo = "Empty search string".into();
            return;
        }
        let r = Replace { from, to, at: self.buf().cursor, count: 0 };
        let from_pos = self.buf().cursor;
        self.replace_next(r, from_pos);
    }

    fn replace_next(&mut self, mut r: Replace, from: usize) {
        let hay = self.buf().text.all();
        let from = ceil_boundary(&hay, from);
        match hay[from..].find(&r.from) {
            Some(i) => {
                r.at = from + i;
                self.buf_mut().cursor = r.at + r.from.len();
                self.mode = Mode::Replace(r);
            }
            None => self.echo = format!("Replaced {} occurrences", r.count),
        }
    }

    fn do_replace(&mut self, r: &mut Replace) -> usize {
        self.buf_mut().new_group();
        let end = r.at + r.from.len();
        self.buf_mut().delete(r.at, end);
        let at = r.at;
        let to = r.to.clone();
        self.buf_mut().insert(at, &to);
        self.buf_mut().cursor = at + to.len();
        r.count += 1;
        at + to.len()
    }

    fn replace_key(&mut self, mut r: Replace, key: Key) {
        match key {
            Key::Char('y') | Key::Char(' ') => {
                let next = self.do_replace(&mut r);
                self.replace_next(r, next);
            }
            Key::Char('n') => {
                let next = r.at + r.from.len();
                self.replace_next(r, next);
            }
            Key::Char('!') => {
                let mut next = self.do_replace(&mut r);
                loop {
                    let hay = self.buf().text.all();
                    let from = ceil_boundary(&hay, next);
                    match hay[from..].find(&r.from) {
                        Some(i) => {
                            r.at = from + i;
                            next = self.do_replace(&mut r);
                        }
                        None => break,
                    }
                }
                self.echo = format!("Replaced {} occurrences", r.count);
            }
            Key::Enter | Key::Char('q') | Key::Ctrl('g') => {
                self.echo = format!("Replaced {} occurrences", r.count);
            }
            _ => self.mode = Mode::Replace(r),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(content: &str) -> Editor {
        let mut e = Editor::new();
        e.buffers.push(Buffer::new("test".into(), None, content.into(), None));
        e
    }

    fn keys(e: &mut Editor, ks: &[Key]) {
        for &k in ks {
            e.handle_key(k);
        }
    }

    fn typing(e: &mut Editor, s: &str) {
        for c in s.chars() {
            e.handle_key(if c == '\n' { Key::Enter } else { Key::Char(c) });
        }
    }

    #[test]
    fn insert_and_move() {
        let mut e = ed("");
        typing(&mut e, "hello\nworld");
        assert_eq!(e.buf().text.all(), "hello\nworld");
        assert_eq!(e.buf().cursor, 11);
        keys(&mut e, &[Key::Ctrl('a')]);
        assert_eq!(e.buf().cursor, 6);
        keys(&mut e, &[Key::Ctrl('p'), Key::Ctrl('e')]);
        assert_eq!(e.buf().cursor, 5);
        keys(&mut e, &[Key::Meta('<')]);
        assert_eq!(e.buf().cursor, 0);
        keys(&mut e, &[Key::Meta('f')]);
        assert_eq!(e.buf().cursor, 5);
    }

    #[test]
    fn kill_and_yank() {
        let mut e = ed("one two\nthree");
        keys(&mut e, &[Key::Ctrl('k')]);
        assert_eq!(e.buf().text.all(), "\nthree");
        keys(&mut e, &[Key::Ctrl('y')]);
        assert_eq!(e.buf().text.all(), "one two\nthree");
        assert_eq!(e.buf().cursor, 7);
    }

    #[test]
    fn kill_coalesces() {
        let mut e = ed("ab\ncd\nef");
        keys(&mut e, &[Key::Ctrl('k'), Key::Ctrl('k')]);
        assert_eq!(e.buf().text.all(), "cd\nef");
        keys(&mut e, &[Key::Ctrl('y')]);
        assert_eq!(e.buf().text.all(), "ab\ncd\nef");
    }

    #[test]
    fn region_and_yank_pop() {
        let mut e = ed("abcdef");
        keys(&mut e, &[Key::Ctrl(' ')]);
        keys(&mut e, &[Key::Ctrl('f'), Key::Ctrl('f'), Key::Ctrl('f')]);
        keys(&mut e, &[Key::Ctrl('w')]);
        assert_eq!(e.buf().text.all(), "def");
        typing(&mut e, "x");
        keys(&mut e, &[Key::Ctrl('k')]);
        assert_eq!(e.buf().text.all(), "x");
        keys(&mut e, &[Key::Ctrl('y')]);
        assert_eq!(e.buf().text.all(), "xdef");
        keys(&mut e, &[Key::Meta('y')]);
        assert_eq!(e.buf().text.all(), "xabc");
    }

    #[test]
    fn undo_groups_and_chain() {
        let mut e = ed("");
        typing(&mut e, "abc");
        keys(&mut e, &[Key::Ctrl('f')]);
        typing(&mut e, "def");
        assert_eq!(e.buf().text.all(), "abcdef");
        keys(&mut e, &[Key::Ctrl('_')]);
        assert_eq!(e.buf().text.all(), "abc");
        keys(&mut e, &[Key::Ctrl('_')]);
        assert_eq!(e.buf().text.all(), "");
        keys(&mut e, &[Key::Ctrl('f')]);
        keys(&mut e, &[Key::Ctrl('_'), Key::Ctrl('_')]);
        assert_eq!(e.buf().text.all(), "abcdef");
    }

    #[test]
    fn undo_delete() {
        let mut e = ed("hello");
        keys(&mut e, &[Key::Ctrl('k')]);
        assert_eq!(e.buf().text.all(), "");
        keys(&mut e, &[Key::Ctrl('_')]);
        assert_eq!(e.buf().text.all(), "hello");
        assert_eq!(e.buf().cursor, 0);
    }

    #[test]
    fn isearch() {
        let mut e = ed("alpha beta alpha");
        keys(&mut e, &[Key::Ctrl('s')]);
        typing(&mut e, "alpha");
        assert_eq!(e.buf().cursor, 5);
        keys(&mut e, &[Key::Ctrl('s')]);
        assert_eq!(e.buf().cursor, 16);
        keys(&mut e, &[Key::Enter]);
        keys(&mut e, &[Key::Ctrl('r')]);
        keys(&mut e, &[Key::Ctrl('r')]);
        assert_eq!(e.buf().cursor, 11);
        keys(&mut e, &[Key::Ctrl('g')]);
        assert_eq!(e.buf().cursor, 16);
    }

    #[test]
    fn isearch_abort_restores() {
        let mut e = ed("foo bar");
        keys(&mut e, &[Key::Ctrl('s')]);
        typing(&mut e, "bar");
        assert_eq!(e.buf().cursor, 7);
        keys(&mut e, &[Key::Ctrl('g')]);
        assert_eq!(e.buf().cursor, 0);
    }

    #[test]
    fn query_replace() {
        let mut e = ed("cat dog cat dog cat");
        keys(&mut e, &[Key::Meta('%')]);
        typing(&mut e, "cat");
        keys(&mut e, &[Key::Enter]);
        typing(&mut e, "owl");
        keys(&mut e, &[Key::Enter]);
        keys(&mut e, &[Key::Char('y')]);
        keys(&mut e, &[Key::Char('n')]);
        keys(&mut e, &[Key::Char('!')]);
        assert_eq!(e.buf().text.all(), "owl dog cat dog owl");
        assert!(e.echo.contains("Replaced 2"));
    }

    #[test]
    fn numeric_argument() {
        let mut e = ed("");
        keys(&mut e, &[Key::Ctrl('u')]);
        typing(&mut e, "x");
        assert_eq!(e.buf().text.all(), "xxxx");
        keys(&mut e, &[Key::Ctrl('u')]);
        typing(&mut e, "12y");
        assert_eq!(e.buf().text.all(), "xxxxyyyyyyyyyyyy");
    }

    #[test]
    fn goto_line() {
        let mut e = ed("a\nb\nc\nd");
        keys(&mut e, &[Key::Meta('g'), Key::Char('g')]);
        typing(&mut e, "3");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.buf().cursor, 4);
    }

    #[test]
    fn buffer_switching() {
        let mut e = ed("first");
        e.buffers.push(Buffer::new("second".into(), None, "two".into(), None));
        e.handle_key(Key::Ctrl('x'));
        e.handle_key(Key::Char('b'));
        typing(&mut e, "second");
        e.handle_key(Key::Enter);
        assert_eq!(e.buf().name, "second");
        e.handle_key(Key::Ctrl('x'));
        e.handle_key(Key::Char('b'));
        e.handle_key(Key::Enter);
        assert_eq!(e.buf().name, "test");
    }

    #[test]
    fn kill_buffer_keeps_one() {
        let mut e = ed("only");
        keys(&mut e, &[Key::Ctrl('x'), Key::Char('k')]);
        assert_eq!(e.buffers.len(), 1);
        assert_eq!(e.buf().name, "*scratch*");
    }

    #[test]
    fn quit_unmodified() {
        let mut e = ed("clean");
        keys(&mut e, &[Key::Ctrl('x'), Key::Ctrl('c')]);
        assert!(e.quit);
    }

    #[test]
    fn quit_modified_confirms() {
        let mut e = ed("");
        typing(&mut e, "dirty");
        keys(&mut e, &[Key::Ctrl('x'), Key::Ctrl('c')]);
        assert!(!e.quit);
        keys(&mut e, &[Key::Char('y')]);
        assert!(e.quit);
    }

    #[test]
    fn word_kill_backward() {
        let mut e = ed("one two three");
        keys(&mut e, &[Key::Meta('>'), Key::MetaBackspace]);
        assert_eq!(e.buf().text.all(), "one two ");
        keys(&mut e, &[Key::MetaBackspace]);
        assert_eq!(e.buf().text.all(), "one ");
        keys(&mut e, &[Key::Ctrl('y')]);
        assert_eq!(e.buf().text.all(), "one two three");
    }

    #[test]
    fn mark_activation_lifecycle() {
        let mut e = ed("hello");
        assert!(!e.buf().mark_active);
        keys(&mut e, &[Key::Ctrl(' ')]);
        assert!(e.buf().mark_active);
        keys(&mut e, &[Key::Ctrl('f'), Key::Ctrl('f')]);
        assert!(e.buf().mark_active);
        typing(&mut e, "x");
        assert!(!e.buf().mark_active);
        keys(&mut e, &[Key::Ctrl(' '), Key::Ctrl('f')]);
        assert!(e.buf().mark_active);
        keys(&mut e, &[Key::Ctrl('g')]);
        assert!(!e.buf().mark_active);
        assert!(e.buf().mark.is_some());
        keys(&mut e, &[Key::Ctrl('x'), Key::Ctrl('x')]);
        assert!(e.buf().mark_active);
    }

    #[test]
    fn copy_deactivates_mark() {
        let mut e = ed("abc");
        keys(&mut e, &[Key::Ctrl(' '), Key::Ctrl('f'), Key::Ctrl('f'), Key::Meta('w')]);
        assert!(!e.buf().mark_active);
        keys(&mut e, &[Key::Meta('>'), Key::Ctrl('y')]);
        assert_eq!(e.buf().text.all(), "abcab");
    }

    #[test]
    fn buffer_completion() {
        let mut e = ed("first");
        e.buffers.push(Buffer::new("second".into(), None, String::new(), None));
        e.buffers.push(Buffer::new("settings".into(), None, String::new(), None));
        keys(&mut e, &[Key::Ctrl('x'), Key::Char('b')]);
        keys(&mut e, &[Key::Tab]);
        match &e.mode {
            Mode::Prompt(p) => {
                assert!(p.input.is_empty());
                assert!(p.matches.contains("test") && p.matches.contains("second"));
            }
            _ => panic!("expected prompt"),
        }
        typing(&mut e, "se");
        keys(&mut e, &[Key::Tab]);
        match &e.mode {
            Mode::Prompt(p) => assert_eq!(p.input, "se"),
            _ => panic!("expected prompt"),
        }
        typing(&mut e, "c");
        keys(&mut e, &[Key::Tab]);
        match &e.mode {
            Mode::Prompt(p) => assert_eq!(p.input, "second"),
            _ => panic!("expected prompt"),
        }
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.buf().name, "second");
    }

    #[test]
    fn list_buffers() {
        let mut e = ed("");
        e.buffers.push(Buffer::new("other".into(), None, String::new(), None));
        typing(&mut e, "x");
        keys(&mut e, &[Key::Ctrl('x'), Key::Ctrl('b')]);
        assert_eq!(e.echo, "test*  other");
    }

    #[test]
    fn common_prefix_works() {
        let items: Vec<String> = vec!["second".into(), "settings".into()];
        assert_eq!(common_prefix(&items), "se");
        let one: Vec<String> = vec!["only".into()];
        assert_eq!(common_prefix(&one), "only");
    }

    #[test]
    fn cmd_line_eval() {
        let mut e = ed("");
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "1 2 +");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.echo, "ok ( 3 )");
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "drop \"hi\" insert");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.buf().text.all(), "hi");
        assert_eq!(e.buf().cursor, 2);
    }

    #[test]
    fn cmd_line_error_keeps_stack() {
        let mut e = ed("");
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "7");
        keys(&mut e, &[Key::Enter]);
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "nosuch");
        keys(&mut e, &[Key::Enter]);
        assert!(e.echo.contains("unknown word"));
        keys(&mut e, &[Key::Meta('x'), Key::Enter]);
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "1 +");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.echo, "ok ( 8 )");
    }

    #[test]
    fn eval_at_point_definition_then_word() {
        let mut e = ed(": double dup + ;\n21 double");
        e.buf_mut().cursor = 5;
        keys(&mut e, &[Key::Meta(';')]);
        assert_eq!(e.echo, "ok ( )");
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "21 double");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.echo, "ok ( 42 )");
    }

    #[test]
    fn eval_at_point_token() {
        let mut e = ed("42");
        e.buf_mut().cursor = 1;
        keys(&mut e, &[Key::Meta(';')]);
        assert_eq!(e.echo, "ok ( 42 )");
    }

    #[test]
    fn forth_edits_undo_as_one_group() {
        let mut e = ed("");
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "\"abc\" insert \"def\" insert");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.buf().text.all(), "abcdef");
        keys(&mut e, &[Key::Ctrl('_')]);
        assert_eq!(e.buf().text.all(), "");
    }

    #[test]
    fn forth_selection_words() {
        let mut e = ed("hello world");
        keys(&mut e, &[Key::Ctrl(' '), Key::Meta('f')]);
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "sel@ text@");
        keys(&mut e, &[Key::Enter]);
        assert_eq!(e.echo, "ok ( \"hello\" )");
    }

    #[test]
    fn cmd_completion() {
        let mut e = ed("");
        keys(&mut e, &[Key::Meta('x')]);
        typing(&mut e, "5 cur");
        keys(&mut e, &[Key::Tab]);
        match &e.mode {
            Mode::Prompt(p) => {
                assert_eq!(p.input, "5 cursor");
                assert!(p.matches.is_empty());
            }
            _ => panic!("expected prompt"),
        }
        keys(&mut e, &[Key::Tab]);
        match &e.mode {
            Mode::Prompt(p) => assert!(p.matches.contains("cursor@") && p.matches.contains("cursor!")),
            _ => panic!("expected prompt"),
        }
    }

    #[test]
    fn vertical_goal_column() {
        let mut e = ed("long line here\nab\nanother long line");
        keys(&mut e, &[Key::Ctrl('e')]);
        assert_eq!(e.buf().cursor, 14);
        keys(&mut e, &[Key::Ctrl('n')]);
        assert_eq!(e.buf().cursor, 17);
        keys(&mut e, &[Key::Ctrl('n')]);
        assert_eq!(e.buf().cursor, 32);
    }
}
