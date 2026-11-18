use dp_formats::hash::{goal_hash, h, imports_hash, sha256, task_hash};
use dp_formats::sexp::Sexp;

#[test]
fn sha256_matches_the_standard_test_vector() {
    assert_eq!(
        sha256(b"abc").to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn domain_tags_separate_hashes() {
    let x = [Sexp::atom(b"same bytes")];
    assert_ne!(h("dp/task/v1", &x), h("dp/goal/v1", &x));
    assert_ne!(goal_hash(b"x"), imports_hash(&[]));
}

#[test]
fn field_boundaries_are_part_of_the_hash() {
    // length-prefixed atoms: moving a byte between fields changes the hash
    assert_ne!(task_hash(b"ab", b"c", b""), task_hash(b"a", b"bc", b""));
    assert_ne!(task_hash(b"goal", b"d", b"p"), task_hash(b"goal", b"p", b"d"));
}
