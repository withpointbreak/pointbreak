// The attention lens (#attention): tiered cards over the outstanding review
// state the `/api/attention` projection surfaces — open asks, ambiguous
// assessments, competing heads, stale decisions, failed checks, and outstanding
// follow-ups. State-reading + DOM-writing, modeled on `renderRevisionList`.
//
// Every card root carries the `unit-card` class + `data-revision-id`, so the
// `#master` click delegate (render.ts) selects the anchored revision without a
// per-card listener. A kind-qualified `data-entry-id` keys the lens-local
// keyboard cursor (keyboard.ts). `data-open-diff` is deliberately NOT on the root
// — the delegate handles it before revision selection and would shadow it.

import { CLASS } from "../classNames";
import { $ } from "../dom";
import { escapeHtml } from "../escape";
import { fmtDateTime } from "../format";
import { linkify, shortRef } from "../refs";
import type { AttentionItem } from "../store";
import { getState } from "../store";
import { attentionHandoffs, renderWorkflowHandoffs } from "../workflow-handoff";

/**
 * Split attention items into the needs-input (primary) and advisory (secondary)
 * tiers — the one tier predicate, shared with the tab badge so the lens and the
 * badge can never disagree on what counts as needing input.
 */
export function partitionAttentionTiers(items: AttentionItem[]): {
  primary: AttentionItem[];
  secondary: AttentionItem[];
} {
  return {
    primary: items.filter((item) => item.tier !== "secondary"),
    secondary: items.filter((item) => item.tier === "secondary"),
  };
}

/** Paint the attention items into the `#attention` body, grouped by tier. */
export function renderAttention(): void {
  const el = $("#attention");
  if (!el) return;
  const items = getState().attention?.items ?? [];
  if (!items.length) {
    el.innerHTML = `<p class="${CLASS.attentionEmpty}" style="color:var(--fg-dim)">Nothing needs attention in this store.</p>`;
    return;
  }
  const { primary, secondary } = partitionAttentionTiers(items);
  // Re-apply the lens-local cursor from state so it survives this repaint.
  const focus = getState().attentionFocus;
  // The queue's order is fixed (no user sort control anywhere on this lens),
  // so the order is stated as a static label instead.
  el.innerHTML =
    `<div class="${CLASS.attentionOrderLabel}">longest waiting first</div>` +
    renderTier("Needs input", primary, focus) +
    renderTier("Advisory", secondary, focus);
}

/** A tier section: a heading and its cards, or nothing when the tier is empty. */
function renderTier(
  label: string,
  items: AttentionItem[],
  focus: string | null,
): string {
  if (!items.length) return "";
  return `<h3 class="${CLASS.attentionTier}">${escapeHtml(label)} (${items.length})</h3>${items
    .map((item) => renderAttentionCard(item, focus))
    .join("")}`;
}

/** The revision an item activates to: the item's anchor, or the smallest head
 * of a thread-scoped competing-heads item (its sorted `headRevisionIds[0]`).
 * Shared with the detail page's outstanding block — one anchor rule. */
export function anchorRevision(item: AttentionItem): string {
  if (item.revisionId) return item.revisionId;
  return item.headRevisionIds?.[0] ?? "";
}

/** One attention card: kind chip, subject short-ref, ask, reason, actor. */
function renderAttentionCard(
  item: AttentionItem,
  focus: string | null,
): string {
  const anchor = anchorRevision(item);
  const focusClass = item.id === focus ? ` ${CLASS.attentionFocus}` : "";
  const kind = escapeHtml(item.kind.replace(/_/g, "-"));
  const subject = anchor ? shortRef(anchor) : "thread";
  const freshness =
    item.freshness?.state === "superseded"
      ? `<span class="${CLASS.attentionFreshness}">superseded${
          item.freshness.supersededBy?.length
            ? ` by ${item.freshness.supersededBy.map((id) => linkify(id)).join(", ")}`
            : ""
        }</span>`
      : "";
  const rows: string[] = [];
  const push = (k: string, v: string, medium = false): void => {
    const tier = medium ? ` class="${CLASS.tierMedium}"` : "";
    rows.push(`<span${tier}>${escapeHtml(k)}</span><b${tier}>${v}</b>`);
  };
  push("subject", escapeHtml(subject));
  for (const [k, v] of detailRows(item))
    push(k, v, k === "reason" || k === "track" || k === "actor");
  push("observed", escapeHtml(fmtDateTime(item.observedAt ?? "")), true);

  // Kind-specific copyable commands, rendered only when the item carries every
  // authoritative field the command needs (workflow-handoff.ts is the gate).
  // The block's clicks are guarded by the #master delegate so command text is
  // selectable and the copy button never doubles as card selection.
  return `<div class="${CLASS.unitCard} ${CLASS.attentionCard}${focusClass}" data-entry-id="${escapeHtml(item.id)}" data-revision-id="${escapeHtml(anchor)}" title="${escapeHtml(item.id)}">
      <h3><span class="${CLASS.attentionKind}">${kind}</span> ${escapeHtml(askLabel(item))}</h3>
      ${freshness}
      <div class="${CLASS.kv}">${rows.join("")}</div>
      ${renderWorkflowHandoffs(attentionHandoffs(item))}
    </div>`;
}

/** The one-line "ask" for an item — what a reader would scan first. Shared
 * with the detail page's outstanding block — one ask vocabulary. */
export function askLabel(item: AttentionItem): string {
  switch (item.kind) {
    case "open_input_request":
      return item.title ?? "open input request";
    case "ambiguous_assessment":
      return `${item.assessments?.length ?? 0} competing assessments`;
    case "competing_heads":
      return `${item.headRevisionIds?.length ?? 0} competing heads`;
    case "stale_assessment":
      return `stale ${item.assessment ?? "assessment"}`;
    case "failed_validation":
      return `${item.checkName ?? "check"} ${item.status ?? "failed"}`;
    case "follow_up_outstanding":
      return "follow-up outstanding";
    default:
      return item.kind.replace(/_/g, "-");
  }
}

/** The escaped key/value rows the card renders below the ask, per kind. */
function detailRows(item: AttentionItem): [string, string][] {
  const actor = item.openedBy ?? item.recordedBy;
  const rows: [string, string][] = [];
  if (item.reasonCode) rows.push(["reason", escapeHtml(item.reasonCode)]);
  if (item.mode) rows.push(["mode", escapeHtml(item.mode)]);
  if (item.trackId) rows.push(["track", escapeHtml(item.trackId)]);
  if (actor) rows.push(["actor", linkify(actor)]);
  if (item.kind === "competing_heads" && item.headRevisionIds) {
    rows.push([
      "heads",
      item.headRevisionIds.map((id) => linkify(id)).join(" "),
    ]);
  }
  if (item.kind === "ambiguous_assessment" && item.assessments) {
    rows.push([
      "assessments",
      item.assessments
        .map(
          (a) =>
            `${escapeHtml(a.assessment ?? "")} (${escapeHtml(a.trackId ?? "")})`,
        )
        .join(", "),
    ]);
  }
  if (item.kind === "failed_validation" && item.exitCode != null) {
    rows.push(["exit", escapeHtml(String(item.exitCode))]);
  }
  if (item.kind === "follow_up_outstanding" && item.openInputRequestIds) {
    rows.push([
      "requests",
      item.openInputRequestIds.map((id) => linkify(id)).join(" "),
    ]);
  }
  return rows;
}
