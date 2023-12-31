use std::{
    fmt::Display,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    str::FromStr,
};

// Configuration for rabi
#[derive(Default, Debug)]
pub struct Config {
    // The size of tab, default is 4
    pub tab_stop: usize,

    pub quit_times: usize,

    // The duration of shown message, in seconds
    pub message_duration: usize,

    // Whether to show line numbers
    pub show_line_numbers: bool,

    pub config_folder: PathBuf,
}

impl Config {
    pub fn load(config_folder: PathBuf) -> Result<Self, String> {
        let mut config = Config {
            tab_stop: 4,
            quit_times: 2,
            message_duration: 5,
            show_line_numbers: true,
            config_folder: config_folder.clone(),
        };
        parse_ini_file(
            config_folder.join("rabi.ini").as_path(),
            &mut |key, value| {
                match key {
                    "tab_stop" => match parse_value(value)? {
                        0 => return Err("tab_stop must be greater than 0".to_string()),
                        v => config.tab_stop = v,
                    },
                    "quit_times" => match parse_value(value)? {
                        0 => return Err("quit_times must be greater than 0".to_string()),
                        v => config.quit_times = v,
                    },
                    "message_duration" => config.message_duration = parse_value(value)?,
                    "show_line_numbers" => config.show_line_numbers = parse_value(value)?,
                    _ => return Err("Unknown key in configuration file: {key}".to_string()),
                }
                Ok(())
            },
        )?;
        Ok(config)
    }
}

pub fn parse_ini_file(
    path: &Path,
    func: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    // eprintln!("parse {}", path.display());
    let file = File::open(path).map_err(|e| e.to_string())?;
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        let mut parts = line.trim().splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some(""), None) | (None, _) => (),
            (Some(comment), _) if comment.starts_with(&['#', ';'][..]) => (),
            (Some(key), Some(value)) => func(key.trim_end(), value)?,
            (Some(_), None) => {
                return Err(format!("Syntax error on configuration file line {}", i + 1))
            }
        }
    }
    Ok(())
}

pub fn parse_value<T: FromStr<Err = impl Display>>(value: &str) -> Result<T, String> {
    value
        .trim()
        .parse()
        .map_err(|e| format!("Parser error: {e}"))
}

pub fn parse_values<T: FromStr<Err = impl Display>>(value: &str) -> Result<Vec<T>, String> {
    value.split(',').map(parse_value).collect()
}
