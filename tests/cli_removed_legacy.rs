mod support;

use support::pointbreak;

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
        hint_contains: &["pointbreak input-request"],
    },
    RemovedPath {
        argv: &["review", "lineage", "--help"],
        hint_contains: &[
            "pointbreak capture --review-cursor <token>",
            "--advance replace|parallel",
        ],
    },
    RemovedPath {
        argv: &["review", "unit", "--help"],
        hint_contains: &["pointbreak revision list", "pointbreak revision show"],
    },
    // Flattened families: `pointbreak review <verb>` retired, points at `pointbreak <verb>`.
    RemovedPath {
        argv: &["review", "capture", "--help"],
        hint_contains: &["pointbreak capture"],
    },
    RemovedPath {
        argv: &["review", "history", "--help"],
        hint_contains: &["pointbreak history"],
    },
    RemovedPath {
        argv: &["review", "endorse", "--help"],
        hint_contains: &["pointbreak endorse"],
    },
    RemovedPath {
        argv: &["review", "observation", "add", "--help"],
        hint_contains: &["pointbreak observation"],
    },
    RemovedPath {
        argv: &["review", "assessment", "add", "--help"],
        hint_contains: &["pointbreak assessment"],
    },
    RemovedPath {
        argv: &["review", "validation", "add", "--help"],
        hint_contains: &["pointbreak validation"],
    },
    RemovedPath {
        argv: &["review", "input-request", "open", "--help"],
        hint_contains: &["pointbreak input-request"],
    },
    // The association grammar rewrite: compounds point at the new verbs, and the
    // family path points at the top-level family.
    RemovedPath {
        argv: &["review", "association", "associate-commit", "--help"],
        hint_contains: &["pointbreak association record"],
    },
    RemovedPath {
        argv: &["review", "association", "associate-ref", "--help"],
        hint_contains: &["pointbreak association record --ref"],
    },
    RemovedPath {
        argv: &["review", "association", "withdraw-commit", "--help"],
        hint_contains: &["pointbreak association withdraw"],
    },
    RemovedPath {
        argv: &["review", "association", "withdraw-ref", "--help"],
        hint_contains: &["pointbreak association withdraw"],
    },
    RemovedPath {
        argv: &["review", "association", "list", "--help"],
        hint_contains: &["pointbreak association record|withdraw|list"],
    },
    // The old get-one verb `fetch` at the new top level points at `show`.
    RemovedPath {
        argv: &["input-request", "fetch", "--help"],
        hint_contains: &["pointbreak input-request show"],
    },
    // The revision family: the verb-less plural and the digest both moved.
    RemovedPath {
        argv: &["review", "revisions", "--help"],
        hint_contains: &["pointbreak revision list"],
    },
    RemovedPath {
        argv: &["review", "show", "--help"],
        hint_contains: &["pointbreak revision show"],
    },
    // `identity enroll` renamed to `delegate` (the verb collided with `keys enroll`).
    RemovedPath {
        argv: &["identity", "enroll", "--help"],
        hint_contains: &["pointbreak identity delegate <AGENT> --principal <P>"],
    },
    // The `keys` family noun singularized; subverbs are unchanged.
    RemovedPath {
        argv: &["keys", "init", "--help"],
        hint_contains: &["The `keys` family is now `key`"],
    },
    // The legacy working-tree surfaces retired end-to-end (ADR-0030 second amendment).
    RemovedPath {
        argv: &["dump", "--help"],
        hint_contains: &[
            "pointbreak diff",
            "pointbreak inspect",
            "pointbreak revision show",
        ],
    },
    RemovedPath {
        argv: &["show", "--help"],
        hint_contains: &[
            "pointbreak diff",
            "pointbreak inspect",
            "pointbreak revision show",
        ],
    },
    RemovedPath {
        argv: &["notes", "apply", "--help"],
        hint_contains: &["pointbreak observation", "pointbreak revision show"],
    },
];

#[test]
fn review_namespace_is_retired_with_a_hint() {
    for argv in [vec!["review"], vec!["review", "--help"]] {
        let out = pointbreak(argv.clone());
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
fn assessment_replace_is_unregistered_with_a_recovery_hint() {
    let output = pointbreak(["assessment", "replace"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "replace must remain unregistered");
    assert!(
        stderr.contains("unrecognized subcommand 'replace'"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("pointbreak assessment add --replaces <assessment-id>"),
        "replacement recovery missing:\n{stderr}"
    );

    for argv in [
        vec!["assessment", "retire"],
        vec!["observation", "assessment", "replace"],
    ] {
        let output = pointbreak(argv.clone());
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "{argv:?} should be rejected");
        assert!(
            stderr.contains("unrecognized subcommand"),
            "{argv:?} stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("pointbreak assessment add --replaces"),
            "{argv:?} must not receive assessment replacement recovery:\n{stderr}"
        );
    }
}

#[test]
fn removed_review_paths_are_unregistered_and_hint_at_successors() {
    for case in REMOVED_PATHS {
        let output = pointbreak(case.argv.iter().copied());
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
