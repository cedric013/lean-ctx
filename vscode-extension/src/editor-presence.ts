import * as vscode from "vscode";
import { runLeanCtx } from "./leanctx";

const HEARTBEAT_INTERVAL_MS = 60_000;
const SOURCE = "vscode";

type PresenceEvent = "open" | "heartbeat" | "close";

function presenceEnabled(): boolean {
  return vscode.workspace
    .getConfiguration("leanctx")
    .get<boolean>("editorPresence.enabled", true);
}

function currentWorkspace(): string | undefined {
  return (
    vscode.workspace.workspaceFile?.fsPath ??
    vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
  );
}

function sendPresence(
  event: PresenceEvent,
  workspace: string,
  sessionId: string
): void {
  void runLeanCtx([
    "editor-session",
    "--event",
    event,
    "--source",
    SOURCE,
    "--workspace",
    workspace,
    "--session-id",
    sessionId,
  ]).catch(() => {
    /* Missing or older binaries remain compatible and expire stale presence. */
  });
}

export function registerEditorPresence(
  context: vscode.ExtensionContext
): void {
  const sessionId = `${vscode.env.sessionId}:${process.pid}`;
  let activeWorkspace: string | undefined;

  const syncWorkspace = (): void => {
    const nextWorkspace = presenceEnabled() ? currentWorkspace() : undefined;
    if (nextWorkspace === activeWorkspace) {
      return;
    }
    if (activeWorkspace) {
      sendPresence("close", activeWorkspace, sessionId);
    }
    activeWorkspace = nextWorkspace;
    if (activeWorkspace) {
      sendPresence("open", activeWorkspace, sessionId);
    }
  };

  syncWorkspace();
  const heartbeatTimer = setInterval(() => {
    if (activeWorkspace) {
      sendPresence("heartbeat", activeWorkspace, sessionId);
    }
  }, HEARTBEAT_INTERVAL_MS);

  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(syncWorkspace),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("leanctx.editorPresence.enabled")) {
        syncWorkspace();
      }
    }),
    {
      dispose: () => {
        clearInterval(heartbeatTimer);
        if (activeWorkspace) {
          sendPresence("close", activeWorkspace, sessionId);
          activeWorkspace = undefined;
        }
      },
    }
  );
}
