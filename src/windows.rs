use winapi_util::{console, HandleRef};

pub type TerminalMode = (u32, u32);

pub fn get_winsize() -> Result<(usize, usize), String> {

    todo!()
}

pub const fn monitor_winsize() -> Result<(), String> {
    Ok(())
}

pub const fn winsize_changed() -> bool {
    false
}

pub fn set_terminal_mode((stdin_mode, stdout_mode): TerminalMode) -> Result<(), String> {
    console::set_mode(HandleRef::stdin(), stdin_mode).map_err(|e| e.to_string())?;
    console::set_mode(HandleRef::stdout(), stdout_mode).map_err(|e| e.to_string())
}

pub fn enable_raw_mode() -> Result<TerminalMode, String> {

    todo!()
}