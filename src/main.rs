mod editor;
mod file;
mod forth;
mod sys;
mod term;
mod text;
mod ui;
mod width;

use std::io::{self, Write};
use std::path::Path;
use std::sync::OnceLock;

static SAVED: OnceLock<sys::Termios> = OnceLock::new();

fn main() {
    let mut ed = editor::Editor::new();
    if let Some(home) = std::env::var_os("HOME") {
        match std::fs::read_to_string(Path::new(&home).join(".edy.f")) {
            Ok(src) => {
                if let Err(e) = ed.vm.load(&src) {
                    ed.echo = format!("edy.f: {}", e);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => ed.echo = format!("edy.f: {}", e),
        }
    }
    for arg in std::env::args().skip(1) {
        ed.find_file(Path::new(&arg));
    }
    ed.ensure_buffer();
    ed.current = 0;

    let raw = match term::Raw::enter() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("edy: stdin is not a terminal");
            std::process::exit(1);
        }
    };
    let _ = SAVED.set(raw.saved());
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(t) = SAVED.get() {
            term::restore(t);
        }
        hook(info);
    }));

    let mut out = io::BufWriter::new(io::stdout());
    let mut tty = term::Tty;
    let mut size = (0, 0);
    let mut dirty = true;
    while !ed.quit {
        let ws = sys::winsize();
        if ws != size {
            size = ws;
            ed.rows = ws.0;
            ed.cols = ws.1;
            dirty = true;
        }
        if dirty {
            ed.reframe();
            let _ = ui::render(&ed, &mut out);
            dirty = false;
        }
        if sys::wait_input(250) {
            loop {
                match term::read_key(&mut tty) {
                    Some(key) => ed.handle_key(key),
                    None => break,
                }
                if ed.quit || !sys::wait_input(0) {
                    break;
                }
            }
            dirty = true;
        }
        if let Some(s) = ed.take_clip() {
            let _ = term::osc52(&mut out, &s);
            let _ = out.flush();
        }
    }
    drop(raw);
}
