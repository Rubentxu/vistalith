import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import type { DecisionsLens } from "@vistalith/client";
import { VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DecisionLensPanel } from "../src/components/DecisionLensPanel.tsx";

afterEach(cleanup);

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

const LENS: DecisionsLens = {
  decisions: [
    {
      decision: "vistalith:decision:d-1",
      question: "vistalith:question:q-1",
      selected: "vistalith:option:option-b",
      rejected: [
        { option: "vistalith:option:option-a", via: "rejected_in_favor_of" },
      ],
      motivated_by: ["arch:container:req-payment"],
      evidence: ["verification:evidence:bench-1"],
      contradicts: [],
      revisits: [],
      deprecated: false,
    },
  ],
};

describe("DecisionLensPanel (slice 13, M9)", () => {
  it("renders the decision chain: question, winner, rejected, evidence", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL) => {
      const path = new URL(String(input)).pathname;
      if (path === "/lens/decisions") {
        return Promise.resolve(jsonResponse(200, LENS));
      }
      throw new Error(`unexpected fetch: ${path}`);
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <DecisionLensPanel client={client} />
      </QueryClientProvider>,
    );

    await waitFor(() => screen.getByText("vistalith:decision:d-1"));
    // The M9 chain renders as an explainable narrative: query by role-scoped
    // definition rows (dt/dd pairs).
    const facts = screen
      .getByText("vistalith:decision:d-1")
      .closest(".decision-entry") as HTMLElement;
    const text = facts.textContent ?? "";
    // The M9 chain renders as an explainable narrative; whitespace is
    // layout-level, so assert on label/value adjacency.
    for (const fact of [
      "question:vistalith:question:q-1",
      "selected:vistalith:option:option-b",
      "rejected:vistalith:option:option-a",
      "motivated by:arch:container:req-payment",
      "evidence:verification:evidence:bench-1",
    ]) {
      expect(text).toContain(fact);
    }
  });

  it("says so when there are no decisions", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL) => {
      const path = new URL(String(input)).pathname;
      if (path === "/lens/decisions") {
        return Promise.resolve(jsonResponse(200, { decisions: [] }));
      }
      throw new Error(`unexpected fetch: ${path}`);
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <DecisionLensPanel client={client} />
      </QueryClientProvider>,
    );
    await waitFor(() => screen.getByText("no decisions in the graph"));
  });
});
