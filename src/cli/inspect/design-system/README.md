# shore inspector — design system

Source for the `shore-inspector-ds` [Claude Design](https://claude.ai/design) gallery and
the tokenized status palette consumed by the inspector's `../assets/app.css`.

The gallery is a shared tokens component/state preview for critical inspector surfaces; it is not a full live-app mirror.
It keeps representative static states for status/readback/diff/shell/feedback
review, while runtime behavior such as routing, localStorage, copy-to-clipboard, and lazy rendering
stays in the live inspector.

## Layout

| Path | Role |
| --- | --- |
| `../assets/tokens.css` | The single source of truth for the palette (the only `:root`). |
| `styles.css` | Component rules only — references the tokens via `var(--…)`. |
| `_bodies/*.body.html` | Per-card markup fragments (the authored content of each card). |
| `<group>/<card>.html` | **Generated, git-ignored.** Run the baker to produce them. |
| `_bodies/bake.sh` | Bakes self-contained preview cards from a fragment + the tokens + `styles.css`. |

Each baked card is self-contained: the baker prepends the
`<!-- @dsCard group="…" -->` marker the gallery indexes on, inlines
`../assets/tokens.css` then `styles.css`, then appends the body fragment. Cards
are grouped as **Foundations, Navigation, Inputs, Data, Feedback**. The
Foundations card (`_bodies/foundations.body.html`) carries its own swatch JS and
reads the inlined token values via `getComputedStyle`.

## Workflow

1. Edit `styles.css` and/or a `_bodies/*.body.html` fragment.
2. Regenerate the cards:
   ```sh
   bash _bodies/bake.sh
   ```
3. Sync to claude.ai/design via the DesignSync tool / `/design-sync` skill
   (project `shore-inspector-ds`): `list_files` → `finalize_plan` → `write_files`.

The palette is single-sourced in `../assets/tokens.css` (the served frontend's
only `:root`); `bake.sh` inlines that same file into every card, so the gallery
and the live inspector resolve the same shared tokens. Add or rename a token in
`tokens.css` and re-bake.
