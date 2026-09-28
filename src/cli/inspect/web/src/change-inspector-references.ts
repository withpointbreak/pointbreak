/**
 * Activation for bare reference ids inside rendered prose.
 *
 * The retained Markdown renderer linkifies every recognized id as a chip. The
 * active composition does not register the legacy document-level delegate;
 * instead each rendered body passes through `bindReferenceChips` with the
 * server-supplied targets of the response it came from. A fact id routes only
 * to the event the server named as its recording event; every other chip
 * loses its clickable affordance and reads as plain text.
 */

import type { ChangeInspectorRoute } from "./change-inspector-router";
import { referenceTargetRoute } from "./change-inspector-router";
import type { ChangeRevisionDetail } from "./change-protocol";

type NavigableRoute = Exclude<ChangeInspectorRoute, { kind: "invalid" }>;

/** Fact id → recording event id, exactly as the exact response carries it. */
export function recordingEventTargets(
  facts: ChangeRevisionDetail["factPresentations"],
): Map<string, string> {
  const targets = new Map<string, string>();
  for (const fact of facts) {
    if (fact.recordingEventId !== undefined)
      targets.set(fact.factId, fact.recordingEventId);
  }
  return targets;
}

/**
 * Give reference chips inside one rendered body an activation path. A chip
 * whose id the server resolved becomes a native button that opens the
 * recording event in the Timeline; it never carries `data-fact-id`. Each
 * resolved chip owns its listener, so no document-level delegate exists.
 */
export function bindReferenceChips(
  container: HTMLElement,
  targets: ReadonlyMap<string, string>,
  from: ChangeInspectorRoute,
  navigate: (route: NavigableRoute) => void,
): void {
  for (const chip of container.querySelectorAll<HTMLElement>(
    "[data-ref-kind]",
  )) {
    const referenceId = chip.dataset.refId ?? "";
    const route = referenceTargetRoute(referenceId, targets, from);
    if (route === null) {
      chip.removeAttribute("role");
      chip.removeAttribute("tabindex");
      chip.removeAttribute("data-ref-kind");
      continue;
    }
    const button = document.createElement("button");
    button.type = "button";
    button.className = chip.className;
    button.dataset.refKind = chip.dataset.refKind;
    button.dataset.refId = referenceId;
    button.dataset.relationFactId = referenceId;
    button.textContent = chip.textContent;
    button.title = referenceId;
    button.setAttribute(
      "aria-label",
      `Open the Timeline event that recorded ${referenceId}`,
    );
    button.addEventListener("click", (event) => {
      // A surrounding row or card may own its own activation; the chip's
      // route is the only one this click selects.
      event.stopPropagation();
      navigate(route);
    });
    chip.replaceWith(button);
  }
}
