# shore inspector — design system

Source for the `shore-inspector-ds` [Claude Design](https://claude.ai/design) gallery and
the tokenized status palette consumed by the inspector's `../assets/app.css`.

## Layout

| Path | Role |
| --- | --- |
| `styles.css` | Canonical tokenized stylesheet — the source of truth for the palette. |
| `_bodies/*.body.html` | Per-card markup fragments (the authored content of each card). |
| `_bodies/bake.sh` | Bakes self-contained preview cards from a fragment + `styles.css`. |
| `foundations/foundations.html` | Hand-authored Foundations card (carries its own swatch JS). |
| `<group>/<card>.html` | **Generated, git-ignored.** Run the baker to produce them. |

Each baked card is self-contained: the baker prepends the
`<!-- @dsCard group="…" -->` marker the gallery indexes on, inlines `styles.css`,
then appends the body fragment. Cards are grouped as **Foundations, Navigation,
Inputs, Data, Feedback**.

## Workflow

1. Edit `styles.css` and/or a `_bodies/*.body.html` fragment.
2. Regenerate the cards:
   ```sh
   bash _bodies/bake.sh
   ```
3. Sync to claude.ai/design via the DesignSync tool / `/design-sync` skill
   (project `shore-inspector-ds`): `list_files` → `finalize_plan` → `write_files`.

The tokens in `styles.css` mirror the `:root` palette declared in
`../assets/app.css`; keep the two in step when adding or renaming a token.
