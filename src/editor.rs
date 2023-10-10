use std::{
    ffi::OsStr,
    fs::{metadata, File},
    io::{BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom},
    iter::successors,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    sys::{enable_raw_mode, monitor_winsize, TerminalMode},
    Config, HlState, Row, SyntaxConfig,
};

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

impl Command {
    pub fn process_key(self, editor: &mut Editor, key: &Key) -> Result<Option<Self>, String> {
        todo!()
    }
}

enum CommandState {
    Active(String),
    Completed(String),
    Cancelled,
}

// Cursor position, 0-indexed
#[derive(Default)]
struct Cursor {
    x: usize,
    y: usize,
    row_offset: usize,
    col_offset: usize,
}

#[derive(Default)]
pub struct Editor {
    config: Config,
    quit_times: usize,
    file_name: Option<String>,
    syntax: SyntaxConfig,
    status_message: Option<(String, Instant)>,

    cursor: Cursor,
    mode: Option<Command>,
    left_padding: usize,
    window_width: usize,
    rows: Vec<Row>,
    dirty: bool,

    // Editor size, excluding padding and bar
    text_rows: usize,
    text_cols: usize,
    n_bytes: usize,
    origin_ternimal_mode: Option<TerminalMode>,
    copied_row: Vec<u8>,
}

impl Editor {
    pub fn new(config: Config) -> Result<Self, String> {
        monitor_winsize()?;
        let mut editor = Self::default();
        editor.quit_times = config.quit_times;
        editor.config = config;
        editor.origin_ternimal_mode = Some(enable_raw_mode()?);
        editor.update_winsize()?;
        editor.set_status(HELP_MESSAGE.to_string());
        Ok(editor)
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.cursor.y)
    }

    // Cursor position in rendered characters
    fn rx(&self) -> usize {
        self.current_row().map_or(0, |row| row.c2r[self.cursor.x])
    }

    fn set_status(&mut self, message: String) {
        self.status_message = Some((message, Instant::now()));
    }

    fn update_winsize(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn move_cursor(&mut self, key: ArrowKey, ctrl: bool) {
        let mut x = self.cursor.x;
        match (key, self.current_row()) {
            (ArrowKey::Left, Some(row)) if x > 0 => {
                x -= row.get_char_size(row.c2r[x]);
                while ctrl && x > 0 && row.chars[x - 1] != b' ' {
                    x -= row.get_char_size(row.c2r[x] - 1);
                }
            }
            (ArrowKey::Left, _) if self.cursor.y > 0 => {
                x = usize::MAX;
                self.cursor.y -= 1;
            }
            (ArrowKey::Right, Some(row)) if x < row.chars.len() => {
                x += row.get_char_size(row.c2r[x]);
                while ctrl && x < row.chars.len() && row.chars[x] != b' ' {
                    x += row.get_char_size(row.c2r[x]);
                }
            }
            (ArrowKey::Right, Some(_)) => {
                x = 0;
                self.cursor.y += 1;
            }
            (ArrowKey::Up, _) if self.cursor.y > 0 => self.cursor.y -= 1,
            (ArrowKey::Down, _) => self.cursor.y += 1,
            _ => (),
        }
        self.cursor.x = x.min(self.current_row().map_or(0, |row| row.chars.len()));
    }

    fn select_syntax(&mut self, path: &Path) -> Result<(), String> {
        if let Some(ext) = path.extension().and_then(OsStr::to_str) {
            if let Some(syntax) = SyntaxConfig::from_ext(ext)? {
                self.syntax = syntax;
            }
        }
        Ok(())
    }

    fn process_key(&mut self, key: Key) -> (bool, Option<Command>) {
        let mut quit_times = self.config.quit_times;
        let command = None;
        match key {
            Key::Arrow(arrow) => self.move_cursor(arrow, false),
            Key::CtrlArrow(arrow) => self.move_cursor(arrow, true),
            Key::PageUp => {
                self.cursor.y = self.cursor.row_offset.saturating_sub(self.text_rows);
                self.cursor.x = self
                    .cursor
                    .x
                    .min(self.current_row().map_or(0, |row| row.chars.len()));
            }
            Key::PageDown => {
                self.cursor.y =
                    (self.cursor.row_offset + 2 * self.text_rows - 1).min(self.rows.len());
                self.cursor.x = self
                    .cursor
                    .x
                    .min(self.current_row().map_or(0, |row| row.chars.len()));
            }
            Key::Home => self.cursor.x = 0,
            Key::End => self.cursor.x = self.current_row().map_or(0, |row| row.chars.len()),
            Key::Delete => todo!(),
            Key::Escape => todo!(),
            Key::Char(_) => todo!(),
        }
        self.quit_times = quit_times;
        (false, command)
    }

    fn update_row(&mut self, y: usize, ignore_following: bool) {
        let mut hl_state = if y > 0 {
            self.rows[y - 1].hl_state
        } else {
            HlState::Normal
        };
        for row in self.rows.iter_mut().skip(y) {
            let pre_hl_state = row.hl_state;
            hl_state = row.update(&self.syntax, hl_state, self.config.tab_stop);
            if ignore_following || hl_state == pre_hl_state {
                return;
            }
        }
    }

    fn update_all_rows(&mut self) {
        let mut hl_state = HlState::Normal;
        for row in &mut self.rows {
            hl_state = row.update(&self.syntax, hl_state, self.config.tab_stop);
        }
    }

    fn update_cols(&mut self) {
        let n = self.rows.len().to_string().len();
        self.left_padding = if self.config.show_line_numbers && n + 2 < self.window_width / 4 {
            n + 2
        } else {
            0
        };
        self.text_cols = self.window_width.saturating_sub(self.left_padding);
    }

    fn refresh(&mut self) -> Result<(), String> {
        todo!()
    }

    fn wait_for_key(&mut self) -> Result<Key, String> {
        todo!()
    }

    pub fn run(&mut self, filename: Option<String>) -> Result<(), String> {
        if let Some(path) = filename.map(|p| PathBuf::from(p)) {
            self.file_name = Some(path.to_string_lossy().to_string());
            let path = path.as_path();
            self.select_syntax(path)?;
            let ft = metadata(path).map_err(|e| e.to_string())?.file_type();
            if !ft.is_file() && !ft.is_symlink() {
                return Err("Invalid file".to_string());
            }
            match File::open(path) {
                Ok(file) => {
                    for line in BufReader::new(file).split(b'\n') {
                        self.rows.push(Row::new(line.map_err(|e| e.to_string())?));
                    }

                    let mut file = File::open(path).map_err(|e| e.to_string())?;
                    file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
                    if file
                        .bytes()
                        .next()
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .map_or(true, |b| b == b'\n')
                    {
                        self.rows.push(Row::new(vec![]));
                    }
                    self.update_all_rows();
                    self.update_cols();
                    self.n_bytes = self.rows.iter().map(|row| row.chars.len()).sum();
                }
                Err(e) if e.kind() == ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
            }
        } else {
            self.file_name = None;
            self.rows.push(Row::new(vec![]));
        }
        loop {
            if let Some(mode) = self.mode.as_ref() {
                self.set_status(match &mode {
                    Command::Save(s) => format!("Save as {s}"),
                    Command::Find(s, ..) => format!("Search (Use ESC/Arrows/Enter): {s}"),
                    Command::GoTo(s) => format!("Enter line number[:column number]: {s}"),
                    Command::Execute(s) => format!("Command to execute: {s}"),
                })
            }
            self.refresh()?;
            let key = self.wait_for_key()?;
            self.mode = match self.mode.take() {
                Some(mode) => mode.process_key(self, &key)?,
                None => match self.process_key(key) {
                    (true, _) => return Ok(()),
                    (false, mode) => mode,
                },
            }
        }
    }
}
