#!/usr/bin/env bash
# Behavioral tests for `project-sync-labels.sh`, the decisions behind
# `.github/workflows/project-sync.yml`. Hermetic: no network, no files written.
# Run through `just workflow-lint`.
# The jq assertions quote backticks literally, so single quotes are intended.
# shellcheck disable=SC2016
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
decide="$script_dir/project-sync-labels.sh"
failures=0

# expect <name> <subcommand> <input json> <jq assertion over the output>
expect() {
  local name="$1" subcommand="$2" input="$3" assertion="$4" output
  output="$(printf '%s' "$input" | "$decide" "$subcommand")"
  if jq -e "$assertion" <<<"$output" >/dev/null; then
    echo "ok   $name"
  else
    echo "FAIL $name" >&2
    echo "     input:  $input" >&2
    echo "     output: $output" >&2
    echo "     expect: $assertion" >&2
    failures=$((failures + 1))
  fi
}

# guard_input <action> <label> <actor> <actor type> <role> <labels json> <history json> [event at]
# The event time defaults to 2026-09-01T00:00:00Z, the first history entry of most cases.
guard_input() {
  jq -cn --arg action "$1" --arg label "$2" --arg actor "$3" --arg type "$4" --arg role "$5" \
    --argjson labels "$6" --argjson history "$7" --arg at "${8:-2026-09-01T00:00:00Z}" \
    '{action: $action, label: $label, actor: $actor, at: $at, actorType: $type, role: $role, labels: $labels, history: $history}'
}

no_action='.stale == false and .revert == null and .remove == [] and .comment == null'

# --- fields: board values from the current label set -------------------------

expect "ready work item maps both fields" fields \
  '["priority:P2-backlog", "effort:low", "cli"]' \
  '. == {priority: "P2 backlog", effort: "Low", workflow: "Ready", problems: []}'

expect "untriaged issue clears every field without problems" fields \
  '["status:needs-triage", "bug"]' \
  '. == {priority: "", effort: "", workflow: "", problems: []}'

expect "untriaged issue with a draft priority is still not ready" fields \
  '["status:needs-triage", "priority:P2-backlog", "effort:low"]' \
  '.workflow == "" and .priority == "P2 backlog" and .problems == []'

expect "zero labels is not ready and names both gaps" fields \
  '[]' \
  '.workflow == "" and .priority == "" and .effort == ""
   and (.problems | length) == 2
   and (.problems | any(test("no priority:"))) and (.problems | any(test("no effort:")))'

expect "missing effort alone keeps the item out of Ready" fields \
  '["priority:P1-1.0-candidate"]' \
  '.workflow == "" and .priority == "P1 1.0 candidate" and .problems == ["no effort: label"]'

expect "conflicting priority labels are flagged, not resolved" fields \
  '["priority:P2-backlog", "priority:P1-1.0-candidate", "effort:low"]' \
  '.priority == "" and .workflow == "" and .effort == "Low"
   and (.problems | length) == 1
   and (.problems[0] | test("conflicting priority:") and test("P2-backlog") and test("P1-1.0-candidate"))'

expect "conflict handling does not depend on label order" fields \
  '["effort:low", "priority:P1-1.0-candidate", "priority:P2-backlog"]' \
  '.priority == "" and .workflow == "" and (.problems[0] | test("conflicting priority:"))'

expect "conflicting effort on a gated item keeps the gate and flags the conflict" fields \
  '["status:demand-gated", "priority:P3-later", "effort:low", "effort:high"]' \
  '.workflow == "Demand-gated" and .effort == "" and .priority == "P3 later"
   and (.problems | length) == 1 and (.problems[0] | test("conflicting effort:"))'

expect "tracking parents carry no priority or effort and report nothing" fields \
  '["tracking"]' \
  '. == {priority: "", effort: "", workflow: "Tracking", problems: []}'

expect "an unknown namespace label is reported, not mapped" fields \
  '["priority:P9-someday", "effort:low"]' \
  '.priority == "" and .workflow == "" and .problems == ["unknown label priority:P9-someday"]'

expect "workflow precedence: research before needs-decision" fields \
  '["research", "status:needs-decision", "priority:P2-backlog", "effort:medium"]' \
  '.workflow == "Research" and .problems == []'

# --- guard: event decisions against the re-read label set --------------------

expect "writer adds a second priority: the label added last wins" guard \
  "$(guard_input labeled priority:P1-1.0-candidate alice User admin \
    '["priority:P2-backlog", "priority:P1-1.0-candidate", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"labeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-02T00:00:00Z","id":2}]' 2026-09-02T00:00:00Z)" \
  '.stale == false and .revert == null and .remove == ["priority:P2-backlog"] and .comment == null'

expect "bot label events are normalized too" guard \
  "$(guard_input labeled effort:high triage-app[bot] Bot none \
    '["priority:P2-backlog", "effort:low", "effort:high"]' \
    '[{"event":"labeled","label":"effort:low","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"labeled","label":"effort:high","actor":"triage-app[bot]","at":"2026-09-02T00:00:00Z","id":2}]' 2026-09-02T00:00:00Z)" \
  '.stale == false and .revert == null and .remove == ["effort:low"]'

expect "bots skip the role check" guard \
  "$(guard_input labeled status:needs-triage template-app[bot] Bot none \
    '["status:needs-triage", "bug"]' \
    '[{"event":"labeled","label":"status:needs-triage","actor":"template-app[bot]","at":"2026-09-01T00:00:00Z","id":1}]')" \
  "$no_action"

expect "a non-writer's planning label is reverted without normalizing" guard \
  "$(guard_input labeled priority:P0-release-blocker mallory User triage \
    '["priority:P2-backlog", "priority:P0-release-blocker", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"labeled","label":"priority:P0-release-blocker","actor":"mallory","at":"2026-09-02T00:00:00Z","id":2}]' 2026-09-02T00:00:00Z)" \
  '.revert == "remove" and .remove == [] and (.comment | test("^Reverted `priority:P0-release-blocker`"))'

expect "a non-writer's removal is restored" guard \
  "$(guard_input unlabeled effort:low mallory User read \
    '["priority:P2-backlog"]' \
    '[{"event":"labeled","label":"effort:low","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"unlabeled","label":"effort:low","actor":"mallory","at":"2026-09-02T00:00:00Z","id":2}]' 2026-09-02T00:00:00Z)" \
  '.revert == "restore" and (.comment | test("^Restored `effort:low`"))'

expect "stale labeled event: the label is already gone" guard \
  "$(guard_input labeled priority:P1-1.0-candidate alice User admin \
    '["priority:P2-backlog", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"unlabeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-01T00:00:05Z","id":2}]')" \
  '.stale == true and .revert == null and .remove == [] and .comment == null'

expect "stale unlabeled event: the label is back" guard \
  "$(guard_input unlabeled priority:P2-backlog mallory User none \
    '["priority:P2-backlog", "effort:low"]' \
    '[{"event":"unlabeled","label":"priority:P2-backlog","actor":"mallory","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"labeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:05Z","id":2}]')" \
  '.stale == true and .revert == null and .comment == null'

expect "stale event: a later actor touched the same label" guard \
  "$(guard_input labeled priority:P1-1.0-candidate mallory User none \
    '["priority:P1-1.0-candidate", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P1-1.0-candidate","actor":"mallory","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"unlabeled","label":"priority:P1-1.0-candidate","actor":"mallory","at":"2026-09-01T00:00:01Z","id":2},
      {"event":"labeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-01T00:00:02Z","id":3}]')" \
  '.stale == true and .revert == null'

expect "an older event never removes a label added after it" guard \
  "$(guard_input labeled priority:P1-1.0-candidate alice User admin \
    '["priority:P1-1.0-candidate", "priority:P2-backlog", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"labeled","label":"priority:P2-backlog","actor":"bob","at":"2026-09-01T00:00:03Z","id":2}]')" \
  '.stale == false and .remove == [] and (.notes | any(test("priority:P2-backlog was labeled after")))'

expect "timestamps, not payload order, decide which label is newer" guard \
  "$(guard_input labeled priority:P1-1.0-candidate alice User admin \
    '["priority:P1-1.0-candidate", "priority:P2-backlog", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-02T00:00:00Z","id":2},
      {"event":"labeled","label":"priority:P2-backlog","actor":"bob","at":"2026-09-01T00:00:00Z","id":1}]')" \
  '.remove == ["priority:P2-backlog"]'

expect "history that lags behind the event still lets the event label win" guard \
  "$(guard_input labeled priority:P1-1.0-candidate alice User admin \
    '["priority:P2-backlog", "priority:P1-1.0-candidate", "effort:low"]' \
    '[{"event":"labeled","label":"priority:P2-backlog","actor":"bob","at":"2026-09-02T00:00:00Z","id":1}]' \
    2026-09-03T00:00:00Z)" \
  '.stale == false and .remove == ["priority:P2-backlog"]'

expect "removing the only priority label says so on the issue" guard \
  "$(guard_input unlabeled priority:P2-backlog alice User maintain \
    '["effort:low", "cli"]' \
    '[{"event":"unlabeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:00Z","id":1}]')" \
  '.stale == false and .revert == null and .remove == []
   and (.comment | test("only `priority:\\*` label") and test("untriaged"))'

expect "removing the only effort label on a bot event says so too" guard \
  "$(guard_input unlabeled effort:medium triage-app[bot] Bot none \
    '["priority:P2-backlog"]' \
    '[{"event":"unlabeled","label":"effort:medium","actor":"triage-app[bot]","at":"2026-09-01T00:00:00Z","id":1}]')" \
  '.revert == null and (.comment | test("only `effort:\\*` label"))'

expect "a swap that leaves one priority label needs no comment" guard \
  "$(guard_input unlabeled priority:P2-backlog alice User write \
    '["priority:P1-1.0-candidate", "effort:low"]' \
    '[{"event":"unlabeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:00Z","id":1},
      {"event":"labeled","label":"priority:P1-1.0-candidate","actor":"alice","at":"2026-09-01T00:00:02Z","id":2}]')" \
  "$no_action"

expect "clearing priority on an untriaged issue needs no comment" guard \
  "$(guard_input unlabeled priority:P2-backlog alice User admin \
    '["status:needs-triage"]' \
    '[{"event":"unlabeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:00Z","id":1}]')" \
  "$no_action"

expect "clearing priority on a tracking parent needs no comment" guard \
  "$(guard_input unlabeled priority:P2-backlog alice User admin \
    '["tracking"]' \
    '[{"event":"unlabeled","label":"priority:P2-backlog","actor":"alice","at":"2026-09-01T00:00:00Z","id":1}]')" \
  "$no_action"

expect "a writer's status label needs nothing" guard \
  "$(guard_input labeled status:demand-gated alice User admin \
    '["status:demand-gated", "priority:P3-later", "effort:low"]' \
    '[{"event":"labeled","label":"status:demand-gated","actor":"alice","at":"2026-09-01T00:00:00Z","id":1}]')" \
  "$no_action"

expect "descriptive labels are outside the guard" guard \
  "$(guard_input labeled bug mallory User none '["bug"]' \
    '[{"event":"labeled","label":"bug","actor":"mallory","at":"2026-09-01T00:00:00Z","id":1}]')" \
  ".planning == false and $no_action"

expect "missing history falls back to the event label winning" guard \
  "$(guard_input labeled effort:high alice User admin \
    '["effort:low", "effort:high"]' '[]')" \
  '.remove == ["effort:low"]'

if [ "$failures" -ne 0 ]; then
  echo "project-sync-labels selftest: $failures failure(s)" >&2
  exit 1
fi
echo "project-sync-labels selftest ok"
