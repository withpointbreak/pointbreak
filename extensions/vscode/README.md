# Pointbreak

Pointbreak Review companion for Visual Studio Code. In development.

## Local dogfood

From the repository root, build a host-specific VSIX with a bundled release CLI:

```sh
just extension-package
code --install-extension extensions/vscode/pointbreak-*.vsix
```

Open a Git worktree with a Pointbreak store, then open the Pointbreak activity-bar view. The
extension checks the bundled CLI handshake before it reads review data.

The Capture command prompts for an optional short summary after the source choices. Summaries are
passed to `pointbreak capture --summary` and become the primary labels in Recent revisions and every
revision picker; legacy captures without a summary continue to fall back to their short revision ID.

To use a development build without repackaging, set `pointbreak.binaryPath` to the absolute path
of that Pointbreak binary and reload the extension host. The configured file may have any basename,
but it must provide the exact compatible `pointbreak.version` handshake.

## Recommend the extension in a repository

A repository that reviews with Pointbreak can recommend the extension to everyone who opens it in
VS Code with a workspace recommendation in `.vscode/extensions.json` at the repository root:

```json
{
  "recommendations": ["pointbreak.pointbreak"]
}
```

If the file already exists, add `"pointbreak.pointbreak"` to its `recommendations` array. VS Code
offers to install recommended extensions when the folder is opened and lists them under
**Extensions: Show Recommended Extensions**. The recommendation is advisory: it installs nothing on
its own and never requires the extension. It names the extension by its identifier
(`<publisher>.<name>` from `package.json`), so an extension installed from a locally built VSIX
satisfies it the same way a Marketplace install does.

This repository's own [`.vscode/extensions.json`](../../.vscode/extensions.json) is the working
example.
