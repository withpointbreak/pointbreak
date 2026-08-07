import {
  bootstrapCapability,
  installDefaultAuthCoordinator,
  requestReconnect,
} from "./auth";
import {
  configureConnectionActions,
  initConnectionControls,
} from "./connection";
import { fetchJSON } from "./http";

type Availability = "migration_required" | "migration_in_progress" | "ready";

interface ReaderProfile {
  schema: "pointbreak.inspect-reader-profile";
  version: 1;
  availability: Availability;
  minimumReaderProfile?: string;
  authorityCursor: { eventCount?: number };
  documents: Record<string, number>;
}

interface RevisionRef {
  revisionId: string;
  objectArtifactContentHash: string;
}

interface ChangeSummary {
  changeId: string;
  topology: string;
  lifecycle: string;
  attentionSummary: string;
  availabilitySummary: string;
  currentRevisionRefs: RevisionRef[];
  diagnostics?: string[];
  projectionStamp: string;
}

interface ChangesPage {
  schema: "pointbreak.inspect-changes-page";
  version: 1;
  changes: ChangeSummary[];
  diagnostics: string[];
  projectionStamp: string;
}

interface AttentionPage {
  schema: "pointbreak.inspect-attention";
  version: 2;
  changes: ChangeSummary[];
  projectionStamp: string;
}

interface ChangeDetail {
  schema: "pointbreak.review-change";
  version: 1;
  summary: ChangeSummary;
  relationClaims: Array<{
    claimId: string;
    active: boolean;
    successor: RevisionRef;
    predecessor: RevisionRef;
    supports: Array<{ actorId: string; eventId: string }>;
    withdrawals: Array<{ actorId: string; eventId: string }>;
  }>;
  diagnostics: string[];
  projectionStamp: string;
}

interface ChangeRevisionDetail {
  schema: "pointbreak.review-change-revision";
  version: 1;
  changeId: string;
  revision: RevisionRef;
  revisionCurrency: string;
  relationClassification: string;
  availability: string;
  factPresentations: Array<{
    factId: string;
    family: string;
    originRevision: RevisionRef;
    revisionCurrency: string;
    familyState: string;
    availability: string;
  }>;
  associations: Array<{
    state: string;
    proofAvailability: string;
    comparison: { revision: RevisionRef; commitOid: string };
  }>;
  diagnostics: string[];
  projectionStamp: string;
}

interface RevisionResource {
  schema: "pointbreak.review-revision-resource";
  version: 1;
  resource: { revision: RevisionRef; objectId: string };
  availability: string;
  capturedDocumentHash?: string;
  capturedDocument?: unknown;
  diagnostics: string[];
}

interface RevisionInterdiff {
  schema: "pointbreak.review-revision-interdiff";
  version: 1;
  interdiff: { from: RevisionRef; to: RevisionRef };
  availability: string;
  comparison?: unknown;
  diagnostics: string[];
}

const REQUIRED_DOCUMENTS: Readonly<Record<string, number>> = {
  "pointbreak.inspect-reader-profile": 1,
  "pointbreak.inspect-changes-page": 1,
  "pointbreak.review-change": 1,
  "pointbreak.review-change-revision": 1,
  "pointbreak.review-revision": 3,
  "pointbreak.review-revision-resource": 1,
  "pointbreak.review-association-comparison": 1,
  "pointbreak.review-revision-interdiff": 1,
  "pointbreak.inspect-attention": 2,
  "pointbreak.reader-upgrade-required": 1,
  "pointbreak.store-migration-required": 1,
  "pointbreak.store-migration-in-progress": 1,
};

let pollTimer: ReturnType<typeof setInterval> | null = null;
let generation = 0;
let connectionControlsInitialized = false;

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
    const profile = validateProfile(
      (await fetchJSON("/api/v2/profile")) as ReaderProfile,
    );
    prepareChangeShell();
    if (profile.availability !== "ready") {
      renderUnavailable(profile.availability);
      return;
    }
    await loadGeneration(profile);
    if (options.poll !== false) {
      pollTimer = setInterval(() => {
        void refresh();
      }, 3000);
    }
  } catch (error) {
    renderRefusal(error);
  }
}

export function stopChangeReader(): void {
  generation += 1;
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function refresh(): Promise<void> {
  try {
    const profile = validateProfile(
      (await fetchJSON("/api/v2/profile")) as ReaderProfile,
    );
    if (profile.availability !== "ready") {
      renderUnavailable(profile.availability);
      stopChangeReader();
      return;
    }
    await loadGeneration(profile);
  } catch (error) {
    renderRefusal(error);
  }
}

function validateProfile(profile: ReaderProfile): ReaderProfile {
  if (
    profile.schema !== "pointbreak.inspect-reader-profile" ||
    profile.version !== 1 ||
    !["migration_required", "migration_in_progress", "ready"].includes(
      profile.availability,
    )
  ) {
    throw new Error("incompatible Inspector reader profile");
  }
  for (const [schema, version] of Object.entries(REQUIRED_DOCUMENTS)) {
    if (profile.documents?.[schema] !== version) {
      throw new Error(`reader profile is missing ${schema} v${version}`);
    }
  }
  if (
    profile.availability === "ready" &&
    profile.minimumReaderProfile !== "review_change_revision_v1"
  ) {
    throw new Error("incompatible minimum reader profile");
  }
  return profile;
}

async function loadGeneration(profile: ReaderProfile): Promise<void> {
  const requestedGeneration = ++generation;
  const [changes, attention] = (await Promise.all([
    fetchJSON("/api/v2/changes"),
    fetchJSON("/api/v2/attention"),
  ])) as [ChangesPage, AttentionPage];
  if (requestedGeneration !== generation) return;
  if (
    changes.schema !== "pointbreak.inspect-changes-page" ||
    changes.version !== 1 ||
    attention.schema !== "pointbreak.inspect-attention" ||
    attention.version !== 2 ||
    !changes.projectionStamp ||
    changes.projectionStamp !== attention.projectionStamp ||
    changes.changes.some(
      (change) => change.projectionStamp !== changes.projectionStamp,
    ) ||
    attention.changes.some(
      (change) => change.projectionStamp !== changes.projectionStamp,
    )
  ) {
    throw new Error("Change documents do not form one coherent generation");
  }
  renderGeneration(profile, changes, attention);
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
    for (const revision of change.currentRevisionRefs) {
      const select = document.createElement("button");
      select.type = "button";
      select.className = "ghost mono";
      select.dataset.revisionId = revision.revisionId;
      select.textContent = revision.revisionId;
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
  master.replaceChildren(list);
  setText(
    "#stat-events",
    `${profile.authorityCursor.eventCount ?? "—"} events`,
  );
  setText("#stat-units", `${page.changes.length} Changes`);
  setText("#stat-threads", `${attention.changes.length} need attention`);
  setText("#stat-hash", page.projectionStamp);
}

async function loadChangeDetail(change: ChangeSummary): Promise<void> {
  try {
    const detail = (await fetchJSON(
      `/api/v2/changes/${encodeURIComponent(change.changeId)}`,
    )) as ChangeDetail;
    if (
      detail.summary.changeId !== change.changeId ||
      detail.projectionStamp !== change.projectionStamp
    ) {
      throw new Error("Change detail generation is stale; refresh and retry");
    }
    const body = detailBody();
    body.append(
      heading(change.changeId),
      line(`topology: ${words(detail.summary.topology)}`),
      line(`lifecycle: ${words(detail.summary.lifecycle)}`),
    );
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
    body.append(relations);
  } catch (error) {
    renderRefusal(error);
  }
}

async function loadRevisionDetail(
  change: ChangeSummary,
  revision: RevisionRef,
): Promise<void> {
  try {
    const params = new URLSearchParams({
      artifactHash: revision.objectArtifactContentHash,
    });
    const detail = (await fetchJSON(
      `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}?${params}`,
    )) as ChangeRevisionDetail;
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
    const body = detailBody();
    body.append(
      heading(revision.revisionId),
      line(`currency: ${words(detail.revisionCurrency)}`),
      line(`relation: ${words(detail.relationClassification)}`),
      line(`captured resource: ${words(detail.availability)}`),
    );
    const facts = document.createElement("section");
    facts.append(heading("Facts", 3));
    for (const fact of detail.factPresentations) {
      facts.append(
        line(
          `${fact.family}: ${fact.factId} · origin ${fact.originRevision.revisionId} · ${words(fact.revisionCurrency)} · ${words(fact.familyState)} · ${words(fact.availability)}`,
        ),
      );
    }
    if (detail.factPresentations.length === 0)
      facts.append(message("No facts."));
    body.append(facts);
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
    body.append(associations);
    const resource = document.createElement("button");
    resource.type = "button";
    resource.className = "ghost";
    resource.textContent = "Open exact captured resource";
    resource.addEventListener("click", () => {
      void loadRevisionResource(change, revision);
    });
    body.append(resource);
  } catch (error) {
    renderRefusal(error);
  }
}

async function loadRevisionResource(
  change: ChangeSummary,
  revision: RevisionRef,
): Promise<void> {
  try {
    const params = new URLSearchParams({
      artifactHash: revision.objectArtifactContentHash,
    });
    const resource = (await fetchJSON(
      `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}/resource?${params}`,
    )) as RevisionResource;
    if (!sameRevision(resource.resource.revision, revision)) {
      throw new Error(
        "captured resource identity does not match its exact route",
      );
    }
    const body = detailBody();
    body.append(
      heading(`Captured resource · ${revision.revisionId}`),
      line(`availability: ${words(resource.availability)}`),
    );
    if (resource.capturedDocumentHash) {
      body.append(line(`document hash: ${resource.capturedDocumentHash}`));
    }
    if (resource.capturedDocument !== undefined) {
      const captured = document.createElement("pre");
      captured.textContent = JSON.stringify(resource.capturedDocument, null, 2);
      body.append(captured);
    }
    for (const diagnostic of resource.diagnostics)
      body.append(line(diagnostic));
  } catch (error) {
    renderRefusal(error);
  }
}

async function loadInterdiff(
  change: ChangeSummary,
  from: RevisionRef,
  to: RevisionRef,
): Promise<void> {
  try {
    const params = new URLSearchParams({
      fromArtifactHash: from.objectArtifactContentHash,
      toArtifactHash: to.objectArtifactContentHash,
    });
    const interdiff = (await fetchJSON(
      `/api/v2/changes/${encodeURIComponent(change.changeId)}/interdiff/${encodeURIComponent(from.revisionId)}/${encodeURIComponent(to.revisionId)}?${params}`,
    )) as RevisionInterdiff;
    if (
      !sameRevision(interdiff.interdiff.from, from) ||
      !sameRevision(interdiff.interdiff.to, to)
    ) {
      throw new Error(
        "Revision interdiff identity does not match its exact route",
      );
    }
    const body = detailBody();
    body.append(
      heading(`Revision interdiff · ${from.revisionId} → ${to.revisionId}`),
      line(`availability: ${words(interdiff.availability)}`),
    );
    if (interdiff.comparison !== undefined) {
      const comparison = document.createElement("pre");
      comparison.textContent = JSON.stringify(interdiff.comparison, null, 2);
      body.append(comparison);
    }
    for (const diagnostic of interdiff.diagnostics)
      body.append(line(diagnostic));
  } catch (error) {
    renderRefusal(error);
  }
}

function detailBody(): HTMLElement {
  const body = document.querySelector<HTMLElement>("#detail-body");
  if (!body) throw new Error("Inspector detail container is absent");
  body.replaceChildren();
  return body;
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

function clearSemanticPresentation(): void {
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
