use rabi::{Config, Editor};

fn main() -> Result<(), String> {
    let mut args = std::env::args();
    match (args.nth(1), args.len()) {
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
        (file_name, 0) => Editor::new(Config::load()?)?.run(file_name)?,
        _ => return Err(String::from("Arguments error. Run rabi --help for usage.")),
    }
    Ok(())
}
