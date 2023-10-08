use crate::{Config, SyntaxConfig};

const HELP_MESSAGE: &str = "^S save | ^Q quit | ^F find | ^G go to | ^D duplicate | ^E execute | ^C copy | ^X cut | ^V paste";

enum Key {
    Arrow(ArrowKey),
    CtrlArrow(ArrowKey),
    PageUp,
    PageDown,
    Home,
    End,
    Delete,
    Escape,
    Char(u8),
}

enum ArrowKey {
    Left,
    Right,
    Up,
    Down,
}

enum Command {
    Save(String),
    Find(String, Cursor, Option<usize>),
    GoTo(String),
    Execute(String),
}

// Cursor position, 0-indexed
#[derive(Default)]
struct Cursor {
    x: usize,
    y: usize,
    row_offset: usize,
    col_offset: usize,
}

impl Cursor {
    fn move_to_next_line(&mut self) {
        self.x = 0;
        self.y = self.y + 1;
    }
}

#[derive(Default)]
pub struct Editor {
    config: Config,
    quit_times: usize,
    file_name: Option<String>,
    syntax: SyntaxConfig,

    cursor: Cursor,
    mode: Option<Command>,
    left_padding: usize,
    window_width: usize,
    dirty: bool,

    // Editor size, excluding padding and bar
    text_rows: usize,
    text_cols: usize,
}

impl Editor {
    pub fn new(config: Config) -> Result<Self, String> {
        todo!()
    }

    pub fn run(&mut self, filename: Option<String>) -> Result<(), String> {
        todo!()
    }
}
