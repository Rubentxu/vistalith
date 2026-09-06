import { describe, expect, it } from "vitest";
import type { LayoutEdge, LayoutNode } from "../src/flow/layout.ts";
import { runElkLayout } from "../src/flow/layout.ts";

describe("runElkLayout (slice 21, SPK-010)", () => {
  const nodes: LayoutNode[] = [
    { id: "work:workflow:flow-1", label: "flow-1", kind: "workflow" },
    { id: "work:workflow-node:step-1", label: "step-1", kind: "workflow-node" },
    { id: "work:workflow-node:step-2", label: "step-2", kind: "workflow-node" },
    { id: "work:workflow-run:run-1", label: "run-1", kind: "workflow-run" },
  ];
  const edges: LayoutEdge[] = [
    {
      id: "work:workflow:flow-1-contains-work:workflow-node:step-1",
      source: "work:workflow:flow-1",
      target: "work:workflow-node:step-1",
      kind: "contains",
    },
    {
      id: "work:workflow-node:step-2-depends_on-work:workflow-node:step-1",
      source: "work:workflow-node:step-2",
      target: "work:workflow-node:step-1",
      kind: "depends_on",
    },
  ];

  it("assigns a position to every node", async () => {
    const result = await runElkLayout(nodes, edges);
    expect(Object.keys(result.positions).length).toBe(nodes.length);
    for (const node of nodes) {
      expect(result.positions[node.id]).toBeDefined();
    }
    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("is deterministic for the same input", async () => {
    const first = await runElkLayout(nodes, edges);
    const second = await runElkLayout(nodes, edges);
    expect(first.positions).toEqual(second.positions);
  });

  it("separates connected nodes (layered layout has real geometry)", async () => {
    const result = await runElkLayout(nodes, edges);
    const step1 = result.positions["work:workflow-node:step-1"];
    const step2 = result.positions["work:workflow-node:step-2"];
    expect(step1).toBeDefined();
    expect(step2).toBeDefined();
    const distance = Math.hypot(
      (step1?.x ?? 0) - (step2?.x ?? 0),
      (step1?.y ?? 0) - (step2?.y ?? 0),
    );
    expect(distance).toBeGreaterThan(0);
  });

  it("handles an edgeless node set", async () => {
    const result = await runElkLayout(nodes.slice(0, 2), []);
    expect(Object.keys(result.positions).length).toBe(2);
  });
});
