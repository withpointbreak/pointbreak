use std::io::Read;
use std::path::Path;

use clap::ValueEnum;
use shoreline::model::Side;
use shoreline::session::DelegationMap;

/// Discover the layered delegation map under `<worktree-root>/.shore/`.
///
/// Two files compose, git-config style: the committed shared default
/// `.shore/delegates.json` and a locally-excluded private override
/// `.shore/delegates.local.json`. The local file's records for an agent fully
/// replace the committed records for that agent (via
/// [`DelegationMap::with_local_override`]); agents absent from the local file
/// inherit the committed map; either file may exist alone.
///
/// `repo` may be the worktree root or any path inside it, matching what the read
/// commands accept — so discovery resolves the worktree root first (the same root
/// the store resolves against) before joining the delegates paths; a non-git
/// context (e.g. an exported bundle) falls back to `repo` as given, per the ADR's
/// "works in any non-git context".
///
/// Presence is per-file: when **neither** file exists, returns `None` (zero-setup
/// stores see zero change). A malformed file is **advisory** — a one-line warning
/// to stderr names the parse error and that file is treated as absent, so a bad
/// local override never poisons the committed default (ADR-0003). Shared by every
/// review read command and the inspector server.
pub(crate) fn discover_delegation_map(repo: &Path) -> Option<DelegationMap> {
    let worktree_root =
        shoreline::git::git_worktree_root(repo).unwrap_or_else(|_| repo.to_path_buf());
    let committed = load_optional_delegates(&worktree_root.join(".shore/delegates.json"));
    let local = load_optional_delegates(&worktree_root.join(".shore/delegates.local.json"));
    match (committed, local) {
        (None, None) => None,
        (committed, local) => Some(
            committed
                .unwrap_or_default()
                .with_local_override(local.unwrap_or_default()),
        ),
    }
}

/// Load a delegates file if present; a malformed file is advisory — warn once to
/// stderr and treat it as absent (per-file, so a bad local never poisons the
/// committed default).
fn load_optional_delegates(path: &Path) -> Option<DelegationMap> {
    if !path.exists() {
        return None;
    }
    match DelegationMap::from_delegates_file(path) {
        Ok(map) => Some(map),
        Err(error) => {
            eprintln!("warning: ignoring {}: {error}", path.display());
            None
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(super) enum SideArg {
    Old,
    New,
}

pub(crate) fn read_body_input(
    inline: Option<&str>,
    file: Option<&Path>,
    stdin: bool,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if let Some(inline) = inline {
        return Ok(Some(inline.to_owned()));
    }
    if let Some(path) = file {
        return Ok(Some(std::fs::read_to_string(path)?));
    }
    if stdin {
        let mut body = String::new();
        std::io::stdin().read_to_string(&mut body)?;
        return Ok(Some(body));
    }
    Ok(None)
}

impl From<SideArg> for Side {
    fn from(value: SideArg) -> Self {
        match value {
            SideArg::Old => Side::Old,
            SideArg::New => Side::New,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use shoreline::model::ActorId;
    use shoreline::session::PrincipalResolution;

    // Minimal committed map: claude-code -> KEVIN.
    const COMMITTED: &str = r#"{"delegates":{"actor:agent:claude-code":[
      {"principal":"actor:git-email:kevin@swiber.dev","validFrom":"2026-06-10T00:00:00Z","validUntil":null}]}}"#;
    // Local override: claude-code -> ALICE.
    const LOCAL: &str = r#"{"delegates":{"actor:agent:claude-code":[
      {"principal":"actor:git-email:alice@example.com","validFrom":"2026-06-10T00:00:00Z","validUntil":null}]}}"#;

    #[test]
    fn read_body_input_prefers_inline_then_file_then_stdin_false() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let body_path = dir.path().join("body.txt");
        std::fs::write(&body_path, "from file").expect("write body file");

        let body = super::read_body_input(Some("from inline"), Some(&body_path), false)
            .expect("body input resolves");

        assert_eq!(body, Some("from inline".to_string()));
    }

    #[test]
    fn discovers_committed_delegates_json() {
        let repo = git_repo();
        write(&repo, ".shore/delegates.json", COMMITTED);
        let map = super::discover_delegation_map(repo.path()).expect("committed map discovered");
        assert!(matches!(
            map.resolve(&ActorId::new("actor:agent:claude-code"), "2026-06-12T00:00:00Z"),
            PrincipalResolution::Resolved(p) if p.as_str() == "actor:git-email:kevin@swiber.dev"));
    }

    #[test]
    fn local_override_layers_over_committed() {
        let repo = git_repo();
        write(&repo, ".shore/delegates.json", COMMITTED);
        write(&repo, ".shore/delegates.local.json", LOCAL);
        let map = super::discover_delegation_map(repo.path()).expect("layered map");
        assert!(matches!(
            map.resolve(&ActorId::new("actor:agent:claude-code"), "2026-06-12T00:00:00Z"),
            PrincipalResolution::Resolved(p) if p.as_str() == "actor:git-email:alice@example.com"));
    }

    #[test]
    fn local_alone_is_used_when_committed_absent() {
        let repo = git_repo();
        write(&repo, ".shore/delegates.local.json", LOCAL);
        assert!(super::discover_delegation_map(repo.path()).is_some());
    }

    #[test]
    fn neither_file_present_returns_none() {
        let repo = git_repo();
        assert!(super::discover_delegation_map(repo.path()).is_none());
    }

    #[test]
    fn malformed_local_is_advisory_and_falls_back_to_committed() {
        let repo = git_repo();
        write(&repo, ".shore/delegates.json", COMMITTED);
        write(&repo, ".shore/delegates.local.json", "{ not json");
        // Malformed local is advisory (ADR-0003): the committed default still applies.
        let map =
            super::discover_delegation_map(repo.path()).expect("committed survives bad local");
        assert!(matches!(
            map.resolve(&ActorId::new("actor:agent:claude-code"), "2026-06-12T00:00:00Z"),
            PrincipalResolution::Resolved(p) if p.as_str() == "actor:git-email:kevin@swiber.dev"));
    }

    fn git_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("create temp git repository directory");
        let output = Command::new("git")
            .arg("init")
            .current_dir(repo.path())
            .output()
            .expect("run git init");
        assert!(output.status.success(), "git init failed");
        repo
    }

    fn write(repo: &tempfile::TempDir, rel: &str, contents: &str) {
        let path = repo.path().join(rel);
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(path, contents).expect("write file");
    }
}
