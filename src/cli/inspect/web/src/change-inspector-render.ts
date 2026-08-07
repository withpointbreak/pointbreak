/** DOM projection for the Change-first shell. It fetches nothing and owns no history. */

import { changeCardPresentation } from "./change-inspector-cards";
import type { ChangeInspectorRoute } from "./change-inspector-router";
import {
  lensForRoute,
  parseChangeInspectorRoute,
} from "./change-inspector-router";
import type { ChangeInspectorSnapshot } from "./change-inspector-state";
import type { ChangePageQuery } from "./change-protocol";

export interface ChangeInspectorRenderActions {
  navigate(route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>): void;
}

const FILTER_OPTIONS = [
  [
    "topology",
    [
      "initial",
      "replacement",
      "replacement_divergent",
      "consolidation",
      "parallel_current",
      "mixed",
      "incomplete",
      "cycle_conflicted",
    ],
  ],
  ["lifecycle", ["incomplete", "conflicted", "in_progress", "accepted"]],
  ["attention", ["clear", "in_progress", "incomplete", "conflicted"]],
  ["availability", ["available", "incomplete"]],
] as const;

function message(text: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.className = "empty";
  element.textContent = text;
  return element;
}

function setText(selector: string, value: string): void {
  const element = document.querySelector<HTMLElement>(selector);
  if (element) element.textContent = value;
}

/** Prepare retained model-neutral chrome for the two Change lenses. */
export function prepareChangeInspectorShell(
  actions: ChangeInspectorRenderActions,
): void {
  document.querySelector("#view-controls")?.classList.add("hidden");
  document.querySelector("#derived-access-status")?.classList.add("hidden");
  document.querySelector("#follow-toggle")?.classList.add("hidden");
  const switcher = document.querySelector<HTMLElement>("#lens-switcher");
  if (switcher) {
    switcher.replaceChildren();
    for (const lens of ["changes", "attention"] as const) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "lens-tab";
      button.dataset.lens = lens;
      button.textContent = lens === "changes" ? "Changes" : "Attention";
      button.addEventListener("click", () =>
        actions.navigate({ kind: "lens", lens, query: {} }),
      );
      switcher.append(button);
    }
  }
  const back = document.querySelector<HTMLButtonElement>("#detail-back");
  if (back) {
    back.textContent = "‹ Changes";
    back.onclick = () =>
      actions.navigate({ kind: "lens", lens: "changes", query: {} });
  }
  const search = document.querySelector<HTMLInputElement>("#filter-text");
  if (search) search.placeholder = "Search Changes and current Revisions";
  const filterTypes = document.querySelector<HTMLElement>("#filter-types");
  if (filterTypes) {
    filterTypes.replaceChildren();
    const heading = document.createElement("h2");
    heading.id = "filter-types-label";
    heading.className = "control-heading";
    heading.textContent = "Change status";
    filterTypes.append(heading);
    for (const [name, values] of FILTER_OPTIONS) {
      const label = document.createElement("label");
      label.textContent = name.replaceAll("_", " ");
      const select = document.createElement("select");
      select.id = `change-filter-${name}`;
      for (const [labelText, value] of [
        ["Any", ""],
        ...values.map((value) => [value.replaceAll("_", " "), value]),
      ]) {
        const option = document.createElement("option");
        option.textContent = labelText;
        option.value = value;
        select.append(option);
      }
      select.addEventListener("change", () => {
        const current = parseChangeInspectorRoute(location.hash || "#/changes");
        const base =
          current.kind === "invalid"
            ? { kind: "lens" as const, lens: "changes" as const, query: {} }
            : current;
        actions.navigate({
          ...base,
          query: {
            ...base.query,
            after: undefined,
            [name]: select.value || undefined,
          },
        } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
      });
      label.append(select);
      filterTypes.append(label);
    }
  }
  const clear = document.querySelector<HTMLButtonElement>("#filter-clear");
  if (clear) {
    clear.onclick = () => {
      const current = parseChangeInspectorRoute(location.hash || "#/changes");
      const base =
        current.kind === "invalid"
          ? { kind: "lens" as const, lens: "changes" as const, query: {} }
          : current;
      actions.navigate({
        ...base,
        query: {},
      } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
    };
  }
}

function copyExact(value: string): void {
  if (navigator.clipboard) void navigator.clipboard.writeText(value);
}

function clearError(): void {
  const banner = document.querySelector<HTMLElement>("#error");
  if (!banner) return;
  banner.textContent = "";
  banner.classList.add("hidden");
}

function filterValues(query: ChangePageQuery): Array<[string, string]> {
  const values: Array<[string, string]> = [];
  if (query.q) values.push(["search", query.q]);
  for (const [name] of FILTER_OPTIONS) {
    const value = query[name];
    if (value) values.push([name, value]);
  }
  return values;
}

function syncFilterChrome(route: ChangeInspectorRoute): void {
  if (route.kind === "invalid") return;
  const input = document.querySelector<HTMLInputElement>("#filter-text");
  if (input) input.value = route.query.q ?? "";
  for (const [name] of FILTER_OPTIONS) {
    const select = document.querySelector<HTMLSelectElement>(
      `#change-filter-${name}`,
    );
    if (select) select.value = route.query[name] ?? "";
  }
  const values = filterValues(route.query);
  const chips = document.querySelector<HTMLElement>("#filter-chips");
  if (chips) {
    chips.replaceChildren(
      ...values.map(([name, value]) => {
        const chip = document.createElement("span");
        chip.className = "badge";
        chip.textContent = `${name}: ${value.replaceAll("_", " ")}`;
        return chip;
      }),
    );
  }
  document
    .querySelector<HTMLElement>("#filter-chips-empty")
    ?.classList.toggle("hidden", values.length > 0);
  const toggle = document.querySelector<HTMLElement>("#filters-toggle");
  if (toggle)
    toggle.textContent = values.length
      ? `Filters · ${values.length}`
      : "Filters";
}

function renderDetail(
  snapshot: ChangeInspectorSnapshot,
  actions: ChangeInspectorRenderActions,
): void {
  const detail = document.querySelector<HTMLElement>("#detail-body");
  if (!detail) return;
  if (snapshot.route.kind === "invalid") {
    detail.replaceChildren(message(snapshot.route.message));
    return;
  }
  if (snapshot.diagnostic) {
    detail.replaceChildren(message(snapshot.diagnostic));
    return;
  }
  if (snapshot.route.kind === "lens" || snapshot.generation === null) {
    detail.replaceChildren(message("Select a Change or exact Revision."));
    return;
  }
  const heading = document.createElement("h2");
  heading.textContent =
    snapshot.route.kind === "change" ? "Change" : "Exact Revision";
  const identity = document.createElement("p");
  identity.className = "mono";
  identity.textContent =
    snapshot.route.kind === "change"
      ? `Change ID: ${snapshot.route.changeId}`
      : `Revision ID: ${snapshot.route.revision.revisionId} · artifact hash: ${snapshot.route.revision.objectArtifactContentHash}`;
  const placeholder = message(
    snapshot.route.kind === "change"
      ? "Select an explicit current Revision to inspect its exact context."
      : "Exact Revision selected. Rich facts and captured resources load in the next Inspector slice.",
  );
  const copyLink = document.createElement("button");
  copyLink.type = "button";
  copyLink.className = "ghost";
  copyLink.textContent = "Copy link";
  copyLink.addEventListener("click", () => copyExact(location.href));
  const peers = document.createElement("section");
  if (snapshot.route.kind === "change" && snapshot.selected !== null) {
    const changeRoute = snapshot.route;
    const peerHeading = document.createElement("h3");
    peerHeading.textContent = "Current Revisions";
    peers.append(peerHeading);
    for (const revision of snapshot.selected.currentRevisionRefs) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "ghost mono";
      button.textContent = revision.revisionId;
      button.addEventListener("click", () =>
        actions.navigate({
          kind: "revision",
          changeId: changeRoute.changeId,
          revision,
          query: changeRoute.query,
        }),
      );
      peers.append(button);
    }
  }
  detail.replaceChildren(heading, identity, copyLink, placeholder, peers);
}

/** Paint a single already-published generation and its independently resolved route. */
export function renderChangeInspector(
  snapshot: ChangeInspectorSnapshot,
  actions: ChangeInspectorRenderActions,
): void {
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) return;
  const routeDiagnostic =
    document.querySelector<HTMLElement>("#route-diagnostic");
  if (routeDiagnostic) {
    routeDiagnostic.textContent = snapshot.diagnostic ?? "";
    routeDiagnostic.classList.toggle("hidden", snapshot.diagnostic === null);
  }
  syncFilterChrome(snapshot.route);
  clearError();
  if (snapshot.route.kind === "invalid") {
    master.replaceChildren(message("Cannot open this Inspector link."));
    renderDetail(snapshot, actions);
    return;
  }
  const route = snapshot.route;
  if (snapshot.generation === null) {
    master.replaceChildren(message("Loading Change generation…"));
    renderDetail(snapshot, actions);
    return;
  }
  const lens = lensForRoute(route);
  const page =
    lens === "changes"
      ? snapshot.generation.changes
      : snapshot.generation.attention;
  const list = document.createElement("section");
  list.className = "units";
  const heading = document.createElement("h1");
  heading.textContent = `${lens === "changes" ? "Changes" : "Attention"} · ${page.changes.length}`;
  list.append(heading);
  for (const summary of page.changes) {
    const card = changeCardPresentation(
      summary,
      page.presentations?.[summary.changeId],
    );
    const element = document.createElement("article");
    element.className = "unit-card";
    element.dataset.changeId = summary.changeId;
    const badges = document.createElement("p");
    badges.className = "change-card-badges";
    for (const value of card.badges) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = value;
      badges.append(badge, " ");
    }
    element.append(badges);
    if (card.peers.length === 0) {
      const unavailable = document.createElement("h3");
      unavailable.textContent = "Current Revision unavailable";
      element.append(unavailable);
    } else if (card.peers.length > 1) {
      const peerHeading = document.createElement("h3");
      peerHeading.textContent = "Current Revisions";
      element.append(peerHeading);
    }
    for (const peer of card.peers) {
      const peerRow = document.createElement("div");
      peerRow.className = "change-card-peer";
      const choose = document.createElement("button");
      choose.type = "button";
      choose.className = "ghost change-card-peer-open";
      choose.textContent = peer.label;
      choose.title = peer.copyText;
      choose.addEventListener("click", () =>
        actions.navigate({
          kind: "revision",
          changeId: summary.changeId,
          revision: peer.revision,
          query: route.query,
        }),
      );
      const copyPeer = document.createElement("button");
      copyPeer.type = "button";
      copyPeer.className = "ghost";
      copyPeer.textContent = "Copy exact Revision";
      copyPeer.addEventListener("click", () => copyExact(peer.copyText));
      peerRow.append(choose, copyPeer);
      element.append(peerRow);
    }
    const actionsElement = document.createElement("div");
    actionsElement.className = "actions change-card-actions";
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost";
    open.textContent = "Open Change";
    open.addEventListener("click", () =>
      actions.navigate({
        kind: "change",
        changeId: summary.changeId,
        query: route.query,
      }),
    );
    const changeIdentity = document.createElement("code");
    changeIdentity.className = "mono";
    changeIdentity.textContent = summary.changeId;
    const copyChange = document.createElement("button");
    copyChange.type = "button";
    copyChange.className = "ghost";
    copyChange.textContent = "Copy Change ID";
    copyChange.addEventListener("click", () => copyExact(summary.changeId));
    actionsElement.append(open, changeIdentity, copyChange);
    element.append(actionsElement);
    list.append(element);
  }
  if (page.changes.length === 0)
    list.append(
      message(
        lens === "changes" ? "No Changes." : "No Changes need attention.",
      ),
    );
  const nextPage = page.next;
  if (nextPage !== null) {
    const next = document.createElement("button");
    next.type = "button";
    next.className = "ghost";
    next.textContent = "Next page";
    next.addEventListener("click", () =>
      actions.navigate({
        kind: "lens",
        lens,
        query: {
          ...route.query,
          after: nextPage,
        },
      }),
    );
    list.append(next);
  }
  master.replaceChildren(list);
  document
    .querySelectorAll<HTMLButtonElement>("#lens-switcher [data-lens]")
    .forEach((button) => {
      button.setAttribute("aria-pressed", String(button.dataset.lens === lens));
    });
  setText(
    "#stat-events",
    `${snapshot.generation.profile.authorityCursor.eventCount ?? "—"} events`,
  );
  setText(
    "#stat-units",
    `${snapshot.generation.changes.changes.length} Changes`,
  );
  setText(
    "#stat-threads",
    `${snapshot.generation.attention.changes.length} need attention`,
  );
  setText("#stat-hash", snapshot.generation.changes.projectionStamp);
  renderDetail(snapshot, actions);
}

export function renderChangeInspectorUnavailable(
  availability: "migration_required" | "migration_in_progress",
): void {
  clearError();
  const master = document.querySelector<HTMLElement>("#master");
  master?.replaceChildren(
    message(
      availability === "migration_required"
        ? "Store migration required. No Change state was loaded."
        : "Store migration in progress. Partial Change state is unavailable.",
    ),
  );
  document
    .querySelector<HTMLElement>("#detail-body")
    ?.replaceChildren(message("Change state is unavailable."));
}

export function renderChangeInspectorRefusal(error: unknown): void {
  const text = error instanceof Error ? error.message : String(error);
  document
    .querySelector<HTMLElement>("#master")
    ?.replaceChildren(message(`Reader refused: ${text}`));
  document
    .querySelector<HTMLElement>("#detail-body")
    ?.replaceChildren(message("Change state was not published."));
  const diagnostic = document.querySelector<HTMLElement>("#route-diagnostic");
  if (diagnostic) {
    diagnostic.textContent = "";
    diagnostic.classList.add("hidden");
  }
  const banner = document.querySelector<HTMLElement>("#error");
  if (banner) {
    banner.textContent = `error: ${text}`;
    banner.classList.remove("hidden");
  }
}
