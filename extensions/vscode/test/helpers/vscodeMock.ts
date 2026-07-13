export interface MockWorkspaceFolder {
  name: string;
  uri: { fsPath: string; toString(): string };
}

export function workspaceFolder(
  path: string,
  name = path,
): MockWorkspaceFolder {
  return {
    name,
    uri: { fsPath: path, toString: () => `file://${path}` },
  };
}
