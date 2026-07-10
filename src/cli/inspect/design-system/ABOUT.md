# About Pointbreak Review / shore inspect — context for this design system

Read this before generating or reviewing designs with these tokens. It
explains what the product is and the vocabulary the UI speaks.

## The product

**Pointbreak Review** is a durable, local-first **review record** for code changes
that humans and coding agents build together — used in the inner dev loop,
long before a pull request opens. Coding agents generate more activity than
anyone can read; instead of storing transcripts, Pointbreak Review keeps only the
facts that move a review forward — what changed and why, the open questions,
each assessment — as an append-only log. Every fact is attributed to the
actor that asserted it (human or agent) and can be signed (Ed25519): a
reader can tell *merely signed* from *bound to a trusted identity*.

**This design system covers `shore inspect`**, the local web inspector over
that record: a filterable event timeline with per-actor tracks, revision
pages, annotated diffs, and signature-trust badges. Dense, terminal-adjacent,
information-first — the product shows facts with attribution, not chatter.

Pointbreak is the overall product brand. The Review qualifier matters in this
surface because Pointbreak also covers debugging collaboration tools.

## Design language

- **Dark-first, light-real.** `:root` is the dark theme;
  `[data-theme="light"]` is a full semantic-alias override. Status hues are
  WCAG AA contrast-checked against the surfaces they actually render on, in
  both themes by the product-local `contrast-check.mjs`, which parses the live
  `../assets/tokens.css` source. Don't introduce colors casually.
- **Harbor surfaces** (`--bg → --bg-elev → --bg-row → --bg-row-sel`): an
  ocean-navy wash — `--bg` (`#0a1929`) is shared with the marketing chrome —
  quiet enough that the status hues still carry all the meaning. The **accent**
  is sky-blue (`--accent`, re-pointing to ocean-primary in light for AA); the
  logo's **wave ramp** (`--wave-*`) is reserved for identity moments, not
  working UI.
- **The status system is the identity**: success/warning/danger/assess/
  validation/info/teal; one hue per concept. Event types get their own
  palette (`--evt-*`) that color-codes timeline rails, filter toggles, and
  labels. Diff surfaces have dedicated add/del/emphasis tokens, and syntax
  highlighting (`--tok-*`) aliases the semantic hues.
- **Non-color redundancy**: non-positive states (failed, stale, open,
  superseded, errored, skipped) always carry a glyph (✕ ! ? ~ ○) via CSS —
  meaning never rides on hue alone. Head-vs-superseded in the revision DAG
  reads as solid-vs-dashed stroke, not color.
- **Density**: comfortable by default; `.compact` tightens rhythm tokens
  (`--row-pad`, `--line`) with no component changes.
- **Type**: a dense register — 11/12/13/14px body steps plus one 19px
  heading anchor. Mono-heavy (identifiers, timestamps, chips, diffs). The
  product ships **zero webfonts** (system mono stack); only this gallery
  self-hosts JetBrains Mono so cards render consistently.

The gallery's multiband logo and self-hosted fonts are locally vendored brand
inputs pinned by `pointbreak-brand.lock.json`. `brand-check.mjs` verifies their
bytes and logo geometry offline. The live inspector continues to use its compact
mono logo and system font stack.

The gallery also contains a temporary `instrument-neutral` comparison variant.
It is not shipped, served, or a new product status system; it changes only the
surface, text, border, and accent aliases used to study visual alignment with
the accepted marketing system. Operational status, event, diff, syntax,
density, typography, and compact chrome remain the current Review system.

## Vocabulary the UI speaks

Use surface words in any generated copy: **change, review, revision,
observation, question / input request, assessment, accepted / needs changes /
needs clarification, signed by, track, actor, head, superseded.** Avoid
internal substrate vocabulary: "work objects," "supersession DAG,"
"projections" — these never appear in user-facing UI.

Tone: precise, calm, instrument-like.
