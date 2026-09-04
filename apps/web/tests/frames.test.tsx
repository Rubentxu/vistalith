import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FramesPanel } from "../src/components/FramesPanel.tsx";

afterEach(cleanup);

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

describe("FramesPanel (slice 8)", () => {
  type Handler = [string, unknown] | [string, unknown, number];

  function framesClient(handlers: Handler[]): VistalithClient {
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

  it("creates a frame from the goal input and lists it with bounds", async () => {
    let frames: unknown = { frames: [] };
    const client = framesClient([
      ["GET /frames", () => frames],
      [
        "POST /frames",
        () => {
          frames = {
            frames: [
              {
                frame: "agentic:frame:f1",
                goal: "assess impact",
                status: "open",
                turns: 0,
                max_turns: 5,
                used_tokens: 0,
                token_budget: 8000,
                permitted_tools: ["graph_search"],
              },
            ],
          };
          return { frame: "agentic:frame:f1", thread: "agentic:thread:t1" };
        },
        201,
      ],
      [
        "GET /frames/f1",
        {
          frame: {
            frame: "agentic:frame:f1",
            goal: "assess impact",
            status: "open",
            turns: 0,
            max_turns: 5,
            used_tokens: 0,
            token_budget: 8000,
            permitted_tools: ["graph_search"],
          },
          messages: [],
        },
      ],
    ]);

    render(<FramesPanel client={client} />);
    fireEvent.change(screen.getByLabelText("frame goal"), {
      target: { value: "assess impact" },
    });
    fireEvent.click(screen.getByRole("button", { name: "+ frame" }));

    await waitFor(() => screen.getByText("assess impact"));
    expect(screen.getByText(/open · 0\/5 turns/)).toBeInTheDocument();
  });

  it("runs a bounded turn and refreshes the frame thread", async () => {
    let messages: { role: string; content: string }[] = [];
    const client = framesClient([
      [
        "GET /frames",
        () => ({
          frames: [
            {
              frame: "agentic:frame:f1",
              goal: "assess impact",
              status: "open",
              turns: 1,
              max_turns: 5,
              used_tokens: 120,
              token_budget: 8000,
              permitted_tools: [],
            },
          ],
        }),
      ],
      [
        "GET /frames/f1",
        () => ({
          frame: {
            frame: "agentic:frame:f1",
            goal: "assess impact",
            status: "open",
            turns: 1,
            max_turns: 5,
            used_tokens: 120,
            token_budget: 8000,
            permitted_tools: [],
          },
          messages,
        }),
      ],
      [
        "POST /frames/f1/turns",
        () => {
          messages = [
            ...messages,
            { role: "user", content: "run the assessment" },
            { role: "assistant", content: "frame answer" },
          ];
          return {
            frame: "agentic:frame:f1",
            turn: 1,
            content: { turns_used: 1, used_tokens: 120 },
            auto_closed: null,
          };
        },
      ],
    ]);

    render(<FramesPanel client={client} />);
    await waitFor(() => screen.getByText("assess impact"));

    fireEvent.click(screen.getByText("assess impact"));
    await waitFor(() =>
      screen.getByText("select a frame and run a bounded turn"),
    );

    fireEvent.change(screen.getByLabelText("frame turn"), {
      target: { value: "run the assessment" },
    });
    fireEvent.click(screen.getByRole("button", { name: "turn" }));
    await waitFor(() => screen.getByText("frame answer"));
  });
});
