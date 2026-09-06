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

  function sseBody(frames: string[]): ReadableStream<Uint8Array> {
    const encoder = new TextEncoder();
    return new ReadableStream({
      start(controller) {
        for (const frame of frames) {
          controller.enqueue(encoder.encode(frame));
        }
        controller.close();
      },
    });
  }

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
      // SSE frames (arrays) stream through a ReadableStream body.
      if (Array.isArray(payload)) {
        return Promise.resolve({
          status,
          body: sseBody(payload as string[]),
        } as unknown as Response);
      }
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
        "POST /threads/t1/messages/stream",
        () => {
          // Server-side state advances durably (the streamed turn appends
          // the same events), so the refetched view includes turn 2.
          threadView = {
            thread: { thread: "agentic:thread:t1", title: "chat x", turns: 2 },
            messages: [
              ...threadView.messages,
              { message: "m3", role: "user", content: "hello again", turn: 2 },
              { message: "m4", role: "assistant", content: "answer", turn: 2 },
            ],
          };
          return [
            "event: delta\ndata: answer\n\n",
            `event: done\ndata: ${JSON.stringify({
              turn: 2,
              message: "agentic:message:m4",
              content: "answer",
              usage: { input_tokens: 2, output_tokens: 3, total_tokens: 5 },
            })}\n\n`,
          ];
        },
      ],
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

  it("sends model/agent overrides and renders mention chips (slice 23)", async () => {
    let threadView = {
      thread: { thread: "agentic:thread:t9", title: "chat y", turns: 1 },
      messages: [
        {
          message: "m1",
          role: "user",
          content: "check @arch:system:ledger",
          turn: 1,
          mentions: ["arch:system:ledger"],
        },
        { message: "m2", role: "assistant", content: "reviewed", turn: 1 },
      ],
    };
    let sentBody: Record<string, unknown> = {};
    const client = chatClient([
      [
        "GET /threads",
        {
          threads: [{ thread: "agentic:thread:t9", title: "chat y", turns: 1 }],
        },
      ],
      [
        "GET /agents",
        {
          agents: [
            {
              agent: "agentic:agent:rev1",
              role: "reviewer",
              instructions: "be thorough",
              tools: [],
            },
          ],
        },
      ],
      ["POST /threads", { thread: "agentic:thread:t9" }, 201],
      ["GET /threads/t9", () => threadView],
      [
        "POST /threads/t9/messages/stream",
        () => {
          threadView = {
            thread: { thread: "agentic:thread:t9", title: "chat y", turns: 2 },
            messages: [
              ...threadView.messages,
              {
                message: "m3",
                role: "user",
                content: "please review",
                turn: 2,
              },
              { message: "m4", role: "assistant", content: "done", turn: 2 },
            ],
          };
          return [
            `event: done\ndata: ${JSON.stringify({
              turn: 2,
              message: "agentic:message:m4",
              content: "done",
              model: "fake/custom-3",
              mentions: ["arch:system:ledger"],
              unresolved_mentions: [],
              usage: { input_tokens: 2, output_tokens: 3, total_tokens: 5 },
            })}\n\n`,
          ];
        },
      ],
    ]);
    // capture the streamed body via a wrapper around fetch
    const rawFetch = (client as unknown as { fetchImpl: typeof fetch })
      .fetchImpl;
    const wrapped: typeof fetch = (input, init) => {
      if (String(input).endsWith("/stream") && init?.body) {
        sentBody = JSON.parse(String(init.body));
      }
      return rawFetch(input, init);
    };
    (client as unknown as { fetchImpl: typeof fetch }).fetchImpl = wrapped;

    render(<ChatPanel client={client} />);
    await waitFor(() => screen.getByText("chat y"));

    // open the thread (send requires an active thread)
    fireEvent.click(screen.getByText("chat y"));
    await waitFor(() => screen.getByText("check @arch:system:ledger"));

    // selector: pick the agent persona and a model override
    fireEvent.change(screen.getByLabelText("model override"), {
      target: { value: "fake/custom-3" },
    });
    fireEvent.change(screen.getByLabelText("agent persona"), {
      target: { value: "rev1" },
    });
    fireEvent.change(screen.getByLabelText("chat message"), {
      target: { value: "please review" },
    });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() => screen.getByText("done"));

    expect(sentBody.model).toBe("fake/custom-3");
    expect(sentBody.agent).toBe("rev1");

    // mention chips render on the user message (identity, not prose)
    await waitFor(() => screen.getByText("@arch:system:ledger"));
  });
});

describe("ChatPanel fork (SPEC-011)", () => {
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
      return Promise.resolve(jsonResponse(status, body));
    });
    return new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  }

  it("forks the active thread and opens the fork", async () => {
    const client = chatClient([
      [
        "GET /threads",
        {
          threads: [
            {
              thread: "agentic:thread:t1",
              title: "source thread",
              turns: 2,
              forked_from: null,
            },
          ],
        },
      ],
      [
        "POST /threads/t1/fork",
        {
          fork: "agentic:thread:f9",
          source: "agentic:thread:t1",
          up_to_turn: 2,
          copied_events: 4,
        },
        201,
      ],
      [
        "GET /threads/f9",
        {
          thread: {
            thread: "agentic:thread:f9",
            title: "source thread (fork ≤ turn 2)",
            turns: 2,
            forked_from: "agentic:thread:t1",
          },
          messages: [
            {
              message: "fm1",
              role: "user",
              content: "copied question",
              turn: 1,
              forked_of: "agentic:message:m1",
            },
          ],
        },
      ],
    ]);

    render(<ChatPanel client={client} />);
    await waitFor(() => screen.getByText("source thread"));

    fireEvent.click(
      screen.getByRole("button", { name: "fork thread source thread" }),
    );
    // The fork opens with its copied items and their forked_of markers.
    await waitFor(() => screen.getByText("copied question"));
    expect(screen.getByText(/⎇ forked/)).toBeInTheDocument();
    // The copied item carries the forked_of marker.
    expect(screen.getByText("copied question")).toBeInTheDocument();
    expect(screen.getByText(/⎇ forked/)).toBeInTheDocument();
  });
});
