//! `kata-app`: what the Start menu and desktop shortcuts open.
//!
//! A windowed program (no console flashes up) that starts Kata's page in the
//! background, or reuses the one already running, and opens it in the browser.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::Command;

fn main() {
    let exe = std::env::current_exe().expect("own path");
    let dir = exe.parent().expect("own folder");
    let web = dir.join(if cfg!(windows) {
        "kata-web.exe"
    } else {
        "kata-web"
    });
    let mut command = Command::new(web);
    command.arg("--open").current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Err(error) = command.spawn() {
        eprintln!("kata-app: could not start kata-web: {error}");
        std::process::exit(1);
    }
}
