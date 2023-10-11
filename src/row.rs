use std::{fmt::Write, iter::repeat, ops::Range};

use unicode_width::UnicodeWidthChar;

use crate::{ansi_escape::*, Color, HlState, SyntaxConfig};

#[derive(Default, Debug)]
pub struct Row {
    pub chars: Vec<u8>,
    render: String,
    // Mapping between chars and render
    pub c2r: Vec<usize>,
    pub r2c: Vec<usize>,
    hl: Vec<Color>,
    pub hl_state: HlState,
    pub match_range: Option<Range<usize>>,
}

impl Row {
    pub fn new(chars: Vec<u8>) -> Self {
        Self {
            chars,
            c2r: vec![0],
            ..Default::default()
        }
    }

    const fn is_sep(c: u8) -> bool {
        c.is_ascii_whitespace() || c == b'\0' || (c.is_ascii_punctuation() && c != b'_')
    }

    pub fn get_char_size(&self, rx: usize) -> usize {
        self.r2c
            .iter()
            .skip(rx + 1)
            .map(|cx| cx - self.r2c[rx])
            .find(|d| *d > 0)
            .unwrap_or(1)
    }

    pub fn update(&mut self, syntax: &SyntaxConfig, mut hl_state: HlState, tab: usize) -> HlState {
        self.render.clear();
        self.c2r.clear();
        self.r2c.clear();
        let (mut cx, mut rx) = (0, 0);
        for c in String::from_utf8_lossy(&self.chars).chars() {
            let n = if c == '\t' {
                tab - rx % tab
            } else {
                c.width().unwrap_or(1)
            };
            self.render.push_str(
                &(if c == '\t' {
                    " ".repeat(n)
                } else {
                    c.to_string()
                }),
            );
            self.c2r.extend(repeat(rx).take(c.len_utf8()));
            self.r2c.extend(repeat(cx).take(n));
            cx += c.len_utf8();
            rx += n;
        }
        self.c2r.push(rx);
        self.r2c.push(cx);

        self.hl.clear();
        let line = self.render.as_bytes();

        'outer_loop: while self.hl.len() < line.len() {
            let i = self.hl.len();
            let find_str = |s: &str| {
                line.get(i..i + s.len())
                    .map_or(false, |r| r == s.as_bytes())
            };

            if hl_state == HlState::Normal && syntax.slcomment_start.iter().any(|s| find_str(s)) {
                self.hl.extend(repeat(Color::Blue).take(line.len() - i));
                continue;
            }

            // Highlighting for comments and strings
            for (delims, mstate, mtype) in [
                (
                    &syntax.mlcomment_delims.as_ref().map(|(a, b)| (a, b)),
                    HlState::MlComment,
                    Color::Blue,
                ),
                (
                    &syntax.mlstring_delims.as_ref().map(|x| (x, x)),
                    HlState::MlString,
                    Color::Green,
                ),
            ] {
                if let Some((start, end)) = delims {
                    if hl_state == mstate {
                        if find_str(end) {
                            self.hl.extend(repeat(mtype).take(end.len()));
                            hl_state = HlState::Normal;
                        } else {
                            self.hl.push(mtype);
                        }
                        continue 'outer_loop;
                    } else if hl_state == HlState::Normal && find_str(start) {
                        self.hl.extend(repeat(mtype).take(start.len()));
                        hl_state = mstate;
                        continue 'outer_loop;
                    }
                }
            }

            let c = line[i];

            if let HlState::String(quote) = hl_state {
                self.hl.push(Color::Green);
                if c == quote {
                    hl_state = HlState::Normal;
                } else if c == b'\\' && i != line.len() - 1 {
                    self.hl.push(Color::Green);
                }
                continue;
            } else if syntax.slstring_quotes.contains(&(c as char)) {
                hl_state = HlState::String(c);
                self.hl.push(Color::Green);
                continue;
            }

            let prev_sep = i == 0 || Self::is_sep(line[i - 1]);
            if syntax.highlight_numbers
                && ((c.is_ascii_digit() && prev_sep)
                    || (i != 0 && self.hl[i - 1] == Color::Red && !prev_sep && !Self::is_sep(c)))
            {
                self.hl.push(Color::Red);
                continue;
            }

            if prev_sep {
                let s_filter = |s: &str| line.get(i + s.len()).map_or(true, |c| Self::is_sep(*c));
                for (color, kws) in &syntax.keywords {
                    for keyword in kws.iter().filter(|kw| find_str(kw) && s_filter(kw)) {
                        self.hl.extend(repeat(*color).take(keyword.len()));
                    }
                }
            }

            self.hl.push(Color::Default);
        }

        if let HlState::String(_) = self.hl_state {
            self.hl_state = HlState::Normal;
        }
        self.hl_state
    }

    pub fn draw(&self, offset: usize, max_len: usize, buffer: &mut String) -> Result<(), String> {
        let mut current_color = Color::Default;
        let chars = self.render.chars().skip(offset).take(max_len);
        let mut rx = self
            .render
            .chars()
            .take(offset)
            .map(|c| c.width().unwrap_or(1))
            .sum();
        for (c, mut color) in chars.zip(self.hl.iter().skip(offset)) {
            if c.is_ascii_control() {
                let c = if (c as u8) < 26 {
                    (b'@' + c as u8) as char
                } else {
                    '?'
                };
                write!(buffer, "{REVERSE_VIDEO}{c}{RESET_FMT}").map_err(|e| e.to_string())?;
                if current_color != Color::Default {
                    buffer.push_str(&current_color.to_string());
                }
            } else {
                if let Some(range) = &self.match_range {
                    if range.contains(&rx) {
                        color = &Color::CyanBG;
                    } else if rx == range.end {
                        buffer.push_str(RESET_FMT);
                    }
                }
                if current_color != *color {
                    buffer.push_str(&color.to_string());
                    current_color = *color;
                }
                buffer.push(c);
            }
            rx += c.width().unwrap_or(1);
        }
        buffer.push_str(RESET_FMT);
        Ok(())
    }
}
