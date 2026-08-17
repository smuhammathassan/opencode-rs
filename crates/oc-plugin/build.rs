fn main() {
    #[cfg(target_os = "windows")]
    {
        cc::Build::new()
            .file("src/compat_windows.c")
            .compile("quickjs_win_compat");
    }
}
