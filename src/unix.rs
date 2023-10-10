// TODO: unix support

pub use libc::termios as TerminalMode;

pub fn monitor_winsize() -> Result<(), String> {
    todo!()
}
