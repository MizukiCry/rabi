use std::path::PathBuf;

use rabi::{Config, Editor};

fn main() -> Result<(), String> {
    let mut args = std::env::args();
    let mut config_folder = PathBuf::from(args.next().unwrap());
    config_folder.pop();
    config_folder.push("config");
    // eprintln!("config_folder: {}", config_folder.display());
    match (args.next(), args.len()) {
        (Some(arg), 0) if arg == "--help" => {
            println!(
                "Rabi - A simple text editor.\n\
                Usage:\n\
                rabi        # Create a new file.\n\
                rabi <file> # Open the specified file.\n\
                rabi --help # Show this help message.\n"
            );
        }
        (Some(arg), 0) if arg.starts_with('-') => {
            return Err(String::from("Arguments error. Run rabi --help for usage."))
        }
        (file_name, 0) => Editor::new(Config::load(config_folder)?)?.run(file_name)?,
        _ => return Err(String::from("Arguments error. Run rabi --help for usage.")),
    }
    Ok(())
}
