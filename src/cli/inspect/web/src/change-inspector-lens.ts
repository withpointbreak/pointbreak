/** Shared accessible hierarchy for the three active Inspector lenses. */

import { CLASS } from "./classNames";

export function createLensHeading(
  label: "Timeline" | "Changes" | "Attention",
  metadata: string,
): [HTMLHeadingElement, HTMLParagraphElement] {
  const heading = document.createElement("h1");
  heading.className = CLASS.lensHeading;
  heading.textContent = label;
  const meta = document.createElement("p");
  meta.className = CLASS.lensMeta;
  meta.textContent = metadata;
  return [heading, meta];
}
