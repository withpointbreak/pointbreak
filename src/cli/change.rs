use std::io::Write;
use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use pointbreak::documents::{
    AssociationComparisonDocumentV1, AssociationComparisonRefV1, AssociationComparisonStateV1,
    AssociationProofAvailabilityV1, ChangeDocumentFacadeV1, ChangeQueryUnavailableDocumentV1,
    ContentAvailabilityV1, FactFamilyStateV1, FactPresentationV1, ReaderProfileDocumentV1,
    RevisionInterdiffAvailabilityV1, RevisionInterdiffDocumentV1, RevisionInterdiffRefV1,
    RevisionResourceDocumentV1, RevisionResourceProjectionV1, RevisionResourceRefV1,
    revision_show_document_v3,
};
use pointbreak::model::{
    ActorId, ChangeId, ChangeIdentityDescriptorV1, ChangeMembershipClaimId,
    ChangeRevisionRelationClaimId, RevisionId, RevisionRefV1,
};
use pointbreak::session::event::ChangeLinkRelationV1;
use pointbreak::session::{
    AssessmentRecordStatus, BulkAdoptionDryRunDocumentV1, BulkAdoptionDryRunOptions,
    BulkAdoptionMigrationOptions, BulkAdoptionOwnerDecisionManifestV1, CaptureOptions,
    ChangeAdvanceV1, ChangeCaptureOptions, ChangeCreateOptions, ChangeLinkOptions,
    ChangeMembershipOptions, ChangeMembershipWithdrawalOptions, ChangeReaderReadyV1,
    ChangeReaderStateV1, ChangeRelationOptions, ChangeRelationWithdrawalOptions, ObservationStatus,
    ReviewCursorV1, ReviewSourceBindingV1, ReviewSourceRequestV1, RevisionShowOptions,
    SnapshotContentState, WorktreeSpec, assert_change_revision_relation, capture_change_revision,
    change_reader_state_for_repo, create_change, dry_run_bulk_adoption, join_revision_to_change,
    link_changes, migrate_bulk_adoption, restore_bulk_adoption_backup, review_source_binding,
    select_review_cursor, show_revision_for_change_reader_ready, validate_review_cursor_for_write,
    withdraw_change_revision_relation, withdraw_revision_from_change,
};

use crate::cli::{common, output};

#[derive(Debug, Args)]
pub(super) struct ChangeArgs {
    #[command(subcommand)]
    command: ChangeCommand,
}

#[derive(Debug, Subcommand)]
enum ChangeCommand {
    /// Report the store capability before any Change payload is read
    Profile(ReadArgs),
    /// List stable Changes
    List(ReadArgs),
    /// List Changes that still require judgment
    Attention(ReadArgs),
    /// Show one stable Change and every exact current candidate
    Show(ChangeReadArgs),
    /// Select one exact current Revision and emit a self-hashed review cursor
    Select(SelectArgs),
    /// Show one exact Revision in a named Change context
    Revision(ExactReadArgs),
    /// Read the immutable captured resource for one exact Revision
    Resource(ExactReadArgs),
    /// Describe a separately identified comparison between two exact Revisions
    Interdiff(InterdiffArgs),
    /// Create a stable Change identity in an activated store
    Create(CreateArgs),
    /// Adopt one existing exact Revision into a Change
    Join(JoinArgs),
    /// Withdraw one exact membership claim
    WithdrawMembership(WithdrawMembershipArgs),
    /// Assert one exact replacement edge inside a Change
    AssertRelation(RelationArgs),
    /// Withdraw one exact replacement claim
    WithdrawRelation(WithdrawRelationArgs),
    /// Record an explicit relationship between two Changes
    Link(LinkArgs),
    /// Capture an initial, replacement, parallel, or consolidation Revision
    Capture(ChangeCaptureArgs),
    /// Plan deterministic adoption for untouched stores without writing
    MigrateDryRun(MigrationDryRunArgs),
    /// Activate one exact owner-approved migration plan after a verified backup
    Migrate(MigrationArgs),
    /// Restore a verified L0 backup as a separate recovery fork
    MigrateRestore(MigrationRestoreArgs),
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("identity").required(true).multiple(false)))]
struct CreateArgs {
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Stable retry identifier. Reusing it with different inputs is refused.
    #[arg(long)]
    operation_id: String,
    /// Explicit 32-byte lowercase hexadecimal Change nonce.
    #[arg(long, group = "identity")]
    nonce: Option<String>,
    /// Explicit rendezvous Revision for a root-derived Change identity.
    #[arg(long, group = "identity")]
    root_revision: Option<String>,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct JoinArgs {
    change: String,
    revision: String,
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    operation_id: String,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct WithdrawMembershipArgs {
    claim: String,
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    operation_id: String,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct RelationArgs {
    change: String,
    successor: String,
    predecessor: String,
    #[arg(long)]
    successor_artifact_hash: String,
    #[arg(long)]
    predecessor_artifact_hash: String,
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    operation_id: String,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct WithdrawRelationArgs {
    claim: String,
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    operation_id: String,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum LinkRelationArg {
    SameWork,
    RelatedWork,
}

#[derive(Debug, Args)]
struct LinkArgs {
    first_change: String,
    second_change: String,
    #[arg(long, value_enum)]
    relation: LinkRelationArg,
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    operation_id: String,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum CaptureAdvanceArg {
    Replace,
    Parallel,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("transition").required(true).multiple(false)))]
struct ChangeCaptureArgs {
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    operation_id: String,
    /// Start a new Change with this explicit 32-byte lowercase hexadecimal nonce.
    #[arg(long, group = "transition")]
    initial_nonce: Option<String>,
    /// Advance this exact cursor by replacement or parallel capture.
    #[arg(long, group = "transition", requires = "advance")]
    cursor: Option<String>,
    #[arg(long, value_enum, requires = "cursor")]
    advance: Option<CaptureAdvanceArg>,
    /// Additional exact current predecessor for consolidation. May be repeated.
    #[arg(long, requires = "cursor")]
    predecessor: Vec<String>,
    /// Artifact hash paired by position with each --predecessor.
    #[arg(long, requires = "predecessor")]
    predecessor_artifact_hash: Vec<String>,
    #[arg(long)]
    include_untracked: bool,
    #[arg(long)]
    allow_empty: bool,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long = "path")]
    paths: Vec<String>,
    /// Select a signing key for this write.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct MigrationDryRunArgs {
    /// Repository root used when no --root values are supplied.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Repository roots to inventory together. May be repeated.
    #[arg(long = "root")]
    roots: Vec<PathBuf>,
    /// Actor identity frozen into the proposed Change claim envelopes.
    #[arg(long, default_value = "actor:bulk-adoption-dry-run")]
    actor: String,
    /// Owner-authored anomaly and overlap decisions to apply to the exact re-plan.
    #[arg(long)]
    owner_decisions: Option<PathBuf>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct MigrationArgs {
    /// Repository whose exact resolved store will be activated.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Exact JSON document emitted by `change migrate-dry-run`.
    #[arg(long)]
    dry_run: PathBuf,
    /// Exact top-level self-hash printed in the approved dry-run document.
    #[arg(long)]
    ack_manifest: String,
    /// Exact cohort manifest hash for this root in the approved dry run.
    #[arg(long)]
    ack_cohort_manifest: String,
    /// Minimum reader profile accepted after the irreversible activation append.
    #[arg(long)]
    ack_minimum_reader: String,
    /// Acknowledge that v0.9 readers are unsupported after activation.
    #[arg(long)]
    ack_v0_9_unsupported: bool,
    /// Fresh external directory that retains the verified L0 backup and resume plan.
    #[arg(long)]
    backup: PathBuf,
    /// Stable retry identifier. Reuse it to resume the same retained plan.
    #[arg(long)]
    operation_id: String,
    /// Exact owner decision manifest used to produce the approved dry run.
    #[arg(long)]
    owner_decisions: Option<PathBuf>,
    /// Select the strict signing key for a new L0 activation.
    #[arg(long)]
    sign_key: Option<String>,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct MigrationRestoreArgs {
    /// External directory containing the verified pre-activation backup.
    #[arg(long)]
    backup: PathBuf,
    /// Repository whose empty, separately identified store receives the L0 fork.
    #[arg(long)]
    target_repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct ReadArgs {
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct ChangeReadArgs {
    change: String,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct SelectArgs {
    change: String,
    /// Select this exact Revision. Required when the Change has multiple current Revisions.
    #[arg(long)]
    revision: Option<String>,
    /// Permit a captured-resource cursor over a non-current member.
    #[arg(long)]
    allow_historical: bool,
    /// Revalidate a previously emitted cursor against the current Change graph before selecting.
    #[arg(long)]
    cursor: Option<String>,
    /// Bind the cursor to immutable captured bytes, the matching live worktree,
    /// or a commit that materializes the exact captured change.
    #[arg(long, value_name = "captured|worktree|commit:<rev>")]
    source: Option<String>,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct ExactReadArgs {
    change: String,
    revision: String,
    /// Exact object-artifact hash from the selected RevisionRefV1.
    #[arg(long)]
    artifact_hash: String,
    /// Hydrate body-like fact text inside the exact Revision document.
    #[arg(long)]
    include_body: bool,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct InterdiffArgs {
    change: String,
    from: String,
    to: String,
    #[arg(long)]
    from_artifact_hash: String,
    #[arg(long)]
    to_artifact_hash: String,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

pub(super) fn run(
    args: ChangeArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        ChangeCommand::Profile(args) => {
            let state = change_reader_state_for_repo(&args.repo)?;
            write(
                &args.format_args,
                stdout,
                &ReaderProfileDocumentV1::from(&state.capability),
            )
        }
        ChangeCommand::List(args) => {
            with_facade(&args.repo, &args.format_args, stdout, |facade, _| {
                Ok(serde_json::to_value(facade.list_document())?)
            })
        }
        ChangeCommand::Attention(args) => {
            with_facade(&args.repo, &args.format_args, stdout, |facade, _| {
                Ok(serde_json::to_value(facade.attention_document(false))?)
            })
        }
        ChangeCommand::Show(args) => {
            with_facade(&args.repo, &args.format_args, stdout, |facade, _| {
                Ok(serde_json::to_value(
                    facade.detail_document(&ChangeId::new(args.change))?,
                )?)
            })
        }
        ChangeCommand::Select(args) => run_select(args, stdout),
        ChangeCommand::Revision(args) => run_exact(args, stdout, true),
        ChangeCommand::Resource(args) => run_exact(args, stdout, false),
        ChangeCommand::Interdiff(args) => run_interdiff(args, stdout),
        ChangeCommand::Create(args) => run_create(args, stdout, stderr),
        ChangeCommand::Join(args) => run_join(args, stdout, stderr),
        ChangeCommand::WithdrawMembership(args) => run_withdraw_membership(args, stdout, stderr),
        ChangeCommand::AssertRelation(args) => run_assert_relation(args, stdout, stderr),
        ChangeCommand::WithdrawRelation(args) => run_withdraw_relation(args, stdout, stderr),
        ChangeCommand::Link(args) => run_link(args, stdout, stderr),
        ChangeCommand::Capture(args) => run_change_capture(args, stdout, stderr),
        ChangeCommand::MigrateDryRun(args) => run_migration_dry_run(args, stdout),
        ChangeCommand::Migrate(args) => run_migration(args, stdout, stderr),
        ChangeCommand::MigrateRestore(args) => run_migration_restore(args, stdout),
    }
}

fn run_create(
    args: CreateArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let descriptor = match (args.nonce.as_deref(), args.root_revision) {
        (Some(nonce), None) => ChangeIdentityDescriptorV1::opaque_nonce(parse_nonce(nonce)?),
        (None, Some(revision)) => {
            ChangeIdentityDescriptorV1::root_revision(RevisionId::new(revision))
        }
        _ => return Err("exactly one Change identity is required".into()),
    };
    let (options, skip) = signed_options(
        &args.repo,
        args.sign_key.as_deref(),
        stderr,
        ChangeCreateOptions::new(&args.repo, args.operation_id, descriptor),
    );
    let receipt = create_change(options)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn run_join(
    args: JoinArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let (options, skip) = signed_options(
        &args.repo,
        args.sign_key.as_deref(),
        stderr,
        ChangeMembershipOptions::new(
            &args.repo,
            args.operation_id,
            ChangeId::new(args.change),
            RevisionId::new(args.revision),
        ),
    );
    let receipt = join_revision_to_change(options)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn run_withdraw_membership(
    args: WithdrawMembershipArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let (options, skip) = signed_options(
        &args.repo,
        args.sign_key.as_deref(),
        stderr,
        ChangeMembershipWithdrawalOptions::new(
            &args.repo,
            args.operation_id,
            ChangeMembershipClaimId::new(args.claim),
        ),
    );
    let receipt = withdraw_revision_from_change(options)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn run_assert_relation(
    args: RelationArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let successor = RevisionRefV1::new(
        RevisionId::new(args.successor),
        args.successor_artifact_hash,
    )?;
    let predecessor = RevisionRefV1::new(
        RevisionId::new(args.predecessor),
        args.predecessor_artifact_hash,
    )?;
    let (options, skip) = signed_options(
        &args.repo,
        args.sign_key.as_deref(),
        stderr,
        ChangeRelationOptions::new(
            &args.repo,
            args.operation_id,
            ChangeId::new(args.change),
            successor,
            predecessor,
        ),
    );
    let receipt = assert_change_revision_relation(options)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn run_withdraw_relation(
    args: WithdrawRelationArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let (options, skip) = signed_options(
        &args.repo,
        args.sign_key.as_deref(),
        stderr,
        ChangeRelationWithdrawalOptions::new(
            &args.repo,
            args.operation_id,
            ChangeRevisionRelationClaimId::new(args.claim),
        ),
    );
    let receipt = withdraw_change_revision_relation(options)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn run_link(
    args: LinkArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let relation = match args.relation {
        LinkRelationArg::SameWork => ChangeLinkRelationV1::SameWork,
        LinkRelationArg::RelatedWork => ChangeLinkRelationV1::RelatedWork,
    };
    let (options, skip) = signed_options(
        &args.repo,
        args.sign_key.as_deref(),
        stderr,
        ChangeLinkOptions::new(
            &args.repo,
            args.operation_id,
            ChangeId::new(args.first_change),
            ChangeId::new(args.second_change),
            relation,
        ),
    );
    let receipt = link_changes(options)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn run_change_capture(
    args: ChangeCaptureArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.predecessor.len() != args.predecessor_artifact_hash.len() {
        return Err("each --predecessor requires one --predecessor-artifact-hash".into());
    }
    let mut capture = CaptureOptions::new(&args.repo);
    if args.include_untracked {
        capture = capture.with_worktree(WorktreeSpec::new().with_include_untracked());
    }
    if args.allow_empty {
        capture = capture.with_allow_empty();
    }
    if let Some(summary) = args.summary {
        capture = capture.with_summary(summary);
    }
    if !args.paths.is_empty() {
        capture = capture.with_pathspecs(args.paths);
    }
    let (capture, skip) = signed_options(&args.repo, args.sign_key.as_deref(), stderr, capture);
    let mut operation = if let Some(nonce) = args.initial_nonce {
        ChangeCaptureOptions::initial(
            args.operation_id,
            capture,
            ChangeIdentityDescriptorV1::opaque_nonce(parse_nonce(&nonce)?),
        )
    } else {
        let cursor = args
            .cursor
            .ok_or("--cursor is required for an advancing capture")?;
        let advance = match args.advance.ok_or("--advance is required with --cursor")? {
            CaptureAdvanceArg::Replace => ChangeAdvanceV1::Replace,
            CaptureAdvanceArg::Parallel => ChangeAdvanceV1::Parallel,
        };
        ChangeCaptureOptions::advance(args.operation_id, capture, cursor, advance)
    };
    for (revision, artifact_hash) in args
        .predecessor
        .into_iter()
        .zip(args.predecessor_artifact_hash)
    {
        operation = operation.with_additional_predecessor(RevisionRefV1::new(
            RevisionId::new(revision),
            artifact_hash,
        )?);
    }
    let receipt = capture_change_revision(operation)?;
    common::surface_best_effort_skip(&skip, stderr);
    write(&args.format_args, stdout, &receipt)
}

fn signed_options<O: common::SignableOptions>(
    repo: &std::path::Path,
    sign_key: Option<&str>,
    stderr: &mut dyn Write,
    options: O,
) -> (O, common::SigningSkip) {
    if let Some(resolved) = common::resolve_and_surface_signer(repo, sign_key, stderr) {
        common::apply_resolved_signer(options, resolved)
    } else {
        (options, None)
    }
}

fn parse_nonce(value: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Change nonce must be 32 lowercase-hex bytes".into());
    }
    let mut nonce = [0_u8; 32];
    for (index, byte) in nonce.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
    }
    Ok(nonce)
}

fn run_migration_dry_run(
    args: MigrationDryRunArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let roots = if args.roots.is_empty() {
        vec![args.repo]
    } else {
        args.roots
    };
    let mut options =
        BulkAdoptionDryRunOptions::new(&roots[0]).with_actor_id(ActorId::new(args.actor));
    for root in &roots[1..] {
        options = options.with_root(root);
    }
    if let Some(path) = args.owner_decisions {
        let decisions: BulkAdoptionOwnerDecisionManifestV1 =
            serde_json::from_slice(&std::fs::read(path)?)?;
        options = options.with_owner_decisions(decisions);
    }
    write(&args.format_args, stdout, &dry_run_bulk_adoption(options)?)
}

fn run_migration(
    args: MigrationArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let dry_run: BulkAdoptionDryRunDocumentV1 =
        serde_json::from_slice(&std::fs::read(&args.dry_run)?)?;
    let mut options = BulkAdoptionMigrationOptions::new(
        &args.repo,
        dry_run,
        args.ack_manifest,
        args.ack_cohort_manifest,
        &args.backup,
        args.operation_id,
    )
    .with_minimum_reader_ack(args.ack_minimum_reader)
    .with_derived_enabled(
        std::env::var_os("POINTBREAK_DERIVED_ACCESS").as_deref()
            != Some(std::ffi::OsStr::new("off")),
    );
    if args.ack_v0_9_unsupported {
        options = options.with_legacy_reader_unsupported_ack();
    }
    if let Some(path) = args.owner_decisions {
        let decisions: BulkAdoptionOwnerDecisionManifestV1 =
            serde_json::from_slice(&std::fs::read(path)?)?;
        options = options.with_owner_decisions(decisions);
    }
    if let Some(resolved) =
        common::resolve_and_surface_signer(&args.repo, args.sign_key.as_deref(), stderr)
    {
        options = options.sign_with(resolved.signer);
    }
    write(&args.format_args, stdout, &migrate_bulk_adoption(options)?)
}

fn run_migration_restore(
    args: MigrationRestoreArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    write(
        &args.format_args,
        stdout,
        &restore_bulk_adoption_backup(&args.backup, &args.target_repo)?,
    )
}

fn with_facade(
    repo: &std::path::Path,
    format_args: &output::FormatArgs,
    stdout: &mut dyn Write,
    build: impl FnOnce(
        &ChangeDocumentFacadeV1,
        &ChangeReaderReadyV1,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = change_reader_state_for_repo(repo)?;
    if let Some(unavailable) = ChangeQueryUnavailableDocumentV1::for_inspection(&state.capability) {
        return write(format_args, stdout, &unavailable);
    }
    let ready = ready(&state)?;
    let facade =
        ChangeDocumentFacadeV1::new(ready.projection.clone(), ready.document_projection.clone())?;
    let document = build(&facade, ready)?;
    write(format_args, stdout, &document)
}

fn ready(state: &ChangeReaderStateV1) -> Result<&ChangeReaderReadyV1, Box<dyn std::error::Error>> {
    state
        .ready()
        .ok_or_else(|| "Change reader state has no complete semantic projection".into())
}

fn run_select(args: SelectArgs, stdout: &mut dyn Write) -> Result<(), Box<dyn std::error::Error>> {
    with_facade(&args.repo, &args.format_args, stdout, |_facade, ready| {
        let change_id = ChangeId::new(args.change);
        let change = ready
            .projection
            .changes
            .get(&change_id)
            .ok_or_else(|| format!("Change {} is unavailable", change_id.as_str()))?;
        let previous = args
            .cursor
            .as_deref()
            .map(ReviewCursorV1::decode_token)
            .transpose()?;
        if let (Some(token), Some(previous)) = (args.cursor.as_deref(), previous.as_ref()) {
            let current_binding = review_source_binding(
                &args.repo,
                &previous.revision,
                source_request_from_binding(&previous.source_binding),
            )?;
            validate_review_cursor_for_write(
                token,
                change,
                &ready.document_projection,
                &current_binding,
            )
            .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        }
        let revision_id = args.revision.map(RevisionId::new).or_else(|| {
            previous
                .as_ref()
                .map(|cursor| cursor.revision.revision_id.clone())
        });
        let provisional = select_review_cursor(
            change,
            &ready.document_projection,
            revision_id.as_ref(),
            args.allow_historical,
            ReviewSourceBindingV1::Captured,
        )
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        let source_request = match args.source.as_deref() {
            Some(source) => parse_source_request(source)?,
            None => previous
                .as_ref()
                .map(|cursor| source_request_from_binding(&cursor.source_binding))
                .unwrap_or(ReviewSourceRequestV1::Captured),
        };
        let source_binding =
            review_source_binding(&args.repo, &provisional.cursor.revision, source_request)?;
        let selected = select_review_cursor(
            change,
            &ready.document_projection,
            revision_id.as_ref(),
            args.allow_historical,
            source_binding,
        )
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        Ok(serde_json::to_value(selected)?)
    })
}

fn parse_source_request(source: &str) -> Result<ReviewSourceRequestV1, Box<dyn std::error::Error>> {
    match source {
        "captured" => Ok(ReviewSourceRequestV1::Captured),
        "worktree" => Ok(ReviewSourceRequestV1::Worktree),
        _ => source
            .strip_prefix("commit:")
            .filter(|revision| !revision.is_empty())
            .map(|revision| ReviewSourceRequestV1::Commit(revision.to_owned()))
            .ok_or_else(|| {
                "--source must be captured, worktree, or commit:<rev>"
                    .to_owned()
                    .into()
            }),
    }
}

fn source_request_from_binding(binding: &ReviewSourceBindingV1) -> ReviewSourceRequestV1 {
    match binding {
        ReviewSourceBindingV1::Captured => ReviewSourceRequestV1::Captured,
        ReviewSourceBindingV1::WorktreeMatchV1 { .. } => ReviewSourceRequestV1::Worktree,
        ReviewSourceBindingV1::CommitMatchV1 { commit_oid, .. } => {
            ReviewSourceRequestV1::Commit(commit_oid.clone())
        }
    }
}

fn run_exact(
    args: ExactReadArgs,
    stdout: &mut dyn Write,
    contextual: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    with_facade(&args.repo, &args.format_args, stdout, |facade, ready| {
        let change_id = ChangeId::new(args.change);
        let exact = exact_ref(
            ready,
            &change_id,
            &RevisionId::new(args.revision),
            &args.artifact_hash,
        )?;
        let exact_read = build_exact_read(&args.repo, ready, &exact, args.include_body)?;
        if contextual {
            Ok(serde_json::to_value(facade.contextual_revision_document(
                &change_id,
                &exact,
                exact_read.resource,
                exact_read.facts,
                exact_read.associations,
            )?)?)
        } else {
            Ok(serde_json::to_value(exact_read.resource)?)
        }
    })
}

fn run_interdiff(
    args: InterdiffArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    with_facade(&args.repo, &args.format_args, stdout, |_facade, ready| {
        let change_id = ChangeId::new(args.change);
        Ok(serde_json::to_value(build_interdiff(
            ready,
            &change_id,
            &RevisionId::new(args.from),
            &args.from_artifact_hash,
            &RevisionId::new(args.to),
            &args.to_artifact_hash,
        )?)?)
    })
}

pub(crate) fn build_interdiff(
    ready: &ChangeReaderReadyV1,
    change_id: &ChangeId,
    from_revision_id: &RevisionId,
    from_artifact_hash: &str,
    to_revision_id: &RevisionId,
    to_artifact_hash: &str,
) -> Result<RevisionInterdiffDocumentV1, Box<dyn std::error::Error>> {
    let from = exact_ref(ready, change_id, from_revision_id, from_artifact_hash)?;
    let to = exact_ref(ready, change_id, to_revision_id, to_artifact_hash)?;
    Ok(RevisionInterdiffDocumentV1::new(
        RevisionInterdiffRefV1 {
            from,
            to,
            algorithm_version: "unavailable-v1".to_owned(),
            scope: Vec::new(),
        },
        RevisionInterdiffAvailabilityV1::Unavailable,
        None,
        vec!["revision_interdiff_not_available".to_owned()],
    )?)
}

pub(crate) fn exact_ref(
    ready: &ChangeReaderReadyV1,
    change_id: &ChangeId,
    revision_id: &RevisionId,
    artifact_hash: &str,
) -> Result<RevisionRefV1, Box<dyn std::error::Error>> {
    let change = ready
        .projection
        .changes
        .get(change_id)
        .ok_or_else(|| format!("Change {} is unavailable", change_id.as_str()))?;
    if !change.members.contains(revision_id) {
        return Err("exact Revision is not an active member of the Change".into());
    }
    let candidate = RevisionRefV1::new(revision_id.clone(), artifact_hash.to_owned())?;
    if !ready
        .document_projection
        .revision_refs
        .get(revision_id)
        .is_some_and(|references| references.contains(&candidate))
    {
        return Err("exact Revision/hash selector does not match authoritative state".into());
    }
    Ok(candidate)
}

pub(crate) struct ExactRead {
    pub(crate) resource: RevisionResourceDocumentV1,
    pub(crate) facts: Vec<FactPresentationV1>,
    pub(crate) associations: Vec<AssociationComparisonDocumentV1>,
}

pub(crate) fn build_exact_read(
    repo: &std::path::Path,
    ready: &ChangeReaderReadyV1,
    exact: &RevisionRefV1,
    include_body: bool,
) -> Result<ExactRead, Box<dyn std::error::Error>> {
    let result = show_revision_for_change_reader_ready(
        RevisionShowOptions::new(repo)
            .with_revision_id(exact.revision_id.clone())
            .with_exact(true)
            .with_include_body(include_body)
            .with_read_for_display(true),
        ready,
    )?;
    if result.revision.object_artifact_content_hash != exact.object_artifact_content_hash {
        return Err("exact Revision projection returned a different artifact hash".into());
    }
    let facts = fact_presentations(&result, exact);
    let associations = association_documents(&result, exact)?;
    let resource_ref = RevisionResourceRefV1 {
        revision: exact.clone(),
        object_id: result.revision.object_id.clone(),
    };
    let projection = RevisionResourceProjectionV1 {
        track_id: result.filters.track_id.clone(),
        include_body,
    };
    let memberships = ready
        .document_projection
        .membership_claims
        .iter()
        .filter(|claim| claim.active && claim.revision_id == exact.revision_id)
        .cloned()
        .collect();
    let state = result.snapshot_content_state;
    let unavailable = unavailable_content_availability(&result.diagnostics);
    let exact_document = revision_show_document_v3(result, exact.clone(), memberships)?;
    let resource = match state {
        SnapshotContentState::Present => RevisionResourceDocumentV1::available(
            resource_ref,
            projection,
            &exact.object_artifact_content_hash,
            serde_json::to_value(exact_document)?,
        )?,
        SnapshotContentState::SuppressedPresent | SnapshotContentState::PhysicallyRemoved => {
            RevisionResourceDocumentV1::unavailable(
                resource_ref,
                projection,
                ContentAvailabilityV1::Removed,
            )?
        }
        SnapshotContentState::Unavailable => {
            RevisionResourceDocumentV1::unavailable(resource_ref, projection, unavailable)?
        }
    };
    Ok(ExactRead {
        resource,
        facts,
        associations,
    })
}

fn unavailable_content_availability(
    diagnostics: &[pointbreak::session::ProjectionDiagnostic],
) -> ContentAvailabilityV1 {
    if diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "snapshot_content_unavailable"
            && diagnostic.message.to_ascii_lowercase().contains("mismatch")
    }) {
        ContentAvailabilityV1::Mismatch
    } else {
        ContentAvailabilityV1::Missing
    }
}

fn fact_presentations(
    result: &pointbreak::session::RevisionShowResult,
    exact: &RevisionRefV1,
) -> Vec<FactPresentationV1> {
    let mut facts = Vec::new();
    for view in &result.observations {
        facts.push(fact(
            view.id.as_str(),
            "observation",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.status == ObservationStatus::Active {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
        ));
    }
    for view in &result.input_requests {
        facts.push(fact(
            view.id.as_str(),
            "input_request",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            FactFamilyStateV1::Current,
        ));
    }
    for view in &result.assessments {
        facts.push(fact(
            view.id.as_str(),
            "assessment",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.status == AssessmentRecordStatus::Current {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
        ));
    }
    for view in &result.validation_checks {
        facts.push(fact(
            view.id.as_str(),
            "validation",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.superseded_by_revisions.is_empty() {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
        ));
    }
    facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
    facts
}

fn fact(
    fact_id: &str,
    family: &str,
    exact: &RevisionRefV1,
    actor_id: &pointbreak::model::ActorId,
    track_id: Option<pointbreak::model::TrackId>,
    family_state: FactFamilyStateV1,
) -> FactPresentationV1 {
    FactPresentationV1 {
        fact_id: fact_id.to_owned(),
        family: family.to_owned(),
        origin_revision: exact.clone(),
        context_change_id: None,
        presented_in_revision: None,
        port_relation: None,
        actor_id: actor_id.clone(),
        track_id,
        family_state,
        revision_currency: pointbreak::documents::ChangeRevisionCurrencyV1::Current,
        availability: ContentAvailabilityV1::Available,
    }
}

fn association_documents(
    result: &pointbreak::session::RevisionShowResult,
    exact: &RevisionRefV1,
) -> Result<Vec<AssociationComparisonDocumentV1>, Box<dyn std::error::Error>> {
    result
        .commit_range
        .current_commits
        .iter()
        .filter_map(|association| {
            association
                .commit_association_id
                .clone()
                .map(|association_id| {
                    AssociationComparisonDocumentV1::new(
                        AssociationComparisonRefV1 {
                            revision: exact.clone(),
                            association_id,
                            commit_oid: association.commit_oid.clone(),
                            comparison_base: "captured_revision".to_owned(),
                            view_kind: "landing".to_owned(),
                            proof_ref: None,
                        },
                        AssociationComparisonStateV1::Unknown,
                        AssociationProofAvailabilityV1::NotRequested,
                        vec!["comparison_proof_not_requested".to_owned()],
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn write<T: serde::Serialize>(
    format_args: &output::FormatArgs,
    stdout: &mut dyn Write,
    document: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = output::resolve_format(format_args.explicit(), output::OutputFormat::Json)?;
    output::write_document(stdout, format, document, || {
        serde_json::to_string_pretty(document).unwrap_or_else(|_| "unavailable".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_content_preserves_mismatch_as_a_distinct_typed_state() {
        let mismatch = pointbreak::session::ProjectionDiagnostic {
            code: "snapshot_content_unavailable".to_owned(),
            message: "snapshot content is unavailable: object artifact content hash mismatch"
                .to_owned(),
        };
        assert_eq!(
            unavailable_content_availability(&[mismatch]),
            ContentAvailabilityV1::Mismatch
        );
        assert_eq!(
            unavailable_content_availability(&[]),
            ContentAvailabilityV1::Missing
        );
    }
}
