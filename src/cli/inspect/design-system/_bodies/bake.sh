#!/usr/bin/env bash
# Bake self-contained design-system preview cards: marker + inlined styles.css + body.
# Re-run after editing styles.css or any *.body.html fragment.
set -euo pipefail
DS="$(cd "$(dirname "$0")/.." && pwd)"
TOKENS="$DS/../assets/tokens.css"
STYLES="$DS/styles.css"

# Publish a token file for the synced design-system project: the shared product
# tokens plus the self-hosted JetBrains Mono @font-face block (fonts.css). The
# project's styles.css @imports this, so the compiler resolves both the tokens
# and the font faces. The baked cards still inline only the product tokens
# ($TOKENS, system --mono stack, no webfont), so the inspector and the cards stay
# zero-webfont; this concatenated copy is solely the project's bindable layer.
# Gitignored — regenerated here, never committed.
cat "$TOKENS" "$DS/fonts.css" > "$DS/tokens.css"
echo "published tokens.css (+ @font-face)"

bake() {
  local body="$1" out="$2" group="$3" title="$4" with_fonts="${5:-}" theme="${6:-}"
  # The light cards are identical snapshots with data-theme="light" on <html>; the
  # tokens.css [data-theme="light"] aliases do the re-theming, no JS toggle needed.
  local html_tag='<html lang="en">'
  if [ -n "$theme" ]; then html_tag="<html lang=\"en\" data-theme=\"$theme\">"; fi
  mkdir -p "$(dirname "$DS/$out")"
  {
    printf '<!-- @dsCard group="%s" -->\n' "$group"
    printf '<!doctype html>\n%s\n  <head>\n    <meta charset="utf-8" />\n' "$html_tag"
    printf '    <title>%s</title>\n    <style>\n' "$title"
    cat "$TOKENS"
    cat "$STYLES"
    # Cards that demo the self-hosted font opt in (5th arg): inline the @font-face
    # faces, path-rewritten one dir up since cards live in a subdirectory.
    if [ -n "$with_fonts" ]; then sed 's#url("fonts/#url("../fonts/#g' "$DS/fonts.css"; fi
    printf '    </style>\n  </head>\n  <body>\n'
    cat "$DS/_bodies/$body"
    printf '  </body>\n</html>\n'
  } > "$DS/$out"
  echo "baked $out"
}

bake foundations.body.html         foundations/foundations.html Foundations "Foundations — tokens" with-fonts
bake navigation-topbar.body.html   navigation/topbar.html      Navigation "Navigation — top bar, tabs, stats"
bake inputs-controls.body.html     inputs/controls.html        Inputs     "Inputs — toolbar, buttons, toggles"
bake data-timeline.body.html       data/timeline.html          Data       "Data — timeline & detail pane"
bake data-cards.body.html          data/cards.html             Data       "Data — unit & revision-thread cards"
bake data-review-facts.body.html   data/review-facts.html      Data       "Data — verdict, facts, endorsements"
bake data-diff.body.html           data/diff.html              Data       "Data — annotated diff"
bake feedback-diagnostics.body.html feedback/diagnostics.html  Feedback   "Feedback — diagnostics & errors"

# Light-theme variants — the AA-tuned [data-theme="light"] palette is invisible in
# the otherwise dark-only gallery, so these showcase it on the cards where it reads
# most: the full token swatches plus the data-dense surfaces. Same bodies, re-themed
# by the tokens alone. The foundations light card keeps the @font-face opt-in (5th arg)
# so its JetBrains Mono ramp still renders; the rest stay zero-webfont.
bake foundations.body.html         foundations/foundations-light.html "Light theme" "Foundations — light theme"      with-fonts light
bake data-timeline.body.html       data/timeline-light.html           "Light theme" "Data — timeline, light theme"   "" light
bake data-cards.body.html          data/cards-light.html              "Light theme" "Data — revision thread, light"  "" light
bake data-review-facts.body.html   data/review-facts-light.html       "Light theme" "Data — review facts, light"     "" light
bake data-diff.body.html           data/diff-light.html               "Light theme" "Data — annotated diff, light"   "" light
