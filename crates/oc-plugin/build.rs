fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        cc::Build::new()
            .file("src/compat_windows.c")
            .compile("quickjs_win_compat");
    }
}
