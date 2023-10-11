use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::{parse_ini_file, parse_value, parse_values, Color};

#[derive(Default, Debug)]
pub struct SyntaxConfig {
    pub name: String,
    pub highlight_numbers: bool,
    pub slcomment_start: Vec<String>,
    pub slstring_quotes: Vec<char>,
    pub mlcomment_delims: Option<(String, String)>,
    pub mlstring_delims: Option<String>,
    pub keywords: Vec<(Color, Vec<String>)>,
}

impl SyntaxConfig {
    pub fn from_ext(ext: &str) -> Result<Option<Self>, String> {
        let dir_entries = PathBuf::from("./config/")
            .read_dir()
            .map_err(|e| e.to_string())?;
        for dir_entry in dir_entries {
            let dir_entry = dir_entry.map_err(|e| e.to_string())?;
            if dir_entry.path().file_name() == Some(OsStr::new("rabi.ini")) {
                continue;
            }
            let (config, extensions) = Self::from_file(&dir_entry.path())?;
            if extensions.contains(&ext.to_string()) {
                return Ok(Some(config));
            }
        }
        Ok(None)
    }

    pub fn from_file(path: &Path) -> Result<(Self, Vec<String>), String> {
        let mut config = Self::default();
        let mut extensions = Vec::new();
        parse_ini_file(path, &mut |key, value| {
            match key {
                "name" => config.name = parse_value(value)?,
                "extensions" => extensions.extend(value.split(',').map(|s| s.trim().to_string())),
                "highlight_numbers" => config.highlight_numbers = parse_value(value)?,
                "singleline_comment_start" => config.slcomment_start = parse_values(value)?,
                "singleline_string_quotes" => config.slstring_quotes = parse_values(value)?,
                "multiline_comment_delims" => {
                    config.mlcomment_delims = match &value.split(',').collect::<Vec<_>>()[..] {
                        [v1, v2] => Some((parse_value(v1)?, parse_value(v2)?)),
                        _ => return Err("mlcomment_delims must have two values".to_string()),
                    }
                }
                "multiline_string_delim" => config.mlstring_delims = Some(parse_value(value)?),
                "keywords_1" => config.keywords.push((Color::Yellow, parse_values(value)?)),
                "keywords_2" => config.keywords.push((Color::Magenta, parse_values(value)?)),
                _ => return Err(format!("Unknown key: {}", key)),
            }
            Ok(())
        })?;
        Ok((config, extensions))
    }
}

#[derive(Default, PartialEq, Clone, Copy, Debug)]
pub enum HlState {
    #[default]
    Normal,
    MlComment,
    String(u8),
    MlString,
}
