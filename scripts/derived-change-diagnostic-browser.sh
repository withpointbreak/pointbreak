#!/usr/bin/env bash
# Run the narrow Change-detail browser diagnostic over one disposable public fixture.

set -euo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
usage: POINTBREAK_BINARY=<absolute exact binary> derived-change-diagnostic-browser.sh \
  --root <empty external case root> --campaign-id <nonempty> \
  --iterations <positive bounded count>
EOF
}

resolve_program() {
  local requested="$1"
  local explicit="$2"
  if [ -n "$explicit" ]; then
    case "$explicit" in
      /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
      *) die "$explicit must be an absolute program path" ;;
    esac
  fi
  command -v "$requested" 2>/dev/null || die "$requested is required"
}

git_program="$(resolve_program "${POINTBREAK_GIT_PROGRAM:-git}" "${POINTBREAK_GIT_PROGRAM:-}")"
jq_program="$(resolve_program "${POINTBREAK_JQ_PROGRAM:-jq}" "${POINTBREAK_JQ_PROGRAM:-}")"
node_program="$(resolve_program "${POINTBREAK_NODE_PROGRAM:-node}" "${POINTBREAK_NODE_PROGRAM:-}")"
shasum_program="$(resolve_program "${POINTBREAK_SHASUM_PROGRAM:-shasum}" "${POINTBREAK_SHASUM_PROGRAM:-}")"
find_program="$(resolve_program "${POINTBREAK_FIND_PROGRAM:-find}" "${POINTBREAK_FIND_PROGRAM:-}")"
sort_program="$(resolve_program "${POINTBREAK_SORT_PROGRAM:-sort}" "${POINTBREAK_SORT_PROGRAM:-}")"
wc_program="$(resolve_program "${POINTBREAK_WC_PROGRAM:-wc}" "${POINTBREAK_WC_PROGRAM:-}")"
tr_program="$(resolve_program "${POINTBREAK_TR_PROGRAM:-tr}" "${POINTBREAK_TR_PROGRAM:-}")"
awk_program="$(resolve_program "${POINTBREAK_AWK_PROGRAM:-awk}" "${POINTBREAK_AWK_PROGRAM:-}")"
cp_program="$(resolve_program "${POINTBREAK_CP_PROGRAM:-cp}" "${POINTBREAK_CP_PROGRAM:-}")"
head_program="$(resolve_program "${POINTBREAK_HEAD_PROGRAM:-head}" "${POINTBREAK_HEAD_PROGRAM:-}")"
dirname_program="$(resolve_program "${POINTBREAK_DIRNAME_PROGRAM:-dirname}" "${POINTBREAK_DIRNAME_PROGRAM:-}")"
mkdir_program="$(resolve_program "${POINTBREAK_MKDIR_PROGRAM:-mkdir}" "${POINTBREAK_MKDIR_PROGRAM:-}")"
rm_program="$(resolve_program "${POINTBREAK_RM_PROGRAM:-rm}" "${POINTBREAK_RM_PROGRAM:-}")"
chmod_program="$(resolve_program "${POINTBREAK_CHMOD_PROGRAM:-chmod}" "${POINTBREAK_CHMOD_PROGRAM:-}")"
case "${BASH:-}" in
  /*) bash_program="$BASH" ;;
  *) die "current Bash interpreter must be absolute" ;;
esac
allowed_signers_path="${POINTBREAK_ALLOWED_SIGNERS_PATH:-}"
expected_fixture_id="${POINTBREAK_EXPECTED_FIXTURE_ID:-}"
expected_authoritative_inventory_sha256="${POINTBREAK_EXPECTED_AUTHORITATIVE_INVENTORY_SHA256:-}"
expected_fixture_witness_sha256="${POINTBREAK_EXPECTED_FIXTURE_WITNESS_SHA256:-}"
expected_topology_materializer_sha256="${POINTBREAK_EXPECTED_TOPOLOGY_MATERIALIZER_SHA256:-}"
expected_source_commit="${POINTBREAK_EXPECTED_SOURCE_COMMIT:-}"
expected_source_tree="${POINTBREAK_EXPECTED_SOURCE_TREE:-}"
cygpath_binding="${POINTBREAK_CYGPATH_PROGRAM:-}"

if [ -n "$allowed_signers_path" ]; then
  case "$allowed_signers_path" in
    /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
    *) die "POINTBREAK_ALLOWED_SIGNERS_PATH must be absolute" ;;
  esac
  [ -f "$allowed_signers_path" ] && [ ! -L "$allowed_signers_path" ] \
    || die "POINTBREAK_ALLOWED_SIGNERS_PATH must be a regular non-symlink file"
fi
[ -n "$expected_fixture_id" ] || die "POINTBREAK_EXPECTED_FIXTURE_ID is required"
[[ "$expected_authoritative_inventory_sha256" =~ ^[0-9a-f]{64}$ ]] \
  || die "POINTBREAK_EXPECTED_AUTHORITATIVE_INVENTORY_SHA256 must be a SHA-256"
[[ "$expected_fixture_witness_sha256" =~ ^[0-9a-f]{64}$ ]] \
  || die "POINTBREAK_EXPECTED_FIXTURE_WITNESS_SHA256 must be a SHA-256"
[[ "$expected_topology_materializer_sha256" =~ ^[0-9a-f]{64}$ ]] \
  || die "POINTBREAK_EXPECTED_TOPOLOGY_MATERIALIZER_SHA256 must be a SHA-256"
[[ "$expected_source_commit" =~ ^[0-9a-f]{40}$ ]] \
  || die "POINTBREAK_EXPECTED_SOURCE_COMMIT must be a Git commit ID"
[[ "$expected_source_tree" =~ ^[0-9a-f]{40}$ ]] \
  || die "POINTBREAK_EXPECTED_SOURCE_TREE must be a Git tree ID"
case "$cygpath_binding" in
  absent) cygpath_program="" ;;
  /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*)
    cygpath_program="$(resolve_program "$cygpath_binding" "$cygpath_binding")"
    ;;
  *) die "POINTBREAK_CYGPATH_PROGRAM must be an absolute program path or absent" ;;
esac

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
template="$script_dir/derived-change-diagnostic-browser.mjs"
diagnostics="$script_dir/change-inspector-browser-diagnostics.mjs"
materializer="$script_dir/materialize-inspector-decision-matrix.sh"
pointbreak_binary="${POINTBREAK_BINARY:-}"
root=""
campaign_id=""
iterations=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root="${2:-}"; shift 2 ;;
    --campaign-id) campaign_id="${2:-}"; shift 2 ;;
    --iterations) iterations="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[ -n "$root" ] || die "--root <empty external case root> is required"
[ -n "$campaign_id" ] || die "--campaign-id <nonempty> is required"
[[ "$campaign_id" != *$'\n'* && "$campaign_id" != *$'\r'* ]] || die "--campaign-id must be one line"
[[ "$iterations" =~ ^[1-9][0-9]*$ ]] || die "--iterations must be a positive integer"
[ "$iterations" -le 32 ] || die "--iterations must not exceed 32"
[ -n "$pointbreak_binary" ] || die "POINTBREAK_BINARY must name the exact worktree binary"
[ -x "$pointbreak_binary" ] || die "POINTBREAK_BINARY is not executable: $pointbreak_binary"
case "$pointbreak_binary" in
  /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
  *) die "POINTBREAK_BINARY must be an absolute executable path" ;;
esac

[ -f "$template" ] || die "browser diagnostic template is missing"
[ -f "$diagnostics" ] || die "browser diagnostics are missing"
[ -x "$materializer" ] || die "public fixture materializer is not executable"

verify_source_state() {
  local phase="$1"
  [ -z "$("$git_program" -C "$repo_root" status --porcelain --untracked-files=all)" ] \
    || die "source worktree must be clean (${phase})"
  source_commit="$("$git_program" -C "$repo_root" rev-parse HEAD)"
  [ "$source_commit" = "$expected_source_commit" ] \
    || die "source ${phase} check found a commit that differs from the expected authority"
  source_tree="$("$git_program" -C "$repo_root" rev-parse "$source_commit^{tree}")"
  [ "$source_tree" = "$expected_source_tree" ] \
    || die "source ${phase} check found a tree that differs from the expected authority"
}

verify_source_state "before snapshot"
if [ -n "$allowed_signers_path" ]; then
  "$git_program" -c "gpg.ssh.allowedSignersFile=$allowed_signers_path" -C "$repo_root" verify-commit "$source_commit" >/dev/null \
    || die "source commit must have a valid signature"
else
  "$git_program" -C "$repo_root" verify-commit "$source_commit" >/dev/null \
    || die "source commit must have a valid signature"
fi

if [ -e "$root" ]; then
  [ -d "$root" ] || die "root exists and is not a directory"
  [ -z "$("$find_program" "$root" -mindepth 1 -maxdepth 1 -print -quit)" ] || die "root must be empty"
else
  "$mkdir_program" -p "$root"
fi
root="$(cd "$root" && pwd -P)"
case "$root" in
  "$repo_root"|"$repo_root"/*) die "root must be outside the source worktree" ;;
esac

log_dir="$root/logs"
artifact_dir="$root/browser-artifacts"
harness_dir="$root/harness"
snapshot_scripts="$harness_dir/scripts"
snapshot_ready_store="$harness_dir/tests/support/assets/change-ready-store"
fixture_root="$root/public-l2"
fixture_repo="$fixture_root/repository"
pointbreak_home="$fixture_root/pointbreak-home"
"$mkdir_program" -p "$log_dir" "$artifact_dir" "$snapshot_scripts" "$snapshot_ready_store" "$fixture_root" "$pointbreak_home"

background_pids=()
session=""
browser_open=false
pwcli=()

run_pw() {
  (cd "$artifact_dir" && "${pwcli[@]}" -s="$session" "$@")
}

cleanup() {
  local pid
  if [ "$browser_open" = true ]; then
    run_pw close >"$log_dir/browser-close.log" 2>&1 || true
    browser_open=false
  fi
  for pid in "${background_pids[@]}"; do
    if kill -0 "$pid" >/dev/null 2>&1; then kill "$pid" >/dev/null 2>&1 || true; fi
    wait "$pid" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

activation_fixture="5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
completion_fixture="f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json"
for path in \
  scripts/derived-change-diagnostic-browser.sh \
  scripts/derived-change-diagnostic-browser.mjs \
  scripts/change-inspector-browser-diagnostics.mjs \
  scripts/materialize-inspector-decision-matrix.sh; do
  "$git_program" -C "$repo_root" show "$source_commit:$path" >"$harness_dir/$path"
done
for fixture in "$activation_fixture" "$completion_fixture"; do
  "$git_program" -C "$repo_root" show "$source_commit:tests/support/assets/change-ready-store/$fixture" >"$snapshot_ready_store/$fixture"
done
"$chmod_program" 0555 "$snapshot_scripts/derived-change-diagnostic-browser.sh" "$snapshot_scripts/materialize-inspector-decision-matrix.sh"
"$chmod_program" 0444 "$snapshot_scripts/derived-change-diagnostic-browser.mjs" "$snapshot_scripts/change-inspector-browser-diagnostics.mjs" "$snapshot_ready_store/$activation_fixture" "$snapshot_ready_store/$completion_fixture"

requested_binary="$pointbreak_binary"
binary_sha256="$("$shasum_program" -a 256 "$pointbreak_binary" | "$awk_program" '{print $1}')"
binary_snapshot="$harness_dir/pointbreak"
"$cp_program" "$pointbreak_binary" "$binary_snapshot"
"$chmod_program" 0555 "$binary_snapshot"
[ "$("$shasum_program" -a 256 "$binary_snapshot" | "$awk_program" '{print $1}')" = "$binary_sha256" ] || die "binary snapshot did not match the injected executable"

shell_sha256="$("$shasum_program" -a 256 "$snapshot_scripts/derived-change-diagnostic-browser.sh" | "$awk_program" '{print $1}')"
template_sha256="$("$shasum_program" -a 256 "$snapshot_scripts/derived-change-diagnostic-browser.mjs" | "$awk_program" '{print $1}')"
diagnostics_sha256="$("$shasum_program" -a 256 "$snapshot_scripts/change-inspector-browser-diagnostics.mjs" | "$awk_program" '{print $1}')"
materializer_sha256="$("$shasum_program" -a 256 "$snapshot_scripts/materialize-inspector-decision-matrix.sh" | "$awk_program" '{print $1}')"
[ "$materializer_sha256" = "$expected_topology_materializer_sha256" ] \
  || die "topology materializer snapshot differs from the expected SHA-256"
materializer_tool_inventory() {
  local cygpath_sha256=""
  if [ -n "$cygpath_program" ]; then
    cygpath_sha256="$("$shasum_program" -a 256 "$cygpath_program" | "$awk_program" '{print $1}')"
  fi
  "$jq_program" -cn \
    --arg bashPath "$bash_program" --arg bashSha256 "$("$shasum_program" -a 256 "$bash_program" | "$awk_program" '{print $1}')" \
    --arg gitPath "$git_program" --arg gitSha256 "$("$shasum_program" -a 256 "$git_program" | "$awk_program" '{print $1}')" \
    --arg jqPath "$jq_program" --arg jqSha256 "$("$shasum_program" -a 256 "$jq_program" | "$awk_program" '{print $1}')" \
    --arg findPath "$find_program" --arg findSha256 "$("$shasum_program" -a 256 "$find_program" | "$awk_program" '{print $1}')" \
    --arg sortPath "$sort_program" --arg sortSha256 "$("$shasum_program" -a 256 "$sort_program" | "$awk_program" '{print $1}')" \
    --arg wcPath "$wc_program" --arg wcSha256 "$("$shasum_program" -a 256 "$wc_program" | "$awk_program" '{print $1}')" \
    --arg trPath "$tr_program" --arg trSha256 "$("$shasum_program" -a 256 "$tr_program" | "$awk_program" '{print $1}')" \
    --arg awkPath "$awk_program" --arg awkSha256 "$("$shasum_program" -a 256 "$awk_program" | "$awk_program" '{print $1}')" \
    --arg hashPath "$shasum_program" --arg hashSha256 "$("$shasum_program" -a 256 "$shasum_program" | "$awk_program" '{print $1}')" \
    --arg cpPath "$cp_program" --arg cpSha256 "$("$shasum_program" -a 256 "$cp_program" | "$awk_program" '{print $1}')" \
    --arg headPath "$head_program" --arg headSha256 "$("$shasum_program" -a 256 "$head_program" | "$awk_program" '{print $1}')" \
    --arg dirnamePath "$dirname_program" --arg dirnameSha256 "$("$shasum_program" -a 256 "$dirname_program" | "$awk_program" '{print $1}')" \
    --arg mkdirPath "$mkdir_program" --arg mkdirSha256 "$("$shasum_program" -a 256 "$mkdir_program" | "$awk_program" '{print $1}')" \
    --arg rmPath "$rm_program" --arg rmSha256 "$("$shasum_program" -a 256 "$rm_program" | "$awk_program" '{print $1}')" \
    --arg cygpathPath "$cygpath_program" --arg cygpathSha256 "$cygpath_sha256" \
    '{bash: {path: $bashPath, sha256: $bashSha256}, git: {path: $gitPath, sha256: $gitSha256}, jq: {path: $jqPath, sha256: $jqSha256}, find: {path: $findPath, sha256: $findSha256}, sort: {path: $sortPath, sha256: $sortSha256}, wc: {path: $wcPath, sha256: $wcSha256}, tr: {path: $trPath, sha256: $trSha256}, awk: {path: $awkPath, sha256: $awkSha256}, hash: {path: $hashPath, sha256: $hashSha256}, cp: {path: $cpPath, sha256: $cpSha256}, head: {path: $headPath, sha256: $headSha256}, dirname: {path: $dirnamePath, sha256: $dirnameSha256}, mkdir: {path: $mkdirPath, sha256: $mkdirSha256}, rm: {path: $rmPath, sha256: $rmSha256}} + (if $cygpathPath == "" then {} else {cygpath: {path: $cygpathPath, sha256: $cygpathSha256}} end)'
}
materializer_tools="$(materializer_tool_inventory)"
"$jq_program" -n \
  --arg campaignId "$campaign_id" --arg sourceCommit "$source_commit" --arg sourceTree "$source_tree" \
  --arg requestedBinary "$requested_binary" --arg binarySha256 "$binary_sha256" \
  --arg shellSha256 "$shell_sha256" --arg templateSha256 "$template_sha256" \
  --arg diagnosticsSha256 "$diagnostics_sha256" --arg materializerSha256 "$materializer_sha256" \
  --arg fixtureId "$expected_fixture_id" --arg authoritativeInventorySha256 "$expected_authoritative_inventory_sha256" \
  --arg fixtureWitnessSha256 "$expected_fixture_witness_sha256" \
  --arg topologyMaterializerSha256 "$expected_topology_materializer_sha256" --arg cygpathBinding "$cygpath_binding" --argjson materializerTools "$materializer_tools" \
  '{campaignId: $campaignId, sourceCommit: $sourceCommit, sourceTree: $sourceTree, binary: {requestedPath: $requestedBinary, sha256: $binarySha256}, harness: {shellSha256: $shellSha256, templateSha256: $templateSha256, diagnosticsSha256: $diagnosticsSha256, materializerSha256: $materializerSha256, materializerCygpathBinding: $cygpathBinding, materializerTools: $materializerTools}, fixture: {id: $fixtureId, authoritativeInventorySha256: $authoritativeInventorySha256, witnessSha256: $fixtureWitnessSha256, topologyMaterializerSha256: $topologyMaterializerSha256}}' \
  >"$log_dir/harness.json"

pointbreak_binary="$binary_snapshot"
"$pointbreak_binary" version --format json >"$log_dir/pointbreak-version.json"
"$jq_program" -e --arg source_commit "$source_commit" '.schema == "pointbreak.version" and .version == 1 and .build.source == "git" and .build.commit == $source_commit and .build.dirty == false' "$log_dir/pointbreak-version.json" >/dev/null || die "injected binary does not attest the clean exact source commit"

POINTBREAK_HOME="$pointbreak_home" POINTBREAK_BINARY="$pointbreak_binary" POINTBREAK_CHANGE_READY_FIXTURE_DIR="$snapshot_ready_store" \
  POINTBREAK_GIT_PROGRAM="$git_program" POINTBREAK_JQ_PROGRAM="$jq_program" POINTBREAK_FIND_PROGRAM="$find_program" POINTBREAK_SORT_PROGRAM="$sort_program" POINTBREAK_WC_PROGRAM="$wc_program" POINTBREAK_TR_PROGRAM="$tr_program" POINTBREAK_AWK_PROGRAM="$awk_program" POINTBREAK_HASH_PROGRAM="$shasum_program" POINTBREAK_HASH_PROGRAM_MODE=shasum POINTBREAK_CP_PROGRAM="$cp_program" POINTBREAK_HEAD_PROGRAM="$head_program" POINTBREAK_DIRNAME_PROGRAM="$dirname_program" POINTBREAK_MKDIR_PROGRAM="$mkdir_program" POINTBREAK_RM_PROGRAM="$rm_program" POINTBREAK_CYGPATH_PROGRAM="$cygpath_binding" \
  "$BASH" "$snapshot_scripts/materialize-inspector-decision-matrix.sh" "$fixture_repo" >"$log_dir/fixture-witness.json" 2>"$log_dir/fixture-materialize.log"
fixture_witness_sha256="$("$shasum_program" -a 256 "$log_dir/fixture-witness.json" | "$awk_program" '{print $1}')"
[ "$fixture_witness_sha256" = "$expected_fixture_witness_sha256" ] \
  || die "fixture witness bytes differ from the expected SHA-256"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store derived build --repo "$fixture_repo" --format json >"$log_dir/derived-build.json" 2>"$log_dir/derived-build.log"
"$jq_program" -e --arg fixture_id "$expected_fixture_id" --arg authoritative_inventory_sha256 "$expected_authoritative_inventory_sha256" \
  '.schema == "pointbreak.qualification-derived-change-fixture-witness.v1" and .fixtureId == $fixture_id and .authoritativeInventorySha256 == $authoritative_inventory_sha256' \
  "$log_dir/fixture-witness.json" >/dev/null || die "public fixture witness differs from the expected authority"

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" inspect --repo "$fixture_repo" --port 0 --format json >"$log_dir/inspect-startup.json" 2>"$log_dir/inspect-server.log" &
server_pid=$!
background_pids+=("$server_pid")
for _ in $(seq 1 100); do
  [ -s "$log_dir/inspect-startup.json" ] && break
  kill -0 "$server_pid" >/dev/null 2>&1 || die "Inspector exited before startup"
  sleep 0.05
done
"$jq_program" -e '.schema == "pointbreak.inspect-startup" and .version == 1 and (.port > 0) and (.token | length > 0)' "$log_dir/inspect-startup.json" >/dev/null || die "Inspector did not emit valid startup JSON"
server="$("$jq_program" -c '{baseUrl: ("http://" + .host + ":" + (.port | tostring)), token}' "$log_dir/inspect-startup.json")"

if [ -n "${PLAYWRIGHT_CLI:-}" ]; then
  pwcli=("$PLAYWRIGHT_CLI")
elif command -v playwright-cli >/dev/null 2>&1; then
  pwcli=(playwright-cli)
else
  command -v npx >/dev/null 2>&1 || die "playwright-cli and npx are unavailable"
  pwcli=(npx --yes --package @playwright/cli@0.1.17 playwright-cli)
fi
session="pointbreak-derived-change-diagnostic-browser-$$"
browser_open=true
browser_config="$("$jq_program" -cn --arg campaignId "$campaign_id" --arg artifactDir "$artifact_dir" --arg sourceCommit "$source_commit" --arg sourceTree "$source_tree" --arg fixtureId "$expected_fixture_id" --arg authoritativeInventorySha256 "$expected_authoritative_inventory_sha256" --arg witnessSha256 "$expected_fixture_witness_sha256" --arg topologyMaterializerSha256 "$expected_topology_materializer_sha256" --argjson server "$server" --argjson iterations "$iterations" '{campaignId: $campaignId, artifactDir: $artifactDir, source: {commit: $sourceCommit, tree: $sourceTree}, fixture: {id: $fixtureId, authoritativeInventorySha256: $authoritativeInventorySha256, witnessSha256: $witnessSha256, topologyMaterializerSha256: $topologyMaterializerSha256}, server: $server, iterations: $iterations}')"
browser_program="$log_dir/browser-program.mjs"
"$node_program" --input-type=module -e '
  import fs from "node:fs";
  const source = fs.readFileSync(process.argv[1], "utf8");
  if (!source.includes("__POINTBREAK_DERIVED_CHANGE_DIAGNOSTIC_BROWSER_CONFIG__")) throw new Error("browser diagnostic template marker is missing");
  fs.writeFileSync(process.argv[3], source.replace("__POINTBREAK_DERIVED_CHANGE_DIAGNOSTIC_BROWSER_CONFIG__", process.argv[2]));
' "$snapshot_scripts/derived-change-diagnostic-browser.mjs" "$browser_config" "$browser_program"

run_pw open about:blank >"$log_dir/browser-open.log" 2>&1
browser_status=0
run_pw run-code --filename="$browser_program" >"$log_dir/browser.log" 2>&1 || browser_status=$?
browser_result="$log_dir/browser-result.json"
browser_result_line="$("$awk_program" '{ line = $0; sub(/\r$/, "", line); if (after_result) { result = line; after_result = 0 }; if (line == "### Result") after_result = 1 } END { if (result != "") print result }' "$log_dir/browser.log")"
[ -n "$browser_result_line" ] || die "browser did not return a diagnostic result"
printf '%s\n' "$browser_result_line" >"$browser_result"
"$jq_program" -e --arg campaign_id "$campaign_id" --argjson iterations "$iterations" '.schema == "pointbreak.derived-change-diagnostic-collection.v1" and .campaignId == $campaign_id and .iterations == $iterations and (.status == "passed" or .status == "failed") and (.cases | type == "array") and ((.cases | length) == ($iterations + 3)) and (.cases | any(.id == "browser-bootstrap")) and (.cases | any(.id == "browser-runtime-pageerror")) and (.cases | any(.id == "browser-runtime-console")) and (.cases | all(.status == "passed" or .status == "failed" or .status == "skipped")) and (.status == (if (.cases | all(.status == "passed")) then "passed" else "failed" end))' "$browser_result" >/dev/null || die "browser returned an invalid diagnostic result"
[ "$browser_status" -eq 0 ] || die "browser diagnostic program did not complete"
[ "$("$shasum_program" -a 256 "$pointbreak_binary" | "$awk_program" '{print $1}')" = "$binary_sha256" ] || die "executed binary snapshot changed during the diagnostic"
[ "$("$shasum_program" -a 256 "$snapshot_scripts/derived-change-diagnostic-browser.mjs" | "$awk_program" '{print $1}')" = "$template_sha256" ] || die "browser template snapshot changed during the diagnostic"
[ "$("$shasum_program" -a 256 "$snapshot_scripts/materialize-inspector-decision-matrix.sh" | "$awk_program" '{print $1}')" = "$materializer_sha256" ] || die "materializer snapshot changed during the diagnostic"
[ "$(materializer_tool_inventory)" = "$materializer_tools" ] || die "materializer tools changed during the diagnostic"
verify_source_state "after browser diagnostic"

if [ "$("$jq_program" -r '.status' "$browser_result")" != "passed" ]; then
  "$jq_program" -r '.cases[] | select(.status == "failed") | "[\(.id)] \(.phase): \(.actual | tojson)"' "$browser_result" >&2
  exit 1
fi
