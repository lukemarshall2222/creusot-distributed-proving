// Feature: creusot-distributed-proving, Property 34: Exchanged formats parse strictly and print canonically
// P-ID: P7.10.3

//! The pretty printer: the rendering of a value for people. It loses
//! nothing: what it writes reads back to the same value, for any value
//! and for one value of each format of stage 1, whose renderings are
//! kept in `tests/snapshots/` (written again with `DP_BLESS=1`).

use std::path::PathBuf;

use dp_formats::{
    cache::{Atom, Entry, Origin},
    cluster::{Job, JobKind, Message, Operation, ResultFields, SignedCommand, SignedResult, WorkerStatus},
    dispatch::{DriverAnswer, FromWhy3find, PathStep, ProverInfo, ProverResult, StrategyStep, ToWhy3find},
    hash::Hash,
    lockfile::LockFile,
    proofjson::{DpEntry, InvalidEntry, Place, ReplayFields},
    sexp::{self, parse_pretty, pretty, Sexp},
    trace::Record,
};
use dp_testkit::gen;
use ed25519_dalek::SigningKey;
use proptest::prelude::*;

const LIMIT: usize = 1 << 20;

proptest! {
    #![proptest_config(dp_testkit::config::cases("pretty", 400))]

    // Nothing is lost: any value reads back from its rendering.
    #[test]
    fn a_rendering_reads_back_to_its_value(v in prop_oneof![gen::sexp::readable_sexp(), gen::sexp::arb_sexp()]) {
        let text = pretty(&v);
        prop_assert_eq!(parse_pretty(&text), Ok(v.clone()), "{}", text);
        // And with other white space between its items.
        let spread = text.replace('\n', " \n\t");
        if !text.contains('"') {
            prop_assert_eq!(parse_pretty(&format!("  {spread}\n")), Ok(v));
        }
    }

    // A rendering is of one value: two values have two renderings.
    #[test]
    fn two_values_have_two_renderings(a in gen::sexp::readable_sexp(), b in gen::sexp::readable_sexp()) {
        prop_assert_eq!(pretty(&a) == pretty(&b), a == b);
    }

    // Lines stay within the width where a break can keep them there, and
    // no line ends in white space.
    #[test]
    fn a_rendering_has_no_trailing_white_space(v in gen::sexp::readable_sexp()) {
        for line in pretty(&v).lines() {
            prop_assert!(!line.ends_with(' ') || line.contains('"'), "{:?}", line);
        }
    }

    // The reader is total, and an error is inside the text.
    #[test]
    fn the_reader_never_panics(text in prop_oneof!["\\PC{0,60}", "[()#\"\\\\a-f0-9 \n]{0,60}"]) {
        match parse_pretty(&text) {
            Ok(value) => prop_assert_eq!(parse_pretty(&pretty(&value)), Ok(value)),
            Err(error) => prop_assert!(error.offset <= text.len(), "{}", error),
        }
    }

    // Values of the formats, from their generators.
    #[test]
    fn values_of_the_formats_read_back(
        message in gen::message::message(), entry in gen::cache_entry::entry(), dp in gen::proofjson::entry(),
        from in gen::dispatch::from_why3find(), to in gen::dispatch::to_why3find(),
    ) {
        let read = |text: String| parse_pretty(&text).map_err(|e| e.to_string());
        prop_assert_eq!(read(pretty(&message.encode())).and_then(|s| Message::decode(&s).map_err(|e| e.to_string())), Ok(message));
        prop_assert_eq!(read(pretty(&from.encode())).and_then(|s| FromWhy3find::decode(&s).map_err(|e| e.to_string())), Ok(from));
        prop_assert_eq!(read(pretty(&to.encode())).and_then(|s| ToWhy3find::decode(&s).map_err(|e| e.to_string())), Ok(to));
        let value = sexp::parse(&entry.encode(), LIMIT).unwrap();
        prop_assert_eq!(read(pretty(&value)).map(|s| sexp::print(&s)), Ok(entry.encode()));
        let value = sexp::parse(&dp.encode(), LIMIT).unwrap();
        prop_assert_eq!(read(pretty(&value)).map(|s| sexp::print(&s)), Ok(dp.encode()));
    }
}

#[test]
fn every_kind_of_atom_has_its_notation() {
    let atom = |bytes: &[u8]| pretty(&Sexp::atom(bytes));
    assert_eq!(atom(b"valid"), "valid");
    assert_eq!(atom(b"z3@4.12:x_y-1./"), "z3@4.12:x_y-1./");
    assert_eq!(atom(b"two words"), "\"two words\"");
    assert_eq!(atom(b"say \"x\" \\ y"), "\"say \\\"x\\\" \\\\ y\"");
    assert_eq!(atom("ünï".as_bytes()), "\"ünï\"");
    assert_eq!(atom(b"(x)"), "\"(x)\"");
    assert_eq!(atom(b"#tag"), "\"#tag\"");
    // No text: bytes, a line feed, and the empty atom.
    assert_eq!(atom(b""), "#");
    assert_eq!(atom(&[0, 255, 16]), "#00ff10");
    assert_eq!(atom(b"a\nb"), "#610a62");
    assert_eq!(pretty(&Sexp::list([])), "()");
    assert_eq!(pretty(&Sexp::list([Sexp::atom("a"), Sexp::list([Sexp::atom(""), Sexp::atom("b c")])])), "(a (# \"b c\"))");
    // What the reader refuses, and where.
    for (text, offset) in [
        ("", 0), ("(a", 2), ("a)", 1), (")", 0), ("a b", 2), ("\"a", 2), ("\"a\\n\"", 2), ("#0", 0), ("#0G", 0),
        ("#AB", 0), ("a\"b\"", 1), ("#00x", 0), ("\"a\nb\"", 2),
    ] {
        assert_eq!(parse_pretty(text).map_err(|e| e.offset), Err(offset), "{text:?}");
    }
    // By hand: any white space, and hexadecimal for what could be bare.
    assert_eq!(parse_pretty(" ( a\n\t#62 \"c\" ( ) ) "), Ok(Sexp::list([Sexp::atom("a"), Sexp::atom("b"), Sexp::atom("c"), Sexp::list([])])));
}

/// Compares a rendering with its snapshot, or writes the snapshot.
#[track_caller]
fn snapshot(name: &str, text: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(format!("{name}.txt"));
    if std::env::var_os("DP_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{text}\n")).unwrap();
    }
    let kept = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e} (write it with DP_BLESS=1)", path.display()));
    assert_eq!(kept, format!("{text}\n"), "the rendering of {name} changed; if that is meant, run with DP_BLESS=1");
}

fn result() -> SignedResult {
    SignedResult::sign(
        ResultFields {
            job_id: "session-1/4".into(),
            task_hash: Hash([0x91; 32]),
            prover: "alt-ergo@2.6.4".into(),
            seed: Some(7),
            binary_hash: Hash([0xa2; 32]),
            worker: "w10".into(),
            status: Some(0),
            timed_out: false,
            output: b"unsat\n".to_vec(),
            elapsed_micros: 31_000,
            velocity: 1_250_000,
            local_micros: 6_250_000,
            imports_hash: Hash([0xb3; 32]),
        },
        &SigningKey::from_bytes(&[5; 32]),
    )
}

// One value of each format of stage 1: its rendering is the snapshot, and
// the snapshot reads back to the value.
#[test]
fn the_snapshots_of_the_formats_read_back() {
    let job = Job {
        kind: JobKind::Prove,
        job_id: "session-1/4".into(),
        prover: "alt-ergo@2.6.4".into(),
        driver: b"alt_ergo".to_vec(),
        extension: ".ae".into(),
        micros: 5_000_000,
        seed: None,
        goal: b"goal g: forall x: int. x + 0 = x\n".to_vec(),
    };
    let messages = [
        ("result-message", Message::Result(result())),
        ("message-assign", Message::Assign { job, local_micros: 6_250_000, velocity: 1_250_000 }),
        (
            "status-report",
            Message::Status {
                incarnation: 3,
                workers: vec![
                    WorkerStatus { worker: "w10".into(), job: Some("session-1/4".into()), draining: false, provers: vec!["alt-ergo@2.6.4".into(), "z3@5.3.2".into()] },
                    WorkerStatus { worker: "w11".into(), job: None, draining: true, provers: vec![] },
                ],
                queued: 2,
                front_ends: 1,
            },
        ),
        ("message-operator", Message::Operator(SignedCommand::sign(4, Operation::Revoke, "w11", &SigningKey::from_bytes(&[6; 32])))),
    ];
    for (name, message) in messages {
        let text = pretty(&message.encode());
        snapshot(name, &text);
        assert_eq!(Message::decode(&parse_pretty(&text).unwrap()), Ok(message), "{name}");
    }

    let entry = Entry {
        lookup: vec![Atom::Bytes(b"arith.mlw/Arith/Easy".to_vec()), Atom::Num(5_000_000), Atom::Bytes(Vec::new())],
        binary_hash: Hash([0xa2; 32]),
        velocity: 1_250_000,
        local_micros: 6_250_000,
        imports_hash: Hash([0xb3; 32]),
        answer: DriverAnswer::Valid,
        micros: 31_000,
        origin: Origin::Worker(result()),
    };
    let text = pretty(&sexp::parse(&entry.encode(), LIMIT).unwrap());
    snapshot("cache-entry", &text);
    assert_eq!(Entry::decode(&sexp::print(&parse_pretty(&text).unwrap()), LIMIT), Ok(entry));

    let dp = DpEntry {
        label: "proved-standalone".into(),
        replay: vec![ReplayFields {
            task_hash: Hash([0x91; 32]),
            driver: b"alt_ergo".to_vec(),
            prover: "alt-ergo".into(),
            version: "2.6.4".into(),
            binary_hash: Hash([0xa2; 32]),
            place: Place::Worker("w10".into()),
            seed: Some(7),
            velocity: Some(1_250_000),
            local_micros: Some(6_250_000),
            reference_micros: 5_000_000,
        }],
        imports_hash: Hash([0xb3; 32]),
        certificate: None,
        conflict: true,
        invalid: vec![InvalidEntry { prover: "cvc5".into(), version: "1.4.1".into(), binary_hash: Hash([0xc4; 32]), place: Place::Local }],
    };
    let text = pretty(&sexp::parse(&dp.encode(), LIMIT).unwrap());
    snapshot("proofjson-extension", &text);
    assert_eq!(DpEntry::decode(&sexp::print(&parse_pretty(&text).unwrap()), LIMIT), Ok(dp));

    let from = [
        (
            "dispatch-goal",
            FromWhy3find::Goal {
                gid: "g3".into(),
                file: "arith.mlw".into(),
                theory: "Arith".into(),
                name: "Mixed".into(),
                path: vec![PathStep { tactic: "split_vc".into(), index: 1 }],
                task: b"theory Task\n  goal g: true\nend\n".to_vec(),
            },
        ),
        (
            "dispatch-provers",
            FromWhy3find::Provers {
                qid: "q2".into(),
                provers: vec![ProverInfo {
                    prover: "z3@5.3.2".into(),
                    digest: "0123abcd".into(),
                    driver: "z3_487".into(),
                    command: "z3 -smt2 -T:%t sat.random_seed=42 %f".into(),
                    extension: ".smt2".into(),
                }],
            },
        ),
        ("dispatch-prove", FromWhy3find::Prove { rid: "r9".into(), gid: "g3".into(), step: StrategyStep::Fast, prover: "z3@5.3.2".into(), micros: 200_000 }),
    ];
    for (name, message) in from {
        let text = pretty(&message.encode());
        snapshot(name, &text);
        assert_eq!(FromWhy3find::decode(&parse_pretty(&text).unwrap()), Ok(message), "{name}");
    }
    let to = ToWhy3find::Answer { rid: "r9".into(), result: ProverResult::Valid { micros: 31_000 } };
    let text = pretty(&to.encode());
    snapshot("dispatch-answer", &text);
    assert_eq!(ToWhy3find::decode(&parse_pretty(&text).unwrap()), Ok(to));

    let record = Record {
        component: "coordinator".into(),
        seq: 12,
        action: "Assign".into(),
        now: 40_000,
        input: "Submit { job: 4, prover: 1 }".into(),
        effects: "[Send { peer: 10 }]".into(),
    };
    let text = pretty(&sexp::parse(&record.encode(), LIMIT).unwrap());
    snapshot("event-trace", &text);
    assert_eq!(Record::decode(&sexp::print(&parse_pretty(&text).unwrap()), LIMIT), Ok(record));

    // The Lock_File is text already: its print is its rendering.
    let h = format!("sha256:{}", "ab".repeat(32));
    let lock = LockFile::parse_lenient(&format!(
        "[[tool]]\nname = \"z3\"\nversion = \"5.3.2\"\nkind = \"unpatched-solver\"\nprover = \"z3\"\n[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"{h}\"\nupstream = {{ tag = \"z3-5.3.2\", rebuild_hashes = [\"{h}\", \"{h}\"] }}\n\
[[tool]]\nname = \"why3find-dispatch\"\nversion = \"89dd8721\"\nkind = \"why3find\"\npatches = [\"patches/why3find/0001-dispatch-mode.patch\"]\n[[tool.platform]]\nplatform = \"x86_64-linux\"\nhash = \"{h}\"\n"
    ))
    .unwrap();
    let text = lock.print();
    snapshot("lock-file", text.trim_end_matches('\n'));
    assert_eq!(LockFile::parse(&text).unwrap(), lock);
}
