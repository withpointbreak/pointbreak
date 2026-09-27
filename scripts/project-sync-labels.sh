#!/usr/bin/env bash
# Pure label decisions for `.github/workflows/project-sync.yml`.
#
# The workflow does every GitHub read and write; this script only decides, so
# the decisions can be tested without the network (`project-sync-labels-selftest.sh`).
# Both subcommands read one JSON document on stdin and print one JSON document.
#
#   fields  stdin: the issue's current label names, e.g. ["priority:P2-backlog", "effort:low"]
#           stdout: {"priority", "effort", "workflow", "problems"}; an empty string clears
#           the board field. Conflicting or missing priority:/effort: labels are reported
#           in `problems` and never resolved by picking one.
#
#   guard   stdin: {"action": "labeled"|"unlabeled", "label", "actor", "at",
#                   "actorType": "User"|"Bot"|..., "role", "labels": [current names],
#                   "history": [{"event": "labeled"|"unlabeled", "label", "actor", "at", "id"}]}
#           `at` is when the event happened (ISO 8601 UTC, as GitHub prints it); the
#           history may lag behind the event and need not contain it. GitHub timestamps
#           have one-second precision, so history is ordered by (at, id): event ids
#           increase over time. The triggering event is located in the history by
#           (label, event, actor, at); when exactly one entry matches, its id orders it
#           against same-second events. When a same-second tie cannot be ordered, the
#           guard does nothing destructive: a tie on the same label counts as stale,
#           and a tie in the namespace removes no label (reconciliation then flags the
#           conflict on the board).
#           stdout: {"planning", "stale", "revert": "remove"|"restore"|null,
#                    "remove": [labels], "comment": string|null, "notes": [strings]}
#           `labels` and `history` are read back from the issue when the run starts, so
#           the decision reflects the current label set rather than the event payload.
set -euo pipefail

usage() {
  echo "usage: $0 fields|guard < input.json" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage

case "$1" in
  fields)
    jq -c '
      . as $labels
      | def has($l): ($labels | index($l)) != null;
        def ns($p): [$labels[] | select(startswith($p))];
        {
          "priority:P0-release-blocker": "P0 blocker",
          "priority:P1-1.0-candidate": "P1 1.0 candidate",
          "priority:P2-backlog": "P2 backlog",
          "priority:P3-later": "P3 later",
          "effort:low": "Low",
          "effort:medium": "Medium",
          "effort:high": "High"
        } as $options
      | (has("tracking")) as $tracking
      | (has("status:needs-triage")) as $untriaged
      # One namespace: exactly one known label maps to its option; zero or
      # several clear the field and are reported, never resolved by order.
      | def field($p):
          ns($p) as $present
          | if ($present | length) == 1 then
              if $options[$present[0]] then {value: $options[$present[0]], problems: []}
              else {value: "", problems: ["unknown label \($present[0])"]}
              end
            elif ($present | length) > 1 then
              {value: "", problems: ["conflicting \($p) labels (\($present | join(", "))); the field is cleared rather than picking one"]}
            elif $tracking or $untriaged then {value: "", problems: []}
            else {value: "", problems: ["no \($p) label"]}
            end;
        field("priority:") as $priority
      | field("effort:") as $effort
      | ($priority.problems + $effort.problems) as $problems
      # Precedence when several apply: an umbrella is Tracking whatever else it
      # carries; a study is Research; a gated item is parked before it is a
      # decision; needs-triage clears the field. An issue that would otherwise
      # be Ready but lacks exactly one priority: and effort: label is not ready:
      # it reads as untriaged (cleared) until the labels are fixed.
      | (if $tracking then "Tracking"
         elif has("research") then "Research"
         elif has("status:demand-gated") then "Demand-gated"
         elif has("status:needs-decision") then "Needs decision"
         elif $untriaged then ""
         elif ($problems | length) > 0 then ""
         else "Ready"
         end) as $workflow
      | {priority: $priority.value, effort: $effort.value, workflow: $workflow, problems: $problems}
    '
    ;;
  guard)
    jq -c '
      def planning: test("^(priority|effort|status):") or . == "research" or . == "tracking";
      def namespace: if test("^(priority|effort):") then sub(":.*$"; ":") else null end;
      . as $in
      | ($in.history // [] | sort_by([.at // "", .id // 0])) as $history
      | ($in.labels | index($in.label) != null) as $present
      | ($in.at // "") as $at
      # Where the triggering event sits in the history: its own entry when exactly
      # one matches, else only its timestamp (`id` null), else nowhere.
      | ([$history[] | select(.label == $in.label and .event == $in.action
          and .actor == $in.actor and (.at // "") == $at and $at != "")]) as $self
      | (if ($self | length) == 1 then {at: $at, id: $self[0].id}
         elif $at != "" then {at: $at, id: null}
         else null
         end) as $point
      # after: definitely later than the triggering event; tie: same second and
      # the order cannot be established.
      | def after($e): $point != null and (
          if $point.id != null then [($e.at // ""), ($e.id // 0)] > [$point.at, $point.id]
          else ($e.at // "") > $point.at
          end);
        def tie($e): $point != null and $point.id == null and ($e.at // "") == $point.at;
      # Label events on the same label that this run did not cause (a different
      # action or actor), after the triggering event or tied with it.
        [$history[] | select(.label == $in.label
          and (.event != $in.action or .actor != $in.actor))] as $others_on_label
      | [$others_on_label[] | select(after(.))] as $later
      | [$others_on_label[] | select(tie(.))] as $tied
      | {planning: ($in.label | planning), stale: false, revert: null, remove: [], comment: null, notes: []}
      | if .planning | not then .notes += ["not a planning label: \($in.label)"]
        # A stale event: the label set already moved on (the label is gone again,
        # or back again), or a later labeled/unlabeled event for the same label
        # exists, or one in the same second that cannot be ordered against it.
        # The run for that other event owns the decision.
        elif ($in.action == "labeled" and ($present | not))
          or ($in.action == "unlabeled" and $present)
          or ($later | length) > 0 or ($tied | length) > 0 then
          .stale = true
          | .notes += ["stale \($in.action) event for \($in.label): the current label set no longer reflects it; nothing to do"]
        # Planning labels are maintainer-owned. Bots (templates, apps) skip the
        # role check but not the namespace rules below.
        elif $in.actorType != "Bot" and ((["admin", "maintain", "write"] | index($in.role)) == null) then
          .revert = (if $in.action == "labeled" then "remove" else "restore" end)
          | .comment = "\(if $in.action == "labeled" then "Reverted" else "Restored" end) `\($in.label)`: planning labels (`priority:*`, `effort:*`, `status:*`, `research`, `tracking`) are set by maintainers with write access. See CONTRIBUTING.md, \"Planning Labels\"."
        elif ($in.label | namespace) == null then .
        elif $in.action == "labeled" then
          ($in.label | namespace) as $ns
          | [$in.labels[] | select(startswith($ns) and . != $in.label)] as $others
          # Keep the label added last. The event label wins only when every other
          # present label in its namespace was last labeled before it; a label
          # labeled after it, or in the same second without an order, removes
          # nothing.
          | def last_labeled($l): [$history[] | select(.label == $l and .event == "labeled")] | last;
            [$others[] | last_labeled(.) as $e | select($e != null and after($e))] as $newer
          | [$others[] | last_labeled(.) as $e | select($e != null and tie($e))] as $undecided
          | if ($others | length) == 0 then .
            elif ($newer | length) > 0 then
              .notes += ["\($newer | join(", ")) was labeled after \($in.label); that event keeps its namespace"]
            elif ($undecided | length) > 0 then
              .notes += ["\($undecided | join(", ")) was labeled in the same second as \($in.label) and the order is unknown; no label removed"]
            else .remove = $others
            end
        else
          # unlabeled: removing the only label of a namespace leaves none. Say so
          # on the issue; the board reads it as untriaged until one is set.
          ($in.label | namespace) as $ns
          | if ([$in.labels[] | select(startswith($ns))] | length) == 0
              and (($in.labels | index("tracking")) == null)
              and (($in.labels | index("status:needs-triage")) == null) then
              .comment = "Removed `\($in.label)`, the only `\($ns)*` label: this issue now has none, so the board treats it as untriaged until exactly one `\($ns)*` label is set. See CONTRIBUTING.md, \"Planning Labels\"."
            else .
            end
        end
    '
    ;;
  *)
    usage
    ;;
esac
