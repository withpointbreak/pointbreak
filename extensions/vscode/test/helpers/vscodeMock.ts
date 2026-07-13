export interface MockWorkspaceFolder {
  name: string;
  uri: { fsPath: string };
}

export function workspaceFolder(
  path: string,
  name = path,
): MockWorkspaceFolder {
  return { name, uri: { fsPath: path } };
}
