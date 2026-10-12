//! Size-limited file reading and link detection.

use std::fs;
use std::io::Read;
use std::path::Path;

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadError {
    /// The file is larger than the limit.
    TooLarge,
    /// The file is not valid UTF-8 (text reads only).
    NotUtf8,
    /// An I/O error, as an English message.
    Io(String),
}

/// Returns whether an entry is a symbolic link or a name-surrogate reparse point (such as an
/// NTFS junction). Other reparse points (cloud placeholders and similar) are not links.
pub(crate) fn is_link(file_type: &fs::FileType) -> bool {
    file_type.is_symlink()
}

/// Reads a file of at most `max_bytes` bytes.
///
/// The size is checked before opening, and again while reading, in case the file grew in
/// between. The caller has already checked that the path is not a link.
pub(crate) fn read_limited(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ReadError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| ReadError::Io(e.to_string()))?;
    if metadata.len() > max_bytes {
        return Err(ReadError::TooLarge);
    }
    let file = fs::File::open(path).map_err(|e| ReadError::Io(e.to_string()))?;
    let mut buf = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| ReadError::Io(e.to_string()))?;
    if buf.len() as u64 > max_bytes {
        return Err(ReadError::TooLarge);
    }
    Ok(buf)
}

/// Reads a UTF-8 text file of at most `max_bytes` bytes. See [`read_limited`].
pub(crate) fn read_text_limited(path: &Path, max_bytes: u64) -> Result<String, ReadError> {
    let bytes = read_limited(path, max_bytes)?;
    String::from_utf8(bytes).map_err(|_| ReadError::NotUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_size_limit_and_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "12345").unwrap();
        assert_eq!(read_text_limited(&p, 5).unwrap(), "12345");
        assert_eq!(read_text_limited(&p, 4), Err(ReadError::TooLarge));
        fs::write(&p, [0xff, 0xfe]).unwrap();
        assert_eq!(read_text_limited(&p, 10), Err(ReadError::NotUtf8));
        assert!(matches!(
            read_text_limited(&dir.path().join("missing"), 10),
            Err(ReadError::Io(_))
        ));
    }
}
