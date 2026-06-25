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
  local body="$1" out="$2" group="$3" title="$4" with_fonts="${5:-}" theme="${6:-}" name="${7:-}" subtitle="${8:-}"
  # The light cards are identical snapshots with data-theme="light" on <html>; the
  # tokens.css [data-theme="light"] aliases do the re-theming, no JS toggle needed.
  local html_tag='<html lang="en">'
  if [ -n "$theme" ]; then html_tag="<html lang=\"en\" data-theme=\"$theme\">"; fi
  # @dsCard marker: always group; paired (dark + light twin) cards add name/subtitle so
  # the Design pane shows each theme beside its counterpart under one distinct label.
  local marker="<!-- @dsCard group=\"$group\""
  if [ -n "$name" ]; then marker="$marker name=\"$name\""; fi
  if [ -n "$subtitle" ]; then marker="$marker subtitle=\"$subtitle\""; fi
  marker="$marker -->"
  mkdir -p "$(dirname "$DS/$out")"
  {
    printf '%s\n' "$marker"
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

# Dark cards. The five with a light twin carry an explicit name so each theme pair
# reads cleanly in the Design pane; the three un-paired cards keep a group-only marker.
bake foundations.body.html         foundations/foundations.html Foundations "Foundations — tokens"                with-fonts "" "Foundations"
bake navigation-topbar.body.html   navigation/topbar.html      Navigation "Navigation — top bar, tabs, stats"
bake inputs-controls.body.html     inputs/controls.html        Inputs     "Inputs — toolbar, buttons, toggles"
bake data-timeline.body.html       data/timeline.html          Data       "Data — timeline & detail pane"        "" "" "Timeline"
bake data-cards.body.html          data/cards.html             Data       "Data — unit & revision-thread cards"  "" "" "Revision thread"
bake data-review-facts.body.html   data/review-facts.html      Data       "Data — verdict, facts, endorsements"  "" "" "Review facts"
bake data-diff.body.html           data/diff.html              Data       "Data — annotated diff"                "" "" "Annotated diff"
bake feedback-diagnostics.body.html feedback/diagnostics.html  Feedback   "Feedback — diagnostics & errors"

# Light-theme variants — paired beside their dark twin in the SAME group (not a
# separate one), each carrying a "— light" name + "Light theme" subtitle so the pair
# is unambiguous even where the pane shows only the name. The AA-tuned tokens.css
# [data-theme="light"] aliases re-theme everything; no JS toggle. The foundations light
# card keeps the @font-face opt-in (5th arg) for its mono ramp; the rest stay zero-webfont.
bake foundations.body.html         foundations/foundations-light.html Foundations "Foundations — light theme"     with-fonts light "Foundations — light"     "Light theme"
bake data-timeline.body.html       data/timeline-light.html           Data        "Data — timeline, light theme"  "" light "Timeline — light"        "Light theme"
bake data-cards.body.html          data/cards-light.html              Data        "Data — revision thread, light" "" light "Revision thread — light" "Light theme"
bake data-review-facts.body.html   data/review-facts-light.html       Data        "Data — review facts, light"    "" light "Review facts — light"    "Light theme"
bake data-diff.body.html           data/diff-light.html               Data        "Data — annotated diff, light"  "" light "Annotated diff — light"  "Light theme"
