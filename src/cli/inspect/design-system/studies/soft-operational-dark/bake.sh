#!/usr/bin/env bash
set -euo pipefail

STUDY="$(cd "$(dirname "$0")" && pwd)"
DS="$(cd "$STUDY/../.." && pwd)"
TOKENS="$DS/../assets/tokens.css"
STYLES="$DS/styles.css"
OUT="$STUDY/output"

node "$DS/brand-check.mjs"
node "$DS/contrast-check.mjs" >/dev/null
node "$STUDY/audit.mjs" >/dev/null

mkdir -p "$OUT" "$DS/logo"
cp "$DS/../assets/pointbreak-logo-mono.svg" "$DS/logo/pointbreak-logo-mono.svg"

bake() {
  local body="$1" out="$2" title="$3" tone="${4:-}"
  local tone_attribute=""
  if [ -n "$tone" ]; then tone_attribute=" data-tone=\"$tone\""; fi

  {
    printf '<!doctype html>\n<html lang="en" data-theme="dark"%s>\n  <head>\n' "$tone_attribute"
    printf '    <meta charset="utf-8" />\n    <meta name="viewport" content="width=device-width, initial-scale=1" />\n'
    printf '    <title>%s</title>\n    <style>\n' "$title"
    cat "$TOKENS"
    sed 's#url("logo/#url("../../../logo/#g' "$STYLES"
    if [ -n "$tone" ]; then cat "$STUDY/tokens.css"; fi
    printf '    </style>\n  </head>\n  <body>\n'
    printf '    <div class="ds-card" data-theme="dark"%s>\n' "$tone_attribute"
    cat "$DS/_bodies/$body"
    printf '    </div>\n  </body>\n</html>\n'
  } > "$OUT/$out"
  echo "baked $out"
}

pair() {
  local body="$1" name="$2" title="$3"
  bake "$body" "$name-baseline.html" "$title — pre-trial dark" study-baseline
  bake "$body" "$name-soft.html" "$title — soft operational dark" soft-operational
}

pair foundations.body.html       foundations "Foundations"
pair navigation-topbar.body.html navigation  "Navigation"
pair data-timeline.body.html     timeline    "Timeline"
pair data-attention.body.html    attention   "Attention"
pair data-review-facts.body.html review-facts "Review facts"
pair data-diff.body.html         diff        "Annotated diff"

{
  printf '<!doctype html>\n<html lang="en"><head><meta charset="utf-8" />\n'
  printf '<meta name="viewport" content="width=device-width, initial-scale=1" />\n'
  printf '<title>Soft operational dark comparison</title>\n<style>\n'
  printf ':root { color-scheme: dark; font-family: system-ui, sans-serif; background: #080c0d; color: #e5ebe7; }\n'
  printf 'body { margin: 0; padding: 24px; } h1 { margin: 0 0 8px; font-size: 24px; } p { color: #a5b2ad; margin: 0 0 24px; }\n'
  printf '.pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; margin-bottom: 28px; }\n'
  printf '.frame h2 { font-size: 13px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; color: #a5b2ad; }\n'
  printf 'iframe { width: 100%%; height: 720px; border: 1px solid #2d3d39; border-radius: 8px; background: #080c0d; }\n'
  printf '@media (max-width: 900px) { .pair { grid-template-columns: 1fr; } }\n'
  printf '</style></head><body>\n<h1>Soft operational dark</h1>\n'
  printf '<p>Pre-trial dark on the left; soft-operational live trial on the right. Light theme and operational colors are held constant.</p>\n'
  for name in foundations navigation timeline attention review-facts diff; do
    printf '<section class="pair"><div class="frame"><h2>%s — pre-trial</h2><iframe title="%s pre-trial dark" src="%s-baseline.html"></iframe></div>' "$name" "$name" "$name"
    printf '<div class="frame"><h2>%s — candidate</h2><iframe title="%s soft operational dark" src="%s-soft.html"></iframe></div></section>\n' "$name" "$name" "$name"
  done
  printf '</body></html>\n'
} > "$OUT/index.html"

echo "study output: $OUT"
