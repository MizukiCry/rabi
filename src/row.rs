use std::ops::Range;

use crate::{Color, SyntaxConfig, HlState};

#[derive(Default)]
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

    pub fn update(&mut self, syntax: &SyntaxConfig, hl_state: HlState, tab: usize) -> HlState {
        self.render.clear();
        self.c2r.clear();
        self.r2c.clear();

        Default::default()
    }
}
