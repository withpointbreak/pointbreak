import { describe, expect, it } from "vitest";
import type { ThreadLayout } from "../src/model";
import { renderSupersessionSvg } from "../src/supersession";

const laid: ThreadLayout = {
  nodes: [
    {
      id: "obs:a",
      x: 40,
      y: 20,
      w: 60,
      h: 24,
      isHead: false,
      isSuperseded: true,
    },
    {
      id: "obs:b",
      x: 40,
      y: 70,
      w: 60,
      h: 24,
      isHead: true,
      isSuperseded: false,
    },
  ],
  edges: [
    {
      from: "obs:b",
      to: "obs:a",
      path: [
        [40, 58],
        [40, 32],
      ],
      kind: "supersedes",
    },
  ],
  bounds: { w: 120, h: 100 },
};

describe("renderSupersessionSvg", () => {
  it("paints non-interactive fact nodes keyed on the given id attribute", () => {
    const html = renderSupersessionSvg(laid, {
      idAttr: "data-fact-id",
      ariaNoun: "observation",
      interactive: false,
      isSelected: () => false,
    });
    const doc = new DOMParser().parseFromString(html, "text/html");
    const nodes = doc.querySelectorAll("g.dag-node");
    expect(nodes.length).toBe(2);
    // Keyed on data-fact-id, NOT data-revision-id.
    expect(
      doc.querySelector('g.dag-node[data-fact-id="obs:b"]'),
    ).not.toBeNull();
    expect(doc.querySelector("g.dag-node[data-revision-id]")).toBeNull();
    // Non-interactive: no tabindex / role=link.
    for (const n of Array.from(nodes)) {
      expect(n.getAttribute("tabindex")).toBeNull();
      expect(n.getAttribute("role")).toBeNull();
    }
    // Head / superseded classes come through from the layout.
    expect(doc.querySelectorAll("g.dag-node.head").length).toBe(1);
    expect(doc.querySelectorAll("g.dag-node.superseded").length).toBe(1);
    // The shared revision-dag stylesheet root (inherits --dag-edge etc).
    expect(doc.querySelector("svg.revision-dag")).not.toBeNull();
  });

  it("returns '' for an empty layout", () => {
    expect(
      renderSupersessionSvg(
        { nodes: [], edges: [], bounds: { w: 0, h: 0 } },
        {
          idAttr: "data-fact-id",
          ariaNoun: "observation",
          interactive: false,
          isSelected: () => false,
        },
      ),
    ).toBe("");
  });
});
