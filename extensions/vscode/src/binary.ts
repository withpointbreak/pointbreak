import { existsSync } from "node:fs";
import path from "node:path";

export interface ResolvedBinary {
  path: string;
  source: "setting" | "bundled" | "path";
}

export interface BinaryConfig {
  binaryPath?: string;
  useGlobalCli: boolean;
  platform?: NodeJS.Platform;
  arch?: string;
  path?: string;
  exists?: (candidate: string) => boolean;
  announceFallback?: (message: string) => void;
}

export function resolveBinary(
  config: BinaryConfig,
  extensionRoot: string,
): ResolvedBinary {
  const configuredPath = config.binaryPath?.trim();
  if (configuredPath) {
    return { path: configuredPath, source: "setting" };
  }

  const platform = config.platform ?? process.platform;
  const arch = config.arch ?? process.arch;
  const executable = platform === "win32" ? "shore.exe" : "shore";
  const bundledPath = path.join(
    extensionRoot,
    "bin",
    `${platform}-${arch}`,
    executable,
  );
  const candidateExists = config.exists ?? existsSync;
  const pathBinary = findOnPath(
    executable,
    config.path ?? process.env.PATH ?? "",
    platform,
    candidateExists,
  );

  const bundled = (): ResolvedBinary | undefined =>
    candidateExists(bundledPath)
      ? { path: bundledPath, source: "bundled" }
      : undefined;
  const global = (): ResolvedBinary | undefined =>
    pathBinary ? { path: pathBinary, source: "path" } : undefined;
  const preferred = config.useGlobalCli ? global() : bundled();
  if (preferred) {
    return preferred;
  }

  const fallback = config.useGlobalCli ? bundled() : global();
  if (fallback) {
    config.announceFallback?.(
      config.useGlobalCli
        ? "The shore CLI was not found on PATH; using the bundled CLI."
        : "The bundled shore CLI was not found; using the CLI from PATH.",
    );
    return fallback;
  }

  throw new Error(
    "Pointbreak could not find the shore CLI. Install Pointbreak globally or set pointbreak.binaryPath.",
  );
}

function findOnPath(
  executable: string,
  searchPath: string,
  platform: NodeJS.Platform,
  exists: (candidate: string) => boolean,
): string | undefined {
  const delimiter = platform === "win32" ? ";" : ":";
  for (const directory of searchPath.split(delimiter)) {
    if (!directory) {
      continue;
    }
    const candidate = path.join(directory, executable);
    if (exists(candidate)) {
      return candidate;
    }
  }
  return undefined;
}
