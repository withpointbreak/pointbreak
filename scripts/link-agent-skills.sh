#!/usr/bin/env bash
# Symlink repo Agent Skills into project-local or user-level agent skill
# directories. Re-runnable; safe to call after adding new skills or pulling
# updates.

set -euo pipefail

cd "$(dirname "$0")/.."

show_help() {
  cat <<'EOF'
Symlink repo Agent Skills into local agent skill directories.

Usage:
  scripts/link-agent-skills.sh --project <root> <agent>... [--no-extras]
  scripts/link-agent-skills.sh --user <agent>... [--no-extras]
  scripts/link-agent-skills.sh unlink --project <root> <agent>...
  scripts/link-agent-skills.sh unlink --user <agent>...

Agents:
  claude    -> <root>/.claude/skills or ~/.claude/skills
  agents    -> <root>/.agents/skills or ~/.agents/skills
  codex     -> alias for agents; Codex scans .agents/skills
  opencode  -> <root>/.opencode/skills or ~/.opencode/skills
  codex-legacy -> <root>/.codex/skills or ~/.codex/skills

Examples:
  scripts/link-agent-skills.sh --project ../my-app claude agents
  scripts/link-agent-skills.sh --user claude
  scripts/link-agent-skills.sh unlink --project ../my-app claude agents

Use --user intentionally; without it, this script only links into the project
root supplied by --project. Edits to an existing SKILL.md take effect
in-session, but creating a new skill directory still requires restarting the
agent.
EOF
}

die() {
  echo "error: $*" >&2
  exit 2
}

MODE="link"
SCOPE=""
PROJECT_ROOT=""
INCLUDE_EXTRAS=1
AGENTS=()

set_scope() {
  local next_scope="$1"
  if [ -n "$SCOPE" ] && [ "$SCOPE" != "$next_scope" ]; then
    die "pass only one of --project or --user"
  fi
  SCOPE="$next_scope"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    link)
      MODE="link"
      ;;
    unlink)
      MODE="unlink"
      ;;
    --project)
      shift
      [ "$#" -gt 0 ] || die "--project requires a directory"
      set_scope "project"
      PROJECT_ROOT="$1"
      ;;
    --user)
      set_scope "user"
      ;;
    --agent)
      shift
      [ "$#" -gt 0 ] || die "--agent requires a name"
      AGENTS+=("$1")
      ;;
    --no-extras)
      INCLUDE_EXTRAS=0
      ;;
    -h|--help)
      show_help
      exit 0
      ;;
    --)
      shift
      while [ "$#" -gt 0 ]; do
        AGENTS+=("$1")
        shift
      done
      break
      ;;
    --*)
      die "unknown arg: $1"
      ;;
    *)
      AGENTS+=("$1")
      ;;
  esac
  shift
done

[ -n "$SCOPE" ] || die "pass --project <root> or --user"
[ "${#AGENTS[@]}" -gt 0 ] || die "pass one or more agents: claude, codex, opencode, agents"

if [ "$SCOPE" = "project" ]; then
  [ -n "$PROJECT_ROOT" ] || die "--project requires a directory"
  [ -d "$PROJECT_ROOT" ] || die "project root does not exist: $PROJECT_ROOT"
  PROJECT_ROOT=$(cd "$PROJECT_ROOT" && pwd)
fi

TARGETS=()
CLAUDE_TARGETS=()

add_target() {
  local target="$1"
  local existing
  for existing in "${TARGETS[@]+"${TARGETS[@]}"}"; do
    [ "$existing" = "$target" ] && return 0
  done
  TARGETS+=("$target")
}

add_claude_target() {
  local target="$1"
  local existing
  for existing in "${CLAUDE_TARGETS[@]+"${CLAUDE_TARGETS[@]}"}"; do
    [ "$existing" = "$target" ] && return 0
  done
  CLAUDE_TARGETS+=("$target")
}

agent_target() {
  local agent="$1"
  local root=""

  if [ "$SCOPE" = "project" ]; then
    root="$PROJECT_ROOT"
  else
    root="$HOME"
  fi

  case "$agent" in
    claude) echo "$root/.claude/skills" ;;
    agents|shared|codex) echo "$root/.agents/skills" ;;
    opencode) echo "$root/.opencode/skills" ;;
    codex-legacy) echo "$root/.codex/skills" ;;
    *) die "unknown agent '$agent' (expected claude, agents, codex, opencode, or codex-legacy)" ;;
  esac
}

for agent in "${AGENTS[@]}"; do
  agent=$(printf '%s' "$agent" | tr '[:upper:]' '[:lower:]')
  target=$(agent_target "$agent")
  add_target "$target"
  if [ "$agent" = "claude" ]; then
    add_claude_target "$target"
  fi
done

mkdir -p "${TARGETS[@]}"

link_dir() {
  local src_root="$1"
  [ -d "$src_root" ] || return 0
  for skill in "$src_root"/*/; do
    [ -d "$skill" ] || continue
    local name
    name=$(basename "$skill")
    local src
    src=$(cd "$skill" && pwd)
    local target
    for target in "${TARGETS[@]}"; do
      local link="$target/$name"
      if [ "$MODE" = "unlink" ]; then
        if [ -L "$link" ] && [ "$(readlink "$link")" = "$src" ]; then
          rm "$link"
          echo "unlinked  $link"
        fi
      else
        ln -sfn "$src" "$link"
        echo "linked    $link -> $src"
      fi
    done
  done
}

link_dir "$PWD/skills"

if [ "$INCLUDE_EXTRAS" = "1" ] && [ "${#CLAUDE_TARGETS[@]}" -gt 0 ]; then
  # claude-extras/ only goes into Claude skill directories. Linking it into
  # other agents could shadow the canonical version with Claude-only fields.
  ORIG_TARGETS=("${TARGETS[@]}")
  TARGETS=("${CLAUDE_TARGETS[@]}")
  link_dir "$PWD/claude-extras"
  TARGETS=("${ORIG_TARGETS[@]}")
fi

echo
if [ "$MODE" = "link" ]; then
  echo "Done. Edits to existing SKILL.md files are live in-session."
  echo "Creating new skill directories still requires restarting your agent."
else
  echo "Done. Removed symlinks pointing at this repo."
fi
