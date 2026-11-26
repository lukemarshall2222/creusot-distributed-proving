// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.8

//! The Lock_File of this repository is in its canonical layout, and a
//! Lock_File in any other layout is refused.

use dp_formats::lockfile::{LockFile, HEADER};

fn repository() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../deps/lock.toml")).unwrap()
}

#[test]
fn the_lock_file_of_the_repository_is_canonical() {
    let text = repository();
    let lock = LockFile::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(lock.print(), text);
    assert!(lock.tool.len() >= 17);
}

#[test]
fn an_empty_lock_file_is_its_header() {
    assert_eq!(LockFile::parse(HEADER).unwrap().tool.len(), 0);
    assert_eq!(LockFile::parse(HEADER).unwrap().print(), HEADER);
    assert!(LockFile::parse("").is_err());
    assert!(LockFile::parse_lenient("").is_ok());
}

// Every change of layout that leaves the contents as they are: the text
// still reads leniently to the same Lock_File, and `parse` refuses it with
// the offset of the change.
#[test]
fn another_layout_of_the_same_contents_is_refused() {
    let text = repository();
    let lock = LockFile::parse(&text).unwrap();
    let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle:?} is in the Lock_File"));
    let changes: Vec<(String, usize)> = vec![
        // No header, another header, a comment, an empty line more or less.
        (text[HEADER.len()..].to_string(), 0),
        (text.replacen("do not edit", "do not touch", 1), at("do not edit") + 7),
        (text.replacen("[[tool]]\n", "[[tool]]\n# a note\n", 1), at("[[tool]]\n") + 9),
        (text.replacen("\n\n[[tool.platform]]", "\n[[tool.platform]]", 1), at("\n\n[[tool.platform]]") + 1),
        (format!("{text}\n"), text.len()),
        (text.trim_end().to_string(), text.len() - 1),
        // Spaces, quotes and the order of fields.
        (text.replacen("name = ", "name=", 1), at("name = ") + 4),
        (text.replacen("version = \"", "version = '", 1).replacen("\"\nkind", "'\nkind", 1), at("version = \"") + 10),
        (
            {
                let start = at("name = ");
                let end = start + text[start..].find("kind = ").unwrap();
                let (name, version) = text[start..end].split_once('\n').unwrap();
                format!("{}{version}{name}\n{}", &text[..start], &text[end..])
            },
            at("name = "),
        ),
        (text.replacen("rebuild_hashes = []", "rebuild_hashes = [ ]", 1), at("rebuild_hashes = []") + 18),
        (text.replacen("upstream = { tag", "upstream = {tag", 1), at("upstream = { tag") + 12),
        // Windows line ends.
        (text.replacen('\n', "\r\n", 1), at("\n")),
    ];
    for (changed, offset) in changes {
        assert_ne!(changed, text);
        let lenient = LockFile::parse_lenient(&changed).unwrap_or_else(|e| panic!("{e}: {}", &changed[..200.min(changed.len())]));
        assert_eq!(lenient, lock, "the change is one of layout only");
        let refused = LockFile::parse(&changed).unwrap_err().to_string();
        assert_eq!(refused, format!("Lock_File: lock-file: not the canonical form of its value, at byte {offset}"));
    }
}

// What the printer writes for every shape of an entry reads back.
#[test]
fn every_shape_of_an_entry_prints_and_reads_back() {
    let h = format!("sha256:{}", "ab".repeat(32));
    let text = format!(
        "{HEADER}\n[[tool]]\nname = \"z3\"\nversion = \"1\"\nkind = \"unpatched-solver\"\nprover = \"z3\"\n\n\
[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"{h}\"\nupstream = {{ published_checksum = \"{h}\", tag = \"z3-1\", rebuild_hashes = [\"{h}\", \"{h}\"] }}\n\n\
[[tool.platform]]\nplatform = \"aarch64-darwin\"\nhash = \"{h}\"\n\n\
[[tool]]\nname = \"dep\"\nversion = \"2\"\nkind = \"why3find\"\npatches = [\"patches/dep/0001.patch\", \"patches/dep/0002.patch\"]\n\n\
[[tool]]\nname = \"later\"\nversion = \"3\"\nkind = \"lean\"\n"
    );
    let lock = LockFile::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(lock.print(), text);
    assert_eq!((lock.tool.len(), lock.tool[0].platform.len(), lock.tool[1].patches.len()), (3, 2, 2));
    // A string that would need an escape has no canonical text.
    assert!(LockFile::parse_lenient(&text.replace("z3-1", "z3\\\"1")).is_err());
    assert!(LockFile::parse_lenient(&text.replace("z3-1", "z3\\\\1")).is_err());
    assert!(LockFile::parse_lenient(&text.replace("z3-1", "z3\\n1")).is_err());
}
