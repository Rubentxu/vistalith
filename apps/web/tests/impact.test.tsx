import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { parseSubjectRef, VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ImpactPanel } from "../src/components/ImpactPanel.tsx";

afterEach(cleanup);

function withClient(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

describe("ImpactPanel (slice 7)", () => {
  it("shows the transitive impact set of the selected subject", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL) => {
      const url = new URL(String(input));
      if (url.pathname === "/algorithms/impact/arch/container/ledger") {
        return Promise.resolve(
          jsonResponse(200, {
            root: "arch:container:ledger",
            impacted: [
              "arch:container:payment-service",
              "arch:container:gateway",
            ],
          }),
        );
      }
      return Promise.resolve(jsonResponse(404, { error: "not found" }));
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });

    withClient(
      <ImpactPanel
        selected={parseSubjectRef("arch:container:ledger")}
        client={client}
      />,
    );
    await waitFor(() => screen.getByText(/2 subjects transitively impacted/));
    expect(
      screen.getByText("arch:container:payment-service"),
    ).toBeInTheDocument();
    expect(screen.getByText("arch:container:gateway")).toBeInTheDocument();
  });

  it("builds a context view whose provenance explains inclusions", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input));
      if (url.pathname === "/views/context" && init?.method === "POST") {
        const request = JSON.parse(String(init.body));
        expect(request.roots).toEqual(["arch:container:ledger"]);
        return Promise.resolve(
          jsonResponse(200, {
            roots: ["arch:container:ledger"],
            items: [
              {
                subject: "arch:container:ledger",
                authority: "authoritative",
                depth: 0,
                reason: { reason: "root" },
                properties: {},
                last_event_sequence: 1,
                last_touch: "2026-09-04T12:00:00Z",
                last_actor: "user:ruben",
                estimated_tokens: 18,
              },
              {
                subject: "arch:container:payment-service",
                authority: "authoritative",
                depth: 1,
                reason: {
                  reason: "via",
                  from: "arch:container:ledger",
                  kind: "depends_on",
                  depth: 1,
                },
                properties: {},
                last_event_sequence: 2,
                last_touch: "2026-09-04T12:00:00Z",
                last_actor: "user:ruben",
                estimated_tokens: 24,
              },
            ],
            exclusions: [],
            estimated_tokens: 42,
            token_budget: 4000,
            truncated: false,
          }),
        );
      }
      return Promise.resolve(jsonResponse(200, { root: "x", impacted: [] }));
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });

    withClient(
      <ImpactPanel
        selected={parseSubjectRef("arch:container:ledger")}
        client={client}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "build context view" }));
    await waitFor(() => screen.getByTestId("context-view"));
    expect(screen.getByText(/2 items · ~42\/4000 tokens/)).toBeInTheDocument();
    expect(screen.getByText(/^root · \d+ tok$/)).toBeInTheDocument();
    expect(screen.getByText(/via depends_on \(depth 1\)/)).toBeInTheDocument();
  });
});
