#!/usr/bin/env bash
# Verify the Change-first Inspector in a real browser over a disposable public L2 fixture.
# Prefer `just change-inspector-browser-verify <empty-root>`; every generated file stays below root.

set -euo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
usage: change-inspector-browser-verify.sh --root <empty-directory>
       change-inspector-browser-verify.sh --shakedown
       change-inspector-browser-verify.sh --shakedown-timeline-boundary
       change-inspector-browser-verify.sh --shakedown-exact-history-focus
       change-inspector-browser-verify.sh --shakedown-return-destinations

Runs the public L2 Change matrix against an exact injected Pointbreak binary.
The root must be empty and outside this worktree. Logs, screenshots, fixture
repositories, the disposable POINTBREAK_HOME, and completion-last manifest all
remain under that root.

Every --shakedown* mode creates and cleans its own temporary root, exercises
the shared fixture/server/browser path through one literal representative
journey, and retains nothing on success. None can be combined with --root.
EOF
}

for command in git jq node rg shasum find basename sort wc tr mv curl cp chmod mktemp rm date du uname ps awk sleep seq; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
browser_program_template="$script_dir/change-inspector-browser-verify.mjs"
browser_diagnostics="$script_dir/change-inspector-browser-diagnostics.mjs"
browser_manifest_publisher="$script_dir/change-inspector-browser-manifest.mjs"
matrix_materializer="$script_dir/materialize-inspector-decision-matrix.sh"
pointbreak_binary="${POINTBREAK_BINARY:-}"
root=""
mode="full"
shakedown_parent=""
shakedown_root=""
shakedown_started_at=""

BROWSER_PROGRAM_TIMEOUT_SECONDS=600
BROWSER_STAGE_HEARTBEAT_SECONDS=15
BROWSER_STAGE_HEARTBEAT_RENDER_RESERVE_SECONDS=2
BROWSER_STAGE_GROUP_CLEANUP_SECONDS=10

atomic_publish_stage_json() {
  local target="$1"
  local document="$2"
  local candidate="$target.tmp.$$.$RANDOM"
  [ ! -e "$target" ] || return 1
  umask 077
  printf '%s\n' "$document" >"$candidate" || return 1
  if [ -e "$target" ]; then
    rm -f -- "$candidate"
    return 1
  fi
  mv "$candidate" "$target"
}

read_process_group_id() {
  ps -o pgid= -p "$1" 2>/dev/null \
    | awk 'NR == 1 {gsub(/[[:space:]]/, ""); print; exit}'
}

is_positive_process_id() {
  case "${1:-}" in
    ""|*[!0-9]*) return 1 ;;
  esac
  [ "$1" -gt 0 ]
}

process_group_member_pids() {
  local listing
  if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST_PS_FAILURE:-}" = 1 ]; then
    return 70
  fi
  listing="$(ps -ax -o pid= -o pgid= 2>/dev/null)" || return 70
  printf '%s\n' "$listing" | awk -v group="$1" '$2 == group {print $1}'
}

latest_stage_screenshot() {
  local directory="$1"
  local candidate
  local latest=""
  for candidate in "$directory"/*.png; do
    [ -f "$candidate" ] || continue
    if [ -z "$latest" ] || [ "$candidate" -nt "$latest" ]; then
      latest="$candidate"
    fi
  done
  if [ -n "$latest" ]; then
    basename "$latest"
  fi
}

stage_screenshot_count() {
  find "$1" -maxdepth 1 -type f -name '*.png' | wc -l | tr -d ' '
}

stage_gate_log_bytes() {
  if [ -f "$1" ]; then
    wc -c <"$1" | tr -d ' '
  else
    printf '0\n'
  fi
}

render_browser_stage_heartbeat() {
  local stage_mode="$1"
  local stage_started_seconds="$2"
  local stage_child_pid="$3"
  local stage_artifact_dir="$4"
  local stage_gate_log="$5"
  local child_alive=false
  local latest
  local screenshot_count
  local gate_log_bytes
  if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST_RENDER_HANG:-}" = 1 ] \
    && [ "${POINTBREAK_BROWSER_STAGE_WORKER_RENDER:-}" = 1 ]; then
    while :; do :; done
  fi
  kill -0 "$stage_child_pid" >/dev/null 2>&1 && child_alive=true
  latest="$(latest_stage_screenshot "$stage_artifact_dir")" || return 1
  screenshot_count="$(stage_screenshot_count "$stage_artifact_dir")" || return 1
  gate_log_bytes="$(stage_gate_log_bytes "$stage_gate_log")" || return 1
  jq -cn \
    --arg mode "$stage_mode" \
    --argjson elapsedSeconds "$((SECONDS - stage_started_seconds))" \
    --argjson childAlive "$child_alive" \
    --argjson screenshotCount "$screenshot_count" \
    --arg latestScreenshot "$latest" \
    --argjson gateLogBytes "$gate_log_bytes" \
    '{mode: $mode, elapsedSeconds: $elapsedSeconds, childAlive: $childAlive,
      screenshotCount: $screenshotCount,
      latestScreenshot: (if $latestScreenshot == "" then null else $latestScreenshot end),
      gateLogBytes: $gateLogBytes}'
}

append_browser_stage_heartbeat() {
  local document
  document="$(render_browser_stage_heartbeat "$@")" || return 1
  printf '%s\n' "$document" >>"$6"
}

browser_stage_heartbeat_deadline_guard() {
  local next_deadline="$1"
  local supervisor_pid="$2"
  local guard_timer_pid=""
  local cleanup_done=false
  local now_seconds
  local wait_seconds

  # Invoked by the guard's EXIT trap.
  # shellcheck disable=SC2329
  cleanup_heartbeat_guard() {
    [ "$cleanup_done" = false ] || return 0
    cleanup_done=true
    [ -n "$guard_timer_pid" ] || return 0
    kill "$guard_timer_pid" >/dev/null 2>&1 || true
    wait "$guard_timer_pid" >/dev/null 2>&1 || true
    guard_timer_pid=""
  }
  trap 'cleanup_heartbeat_guard' EXIT
  trap 'exit 0' TERM INT
  while :; do
    now_seconds="$(date +%s)"
    [ "$now_seconds" -le "$next_deadline" ] || break
    wait_seconds="$((next_deadline - now_seconds + 1))"
    sleep "$wait_seconds" &
    guard_timer_pid=$!
    case "${POINTBREAK_BROWSER_STAGE_SELFTEST_CASE:-}" in
      heartbeat-render-bounded|heartbeat-render-deadline)
        printf '%s\n' "$guard_timer_pid" \
          >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/heartbeat-guard-timer.pid"
        ;;
    esac
    wait "$guard_timer_pid" || exit 0
    guard_timer_pid=""
  done
  kill -USR2 "$supervisor_pid" >/dev/null 2>&1 || true
}

browser_stage_heartbeat_worker() {
  local stage_mode="$1"
  local stage_started_seconds="$2"
  local stage_child_pid="$3"
  local stage_artifact_dir="$4"
  local stage_gate_log="$5"
  local stage_heartbeat_log="$6"
  local heartbeat_seconds="$7"
  local heartbeat_render_reserve_seconds="$8"
  local supervisor_pid="$9"
  local timer_pid=""
  local guard_pid=""
  local render_pid=""
  local render_output="$stage_heartbeat_log.render.$RANDOM"
  local cleanup_done=false
  local dispatch_at
  local document
  local next_deadline
  local now_seconds
  local wait_seconds

  # Invoked by the worker's signal trap.
  # shellcheck disable=SC2329
  stop_heartbeat_timer() {
    [ -n "$timer_pid" ] || return 0
    kill "$timer_pid" >/dev/null 2>&1 || true
    wait "$timer_pid" >/dev/null 2>&1 || true
    timer_pid=""
  }
  # Invoked by the worker's EXIT trap and the successful append path.
  # shellcheck disable=SC2329
  stop_heartbeat_guard() {
    [ -n "$guard_pid" ] || return 0
    kill "$guard_pid" >/dev/null 2>&1 || true
    wait "$guard_pid" >/dev/null 2>&1 || true
    guard_pid=""
  }
  # Invoked by the worker's signal trap.
  # shellcheck disable=SC2329
  stop_heartbeat_render() {
    [ -n "$render_pid" ] || return 0
    kill "$render_pid" >/dev/null 2>&1 || true
    wait "$render_pid" >/dev/null 2>&1 || true
    render_pid=""
    rm -f -- "$render_output"
  }
  # Invoked by the worker's EXIT trap.
  # shellcheck disable=SC2329
  cleanup_heartbeat_worker() {
    [ "$cleanup_done" = false ] || return 0
    cleanup_done=true
    stop_heartbeat_timer
    stop_heartbeat_guard
    stop_heartbeat_render
  }
  trap 'cleanup_heartbeat_worker' EXIT
  trap 'exit 0' TERM INT
  next_deadline="$(($(date +%s) + heartbeat_seconds))"
  if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST_TIMEOUT_HEARTBEAT_IDLE:-}" = 1 ]; then
    while :; do
      sleep "$heartbeat_seconds" &
      timer_pid=$!
      wait "$timer_pid" || exit 0
      timer_pid=""
    done
  fi
  while :; do
    dispatch_at="$((next_deadline - heartbeat_render_reserve_seconds))"
    now_seconds="$(date +%s)"
    wait_seconds="$((dispatch_at - now_seconds))"
    if [ "$wait_seconds" -gt 0 ]; then
      sleep "$wait_seconds" &
      timer_pid=$!
      wait "$timer_pid" || exit 0
      timer_pid=""
    fi
    now_seconds="$(date +%s)"
    if [ "$now_seconds" -gt "$dispatch_at" ]; then
      kill -USR2 "$supervisor_pid" >/dev/null 2>&1 || true
      exit 1
    fi
    browser_stage_heartbeat_deadline_guard "$next_deadline" "$supervisor_pid" &
    guard_pid=$!
    case "${POINTBREAK_BROWSER_STAGE_SELFTEST_CASE:-}" in
      heartbeat-render-bounded|heartbeat-render-deadline)
        printf '%s\n' "$guard_pid" \
          >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/heartbeat-guard.pid"
        ;;
    esac
    if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST_HEARTBEAT_FAILURE:-}" = 1 ]; then
      kill -USR2 "$supervisor_pid" >/dev/null 2>&1 || true
      exit 1
    fi
    if [ -n "${POINTBREAK_BROWSER_STAGE_SELFTEST_HEARTBEAT_DELAY_SECONDS:-}" ]; then
      sleep "$POINTBREAK_BROWSER_STAGE_SELFTEST_HEARTBEAT_DELAY_SECONDS" &
      timer_pid=$!
      wait "$timer_pid" || exit 0
      timer_pid=""
    fi
    (
      export POINTBREAK_BROWSER_STAGE_WORKER_RENDER=1
      render_browser_stage_heartbeat \
        "$stage_mode" "$stage_started_seconds" "$stage_child_pid" \
        "$stage_artifact_dir" "$stage_gate_log" "$stage_heartbeat_log"
    ) >"$render_output" &
    render_pid=$!
    if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST:-}" = 1 ]; then
      printf '%s\n' "$render_pid" \
        >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/heartbeat-render.pid"
    fi
    if ! wait "$render_pid"; then
      render_pid=""
      rm -f -- "$render_output"
      kill -USR2 "$supervisor_pid" >/dev/null 2>&1 || true
      exit 1
    fi
    render_pid=""
    document="$(<"$render_output")"
    rm -f -- "$render_output"
    printf '%s\n' "$document" >>"$stage_heartbeat_log" || {
      kill -USR2 "$supervisor_pid" >/dev/null 2>&1 || true
      exit 1
    }
    now_seconds="$(date +%s)"
    if [ "$now_seconds" -gt "$next_deadline" ]; then
      kill -USR2 "$supervisor_pid" >/dev/null 2>&1 || true
      exit 1
    fi
    stop_heartbeat_guard
    if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST_CASE:-}" = heartbeat-render-bounded ]; then
      : >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/heartbeat-render-bounded-published"
    fi
    next_deadline="$((next_deadline + heartbeat_seconds))"
  done
}

browser_stage_watchdog_worker() {
  local timeout_seconds="$1"
  local supervisor_pid="$2"
  local timer_pid=""
  # Invoked by the worker's signal trap.
  # shellcheck disable=SC2329
  stop_watchdog_timer() {
    [ -n "$timer_pid" ] || return 0
    kill "$timer_pid" >/dev/null 2>&1 || true
    wait "$timer_pid" >/dev/null 2>&1 || true
    timer_pid=""
  }
  trap 'stop_watchdog_timer; exit 0' TERM INT
  case "${POINTBREAK_BROWSER_STAGE_SELFTEST_CASE:-}" in
    precedence-exit-timeout|precedence-timeout-exit)
      while [ ! -e "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/watchdog-release" ]; do
        sleep 0.01 &
        timer_pid=$!
        wait "$timer_pid" || exit 0
        timer_pid=""
      done
      : >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/watchdog-fired"
      kill -USR1 "$supervisor_pid" >/dev/null 2>&1 || true
      return 0
      ;;
  esac
  sleep "$timeout_seconds" &
  timer_pid=$!
  wait "$timer_pid" || exit 0
  timer_pid=""
  kill -USR1 "$supervisor_pid" >/dev/null 2>&1 || true
}

browser_stage_terminal_kind=""
browser_stage_terminal_code=""
browser_stage_terminal_signal=""

observe_browser_stage_terminal() {
  local kind="$1"
  local code="${2:-}"
  local signal="${3:-}"
  [ -z "$browser_stage_terminal_kind" ] || return 1
  browser_stage_terminal_kind="$kind"
  browser_stage_terminal_code="$code"
  browser_stage_terminal_signal="$signal"
}

stop_browser_stage_worker() {
  local pid="${1:-}"
  [ -n "$pid" ] || return 0
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
}

browser_stage_child_reaped=false
browser_stage_child_status=""

reap_browser_stage_child_if_done() {
  local pid="$1"
  local process_state
  local process_status
  [ "$browser_stage_child_reaped" = false ] || return 0
  process_status=0
  process_state="$(ps -o stat= -p "$pid" 2>/dev/null \
    | awk 'NR == 1 {gsub(/[[:space:]]/, ""); print; exit}')" \
    || process_status=$?
  if [ "$process_status" -ne 0 ] && kill -0 "$pid" >/dev/null 2>&1; then
    return 1
  fi
  case "$process_state" in
    ""|Z*)
      if wait "$pid" >/dev/null 2>&1; then
        browser_stage_child_status=0
      else
        browser_stage_child_status=$?
      fi
      browser_stage_child_reaped=true
      return 0
      ;;
  esac
  return 1
}

cleanup_browser_stage_group() {
  local child_pid="$1"
  local group_id="$2"
  local cleanup_seconds="$3"
  local cleanup_started="$SECONDS"
  local kill_after="$((cleanup_started + (cleanup_seconds / 2)))"
  local cleanup_deadline="$((cleanup_started + cleanup_seconds))"
  local sent_kill=false
  local members=""
  local members_known=false

  if members="$(process_group_member_pids "$group_id")"; then
    members_known=true
  fi
  if [ "$members_known" = false ] || [ -n "$members" ]; then
    kill -TERM -- "-$group_id" >/dev/null 2>&1 || true
  fi
  while { [ "$members_known" = false ] || [ -n "$members" ]; } \
    && [ "$SECONDS" -lt "$cleanup_deadline" ]; do
    reap_browser_stage_child_if_done "$child_pid" || true
    if [ "$sent_kill" = false ] && [ "$SECONDS" -ge "$kill_after" ]; then
      kill -KILL -- "-$group_id" >/dev/null 2>&1 || true
      sent_kill=true
    fi
    sleep 0.05
    members=""
    members_known=false
    if members="$(process_group_member_pids "$group_id")"; then
      members_known=true
    fi
  done
  if { [ "$members_known" = false ] || [ -n "$members" ]; } \
    && [ "$sent_kill" = false ]; then
    kill -KILL -- "-$group_id" >/dev/null 2>&1 || true
  fi
  reap_browser_stage_child_if_done "$child_pid" || true
  members=""
  members_known=false
  if members="$(process_group_member_pids "$group_id")"; then
    members_known=true
  fi
  [ "$members_known" = true ] \
    && [ -z "$members" ] \
    && [ "$browser_stage_child_reaped" = true ]
}

run_browser_program_stage() {
  local stage_mode="$1"
  local stage_log_dir="$2"
  local stage_artifact_dir="$3"
  local timeout_seconds="$4"
  local heartbeat_seconds="$5"
  local heartbeat_render_reserve_seconds="$6"
  local cleanup_seconds="$7"
  shift 7
  [ "${1:-}" = "--" ] || return 125
  shift

  case "$heartbeat_seconds" in
    ""|*[!0-9]*) return 125 ;;
  esac
  case "$heartbeat_render_reserve_seconds" in
    ""|*[!0-9]*) return 125 ;;
  esac
  [ "$heartbeat_seconds" -gt 0 ] || return 125
  [ "$heartbeat_render_reserve_seconds" -lt "$heartbeat_seconds" ] || return 125

  local stage_start="$stage_log_dir/browser-stage-start.json"
  local stage_heartbeats="$stage_log_dir/browser-stage-heartbeat.log"
  local stage_terminal="$stage_log_dir/browser-stage-terminal.json"
  local stage_gate_log="$stage_log_dir/browser-gate.log"
  local launch_gate="$stage_log_dir/.browser-stage-launch.$$.$RANDOM"
  local started_at
  local finished_at
  local started_seconds="$SECONDS"
  local shell_group_id
  local child_group_id
  local child_pid
  local heartbeat_pid=""
  local watchdog_pid=""
  local cleanup_status="complete"
  local stage_return=0
  local latest
  local screenshot_count
  local gate_log_bytes
  local terminal_document
  local start_document
  local supervisor_pid="$$"

  browser_stage_terminal_kind=""
  browser_stage_terminal_code=""
  browser_stage_terminal_signal=""
  browser_stage_child_reaped=false
  browser_stage_child_status=""
  started_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  shell_group_id=""
  if ! shell_group_id="$(read_process_group_id "$$")" \
    || ! is_positive_process_id "$shell_group_id"; then
    return 125
  fi

  set -m
  (
    while [ ! -e "$launch_gate" ]; do sleep 0.01; done
    "$@"
  ) >"$stage_gate_log" 2>&1 &
  child_pid=$!
  set +m

  child_group_id=""
  if ! is_positive_process_id "$child_pid" \
    || ! child_group_id="$(read_process_group_id "$child_pid")" \
    || ! is_positive_process_id "$child_group_id" \
    || [ "$child_pid" != "$child_group_id" ] \
    || [ "$child_group_id" = "$shell_group_id" ]; then
    kill "$child_pid" >/dev/null 2>&1 || true
    wait "$child_pid" >/dev/null 2>&1 || true
    rm -f -- "$launch_gate"
    return 125
  fi

  trap 'observe_browser_stage_terminal signal 130 INT || true' INT
  trap 'observe_browser_stage_terminal signal 143 TERM || true' TERM
  trap 'observe_browser_stage_terminal timeout 124 "" || true' USR1
  trap 'observe_browser_stage_terminal internal 125 "" || true' USR2

  start_document="$(jq -cn \
    --arg mode "$stage_mode" \
    --arg startedAt "$started_at" \
    --argjson timeoutSeconds "$timeout_seconds" \
    --argjson childPid "$child_pid" \
    --argjson processGroupId "$child_group_id" \
    '{schema: "pointbreak.browser-stage-start", version: 1, mode: $mode,
      startedAt: $startedAt, timeoutSeconds: $timeoutSeconds,
      childPid: $childPid, processGroupId: $processGroupId}')"
  atomic_publish_stage_json "$stage_start" "$start_document" || {
    kill "$child_pid" >/dev/null 2>&1 || true
    wait "$child_pid" >/dev/null 2>&1 || true
    rm -f -- "$launch_gate"
    trap - INT TERM USR1 USR2
    return 125
  }
  : >"$stage_heartbeats"

  append_browser_stage_heartbeat \
    "$stage_mode" "$started_seconds" "$child_pid" \
    "$stage_artifact_dir" "$stage_gate_log" "$stage_heartbeats" || {
      observe_browser_stage_terminal internal 125 "" || true
    }
  browser_stage_heartbeat_worker \
    "$stage_mode" "$started_seconds" "$child_pid" \
    "$stage_artifact_dir" "$stage_gate_log" "$stage_heartbeats" \
    "$heartbeat_seconds" "$heartbeat_render_reserve_seconds" "$supervisor_pid" &
  heartbeat_pid=$!
  browser_stage_watchdog_worker "$timeout_seconds" "$supervisor_pid" &
  watchdog_pid=$!
  : >"$launch_gate"

  while [ -z "$browser_stage_terminal_kind" ]; do
    if reap_browser_stage_child_if_done "$child_pid"; then
      observe_browser_stage_terminal exit "$browser_stage_child_status" "" || true
      break
    fi
    sleep 0.05
  done

  case "${POINTBREAK_BROWSER_STAGE_SELFTEST_CASE:-}" in
    precedence-exit-timeout|precedence-timeout-exit)
      printf '%s\n' "$browser_stage_terminal_kind" \
        >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/winner-observed"
      while [ ! -e "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/supervisor-release" ]; do
        sleep 0.01
      done
      ;;
  esac
  if reap_browser_stage_child_if_done "$child_pid"; then
    observe_browser_stage_terminal exit "$browser_stage_child_status" "" || true
  fi

  stop_browser_stage_worker "$heartbeat_pid"
  stop_browser_stage_worker "$watchdog_pid"
  rm -f -- "$launch_gate"
  if ! cleanup_browser_stage_group \
    "$child_pid" "$child_group_id" "$cleanup_seconds"; then
    cleanup_status="failed"
  fi

  finished_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  latest="$(latest_stage_screenshot "$stage_artifact_dir")"
  screenshot_count="$(stage_screenshot_count "$stage_artifact_dir")"
  gate_log_bytes="$(stage_gate_log_bytes "$stage_gate_log")"
  terminal_document="$(jq -cn \
    --arg mode "$stage_mode" \
    --arg startedAt "$started_at" \
    --arg finishedAt "$finished_at" \
    --arg winner "$browser_stage_terminal_kind" \
    --arg terminalSignal "$browser_stage_terminal_signal" \
    --arg latestScreenshot "$latest" \
    --arg cleanup "$cleanup_status" \
    --argjson elapsedSeconds "$((SECONDS - started_seconds))" \
    --argjson timeoutSeconds "$timeout_seconds" \
    --argjson childPid "$child_pid" \
    --argjson processGroupId "$child_group_id" \
    --argjson exitCode "${browser_stage_terminal_code:-null}" \
    --argjson screenshotCount "$screenshot_count" \
    --argjson gateLogBytes "$gate_log_bytes" \
    '{schema: "pointbreak.browser-stage-terminal", version: 1, mode: $mode,
      startedAt: $startedAt, finishedAt: $finishedAt,
      elapsedSeconds: $elapsedSeconds, timeoutSeconds: $timeoutSeconds,
      winner: $winner, exitCode: $exitCode,
      signal: (if $terminalSignal == "" then null else $terminalSignal end),
      childPid: $childPid, processGroupId: $processGroupId,
      screenshotCount: $screenshotCount,
      latestScreenshot: (if $latestScreenshot == "" then null else $latestScreenshot end),
      gateLogBytes: $gateLogBytes, runCodeGroupCleanup: $cleanup,
      browserSessionCleanup: "pending"}')"
  if ! atomic_publish_stage_json "$stage_terminal" "$terminal_document"; then
    trap - INT TERM USR1 USR2
    return 125
  fi
  printf '%s\n' "$terminal_document"
  trap - INT TERM USR1 USR2

  if [ "$cleanup_status" = failed ]; then
    stage_return=125
  else
    case "$browser_stage_terminal_kind" in
      exit) stage_return="${browser_stage_terminal_code:-125}" ;;
      timeout) stage_return=124 ;;
      signal) stage_return="${browser_stage_terminal_code:-125}" ;;
      *) stage_return=125 ;;
    esac
  fi
  return "$stage_return"
}

browser_stage_selftest_child() {
  (
    trap 'exit 0' TERM INT
    while :; do sleep 1; done
  ) &
  printf '%s\n' "$!" >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/fake-descendant.pid"
  case "$POINTBREAK_BROWSER_STAGE_SELFTEST_CASE" in
    exit-0|ps-failed)
      sleep 0.4
      return 0
      ;;
    exit-23)
      sleep 0.4
      return 23
      ;;
    precedence-exit-timeout|precedence-timeout-exit)
      while [ ! -e "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/child-release" ]; do
        sleep 0.01
      done
      : >"$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/fake-child-exited"
      return 0
      ;;
    heartbeat-render-hung)
      sleep 1.4
      return 0
      ;;
    heartbeat-render-bounded)
      while [ ! -e "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/heartbeat-render-bounded-published" ]; do
        sleep 0.01
      done
      return 0
      ;;
    timeout|signal-int|signal-term|heartbeat-failed|heartbeat-late|heartbeat-render-deadline)
      while :; do sleep 1; done
      ;;
  esac
  return 125
}

if [ "${POINTBREAK_BROWSER_STAGE_SELFTEST:-}" = 1 ]; then
  [ -n "${POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT:-}" ] \
    || die "browser-stage selftest root is required"
  [ -d "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT" ] \
    || die "browser-stage selftest root must exist"
  [ -z "$(find "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT" -mindepth 1 -maxdepth 1 -print -quit)" ] \
    || die "browser-stage selftest root must be empty"
  case "${POINTBREAK_BROWSER_STAGE_SELFTEST_CASE:-}" in
    exit-0|exit-23|timeout|signal-int|signal-term|precedence-exit-timeout|precedence-timeout-exit|heartbeat-failed|heartbeat-late|heartbeat-render-hung|heartbeat-render-bounded|heartbeat-render-deadline|ps-failed) ;;
    *) die "unsupported browser-stage selftest case" ;;
  esac
  mkdir -p \
    "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/logs" \
    "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/browser-artifacts"
  browser_stage_selftest_timeout=2
  browser_stage_selftest_heartbeat_seconds=1
  browser_stage_selftest_render_reserve_seconds=0
  case "$POINTBREAK_BROWSER_STAGE_SELFTEST_CASE" in
    timeout) export POINTBREAK_BROWSER_STAGE_SELFTEST_TIMEOUT_HEARTBEAT_IDLE=1 ;;
    heartbeat-failed) export POINTBREAK_BROWSER_STAGE_SELFTEST_HEARTBEAT_FAILURE=1 ;;
    heartbeat-late)
      export POINTBREAK_BROWSER_STAGE_SELFTEST_HEARTBEAT_DELAY_SECONDS=2
      browser_stage_selftest_timeout=4
      ;;
    heartbeat-render-hung) export POINTBREAK_BROWSER_STAGE_SELFTEST_RENDER_HANG=1 ;;
    heartbeat-render-bounded)
      export POINTBREAK_BROWSER_STAGE_SELFTEST_HEARTBEAT_DELAY_SECONDS=1
      browser_stage_selftest_timeout=8
      browser_stage_selftest_heartbeat_seconds=4
      browser_stage_selftest_render_reserve_seconds=2
      ;;
    heartbeat-render-deadline)
      export POINTBREAK_BROWSER_STAGE_SELFTEST_RENDER_HANG=1
      browser_stage_selftest_timeout=8
      browser_stage_selftest_heartbeat_seconds=4
      browser_stage_selftest_render_reserve_seconds=2
      ;;
    ps-failed) export POINTBREAK_BROWSER_STAGE_SELFTEST_PS_FAILURE=1 ;;
  esac
  browser_stage_selftest_status=0
  run_browser_program_stage \
    "selftest" \
    "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/logs" \
    "$POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT/browser-artifacts" \
    "$browser_stage_selftest_timeout" \
    "$browser_stage_selftest_heartbeat_seconds" \
    "$browser_stage_selftest_render_reserve_seconds" 2 \
    -- browser_stage_selftest_child \
    || browser_stage_selftest_status=$?
  exit "$browser_stage_selftest_status"
fi

cleanup_shakedown_root() {
  local cleanup_root="${shakedown_root:-}"
  [ -n "$cleanup_root" ] || return 0
  [ -n "$shakedown_parent" ] || return 1
  [ "$cleanup_root" != "/" ] || return 1
  case "$cleanup_root" in
    "$shakedown_parent"/pointbreak-change-inspector-shakedown.*) ;;
    *) return 1 ;;
  esac
  rm -rf -- "$cleanup_root"
  [ ! -e "$cleanup_root" ] || return 1
  shakedown_root=""
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root="${2:-}"; shift 2 ;;
    --shakedown) mode="shakedown"; shift ;;
    --shakedown-timeline-boundary) mode="shakedown-timeline-boundary"; shift ;;
    --shakedown-exact-history-focus) mode="shakedown-exact-history-focus"; shift ;;
    --shakedown-return-destinations) mode="shakedown-return-destinations"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

case "$mode" in
  shakedown|shakedown-timeline-boundary|shakedown-exact-history-focus|shakedown-return-destinations)
    [ -z "$root" ] || die "$mode creates its own root and cannot use --root"
    shakedown_parent="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
    shakedown_root="$(mktemp -d "$shakedown_parent/pointbreak-change-inspector-shakedown.XXXXXX")"
    root="$shakedown_root"
    shakedown_started_at="$(date +%s)"
    trap cleanup_shakedown_root EXIT
    ;;
  full)
    [ -n "$root" ] || die "--root <empty-directory> is required"
    ;;
  *)
    die "unsupported browser verification mode: $mode"
    ;;
esac
[ -n "$pointbreak_binary" ] || die "POINTBREAK_BINARY must name the exact worktree binary"
[ -x "$pointbreak_binary" ] || die "POINTBREAK_BINARY is not executable: $pointbreak_binary"
case "$pointbreak_binary" in
  /* | [A-Za-z]:/* | [A-Za-z]:\\* | \\\\*) ;;
  *) die "POINTBREAK_BINARY must be an absolute executable path" ;;
esac
case "${POINTBREAK_DERIVED_ACCESS:-}" in
  "" | sqlite-wal-bodyless-v1) ;;
  *) die "POINTBREAK_DERIVED_ACCESS must be unset or sqlite-wal-bodyless-v1" ;;
esac
[ -f "$browser_program_template" ] || die "browser program is missing: $browser_program_template"
[ -f "$browser_diagnostics" ] || die "browser diagnostics are missing: $browser_diagnostics"
[ -f "$browser_manifest_publisher" ] || die "browser manifest publisher is missing: $browser_manifest_publisher"
[ -x "$matrix_materializer" ] || die "matrix materializer is not executable: $matrix_materializer"

[ -z "$(git -C "$repo_root" status --porcelain --untracked-files=all)" ] \
  || die "source worktree must be clean so the manifest names an exact source commit"

if [ -e "$root" ]; then
  [ -d "$root" ] || die "root exists and is not a directory: $root"
  [ -z "$(find "$root" -mindepth 1 -maxdepth 1 -print -quit)" ] \
    || die "root is not empty: $root"
else
  mkdir -p "$root"
fi
root="$(cd "$root" && pwd -P)"
case "$root" in
  "$repo_root"|"$repo_root"/*) die "root must be outside the Pointbreak source worktree" ;;
esac

fixture_root="$root/public-l2-change-matrix"
fixture_repo="$fixture_root/repository"
pointbreak_home="$fixture_root/pointbreak-home"
artifact_dir="$root/browser-artifacts"
log_dir="$root/logs"
mkdir -p "$fixture_root" "$pointbreak_home" "$artifact_dir" "$log_dir"

# Own every asynchronous child from the moment it is spawned. The EXIT trap is
# installed before asynchronous work begins, while browser cleanup remains
# disabled until its session command has been resolved.
background_pids=()
pwcli=()
session=""
browser_cleanup_enabled=false

run_pw() {
  (cd "$artifact_dir" && "${pwcli[@]}" -s="$session" "$@")
}

register_background_process() {
  background_pids+=("$1")
}

forget_background_process() {
  local completed_pid="$1"
  local retained_pids=()
  local pid
  for pid in "${background_pids[@]}"; do
    [ "$pid" = "$completed_pid" ] || retained_pids+=("$pid")
  done
  background_pids=("${retained_pids[@]}")
}

stop_background_process() {
  local pid="$1"
  [ -n "$pid" ] || return 0
  if kill -0 "$pid" >/dev/null 2>&1; then
    kill "$pid" >/dev/null 2>&1 || true
  fi
  wait "$pid" >/dev/null 2>&1 || true
}

cleanup() {
  local exit_status=$?
  local mode="${1:-best-effort}"
  local browser_close_status=0
  local root_cleanup_status=0
  local pid
  if [ "$browser_cleanup_enabled" = true ]; then
    if run_pw close >"$log_dir/browser-close.log" 2>&1; then
      browser_close_status=0
    else
      browser_close_status=$?
    fi
    browser_cleanup_enabled=false
  fi
  for pid in "${background_pids[@]}"; do
    stop_background_process "$pid"
  done
  background_pids=()
  local failed_shakedown=false
  if [ -n "$shakedown_root" ] && { [ "$exit_status" -ne 0 ] || [ "$browser_close_status" -ne 0 ]; }; then
    failed_shakedown=true
    node - "$log_dir" "${browser_result:-}" 2>/dev/null <<'NODE' || printf '%s\n' '{"gate":"change-inspector-browser-failure","retention":"failed"}' >&2
const fs = require('node:fs');
const [logs, reportPath] = process.argv.slice(2);
const secrets = fs.readdirSync(logs).filter(name => name.endsWith('-startup.json'))
  .map(name => JSON.parse(fs.readFileSync(`${logs}/${name}`, 'utf8')).token).filter(Boolean);
const redact = value => {
  if (typeof value === 'string') {
    for (const secret of secrets) value = value.replaceAll(secret, 'REDACTED').replaceAll(encodeURIComponent(secret), 'REDACTED');
    return value;
  }
  if (Array.isArray(value)) return value.map(redact);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, key.toLowerCase() === 'token' ? 'REDACTED' : redact(item)]));
  return value;
};
const report = reportPath && fs.existsSync(reportPath) ? JSON.parse(fs.readFileSync(reportPath, 'utf8')) : null;
console.log(JSON.stringify({gate:'change-inspector-browser-failure', report:redact(report)}));
NODE
  fi
  cleanup_shakedown_root || root_cleanup_status=$?
  if [ "$failed_shakedown" = true ]; then
    printf '{"gate":"change-inspector-browser-cleanup","browserCloseStatus":%s,"rootCleanupStatus":%s}\n' "$browser_close_status" "$root_cleanup_status"
  fi
  if [ "$mode" = strict ] && [ "$browser_close_status" -ne 0 ]; then
    return "$browser_close_status"
  fi
  if [ "$mode" = strict ] && [ "$root_cleanup_status" -ne 0 ]; then
    return "$root_cleanup_status"
  fi
  return 0
}
trap cleanup EXIT

source_commit="$(git -C "$repo_root" rev-parse HEAD)"
requested_binary="$pointbreak_binary"
binary_sha256="$(shasum -a 256 "$pointbreak_binary" | awk '{print $1}')"
snapshot_root="$root/harness"
snapshot_scripts="$snapshot_root/scripts"
snapshot_ready_store="$snapshot_root/tests/support/assets/change-ready-store"
snapshot_timeline_compat_store="$snapshot_root/tests/support/assets/inspector-timeline-compat-v1"
snapshot_legacy_note_store="$snapshot_root/tests/fixtures/legacy_stores/review_note_imported/store"
activation_fixture="5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
completion_fixture="f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json"
mkdir -p "$snapshot_scripts" "$snapshot_ready_store" \
  "$snapshot_timeline_compat_store" "$snapshot_legacy_note_store"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-verify.sh" \
  >"$snapshot_scripts/change-inspector-browser-verify.sh"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-verify.mjs" \
  >"$snapshot_scripts/change-inspector-browser-verify.mjs"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-diagnostics.mjs" \
  >"$snapshot_scripts/change-inspector-browser-diagnostics.mjs"
git -C "$repo_root" show "$source_commit:scripts/change-inspector-browser-manifest.mjs" \
  >"$snapshot_scripts/change-inspector-browser-manifest.mjs"
git -C "$repo_root" show "$source_commit:scripts/materialize-inspector-decision-matrix.sh" \
  >"$snapshot_scripts/materialize-inspector-decision-matrix.sh"
git -C "$repo_root" show "$source_commit:tests/support/assets/change-ready-store/$activation_fixture" \
  >"$snapshot_ready_store/$activation_fixture"
git -C "$repo_root" show "$source_commit:tests/support/assets/change-ready-store/$completion_fixture" \
  >"$snapshot_ready_store/$completion_fixture"

snapshot_git_tree() {
  local source_prefix="$1"
  local destination_root="$2"
  local source_path relative_path destination_path
  while IFS= read -r source_path; do
    relative_path="${source_path#"$source_prefix"/}"
    destination_path="$destination_root/$relative_path"
    mkdir -p "$(dirname "$destination_path")"
    git -C "$repo_root" show "$source_commit:$source_path" >"$destination_path"
  done < <(git -C "$repo_root" ls-tree -r --name-only "$source_commit" -- "$source_prefix")
}

snapshot_git_tree \
  "tests/support/assets/inspector-timeline-compat-v1" \
  "$snapshot_timeline_compat_store"
snapshot_git_tree \
  "tests/fixtures/legacy_stores/review_note_imported/store" \
  "$snapshot_legacy_note_store"
[ "$(find "$snapshot_timeline_compat_store" -maxdepth 1 -type f -name '*.json' | wc -l | tr -d '[:space:]')" -eq 9 ] \
  || die "source-bound Timeline compatibility fixture event count drifted"
chmod 0444 \
  "$snapshot_scripts/change-inspector-browser-verify.mjs" \
  "$snapshot_scripts/change-inspector-browser-diagnostics.mjs" \
  "$snapshot_scripts/change-inspector-browser-manifest.mjs" \
  "$snapshot_ready_store/$activation_fixture" \
  "$snapshot_ready_store/$completion_fixture"
find "$snapshot_timeline_compat_store" "$snapshot_legacy_note_store" -type f -exec chmod 0444 {} +
chmod 0555 \
  "$snapshot_scripts/change-inspector-browser-verify.sh" \
  "$snapshot_scripts/materialize-inspector-decision-matrix.sh"

binary_snapshot="$snapshot_root/pointbreak"
cp "$pointbreak_binary" "$binary_snapshot"
chmod 0555 "$binary_snapshot"
[ "$(shasum -a 256 "$binary_snapshot" | awk '{print $1}')" = "$binary_sha256" ] \
  || die "binary snapshot did not match the injected executable"

shell_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-verify.sh" | awk '{print $1}')"
template_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-verify.mjs" | awk '{print $1}')"
diagnostics_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-diagnostics.mjs" | awk '{print $1}')"
publisher_sha256="$(shasum -a 256 "$snapshot_scripts/change-inspector-browser-manifest.mjs" | awk '{print $1}')"
materializer_sha256="$(shasum -a 256 "$snapshot_scripts/materialize-inspector-decision-matrix.sh" | awk '{print $1}')"
activation_fixture_sha256="$(shasum -a 256 "$snapshot_ready_store/$activation_fixture" | awk '{print $1}')"
completion_fixture_sha256="$(shasum -a 256 "$snapshot_ready_store/$completion_fixture" | awk '{print $1}')"
compatibility_fixture_inventory="$(
  find "$snapshot_timeline_compat_store" "$snapshot_legacy_note_store" -type f -print \
    | LC_ALL=C sort \
    | while IFS= read -r fixture_file; do
        relative_path="${fixture_file#"$snapshot_root"/}"
        fixture_sha256="$(shasum -a 256 "$fixture_file" | awk '{print $1}')"
        jq -cn --arg path "$relative_path" --arg sha256 "$fixture_sha256" \
          '{path: $path, sha256: $sha256}'
      done \
    | jq -cs '.'
)"
[ "$(shasum -a 256 "$script_dir/change-inspector-browser-verify.sh" | awk '{print $1}')" = "$shell_sha256" ] \
  || die "running browser verifier did not match the exact source commit"
jq -n \
  --arg sourceCommit "$source_commit" \
  --arg requestedBinary "$requested_binary" \
  --arg executedBinary "$binary_snapshot" \
  --arg binarySha256 "$binary_sha256" \
  --arg shellSha256 "$shell_sha256" \
  --arg templateSha256 "$template_sha256" \
  --arg diagnosticsSha256 "$diagnostics_sha256" \
  --arg publisherSha256 "$publisher_sha256" \
  --arg materializerSha256 "$materializer_sha256" \
  --arg activationFixture "$activation_fixture" \
  --arg activationFixtureSha256 "$activation_fixture_sha256" \
  --arg completionFixture "$completion_fixture" \
  --arg completionFixtureSha256 "$completion_fixture_sha256" \
  --argjson compatibilityFixtureInventory "$compatibility_fixture_inventory" \
  '{schema: "pointbreak.change-inspector-browser-harness", version: 1,
    sourceCommit: $sourceCommit,
    binary: {requestedPath: $requestedBinary, executedPath: $executedBinary, sha256: $binarySha256},
    files: ([
      {path: "scripts/change-inspector-browser-verify.sh", sha256: $shellSha256},
      {path: "scripts/change-inspector-browser-verify.mjs", sha256: $templateSha256},
      {path: "scripts/change-inspector-browser-diagnostics.mjs", sha256: $diagnosticsSha256},
      {path: "scripts/change-inspector-browser-manifest.mjs", sha256: $publisherSha256},
      {path: "scripts/materialize-inspector-decision-matrix.sh", sha256: $materializerSha256},
      {path: ("tests/support/assets/change-ready-store/" + $activationFixture), sha256: $activationFixtureSha256},
      {path: ("tests/support/assets/change-ready-store/" + $completionFixture), sha256: $completionFixtureSha256}
    ] + $compatibilityFixtureInventory)}' >"$log_dir/harness-digests.json"
harness_record_sha256="$(shasum -a 256 "$log_dir/harness-digests.json" | awk '{print $1}')"

pointbreak_binary="$binary_snapshot"
browser_program_template="$snapshot_scripts/change-inspector-browser-verify.mjs"
browser_diagnostics="$snapshot_scripts/change-inspector-browser-diagnostics.mjs"
browser_manifest_publisher="$snapshot_scripts/change-inspector-browser-manifest.mjs"
matrix_materializer="$snapshot_scripts/materialize-inspector-decision-matrix.sh"
"$pointbreak_binary" version --format json >"$log_dir/pointbreak-version.json"
jq -e --arg source_commit "$source_commit" '
  .schema == "pointbreak.version" and .version == 1 and
  .build.source == "git" and .build.commit == $source_commit and .build.dirty == false
' "$log_dir/pointbreak-version.json" >/dev/null \
  || die "injected binary does not attest the clean exact source commit"

# The retained matrix supplies multiple topology and unavailable-resource cases.
# Add enough distinct Change captures to exercise the 363+ list contract without
# borrowing any owner records. Keep the scale input tracked and overwrite it
# for every capture so each captured diff stays constant-sized.
POINTBREAK_HOME="$pointbreak_home" POINTBREAK_BINARY="$pointbreak_binary" \
  POINTBREAK_TIMELINE_COMPAT_FIXTURE_DIR="$snapshot_timeline_compat_store" \
  POINTBREAK_LEGACY_NOTE_FIXTURE_DIR="$snapshot_legacy_note_store" \
  "$matrix_materializer" "$fixture_repo" \
  >"$log_dir/base-matrix.json" 2>"$log_dir/base-matrix.log"

if [ "$mode" = "full" ]; then
printf 'pub const BROWSER_SCALE: u32 = 0;\n' >"$fixture_repo/src/browser-scale.rs"
git -C "$fixture_repo" add src/browser-scale.rs
git -C "$fixture_repo" commit --quiet -m "browser scale source"

for ordinal in $(seq 1 351); do
  printf 'pub const BROWSER_SCALE_%s: u32 = %s;\n' "$ordinal" "$ordinal" \
    >"$fixture_repo/src/browser-scale.rs"
  POINTBREAK_HOME="$pointbreak_home" \
    POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
    "$pointbreak_binary" capture --repo "$fixture_repo" \
      --summary "Browser scale Change $ordinal" --format json \
      >>"$log_dir/scale-captures.jsonl" 2>>"$log_dir/scale-captures.log"
done

# An initial Change capture mints its declaration and membership assertion from
# one operation timestamp. Bind the first supported receipt's exact event pair
# so the browser can prove the Timeline's event-id tie break without depending
# on scheduler or wall-clock coincidence between separate writer processes.
equal_timestamp_pair="$(jq -s -ce '
  .[0] as $capture
  | ($capture | .changeEvents | map(select(.outcome == "created"))) as $events
  | [$events[] | select(
      .eventType == "change_declared" or
      .eventType == "change_membership_asserted"
    )] as $pair
  | if (($pair | length) == 2 and
        ($pair | map(.eventType) | unique | length) == 2 and
        ($capture.changeId | startswith("change:sha256:")))
    then {changeId: $capture.changeId, tieBreak: "event_id_asc",
      eventIds: ($pair | map(.eventId) | sort)}
    else error("first scale capture did not emit one created declaration/membership pair") end
' "$log_dir/scale-captures.jsonl")" \
  || die "supported Change capture did not provide a deterministic equal-occurredAt pair"

# One explicit exact Revision is the removed-resource case. The removal claim
# models intentional unavailability without erasing bytes, preserving a
# replayable fixture history distinct from the recoverably missing case below.
printf 'pub const BROWSER_EXACT: &str = "exact";\n' >"$fixture_repo/src/browser-scale.rs"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" capture --repo "$fixture_repo" \
    --summary "Browser exact Change" --format json \
    >"$log_dir/exact-capture.json" 2>"$log_dir/exact-capture.log"
exact_change="$(jq -er '.changeId' "$log_dir/exact-capture.json")"
exact_revision="$(jq -er '.revision.revisionId' "$log_dir/exact-capture.json")"
exact_artifact="$(jq -er '.revision.objectArtifactContentHash' "$log_dir/exact-capture.json")"
jq -e '
  .schema == "pointbreak.change-capture-receipt.v1" and .version == 1 and
  (.revision.revisionId | startswith("rev:sha256:")) and
  (.revision.objectArtifactContentHash | startswith("sha256:"))
' "$log_dir/exact-capture.json" >/dev/null \
  || die "direct Change capture did not emit the expected exact Revision schema"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" store remove --repo "$fixture_repo" --revision "$exact_revision" --format json \
    >"$log_dir/exact-resource-removed.json" 2>"$log_dir/exact-resource-removed.log"

# Turn the materializer's exact missing-object Change into an honest missing-
# resource case by moving its bound artifact to a retained recovery directory.
# Refuse unexpected hashes, symlinks, and store locations before moving bytes;
# every path remains within this caller-owned disposable evidence root.
missing_change="$(jq -er '.missing_change' "$log_dir/base-matrix.json")"
missing_revision="$(jq -er '.missing_revision' "$log_dir/base-matrix.json")"
missing_artifact="$(jq -er '.missing_artifact' "$log_dir/base-matrix.json")"
[[ "$missing_artifact" =~ ^sha256:[0-9a-f]{64}$ ]] \
  || die "missing-resource fixture emitted an invalid artifact hash: $missing_artifact"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store paths \
  --repo "$fixture_repo" --format json >"$log_dir/store-paths.json"
common_store="$(jq -er '.commonStore' "$log_dir/store-paths.json")"
[ -d "$common_store/artifacts/objects" ] \
  || die "missing-resource object store is absent: $common_store/artifacts/objects"
artifact_objects="$(cd "$common_store/artifacts/objects" && pwd -P)"
case "$artifact_objects" in
  "$root"/*) ;;
  *) die "missing-resource object store escaped the disposable root: $artifact_objects" ;;
esac
missing_digest="${missing_artifact#sha256:}"
missing_artifact_path="$artifact_objects/$missing_digest.json"
[ -f "$missing_artifact_path" ] \
  || die "bound missing-resource artifact is absent before the fixture move"
[ ! -L "$missing_artifact_path" ] \
  || die "bound missing-resource artifact must not be a symlink"
case "$missing_artifact_path" in
  "$artifact_objects"/*) ;;
  *) die "bound missing-resource artifact escaped its object store" ;;
esac
missing_recovery_dir="$fixture_root/recoverable-missing-resource"
mkdir -p "$missing_recovery_dir"
missing_recovery_dir="$(cd "$missing_recovery_dir" && pwd -P)"
case "$missing_recovery_dir" in
  "$root"/*) ;;
  *) die "missing-resource recovery directory escaped the disposable root" ;;
esac
missing_recovery_path="$missing_recovery_dir/$missing_digest.json"
[ ! -e "$missing_recovery_path" ] \
  || die "missing-resource recovery target already exists"
mv "$missing_artifact_path" "$missing_recovery_path"
[ ! -e "$missing_artifact_path" ] && [ -f "$missing_recovery_path" ] \
  || die "missing-resource artifact move did not preserve exactly one recovery copy"

# Prove the two bodyless states before publishing derived access. Removed is
# event-authorized; missing is a physical absence with its bytes still retained
# under the evidence root. Neither exact read may substitute live Git bytes.
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change resource \
  "$exact_change" "$exact_revision" --artifact-hash "$exact_artifact" \
  --repo "$fixture_repo" --format json >"$log_dir/removed-resource-preflight.json"
jq -e --arg revision "$exact_revision" --arg artifact "$exact_artifact" '
  .schema == "pointbreak.review-revision-resource" and .version == 1 and
  .availability == "removed" and
  .resource.revision.revisionId == $revision and
  .resource.revision.objectArtifactContentHash == $artifact and
  .capturedDocument == null and .capturedDocumentHash == null and
  .diagnostics == ["captured_resource_removed"]
' "$log_dir/removed-resource-preflight.json" >/dev/null \
  || die "removed-resource preflight did not remain exact and bodyless for $exact_change"

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change resource \
  "$missing_change" "$missing_revision" --artifact-hash "$missing_artifact" \
  --repo "$fixture_repo" --format json >"$log_dir/missing-resource-preflight.json"
jq -e --arg revision "$missing_revision" --arg artifact "$missing_artifact" '
  .schema == "pointbreak.review-revision-resource" and .version == 1 and
  .availability == "missing" and
  .resource.revision.revisionId == $revision and
  .resource.revision.objectArtifactContentHash == $artifact and
  .capturedDocument == null and .capturedDocumentHash == null and
  .diagnostics == ["captured_resource_missing"]
' "$log_dir/missing-resource-preflight.json" >/dev/null \
  || die "missing-resource preflight did not remain exact and bodyless for $missing_change"

# Extend the retained matrix exactly once with event families whose historical
# semantics cannot be inferred from the final Change cards alone. All writes
# use supported public commands against this disposable L2 root.
primary_change="$(jq -er '.topology.initial.change' "$log_dir/base-matrix.json")"
primary_revision="$(jq -er '.primary_revision' "$log_dir/base-matrix.json")"
primary_artifact="$(jq -er '.topology.initial.current.artifact' "$log_dir/base-matrix.json")"
historical_change="$(jq -er '.topology.parallel_current.change' "$log_dir/base-matrix.json")"
shared_revision="$(jq -er '.shared_revision.revision' "$log_dir/base-matrix.json")"
graph_change="$(jq -s -e -r '.[0].changeId' "$log_dir/scale-captures.jsonl")"
graph_successor_revision="$(jq -s -e -r '.[0].revision.revisionId' "$log_dir/scale-captures.jsonl")"
graph_successor_artifact="$(jq -s -e -r '.[0].revision.objectArtifactContentHash' "$log_dir/scale-captures.jsonl")"
graph_context_revision="$(jq -s -e -r '.[1].revision.revisionId' "$log_dir/scale-captures.jsonl")"
graph_context_artifact="$(jq -s -e -r '.[1].revision.objectArtifactContentHash' "$log_dir/scale-captures.jsonl")"

# Build claim-only relationship context from two otherwise clean scale
# Changes. Assert the relation while both exact Revisions are members, then
# withdraw the contextual membership. The active relation must remain visible
# as typed incomplete context without making an unavailable node actionable.
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change join "$graph_change" "$graph_context_revision" \
    --operation-id "change-operation:browser-graph-context-join-v1" \
    --repo "$fixture_repo" --format json \
    >"$log_dir/graph-context-join.json" \
    2>"$log_dir/graph-context-join.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show \
  "$graph_change" --repo "$fixture_repo" --format json \
  >"$log_dir/graph-context-after-join.json"
graph_context_membership_claim="$(jq -er --arg revision "$graph_context_revision" '
  [.membershipClaims[] | select(.revisionId == $revision and .active == true)]
  | if length == 1 then .[0].claimId
    else error("expected one active graph context membership claim") end
' "$log_dir/graph-context-after-join.json")"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change assert-relation "$graph_change" \
    "$graph_successor_revision" "$graph_context_revision" \
    --successor-artifact-hash "$graph_successor_artifact" \
    --predecessor-artifact-hash "$graph_context_artifact" \
    --operation-id "change-operation:browser-graph-context-relation-v1" \
    --repo "$fixture_repo" --format json \
    >"$log_dir/graph-context-relation.json" \
    2>"$log_dir/graph-context-relation.log"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change withdraw-membership "$graph_context_membership_claim" \
    --operation-id "change-operation:browser-graph-context-withdraw-v1" \
    --repo "$fixture_repo" --format json \
    >"$log_dir/graph-context-withdraw.json" \
    2>"$log_dir/graph-context-withdraw.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show \
  "$graph_change" --repo "$fixture_repo" --format json \
  >"$log_dir/graph-context-final.json"

POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" observation add --repo "$fixture_repo" \
    --exact-revision "$primary_revision" --track "agent:browser-history-cases" \
    --title "Browser correction origin" \
    --body "This observation is retained as the superseded historical fact." \
    --idempotency-key "browser-history-correction-origin-v1" --format json \
    >"$log_dir/correction-origin.json" 2>"$log_dir/correction-origin.log"
correction_origin_id="$(jq -er '.observationId' "$log_dir/correction-origin.json")"

POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" observation add --repo "$fixture_repo" \
    --exact-revision "$primary_revision" --track "agent:browser-history-cases" \
    --title "Browser correction replacement" \
    --body "This observation explicitly corrects the retained historical fact." \
    --supersedes "$correction_origin_id" \
    --idempotency-key "browser-history-correction-replacement-v1" --format json \
    >"$log_dir/correction-replacement.json" 2>"$log_dir/correction-replacement.log"

# The primary exact Revision is directly owned by its original Change. Add one
# historical membership in a second Change, resolve the exact claim from the
# typed Change document, and withdraw only that claim. Historical correlation
# must remain visible even though effective current membership does not.
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change join "$historical_change" "$primary_revision" \
    --repo "$fixture_repo" \
    --operation-id "change-operation:browser-history-membership-join-v1" --format json \
    >"$log_dir/historical-membership-join.json" \
    2>"$log_dir/historical-membership-join.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show \
  "$historical_change" --repo "$fixture_repo" --format json \
  >"$log_dir/historical-membership-after-join.json"
historical_membership_claim="$(jq -er --arg revision "$primary_revision" '
  [.membershipClaims[] | select(.revisionId == $revision and .active == true)]
  | if length == 1 then .[0].claimId
    else error("expected one active browser historical membership claim") end
' "$log_dir/historical-membership-after-join.json")"
POINTBREAK_HOME="$pointbreak_home" \
  POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
  "$pointbreak_binary" change withdraw-membership "$historical_membership_claim" \
    --repo "$fixture_repo" \
    --operation-id "change-operation:browser-history-membership-withdraw-v1" --format json \
    >"$log_dir/historical-membership-withdraw.json" \
    2>"$log_dir/historical-membership-withdraw.log"

POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store derived build \
  --repo "$fixture_repo" --format json \
  >"$log_dir/derived-build.json" 2>"$log_dir/derived-build.log"
POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change list --repo "$fixture_repo" --format json \
  >"$log_dir/changes.json"
change_count="$(jq -er '.changes | length' "$log_dir/changes.json")"
[ "$change_count" -ge 363 ] || die "expected at least 363 public matrix Changes, found $change_count"

# Prove that every topology named by the browser contract is a retained final
# state at its exact fixture identity. The shared Revision must remain an
# active member of all four non-initial topology Changes, including after it is
# historical in the consolidation row.
for topology in initial replacement parallel_current replacement_divergent consolidation; do
  topology_change="$(jq -er --arg topology "$topology" '.topology[$topology].change' "$log_dir/base-matrix.json")"
  topology_current="$(jq -c --arg topology "$topology" '
    .topology[$topology].current
    | if type == "array" then . else [.] end
    | map({revisionId: .revision, objectArtifactContentHash: .artifact})
    # Matrix metadata records fixture construction order. Change documents
    # expose the canonical RevisionId order owned by the ChangeView BTreeSet.
    | sort_by(.revisionId, .objectArtifactContentHash)
  ' "$log_dir/base-matrix.json")"
  POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" change show "$topology_change" \
    --repo "$fixture_repo" --format json >"$log_dir/topology-$topology.json"
  jq -e --arg change "$topology_change" --arg topology "$topology" \
    --argjson current "$topology_current" '
      .summary.changeId == $change and .summary.topology == $topology and
      .currentRevisionRefs == $current
    ' "$log_dir/topology-$topology.json" >/dev/null \
    || die "topology fixture $topology did not retain its exact final state"
done
for topology in replacement parallel_current replacement_divergent consolidation; do
  jq -e --arg revision "$shared_revision" '
    any(.memberRevisions[]?; .revision.revisionId == $revision)
  ' "$log_dir/topology-$topology.json" >/dev/null \
    || die "topology fixture $topology omitted the shared exact Revision"
done
jq -e \
  --arg change "$graph_change" \
  --arg successorRevision "$graph_successor_revision" \
  --arg successorArtifact "$graph_successor_artifact" \
  --arg predecessorRevision "$graph_context_revision" \
  --arg predecessorArtifact "$graph_context_artifact" '
    .summary.changeId == $change and
    .summary.topology == "incomplete" and
    .summary.currentRevisionRefs == [{
      revisionId: $successorRevision,
      objectArtifactContentHash: $successorArtifact
    }] and
    (.effectiveSupersedes | length) == 0 and
    any(.pendingOrConflictingEdges[]?;
      .active == true and
      .successor.revisionId == $successorRevision and
      .successor.objectArtifactContentHash == $successorArtifact and
      .predecessor.revisionId == $predecessorRevision and
      .predecessor.objectArtifactContentHash == $predecessorArtifact) and
    any(.diagnostics[]?; . == "change_relation_membership_incomplete")
  ' "$log_dir/graph-context-final.json" >/dev/null \
  || die "pending nonmember graph context changed or disappeared from typed topology"

rich_revision="$(jq -er '.primary_revision' "$log_dir/base-matrix.json")"
rich_change="$(jq -er --arg revision "$rich_revision" '
  [.changes[] | select(any(.currentRevisionRefs[]?; .revisionId == $revision))]
  | if length == 1 then .[0].changeId else error("expected one rich Change") end
' "$log_dir/changes.json")"
rich_artifact="$(jq -er --arg revision "$rich_revision" '
  [.changes[].currentRevisionRefs[]? | select(.revisionId == $revision)]
  | if length == 1 then .[0].objectArtifactContentHash else error("expected one rich Revision") end
' "$log_dir/changes.json")"

fixture_identity="public-l2-change-matrix-v1"
correction_event="$(jq -er '.eventId' "$log_dir/correction-replacement.json")"
fact_port_event="$(jq -er '.fact_port.event_id' "$log_dir/base-matrix.json")"
fact_port_id="$(jq -er '.fact_port.port_id' "$log_dir/base-matrix.json")"
historical_membership_join_event="$(jq -er '
  [.events[] | select(.eventType == "change_membership_asserted")]
  | if length == 1 then .[0].eventId else error("expected one membership assertion event") end
' "$log_dir/historical-membership-join.json")"
historical_membership_withdraw_event="$(jq -er '
  [.events[] | select(.eventType == "change_membership_withdrawn")]
  | if length == 1 then .[0].eventId else error("expected one membership withdrawal event") end
' "$log_dir/historical-membership-withdraw.json")"
jq -n \
  --arg fixture "$fixture_identity" \
  --arg sourceCommit "$source_commit" \
  --arg exactChange "$exact_change" \
  --arg exactRevision "$exact_revision" \
  --arg exactArtifact "$exact_artifact" \
  --arg missingChange "$missing_change" \
  --arg missingRevision "$missing_revision" \
  --arg missingArtifact "$missing_artifact" \
  --arg missingRecoveryPath "$missing_recovery_path" \
  --arg richChange "$rich_change" \
  --arg richRevision "$rich_revision" \
  --arg richArtifact "$rich_artifact" \
  --arg correctionOrigin "$correction_origin_id" \
  --arg correctionEvent "$correction_event" \
  --arg factPortId "$fact_port_id" \
  --arg factPortEvent "$fact_port_event" \
  --arg graphChange "$graph_change" \
  --arg graphSuccessorRevision "$graph_successor_revision" \
  --arg graphSuccessorArtifact "$graph_successor_artifact" \
  --arg graphContextRevision "$graph_context_revision" \
  --arg graphContextArtifact "$graph_context_artifact" \
  --arg directChange "$primary_change" \
  --arg historicalChange "$historical_change" \
  --arg historicalRevision "$primary_revision" \
  --arg historicalArtifact "$primary_artifact" \
  --arg historicalClaim "$historical_membership_claim" \
  --arg historicalJoinEvent "$historical_membership_join_event" \
  --arg historicalWithdrawEvent "$historical_membership_withdraw_event" \
  --argjson equalTimestamp "$equal_timestamp_pair" \
  --argjson changeCount "$change_count" \
  '{fixture: $fixture, sourceCommit: $sourceCommit, changeCount: $changeCount,
    removed: {changeId: $exactChange, revisionId: $exactRevision, artifactHash: $exactArtifact},
    missing: {changeId: $missingChange, revisionId: $missingRevision,
      artifactHash: $missingArtifact, recoverableArtifactPath: $missingRecoveryPath},
    rich: {changeId: $richChange, revisionId: $richRevision, artifactHash: $richArtifact},
    correction: {originObservationId: $correctionOrigin, eventId: $correctionEvent},
    factPort: {portId: $factPortId, eventId: $factPortEvent},
    graph: {changeId: $graphChange,
      successor: {revisionId: $graphSuccessorRevision, artifactHash: $graphSuccessorArtifact},
      context: {revisionId: $graphContextRevision, artifactHash: $graphContextArtifact}},
    historicalMembership: {directChangeId: $directChange,
      historicalChangeId: $historicalChange, revisionId: $historicalRevision,
      artifactHash: $historicalArtifact, claimId: $historicalClaim,
      joinEventId: $historicalJoinEvent, withdrawEventId: $historicalWithdrawEvent},
    equalTimestamp: $equalTimestamp}' \
  >"$log_dir/fixture.json"
else
  POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" store derived build \
    --repo "$fixture_repo" --format json \
    >"$log_dir/derived-build.json" 2>"$log_dir/derived-build.log"
  rich_change="$(jq -er '.topology.initial.change' "$log_dir/base-matrix.json")"
  rich_revision="$(jq -er '.primary_revision' "$log_dir/base-matrix.json")"
  rich_artifact="$(jq -er '.topology.initial.current.artifact' "$log_dir/base-matrix.json")"
  fixture_identity="public-l2-change-matrix-shakedown-v1"
  jq -n \
    --arg fixture "$fixture_identity" \
    --arg sourceCommit "$source_commit" \
    --arg richChange "$rich_change" \
    --arg richRevision "$rich_revision" \
    --arg richArtifact "$rich_artifact" \
    '{fixture: $fixture, sourceCommit: $sourceCommit,
      rich: {changeId: $richChange, revisionId: $richRevision, artifactHash: $richArtifact}}' \
    >"$log_dir/fixture.json"
fi

session="pointbreak-change-browser-$$"
if [ -n "${PLAYWRIGHT_CLI:-}" ]; then
  pwcli=("$PLAYWRIGHT_CLI")
elif command -v playwright-cli >/dev/null 2>&1; then
  pwcli=(playwright-cli)
else
  command -v npx >/dev/null 2>&1 || die "playwright-cli and npx are unavailable"
  pwcli=(npx --yes --package @playwright/cli@0.1.17 playwright-cli)
fi
browser_cleanup_enabled=true

if [ "$mode" = "full" ]; then
# Retain three tiny reader-state roots beside the primary fixture so the real
# browser can prove readiness sequencing without borrowing owner authority.
# Each repository pins its store to worktree-local ephemeral placement.
reader_state_root="$root/reader-state-fixtures"
reader_state_home="$reader_state_root/pointbreak-home"
reader_empty_l2_repo="$reader_state_root/empty-ready-l2"
reader_l0_repo="$reader_state_root/l0"
reader_m1_repo="$reader_state_root/m1"
mkdir -p "$reader_state_home"
for reader_repo in "$reader_empty_l2_repo" "$reader_l0_repo" "$reader_m1_repo"; do
  git -C "$reader_state_root" init --quiet "$reader_repo"
  git -C "$reader_repo" config user.name "Pointbreak Browser Reader Fixture"
  git -C "$reader_repo" config user.email "pointbreak-browser@example.com"
  git -C "$reader_repo" config commit.gpgsign false
  mkdir -p "$reader_repo/.pointbreak/data/events"
  printf '%s\n' \
    '{"schema":"shore.store-config","version":1,"mode":"ephemeral"}' \
    >"$reader_repo/.pointbreak/store.local.json"
  printf '%s\n' 'public Inspector reader-state fixture' >"$reader_repo/README.md"
  git -C "$reader_repo" add README.md
  git -C "$reader_repo" commit --quiet -m "reader fixture base"
done
ready_store="$snapshot_ready_store"
activation_record="$ready_store/$activation_fixture"
completion_record="$ready_store/$completion_fixture"
[ -f "$activation_record" ] && [ -f "$completion_record" ] \
  || die "public reader-state activation fixtures are unavailable"
cp "$activation_record" "$completion_record" "$reader_empty_l2_repo/.pointbreak/data/events/"
cp "$activation_record" "$reader_m1_repo/.pointbreak/data/events/"

start_reader_state_server() {
  local state="$1"
  local repo="$2"
  local startup="$log_dir/reader-$state-startup.json"
  local server_log="$log_dir/reader-$state-server.log"
  POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1 \
    POINTBREAK_HOME="$reader_state_home" "$pointbreak_binary" inspect \
    --repo "$repo" --port 0 --format json >"$startup" 2>"$server_log" &
  reader_state_started_pid=$!
  register_background_process "$reader_state_started_pid"
  for _ in $(seq 1 100); do
    [ -s "$startup" ] && break
    kill -0 "$reader_state_started_pid" >/dev/null 2>&1 \
      || die "$state Inspector exited before startup"
    sleep 0.05
  done
  jq -e '
    .schema == "pointbreak.inspect-startup" and .version == 1 and
    (.port > 0) and (.token | length > 0)
  ' "$startup" >/dev/null || die "$state Inspector did not emit valid startup JSON"
}

retry_empty_ready_l2() {
  local startup="$log_dir/reader-empty-ready-l2-startup.json"
  local base_url
  local token
  local retry_log="$log_dir/browser-empty-ready-l2-retry.json"
  local ready_log="$log_dir/browser-empty-ready-l2-ready.json"
  local ready_tmp="$ready_log.tmp"
  local response_status

  base_url="http://$(jq -r '.host' "$startup"):$(jq -r '.port' "$startup")"
  token="$(jq -r '.token' "$startup")"
  response_status="$(curl -sS -o "$retry_log" -w '%{http_code}' -X POST \
    -H "Authorization: Bearer $token" \
    "$base_url/api/derived-access/retry")"
  [ "$response_status" = "200" ] \
    || die "empty-ready-l2 derived-access retry returned HTTP $response_status"
  jq -e '
    .schema == "pointbreak.inspect-derived-access-status" and .version == 1 and
    .active == true and (.availability | type == "string") and
    (.rebuildInFlight | type == "boolean") and (.actions | type == "array")
  ' "$retry_log" >/dev/null \
    || die "empty-ready-l2 derived-access retry did not return typed status"

  for _ in $(seq 1 200); do
    response_status="$(curl -sS -o "$ready_tmp" -w '%{http_code}' \
      -H "Authorization: Bearer $token" \
      "$base_url/api/derived-access/status")"
    if [ "$response_status" = "200" ] && jq -e '
      .schema == "pointbreak.inspect-derived-access-status" and .version == 1 and
      .active == true and .servingCurrent == true and .availability == "current" and
      .rebuildInFlight == false and .rebuildPaused == false
    ' "$ready_tmp" >/dev/null; then
      mv "$ready_tmp" "$ready_log"
      return 0
    fi
    sleep 0.05
  done
  [ -f "$ready_tmp" ] && mv "$ready_tmp" "$ready_log"
  die "empty-ready-l2 did not publish a current derived generation after explicit retry"
}

start_reader_state_server "empty-ready-l2" "$reader_empty_l2_repo"
retry_empty_ready_l2
start_reader_state_server "l0" "$reader_l0_repo"
start_reader_state_server "m1" "$reader_m1_repo"
reader_servers="$(jq -cn \
  --slurpfile empty "$log_dir/reader-empty-ready-l2-startup.json" \
  --slurpfile l0 "$log_dir/reader-l0-startup.json" \
  --slurpfile m1 "$log_dir/reader-m1-startup.json" '
    def server($startup): {
      baseUrl: ("http://" + $startup.host + ":" + ($startup.port | tostring)),
      token: $startup.token
    };
    {emptyReadyL2: server($empty[0]), l0: server($l0[0]), m1: server($m1[0])}
  ')"
else
  reader_servers='{}'
fi

retain_primary_derived_access_status() {
  local startup="$log_dir/inspect-startup.json"
  local status_log="$log_dir/browser-primary-derived-access-status.json"
  local status_tmp="$status_log.tmp"
  local base_url
  local token
  local response_status

  base_url="http://$(jq -r '.host' "$startup"):$(jq -r '.port' "$startup")"
  token="$(jq -r '.token' "$startup")"
  for _ in $(seq 1 200); do
    response_status="$(curl -sS -o "$status_tmp" -w '%{http_code}' \
      -H "Authorization: Bearer $token" \
      "$base_url/api/derived-access/status")"
    if [ "$response_status" = "200" ] && jq -e '
      .schema == "pointbreak.inspect-derived-access-status" and .version == 1 and
      .active == true and .servingCurrent == true and .availability == "current" and
      .rebuildInFlight == false and .rebuildPaused == false
    ' "$status_tmp" >/dev/null; then
      mv "$status_tmp" "$status_log"
      return 0
    fi
    sleep 0.05
  done
  [ -f "$status_tmp" ] && mv "$status_tmp" "$status_log"
  die "primary Inspector did not publish an active current derived-access status"
}

POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1 \
  POINTBREAK_HOME="$pointbreak_home" "$pointbreak_binary" inspect --repo "$fixture_repo" --port 0 --format json \
  >"$log_dir/inspect-startup.json" 2>"$log_dir/inspect-server.log" &
server_pid=$!
register_background_process "$server_pid"
for _ in $(seq 1 100); do
  [ -s "$log_dir/inspect-startup.json" ] && break
  kill -0 "$server_pid" >/dev/null 2>&1 || die "Inspector exited before startup"
  sleep 0.05
done
jq -e '.schema == "pointbreak.inspect-startup" and .version == 1 and (.port > 0) and (.token | length > 0)' \
  "$log_dir/inspect-startup.json" >/dev/null || die "Inspector did not emit valid startup JSON"
server="$(jq -c '{baseUrl: ("http://" + .host + ":" + (.port | tostring)), token}' "$log_dir/inspect-startup.json")"
retain_primary_derived_access_status

# L2 remains a deliberately Change-aware reader profile: the restored Timeline
# must use `/api/v2/history`, never reactivate the retired aggregate endpoint.
legacy_status="$(curl -sS -o "$log_dir/legacy-history.json" -w '%{http_code}' \
  -H "Authorization: Bearer $(jq -r '.token' "$log_dir/inspect-startup.json")" \
  "$(jq -r '.baseUrl' <<<"$server")/api/history")"
[ "$legacy_status" = "426" ] \
  || die "legacy /api/history unexpectedly returned HTTP $legacy_status instead of 426"
jq -e '.schema == "pointbreak.reader-upgrade-required" and .version == 1' \
  "$log_dir/legacy-history.json" >/dev/null \
  || die "legacy /api/history did not return the typed reader-upgrade response"

browser_config="$(jq -cn \
  --arg artifactDir "$artifact_dir" \
  --arg appendReceipt "$log_dir/timeline-append.json" \
  --arg mode "$mode" \
  --argjson server "$server" \
  --argjson readerServers "$reader_servers" \
  --slurpfile fixture "$log_dir/fixture.json" \
  --slurpfile matrix "$log_dir/base-matrix.json" \
  '{artifactDir: $artifactDir, appendReceipt: $appendReceipt, mode: $mode, server: $server,
    readerServers: $readerServers,
    fixture: ($fixture[0] + {matrix: $matrix[0]})}')"
browser_program="$log_dir/browser-program.mjs"
# shellcheck disable=SC2016 # JavaScript template literals are intentionally single-quoted from Bash.
node --input-type=module -e '
import fs from "node:fs";
import { pathToFileURL } from "node:url";
const source = fs.readFileSync(process.argv[1], "utf8");
const diagnostics = await import(pathToFileURL(process.argv[2]));
const replacements = new Map([
  ["__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__", diagnostics.BrowserDiagnosticFailure.toString()],
  ["__POINTBREAK_BROWSER_DIAGNOSTICS__", diagnostics.createBrowserDiagnostics.toString()],
  ["__POINTBREAK_CHANGE_BROWSER_CONFIG__", process.argv[3]],
]);
let rendered = source;
for (const [marker, value] of replacements) {
  if (!rendered.includes(marker)) throw new Error(`browser program marker is missing: ${marker}`);
  rendered = rendered.replace(marker, value);
}
fs.writeFileSync(process.argv[4], rendered);
' "$browser_program_template" "$browser_diagnostics" "$browser_config" "$browser_program"

# Create the session without visiting the Inspector. The injected program installs
# console, page-error, and request-failure observers before it performs the
# capability-bearing bootstrap navigation.
run_pw open about:blank >"$log_dir/browser-open.log" 2>&1

# The browser program first parks the initial Timeline and writes the retained
# screenshot below.  Only then append one public fixture event.  This avoids a
# racy sleep while proving that a parked reader remains stable until its
# explicit catch-up action.  The worker changes only the disposable repository
# and writes its receipt below the caller-provided evidence root.
timeline_append_pid=""
if [ "$mode" = "full" ]; then
  timeline_append_marker="$artifact_dir/timeline-parked-before-append.png"
  (
    for _ in $(seq 1 240); do
      [ -f "$timeline_append_marker" ] && break
      sleep 0.25
    done
    [ -f "$timeline_append_marker" ] || exit 1
    printf 'pub const BROWSER_TIMELINE_APPEND: &str = "after-park";\n' \
      >"$fixture_repo/src/browser-scale.rs"
    POINTBREAK_HOME="$pointbreak_home" \
      POINTBREAK_ACTOR_ID="actor:agent:pointbreak-browser-matrix" \
      "$pointbreak_binary" capture --repo "$fixture_repo" \
        --summary "Browser Timeline append after park" --format json \
        >"$log_dir/timeline-append.json" 2>"$log_dir/timeline-append.log"
  ) &
timeline_append_pid=$!
register_background_process "$timeline_append_pid"
fi
browser_gate_status=0
run_browser_program_stage \
  "$mode" "$log_dir" "$artifact_dir" \
  "$BROWSER_PROGRAM_TIMEOUT_SECONDS" \
  "$BROWSER_STAGE_HEARTBEAT_SECONDS" \
  "$BROWSER_STAGE_HEARTBEAT_RENDER_RESERVE_SECONDS" \
  "$BROWSER_STAGE_GROUP_CLEANUP_SECONDS" \
  -- run_pw run-code --filename="$browser_program" \
  || browser_gate_status=$?
browser_result="$log_dir/browser-result.json"
if [ "$browser_gate_status" -ne 0 ]; then
  sed -n '1,240p' "$log_dir/browser-gate.log" >&2
  die "real-browser Change Inspector gate failed"
fi
browser_result_line="$(awk '
  {
    line = $0
    sub(/\r$/, "", line)
    if (after_result) {
      result = line
      after_result = 0
    }
    if (line == "### Result") after_result = 1
  }
  END {
    if (result != "") print result
  }
' "$log_dir/browser-gate.log")"
if [ -n "$browser_result_line" ]; then
  printf '%s\n' "$browser_result_line" >"$browser_result"
  jq -e '
    .schema == "pointbreak.change-inspector-browser-report" and .version == 1 and
    (.status == "passed" or .status == "failed") and
    (.assertionCount | type == "number") and (.assertionCount >= 0) and
    (.screenshotCount | type == "number") and (.screenshotCount >= 0) and
    (.sectionCount | type == "number") and (.sectionCount > 0) and
    (.globalInvalid | type == "boolean") and
    (.sections | type == "array") and ((.sections | length) == .sectionCount) and
    (.failures | type == "array")
  ' "$browser_result" >/dev/null || die "browser emitted an invalid diagnostic report"
fi
[ -s "$browser_result" ] || die "browser did not emit its diagnostic report"
jq -e '
  .status == "passed" and .globalInvalid == false and
  (.failures | length == 0) and
  (.sections | all(.status == "passed" and .failureCount == 0))
' "$browser_result" >/dev/null \
  || {
    if [ -z "$shakedown_root" ]; then
      jq -r '
        .failures[]? |
        "[\(.section)] \(.label): \(.detail)\n  expected=\(.expected | tojson) actual=\(.actual | tojson)\n  route=\(.route) screenshot=\(.screenshot)"
      ' "$browser_result" >&2
    fi
    die "browser diagnostic report did not pass"
  }
case "$mode" in
  shakedown)
    expected_shakedown_section="Shakedown exact reading and quiet polling"
    ;;
  shakedown-timeline-boundary)
    expected_shakedown_section="Shakedown Timeline boundary and quiet polling"
    ;;
  shakedown-exact-history-focus)
    expected_shakedown_section="Shakedown exact history and focus"
    ;;
  shakedown-return-destinations)
    expected_shakedown_section=""
    ;;
  full)
    expected_shakedown_section=""
    ;;
esac
	# POINTBREAK_D78_PROOF_VALIDATION_BEGIN
	repair_base_proof_occurrences="$(awk '
	  {
	    line = $0
	    while (match(line, /"repairBaseProof"[[:space:]]*:/)) {
	      count += 1
	      line = substr(line, RSTART + RLENGTH)
	    }
	  }
	  END { print count + 0 }
	' "$browser_result")"
	if [ "$mode" = "shakedown" ]; then
	  [ "$repair_base_proof_occurrences" -eq 1 ] \
	    || die "shakedown browser report did not contain exactly one repair-base proof"
	  repair_base_proof_json="$(jq -cer '
	    .repairBaseProof as $proof |
	    if $proof == {
	      schema: "pointbreak.change-inspector-d77-repair-base-proof",
	      version: 1,
	      action: {
	        actionClass: "exact-detail-changes-goto",
	        navigationKind: "goto",
	        resultKind: "same-document-null",
	        sourceDiffersFromTarget: true,
	        eventCount: 2,
	        eventOrdinals: [1, 2],
	        eventPhases: ["invoked", "settled"],
	        capturedFrameMatches: [true, true],
	        targetMatches: [true, true],
	        actionCurrent: [true, true],
	        visitStatesBefore: ["pending", "pending"],
	        visitStatesAfter: ["pending", "pending"],
	        routeObservedBefore: [false, true],
	        routeObservedAfter: [true, true],
	        ownershipBefore: [true, false],
	        ownershipAfter: [false, false]
	      },
	      transition: {
	        mainFrameNavigationRequestCount: 0,
	        navigationRootCount: 0,
	        domContentLoadedCount: 0,
	        rootCommitCount: 0,
	        generationUnchanged: true,
	        overflowed: false
	      },
	      certification: {
	        passed: true,
	        sameVisit: true,
	        pendingState: true,
	        changesHash: true,
	        semanticMatchesIntended: true,
	        pageMatchesSemantic: true,
	        exactReadingReady: true,
	        routeObserved: true
	      },
	      reload: {
	        passed: true,
	        navigationKind: "reload",
	        d77Eligible: false
	      },
	      health: {
	        lifecycleFailureCount: 0,
	        unexpectedRequestFailureCount: 0,
	        admissibleProfileSupersessionCount: 0,
	        profileSupersessionAdmissionWithinBound: true
	      }
	    } then $proof
	    else error("repair-base proof did not match the closed D78 contract")
	    end
	  ' "$browser_result")" \
	    || die "shakedown browser report contained an invalid repair-base proof"
	else
	  [ "$repair_base_proof_occurrences" -eq 0 ] \
	    || die "$mode browser report unexpectedly contained a repair-base proof"
	  jq -e 'has("repairBaseProof") | not' "$browser_result" >/dev/null \
	    || die "$mode browser report unexpectedly contained a repair-base proof"
	  repair_base_proof_json='null'
	fi
	# POINTBREAK_D78_PROOF_VALIDATION_END
if [ "$mode" != "full" ]; then
  if [ "$mode" = "shakedown-return-destinations" ]; then
    jq -e '
      .status == "passed" and .globalInvalid == false and
      .sectionCount == 4 and .screenshotCount == 4 and
      (.failures | length == 0) and
      (.sections == [
        {name: "Shakedown retained Timeline return", status: "passed", failureCount: 0},
        {name: "Shakedown Changes terminal return", status: "passed", failureCount: 0},
        {name: "Shakedown parallel-current exact history", status: "passed", failureCount: 0},
        {name: "Shakedown poll supersession request accounting", status: "passed", failureCount: 0}
      ])
    ' "$browser_result" >/dev/null \
      || die "$mode did not complete its four ordered browser sections"
  else
    jq -e --arg expectedSection "$expected_shakedown_section" '
      .status == "passed" and .globalInvalid == false and
      .sectionCount == 1 and .screenshotCount == 1 and
      (.failures | length == 0) and
      (.sections == [{name: $expectedSection, status: "passed", failureCount: 0}])
    ' "$browser_result" >/dev/null \
      || die "$mode did not complete its one representative browser section"
  fi
  screenshot_count="$(find "$artifact_dir" -maxdepth 1 -type f -name '*.png' | wc -l | tr -d ' ')"
  if [ "$mode" = "shakedown-return-destinations" ]; then
    [ "$screenshot_count" -eq 4 ] \
      || die "shakedown expected four temporary screenshots, found $screenshot_count"
    screenshot_names="$(find "$artifact_dir" -maxdepth 1 -type f -name '*.png' -exec basename {} \; | LC_ALL=C sort)"
    expected_screenshot_names="$(printf '%s\n' \
      'shakedown-changes-terminal-return.png' \
      'shakedown-parallel-current-exact-history.png' \
      'shakedown-poll-supersession-request-accounting.png' \
      'shakedown-retained-timeline-return.png')"
    [ "$screenshot_names" = "$expected_screenshot_names" ] \
      || die "shakedown screenshots did not match the four exact journey names"
  else
    [ "$screenshot_count" -eq 1 ] \
      || die "shakedown expected one temporary screenshot, found $screenshot_count"
  fi
  assertion_count="$(jq -er '.assertionCount' "$browser_result")"
  shakedown_finished_at="$(date +%s)"
  shakedown_elapsed_seconds="$((shakedown_finished_at - shakedown_started_at))"
  shakedown_storage_kib="$(du -sk "$root" | awk '{print $1}')"
  shakedown_receipt="$(jq -cn \
    --arg mode "$mode" \
    --arg root "$root" \
    --arg sourceCommit "$source_commit" \
    --arg binarySha256 "$binary_sha256" \
    --arg host "$(uname -srm)" \
    --argjson assertionCount "$assertion_count" \
    --argjson screenshotCount "$screenshot_count" \
    --argjson elapsedSeconds "$shakedown_elapsed_seconds" \
    --argjson temporaryStorageKiB "$shakedown_storage_kib" \
    '{gate: "change-inspector-browser-shakedown", mode: $mode, status: "passed",
      root: $root, sourceCommit: $sourceCommit, binarySha256: $binarySha256,
      assertionCount: $assertionCount, screenshotCount: $screenshotCount,
      cost: {host: $host, elapsedSeconds: $elapsedSeconds, sourceBuilds: 0,
        derivedStoreBuilds: 1, inspectorLaunches: 1, browserLaunches: 1,
        temporaryStorageKiB: $temporaryStorageKiB},
      retained: false, cleanup: "removed own temporary root"}')"
	# POINTBREAK_D78_PROOF_COPY_BEGIN
	if [ "$mode" = "shakedown" ]; then
	  shakedown_receipt="$(jq -cn \
	    --argjson receipt "$shakedown_receipt" \
	    --argjson repairBaseProof "$repair_base_proof_json" \
	    '$receipt + {repairBaseProof: $repairBaseProof}')"
	fi
	# POINTBREAK_D78_PROOF_COPY_END
  completed_shakedown_root="$root"
  cleanup strict || die "shakedown browser session or temporary root did not clean up"
  trap - EXIT
  [ ! -e "$completed_shakedown_root" ] \
    || die "shakedown temporary root remained after cleanup"
  printf '%s\n' "$shakedown_receipt"
  exit 0
fi
if wait "$timeline_append_pid"; then
  forget_background_process "$timeline_append_pid"
else
  forget_background_process "$timeline_append_pid"
  die "disposable Timeline append did not complete after the parked screenshot"
fi
test -s "$log_dir/timeline-append.json" \
  || die "disposable Timeline append did not leave its receipt"
jq -e '
  .schema == "pointbreak.change-capture-receipt.v1" and .version == 1 and
  (.changeId | startswith("change:sha256:")) and
  (.revision.revisionId | startswith("rev:sha256:")) and
  (.revision.objectArtifactContentHash | startswith("sha256:"))
' "$log_dir/timeline-append.json" >/dev/null \
  || die "disposable Timeline append did not emit an exact capture receipt"
if rg -q '^### Error' "$log_dir/browser-gate.log"; then
  sed -n '1,240p' "$log_dir/browser-gate.log" >&2
  die "real-browser Change Inspector gate reported an error"
fi

screenshot_count="$(find "$artifact_dir" -maxdepth 1 -type f -name '*.png' | wc -l | tr -d ' ')"
assertion_count="$(jq -er '.assertionCount' "$browser_result")"
reported_screenshot_count="$(jq -er '.screenshotCount' "$browser_result")"
[ "$screenshot_count" -eq "$reported_screenshot_count" ] \
  || die "browser reported $reported_screenshot_count screenshots but preserved $screenshot_count"
[ "$screenshot_count" -ge 12 ] || die "expected at least 12 browser screenshots, found $screenshot_count"
[ "$(shasum -a 256 "$pointbreak_binary" | awk '{print $1}')" = "$binary_sha256" ] \
  || die "executed binary snapshot changed during browser qualification"
[ "$(shasum -a 256 "$browser_program_template" | awk '{print $1}')" = "$template_sha256" ] \
  || die "browser program snapshot changed during qualification"
[ "$(shasum -a 256 "$browser_diagnostics" | awk '{print $1}')" = "$diagnostics_sha256" ] \
  || die "browser diagnostics snapshot changed during qualification"
[ "$(shasum -a 256 "$browser_manifest_publisher" | awk '{print $1}')" = "$publisher_sha256" ] \
  || die "manifest publisher snapshot changed during qualification"
[ "$(shasum -a 256 "$matrix_materializer" | awk '{print $1}')" = "$materializer_sha256" ] \
  || die "fixture materializer snapshot changed during qualification"
[ "$(shasum -a 256 "$snapshot_ready_store/$activation_fixture" | awk '{print $1}')" = "$activation_fixture_sha256" ] \
  || die "activation fixture snapshot changed during qualification"
[ "$(shasum -a 256 "$snapshot_ready_store/$completion_fixture" | awk '{print $1}')" = "$completion_fixture_sha256" ] \
  || die "completion fixture snapshot changed during qualification"
[ "$(shasum -a 256 "$script_dir/change-inspector-browser-verify.sh" | awk '{print $1}')" = "$shell_sha256" ] \
  || die "browser verifier source changed during qualification"
tool_versions="$(jq -n \
  --arg git "$(git --version)" \
  --arg node "$(node --version)" \
  --arg playwright "$(run_pw --version 2>&1 | tr '\n' ' ')" \
  --slurpfile pointbreak "$log_dir/pointbreak-version.json" \
  '{git: $git, node: $node, playwright: $playwright, pointbreak: $pointbreak[0]}')"

# The completion marker must follow browser shutdown and every child log
# flush. Run the normally trap-owned cleanup explicitly, reap each child, then
# disarm the trap so no evidence file can be written after manifest.json.
cleanup strict || die "browser session did not close cleanly"
trap - EXIT

# The temporary file may be incomplete if serialization fails. Only the final
# atomic rename publishes manifest.json, so its presence remains the completion
# marker for fixture, browser, screenshot, identity, and cleanup verification.
manifest_tmp="$root/.manifest.json.tmp"
[ "$(shasum -a 256 "$log_dir/harness-digests.json" | awk '{print $1}')" = "$harness_record_sha256" ] \
  || die "browser harness digest record changed during qualification"
for required_evidence_path in \
  logs/browser-empty-ready-l2-retry.json \
  logs/browser-empty-ready-l2-ready.json \
  logs/browser-primary-derived-access-status.json \
  logs/browser-stage-start.json \
  logs/browser-stage-heartbeat.log \
  logs/browser-stage-terminal.json \
  logs/browser-result.json \
  logs/browser-gate.log \
  logs/browser-program.mjs; do
  [ -f "$root/$required_evidence_path" ] \
    || die "required browser evidence is missing: $required_evidence_path"
done
evidence_inventory="$({
  for evidence_path in "$artifact_dir"/*.png; do
    [ -f "$evidence_path" ] || continue
    printf 'browser-artifacts/%s\n' "${evidence_path##*/}"
  done
  for evidence_path in "$log_dir"/browser-*; do
    [ -f "$evidence_path" ] || continue
    case "$evidence_path" in
      *.json | *.log | *.mjs) printf 'logs/%s\n' "${evidence_path##*/}" ;;
    esac
  done
} | LC_ALL=C sort | while IFS= read -r evidence_path; do
  evidence_sha256="$(shasum -a 256 "$root/$evidence_path" | awk '{print $1}')"
  jq -cn --arg path "$evidence_path" --arg sha256 "$evidence_sha256" \
    '{path: $path, sha256: $sha256}'
done | jq -s '.')"
jq -n \
  --arg sourceCommit "$source_commit" \
  --arg binary "$requested_binary" \
  --arg executedBinary "$pointbreak_binary" \
  --arg binarySha256 "$binary_sha256" \
  --arg root "$root" \
  --arg fixture "$fixture_identity" \
  --argjson fixtureData "$(cat "$log_dir/fixture.json")" \
  --argjson timelineAppend "$(cat "$log_dir/timeline-append.json")" \
  --argjson toolVersions "$tool_versions" \
  --slurpfile harness "$log_dir/harness-digests.json" \
  --slurpfile primaryDerivedAccess "$log_dir/browser-primary-derived-access-status.json" \
  --arg harnessSha256 "$harness_record_sha256" \
  --argjson assertionCount "$assertion_count" \
  --argjson screenshotCount "$screenshot_count" \
  --argjson evidenceInventory "$evidence_inventory" \
  '{gate: "change-inspector-browser-verify", status: "passed", sourceCommit: $sourceCommit,
    binary: $binary, executedBinary: $executedBinary, binarySha256: $binarySha256,
    harness: $harness[0], harnessSha256: $harnessSha256, root: $root, fixture: $fixture,
    fixtureData: $fixtureData, timelineAppend: $timelineAppend,
    primaryDerivedAccessStatus: $primaryDerivedAccess[0],
    toolVersions: $toolVersions, assertionCount: $assertionCount,
    screenshotCount: $screenshotCount, evidenceInventory: $evidenceInventory}' \
  >"$manifest_tmp"
[ "$(shasum -a 256 "$browser_manifest_publisher" | awk '{print $1}')" = "$publisher_sha256" ] \
  || die "manifest publisher snapshot changed before completion publication"
node "$browser_manifest_publisher" "$manifest_tmp" "$root/manifest.json" "$browser_result"
cat "$root/manifest.json"
