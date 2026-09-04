import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { C4View } from "@vistalith/client";
import { VistalithClient } from "@vistalith/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { C4ViewPanel } from "../src/components/C4ViewPanel.tsx";
import { ChatPanel } from "../src/components/ChatPanel.tsx";
import { resetSelection } from "./helpers.ts";

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

const C4: C4View = {
  revision: 5,
  systems: [
    {
      identity: "arch:system:payments",
      name: "Payments",
      authority: "authoritative",
      deprecated: false,
    },
  ],
  containers: [
    {
      identity: "arch:container:payment-service",
      name: "Payment Service",
      description: "Processes payments.",
      authority: "authoritative",
      deprecated: false,
    },
  ],
  components: [],
  relationships: [
    {
      source: "arch:system:payments",
      target: "arch:container:payment-service",
      kind: "contains",
      authority: "authoritative",
    },
  ],
};

afterEach(cleanup);

beforeEach(async () => {
  await resetSelection();
});

describe("C4ViewPanel", () => {
  it("renders projected elements and relationships", () => {
    render(<C4ViewPanel view={C4} />);
    expect(screen.getByText("Payment Service")).toBeInTheDocument();
    expect(screen.getByText("Payments")).toBeInTheDocument();
    expect(screen.getByText(/-contains->/)).toBeInTheDocument();
  });

  it("selects elements by SubjectRef so lenses share identity", async () => {
    const { useSelection } = await import("../src/state/selection.ts");
    render(<C4ViewPanel view={C4} />);
    fireEvent.click(screen.getByText("Payment Service"));
    expect(useSelection.getState().selected).toEqual({
      namespace: "arch",
      kind: "container",
      id: "payment-service",
    });
  });
});

describe("ChatPanel", () => {
  type Handler = [string, unknown] | [string, unknown, number];

  function chatClient(handlers: Handler[]): VistalithClient {
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

  it("loads threads, sends a message and renders the turn", async () => {
    let threadView = {
      thread: { thread: "agentic:thread:t1", title: "chat x", turns: 1 },
      messages: [
        { message: "m1", role: "user", content: "hello", turn: 1 },
        { message: "m2", role: "assistant", content: "hi there", turn: 1 },
      ],
    };
    const client = chatClient([
      [
        "GET /threads",
        {
          threads: [{ thread: "agentic:thread:t1", title: "chat x", turns: 1 }],
        },
      ],
      ["POST /threads", { thread: "agentic:thread:t1" }, 201],
      ["GET /threads/t1", () => threadView],
      [
        "POST /threads/t1/messages",
        () => {
          // Server-side state advances: the refetched view includes turn 2.
          threadView = {
            thread: { thread: "agentic:thread:t1", title: "chat x", turns: 2 },
            messages: [
              ...threadView.messages,
              { message: "m3", role: "user", content: "hello again", turn: 2 },
              { message: "m4", role: "assistant", content: "answer", turn: 2 },
            ],
          };
          return {
            thread: "agentic:thread:t1",
            message: "agentic:message:m4",
            turn: 2,
            content: "answer",
            usage: { input_tokens: 2, output_tokens: 3, total_tokens: 5 },
          };
        },
      ],
    ]);

    render(<ChatPanel client={client} />);

    // No threads yet -> create one.
    fireEvent.click(screen.getByText("+ new thread"));
    await waitFor(() => screen.getByText("chat x"));

    // The optimistic user message appears immediately.
    fireEvent.change(screen.getByLabelText("chat message"), {
      target: { value: "hello again" },
    });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() => screen.getByText("hello again"));
    await waitFor(() => screen.getByText("answer"));

    expect(screen.getByText("hi there")).toBeInTheDocument();
  });
});
