/**
 * Deterministic SVG layout for the slice-2 graph lens.
 *
 * No force simulation on purpose: subjects are grouped in namespace columns
 * (sorted) and stacked by identity, so the same graph always renders the
 * same picture. petgraph/ELK layouts arrive with ADR-007/ADR-015.
 */
import type {
  GraphState,
  RelationFact,
  SubjectNode,
  SubjectRef,
} from "@vistalith/client";
import { subjectRefToString } from "@vistalith/client";

export const NODE_WIDTH = 170;
export const NODE_HEIGHT = 48;
const ROW_GAP = 20;
const COLUMN_GAP = 80;
const MARGIN = 24;

export interface PositionedNode {
  identity: string;
  node: SubjectNode;
  x: number;
  y: number;
}

export interface PositionedEdge {
  identity: string;
  fact: RelationFact;
  /** Path from source border to target border. */
  path: string;
  labelX: number;
  labelY: number;
}

export interface Layout {
  nodes: PositionedNode[];
  edges: PositionedEdge[];
  width: number;
  height: number;
}

export function computeLayout(graph: GraphState): Layout {
  // Columns: one per namespace, sorted; rows: (kind, id), sorted. Both are
  // already sorted server-side, but do not rely on it.
  const columns = new Map<string, SubjectNode[]>();
  for (const node of [...graph.subjects].sort(byIdentity)) {
    const ns = node.subject.namespace;
    const column = columns.get(ns);
    if (column) {
      column.push(node);
    } else {
      columns.set(ns, [node]);
    }
  }

  const nodes: PositionedNode[] = [];
  let columnX = MARGIN;
  let maxRows = 0;
  for (const namespace of [...columns.keys()].sort()) {
    let y = MARGIN;
    for (const node of columns.get(namespace) ?? []) {
      nodes.push({
        identity: subjectRefToString(node.subject),
        node,
        x: columnX,
        y,
      });
      y += NODE_HEIGHT + ROW_GAP;
    }
    maxRows = Math.max(maxRows, columns.get(namespace)?.length ?? 0);
    columnX += NODE_WIDTH + COLUMN_GAP;
  }

  const centers = new Map<string, { cx: number; cy: number }>();
  for (const positioned of nodes) {
    centers.set(positioned.identity, {
      cx: positioned.x + NODE_WIDTH / 2,
      cy: positioned.y + NODE_HEIGHT / 2,
    });
  }

  const edges: PositionedEdge[] = graph.relations.map((fact) => {
    const from = centers.get(subjectRefToString(fact.relation.from));
    const to = centers.get(subjectRefToString(fact.relation.to));
    // Defensive defaults keep replayed/unknown endpoints renderable.
    const s = from ?? { cx: MARGIN, cy: MARGIN };
    const t = to ?? { cx: MARGIN + NODE_WIDTH, cy: MARGIN };
    const dir = Math.sign(t.cx - s.cx) || 1;
    const x1 = s.cx + (dir * NODE_WIDTH) / 2;
    const y1 = s.cy;
    const x2 = t.cx - (dir * NODE_WIDTH) / 2;
    const y2 = t.cy;
    const path = `M ${x1} ${y1} C ${x1 + dir * 50} ${y1}, ${x2 - dir * 50} ${y2}, ${x2} ${y2}`;
    return {
      identity: edgeIdentity(fact),
      fact,
      path,
      labelX: (x1 + x2) / 2,
      labelY: (y1 + y2) / 2 - 8,
    };
  });

  const width = MARGIN * 2 + columnX - COLUMN_GAP;
  const height = MARGIN * 2 + maxRows * (NODE_HEIGHT + ROW_GAP) - ROW_GAP;
  return {
    nodes,
    edges,
    width: Math.max(width, 320),
    height: Math.max(height, 160),
  };
}

export function edgeIdentity(fact: RelationFact): string {
  return `${subjectRefToString(fact.relation.from)}--${fact.relation.kind}-->${subjectRefToString(fact.relation.to)}`;
}

/** Adjacent identity strings (endpoints of relations touching `selected`). */
export function adjacencyOf(
  graph: GraphState,
  selected: SubjectRef,
): Set<string> {
  const adjacent = new Set<string>();
  const identity = subjectRefToString(selected);
  for (const fact of graph.relations) {
    const from = subjectRefToString(fact.relation.from);
    const to = subjectRefToString(fact.relation.to);
    if (from === identity) adjacent.add(to);
    if (to === identity) adjacent.add(from);
  }
  return adjacent;
}

function byIdentity(a: SubjectNode, b: SubjectNode): number {
  return subjectRefToString(a.subject).localeCompare(
    subjectRefToString(b.subject),
  );
}
