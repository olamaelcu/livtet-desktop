#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some(livtet_desktop_lib::KOREADER_DAEMON_FLAG) {
        std::process::exit(livtet_desktop_lib::run_koreader_daemon());
    }

    livtet_desktop_lib::run()
}
