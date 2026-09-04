import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { GraphView } from "../src/components/GraphView.tsx";
import { SubjectDetails } from "../src/components/SubjectDetails.tsx";
import { SubjectList } from "../src/components/SubjectList.tsx";
import { useSelection } from "../src/state/selection.ts";
import {
  graphState,
  relation,
  resetSelection,
  subjectNode,
} from "./helpers.ts";

const GRAPH = graphState(
  [
    subjectNode("arch", "container", "payment-service", {
      provenance: { source: "user:rubentxu" },
      properties: { name: "Payment Service" },
    }),
    subjectNode("code", "repository", "payments-api"),
    subjectNode("sddk", "work-item", "TEST-MODEL-001", {
      authority: "derived",
      provenance: {
        source: "sddk:v1.82.0",
        source_revision: "d43b120b6e67d467033acd61f7f3c286559a97b7",
        confidence: 0.6,
      },
    }),
  ],
  [
    relation(["code", "repository", "payments-api"], "implements", [
      "arch",
      "container",
      "payment-service",
    ]),
    relation(
      ["sddk", "work-item", "TEST-MODEL-001"],
      "affects",
      ["arch", "container", "payment-service"],
      "advisory",
    ),
  ],
);

afterEach(cleanup);

beforeEach(async () => {
  await resetSelection();
});

/** Clicks the subject button identified by its SubjectRef identity string. */
function clickSubject(identity: string): void {
  const button = document.querySelector(
    `[data-identity="${CSS.escape(identity)}"]`,
  );
  if (!button) throw new Error(`no element for ${identity}`);
  fireEvent.click(button);
}

describe("cross-lens selection (SubjectRefs, not renderer ids)", () => {
  it("selecting in the list is visible in the details lens", () => {
    render(
      <>
        <SubjectList graph={GRAPH} />
        <SubjectDetails graph={GRAPH} />
      </>,
    );

    clickSubject("sddk:work-item:TEST-MODEL-001");

    expect(
      screen.getByText("sddk:work-item:TEST-MODEL-001"),
    ).toBeInTheDocument();
    expect(screen.getByText("derived")).toBeInTheDocument();
    expect(
      screen.getByText("d43b120b6e67d467033acd61f7f3c286559a97b7"),
    ).toBeInTheDocument();
    expect(screen.getByText("0.6")).toBeInTheDocument();
  });

  it("selecting a graph node uses the SubjectRef identity", () => {
    render(<GraphView graph={GRAPH} />);

    const firstNode = screen.getAllByTestId("graph-node")[0];
    expect(firstNode).toBeDefined();
    const rect = firstNode?.querySelector("rect");
    if (!rect) throw new Error("graph node has no rect child");
    fireEvent.click(rect);

    expect(useSelection.getState().selected).toEqual({
      namespace: "arch",
      kind: "container",
      id: "payment-service",
    });
  });

  it("following a relation endpoint re-selects that SubjectRef", () => {
    render(<SubjectDetails graph={GRAPH} />);
    act(() =>
      useSelection
        .getState()
        .select({ namespace: "sddk", kind: "work-item", id: "TEST-MODEL-001" }),
    );

    clickSubject("arch:container:payment-service");

    expect(useSelection.getState().selected).toEqual({
      namespace: "arch",
      kind: "container",
      id: "payment-service",
    });
  });

  it("toggle clears when the same subject is selected twice", () => {
    render(<SubjectList graph={GRAPH} />);
    clickSubject("arch:container:payment-service");
    clickSubject("arch:container:payment-service");
    expect(useSelection.getState().selected).toBeNull();
  });
});

describe("SubjectList", () => {
  it("groups by namespace and filters by identity substring", () => {
    render(<SubjectList graph={GRAPH} />);
    expect(screen.getByText("arch")).toBeInTheDocument();
    expect(screen.getByText("code")).toBeInTheDocument();
    expect(screen.getByText("sddk")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("filter subjects"), {
      target: { value: "payments-api" },
    });
    expect(screen.queryByText("sddk")).not.toBeInTheDocument();
    expect(screen.getByText("payments-api")).toBeInTheDocument();
  });

  it("shows an empty state when nothing matches", () => {
    render(<SubjectList graph={GRAPH} />);
    fireEvent.change(screen.getByLabelText("filter subjects"), {
      target: { value: "zzz-nothing" },
    });
    expect(screen.getByText("no subjects match")).toBeInTheDocument();
  });
});

describe("GraphView", () => {
  it("renders nodes and edge kinds as svg text", () => {
    const { container } = render(<GraphView graph={GRAPH} />);
    expect(
      container.querySelectorAll("[data-testid='graph-node']"),
    ).toHaveLength(3);
    expect(screen.getByText("implements")).toBeInTheDocument();
    expect(screen.getByText("affects")).toBeInTheDocument();
  });

  it("marks the selected node, keeps neighbors lit and dims the rest", () => {
    render(<GraphView graph={GRAPH} />);
    act(() =>
      useSelection
        .getState()
        .select({ namespace: "sddk", kind: "work-item", id: "TEST-MODEL-001" }),
    );

    const nodes = screen.getAllByTestId("graph-node");
    const byIdentity = (identity: string) => {
      const found = nodes.find(
        (n) => n.getAttribute("data-identity") === identity,
      );
      if (!found) throw new Error(`missing graph node: ${identity}`);
      return found;
    };

    expect(
      byIdentity("sddk:work-item:TEST-MODEL-001").getAttribute("class"),
    ).toContain("node-selected");
    // Adjacent subject (via the advisory `affects` edge) stays fully visible.
    expect(
      byIdentity("arch:container:payment-service").getAttribute("class"),
    ).not.toContain("node-dim");
    // Unrelated subject is dimmed.
    expect(
      byIdentity("code:repository:payments-api").getAttribute("class"),
    ).toContain("node-dim");
  });
});

describe("SubjectDetails", () => {
  it("shows an empty state without selection", () => {
    render(<SubjectDetails graph={GRAPH} />);
    expect(screen.getByText(/select a subject/i)).toBeInTheDocument();
  });

  it("warns when the selected ref is not in the current revision", () => {
    render(<SubjectDetails graph={GRAPH} />);
    act(() =>
      useSelection
        .getState()
        .select({ namespace: "visual", kind: "hypothesis", id: "gone" }),
    );
    expect(
      screen.getByText(/is not in the current graph revision/),
    ).toBeInTheDocument();
  });
});
