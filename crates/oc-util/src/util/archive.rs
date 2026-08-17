/// From reference/packages/opencode/src/util/archive.ts
///
/// Extracts a zip archive. On Windows this shells out to PowerShell
/// `Expand-Archive`; everywhere else it shells out to `unzip`.
use crate::util::process::{run, RunError, RunOptions};

pub async fn extract_zip(zip_path: &str, dest_dir: &str) -> Result<(), RunError> {
    #[cfg(windows)]
    {
        let cmd = format!(
            "$global:ProgressPreference = 'SilentlyContinue'; Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            zip_path.replace('\'', "''"),
            dest_dir.replace('\'', "''")
        );
        run(
            &[
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                cmd,
            ],
            &RunOptions::default(),
        )
        .await
        .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        run(
            &[
                "unzip".to_string(),
                "-o".to_string(),
                "-q".to_string(),
                zip_path.to_string(),
                "-d".to_string(),
                dest_dir.to_string(),
            ],
            &RunOptions::default(),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::extract_zip;
    #[cfg(not(windows))]
    use std::path::PathBuf;

    #[cfg(not(windows))]
    fn tmp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oc-util-archive-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn extracts_zip_with_unzip() {
        use std::process::Command;

        if crate::which::which("unzip").is_none() {
            eprintln!("skipping: unzip not available");
            return;
        }

        let dir = tmp_dir("extract");
        let src = dir.join("src.zip");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();

        // Build a zip with python's zipfile (no `zip` CLI guaranteed).
        let build = dir.join("make.py");
        std::fs::write(
            &build,
            r#"import zipfile
with zipfile.ZipFile("src.zip", "w") as z:
    z.writestr("hello.txt", "hello world")
    z.writestr("nested/dir/file.txt", "nested")
"#,
        )
        .unwrap();
        let status = Command::new("python3")
            .current_dir(&dir)
            .arg("make.py")
            .status()
            .unwrap();
        assert!(status.success(), "python3 failed to create fixture zip");

        extract_zip(src.to_str().unwrap(), out.to_str().unwrap())
            .await
            .unwrap();
        let hello = std::fs::read_to_string(out.join("hello.txt")).unwrap();
        assert_eq!(hello, "hello world");
        let nested = std::fs::read_to_string(out.join("nested/dir/file.txt")).unwrap();
        assert_eq!(nested, "nested");
    }
}
