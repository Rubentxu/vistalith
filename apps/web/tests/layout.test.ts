import { describe, expect, it } from "vitest";
import {
  adjacencyOf,
  computeLayout,
  NODE_HEIGHT,
  NODE_WIDTH,
} from "../src/layout.ts";
import { graphState, relation, subjectNode } from "./helpers.ts";

describe("computeLayout", () => {
  it("is deterministic: same graph, same coordinates", () => {
    const graph = graphState(
      [
        subjectNode("arch", "container", "payment-service"),
        subjectNode("code", "repository", "payments-api"),
        subjectNode("sddk", "work-item", "TEST-MODEL-001", {
          authority: "derived",
        }),
      ],
      [
        relation(["code", "repository", "payments-api"], "implements", [
          "arch",
          "container",
          "payment-service",
        ]),
      ],
    );

    const a = computeLayout(graph);
    const b = computeLayout(graph);

    expect(a).toEqual(b);
    expect(a.nodes).toHaveLength(3);
    expect(a.edges).toHaveLength(1);
  });

  it("groups subjects into namespace columns sorted by identity", () => {
    const graph = graphState([
      subjectNode("arch", "container", "payment-service"),
      subjectNode("arch", "system", "vistalith"),
      subjectNode("code", "repository", "payments-api"),
    ]);

    const { nodes } = computeLayout(graph);
    const archNodes = nodes.filter((n) => n.identity.startsWith("arch:"));
    const codeNodes = nodes.filter((n) => n.identity.startsWith("code:"));

    // code column starts after the arch column (alphabetical).
    const archMaxX = Math.max(...archNodes.map((n) => n.x));
    const codeMinX = Math.min(...codeNodes.map((n) => n.x));
    expect(codeMinX).toBeGreaterThanOrEqual(archMaxX + NODE_WIDTH);

    // same column stacks vertically.
    const [first, second] = archNodes;
    expect(second?.y).toBe((first?.y ?? 0) + NODE_HEIGHT + 20);
  });

  it("computes canvas bounds that contain every node", () => {
    const graph = graphState([
      subjectNode("arch", "container", "payment-service"),
      subjectNode("code", "repository", "payments-api"),
      subjectNode("sddk", "work-item", "TEST-MODEL-001"),
    ]);
    const { width, height, nodes } = computeLayout(graph);
    for (const node of nodes) {
      expect(node.x + NODE_WIDTH).toBeLessThanOrEqual(width);
      expect(node.y + NODE_HEIGHT).toBeLessThanOrEqual(height);
    }
  });

  it("adjacency follows relation endpoints", () => {
    const graph = graphState(
      [
        subjectNode("arch", "container", "payment-service"),
        subjectNode("code", "repository", "payments-api"),
        subjectNode("sddk", "work-item", "TEST-MODEL-001"),
      ],
      [
        relation(["code", "repository", "payments-api"], "implements", [
          "arch",
          "container",
          "payment-service",
        ]),
      ],
    );

    const adjacent = adjacencyOf(graph, {
      namespace: "arch",
      kind: "container",
      id: "payment-service",
    });
    expect(adjacent.has("code:repository:payments-api")).toBe(true);
    expect(adjacent.has("sddk:work-item:TEST-MODEL-001")).toBe(false);
  });
});
