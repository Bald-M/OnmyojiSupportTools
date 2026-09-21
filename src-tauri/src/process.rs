use std::path::Path;

use tokio::process::Command;

#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

#[cfg(target_os = "windows")]
pub(crate) fn adb_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn adb_command(program: &Path) -> Command {
    Command::new(program)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use windows_sys::Win32::System::Console::GetConsoleWindow;

    #[test]
    #[ignore = "launched by the production ADB process entry-point tests"]
    fn console_window_probe() {
        assert!(unsafe { GetConsoleWindow() }.is_null());
    }
}
