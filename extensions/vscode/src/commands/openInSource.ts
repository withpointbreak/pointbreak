import path from "node:path";
import {
  Range,
  Selection,
  TextEditorRevealType,
  Uri,
  ViewColumn,
  window,
  workspace,
} from "vscode";
import type { ReviewSnapshotDoc } from "../cli";
import {
  repoRelativeFile,
  type SourceLanding,
  snapshotToLive,
  sourceFileForTarget,
  type ZeroBasedSelection,
} from "../sourceMapping";
import type { SnapshotRangeTarget } from "../webviewProtocol";

export interface SourceReviewContext {
  readonly targetKey: string;
  readonly revisionId: string;
  readonly snapshot: ReviewSnapshotDoc;
  readonly filePath: string;
  readonly side: "old" | "new";
  readonly target: SnapshotRangeTarget;
  readonly lastLanding: Exclude<SourceLanding, { kind: "unavailable" }>;
}

export interface OpenInSourceRequest {
  readonly repoRoot: string;
  readonly targetKey: string;
  readonly revisionId: string;
  readonly snapshot: ReviewSnapshotDoc;
  readonly target: SnapshotRangeTarget;
}

export interface OpenedSourceDocument {
  readonly document: object;
  readonly lines: readonly string[];
}

export interface OpenInSourceHost {
  openDocument(absoluteFile: string): Promise<OpenedSourceDocument>;
  reveal(document: object, range: ZeroBasedSelection): Promise<void>;
  decorateTemporarily(document: object, range: ZeroBasedSelection): void;
  showInformationMessage(message: string): Thenable<string | undefined>;
  dispose(): void;
}

export interface SourceOpener {
  open(request: OpenInSourceRequest): Promise<void>;
}

/** Holds source identity only for live editor document objects. */
export class SourceReviewContextStore {
  private contexts = new WeakMap<object, SourceReviewContext>();

  constructor(private readonly onChange: () => void = () => undefined) {}

  get(document: object): SourceReviewContext | undefined {
    return this.contexts.get(document);
  }

  set(document: object, context: SourceReviewContext): void {
    this.contexts.set(document, context);
    this.onChange();
  }

  delete(document: object): void {
    this.contexts.delete(document);
    this.onChange();
  }

  dispose(): void {
    this.contexts = new WeakMap();
    this.onChange();
  }
}

/** Opens one validated captured range in a native editor. */
export class OpenInSourceCommand implements SourceOpener {
  constructor(
    readonly contexts: SourceReviewContextStore,
    private readonly host: OpenInSourceHost = new VscodeOpenInSourceHost(),
  ) {}

  async open(request: OpenInSourceRequest): Promise<void> {
    const sourceFile = sourceFileForTarget(request.snapshot, request.target);
    if (sourceFile.kind === "unavailable") {
      await this.host.showInformationMessage(sourceFile.message);
      return;
    }

    let absoluteFile: string;
    try {
      absoluteFile = path.resolve(
        request.repoRoot,
        ...sourceFile.filePath.split("/"),
      );
      if (
        repoRelativeFile(request.repoRoot, absoluteFile) !== sourceFile.filePath
      ) {
        throw new Error("snapshot path changed during resolution");
      }
    } catch {
      await this.host.showInformationMessage(
        "Pointbreak refused a source path outside the current repository.",
      );
      return;
    }

    let opened: OpenedSourceDocument;
    try {
      opened = await this.host.openDocument(absoluteFile);
    } catch {
      await this.host.showInformationMessage(
        "Pointbreak could not open the captured source file.",
      );
      return;
    }

    const landing = snapshotToLive(
      request.snapshot,
      request.target,
      opened.lines,
    );
    if (landing.kind === "unavailable") {
      await this.host.showInformationMessage(landing.message);
      return;
    }
    if (landing.kind === "drifted" && landing.message) {
      void this.host.showInformationMessage(landing.message);
    }
    await this.host.reveal(opened.document, landing.range);
    this.host.decorateTemporarily(opened.document, landing.range);
    this.contexts.set(opened.document, {
      targetKey: request.targetKey,
      revisionId: request.revisionId,
      snapshot: request.snapshot,
      filePath: landing.filePath,
      side: request.target.side,
      target: request.target,
      lastLanding: landing,
    });
  }

  dispose(): void {
    this.contexts.dispose();
    this.host.dispose();
  }
}

class VscodeOpenInSourceHost implements OpenInSourceHost {
  private readonly decoration = window.createTextEditorDecorationType({
    isWholeLine: true,
    borderWidth: "0 0 0 3px",
    borderStyle: "solid",
    borderColor: "var(--vscode-focusBorder)",
    overviewRulerColor: "var(--vscode-focusBorder)",
  });
  private readonly editors = new WeakMap<
    object,
    Awaited<ReturnType<typeof window.showTextDocument>>
  >();
  private readonly timers = new Map<object, NodeJS.Timeout>();

  async openDocument(absoluteFile: string): Promise<OpenedSourceDocument> {
    const document = await workspace.openTextDocument(Uri.file(absoluteFile));
    const lines = Array.from(
      { length: document.lineCount },
      (_, index) => document.lineAt(index).text,
    );
    return { document, lines };
  }

  async reveal(document: object, range: ZeroBasedSelection): Promise<void> {
    const editor = await window.showTextDocument(
      document as never,
      ViewColumn.One,
      false,
    );
    const nativeRange = toRange(range);
    editor.revealRange(nativeRange, TextEditorRevealType.InCenter);
    editor.selection = new Selection(nativeRange.start, nativeRange.end);
    this.editors.set(document, editor);
  }

  decorateTemporarily(document: object, range: ZeroBasedSelection): void {
    const editor = this.editors.get(document);
    if (!editor) return;
    const nativeRange = toRange(range);
    editor.setDecorations(this.decoration, [nativeRange]);
    const previous = this.timers.get(document);
    if (previous) clearTimeout(previous);
    const timer = setTimeout(() => {
      editor.setDecorations(this.decoration, []);
      this.timers.delete(document);
    }, 3_000);
    this.timers.set(document, timer);
  }

  showInformationMessage(message: string): Thenable<string | undefined> {
    return window.showInformationMessage(message);
  }

  dispose(): void {
    for (const timer of this.timers.values()) clearTimeout(timer);
    this.timers.clear();
    this.decoration.dispose();
  }
}

function toRange(range: ZeroBasedSelection): Range {
  return new Range(
    range.start.line,
    range.start.character,
    range.end.line,
    range.end.character,
  );
}
