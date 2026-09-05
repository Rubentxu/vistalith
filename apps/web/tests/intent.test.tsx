import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { SubjectRef } from "@vistalith/client";
import { parseSubjectRef, VistalithClient } from "@vistalith/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IntentComposer } from "../src/components/IntentComposer.tsx";
import { useSelection } from "../src/state/selection.ts";
import { resetSelection } from "./helpers.ts";

type Handler = [string, unknown] | [string, unknown, number];

function clientWith(handlers: Handler[]): VistalithClient {
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
    return Promise.resolve({
      status,
      text: async () => JSON.stringify(payload),
    } as unknown as Response);
  });
  return new VistalithClient({
    baseUrl: "http://127.0.0.1:7420",
    fetchImpl: fetchImpl as unknown as typeof fetch,
  });
}

const TARGET: SubjectRef = {
  namespace: "arch",
  kind: "container",
  id: "payment-service",
};

afterEach(cleanup);

beforeEach(async () => {
  await resetSelection();
});

describe("IntentComposer (SPEC-006 lifecycle)", () => {
  it("renders nothing without a selection", () => {
    const { container } = render(
      <IntentComposer client={clientWith([])} selected={null} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("drafts, previews and promotes to a graph patch", async () => {
    const onGraphChanged = vi.fn();
    const client = clientWith([
      [
        "POST /intents",
        { intent: "agentic:visual-proposal:i1", base_revision: 6 },
        201,
      ],
      ["POST /intents/i1/promote", { outcome: "applied", revision: 7 }],
    ]);
    act(() => useSelection.getState().select(TARGET));
    render(
      <IntentComposer
        client={client}
        selected={TARGET}
        onGraphChanged={onGraphChanged}
      />,
    );

    // Draft the rename gesture.
    fireEvent.change(screen.getByLabelText("intent new name"), {
      target: { value: "Payments Service" },
    });
    fireEvent.click(screen.getByRole("button", { name: "propose draft" }));

    // Preview appears, stale-aware and fresh.
    const preview = await waitFor(() => screen.getByTestId("intent-preview"));
    expect(preview).toHaveTextContent("base revision 6");
    expect(preview).toHaveTextContent("fresh");

    // Explicit promotion applies and signals the graph to refresh.
    fireEvent.click(screen.getByRole("button", { name: "promote" }));
    await waitFor(() => screen.getByTestId("intent-outcome"));
    expect(screen.getByTestId("intent-outcome")).toHaveTextContent(
      "applied at revision 7",
    );
    expect(onGraphChanged).toHaveBeenCalled();
  });

  it("shows the governance route for SDDK-owned targets", async () => {
    const client = clientWith([
      [
        "POST /intents",
        { intent: "agentic:visual-proposal:i2", base_revision: 6 },
        201,
      ],
      [
        "POST /intents/i2/promote",
        {
          outcome: "sddk-governed",
          subject: "sddk:work-item:TEST-MODEL-001",
        },
      ],
    ]);
    const sddkTarget: SubjectRef = {
      namespace: "sddk",
      kind: "work-item",
      id: "TEST-MODEL-001",
    };
    act(() => useSelection.getState().select(sddkTarget));
    render(<IntentComposer client={client} selected={sddkTarget} />);

    fireEvent.change(screen.getByLabelText("intent new name"), {
      target: { value: "hijack" },
    });
    fireEvent.click(screen.getByRole("button", { name: "propose draft" }));
    await waitFor(() => screen.getByTestId("intent-preview"));
    fireEvent.click(screen.getByRole("button", { name: "promote" }));

    await waitFor(() => screen.getByTestId("intent-outcome"));
    expect(screen.getByTestId("intent-outcome")).toHaveTextContent(
      "routed to SDDK governance: sddk:work-item:TEST-MODEL-001",
    );
  });

  it("requires a name for rename gestures", async () => {
    const client = clientWith([]);
    act(() => useSelection.getState().select(TARGET));
    render(<IntentComposer client={client} selected={TARGET} />);

    fireEvent.click(screen.getByRole("button", { name: "propose draft" }));
    expect(screen.getByText("give the new name first")).toBeInTheDocument();
  });
});

// --- Governed SDDK promotion (slice 9, SPK-012) ---

import type { IntentDetail } from "@vistalith/client";

describe("IntentComposer governed SDDK promotion (SPEC-012)", () => {
  function jsonResponse(status: number, body: unknown): Response {
    return {
      status,
      text: async () => JSON.stringify(body),
    } as unknown as Response;
  }

  function sddkClient(handlers: {
    draft?: unknown;
    promote: unknown;
    detail?: unknown;
  }): VistalithClient {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input)).pathname;
      const key = `${init?.method ?? "GET"} ${path}`;
      const status = key === "POST /intents" ? 201 : 200;
      const body =
        key === "POST /intents"
          ? handlers.draft
          : key === "GET /intents/i1"
            ? handlers.detail
            : key === "POST /intents/i1/promote"
              ? handlers.promote
              : (() => {
                  throw new Error(`unexpected fetch: ${key}`);
                })();
      return Promise.resolve(jsonResponse(status, body));
    });
    return new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  }

  it("renders the sddk proposal decision and receipt", async () => {
    const detail: IntentDetail = {
      summary: {
        intent: "agentic:visual-proposal:i1",
        target: "sddk:work-item:TEST-1",
        gesture: "annotate",
        status: "draft",
        base_revision: 6,
        stale: false,
      },
      change: { operations: [] },
      current_revision: 6,
    };
    const client = sddkClient({
      draft: {
        intent: "agentic:visual-proposal:i1",
        base_revision: 6,
      },
      detail,
      promote: {
        outcome: "submitted-to-sddk",
        subject: "sddk:work-item:TEST-1",
        proposal: "vistalith:proposal:p1",
        decision: "allowed",
        receipt_id: "cap-evidence-write-abc123def456",
      },
    });

    render(
      <IntentComposer
        client={client}
        selected={parseSubjectRef("sddk:work-item:TEST-1")}
      />,
    );

    // Draft the intent (annotate gesture: no-op draft), then promote it:
    // the server routes through the SDDK gateway and answers with the
    // decision + receipt.
    fireEvent.change(screen.getByLabelText("intent gesture"), {
      target: { value: "annotate" },
    });
    fireEvent.click(screen.getByRole("button", { name: "propose draft" }));
    await waitFor(() => screen.getByText(/base revision 6/));
    fireEvent.click(screen.getByRole("button", { name: "promote" }));
    await waitFor(() => screen.getByText(/submitted to SDDK \(allowed\)/));
    expect(screen.getByText(/receipt cap-evidence-write/)).toBeInTheDocument();
  });
});
