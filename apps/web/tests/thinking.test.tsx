import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ThinkingPanel } from "../src/components/ThinkingPanel.tsx";

afterEach(cleanup);

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

describe("ThinkingPanel (slice 17, VISUAL-THINKING.md)", () => {
  function canvasClient(handlers: {
    list: unknown;
    create?: unknown;
    promote?: unknown;
  }): VistalithClient {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input)).pathname;
      const key = `${init?.method ?? "GET"} ${path}`;
      const raw =
        key === "GET /canvas/subjects"
          ? handlers.list
          : key === "POST /canvas/subjects"
            ? handlers.create
            : key.includes("/promote")
              ? handlers.promote
              : (() => {
                  throw new Error(`unexpected fetch: ${key}`);
                })();
      const payload =
        typeof raw === "function" ? (raw as () => unknown)() : raw;
      const status = key === "GET /canvas/subjects" ? 200 : 201;
      return Promise.resolve(jsonResponse(status, payload));
    });
    return new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  }

  it("creates a primitive and lists it with its attachment", async () => {
    let list: unknown = { subjects: [] as unknown[] };
    const client = canvasClient({
      list: () => list,
      create: () => {
        list = {
          subjects: [
            {
              subject: "vistalith:question:q9",
              kind: "question",
              content: "does the gateway need rate limiting?",
              relates_to: "arch:container:gateway",
              authority: "advisory",
              deprecated: false,
            },
          ],
        };
        return {
          subject: "vistalith:question:q9",
          kind: "question",
        };
      },
    });

    render(<ThinkingPanel client={client} />);

    fireEvent.change(screen.getByLabelText("primitive kind"), {
      target: { value: "question" },
    });
    fireEvent.change(screen.getByLabelText("primitive content"), {
      target: { value: "does the gateway need rate limiting?" },
    });
    fireEvent.change(screen.getByLabelText("relates to"), {
      target: { value: "arch:container:gateway" },
    });
    fireEvent.click(screen.getByRole("button", { name: "sketch" }));

    await waitFor(() =>
      screen.getByText("does the gateway need rate limiting?"),
    );
    expect(screen.getByText(/arch:container:gateway/)).toBeInTheDocument();
    const badge = screen.getAllByText(/^question$/);
    expect(badge.length).toBeGreaterThan(0);
  });

  it("promotes a primitive to a VisualIntent draft", async () => {
    const client = canvasClient({
      list: {
        subjects: [
          {
            subject: "vistalith:hypothesis:h1",
            kind: "hypothesis",
            content: "ledger is the bottleneck",
            relates_to: "arch:container:payment-service",
            authority: "advisory",
            deprecated: false,
          },
        ],
      },
      promote: {
        intent: "visual:visual-proposal:i1",
        target: "arch:container:payment-service",
      },
    });

    render(<ThinkingPanel client={client} />);
    await waitFor(() =>
      screen.getByRole("button", { name: /promote hypothesis/ }),
    );

    fireEvent.click(screen.getByRole("button", { name: /promote hypothesis/ }));
    await waitFor(() => screen.getByText(/drafted visual:visual-proposal:i1/));
  });
});
