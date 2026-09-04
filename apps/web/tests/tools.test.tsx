import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ToolsPanel } from "../src/components/ToolsPanel.tsx";

afterEach(cleanup);

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

describe("ToolsPanel (SPEC-009)", () => {
  type Handler = [string, unknown] | [string, unknown, number];

  function toolsClient(handlers: Handler[]): VistalithClient {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input)).pathname;
      const key = `${init?.method ?? "GET"} ${path}`;
      const entry = handlers.find(([prefix]) => key === prefix);
      const [prefix, body, status = 200] =
        entry ??
        ((): [string, unknown] => {
          throw new Error(`unexpected fetch: ${key}`);
        })();
      void prefix;
      const payload =
        typeof body === "function" ? (body as () => unknown)() : body;
      return Promise.resolve(jsonResponse(status, payload));
    });
    return new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  }

  it("lists the unified catalog with sources and permission badges", async () => {
    const client = toolsClient([
      [
        "GET /tools",
        {
          tools: [
            {
              id: "graph_search",
              description: "search the graph",
              consequence: "readonly",
              source: { kind: "native" },
              parameters: { type: "object" },
              permission: "allow",
              grant_remaining: 0,
            },
            {
              id: "mcp_echo_echo",
              description: "echoes messages",
              consequence: "readonly",
              source: { kind: "mcp", server: "echo" },
              parameters: { type: "object" },
              permission: "allow",
              grant_remaining: 0,
            },
            {
              id: "mcp_echo_append_note",
              description: "appends a note",
              consequence: "write",
              source: { kind: "mcp", server: "echo" },
              parameters: { type: "object" },
              permission: "ask",
              grant_remaining: 0,
            },
          ],
          grants: [],
        },
      ],
    ]);

    render(<ToolsPanel client={client} />);

    await waitFor(() => screen.getByText("graph_search"));
    expect(screen.getAllByText("mcp:echo")).toHaveLength(2);
    // ask-class tools surface the explicit grant action; allow-class don't.
    expect(
      screen.getByRole("button", { name: "grant mcp_echo_append_note" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "grant graph_search" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("perm-graph_search")).toHaveTextContent("allow");
    expect(screen.getByTestId("perm-mcp_echo_append_note")).toHaveTextContent(
      "ask",
    );
  });

  it("granting flips the permission badge and offers revoke", async () => {
    // Server state advances when the grant POST lands; the catalog GET is
    // served from that mutable state (like the real backend).
    let catalog = {
      tools: [
        {
          id: "mcp_echo_append_note",
          description: "appends a note",
          consequence: "write",
          source: { kind: "mcp", server: "echo" },
          parameters: { type: "object" },
          permission: "ask",
          grant_remaining: 0,
        },
      ],
      grants: [] as { tool: string; remaining: number }[],
    };
    const client = toolsClient([
      ["GET /tools", () => catalog],
      [
        "POST /tools/mcp_echo_append_note/grant",
        () => {
          catalog = {
            tools: [
              {
                id: "mcp_echo_append_note",
                description: "appends a note",
                consequence: "write",
                source: { kind: "mcp", server: "echo" },
                parameters: { type: "object" },
                permission: "allow",
                grant_remaining: 1,
              },
            ],
            grants: [{ tool: "mcp_echo_append_note", remaining: 1 }],
          };
          return { tool: "mcp_echo_append_note", remaining: 1 };
        },
      ],
    ]);

    render(<ToolsPanel client={client} />);
    await waitFor(() =>
      screen.getByRole("button", { name: "grant mcp_echo_append_note" }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "grant mcp_echo_append_note" }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("perm-mcp_echo_append_note")).toHaveTextContent(
        "allow (1)",
      ),
    );
    expect(
      screen.getByRole("button", { name: "revoke mcp_echo_append_note" }),
    ).toBeInTheDocument();
  });
});
