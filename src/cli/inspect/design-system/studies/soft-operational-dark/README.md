# Soft operational dark study

This gallery-only study asks whether Pointbreak Review's dark theme can feel
calmer without losing the precision of a dense developer instrument. It does
began without changing the live inspector, canonical gallery cards, or light
theme. After owner approval, the exact candidate values moved into the live
dark token sheet for an uncommitted product trial. The study retains its
pre-trial baseline so the accepted comparison remains reproducible.

## Hypothesis

The current theme creates hierarchy mainly through near-black surfaces, bright
text, and repeated rules. In a dense timeline that can read like a dedicated
high-contrast mode. The candidate instead:

- lifts the canvas and panel ladder into a deep green-charcoal range;
- creates separation through adjacent surface steps;
- quiets primary and secondary text from bright white/gray to warm off-white
  and sage-gray;
- reduces divider prominence; and
- leaves operational colors, syntax, diffs, density, typography, focus, and
  the light theme unchanged.

## Authored deltas

| Role | Current | Candidate |
| --- | --- | --- |
| canvas | `#080c0d` | `#101817` |
| elevated surface | `#0e1416` | `#151f1e` |
| row | `#151e20` | `#1b2725` |
| selected row | `#1b2729` | `#243331` |
| top bar | `#101716` | `#131d1b` |
| selected wash | `#1b2729` | `#243331` |
| code/readout | `#0e1416` | `#121c1a` |
| border | `#354540` | `#2d3d39` |
| primary text | `#f0f5f1` | `#e5ebe7` |
| secondary text | `#a5b2ad` | `#9eaaa5` |

The candidate intentionally does not override the working accent, status/event
colors, diff or syntax colors, density, or type tokens.

## Initial read

The first matched browser pass supports continuing with this candidate:

- Timeline gains the most: the canvas, selected row, detail pane, and readout
  feel layered instead of etched into one black field.
- Attention cards read as calm work surfaces while their cyan labels and amber
  exception remain the scanning anchors.
- Annotated diff keeps its existing semantic and syntax hierarchy; lifting the
  surrounding canvas does not flatten add/delete or intraline emphasis.
- Quieter borders remain visible but stop competing with every row of text.

The composed dark audit passes 69 of 69 gates. The tightest pair is assessment
text on the selected wash at 4.95:1, above the 4.5:1 AA threshold. The browser
matrix produced no console errors. This is enough evidence to consider a live
token trial, but not an automatic promotion decision.

## Run the study

```sh
node src/cli/inspect/design-system/studies/soft-operational-dark/audit.mjs
bash src/cli/inspect/design-system/studies/soft-operational-dark/bake.sh
```

The audit composes the ten overrides over the live dark token map and reuses
the canonical audit implementation. The bake writes ignored, self-contained
baseline/candidate pairs to `output/` for Foundations, Navigation, Timeline,
Attention, Review facts, and Annotated diff. Open `output/index.html` for the
responsive side-by-side comparison.

Nothing under this directory is part of the canonical Claude Design sync set.
Promotion, if selected, requires an explicit live-token decision and a full
inspector/browser validation pass.
