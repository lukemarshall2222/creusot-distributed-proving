//! `arb_checker_tuple`: what the shell establishes about a checker run
//! (Property 27).

use dp_core::verdict::CheckFacts;
use proptest::prelude::*;

/// The facts of a checker run, each mostly true so that the cases near
/// an acceptance are frequent.
pub fn facts() -> impl Strategy<Value = CheckFacts> {
    let mostly = || prop::bool::weighted(0.8);
    (mostly(), mostly(), mostly(), mostly(), mostly(), mostly(), mostly()).prop_map(
        |(produced, checker_pinned, signature_pinned, bound_to_local_print, accepted, complete, stored)| CheckFacts {
            produced,
            checker_pinned,
            signature_pinned,
            bound_to_local_print,
            accepted,
            complete,
            stored,
        },
    )
}

/// The facts as bits, in the order of the fields.
pub fn bits(f: CheckFacts) -> [bool; 7] {
    [f.produced, f.checker_pinned, f.signature_pinned, f.bound_to_local_print, f.accepted, f.complete, f.stored]
}
