import {
  bootstrapCapability,
  installDefaultAuthCoordinator,
  requestReconnect,
} from "./auth";
import {
  type AttentionPage,
  type Availability,
  buildChangePageUrl,
  type ChangePresentation,
  type ChangeSummary,
  type ChangesPage,
  decodeChangeDetail,
  decodeChangePage,
  decodeChangeRevisionDetail,
  decodeReaderProfile,
  decodeRevisionInterdiff,
  decodeRevisionResource,
  MAX_LIVE_CHANGE_ROWS,
  type ReaderProfile,
  type RevisionRef,
  requireCoherentGeneration,
  sameProfileGeneration,
} from "./change-protocol";
import {
  configureConnectionActions,
  initConnectionControls,
} from "./connection";
import { ChangePageFailure, fetchJSON } from "./http";

let pollTimer: ReturnType<typeof setInterval> | null = null;
let readerEpoch = 0;
let detailSelectionEpoch = 0;
let connectionControlsInitialized = false;
interface VisibleGeneration {
  profile: ReaderProfile;
  changes: ChangesPage;
  attention: AttentionPage;
}

interface DetailRequest {
  readerEpoch: number;
  selectionEpoch: number;
  projectionStamp: string;
  visible: VisibleGeneration;
}

let visibleGeneration: VisibleGeneration | null = null;

export interface ChangeBootstrapOptions {
  poll?: boolean;
  reload?: () => void;
}

/**
 * Start the supported Change reader. Profile validation is the first network
 * boundary and completes before any semantic request or paint. Every refresh
 * stages the list and attention documents together, validates their shared
 * projection stamp, and only then replaces the visible generation.
 */
export async function bootstrapChangeReader(
  options: ChangeBootstrapOptions = {},
): Promise<void> {
  stopChangeReader();
  const bootstrapEpoch = readerEpoch;
  let loadingGeneration = false;
  const capability = bootstrapCapability();
  if (capability.token !== null) {
    (options.reload ?? (() => location.reload()))();
    return;
  }
  installDefaultAuthCoordinator();
  const retry = () => bootstrapChangeReader(options);
  configureConnectionActions({
    retry,
    reconnect: async () => {
      if (await requestReconnect()) await retry();
    },
  });
  if (!connectionControlsInitialized) {
    initConnectionControls();
    connectionControlsInitialized = true;
  }
  try {
    const profile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
    if (bootstrapEpoch !== readerEpoch) return;
    prepareChangeShell();
    if (profile.availability !== "ready") {
      renderUnavailable(profile.availability);
      return;
    }
    loadingGeneration = true;
    const publishedEpoch = await loadGeneration(profile);
    if (
      options.poll !== false &&
      publishedEpoch !== null &&
      publishedEpoch === readerEpoch
    ) {
      pollTimer = setInterval(() => {
        void refresh();
      }, 3000);
    }
  } catch (error) {
    if (!loadingGeneration && bootstrapEpoch !== readerEpoch) return;
    renderRefusal(error);
  }
}

export function stopChangeReader(): void {
  readerEpoch += 1;
  detailSelectionEpoch += 1;
  visibleGeneration = null;
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function refresh(force = false): Promise<void> {
  const requestedEpoch = readerEpoch;
  const current = visibleGeneration;
  let loadingGeneration = false;
  try {
    const profile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
    if (requestedEpoch !== readerEpoch || current !== visibleGeneration) {
      return;
    }
    if (profile.availability !== "ready") {
      renderUnavailable(profile.availability);
      stopChangeReader();
      return;
    }
    if (
      !force &&
      current !== null &&
      sameProfileGeneration(current.profile, profile)
    ) {
      return;
    }
    loadingGeneration = true;
    await loadGeneration(profile);
  } catch (error) {
    if (
      !loadingGeneration &&
      (requestedEpoch !== readerEpoch || current !== visibleGeneration)
    ) {
      return;
    }
    renderRefusal(error);
  }
}

async function loadGeneration(
  profile: ReaderProfile,
  restarted = false,
): Promise<number | null> {
  const requestedEpoch = ++readerEpoch;
  detailSelectionEpoch += 1;
  try {
    const [changes, attention] = await Promise.all([
      fetchJSON(buildChangePageUrl("changes")).then((page) =>
        decodeChangePage(page, { lens: "changes", bounded: true }),
      ),
      fetchJSON(buildChangePageUrl("attention")).then((page) =>
        decodeChangePage(page, { lens: "attention", bounded: true }),
      ),
    ]);
    const postflight = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
    if (requestedEpoch !== readerEpoch) return null;
    requireCoherentGeneration(changes, attention);
    if (!sameProfileGeneration(profile, postflight)) {
      throw new Error("Change generation changed during staging");
    }
    renderGeneration(profile, changes, attention);
    return requestedEpoch;
  } catch (error) {
    if (requestedEpoch !== readerEpoch) return null;
    const stalePage =
      error instanceof ChangePageFailure && error.code === "stale_projection";
    const changedDuringStaging =
      error instanceof Error &&
      error.message === "Change generation changed during staging";
    if (!restarted && (stalePage || changedDuringStaging)) {
      renderRestart(error);
      let retryProfile: ReaderProfile;
      try {
        retryProfile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
      } catch (retryError) {
        if (requestedEpoch !== readerEpoch) return null;
        throw retryError;
      }
      if (requestedEpoch !== readerEpoch) return null;
      if (retryProfile.availability !== "ready") {
        renderUnavailable(retryProfile.availability);
        return null;
      }
      return loadGeneration(retryProfile, true);
    }
    throw error;
  }
}

function prepareChangeShell(): void {
  document.querySelector("#toolbar")?.classList.add("hidden");
  document.querySelector("#view-controls")?.classList.add("hidden");
  document.querySelector("#derived-access-status")?.classList.add("hidden");
  const switcher = document.querySelector<HTMLElement>("#lens-switcher");
  if (switcher) {
    switcher.replaceChildren();
    const label = document.createElement("strong");
    label.textContent = "Changes";
    switcher.append(label);
  }
  const master = document.querySelector<HTMLElement>("#master");
  master?.setAttribute("aria-label", "Changes");
  const detail = document.querySelector<HTMLElement>("#detail-body");
  if (detail) {
    detail.replaceChildren(message("Select a Change or exact Revision."));
  }
}

function renderUnavailable(availability: Exclude<Availability, "ready">): void {
  clearSemanticPresentation();
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) return;
  master.replaceChildren(
    message(
      availability === "migration_required"
        ? "Store migration required. No Change state was loaded."
        : "Store migration in progress. Partial Change state is unavailable.",
    ),
  );
}

function renderGeneration(
  profile: ReaderProfile,
  page: ChangesPage,
  attention: AttentionPage,
): void {
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) return;
  // `#master` is the flex shell for one active lens body. Keep the complete
  // Change list inside its scrollable body: appending every card directly to
  // the shell would turn a populated list into hundreds of competing flex
  // children whose content can overlap when they shrink.
  const list = document.createElement("section");
  list.className = "units";
  const heading = document.createElement("h1");
  heading.textContent = `Changes · ${page.changes.length}`;
  list.append(heading);
  for (const change of page.changes) {
    const card = document.createElement("article");
    card.dataset.changeId = change.changeId;
    card.className = "unit-card";
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost mono";
    open.textContent = change.changeId;
    open.addEventListener("click", () => {
      void loadChangeDetail(change);
    });
    card.append(open);
    card.append(
      line(`topology: ${words(change.topology)}`),
      line(`lifecycle: ${words(change.lifecycle)}`),
      line(`attention: ${words(change.attentionSummary)}`),
      line(`availability: ${words(change.availabilitySummary)}`),
    );
    const revisions = document.createElement("div");
    revisions.className = "change-current-revisions";
    const presentation = page.presentations?.[change.changeId];
    for (const revision of change.currentRevisionRefs) {
      const select = document.createElement("button");
      select.type = "button";
      select.className = "ghost mono";
      select.dataset.revisionId = revision.revisionId;
      select.textContent = currentRevisionLabel(revision, presentation);
      select.addEventListener("click", () => {
        void loadRevisionDetail(change, revision);
      });
      revisions.append(select);
    }
    card.append(revisions);
    if (change.currentRevisionRefs.length > 1) {
      const compare = document.createElement("button");
      compare.type = "button";
      compare.className = "ghost";
      compare.textContent = "Compare exact Revisions";
      compare.addEventListener("click", () => {
        void loadInterdiff(
          change,
          change.currentRevisionRefs[0],
          change.currentRevisionRefs[1],
        );
      });
      card.append(compare);
    }
    list.append(card);
  }
  if (page.changes.length === 0) list.append(message("No Changes."));
  if (page.next !== null) {
    const loadMore = document.createElement("button");
    loadMore.type = "button";
    loadMore.className = "ghost";
    loadMore.textContent = "Load more Changes";
    loadMore.addEventListener("click", () => {
      void loadMoreChanges();
    });
    list.append(loadMore);
  }
  master.replaceChildren(list);
  setText(
    "#stat-events",
    `${profile.authorityCursor.eventCount ?? "—"} events`,
  );
  setText("#stat-units", `${page.changes.length} Changes`);
  setText("#stat-threads", `${attention.changes.length} need attention`);
  setText("#stat-hash", page.projectionStamp);
  detailSelectionEpoch += 1;
  visibleGeneration = { profile, changes: page, attention };
}

/**
 * A continuation remains opaque to the browser. It replaces the oldest rows
 * once the bounded live window is full, then validates the same postflight
 * capability/freshness state before a single repaint.
 */
async function loadMoreChanges(): Promise<void> {
  const current = visibleGeneration;
  if (!current?.changes.next) return;
  const requestedEpoch = readerEpoch;
  try {
    const next = decodeChangePage(
      await fetchJSON(
        buildChangePageUrl("changes", { after: current.changes.next }),
      ),
      { lens: "changes", bounded: true },
    );
    if (!isLiveGeneration(requestedEpoch, current)) return;
    const postflight = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
    if (!isLiveGeneration(requestedEpoch, current)) return;
    if (
      next.projectionStamp !== current.changes.projectionStamp ||
      !sameProfileGeneration(current.profile, postflight)
    ) {
      throw new Error(
        "Change page changed during paging; restarting from first page",
      );
    }
    const merged = mergeChangePages(current.changes, next);
    if (!isLiveGeneration(requestedEpoch, current)) return;
    renderGeneration(current.profile, merged, current.attention);
  } catch (error) {
    if (!isLiveGeneration(requestedEpoch, current)) return;
    if (
      (error instanceof ChangePageFailure &&
        error.code === "stale_projection") ||
      (error instanceof Error &&
        error.message ===
          "Change page changed during paging; restarting from first page")
    ) {
      renderRestart(error);
      await refresh(true);
      return;
    }
    renderRefusal(error);
  }
}

function isLiveGeneration(
  requestedEpoch: number,
  expected: VisibleGeneration,
): boolean {
  return requestedEpoch === readerEpoch && visibleGeneration === expected;
}

function beginDetailRequest(change: ChangeSummary): DetailRequest | null {
  const visible = visibleGeneration;
  if (visible === null || !visible.changes.changes.includes(change))
    return null;
  return {
    readerEpoch,
    selectionEpoch: ++detailSelectionEpoch,
    projectionStamp: change.projectionStamp,
    visible,
  };
}

function isLiveDetailRequest(request: DetailRequest): boolean {
  return (
    request.readerEpoch === readerEpoch &&
    request.selectionEpoch === detailSelectionEpoch &&
    request.visible === visibleGeneration &&
    request.visible.changes.projectionStamp === request.projectionStamp
  );
}

async function detailPostflight(request: DetailRequest): Promise<boolean> {
  const profile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
  if (!isLiveDetailRequest(request)) return false;
  if (!sameProfileGeneration(request.visible.profile, profile)) {
    throw new Error("Change detail generation changed during staging");
  }
  return true;
}

function mergeChangePages(
  current: ChangesPage,
  next: ChangesPage,
): ChangesPage {
  const lastCurrent = current.changes.at(-1)?.changeId;
  const firstNext = next.changes[0]?.changeId;
  if (
    lastCurrent !== undefined &&
    firstNext !== undefined &&
    firstNext <= lastCurrent
  ) {
    throw new Error("Change continuation did not advance in server order");
  }
  const seen = new Set(current.changes.map((change) => change.changeId));
  if (next.changes.some((change) => seen.has(change.changeId))) {
    throw new Error("Change continuation repeated an emitted Change ID");
  }
  const changes = [...current.changes, ...next.changes].slice(
    -MAX_LIVE_CHANGE_ROWS,
  );
  const visibleIds = new Set(changes.map((change) => change.changeId));
  const presentations =
    current.presentations !== undefined && next.presentations !== undefined
      ? Object.fromEntries(
          Object.entries({
            ...current.presentations,
            ...next.presentations,
          }).filter(([changeId]) => visibleIds.has(changeId)),
        )
      : undefined;
  return {
    ...next,
    changes,
    presentations,
  };
}

function currentRevisionLabel(
  revision: RevisionRef,
  presentation: ChangePresentation | undefined,
): string {
  const entry = presentation?.currentRevisions.find((candidate) =>
    sameRevision(candidate.revision, revision),
  );
  if (entry?.summarySource === "revision_proposal_summary") {
    return `Current Revision — proposal summary: ${entry.revisionProposalSummary ?? "absent"} · ${revision.revisionId}`;
  }
  return `Current Revision — summary absent · ${revision.revisionId}`;
}

async function loadChangeDetail(change: ChangeSummary): Promise<void> {
  const request = beginDetailRequest(change);
  if (request === null) return;
  try {
    const detail = decodeChangeDetail(
      await fetchJSON(`/api/v2/changes/${encodeURIComponent(change.changeId)}`),
    );
    if (!isLiveDetailRequest(request)) return;
    if (
      detail.summary.changeId !== change.changeId ||
      detail.projectionStamp !== change.projectionStamp
    ) {
      throw new Error("Change detail generation is stale; refresh and retry");
    }
    if (!(await detailPostflight(request))) return;
    const content: Node[] = [
      heading(change.changeId),
      line(`topology: ${words(detail.summary.topology)}`),
      line(`lifecycle: ${words(detail.summary.lifecycle)}`),
    ];
    const relations = document.createElement("section");
    relations.append(heading("Relation claims", 3));
    for (const claim of detail.relationClaims) {
      const supports = claim.supports
        .map((support) => `${support.actorId}/${support.eventId}`)
        .join(", ");
      const withdrawals = claim.withdrawals
        .map((withdrawal) => `${withdrawal.actorId}/${withdrawal.eventId}`)
        .join(", ");
      relations.append(
        line(
          `${claim.active ? "active" : "withdrawn"} ${claim.claimId}: ${claim.successor.revisionId} replaces ${claim.predecessor.revisionId} · support ${supports || "none"} · withdrawal ${withdrawals || "none"}`,
        ),
      );
    }
    if (detail.relationClaims.length === 0) {
      relations.append(message("No relation claims."));
    }
    content.push(relations);
    publishDetail(request, content);
  } catch (error) {
    if (!isLiveDetailRequest(request)) return;
    renderRefusal(error);
  }
}

async function loadRevisionDetail(
  change: ChangeSummary,
  revision: RevisionRef,
): Promise<void> {
  const request = beginDetailRequest(change);
  if (request === null) return;
  try {
    const params = new URLSearchParams({
      artifactHash: revision.objectArtifactContentHash,
    });
    const detail = decodeChangeRevisionDetail(
      await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}?${params}`,
      ),
    );
    if (!isLiveDetailRequest(request)) return;
    if (
      detail.changeId !== change.changeId ||
      !sameRevision(detail.revision, revision) ||
      detail.projectionStamp !== change.projectionStamp ||
      detail.associations.some(
        (association) =>
          !sameRevision(association.comparison.revision, revision),
      )
    ) {
      throw new Error("Revision detail generation is stale; refresh and retry");
    }
    if (!(await detailPostflight(request))) return;
    const content: Node[] = [
      heading(revision.revisionId),
      line(`currency: ${words(detail.revisionCurrency)}`),
      line(`relation: ${words(detail.relationClassification)}`),
      line(`captured resource: ${words(detail.availability)}`),
    ];
    const facts = document.createElement("section");
    facts.append(heading("Facts", 3));
    for (const fact of detail.factPresentations) {
      facts.append(
        line(
          `${fact.family}: ${fact.factId} · origin ${fact.originRevision.revisionId} · ${words(fact.revisionCurrency)} · ${words(fact.familyState)} · ${words(fact.availability)}`,
        ),
      );
    }
    if (detail.factPresentations.length === 0) {
      facts.append(message("No facts."));
    }
    content.push(facts);
    const associations = document.createElement("section");
    associations.append(heading("Association comparisons", 3));
    for (const association of detail.associations) {
      associations.append(
        line(
          `${association.comparison.commitOid} · ${words(association.state)} · proof ${words(association.proofAvailability)}`,
        ),
      );
    }
    if (detail.associations.length === 0) {
      associations.append(message("No association comparisons."));
    }
    content.push(associations);
    const resource = document.createElement("button");
    resource.type = "button";
    resource.className = "ghost";
    resource.textContent = "Open exact captured resource";
    resource.addEventListener("click", () => {
      void loadRevisionResource(change, revision);
    });
    content.push(resource);
    publishDetail(request, content);
  } catch (error) {
    if (!isLiveDetailRequest(request)) return;
    renderRefusal(error);
  }
}

async function loadRevisionResource(
  change: ChangeSummary,
  revision: RevisionRef,
): Promise<void> {
  const request = beginDetailRequest(change);
  if (request === null) return;
  try {
    const params = new URLSearchParams({
      artifactHash: revision.objectArtifactContentHash,
    });
    const resource = decodeRevisionResource(
      await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}/resource?${params}`,
      ),
    );
    if (!isLiveDetailRequest(request)) return;
    if (!sameRevision(resource.resource.revision, revision)) {
      throw new Error(
        "captured resource identity does not match its exact route",
      );
    }
    if (!(await detailPostflight(request))) return;
    const content: Node[] = [
      heading(`Captured resource · ${revision.revisionId}`),
      line(`availability: ${words(resource.availability)}`),
    ];
    if (resource.capturedDocumentHash) {
      content.push(line(`document hash: ${resource.capturedDocumentHash}`));
    }
    if (resource.capturedDocument !== undefined) {
      const captured = document.createElement("pre");
      captured.textContent = JSON.stringify(resource.capturedDocument, null, 2);
      content.push(captured);
    }
    for (const diagnostic of resource.diagnostics) {
      content.push(line(diagnostic));
    }
    publishDetail(request, content);
  } catch (error) {
    if (!isLiveDetailRequest(request)) return;
    renderRefusal(error);
  }
}

async function loadInterdiff(
  change: ChangeSummary,
  from: RevisionRef,
  to: RevisionRef,
): Promise<void> {
  const request = beginDetailRequest(change);
  if (request === null) return;
  try {
    const params = new URLSearchParams({
      fromArtifactHash: from.objectArtifactContentHash,
      toArtifactHash: to.objectArtifactContentHash,
    });
    const interdiff = decodeRevisionInterdiff(
      await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}/interdiff/${encodeURIComponent(from.revisionId)}/${encodeURIComponent(to.revisionId)}?${params}`,
      ),
    );
    if (!isLiveDetailRequest(request)) return;
    if (
      !sameRevision(interdiff.interdiff.from, from) ||
      !sameRevision(interdiff.interdiff.to, to)
    ) {
      throw new Error(
        "Revision interdiff identity does not match its exact route",
      );
    }
    if (!(await detailPostflight(request))) return;
    const content: Node[] = [
      heading(`Revision interdiff · ${from.revisionId} → ${to.revisionId}`),
      line(`availability: ${words(interdiff.availability)}`),
    ];
    if (interdiff.comparison !== undefined) {
      const comparison = document.createElement("pre");
      comparison.textContent = JSON.stringify(interdiff.comparison, null, 2);
      content.push(comparison);
    }
    for (const diagnostic of interdiff.diagnostics) {
      content.push(line(diagnostic));
    }
    publishDetail(request, content);
  } catch (error) {
    if (!isLiveDetailRequest(request)) return;
    renderRefusal(error);
  }
}

function publishDetail(request: DetailRequest, content: Node[]): void {
  if (!isLiveDetailRequest(request)) return;
  const body = document.querySelector<HTMLElement>("#detail-body");
  if (!body) throw new Error("Inspector detail container is absent");
  body.replaceChildren(...content);
}

function renderRefusal(error: unknown): void {
  const text = error instanceof Error ? error.message : String(error);
  clearSemanticPresentation();
  const master = document.querySelector<HTMLElement>("#master");
  master?.replaceChildren(message(`Reader refused: ${text}`));
  const banner = document.querySelector<HTMLElement>("#error");
  if (banner) {
    banner.textContent = `error: ${text}`;
    banner.classList.remove("hidden");
  }
}

function renderRestart(error: unknown): void {
  const text =
    error instanceof ChangePageFailure && error.code === "stale_projection"
      ? "Change page became stale; restarting from the first page."
      : "Change generation changed while loading; restarting from the first page.";
  const banner = document.querySelector<HTMLElement>("#error");
  if (banner) {
    banner.textContent = text;
    banner.classList.remove("hidden");
  }
}

function clearSemanticPresentation(): void {
  readerEpoch += 1;
  detailSelectionEpoch += 1;
  visibleGeneration = null;
  document.querySelector<HTMLElement>("#master")?.replaceChildren();
  document.querySelector<HTMLElement>("#detail-body")?.replaceChildren();
  for (const selector of [
    "#stat-events",
    "#stat-units",
    "#stat-threads",
    "#stat-hash",
  ]) {
    setText(selector, "—");
  }
}

function sameRevision(left: RevisionRef, right: RevisionRef): boolean {
  return (
    left.revisionId === right.revisionId &&
    left.objectArtifactContentHash === right.objectArtifactContentHash
  );
}

function words(value: string): string {
  return value.replaceAll("_", " ");
}

function setText(selector: string, value: string): void {
  const element = document.querySelector<HTMLElement>(selector);
  if (element) element.textContent = value;
}

function message(text: string): HTMLParagraphElement {
  const paragraph = document.createElement("p");
  paragraph.className = "empty";
  paragraph.textContent = text;
  return paragraph;
}

function line(text: string): HTMLParagraphElement {
  const paragraph = document.createElement("p");
  paragraph.textContent = text;
  return paragraph;
}

function heading(text: string, level: 2 | 3 = 2): HTMLHeadingElement {
  const element = document.createElement(`h${level}`) as HTMLHeadingElement;
  element.textContent = text;
  return element;
}
