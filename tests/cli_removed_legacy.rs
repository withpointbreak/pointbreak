mod support;

use support::shore;

struct RemovedPath {
    argv: &'static [&'static str],
    /// Substrings the successor hint must contain (empty = no hint expected).
    hint_contains: &'static [&'static str],
}

// Family/rename tasks APPEND rows here.
const REMOVED_PATHS: &[RemovedPath] = &[
    // Fully removed review verbs: unregistered with no verb-specific successor
    // (the retired-`review` family hint is asserted separately).
    RemovedPath {
        argv: &["review", "publish", "--help"],
        hint_contains: &[],
    },
    RemovedPath {
        argv: &["review", "verdict", "--help"],
        hint_contains: &[],
    },
    RemovedPath {
        argv: &["review", "ack", "--help"],
        hint_contains: &[],
    },
    RemovedPath {
        argv: &["review", "disposition", "--help"],
        hint_contains: &[],
    },
    // Retired names that point at their post-reshape successors.
    RemovedPath {
        argv: &["review", "intervention", "--help"],
        hint_contains: &["shore input-request"],
    },
    RemovedPath {
        argv: &["review", "lineage", "--help"],
        hint_contains: &["shore capture --supersedes", "shore revision list"],
    },
    RemovedPath {
        argv: &["review", "unit", "--help"],
        hint_contains: &["shore revision list", "shore revision show"],
    },
    // Flattened families: `shore review <verb>` retired, points at `shore <verb>`.
    RemovedPath {
        argv: &["review", "capture", "--help"],
        hint_contains: &["shore capture"],
    },
    RemovedPath {
        argv: &["review", "history", "--help"],
        hint_contains: &["shore history"],
    },
    RemovedPath {
        argv: &["review", "endorse", "--help"],
        hint_contains: &["shore endorse"],
    },
    RemovedPath {
        argv: &["review", "observation", "add", "--help"],
        hint_contains: &["shore observation"],
    },
    RemovedPath {
        argv: &["review", "assessment", "add", "--help"],
        hint_contains: &["shore assessment"],
    },
    RemovedPath {
        argv: &["review", "validation", "add", "--help"],
        hint_contains: &["shore validation"],
    },
    RemovedPath {
        argv: &["review", "input-request", "open", "--help"],
        hint_contains: &["shore input-request"],
    },
    // The association grammar rewrite: compounds point at the new verbs, and the
    // family path points at the top-level family.
    RemovedPath {
        argv: &["review", "association", "associate-commit", "--help"],
        hint_contains: &["shore association record"],
    },
    RemovedPath {
        argv: &["review", "association", "associate-ref", "--help"],
        hint_contains: &["shore association record --ref"],
    },
    RemovedPath {
        argv: &["review", "association", "withdraw-commit", "--help"],
        hint_contains: &["shore association withdraw"],
    },
    RemovedPath {
        argv: &["review", "association", "withdraw-ref", "--help"],
        hint_contains: &["shore association withdraw"],
    },
    RemovedPath {
        argv: &["review", "association", "list", "--help"],
        hint_contains: &["shore association record|withdraw|list"],
    },
    // The old get-one verb `fetch` at the new top level points at `show`.
    RemovedPath {
        argv: &["input-request", "fetch", "--help"],
        hint_contains: &["shore input-request show"],
    },
    // The revision family: the verb-less plural and the digest both moved.
    RemovedPath {
        argv: &["review", "revisions", "--help"],
        hint_contains: &["shore revision list"],
    },
    RemovedPath {
        argv: &["review", "show", "--help"],
        hint_contains: &["shore revision show"],
    },
    // `identity enroll` renamed to `delegate` (the verb collided with `keys enroll`).
    RemovedPath {
        argv: &["identity", "enroll", "--help"],
        hint_contains: &["shore identity delegate <AGENT> --principal <P>"],
    },
    // The `keys` family noun singularized; subverbs are unchanged.
    RemovedPath {
        argv: &["keys", "init", "--help"],
        hint_contains: &["The `keys` family is now `key`"],
    },
    // The legacy working-tree surfaces retired end-to-end (ADR-0030 second amendment).
    RemovedPath {
        argv: &["dump", "--help"],
        hint_contains: &["shore diff", "shore inspect", "shore revision show"],
    },
    RemovedPath {
        argv: &["show", "--help"],
        hint_contains: &["shore diff", "shore inspect", "shore revision show"],
    },
    RemovedPath {
        argv: &["notes", "apply", "--help"],
        hint_contains: &["shore observation", "shore revision show"],
    },
];

#[test]
fn review_namespace_is_retired_with_a_hint() {
    for argv in [vec!["review"], vec!["review", "--help"]] {
        let out = shore(argv.clone());
        assert!(!out.status.success(), "{argv:?} should be unregistered");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("unrecognized subcommand"),
            "{argv:?} stderr:\n{err}"
        );
        // The bare-`review` leading-token hint points at the flattened surface.
        assert!(
            err.contains("flattened to the top level"),
            "{argv:?} stderr:\n{err}"
        );
    }
}

#[test]
fn removed_review_paths_are_unregistered_and_hint_at_successors() {
    for case in REMOVED_PATHS {
        let output = shore(case.argv.iter().copied());
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "{:?} must be rejected", case.argv);
        assert!(
            stderr.contains("unrecognized subcommand"),
            "{:?} should be unregistered:\n{stderr}",
            case.argv
        );
        for needle in case.hint_contains {
            assert!(
                stderr.contains(needle),
                "{:?} hint missing {needle:?}:\n{stderr}",
                case.argv
            );
        }
    }
}
