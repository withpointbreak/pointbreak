mod support;

use std::process::Output;

use serde_json::Value;
use support::git_repo::GitRepo;
use support::pointbreak_env;

const AUTHOR_ACTOR: &str = "actor:agent:first-review-author";
const AUTHOR_TRACK: &str = "agent:first-review-author";
const REVIEWER_ACTOR: &str = "actor:agent:first-review-reviewer";
const REVIEWER_TRACK: &str = "agent:first-review-reviewer";
const CAPTURE_SUMMARY: &str = "Explain evidence in first-use guidance";

fn parse_json(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}

/// A disposable repository with a committed baseline and a real modification to
/// an already tracked file — the first-use work source. Deliberately not the
/// canonical example pack.
fn onboarding_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("onboarding.txt", "First useful Review\n");
    repo.commit_all("add onboarding baseline");
    repo.write(
        "onboarding.txt",
        "First useful Review\nChecks are evidence, not a verdict.\n",
    );
    repo
}

/// Run the binary against an isolated key home, optionally as an explicit actor.
/// The short path runs with no actor: only authored facts set one.
fn journey(args: &[&str], home: &str, actor: Option<&str>) -> Output {
    let mut env = vec![("POINTBREAK_HOME", home)];
    if let Some(actor) = actor {
        env.push(("POINTBREAK_ACTOR_ID", actor));
    }
    pointbreak_env(args.iter().copied(), &env)
}

fn journey_json(args: &[&str], home: &str, actor: Option<&str>) -> Value {
    let output = journey(args, home, actor);
    assert!(
        output.status.success(),
        "pointbreak {args:?} failed\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_json(&output.stdout)
}

#[test]
fn first_use_journey_executes_the_flattened_paired_loop() {
    let home = tempfile::tempdir().expect("create isolated key home");
    let home = home.path().to_str().expect("home path is utf-8");
    let repo = onboarding_repo();
    let repo_arg = repo.path().to_str().expect("repo path is utf-8");

    // Short path: capture with a useful immutable summary, no actor/track setup.
    let captured = journey_json(
        &[
            "capture",
            "--repo",
            repo_arg,
            "--summary",
            CAPTURE_SUMMARY,
            "--format",
            "json",
        ],
        home,
        None,
    );
    assert_eq!(captured["schema"], "pointbreak.change-capture-receipt.v1");
    assert_eq!(captured["revision"]["summary"], CAPTURE_SUMMARY);
    let revision_id = captured["revision"]["id"]
        .as_str()
        .expect("captured revision id")
        .to_owned();
    let change_id = captured["changeId"]
        .as_str()
        .expect("captured Change id")
        .to_owned();
    let review_cursor = captured["reviewCursor"]["token"]
        .as_str()
        .expect("capture writer cursor")
        .to_owned();
    assert!(revision_id.starts_with("rev:sha256:"));
    assert!(change_id.starts_with("change:sha256:"));

    // The summary is a discovery label on list and show, and identity stays the
    // captured id.
    let listed = journey_json(&["revision", "list", "--repo", repo_arg], home, None);
    assert_eq!(listed["revisionCount"], 1);
    assert_eq!(listed["entries"][0]["revisionId"], revision_id.as_str());
    assert_eq!(listed["entries"][0]["summary"], CAPTURE_SUMMARY);
    let shown = journey_json(
        &["revision", "show", &revision_id, "--repo", repo_arg],
        home,
        None,
    );
    assert_eq!(shown["revision"]["id"], revision_id.as_str());
    assert_eq!(shown["revision"]["summary"], CAPTURE_SUMMARY);

    // Author handoff: the first authored fact introduces the author actor and
    // track, and surfaces the automatic-signing trust cue as an advisory
    // diagnostic — never a gate.
    let author_observation_output = journey(
        &[
            "observation",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            AUTHOR_TRACK,
            "--title",
            "First-use guidance distinguishes evidence",
            "--body",
            "The tracked change explains that checks are evidence rather than a verdict.",
            "--format",
            "json",
        ],
        home,
        Some(AUTHOR_ACTOR),
    );
    assert!(
        author_observation_output.status.success(),
        "author observation failed\nstderr:\n{}",
        String::from_utf8_lossy(&author_observation_output.stderr)
    );
    let author_signing_cue =
        String::from_utf8_lossy(&author_observation_output.stderr).into_owned();
    assert!(
        author_signing_cue.contains("generated signing key"),
        "the first authored fact surfaces the automatic-signing diagnostic: {author_signing_cue}"
    );
    assert!(
        author_signing_cue.contains("pointbreak key enroll"),
        "the signing diagnostic points at optional enrollment: {author_signing_cue}"
    );
    let author_observation = parse_json(&author_observation_output.stdout);
    assert_eq!(author_observation["trackId"], AUTHOR_TRACK);
    let author_observation_id = author_observation["observationId"]
        .as_str()
        .expect("author observation id")
        .to_owned();

    // The recorded validation corresponds to a command actually executed against
    // the captured content: run it, then record its real exit code as evidence.
    repo.git(["diff", "--check"]);
    let author_validation = journey_json(
        &[
            "validation",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            AUTHOR_TRACK,
            "--check-name",
            "git diff --check",
            "--status",
            "passed",
            "--command",
            "git diff --check",
            "--exit-code",
            "0",
            "--summary",
            "The captured tracked change has no whitespace errors.",
            "--format",
            "json",
        ],
        home,
        Some(AUTHOR_ACTOR),
    );
    assert_eq!(author_validation["trackId"], AUTHOR_TRACK);
    assert_eq!(author_validation["status"], "passed");

    // Reviewer pass: independent read, own track, own actor. The reviewer
    // re-runs the check rather than trusting the author's record.
    let reviewer_observation = journey_json(
        &[
            "observation",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            REVIEWER_TRACK,
            "--title",
            "Release proof remains separate",
            "--body",
            "The walkthrough is reviewable locally, but the released artifact has not repeated it.",
            "--format",
            "json",
        ],
        home,
        Some(REVIEWER_ACTOR),
    );
    let reviewer_observation_id = reviewer_observation["observationId"]
        .as_str()
        .expect("reviewer observation id")
        .to_owned();
    repo.git(["diff", "--check"]);
    let reviewer_validation = journey_json(
        &[
            "validation",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            REVIEWER_TRACK,
            "--check-name",
            "reviewer git diff --check",
            "--status",
            "passed",
            "--command",
            "git diff --check",
            "--exit-code",
            "0",
            "--summary",
            "The reviewer independently reran the whitespace check.",
            "--format",
            "json",
        ],
        home,
        Some(REVIEWER_ACTOR),
    );
    assert_eq!(reviewer_validation["trackId"], REVIEWER_TRACK);

    let question = journey_json(
        &[
            "input-request",
            "open",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            REVIEWER_TRACK,
            "--title",
            "Confirm the recovery boundary",
            "--reason",
            "manual-decision-required",
            "--mode",
            "advisory",
            "--body",
            "Should PATH recovery stay separate from the first useful Review clock?",
            "--format",
            "json",
        ],
        home,
        Some(REVIEWER_ACTOR),
    );
    assert_eq!(question["reasonCode"], "manual_decision_required");
    assert_eq!(question["mode"], "advisory");
    let question_id = question["inputRequestId"]
        .as_str()
        .expect("question id")
        .to_owned();

    let provisional = journey_json(
        &[
            "assessment",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            REVIEWER_TRACK,
            "--assessment",
            "needs-clarification",
            "--summary",
            "The Review is useful, but the clock boundary needs an explicit answer.",
            "--related-observation",
            &reviewer_observation_id,
            "--related-input-request",
            &question_id,
            "--format",
            "json",
        ],
        home,
        Some(REVIEWER_ACTOR),
    );
    let provisional_id = provisional["assessmentId"]
        .as_str()
        .expect("provisional assessment id")
        .to_owned();

    // Author response: the durable answer lives on the input request; the
    // follow-up observation adds context on the author track. Neither touches
    // the captured content, so nothing is recaptured.
    let response = journey_json(
        &[
            "input-request",
            "respond",
            &question_id,
            "--repo",
            repo_arg,
            "--outcome",
            "approved",
            "--reason",
            "PATH recovery is setup assistance outside the first useful Review clock.",
            "--format",
            "json",
        ],
        home,
        Some(AUTHOR_ACTOR),
    );
    assert_eq!(response["outcome"], "approved");
    let author_follow_up = journey_json(
        &[
            "observation",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            AUTHOR_TRACK,
            "--title",
            "Clock boundary is explicit",
            "--body",
            "The walkthrough separates acquisition and PATH recovery from the review clocks.",
            "--format",
            "json",
        ],
        home,
        Some(AUTHOR_ACTOR),
    );
    assert_eq!(author_follow_up["trackId"], AUTHOR_TRACK);

    // Reviewer replacement: an explicit follow-up request plus a replacement
    // assessment. `--replaces` is the only thing that retires the provisional
    // call.
    let follow_up_request = journey_json(
        &[
            "input-request",
            "open",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            REVIEWER_TRACK,
            "--title",
            "Verify the release-candidate rerun",
            "--reason",
            "insufficient-evidence",
            "--mode",
            "operative",
            "--body",
            "The released artifact must repeat this path before a release claim.",
            "--format",
            "json",
        ],
        home,
        Some(REVIEWER_ACTOR),
    );
    let follow_up_request_id = follow_up_request["inputRequestId"]
        .as_str()
        .expect("follow-up request id")
        .to_owned();
    let replacement = journey_json(
        &[
            "assessment",
            "add",
            "--repo",
            repo_arg,
            "--review-cursor",
            &review_cursor,
            "--track",
            REVIEWER_TRACK,
            "--assessment",
            "accepted-with-follow-up",
            "--summary",
            "The walkthrough is accepted; released-artifact proof remains open.",
            "--replaces",
            &provisional_id,
            "--related-input-request",
            &follow_up_request_id,
            "--format",
            "json",
        ],
        home,
        Some(REVIEWER_ACTOR),
    );
    let replacement_id = replacement["assessmentId"]
        .as_str()
        .expect("replacement assessment id")
        .to_owned();

    // The request/response, provisional, replacement, and linked open follow-up
    // all project correctly.
    let requests = journey_json(
        &[
            "input-request",
            "list",
            "--repo",
            repo_arg,
            "--status",
            "all",
        ],
        home,
        None,
    );
    let request_entries = requests["inputRequests"]
        .as_array()
        .expect("input request entries");
    assert_eq!(request_entries.len(), 2);
    let by_id = |id: &str| {
        request_entries
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap_or_else(|| panic!("input request {id} is listed"))
    };
    assert_eq!(by_id(&question_id)["status"], "responded");
    assert_eq!(by_id(&follow_up_request_id)["status"], "open");

    let current = journey_json(&["assessment", "show", "--repo", repo_arg], home, None);
    assert_eq!(current["current"]["assessment"], "accepted_with_follow_up");
    assert_eq!(current["current"]["assessmentId"], replacement_id.as_str());
    let all_assessments = journey_json(
        &["assessment", "show", "--repo", repo_arg, "--all"],
        home,
        None,
    );
    let assessment_entries = all_assessments["assessments"]
        .as_array()
        .expect("assessment entries");
    let replaced = assessment_entries
        .iter()
        .find(|entry| entry["id"] == provisional_id.as_str())
        .expect("the provisional assessment is retained");
    assert_eq!(replaced["status"], "replaced");
    assert_eq!(replaced["assessment"], "needs_clarification");
    let current_entry = assessment_entries
        .iter()
        .find(|entry| entry["id"] == replacement_id.as_str())
        .expect("the replacement assessment is listed");
    assert!(
        current_entry["replaces"]
            .as_array()
            .expect("replacement links")
            .iter()
            .any(|link| link == provisional_id.as_str()),
        "the replacement names the provisional assessment it retires"
    );

    // Attention and the current-assessment projection agree: the answered
    // question is gone, the open follow-up and the follow-up obligation remain.
    let change_attention = journey_json(&["change", "attention", "--repo", repo_arg], home, None);
    assert!(
        change_attention["changes"]
            .as_array()
            .expect("Changes requiring attention")
            .iter()
            .any(|change| change["changeId"] == change_id),
        "the stable Change stays visible while its follow-up remains open"
    );
    let attention = journey_json(&["attention", "list", "--repo", repo_arg], home, None);
    let items = attention["items"].as_array().expect("attention items");
    let kinds: Vec<&str> = items
        .iter()
        .map(|item| item["kind"].as_str().expect("attention item kind"))
        .collect();
    assert!(
        kinds.contains(&"follow_up_outstanding"),
        "the accepted-with-follow-up call keeps its follow-up visible: {kinds:?}"
    );
    assert!(
        kinds.contains(&"open_input_request"),
        "the open advisory follow-up request stays visible: {kinds:?}"
    );
    assert!(
        !kinds.contains(&"ambiguous_assessment"),
        "replacement leaves one current call, not an ambiguity: {kinds:?}"
    );
    assert!(
        items
            .iter()
            .any(|item| item["kind"] == "follow_up_outstanding"
                && item["id"]
                    .as_str()
                    .expect("attention item id")
                    .contains(&replacement_id)),
        "the follow-up obligation anchors to the current assessment"
    );
    assert!(
        items.iter().any(|item| item["kind"] == "open_input_request"
            && item["id"]
                .as_str()
                .expect("attention item id")
                .contains(&follow_up_request_id)),
        "the open item is the follow-up request"
    );
    assert!(
        !items.iter().any(|item| item["id"]
            .as_str()
            .expect("attention item id")
            .contains(&question_id)),
        "the responded question no longer claims attention"
    );

    // Every fact carries its own track and actor.
    let history = journey_json(&["history", "--repo", repo_arg], home, None);
    let attribution: Vec<(String, String, String)> = history["entries"]
        .as_array()
        .expect("history entries")
        .iter()
        .map(|entry| {
            (
                entry["eventType"].as_str().expect("event type").to_owned(),
                entry["trackId"].as_str().unwrap_or_default().to_owned(),
                entry["writer"]["actorId"]
                    .as_str()
                    .expect("writer actor")
                    .to_owned(),
            )
        })
        .collect();
    for expected in [
        ("review_observation_recorded", AUTHOR_TRACK, AUTHOR_ACTOR),
        ("validation_check_recorded", AUTHOR_TRACK, AUTHOR_ACTOR),
        (
            "review_observation_recorded",
            REVIEWER_TRACK,
            REVIEWER_ACTOR,
        ),
        ("validation_check_recorded", REVIEWER_TRACK, REVIEWER_ACTOR),
        ("input_request_opened", REVIEWER_TRACK, REVIEWER_ACTOR),
        ("review_assessment_recorded", REVIEWER_TRACK, REVIEWER_ACTOR),
        // The author answers the reviewer's request: the response rides the
        // request's track while carrying the author's actor.
        ("input_request_responded", REVIEWER_TRACK, AUTHOR_ACTOR),
    ] {
        let (event_type, track, actor) = expected;
        assert!(
            attribution
                .iter()
                .any(|(entry_type, entry_track, entry_actor)| {
                    entry_type == event_type && entry_track == track && entry_actor == actor
                }),
            "history carries {event_type} on {track} by {actor}: {attribution:?}"
        );
    }

    // Landing: commit the already-reviewed content, bind a cursor to that exact
    // commit, then prove the association with the same Revision. No recapture,
    // no replacement.
    repo.git(["add", "onboarding.txt"]);
    repo.git(["commit", "-m", "docs: clarify first Review evidence"]);
    let head_oid = repo.git(["rev-parse", "HEAD"]).stdout.trim().to_owned();
    let commit_source = format!("commit:{head_oid}");
    let landing_selection = journey_json(
        &[
            "change",
            "select",
            &change_id,
            "--repo",
            repo_arg,
            "--revision",
            &revision_id,
            "--source",
            &commit_source,
            "--format",
            "json",
        ],
        home,
        Some(AUTHOR_ACTOR),
    );
    let landing_cursor = landing_selection["token"]
        .as_str()
        .expect("commit-bound landing cursor")
        .to_owned();
    let association = journey_json(
        &[
            "association",
            "land",
            "--repo",
            repo_arg,
            "--review-cursor",
            &landing_cursor,
            "--track",
            AUTHOR_TRACK,
            "--commit",
            &head_oid,
            "--format",
            "json",
        ],
        home,
        Some(AUTHOR_ACTOR),
    );
    assert_eq!(association["schema"], "pointbreak.association-land.v1");
    assert_eq!(association["commitOid"], head_oid.as_str());

    let landed = journey_json(
        &[
            "association",
            "list",
            "--repo",
            repo_arg,
            "--axis",
            "commit",
            "--current",
        ],
        home,
        None,
    );
    let current_commits = landed["currentCommits"]
        .as_array()
        .expect("current commit associations");
    assert_eq!(current_commits.len(), 1);
    assert_eq!(current_commits[0]["commitOid"], head_oid.as_str());

    // Revision identity is unchanged and the revision count did not grow.
    let final_list = journey_json(&["revision", "list", "--repo", repo_arg], home, None);
    assert_eq!(final_list["revisionCount"], 1);
    assert_eq!(final_list["entries"][0]["revisionId"], revision_id.as_str());

    // The author claim is still attributable after landing.
    let final_show = journey_json(
        &["revision", "show", &revision_id, "--repo", repo_arg],
        home,
        None,
    );
    assert_eq!(
        final_show["currentAssessment"]["assessmentId"],
        replacement_id
    );
    assert!(
        final_show["observations"]
            .as_array()
            .expect("shown observations")
            .iter()
            .any(
                |observation| observation["id"] == author_observation_id.as_str()
                    && observation["trackId"] == AUTHOR_TRACK
            ),
        "the author claim stays on the author track after landing"
    );
}

#[test]
fn getting_started_teaches_the_canonical_first_use_sequence() {
    let guide = std::fs::read_to_string("docs/getting-started.md").expect("read getting started");

    // The five public stages are named as one ordered vocabulary.
    assert!(
        guide.contains("Work -> Claims -> Evidence -> Questions -> Call"),
        "the guide names the five review stages in order"
    );

    // One canonical sequence: supported activation, capture, the paired loop,
    // and the same-Revision proof-first landing, in order.
    assert_ordered_anchors(
        &guide,
        &[
            "installation.md",
            "pointbreak change migrate-dry-run",
            "pointbreak change migrate",
            "pointbreak capture --summary",
            "pointbreak inspect --open",
            "POINTBREAK_ACTOR_ID",
            "pointbreak observation add",
            "pointbreak validation add",
            "pointbreak input-request open",
            "pointbreak assessment add",
            "pointbreak input-request respond",
            "--replaces",
            "pointbreak change select",
            "pointbreak association land",
        ],
    );
    assert!(
        guide.contains("--review-cursor \"$REVIEW_CURSOR\""),
        "review writes use the exact capture cursor"
    );
    assert!(
        guide.contains("review_change_revision_v1") && guide.contains("--ack-v0-9-unsupported"),
        "activation states the minimum-reader break explicitly"
    );

    // Trust arrives after value: enrollment is taught after Review opens, as an
    // optional staging step.
    let inspect_at = guide
        .find("pointbreak inspect --open")
        .expect("the guide opens Review");
    let enroll_at = guide
        .find("pointbreak key enroll")
        .expect("the guide explains optional enrollment");
    assert!(
        enroll_at > inspect_at,
        "enrollment guidance follows the first useful Review"
    );
    assert!(
        guide.contains(".pointbreak/allowed-signers.json"),
        "enrollment names the staged trust file"
    );

    // Truthful nouns and the same-revision landing contract.
    assert!(
        guide.contains("evidence, not a verdict"),
        "validation is taught as evidence, never a verdict"
    );
    assert!(
        guide.contains("same revision"),
        "landing is taught as an association on the same revision"
    );
    assert!(
        !guide.contains("--supersedes"),
        "the first-use journey never supersedes reviewed content"
    );
}

#[test]
fn manual_testing_fixes_the_first_use_walkthrough_protocol() {
    let playbook =
        std::fs::read_to_string("docs/manual-testing.md").expect("read manual testing playbook");
    let heading = "## First useful Review walkthrough — fixed protocol";

    // Clock boundaries: supported acquisition context stays separate from the
    // source-built walkthrough clocks, and the two walkthrough clocks are fixed.
    assert_markdown_section_contains(
        &playbook,
        heading,
        &[
            "Clock A",
            "Clock B-short",
            "Clock B-paired",
            "cargo +stable build --locked --bin pointbreak",
            "$POINTBREAK_BINARY",
            "supported installer",
        ],
    );

    // The exact command sequence and its numbered evidence artifacts.
    assert_markdown_section_contains(
        &playbook,
        heading,
        &[
            "00-version.json",
            "00-binary-sha256.txt",
            "00-profile-l0.json",
            "00-migration-dry-run.json",
            "00-migration.json",
            "00-profile-l2.json",
            "01-empty-first-open.png",
            "02-capture.json",
            "03-first-useful-review.png",
            "04-author-observation.json",
            "05-author-validation.json",
            "06-reviewer-observation.json",
            "07-reviewer-validation.json",
            "08-reviewer-question.json",
            "09-provisional-assessment.json",
            "10-author-response.json",
            "11-author-follow-up.json",
            "12-release-follow-up.json",
            "13-current-assessment.json",
            "14-landing.json",
            "15-change-list.json",
            "16-change-attention.json",
            "17-change-revision.json",
        ],
    );
    assert_markdown_section_contains(
        &playbook,
        heading,
        &[
            "--summary \"Explain evidence in first-use guidance\"",
            "manual-decision-required",
            "insufficient-evidence",
            "--replaces",
            "change select",
            "association land",
            "--review-cursor",
        ],
    );

    // Recovery points, the intervention ledger, and the explicit nonclaims.
    assert_markdown_section_contains(
        &playbook,
        heading,
        &[
            "--include-untracked",
            "store paths",
            "key enroll",
            "intervention",
            "do not recapture",
            "five-minute",
            "novice",
            "population",
        ],
    );
}

fn assert_ordered_anchors(text: &str, anchors: &[&str]) {
    let mut last_index = 0;
    let mut last_anchor = "start of document";
    for anchor in anchors {
        let found = text[last_index..]
            .find(anchor)
            .unwrap_or_else(|| panic!("missing anchor {anchor:?} after {last_anchor:?}"));
        last_index += found + anchor.len();
        last_anchor = anchor;
    }
}

fn assert_markdown_section_contains(markdown: &str, heading: &str, required: &[&str]) {
    let start = markdown
        .find(heading)
        .unwrap_or_else(|| panic!("missing section heading: {heading}"));
    let tail = &markdown[start..];
    let end = tail[heading.len()..]
        .find("\n## ")
        .map(|idx| heading.len() + idx)
        .unwrap_or(tail.len());
    let section = &tail[..end];

    for token in required {
        assert!(
            section.contains(token),
            "section {heading} missing token: {token}"
        );
    }
}
