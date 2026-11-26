//! Crash-safe file writes: a file is either absent, or complete with the
//! contents of one write (P5.4.5).

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// The steps of an atomic write, in order. A writer that stops after one
/// of them stands for a process that was ended there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The temporary file exists and is empty.
    Created,
    /// Half of the bytes are in the temporary file.
    HalfWritten,
    /// All bytes are in the temporary file, and synced.
    Synced,
    /// The temporary file has the name of the target.
    Renamed,
}

pub const STEPS: [Step; 4] = [Step::Created, Step::HalfWritten, Step::Synced, Step::Renamed];

/// Writes `bytes` to `path` atomically: a temporary file in the same
/// directory is written and synced, renamed over `path`, and the directory
/// is synced.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_until(path, bytes, None)
}

/// `write_atomic` that stops after `stop`, as a process that was ended
/// there would: nothing after it happens, and nothing is cleaned up. For
/// tests of what such a writer leaves behind.
pub fn write_until(path: &Path, bytes: &[u8], stop: Option<Step>) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("a file path has a parent"))?;
    fs::create_dir_all(dir)?;
    let name = path.file_name().ok_or_else(|| io::Error::other("a file path has a name"))?;
    let tmp = dir.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let mut stopped = false;
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        if stop == Some(Step::Created) {
            stopped = true;
            return Ok(());
        }
        let half = bytes.len() / 2;
        file.write_all(&bytes[..half])?;
        if stop == Some(Step::HalfWritten) {
            stopped = true;
            return Ok(());
        }
        file.write_all(&bytes[half..])?;
        file.sync_all()?;
        if stop == Some(Step::Synced) {
            stopped = true;
            return Ok(());
        }
        fs::rename(&tmp, path)?;
        if stop == Some(Step::Renamed) {
            stopped = true;
            return Ok(());
        }
        fs::File::open(dir)?.sync_all()
    })();
    if result.is_err() && !stopped {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_replaces_the_file_and_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("dp-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("a/b/entry");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    /// P5.4.5: a writer that is ended at any step leaves the file as it
    /// was or complete with the new contents, never in part; and what it
    /// leaves behind does not stop the next writer.
    #[test]
    fn a_write_that_is_ended_at_any_step_leaves_the_old_or_the_new_file() {
        let dir = std::env::temp_dir().join(format!("dp-store-crash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("entry");
        let (old, new) = (b"the old contents".to_vec(), b"the new contents, which are longer".to_vec());
        for existing in [false, true] {
            for step in STEPS {
                let _ = fs::remove_dir_all(&dir);
                if existing {
                    write_atomic(&path, &old).unwrap();
                }
                write_until(&path, &new, Some(step)).unwrap();
                let found = fs::read(&path).ok();
                let expected = match (step, existing) {
                    (Step::Renamed, _) => Some(new.clone()),
                    (_, true) => Some(old.clone()),
                    (_, false) => None,
                };
                assert_eq!(found, expected, "{step:?}, existing {existing}");
                // A reader of the directory finds no entry in what was left:
                // every other file is a temporary one, by its name.
                for other in fs::read_dir(&dir).unwrap().flatten() {
                    let name = other.file_name().to_string_lossy().into_owned();
                    assert!(name == "entry" || (name.starts_with('.') && name.ends_with(".tmp")), "{name}");
                }
                // The next writer completes, and leaves nothing behind.
                write_atomic(&path, b"next").unwrap();
                assert_eq!(fs::read(&path).unwrap(), b"next");
                assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "{step:?}");
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// The temporary file of a writer of another process, ended before its
    /// rename, is left alone and is never taken for the file.
    #[test]
    fn what_another_writer_left_is_not_the_file() {
        let dir = std::env::temp_dir().join(format!("dp-store-other-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("entry");
        let stale = dir.join(".entry.1.tmp");
        fs::write(&stale, b"half of some").unwrap();
        assert!(fs::read(&path).is_err());
        write_atomic(&path, b"whole").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"whole");
        assert_eq!(fs::read(&stale).unwrap(), b"half of some");
        fs::remove_dir_all(dir).unwrap();
    }

    /// A write that cannot be done reports it, and leaves no temporary file.
    #[test]
    fn a_write_that_fails_is_reported_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("dp-store-fail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // The target is a directory that is not empty: the rename fails.
        let path = dir.join("entry");
        fs::create_dir_all(path.join("inside")).unwrap();
        assert!(write_atomic(&path, b"x").is_err());
        let names: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("entry")]);
        // The parent is a file: nothing can be created under it.
        fs::write(dir.join("file"), b"x").unwrap();
        assert!(write_atomic(&dir.join("file/entry"), b"x").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
