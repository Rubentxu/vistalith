import type {
  McpServerInfo,
  ToolInfo,
  ToolsCatalog,
  VistalithClient,
} from "@vistalith/client";
import { useCallback, useEffect, useState } from "react";
import { client as defaultClient } from "../api.ts";

function sourceLabel(tool: ToolInfo): string {
  return tool.source.kind === "mcp" ? `mcp:${tool.source.server}` : "native";
}

/**
 * Unified tool catalog (SPEC-009): native and MCP tools with their
 * consequence class and the current permission decision. Ask-class tools
 * run only with a scoped temporary grant; granting here is an explicit
 * human act, revoke takes it back.
 */
export function ToolsPanel({
  client = defaultClient,
}: {
  client?: VistalithClient;
}) {
  const [catalog, setCatalog] = useState<ToolsCatalog | null>(null);
  const [servers, setServers] = useState<McpServerInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [syncNote, setSyncNote] = useState<string | null>(null);

  const reloadServers = useCallback(async () => {
    try {
      setServers(await client.mcpServers());
    } catch {
      setServers([]);
    }
  }, [client]);

  useEffect(() => {
    void reloadServers();
  }, [reloadServers]);

  const reload = useCallback(async () => {
    try {
      setCatalog(await client.tools());
    } catch (err) {
      setError(String(err));
    }
  }, [client]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // M6: project the SDDK ledger into the SWG.
  const syncSddk = async () => {
    setBusy(true);
    setError(null);
    try {
      const report = await client.syncSddkWorkflow();
      setSyncNote(
        `synced: +${report.subjects_created} created, ~${report.subjects_updated} updated, ${report.events_skipped} skipped`,
      );
    } catch (err) {
      setSyncNote(String(err));
    } finally {
      setBusy(false);
    }
  };

  const serverAction = async (
    name: string,
    action: "refresh" | "disable" | "enable",
  ) => {
    setBusy(true);
    setError(null);
    try {
      if (action === "refresh") await client.refreshMcpServer(name);
      if (action === "disable") await client.disableMcpServer(name);
      if (action === "enable") await client.enableMcpServer(name);
      await reloadServers();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const grant = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await client.grantTool(id, 1);
      await reload();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await client.revokeTool(id);
      await reload();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="tools-panel" data-testid="tools-panel">
      <h3>tools</h3>
      <button
        type="button"
        className="sddk-sync"
        aria-label="sync SDDK workflow"
        disabled={busy}
        onClick={() => void syncSddk()}
      >
        ⟳ sync SDDK workflow
      </button>
      {syncNote ? <p className="context-reason">{syncNote}</p> : null}
      {servers.length > 0 ? (
        <div className="mcp-servers" data-testid="mcp-servers">
          <h3>mcp servers</h3>
          <ul className="mcp-server-list">
            {servers.map((server) => (
              <li key={server.name} className="mcp-server">
                <div className="tools-row">
                  <code>{server.name}</code>
                  <span
                    className={`badge ${server.status === "connected" ? "badge-live" : "badge-deprecated"}`}
                  >
                    {server.status}
                  </span>
                  {server.disabled ? (
                    <span className="badge badge-deprecated">disabled</span>
                  ) : null}
                  {server.auth ? (
                    <span
                      className="badge badge-advisory"
                      title="authenticated server (secret never leaves vistalithd)"
                    >
                      🔒 {server.auth}
                    </span>
                  ) : null}
                  <span className="tools-source">{server.tools} tools</span>
                </div>
                <div className="mcp-server-actions">
                  <button
                    type="button"
                    disabled={busy}
                    aria-label={`refresh ${server.name}`}
                    onClick={() => void serverAction(server.name, "refresh")}
                  >
                    ⟳ refresh
                  </button>
                  {server.disabled ? (
                    <button
                      type="button"
                      disabled={busy}
                      aria-label={`enable ${server.name}`}
                      onClick={() => void serverAction(server.name, "enable")}
                    >
                      enable
                    </button>
                  ) : (
                    <button
                      type="button"
                      disabled={busy}
                      aria-label={`disable ${server.name}`}
                      onClick={() => void serverAction(server.name, "disable")}
                    >
                      disable
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {error ? <p className="chat-error">{error}</p> : null}
      <ul className="tools-list">
        {(catalog?.tools ?? []).map((tool) => (
          <li key={tool.id} className="tools-item">
            <div className="tools-row">
              <code className="tools-id">{tool.id}</code>
              <span className={`badge tools-consequence-${tool.consequence}`}>
                {tool.consequence}
              </span>
              <span className="tools-source">{sourceLabel(tool)}</span>
              <span
                className={`badge tools-perm-${tool.permission}`}
                data-testid={`perm-${tool.id}`}
              >
                {tool.permission}
                {tool.grant_remaining > 0 ? ` (${tool.grant_remaining})` : ""}
              </span>
            </div>
            <p className="tools-description">{tool.description}</p>
            {tool.permission === "ask" ? (
              <button
                type="button"
                className="tools-grant"
                disabled={busy}
                aria-label={`grant ${tool.id}`}
                onClick={() => void grant(tool.id)}
              >
                grant 1 call
              </button>
            ) : null}
            {tool.grant_remaining > 0 ? (
              <button
                type="button"
                className="tools-revoke"
                disabled={busy}
                aria-label={`revoke ${tool.id}`}
                onClick={() => void revoke(tool.id)}
              >
                revoke
              </button>
            ) : null}
          </li>
        ))}
        {catalog && catalog.tools.length === 0 ? (
          <li className="empty">no tools in the catalog</li>
        ) : null}
        {!catalog ? <li className="empty">loading catalog…</li> : null}
      </ul>
    </div>
  );
}
