import { cleanup, render, screen, waitFor } from "@testing-library/react";
import type { GraphState } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FlowPanel } from "../src/components/FlowPanel.tsx";
import * as layout from "../src/flow/layout.ts";

afterEach(cleanup);

// React Flow needs ResizeObserver/DOMMatrixReadOnly, which jsdom lacks.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as never;
globalThis.DOMMatrixReadOnly ??= class {
  m22 = 1;
} as never;

const ref = (identity: string) => {
  const [namespace, kind, id] = identity.split(":");
  return { namespace, kind, id };
};

function graphOf(
  subjects: Array<[string, Record<string, unknown>?]>,
  relations: Array<[string, string, string]>,
): GraphState {
  return {
    revision: 7,
    subjects: subjects.map(([identity, properties]) => ({
      subject: ref(identity),
      authority: "authoritative",
      provenance: { source: "test" },
      properties: properties ?? {},
      deprecated: false,
      last_event_sequence: 1,
    })),
    relations: relations.map(([from, kind, to]) => ({
      relation: { from: ref(from), kind, to: ref(to) },
      authority: "authoritative",
      provenance: { source: "test" },
    })),
  } as unknown as GraphState;
}

vi.mock("../src/flow/layout.ts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/flow/layout.ts")>();
  return {
    ...actual,
    runElkLayout: vi.fn(actual.runElkLayout),
  };
});

const mockedLayout = vi.mocked(layout.runElkLayout);

describe("FlowPanel (slice 21, SPK-010)", () => {
  it("lays out the workflow plane and reports scale stats", async () => {
    const graph = graphOf(
      [
        ["work:workflow:flow-1", { name: "Checkout flow" }],
        ["work:workflow-node:step-1", { status: "completed" }],
        ["work:agent:agent-1", { role: "reviewer" }],
        ["arch:system:unrelated", { name: "not a flow node" }],
      ],
      [
        ["work:workflow:flow-1", "contains", "work:workflow-node:step-1"],
        ["work:agent:agent-1", "executed_by", "work:workflow:flow-1"],
      ],
    );
    render(<FlowPanel graph={graph} />);
    await waitFor(() => screen.getByTestId("flow-panel"));
    expect(screen.getByText(/3 nodes · 2 edges/)).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText(/layout \d+ ms/)).toBeInTheDocument(),
    );
    // only workflow-plane subjects flow; the arch system is filtered out
    expect(screen.queryByText(/not a flow node/)).not.toBeInTheDocument();
  });

  it("does not re-layout on live status updates (positions stable)", async () => {
    const graph = graphOf(
      [
        ["work:workflow:flow-1", { name: "Checkout flow" }],
        ["work:workflow-node:step-1", { status: "pending" }],
      ],
      [["work:workflow:flow-1", "contains", "work:workflow-node:step-1"]],
    );
    const { rerender } = render(<FlowPanel graph={graph} />);
    await waitFor(() => expect(mockedLayout).toHaveBeenCalledTimes(1));

    // the poll brings the same structure with a new status for one node
    const updated = graphOf(
      [
        ["work:workflow:flow-1", { name: "Checkout flow" }],
        ["work:workflow-node:step-1", { status: "completed" }],
      ],
      [["work:workflow:flow-1", "contains", "work:workflow-node:step-1"]],
    );
    updated.revision = 8;
    rerender(<FlowPanel graph={updated} />);
    await waitFor(() =>
      expect(screen.getByText(/· completed/)).toBeInTheDocument(),
    );
    expect(mockedLayout).toHaveBeenCalledTimes(1);

    // adding a NODE changes the structural signature: re-layout
    const grown = graphOf(
      [
        ["work:workflow:flow-1", { name: "Checkout flow" }],
        ["work:workflow-node:step-1", { status: "completed" }],
        ["work:workflow-node:step-2", { status: "pending" }],
      ],
      [
        ["work:workflow:flow-1", "contains", "work:workflow-node:step-1"],
        ["work:workflow:flow-1", "contains", "work:workflow-node:step-2"],
      ],
    );
    grown.revision = 9;
    rerender(<FlowPanel graph={grown} />);
    await waitFor(() => expect(mockedLayout).toHaveBeenCalledTimes(2));
  });

  it("shows the empty state without workflow subjects", () => {
    render(<FlowPanel graph={graphOf([["arch:system:solo", {}]], [])} />);
    expect(screen.getByText(/no workflow\/agent subjects/)).toBeInTheDocument();
  });
});
