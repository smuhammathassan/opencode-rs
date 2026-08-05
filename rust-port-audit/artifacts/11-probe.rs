use std::path::Path;
fn fs_contains(parent: &str, child: &str) -> bool {
    let parent_abs = std::path::absolute(parent).unwrap_or_else(|_| Path::new(parent).to_path_buf());
    let child_abs = std::path::absolute(child).unwrap_or_else(|_| Path::new(child).to_path_buf());
    match child_abs.strip_prefix(&parent_abs) {
        Ok(rest) => rest.as_os_str().is_empty() || !rest.starts_with(".."),
        Err(_) => false,
    }
}
fn main() {
    let ws = "/tmp/opencode/a11/ws";
    let _ = std::fs::remove_dir_all(ws);
    std::fs::create_dir_all(ws).unwrap();
    std::fs::create_dir_all("/tmp/opencode/a11/outside").unwrap();
    // symlink inside workspace pointing outside
    std::os::unix::fs::symlink("/tmp/opencode/a11/outside", format!("{ws}/link")).unwrap();
    // FIFO inside workspace
    std::process::Command::new("mkfifo").arg(format!("{ws}/fifo")).status().unwrap();
    // /dev/zero device file
    let target = format!("{ws}/link/evil.txt");
    println!("fs_contains(ws, ws/link/evil.txt) = {}", fs_contains(ws, &target));
    println!("symlink_metadata(ws/link/evil.txt): {:?}", std::fs::symlink_metadata(&target).map(|m| (m.is_file(), m.is_dir())).unwrap_or_else(|e| (false,false)));
    // write through the symlink (what oc-tool write_with_dirs does)
    std::fs::write(&target, "ESCAPED\n").unwrap();
    let out = std::fs::read("/tmp/opencode/a11/outside/evil.txt").unwrap();
    println!("outside file content after write-through-symlink: {:?}", String::from_utf8_lossy(&out));
    // device / FIFO metadata checks (read tool guard)
    for p in ["/dev/zero", &format!("{ws}/fifo")] {
        let m = std::fs::symlink_metadata(p).unwrap();
        println!("symlink_metadata({p}).is_file={} is_dir={} type={:?}", m.is_file(), m.is_dir(), m.file_type());
    }
}
