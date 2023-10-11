use std::{
    ffi::OsStr,
    fmt::{Display, Write as _},
    fs::{metadata, File},
    io::{self, BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom, Write as _},
    iter,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::{
    ansi_escape::*,
    ctrl_key::*,
    format_size, get_winsize_using_cursor, slice_find,
    sys::{self, enable_raw_mode, monitor_winsize, set_terminal_mode, TerminalMode},
    Config, HlState, Row, SyntaxConfig, HELP_MESSAGE,
};

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
enum ArrowKey {
    Left,
    Right,
    Up,
    Down,
}

enum CommandMode {
    Save(String),
    Find(String, Cursor, Option<usize>),
    GoTo(String),
    Execute(String),
}

fn process_command_key(mut buffer: String, key: Key) -> CommandState {
    match key {
        Key::Char(b'\r') => CommandState::Completed(buffer),
        Key::Escape | Key::Char(EXIT) => CommandState::Cancelled,
        Key::Char(BACKSPACE | DELETE_BIS) => {
            buffer.pop();
            CommandState::Active(buffer)
        }
        Key::Char(c @ 0..=126) if !c.is_ascii_control() => {
            buffer.push(c as char);
            CommandState::Active(buffer)
        }
        _ => CommandState::Active(buffer),
    }
}

impl CommandMode {
    pub fn process_key(self, editor: &mut Editor, key: Key) -> Result<Option<Self>, String> {
        editor.status_message = None;
        match self {
            Self::Save(buffer) => match process_command_key(buffer, key) {
                CommandState::Active(buffer) => return Ok(Some(Self::Save(buffer))),
                CommandState::Cancelled => editor.set_status("Save aborted".to_string()),
                CommandState::Completed(file_name) => editor.save_as(&file_name)?,
            },
            Self::Find(buffer, cursor, last_match) => {
                if let Some(row) = last_match {
                    editor.rows[row].match_range = None;
                }
                match process_command_key(buffer, key) {
                    CommandState::Active(query) => {
                        let (last_match, forward) = match key {
                            Key::Arrow(ArrowKey::Right | ArrowKey::Down) | Key::Char(FIND) => {
                                (last_match, true)
                            }
                            Key::Arrow(ArrowKey::Left | ArrowKey::Up) => (last_match, false),
                            _ => (None, true),
                        };
                        let current_match = editor.find(&query, last_match, forward);
                        return Ok(Some(Self::Find(query, cursor, current_match)));
                    }
                    CommandState::Cancelled => editor.cursor = cursor,
                    CommandState::Completed(_) => (),
                }
            }
            Self::GoTo(buffer) => match process_command_key(buffer, key) {
                CommandState::Active(buffer) => return Ok(Some(Self::GoTo(buffer))),
                CommandState::Cancelled => (),
                CommandState::Completed(buffer) => {
                    let mut split = buffer
                        .splitn(2, ':')
                        .map(|u| u.trim().parse::<usize>().map(|s| s.saturating_sub(1)));
                    match (split.next().transpose(), split.next().transpose()) {
                        (Ok(Some(y)), Ok(x)) => {
                            editor.cursor.y = y.min(editor.rows.len());
                            editor.cursor.x = if let Some(rx) = x {
                                editor.current_row().map_or(0, |r| r.r2c[rx])
                            } else {
                                editor
                                    .cursor
                                    .x
                                    .min(editor.current_row().map_or(0, |r| r.chars.len()))
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            editor.set_status(format!("GoTo error: {}", e))
                        }
                        _ => (),
                    }
                    todo!()
                }
            },
            Self::Execute(buffer) => match process_command_key(buffer, key) {
                CommandState::Active(buffer) => return Ok(Some(Self::Execute(buffer))),
                CommandState::Cancelled => (),
                CommandState::Completed(command) => {
                    let mut args = command.split_whitespace();
                    match Command::new(args.next().unwrap_or_default())
                        .args(args)
                        .output()
                    {
                        Ok(out) if out.status.success() => {
                            out.stdout.into_iter().for_each(|c| match c {
                                b'\n' => editor.insert_new_line(),
                                c => editor.insert_byte(c),
                            })
                        }
                        Ok(out) => editor.set_status(
                            String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
                        ),
                        Err(e) => editor.set_status(e.to_string()),
                    }
                }
            },
        }
        Ok(None)
    }
}

enum CommandState {
    Active(String),
    Completed(String),
    Cancelled,
}

// Cursor position, 0-indexed
#[derive(Default, Clone)]
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
    mode: Option<CommandMode>,
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
        let winsize = sys::get_winsize().or_else(|_| get_winsize_using_cursor())?;
        self.text_rows = winsize.0.saturating_sub(2);
        self.text_cols = winsize.1;
        self.update_padding();
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

    fn delete_char(&mut self) {
        if self.cursor.x > 0 {
            let row = &mut self.rows[self.cursor.y];
            let n = row.get_char_size(row.c2r[self.cursor.x] - 1);
            row.chars
                .splice(self.cursor.x - n..self.cursor.x, iter::empty());
            self.update_row(self.cursor.y, false);
            self.cursor.x -= n;
            self.dirty = self.rows.len() > 1 || self.n_bytes != 0 || self.file_name.is_some();
            self.n_bytes -= n;
        } else if self.cursor.y < self.rows.len() && self.cursor.y > 0 {
            let row = self.rows.remove(self.cursor.y);
            let prev_row = &mut self.rows[self.cursor.y - 1];
            self.cursor.x = prev_row.chars.len();
            prev_row.chars.extend(row.chars);
            self.update_row(self.cursor.y - 1, true);
            self.update_row(self.cursor.y, false);
            self.update_padding();
            self.cursor.y -= 1;
            self.dirty = true;
        } else if self.cursor.y == self.rows.len() {
            self.move_cursor(ArrowKey::Left, false);
        }
    }

    fn insert_new_line(&mut self) {
        let (column, chars) = if self.cursor.x == 0 {
            (self.cursor.y, vec![])
        } else {
            let new_chars = self.rows[self.cursor.y].chars.split_off(self.cursor.x);
            self.update_row(self.cursor.y, false);
            (self.cursor.y + 1, new_chars)
        };
        self.rows.insert(column, Row::new(chars));
        self.update_row(column, false);
        self.update_padding();
        self.cursor.x = 0;
        self.cursor.y += 1;
        self.dirty = true;
    }

    fn delete_current_row(&mut self) {
        if self.cursor.y < self.rows.len() {
            self.rows[self.cursor.y].chars.clear();
            self.update_row(self.cursor.y, false);
            self.cursor.x = 0;
            self.cursor.y += 1;
            self.delete_char();
        }
    }

    fn copy_current_row(&mut self) {
        if let Some(row) = self.current_row() {
            self.copied_row = row.chars.clone();
        }
    }

    fn paste_current_row(&mut self) {
        if self.copied_row.is_empty() {
            return;
        }
        self.n_bytes += self.copied_row.len();
        self.rows.insert(
            (self.cursor.y + 1).min(self.rows.len()),
            Row::new(self.copied_row.clone()),
        );
        self.update_row(
            self.cursor.y + usize::from(self.cursor.y + 1 != self.rows.len()),
            false,
        );
        self.cursor.y += 1;
        self.dirty = true;
        self.update_padding();
    }

    fn duplicate_current_row(&mut self) {
        self.copy_current_row();
        self.paste_current_row();
    }

    fn insert_byte(&mut self, c: u8) {
        if let Some(row) = self.rows.get_mut(self.cursor.y) {
            row.chars.insert(self.cursor.x, c);
        } else {
            self.rows.push(Row::new(vec![c]));
            self.update_padding();
        }
        self.update_row(self.cursor.y, false);
        self.cursor.x += 1;
        self.n_bytes += 1;
        self.dirty = true;
    }

    fn save(&self, file_name: &str) -> Result<usize, String> {
        let mut file = File::create(file_name).map_err(|e| e.to_string())?;
        let mut n = 0;
        for (i, row) in self.rows.iter().enumerate() {
            file.write_all(&row.chars).map_err(|e| e.to_string())?;
            n += row.chars.len();
            if i != self.rows.len() - 1 {
                file.write_all(&[b'\n']).map_err(|e| e.to_string())?;
                n += 1;
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(n)
    }

    fn handle_save(&mut self, file_name: &str) -> bool {
        let saved = self.save(file_name);
        self.set_status(match saved.as_ref() {
            Ok(n) => format!("{} written to {}", format_size(*n), file_name),
            Err(e) => format!("Save I/O error: {}", e),
        });
        self.dirty &= saved.is_err();
        saved.is_ok()
    }

    fn save_as(&mut self, file_name: &str) -> Result<(), String> {
        if self.handle_save(file_name) {
            self.select_syntax(Path::new(file_name))?;
            self.file_name = Some(file_name.to_string());
            self.update_all_rows();
        }
        Ok(())
    }

    fn process_key(&mut self, key: Key) -> (bool, Option<CommandMode>) {
        let mut quit_times = self.config.quit_times;
        let mut command = None;
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
            Key::Delete => {
                self.move_cursor(ArrowKey::Right, false);
                self.delete_char();
            }
            Key::Escape => (),
            Key::Char(b'\r' | b'\n') => self.insert_new_line(),
            Key::Char(BACKSPACE | DELETE_BIS) => self.delete_char(),
            Key::Char(REMOVE_LINE) => self.delete_current_row(),
            Key::Char(REFRESH_SCREEN) => (),
            Key::Char(EXIT) => {
                quit_times = self.quit_times - 1;
                if !self.dirty || quit_times == 0 {
                    return (true, None);
                }
                self.set_status(format!("Press Ctrl+Q {quit_times} more time(s) to quit."));
            }
            Key::Char(SAVE) => {
                if let Some(file_name) = self.file_name.take() {
                    self.handle_save(&file_name);
                    self.file_name = Some(file_name);
                } else {
                    command = Some(CommandMode::Save(String::new()))
                }
            }
            Key::Char(FIND) => {
                command = Some(CommandMode::Find(String::new(), self.cursor.clone(), None))
            }
            Key::Char(GOTO) => command = Some(CommandMode::GoTo(String::new())),
            Key::Char(DUPLICATE) => self.duplicate_current_row(),
            Key::Char(CUT) => {
                self.copy_current_row();
                self.delete_current_row();
            }
            Key::Char(COPY) => self.copy_current_row(),
            Key::Char(PASTE) => self.paste_current_row(),
            Key::Char(EXECUTE) => command = Some(CommandMode::Execute(String::new())),
            Key::Char(c) => self.insert_byte(c),
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

    fn update_padding(&mut self) {
        let n = self.rows.len().to_string().len();
        self.left_padding = if self.config.show_line_numbers && n + 2 < self.window_width / 4 {
            n + 2
        } else {
            0
        };
        self.text_cols = self.window_width.saturating_sub(self.left_padding);
    }

    fn draw_padding<T: Display>(&self, buffer: &mut String, val: T) -> Result<(), String> {
        if self.left_padding >= 2 {
            write!(
                buffer,
                "\x1b[38;5;240m{:>2$} \u{2502}{}",
                val,
                RESET_FMT,
                self.left_padding - 2
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn draw_rows(&self, buffer: &mut String) -> Result<(), String> {
        for (i, row) in self
            .rows
            .iter()
            .map(Some)
            .chain(iter::repeat(None))
            .enumerate()
            .skip(self.cursor.row_offset)
            .take(self.text_rows)
        {
            buffer.push_str(CLEAR_LINE_RIGHT_OF_CURSOR);
            if let Some(row) = row {
                self.draw_padding(buffer, i + 1)?;
                row.draw(self.cursor.col_offset, self.text_cols, buffer)?;
            } else {
                self.draw_padding(buffer, '~')?;
                if self.rows.len() <= 1 && self.n_bytes == 0 && i == self.text_rows / 3 {
                    write!(
                        buffer,
                        "{:^1$.1$}",
                        "Rabi - Usagi Peropero Club!", self.text_cols
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            buffer.push_str("\r\n");
        }
        Ok(())
    }

    fn draw_status(&self, buffer: &mut String) -> Result<(), String> {
        let left = format!(
            "{:.30}{}",
            self.file_name.as_deref().unwrap_or("[No Name]"),
            if self.dirty { " (modified)" } else { "" }
        );
        let right = format!(
            "{} | {} | {}:{}",
            self.syntax.name,
            format_size(self.n_bytes + self.rows.len().saturating_sub(1)),
            self.cursor.y + 1,
            self.rx() + 1
        );
        let rw = self.window_width.saturating_sub(left.len());
        write!(
            buffer,
            "{REVERSE_VIDEO}{left}{right:>rw$.rw$}{RESET_FMT}\r\n"
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn draw_message(&self, buffer: &mut String) {
        buffer.push_str(CLEAR_LINE_RIGHT_OF_CURSOR);
        if let Some((message, time)) = self.status_message.as_ref() {
            if time.elapsed() < Duration::new(self.config.message_duration as u64, 0) {
                buffer.push_str(&message[..message.len().min(self.window_width)]);
            }
        }
    }

    fn refresh(&mut self) -> Result<(), String> {
        self.cursor.row_offset = self.cursor.row_offset.clamp(
            self.cursor
                .y
                .saturating_sub(self.text_rows.saturating_sub(1)),
            self.cursor.y,
        );
        self.cursor.col_offset = self.cursor.col_offset.clamp(
            self.rx().saturating_sub(self.text_cols.saturating_sub(1)),
            self.rx(),
        );

        let mut buffer = format!("{HIDE_CURSOR}{MOVE_CURSOR_TO_START}");
        self.draw_rows(&mut buffer)?;
        self.draw_status(&mut buffer)?;
        self.draw_message(&mut buffer);

        let (cursor_x, cursor_y) = if self.mode.is_none() {
            (
                self.rx() - self.cursor.col_offset + 1 + self.left_padding,
                self.cursor.y - self.cursor.row_offset + 1,
            )
        } else {
            (
                self.status_message
                    .as_ref()
                    .map_or(0, |(message, _)| message.len() + 1),
                self.text_rows + 2,
            )
        };

        print!("{buffer}\x1b[{cursor_y};{cursor_x}H{SHOW_CURSOR}");
        io::stdout().flush().map_err(|e| e.to_string())
    }

    fn wait_for_key(&mut self) -> Result<Key, String> {
        loop {
            if sys::winsize_changed() {
                self.update_winsize()?;
                self.refresh()?;
            }
            let mut bytes = io::stdin().bytes();
            match bytes.next().transpose().map_err(|e| e.to_string())? {
                Some(b'\x1b') => {
                    return Ok(match bytes.next().transpose().map_err(|e| e.to_string())? {
                        Some(b @ (b'[' | b'O')) => {
                            match (b, bytes.next().transpose().map_err(|e| e.to_string())?) {
                                (b'[', Some(b'A')) => Key::Arrow(ArrowKey::Up),
                                (b'[', Some(b'B')) => Key::Arrow(ArrowKey::Down),
                                (b'[', Some(b'C')) => Key::Arrow(ArrowKey::Right),
                                (b'[', Some(b'D')) => Key::Arrow(ArrowKey::Left),
                                (b'[' | b'O', Some(b'H')) => Key::Home,
                                (b'[' | b'O', Some(b'F')) => Key::End,
                                (b'[', mut c @ Some(b'0'..=b'8')) => {
                                    let mut d =
                                        bytes.next().transpose().map_err(|e| e.to_string())?;
                                    if let (Some(b'1'), Some(b';')) = (c, d) {
                                        c = bytes.next().transpose().map_err(|e| e.to_string())?;
                                        d = bytes.next().transpose().map_err(|e| e.to_string())?;
                                    }
                                    match (c, d) {
                                        (Some(c), Some(b'~')) if c == b'1' || c == b'7' => {
                                            Key::Home
                                        }
                                        (Some(c), Some(b'~')) if c == b'4' || c == b'8' => Key::End,
                                        (Some(b'3'), Some(b'~')) => Key::Delete,
                                        (Some(b'5'), Some(b'~')) => Key::PageUp,
                                        (Some(b'6'), Some(b'~')) => Key::PageDown,
                                        (Some(b'5'), Some(b'A')) => Key::CtrlArrow(ArrowKey::Up),
                                        (Some(b'5'), Some(b'B')) => Key::CtrlArrow(ArrowKey::Down),
                                        (Some(b'5'), Some(b'C')) => Key::CtrlArrow(ArrowKey::Right),
                                        (Some(b'5'), Some(b'D')) => Key::CtrlArrow(ArrowKey::Left),
                                        _ => Key::Escape,
                                    }
                                }
                                (b'O', Some(b'a')) => Key::CtrlArrow(ArrowKey::Up),
                                (b'O', Some(b'b')) => Key::CtrlArrow(ArrowKey::Down),
                                (b'O', Some(b'c')) => Key::CtrlArrow(ArrowKey::Right),
                                (b'O', Some(b'd')) => Key::CtrlArrow(ArrowKey::Left),
                                _ => Key::Escape,
                            }
                        }
                        _ => Key::Escape,
                    });
                }
                Some(c) => return Ok(Key::Char(c)),
                None => (),
            }
        }
    }

    fn find(&mut self, query: &str, last_match: Option<usize>, forward: bool) -> Option<usize> {
        let num_rows = self.rows.len();
        let mut current = last_match.unwrap_or(num_rows.saturating_sub(1));
        for _ in 0..num_rows {
            current = (current + if forward { 1 } else { num_rows - 1 }) % num_rows;
            let row = &mut self.rows[current];
            if let Some(cx) = slice_find(&row.chars, query.as_bytes()) {
                self.cursor.x = cx;
                self.cursor.y = current;
                self.cursor.col_offset = 0;
                row.match_range = Some(row.c2r[cx]..row.c2r[cx] + query.len());
                return Some(current);
            }
        }
        None
    }

    pub fn run(&mut self, filename: Option<String>) -> Result<(), String> {
        if let Some(path) = filename.map(PathBuf::from) {
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
                    self.update_padding();
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
                    CommandMode::Save(s) => format!("Save as {s}"),
                    CommandMode::Find(s, ..) => format!("Search (Use ESC/Arrows/Enter): {s}"),
                    CommandMode::GoTo(s) => format!("Enter line number[:column number]: {s}"),
                    CommandMode::Execute(s) => format!("CommandMode to execute: {s}"),
                })
            }
            self.refresh()?;
            let key = self.wait_for_key()?;
            self.mode = match self.mode.take() {
                Some(mode) => mode.process_key(self, key)?,
                None => match self.process_key(key) {
                    (true, _) => return Ok(()),
                    (false, mode) => mode,
                },
            }
        }
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(mode) = self.origin_ternimal_mode {
            set_terminal_mode(mode).expect("Failed to restore original terminal mode.");
        }
        if !std::thread::panicking() {
            print!("{CLEAR_SCREEN}{MOVE_CURSOR_TO_START}");
            io::stdout().flush().expect("Failed to flush stdout.");
        }
    }
}
