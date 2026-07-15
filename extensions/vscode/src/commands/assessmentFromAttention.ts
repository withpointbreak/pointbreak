import { commands, window } from "vscode";
import type { AttentionItemNode } from "../attentionView";
import type {
  AssessmentShowDoc,
  AssessmentValue,
  AssessmentView,
  PointbreakCli,
} from "../cli";
import type {
  HumanWriteContext,
  HumanWriteCoordinator,
} from "../humanWriteCoordinator";
import { shortReferenceId } from "../idDisplay";

type AssessmentAttentionItem = Extract<
  AttentionItemNode["item"],
  {
    kind: "ambiguous_assessment" | "stale_assessment" | "failed_validation";
  }
>;

interface AssessmentConfirmation extends HumanWriteContext {
  revisionId: string;
  assessment: AssessmentValue;
  summary?: string;
  replacementIds: string[];
  remainingCandidateIds: string[];
  crossActorReplacementIds: string[];
}

export interface AssessmentReplacementCandidate {
  view: AssessmentView;
  preselected: boolean;
}

interface AssessmentPreparation {
  replacementIds: string[];
  remainingCandidateIds: string[];
  crossActorReplacementIds: string[];
}

interface AssessmentFromAttentionDependencies {
  humanWrites?: HumanWriteCoordinator;
  pickAssessment(
    item: AssessmentAttentionItem,
  ): Promise<AssessmentValue | undefined>;
  promptSummary(
    item: AssessmentAttentionItem,
    assessment: AssessmentValue,
  ): Promise<string | undefined>;
  pickReplacements(
    candidates: AssessmentReplacementCandidate[],
  ): Promise<AssessmentView[] | undefined>;
  confirmAssessment(context: AssessmentConfirmation): Promise<boolean>;
  routeHeadResolution(node: AttentionItemNode): Promise<unknown>;
  showInformationMessage(message: string): Promise<unknown>;
  showErrorMessage(message: string): Promise<unknown>;
}

const ASSESSMENTS: Array<{ label: string; assessment: AssessmentValue }> = [
  { label: "Accepted", assessment: "accepted" },
  {
    label: "Accepted with follow-up",
    assessment: "accepted-with-follow-up",
  },
  { label: "Needs changes", assessment: "needs-changes" },
  { label: "Needs clarification", assessment: "needs-clarification" },
];
const CONFIRM_ASSESSMENT_ACTION = "Record assessment";

export async function runAssessmentFromAttentionCommand(
  cli: PointbreakCli,
  node: AttentionItemNode | undefined,
  overrides: Partial<AssessmentFromAttentionDependencies> = {},
): Promise<void> {
  const dependencies = { ...defaultDependencies(), ...overrides };
  if (!node) {
    await dependencies.showErrorMessage(
      "Use this command from a matching Pointbreak Attention row.",
    );
    return;
  }
  if (!isAssessmentItem(node.item)) {
    await dependencies.showErrorMessage(
      "This Pointbreak attention item cannot accept an assessment.",
    );
    return;
  }
  const humanWrites = dependencies.humanWrites;
  if (!humanWrites) {
    await dependencies.showErrorMessage(
      "Pointbreak could not prepare the human write.",
    );
    return;
  }
  const item = node.item;
  const revisionId = await assessmentRevision(node, item, dependencies);
  if (!revisionId) return;
  const assessment = await dependencies.pickAssessment(item);
  if (!assessment) return;
  const enteredSummary = await dependencies.promptSummary(item, assessment);
  if (enteredSummary === undefined) return;
  const summary = enteredSummary.trim() || undefined;

  try {
    const result = await humanWrites.run({
      repo: node.folder.uri.fsPath,
      resource: node.folder.uri,
      trackOverride:
        item.kind === "failed_validation" ? item.trackId : undefined,
      prepare: async (context) => {
        const includeEveryCurrentCandidate =
          item.kind === "ambiguous_assessment";
        const document = await cli.showAssessments(node.folder.uri.fsPath, {
          revisionId,
          track: includeEveryCurrentCandidate ? undefined : context.track,
        });
        const candidates = replacementCandidates(
          document,
          context,
          revisionId,
          includeEveryCurrentCandidate,
        );
        const preselected = candidates
          .filter(({ preselected }) => preselected)
          .map(({ view }) => view);
        const requiresPicker =
          preselected.length > 1 ||
          candidates.some(({ preselected }) => !preselected);
        if (!requiresPicker) {
          return assessmentPreparation(candidates, preselected, context);
        }
        const selected = await dependencies.pickReplacements(candidates);
        return selected
          ? assessmentPreparation(candidates, selected, context)
          : undefined;
      },
      confirm: (context, preparation) => {
        if (!preparation) return Promise.resolve(false);
        return dependencies.confirmAssessment({
          ...context,
          revisionId,
          assessment,
          summary,
          replacementIds: preparation.replacementIds,
          remainingCandidateIds: preparation.remainingCandidateIds,
          crossActorReplacementIds: preparation.crossActorReplacementIds,
        });
      },
      write: (context, preparation) => {
        if (!preparation) {
          throw new Error("assessment preparation was cancelled");
        }
        return cli.addAssessment(node.folder.uri.fsPath, {
          revisionId,
          track: context.track,
          assessment,
          summary,
          replaces: preparation.replacementIds,
        });
      },
    });
    if (!result) return;
  } catch (error) {
    await dependencies.showErrorMessage(
      `Pointbreak could not record the assessment: ${errorMessage(error)}`,
    );
    return;
  }
  await dependencies.showInformationMessage("Assessment recorded.");
}

export function sameHumanRevisionCandidates(
  document: AssessmentShowDoc,
  context: HumanWriteContext,
  revisionId: string,
): AssessmentView[] {
  return replacementCandidates(document, context, revisionId, false).map(
    ({ view }) => view,
  );
}

export function replacementCandidates(
  document: AssessmentShowDoc,
  context: HumanWriteContext,
  revisionId: string,
  includeEveryCurrentCandidate: boolean,
): AssessmentReplacementCandidate[] {
  const current = document.assessments.filter(
    (candidate) =>
      candidate.status === "current" &&
      candidate.target.kind === "revision" &&
      candidate.target.revisionId === revisionId,
  );
  return current
    .filter(
      (candidate) =>
        includeEveryCurrentCandidate ||
        (candidate.trackId === context.track &&
          candidate.writer.actorId === context.actorId),
    )
    .map((view) => ({
      view,
      preselected:
        view.trackId === context.track &&
        view.writer.actorId === context.actorId,
    }));
}

function assessmentPreparation(
  candidates: AssessmentReplacementCandidate[],
  selected: AssessmentView[],
  context: HumanWriteContext,
): AssessmentPreparation {
  const selectedIds = new Set(selected.map(({ id }) => id));
  return {
    replacementIds: [...selectedIds],
    remainingCandidateIds: candidates
      .map(({ view }) => view.id)
      .filter((id) => !selectedIds.has(id)),
    crossActorReplacementIds: selected
      .filter(({ writer }) => writer.actorId !== context.actorId)
      .map(({ id }) => id),
  };
}

function isAssessmentItem(
  item: AttentionItemNode["item"],
): item is AssessmentAttentionItem {
  return (
    item.kind === "ambiguous_assessment" ||
    item.kind === "stale_assessment" ||
    item.kind === "failed_validation"
  );
}

async function assessmentRevision(
  node: AttentionItemNode,
  item: AssessmentAttentionItem,
  dependencies: AssessmentFromAttentionDependencies,
): Promise<string | undefined> {
  if (item.kind !== "stale_assessment") {
    if (item.revisionId) return item.revisionId;
    await dependencies.showErrorMessage(
      "Pointbreak assessment attention is missing its exact revision.",
    );
    return undefined;
  }
  const heads = item.headRevisionIds ?? [];
  if (heads.length === 1) return heads[0];
  if (heads.length > 1) {
    await dependencies.routeHeadResolution(node);
    return undefined;
  }
  await dependencies.showErrorMessage(
    "Pointbreak could not find the stale assessment's exact current head.",
  );
  return undefined;
}

function defaultDependencies(): AssessmentFromAttentionDependencies {
  return {
    pickAssessment: async () =>
      (
        await window.showQuickPick(ASSESSMENTS, {
          placeHolder: "Choose an assessment",
        })
      )?.assessment,
    promptSummary: async (_item, assessment) =>
      window.showInputBox({
        prompt: `Optional summary for ${assessment}`,
        placeHolder: "Leave blank to record without a summary",
      }),
    pickReplacements: async (candidates) => {
      const choices = candidates.map(({ view, preselected }) => ({
        label: shortReferenceId(view.id),
        description: `${view.assessment} · ${view.trackId}`,
        detail: [view.writer.actorId, view.createdAt]
          .filter(Boolean)
          .join(" · "),
        candidate: view,
        picked: preselected,
      }));
      return (
        await window.showQuickPick(choices, {
          canPickMany: true,
          placeHolder: "Choose current assessments this judgment replaces",
        })
      )?.map(({ candidate }) => candidate);
    },
    confirmAssessment: async (context) =>
      (await window.showWarningMessage(
        assessmentConfirmation(context),
        { modal: true },
        CONFIRM_ASSESSMENT_ACTION,
      )) === CONFIRM_ASSESSMENT_ACTION,
    routeHeadResolution: async (node) =>
      commands.executeCommand("pointbreak.captureAttentionResolution", node),
    showInformationMessage: async (message) =>
      window.showInformationMessage(message),
    showErrorMessage: async (message) => window.showErrorMessage(message),
  };
}

export function assessmentConfirmation(
  context: AssessmentConfirmation,
): string {
  const replacements = context.replacementIds.length
    ? ` Replace ${context.replacementIds.map(shortReferenceId).join(", ")}.`
    : "";
  const summary = context.summary ? ` Summary: “${context.summary}”.` : "";
  const crossActor = context.crossActorReplacementIds.length
    ? ` This explicitly replaces ${assessmentCount(context.crossActorReplacementIds.length)} from another actor identity; replaced assessments remain in history.`
    : "";
  const remaining = context.remainingCandidateIds.length
    ? ` ${assessmentCount(context.remainingCandidateIds.length)} will remain current and may keep this revision ambiguous.`
    : "";
  return `Record ${context.assessment} on ${shortReferenceId(context.revisionId)} as ${context.actorId} in track ${context.track}.${summary}${replacements}${crossActor}${remaining}`;
}

function assessmentCount(count: number): string {
  return `${count} ${count === 1 ? "assessment" : "assessments"}`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
