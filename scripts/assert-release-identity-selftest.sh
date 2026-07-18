#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
assert_identity="$script_dir/assert-release-identity.sh"
temp_dir=$(mktemp -d)
trap 'rm -rf "$temp_dir"' EXIT
binary="$temp_dir/pointbreak"
version=9.8.7
tag=v9.8.7
commit=0123456789abcdef0123456789abcdef01234567

cat >"$binary" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[ "$#" -eq 3 ] && [ "$1" = version ] && [ "$2" = --format ] && [ "$3" = json ]
commit=0123456789abcdef0123456789abcdef01234567
case "${IDENTITY_CASE:-exact}" in
  exact)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","build":{"source":"git","commit":"%s","describe":"v9.8.7","dirty":false},"documents":{"pointbreak.version":1},"diagnostics":[]}\n' "$commit"
    ;;
  additive)
    printf '{"extra":true,"diagnostics":[],"documents":{"pointbreak.version":1,"future":2},"build":{"dirty":false,"describe":"v9.8.7","commit":"%s","source":"git","future":true},"cliVersion":"9.8.7","version":1,"schema":"pointbreak.version"}\n' "$commit"
    ;;
  dirty)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","build":{"source":"git","commit":"%s","describe":"v9.8.7-dirty","dirty":true},"documents":{"pointbreak.version":1},"diagnostics":[]}\n' "$commit"
    ;;
  package)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","build":{"source":"package","commit":null,"describe":"package:9.8.7","dirty":false},"documents":{"pointbreak.version":1},"diagnostics":[]}\n'
    ;;
  wrong-tag)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","build":{"source":"git","commit":"%s","describe":"v9.8.6","dirty":false},"documents":{"pointbreak.version":1},"diagnostics":[]}\n' "$commit"
    ;;
  wrong-version)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.6","build":{"source":"git","commit":"%s","describe":"v9.8.7","dirty":false},"documents":{"pointbreak.version":1},"diagnostics":[]}\n' "$commit"
    ;;
  diagnostics)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","build":{"source":"git","commit":"%s","describe":"v9.8.7","dirty":false},"documents":{"pointbreak.version":1},"diagnostics":[{"message":"bad"}]}\n' "$commit"
    ;;
  short-commit)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","build":{"source":"git","commit":"0123456","describe":"v9.8.7","dirty":false},"documents":{"pointbreak.version":1},"diagnostics":[]}\n'
    ;;
  missing-build)
    printf '{"schema":"pointbreak.version","version":1,"cliVersion":"9.8.7","documents":{"pointbreak.version":1},"diagnostics":[]}\n'
    ;;
  malformed-document)
    printf '{not-json\n'
    ;;
  *) exit 65 ;;
esac
EOF
chmod +x "$binary"

"$assert_identity" "$binary" "$version" "$tag" "$commit" >/dev/null
IDENTITY_CASE=additive "$assert_identity" "$binary" "$version" "$tag" "$commit" >/dev/null

for scenario in dirty package wrong-tag wrong-version diagnostics short-commit missing-build malformed-document; do
  if IDENTITY_CASE="$scenario" "$assert_identity" "$binary" "$version" "$tag" "$commit" \
      >"$temp_dir/$scenario.log" 2>&1; then
    echo "release identity assertion accepted $scenario" >&2
    exit 1
  fi
done

echo "release identity self-test ok"
