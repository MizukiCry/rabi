mod config;
mod editor;
mod row;
mod syntax;

use std::{
    fmt::{Display, Formatter},
    io::{self, BufRead, Read, Write},
    str::FromStr,
};

pub use config::*;
pub use editor::*;
pub use row::*;
pub use syntax::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;

pub const HELP_MESSAGE: &str = "^S save | ^Q quit | ^F find | ^G go to | ^D duplicate | ^E execute | ^C copy | ^X cut | ^V paste";

// ANSI Escape sequences
pub mod ansi_escape {
    pub const CLEAR_SCREEN: &str = "\x1b[2J"; // Clear from cursor to beginning of the screen
    pub const RESET_FMT: &str = "\x1b[m"; // Reset the formatting
    pub const REVERSE_VIDEO: &str = "\x1b[7m"; // Invert foreground and background color
    pub const MOVE_CURSOR_TO_START: &str = "\x1b[H"; // Move the cursor to 1:1
    pub const HIDE_CURSOR: &str = "\x1b[?25l"; // DECTCTEM: Make the cursor invisible
    pub const SHOW_CURSOR: &str = "\x1b[?25h"; // DECTCTEM: Make the cursor visible
    pub const CLEAR_LINE_RIGHT_OF_CURSOR: &str = "\x1b[K"; // Clear line right of the current position of the cursor
    pub const DEVICE_STATUS_REPORT: &str = "\x1b[6n"; // Report the cursor position to the application.
    pub const REPOSITION_CURSOR_END: &str = "\x1b[999C\x1b[999B"; // Reposition the cursor to the end of the window
}

pub mod ctrl_key {
    const fn ctrl_key(key: u8) -> u8 {
        key & 0x1f
    }
    pub const EXIT: u8 = ctrl_key(b'Q');
    pub const DELETE_BIS: u8 = ctrl_key(b'H');
    pub const REFRESH_SCREEN: u8 = ctrl_key(b'L');
    pub const SAVE: u8 = ctrl_key(b'S');
    pub const FIND: u8 = ctrl_key(b'F');
    pub const GOTO: u8 = ctrl_key(b'G');
    pub const CUT: u8 = ctrl_key(b'X');
    pub const COPY: u8 = ctrl_key(b'C');
    pub const PASTE: u8 = ctrl_key(b'V');
    pub const DUPLICATE: u8 = ctrl_key(b'D');
    pub const EXECUTE: u8 = ctrl_key(b'E');
    pub const REMOVE_LINE: u8 = ctrl_key(b'R');
    pub const BACKSPACE: u8 = 127;
}

#[derive(Clone, Copy, PartialEq)]
pub enum Color {
    Black = 30,
    Red = 31,
    Green = 32,
    Yellow = 33,
    Blue = 34,
    Magenta = 35,
    Cyan = 36,
    White = 37,
    Default = 39,

    BlackBG = 40,
    RedBG = 41,
    GreenBG = 42,
    YellowBG = 43,
    BlueBG = 44,
    MagentaBG = 45,
    CyanBG = 46,
    WhiteBG = 47,
    DefaultBG = 49,
}

impl Display for Color {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "\x1b[{}m", *self as u8)
    }
}

pub use ansi_escape::*;

fn read_value_until<T: FromStr>(stop_byte: u8) -> Result<T, String> {
    let mut buf = Vec::new();
    io::stdin()
        .lock()
        .read_until(stop_byte, &mut buf)
        .map_err(|e| e.to_string())?;
    buf.pop()
        .filter(|u| *u == stop_byte)
        .ok_or("Cursor error.")?;
    std::str::from_utf8(&buf)
        .or(Err("Cursor error."))?
        .parse()
        .or(Err("Cursor error.".to_string()))
}

pub fn get_winsize_using_cursor() -> Result<(usize, usize), String> {
    let mut stdin = io::stdin();
    print!("{REPOSITION_CURSOR_END}{DEVICE_STATUS_REPORT}");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut prefix_buffer = [0_u8; 2];
    stdin
        .read_exact(&mut prefix_buffer)
        .map_err(|e| e.to_string())?;
    if prefix_buffer != [b'\x1b', b'['] {
        return Err("Cursor error.".to_string());
    }
    Ok((read_value_until(b';')?, read_value_until(b'R')?))
}

pub fn format_size(n: usize) -> String {
    if n < 1024 {
        format!("{n}B")
    } else {
        let i = ((64 - n.leading_zeros() + 9) / 10 - 1) as usize;
        let q = 100 * n / (1024 << ((i - 1) * 10));
        format!("{}.{:02}{}B", q / 100, q % 100, b" kMGTPEZ"[i] as char)
    }
}

pub fn slice_find<T: PartialEq>(s: &[T], t: &[T]) -> Option<usize> {
    (0..(s.len() + 1).saturating_sub(t.len())).find(|&i| s[i..].starts_with(t))
}
