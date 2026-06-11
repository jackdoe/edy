use crate::sys;
use std::io::{self, Write};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Meta(char),
    MetaBackspace,
    Enter,
    Tab,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
}

pub trait Input {
    fn byte(&mut self) -> Option<u8>;
    fn pending(&mut self) -> bool;
}

pub struct Tty;

impl Input for Tty {
    fn byte(&mut self) -> Option<u8> {
        sys::read_byte()
    }

    fn pending(&mut self) -> bool {
        sys::wait_input(25)
    }
}

pub fn read_key(inp: &mut impl Input) -> Option<Key> {
    let b = inp.byte()?;
    match b {
        0x0d => Some(Key::Enter),
        0x09 => Some(Key::Tab),
        0x7f => Some(Key::Backspace),
        0x1b => escape(inp),
        0x00 => Some(Key::Ctrl(' ')),
        0x01..=0x1a => Some(Key::Ctrl((b + 0x60) as char)),
        0x1c..=0x1f => Some(Key::Ctrl((b + 0x40) as char)),
        0x20..=0x7e => Some(Key::Char(b as char)),
        _ => utf8(inp, b).map(Key::Char),
    }
}

fn escape(inp: &mut impl Input) -> Option<Key> {
    if !inp.pending() {
        return Some(Key::Esc);
    }
    let b = inp.byte()?;
    match b {
        b'[' => csi(inp),
        b'O' => named(inp.byte()?, 0),
        0x7f => Some(Key::MetaBackspace),
        0x20..=0x7e => Some(Key::Meta(b as char)),
        0x80.. => utf8(inp, b).map(Key::Meta),
        _ => None,
    }
}

fn csi(inp: &mut impl Input) -> Option<Key> {
    let mut cur = 0usize;
    let mut first = None;
    loop {
        let b = inp.byte()?;
        match b {
            b'0'..=b'9' => cur = cur * 10 + (b - b'0') as usize,
            b';' | b':' => {
                first.get_or_insert(cur);
                cur = 0;
            }
            0x40..=0x7e => return named(b, first.unwrap_or(cur)),
            _ => return None,
        }
    }
}

fn named(fin: u8, n: usize) -> Option<Key> {
    match fin {
        b'A' => Some(Key::Up),
        b'B' => Some(Key::Down),
        b'C' => Some(Key::Right),
        b'D' => Some(Key::Left),
        b'H' => Some(Key::Home),
        b'F' => Some(Key::End),
        b'~' => match n {
            1 | 7 => Some(Key::Home),
            3 => Some(Key::Delete),
            4 | 8 => Some(Key::End),
            5 => Some(Key::PageUp),
            6 => Some(Key::PageDown),
            _ => None,
        },
        _ => None,
    }
}

fn utf8(inp: &mut impl Input, first: u8) -> Option<char> {
    let len = match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let mut buf = [first, 0, 0, 0];
    for slot in buf.iter_mut().take(len).skip(1) {
        *slot = inp.byte()?;
    }
    std::str::from_utf8(&buf[..len]).ok()?.chars().next()
}

pub struct Raw {
    saved: sys::Termios,
}

impl Raw {
    pub fn enter() -> io::Result<Raw> {
        let saved = sys::termios_get()?;
        let mut t = saved;
        sys::make_raw(&mut t);
        sys::termios_set(&t)?;
        let mut out = io::stdout();
        out.write_all(b"\x1b[?1049h")?;
        out.flush()?;
        Ok(Raw { saved })
    }

    pub fn saved(&self) -> sys::Termios {
        self.saved
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        restore(&self.saved);
    }
}

pub fn restore(t: &sys::Termios) {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x1b[?2026l\x1b[?25h\x1b[?1049l");
    let _ = out.flush();
    let _ = sys::termios_set(t);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Feed(Vec<u8>, usize);

    impl Input for Feed {
        fn byte(&mut self) -> Option<u8> {
            let b = self.0.get(self.1).copied();
            self.1 += 1;
            b
        }

        fn pending(&mut self) -> bool {
            self.1 < self.0.len()
        }
    }

    fn decode(bytes: &[u8]) -> Option<Key> {
        read_key(&mut Feed(bytes.to_vec(), 0))
    }

    #[test]
    fn decodes_keys() {
        assert_eq!(decode(b"a"), Some(Key::Char('a')));
        assert_eq!(decode(&[0x06]), Some(Key::Ctrl('f')));
        assert_eq!(decode(&[0x00]), Some(Key::Ctrl(' ')));
        assert_eq!(decode(&[0x1f]), Some(Key::Ctrl('_')));
        assert_eq!(decode(&[0x0d]), Some(Key::Enter));
        assert_eq!(decode(&[0x7f]), Some(Key::Backspace));
        assert_eq!(decode(&[0x1b]), Some(Key::Esc));
        assert_eq!(decode(b"\x1bf"), Some(Key::Meta('f')));
        assert_eq!(decode(b"\x1b<"), Some(Key::Meta('<')));
        assert_eq!(decode(&[0x1b, 0x7f]), Some(Key::MetaBackspace));
        assert_eq!(decode(b"\x1b[A"), Some(Key::Up));
        assert_eq!(decode(b"\x1b[1;5C"), Some(Key::Right));
        assert_eq!(decode(b"\x1b[3~"), Some(Key::Delete));
        assert_eq!(decode(b"\x1b[5~"), Some(Key::PageUp));
        assert_eq!(decode(b"\x1bOH"), Some(Key::Home));
        assert_eq!(decode("é".as_bytes()), Some(Key::Char('é')));
        assert_eq!(decode("語".as_bytes()), Some(Key::Char('語')));
    }
}
