#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
finalizer="$script_dir/finalize-cocogitto-release-tag.sh"

for command in cog git gpg; do
  command -v "$command" >/dev/null || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

test_root=$(mktemp -d)
trap 'rm -rf "$test_root"' EXIT

export GNUPGHOME="$test_root/gnupg"
mkdir -m 700 "$GNUPGHOME"
signing_identity='Pointbreak release hook selftest <release-hook-selftest@withpointbreak.invalid>'
gpg --batch --pinentry-mode loopback --passphrase '' \
  --quick-generate-key "$signing_identity" rsa2048 sign 0 >/dev/null 2>&1
signing_key=$(gpg --batch --with-colons --list-secret-keys "$signing_identity" \
  | awk -F: '$1 == "fpr" { print $10; exit }')
[ -n "$signing_key" ]

remote="$test_root/remote.git"
repo="$test_root/repo"
git init --bare --quiet "$remote"
git init --quiet --initial-branch=main "$repo"
git -C "$repo" config user.name 'Pointbreak release hook selftest'
git -C "$repo" config user.email 'release-hook-selftest@withpointbreak.invalid'
git -C "$repo" config user.signingkey "$signing_key"
git -C "$repo" config commit.gpgsign true
git -C "$repo" config tag.gpgsign true
git -C "$repo" config gpg.format openpgp
git -C "$repo" config gpg.program "$(command -v gpg)"
git -C "$repo" remote add origin "$remote"

cat >"$repo/cog.toml" <<EOF
from_latest_tag = true
disable_changelog = true
disable_bump_commit = false
tag_prefix = "v"
scopes = []

pre_bump_hooks = []
post_bump_hooks = []

[bump_profiles.ci]
pre_bump_hooks = [
    "printf 'release\\n' >> payload",
]
post_bump_hooks = [
    "git commit --amend -m 'chore: v{{version}}'",
    "$finalizer v{{version}}",
    "git push origin HEAD:main",
    "git push origin refs/tags/v{{version}}",
]

[commit_types]
feat = { changelog_title = "Features", bump_minor = true, order = 1 }
chore = { changelog_title = "Miscellaneous Chores", omit_from_changelog = true, order = 2 }
EOF

printf 'bootstrap\n' >"$repo/payload"
git -C "$repo" add cog.toml payload
git -C "$repo" commit --quiet -S -m 'chore: bootstrap'
git -C "$repo" tag -s -m v0.7.0 v0.7.0

printf 'source\n' >>"$repo/payload"
git -C "$repo" add payload
git -C "$repo" commit --quiet -S -m 'feat: source'
source_commit=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" push --quiet --set-upstream origin main
git -C "$repo" push --quiet origin refs/tags/v0.7.0

(cd "$repo" && cog bump --version 0.8.0 --hook-profile ci >/dev/null)
release_commit=$(git -C "$repo" rev-parse HEAD)
cog_tag_commit=$(git -C "$repo" rev-parse 'HEAD@{1}')

[ "$(git -C "$repo" rev-parse HEAD^)" = "$source_commit" ]
[ "$(git -C "$repo" cat-file -t refs/tags/v0.8.0)" = tag ]
[ "$(git -C "$repo" rev-parse 'refs/tags/v0.8.0^{}')" = "$release_commit" ]
git -C "$repo" verify-commit "$release_commit" >/dev/null
git -C "$repo" verify-tag v0.8.0 >/dev/null
[ "$(git --git-dir="$remote" rev-parse refs/heads/main)" = "$release_commit" ]
[ "$(git --git-dir="$remote" rev-parse 'refs/tags/v0.8.0^{}')" = "$release_commit" ]

# A remote tag collision must fail before the verified local lightweight ref is
# removed or replaced.
git -C "$repo" update-ref -d refs/tags/v0.8.0
git -C "$repo" update-ref refs/tags/v0.8.0 "$cog_tag_commit"
if (
  cd "$repo"
  "$finalizer" v0.8.0
) >/dev/null 2>&1; then
  echo "tag finalizer accepted an existing remote release tag" >&2
  exit 1
fi
[ "$(git -C "$repo" rev-parse refs/tags/v0.8.0)" = "$cog_tag_commit" ]

echo "Cocogitto release tag finalizer selftest ok"
