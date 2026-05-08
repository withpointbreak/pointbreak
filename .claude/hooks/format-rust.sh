#!/bin/bash
# Format Rust files after Claude edits them.

input=$(cat)
file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')

if [[ "$file_path" == *.rs ]]; then
    cd "$CLAUDE_PROJECT_DIR" && cargo +nightly fmt --all --quiet 2>/dev/null
fi
