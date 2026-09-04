import type { GraphState, SubjectNode } from "@vistalith/client";
import { subjectRefToString } from "@vistalith/client";

export function subjectNode(
  namespace: string,
  kind: string,
  id: string,
  overrides: Partial<SubjectNode> = {},
): SubjectNode {
  return {
    subject: { namespace, kind, id },
    authority: "authoritative",
    provenance: { source: "test" },
    deprecated: false,
    last_event_sequence: 0,
    ...overrides,
  };
}

export function graphState(
  subjects: SubjectNode[],
  relations: GraphState["relations"] = [],
): GraphState {
  return {
    revision: 1,
    subjects: [...subjects].sort((a, b) =>
      subjectRefToString(a.subject).localeCompare(
        subjectRefToString(b.subject),
      ),
    ),
    relations,
  };
}

export function relation(
  from: [string, string, string],
  kind: string,
  to: [string, string, string],
  authority: GraphState["relations"][number]["authority"] = "authoritative",
): GraphState["relations"][number] {
  return {
    relation: {
      from: { namespace: from[0], kind: from[1], id: from[2] },
      kind,
      to: { namespace: to[0], kind: to[1], id: to[2] },
    },
    authority,
    provenance: { source: "test" },
  };
}

/** Resets the zustand selection store between tests (no cross-test leaks). */
export async function resetSelection(): Promise<void> {
  const { useSelection } = await import("../src/state/selection.ts");
  useSelection.getState().clear();
}
