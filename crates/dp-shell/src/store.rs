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

/// The file store of a component: the part of its `Env` that keeps files.
/// It is a trait of its own because the trace writer and the proof files
/// need nothing else of an environment, and are also built where there is
/// no network (the `cluster` feature is off).
pub trait Files {
    /// The whole contents of a file.
    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>>;
    /// Writes a file so that, whenever the component stops, the file is
    /// absent, or as it was, or complete with these contents (P5.4.5).
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Writes a directory of files so that, whenever the component stops,
    /// the directory is absent or complete with these files (R5.4). A
    /// directory that is there already is kept as it is.
    fn write_dir(&mut self, dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()>;
    /// Adds bytes at the end of a file, which is created if it is absent.
    /// A complete trace is written this way (D-11).
    fn append_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// The file system, as a file store.
pub struct OsFiles;

impl Files for OsFiles {
    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        write_atomic(path, bytes)
    }

    fn write_dir(&mut self, dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
        write_dir_atomic(dir, files)
    }

    fn append_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut fs::OpenOptions::new().create(true).append(true).open(path)?, bytes)
    }
}

/// Writes a directory of files so that, whenever the component stops, the
/// directory is absent or complete (R5.4): the files are written and synced
/// in a temporary directory, which is then renamed; the rename is the event
/// that makes the directory complete. A directory that is there already is
/// kept as it is.
pub fn write_dir_atomic(dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let parent = dir.parent().ok_or_else(|| io::Error::other("the directory has no parent"))?;
    let name = dir.file_name().ok_or_else(|| io::Error::other("the directory has no name"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir(&tmp)?;
    let result = (|| {
        for (name, bytes) in files {
            let mut file = fs::File::create(tmp.join(name))?;
            io::Write::write_all(&mut file, bytes)?;
            file.sync_all()?;
        }
        fs::File::open(&tmp)?.sync_all()?;
        fs::rename(&tmp, dir)?;
        fs::File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
        // Another writer may have completed the same directory meanwhile.
        if dir.is_dir() {
            return Ok(());
        }
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
