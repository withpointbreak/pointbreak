import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  prepareChangeInspectorShell,
  renderChangeInspector,
} from "../src/change-inspector-render";
import {
  createChangeInspectorState,
  stageGeneration,
} from "../src/change-inspector-state";
import type {
  AttentionPage,
  ChangesPage,
  ReaderProfile,
} from "../src/change-protocol";
import { mountInspectorDom, resetDom } from "./support/dom";

const profile: ReaderProfile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: { eventCount: 2 },
  commitGraphStamp: "sha256:stamp",
  minimumReaderProfile: "review_change_revision_v1",
  documents: {},
};
const revision = {
  revisionId: "revision:sha256:one",
  objectArtifactContentHash: "sha256:artifact",
};
const changes: ChangesPage = {
  schema: "pointbreak.inspect-changes-page",
  version: 1,
  projectionStamp: "sha256:generation",
  next: null,
  changes: [
    {
      changeId: "change:sha256:one",
      topology: "parallel_current",
      lifecycle: "in_progress",
      attentionSummary: "conflicted",
      availabilitySummary: "incomplete",
      currentRevisionRefs: [revision],
      projectionStamp: "sha256:generation",
    },
  ],
  presentations: {
    "change:sha256:one": {
      currentRevisions: [
        {
          revision,
          revisionProposalSummary: "Server proposal",
          summarySource: "revision_proposal_summary",
        },
      ],
    },
  },
};
const attention: AttentionPage = {
  ...changes,
  schema: "pointbreak.inspect-attention",
  version: 2,
};

beforeEach(() => mountInspectorDom());

describe("Change inspector render", () => {
  it("projects server-owned cards and exact placeholder selection into the retained shell", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:one",
      revision,
      query: {},
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    expect(document.querySelector("#master")?.textContent).toContain(
      "Server proposal",
    );
    expect(document.querySelector("#master")?.textContent).toContain(
      "parallel current",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision selected",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "sha256:artifact",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Copy link",
    );
  });

  it("keeps an off-page exact deep link visible without claiming list absence is refusal", () => {
    const navigate = vi.fn();
    prepareChangeInspectorShell({ navigate });
    const state = createChangeInspectorState({
      kind: "revision",
      changeId: "change:sha256:off-page",
      revision: {
        revisionId: "revision:sha256:stale-but-readable",
        objectArtifactContentHash: "sha256:off-page-artifact",
      },
      query: { q: "narrowed" },
    });
    state.publish(stageGeneration(profile, changes, attention, profile));
    renderChangeInspector(state.snapshot(), { navigate });
    expect(document.querySelector("#route-diagnostic")?.classList).toContain(
      "hidden",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "revision:sha256:stale-but-readable",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision selected",
    );
  });
});

resetDom();
