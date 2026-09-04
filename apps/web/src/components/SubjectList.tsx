import type { GraphState, SubjectNode } from "@vistalith/client";
import { subjectRefToString } from "@vistalith/client";
import { useMemo, useState } from "react";
import { useSelection } from "../state/selection.ts";

/**
 * Subject list lens: groups by namespace and selects by SubjectRef.
 * The filter box narrows by identity substring — still SubjectRefs only.
 */
export function SubjectList({ graph }: { graph: GraphState }) {
  const selected = useSelection((s) => s.selected);
  const toggle = useSelection((s) => s.toggle);
  const [filter, setFilter] = useState("");

  const groups = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const byNamespace = new Map<string, SubjectNode[]>();
    for (const node of graph.subjects) {
      const identity = subjectRefToString(node.subject);
      if (needle && !identity.toLowerCase().includes(needle)) continue;
      const group = byNamespace.get(node.subject.namespace);
      if (group) group.push(node);
      else byNamespace.set(node.subject.namespace, [node]);
    }
    return [...byNamespace.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [graph.subjects, filter]);

  return (
    <div className="subject-list" data-testid="subject-list">
      <input
        type="search"
        placeholder="filter subjects…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        aria-label="filter subjects"
      />
      {groups.map(([namespace, nodes]) => (
        <section key={namespace}>
          <h3>{namespace}</h3>
          <ul>
            {nodes.map((node) => {
              const identity = subjectRefToString(node.subject);
              const isSelected =
                selected !== null && subjectRefToString(selected) === identity;
              return (
                <li key={identity}>
                  <button
                    type="button"
                    className={[
                      "subject-item",
                      `authority-${node.authority}`,
                      isSelected ? "subject-item-selected" : "",
                      node.deprecated ? "subject-item-deprecated" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    onClick={() => toggle(node.subject)}
                    data-identity={identity}
                    title={identity}
                  >
                    <span className="subject-kind">{node.subject.kind}</span>
                    <span className="subject-id">{node.subject.id}</span>
                    {node.deprecated ? (
                      <span className="badge badge-deprecated">deprecated</span>
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
      {groups.length === 0 ? <p className="empty">no subjects match</p> : null}
    </div>
  );
}
