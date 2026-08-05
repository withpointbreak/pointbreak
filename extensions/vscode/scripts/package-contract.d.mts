export interface BinaryEvidence {
  sha256: string;
  versionDocument: string;
  readerProfileDocument?: string;
}

export function assertExactPackageFiles(
  actual: string[],
  binary: string,
  source: string,
): void;

export function assertExactArchiveFiles(
  actual: string[],
  binary: string,
  source: string,
): void;

export function powershellCommand(script: string): string;

export function verifyBundledBinary(
  approved: BinaryEvidence,
  bundled: BinaryEvidence,
): void;
