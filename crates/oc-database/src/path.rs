//! Path helpers for storage columns.
//!
//! Port of `reference/packages/core/src/database/path.ts`. The reference wraps
//! these in Drizzle custom column types (`absoluteColumn`, `directoryColumn`,
//! `pathColumn`, `absoluteArrayColumn`); here they are plain functions applied
//! at the query boundary by the CRUD helpers in [`crate::tables`].
//!
//! The reference types absolute values with `AbsolutePath` (from
//! `core/src/schema`); this crate keeps them as `String` until oc-schema
//! exposes the branded type.
//! // TODO(integration): promote to oc-schema once AbsolutePath exists.

use crate::error::{Error, Result};

/// On Windows, drive/UNC paths are persisted with forward slashes.
/// From reference/packages/core/src/database/path.ts:5
pub fn storage_path(input: &str) -> String {
    if cfg!(windows) {
        input.replace('\\', "/")
    } else {
        input.to_string()
    }
}

/// `C:\`-shaped or `\\server`-shaped storage paths.
/// From reference/packages/core/src/database/path.ts:10
pub fn is_windows_storage_path(input: &str) -> bool {
    let bytes = input.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/')
        || input.starts_with("//")
}

fn is_posix_absolute(input: &str) -> bool {
    input.starts_with('/')
}

/// Validate and normalize to storage form. Throws when the input is not an
/// absolute path. From reference/packages/core/src/database/path.ts:14
pub fn absolute(input: &str) -> Result<String> {
    let result = storage_path(input);
    if !is_posix_absolute(&result) && !(cfg!(windows) && is_windows_storage_path(&result)) {
        return Err(Error::Path(format!("Path is not absolute: {input}")));
    }
    Ok(result)
}

/// Convert storage form back to the host platform form.
/// From reference/packages/core/src/database/path.ts:22
pub fn to_platform(input: &str) -> String {
    if cfg!(windows) && is_windows_storage_path(input) {
        input.replace('/', "\\")
    } else {
        input.to_string()
    }
}

/// `absoluteColumn` driver round-trip.
/// From reference/packages/core/src/database/path.ts:27
pub fn absolute_column_to_driver(input: &str) -> Result<String> {
    absolute(input)
}

/// `absoluteColumn` read round-trip.
/// From reference/packages/core/src/database/path.ts:27
pub fn absolute_column_from_driver(input: &str) -> Result<String> {
    Ok(to_platform(&absolute(input)?))
}

/// `directoryColumn` write round-trip. Legacy sessions may persist an empty
/// directory; that value is kept readable while real directories are validated.
/// From reference/packages/core/src/database/path.ts:45
pub fn directory_column_to_driver(input: &str) -> Result<String> {
    if input.is_empty() {
        Ok(input.to_string())
    } else {
        absolute(input)
    }
}

/// `directoryColumn` read round-trip.
/// From reference/packages/core/src/database/path.ts:45
pub fn directory_column_from_driver(input: &str) -> Result<String> {
    if input.is_empty() {
        Ok(input.to_string())
    } else {
        Ok(to_platform(&absolute(input)?))
    }
}

/// `pathColumn` round-trip: storage normalization only, no validation.
/// From reference/packages/core/src/database/path.ts:61
pub fn path_column(input: &str) -> String {
    storage_path(input)
}

/// `absoluteArrayColumn` write round-trip: JSON array of absolute paths.
/// From reference/packages/core/src/database/path.ts:77
pub fn absolute_array_column_to_driver(input: &[String]) -> Result<String> {
    let abs: Result<Vec<String>> = input.iter().map(|path| absolute(path)).collect();
    Ok(serde_json::to_string(&abs?)?)
}

/// `absoluteArrayColumn` read round-trip.
/// From reference/packages/core/src/database/path.ts:77
pub fn absolute_array_column_from_driver(input: &str) -> Result<Vec<String>> {
    let items: Vec<String> = serde_json::from_str(input)?;
    items
        .iter()
        .map(|path| Ok(to_platform(&absolute(path)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_paths_pass_through() {
        assert_eq!(directory_column_to_driver("").unwrap(), "");
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_paths_pass_through() {
        assert_eq!(storage_path("/home/me/dir"), "/home/me/dir");
        assert_eq!(absolute("/home/me/dir").unwrap(), "/home/me/dir");
        assert!(absolute("not-absolute").is_err());
        assert_eq!(
            absolute_array_column_from_driver(r#"["/a/b","/c/d"]"#).unwrap(),
            vec!["/a/b", "/c/d"]
        );
        assert_eq!(
            absolute_array_column_to_driver(&["/a/b".into()]).unwrap(),
            r#"["/a/b"]"#
        );
        assert_eq!(directory_column_to_driver("").unwrap(), "");
        assert_eq!(directory_column_to_driver("/abs").unwrap(), "/abs");
    }
}
