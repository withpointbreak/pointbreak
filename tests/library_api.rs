//! End-to-end exercise of the supported in-process library API (see
//! `docs/library-api.md`): capture, an attributed write, typed reads, documented
//! JSON, and forwarding events into a second store — all without the CLI.

mod support;

use serde_json::Value;
use shoreline::model::ActorId;
use shoreline::session::event::{InputRequestReasonCode, InputRequestResponseOutcome};
use shoreline::session::{
    CaptureOptions, IngestEventsOptions, InputRequestListOptions, InputRequestOpenOptions,
    InputRequestRespondOptions, InputRequestStatus, InputRequestStatusFilter, ReloadOutcome,
    capture_worktree_review, ingest_events, list_input_requests, open_input_request, read_events,
    respond_input_request,
};
use support::git_repo::GitRepo;

fn modified_repo() -> GitRepo {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 {\n    1\n}\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 {\n    2\n}\n");
    repo
}

/// A federation-bridge-shaped flow: read facts as typed structs, respond on
/// behalf of a remote actor, reproduce the documented JSON, and forward the
/// resulting events into a second clone-local store.
#[test]
fn in_process_consumer_reads_attributes_documents_and_forwards() {
    let origin = modified_repo();

    // Capture and open an operative input request in process.
    capture_worktree_review(CaptureOptions::new(origin.path())).unwrap();
    let opened = open_input_request(
        InputRequestOpenOptions::new(origin.path())
            .with_track("human:kevin")
            .with_title("Need approval")
            .with_reason_code(InputRequestReasonCode::ManualDecisionRequired),
    )
    .unwrap();

    // Respond on behalf of a specific remote reviewer (no env mutation needed).
    respond_input_request(
        InputRequestRespondOptions::new(origin.path(), opened.input_request_id.clone())
            .with_outcome(InputRequestResponseOutcome::Approved)
            .with_actor_id(ActorId::new("actor:agent:remote-reviewer")),
    )
    .unwrap();

    // Read back as typed structs and branch on the typed status (#117).
    let listed = list_input_requests(
        InputRequestListOptions::new(origin.path()).with_status(InputRequestStatusFilter::All),
    )
    .unwrap();
    assert_eq!(listed.input_requests.len(), 1);
    let view = &listed.input_requests[0];
    match view.status {
        InputRequestStatus::Responded => {}
        other => panic!("expected Responded, got {other:?}"),
    }
    assert_eq!(
        view.responses[0].writer.actor_id.as_str(),
        "actor:agent:remote-reviewer",
        "the per-call actor override must be the durable writer"
    );

    // Reproduce the documented `shore.review-input-request-list` JSON in process (#118).
    let document = shoreline::documents::input_request_list_document(listed);
    let json: Value = serde_json::to_value(&document).unwrap();
    assert_eq!(json["schema"], "shore.review-input-request-list");
    assert_eq!(json["version"], 1);
    assert_eq!(json["inputRequests"][0]["status"], "responded");
    assert_eq!(
        json["inputRequests"][0]["responses"][0]["writer"]["actorId"],
        "actor:agent:remote-reviewer"
    );

    // Forward the origin's events into a second clone-local store (#119).
    let events = read_events(origin.path()).unwrap();
    assert!(events.len() >= 3, "captured + opened + responded");
    let dest = modified_repo();
    let result = ingest_events(IngestEventsOptions::new(dest.path(), events.clone())).unwrap();
    assert_eq!(result.events_created, events.len());

    // The forwarded, remotely attributed decision is visible in the destination.
    let mirrored = list_input_requests(
        InputRequestListOptions::new(dest.path()).with_status(InputRequestStatusFilter::All),
    )
    .unwrap();
    assert_eq!(mirrored.input_requests.len(), 1);
    assert_eq!(
        mirrored.input_requests[0].status,
        InputRequestStatus::Responded
    );
    assert_eq!(
        mirrored.input_requests[0].responses[0]
            .writer
            .actor_id
            .as_str(),
        "actor:agent:remote-reviewer"
    );

    // Re-ingest is idempotent.
    let again = ingest_events(IngestEventsOptions::new(dest.path(), events)).unwrap();
    assert_eq!(again.events_created, 0);
}

/// `ReloadOutcome` is part of the supported surface and must be nameable from a
/// non-test external build (#117).
#[test]
fn reload_outcome_is_publicly_nameable() {
    fn _accepts(_: ReloadOutcome) {}
    let _: Option<ReloadOutcome> = None;
}
