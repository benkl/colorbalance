fn main() {
    let mut command = std::process::Command::new("npm");
    command.args(["run", "build"]);
    command.current_dir("../frontend");
    #[cfg(windows)]
    {
        command = std::process::Command::new("cmd");
        command.args(["/c", "npm run build"]);
        command.current_dir("../frontend");
    }
    let _ = command.status();
    tauri_build::build()
}
