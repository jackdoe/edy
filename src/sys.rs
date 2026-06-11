use std::io;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_ulong, c_void};

#[cfg(target_os = "macos")]
mod os {
    pub type Flag = u64;
    pub type Nfds = u32;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: Flag,
        pub c_oflag: Flag,
        pub c_cflag: Flag,
        pub c_lflag: Flag,
        pub c_cc: [u8; 20],
        pub c_ispeed: Flag,
        pub c_ospeed: Flag,
    }

    pub const BRKINT: Flag = 0x2;
    pub const ICRNL: Flag = 0x100;
    pub const INPCK: Flag = 0x10;
    pub const ISTRIP: Flag = 0x20;
    pub const IXON: Flag = 0x200;
    pub const OPOST: Flag = 0x1;
    pub const CS8: Flag = 0x300;
    pub const ECHO: Flag = 0x8;
    pub const ICANON: Flag = 0x100;
    pub const IEXTEN: Flag = 0x400;
    pub const ISIG: Flag = 0x80;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    pub const TIOCGWINSZ: u64 = 0x4008_7468;
}

#[cfg(target_os = "linux")]
mod os {
    pub type Flag = u32;
    pub type Nfds = u64;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: Flag,
        pub c_oflag: Flag,
        pub c_cflag: Flag,
        pub c_lflag: Flag,
        pub c_line: u8,
        pub c_cc: [u8; 32],
        pub c_ispeed: Flag,
        pub c_ospeed: Flag,
    }

    pub const BRKINT: Flag = 0x2;
    pub const ICRNL: Flag = 0x100;
    pub const INPCK: Flag = 0x10;
    pub const ISTRIP: Flag = 0x20;
    pub const IXON: Flag = 0x400;
    pub const OPOST: Flag = 0x1;
    pub const CS8: Flag = 0x30;
    pub const ECHO: Flag = 0x8;
    pub const ICANON: Flag = 0x2;
    pub const IEXTEN: Flag = 0x8000;
    pub const ISIG: Flag = 0x1;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    pub const TIOCGWINSZ: u64 = 0x5413;
}

pub use os::Termios;

const TCSAFLUSH: c_int = 2;
const POLLIN: i16 = 0x1;

#[repr(C)]
struct Winsize {
    row: u16,
    col: u16,
    xpixel: u16,
    ypixel: u16,
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

extern "C" {
    fn tcgetattr(fd: c_int, t: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, action: c_int, t: *const Termios) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn poll(fds: *mut PollFd, nfds: os::Nfds, timeout: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
}

pub fn termios_get() -> io::Result<Termios> {
    let mut t = MaybeUninit::<Termios>::uninit();
    if unsafe { tcgetattr(0, t.as_mut_ptr()) } == 0 {
        Ok(unsafe { t.assume_init() })
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn termios_set(t: &Termios) -> io::Result<()> {
    if unsafe { tcsetattr(0, TCSAFLUSH, t) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn make_raw(t: &mut Termios) {
    t.c_iflag &= !(os::BRKINT | os::ICRNL | os::INPCK | os::ISTRIP | os::IXON);
    t.c_oflag &= !os::OPOST;
    t.c_cflag |= os::CS8;
    t.c_lflag &= !(os::ECHO | os::ICANON | os::IEXTEN | os::ISIG);
    t.c_cc[os::VMIN] = 1;
    t.c_cc[os::VTIME] = 0;
}

pub fn winsize() -> (usize, usize) {
    let mut w = Winsize { row: 0, col: 0, xpixel: 0, ypixel: 0 };
    let ok = unsafe { ioctl(1, os::TIOCGWINSZ as c_ulong, &mut w as *mut Winsize) } == 0;
    if ok && w.col > 0 && w.row > 0 {
        (w.row as usize, w.col as usize)
    } else {
        (24, 80)
    }
}

pub fn wait_input(timeout_ms: i32) -> bool {
    let mut p = PollFd { fd: 0, events: POLLIN, revents: 0 };
    unsafe { poll(&mut p, 1, timeout_ms) > 0 && p.revents & POLLIN != 0 }
}

pub fn read_byte() -> Option<u8> {
    let mut b = 0u8;
    loop {
        let n = unsafe { read(0, &mut b as *mut u8 as *mut c_void, 1) };
        if n == 1 {
            return Some(b);
        }
        if n == 0 {
            return None;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return None;
        }
    }
}
