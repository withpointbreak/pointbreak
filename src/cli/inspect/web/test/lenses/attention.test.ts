import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AttentionDoc } from "../../src/store";
import attentionJson from "../fixtures/attention.json";
import { mountInspectorDom, resetDom } from "../support/dom";

// `lenses/attention.ts` paints the attention lens (`renderAttention`, the
// `#attention` body): tiered cards over the outstanding review state. Each card
// carries the `unit-card` class + `data-revision-id` (so the `#master` delegate
// selects the anchored revision) and a kind-qualified `data-entry-id` (the
// keyboard cursor key). The store and the lens share one `state`, so reset the
// registry and re-import before each test.
type Store = typeof import("../../src/store");
type Attention = typeof import("../../src/lenses/attention");
let store: Store;
let attention: Attention;

beforeEach(async () => {
  vi.resetModules();
  store = await import("../../src/store");
  attention = await import("../../src/lenses/attention");
  mountInspectorDom();
  history.replaceState(null, "", "/");
});

afterEach(() => {
  resetDom();
});

function mountAttentionBody(): void {
  const master = document.querySelector("#master");
  if (master) master.innerHTML = `<div id="attention" class="units"></div>`;
}

function seed(doc: AttentionDoc): void {
  store.commit({ attention: doc });
  mountAttentionBody();
  attention.renderAttention();
}

describe("renderAttention", () => {
  it("renders one card per item with entry and revision anchors", () => {
    seed(attentionJson as unknown as AttentionDoc);
    const cards = document.querySelectorAll(".attention-card");
    expect(cards.length).toBe(attentionJson.items.length);
    for (const card of Array.from(cards)) {
      expect(card.getAttribute("data-entry-id")).toBeTruthy();
      // Every card resolves to a real revision anchor for activation.
      expect(card.getAttribute("data-revision-id")).toBeTruthy();
      // data-open-diff must not shadow revision selection on the card root.
      expect(card.hasAttribute("data-open-diff")).toBe(false);
    }
  });

  it("groups items under a primary and a secondary tier section", () => {
    seed(attentionJson as unknown as AttentionDoc);
    const html = document.querySelector("#attention")?.innerHTML ?? "";
    expect(html).toContain("Needs input");
    expect(html).toContain("Advisory");
    // The tier headings carry the attention-tier class.
    expect(document.querySelectorAll(".attention-tier").length).toBe(2);
  });

  it("shows the ask, reason, and actor on an open request card", () => {
    seed(attentionJson as unknown as AttentionDoc);
    const html = document.querySelector("#attention")?.innerHTML ?? "";
    expect(html).toContain("Runtime trace required");
    expect(html).toContain("insufficient_evidence");
    expect(html).toContain("open-input-request");
  });

  it("badges superseded anchors with a freshness cue", () => {
    seed(attentionJson as unknown as AttentionDoc);
    const badge = document.querySelector(".attention-freshness");
    expect(badge?.textContent ?? "").toContain("superseded");
  });

  it("competing-heads cards anchor to the smallest head", () => {
    seed(attentionJson as unknown as AttentionDoc);
    const competing = Array.from(
      document.querySelectorAll(".attention-card"),
    ).find((card) =>
      card.getAttribute("data-entry-id")?.startsWith("competing_heads:"),
    );
    expect(competing?.getAttribute("data-revision-id")).toBe(
      "rev:sha256:1111aaaa11111111111111111111111111111111111111111111111111111111",
    );
  });

  it("renders a non-empty empty state, never a blank container", () => {
    seed({ items: [] });
    const el = document.querySelector("#attention");
    expect(el?.textContent?.toLowerCase()).toContain("nothing needs attention");
    expect(document.querySelector(".attention-empty")).not.toBeNull();
  });
});

describe("renderAttention kind-specific command handoffs", () => {
  const doc = () => attentionJson as unknown as AttentionDoc;

  function card(entryIdPrefix: string): HTMLElement {
    const found = Array.from(
      document.querySelectorAll<HTMLElement>(".attention-card"),
    ).find((c) => c.getAttribute("data-entry-id")?.startsWith(entryIdPrefix));
    if (!found) throw new Error(`no card for ${entryIdPrefix}`);
    return found;
  }

  it("an open request card offers the exact respond command with a copy control", () => {
    seed(doc());
    const c = card("open_input_request:");
    const code = c.querySelector("[data-workflow-command]");
    expect(code?.textContent).toBe(
      'pointbreak input-request respond input-request:sha256:aaaa111111111111111111111111111111111111111111111111111111111111 --outcome <approved|rejected|dismissed|superseded|abandoned> --reason "<answer>"',
    );
    expect(c.querySelector("[data-copy-workflow-command]")).not.toBeNull();
  });

  it("an ambiguous assessment card replaces both loaded candidates", () => {
    seed(doc());
    const code = card("ambiguous_assessment:").querySelector(
      "[data-workflow-command]",
    );
    const text = code?.textContent ?? "";
    expect(text).toContain(
      "--replaces assess:sha256:bbbb111111111111111111111111111111111111111111111111111111111111",
    );
    expect(text).toContain(
      "--replaces assess:sha256:cccc111111111111111111111111111111111111111111111111111111111111",
    );
    expect(text).toContain("--track <your-track>");
  });

  it("a competing-heads card names every head and conditions the capture", () => {
    seed(doc());
    const c = card("competing_heads:");
    const text = c.querySelector("[data-workflow-command]")?.textContent ?? "";
    expect(text).toContain(
      "--supersedes rev:sha256:1111aaaa11111111111111111111111111111111111111111111111111111111",
    );
    expect(text).toContain(
      "--supersedes rev:sha256:2222bbbb22222222222222222222222222222222222222222222222222222222",
    );
    expect(c.textContent).toContain("genuinely new content");
  });

  it("a stale assessment card is context only — no command, never a guess", () => {
    seed(doc());
    expect(
      card("stale_assessment:").querySelector("[data-workflow-handoff]"),
    ).toBeNull();
  });

  it("an incomplete item renders its card without any command", () => {
    const base = doc();
    const items = base.items.map((entry) =>
      entry.kind === "open_input_request"
        ? { ...entry, inputRequestId: undefined }
        : entry,
    );
    seed({ ...base, items });
    expect(
      card("open_input_request:").querySelector("[data-workflow-handoff]"),
    ).toBeNull();
  });
});
