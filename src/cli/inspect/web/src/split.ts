// The divider controller: the pointer-capture drag, the double-click reset, and
// the WAI-ARIA window-splitter keyboard contract (ArrowLeft/Right step, Enter
// resets) on the `.split` grid's `.divider`. It is the only post-paint writer of
// the `--split-master` width — every write goes through `prefs.applySplit`, so
// the reader-local pref and the live grid can never disagree. It never touches
// the store: the split width is geometry (localStorage), not view state.

import { $ } from "./dom";
import { applySplit, preferredSplit } from "./prefs";
import { commit } from "./store";

// The master pane's percent range. These bound the drag/step math; range
// enforcement itself lives inside applySplit (which clamps every write).
const MIN_PCT = 25;
const MAX_PCT = 75;

/** The divider's current master percent (aria-valuenow is kept authoritative). */
function currentPct(divider: HTMLElement): number {
  const aria = Number(divider.getAttribute("aria-valuenow"));
  if (Number.isFinite(aria) && aria >= MIN_PCT && aria <= MAX_PCT) return aria;
  return preferredSplit() ?? 50;
}

/** Apply a width through the single writer; null resets to the 50/50 default. */
function setPct(divider: HTMLElement, pct: number | null): void {
  const clamped =
    pct === null ? null : Math.round(Math.min(MAX_PCT, Math.max(MIN_PCT, pct)));
  applySplit(clamped);
  divider.setAttribute("aria-valuenow", String(clamped ?? 50));
}

/** Percent step ≈ 24px of the split's width; a fixed 3% when layout yields no width. */
function stepPct(split: HTMLElement): number {
  const w = split.getBoundingClientRect().width;
  return w > 0 ? (24 / w) * 100 : 3;
}

/**
 * Wire the divider's drag + keyboard handlers — once, on the static `.divider`
 * (called by the composition root beside the other init calls). Handled keys
 * stop propagating: the document-level `onKey` would otherwise also act on them
 * (Enter → the ladder).
 */
export function initControls(): void {
  const split = $<HTMLElement>(".split");
  const divider = $<HTMLElement>(".divider");
  if (!split || !divider) return;
  divider.setAttribute("aria-valuenow", String(preferredSplit() ?? 50));

  divider.addEventListener("pointerdown", (ev) => {
    ev.preventDefault();
    // preventDefault suppresses native focus-on-mousedown, so focus explicitly:
    // a click must leave the divider keyboard-operable (arrows/Enter).
    divider.focus();
    divider.setPointerCapture?.(ev.pointerId);
    divider.classList.add("dragging");
  });
  divider.addEventListener("pointermove", (ev) => {
    if (!divider.classList.contains("dragging")) return;
    const r = split.getBoundingClientRect();
    if (r.width <= 0) return;
    const pct = ((ev.clientX - r.left) / r.width) * 100;
    // Past the master floor the pane snaps into reading mode instead of
    // squishing: end the drag and collapse the master behind the rail. The
    // stored width is untouched, so restoring returns to the last good ratio.
    if (pct < MIN_PCT * 0.6) {
      divider.classList.remove("dragging");
      divider.releasePointerCapture?.(ev.pointerId);
      commit({ reading: true });
      return;
    }
    setPct(divider, pct);
  });
  divider.addEventListener("pointerup", (ev) => {
    divider.classList.remove("dragging");
    divider.releasePointerCapture?.(ev.pointerId);
  });
  divider.addEventListener("dblclick", () => setPct(divider, null));
  divider.addEventListener("keydown", (ev) => {
    if (ev.key === "ArrowLeft") {
      ev.preventDefault();
      ev.stopPropagation();
      const next = currentPct(divider) - stepPct(split);
      // Stepping past the floor enters reading mode (the drag-snap's keyboard twin).
      if (next < MIN_PCT) commit({ reading: true });
      else setPct(divider, next);
    } else if (ev.key === "ArrowRight") {
      ev.preventDefault();
      ev.stopPropagation();
      setPct(divider, currentPct(divider) + stepPct(split));
    } else if (ev.key === "Enter") {
      ev.preventDefault();
      ev.stopPropagation();
      setPct(divider, null);
    }
  });
}
