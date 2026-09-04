import type { C4View } from "@vistalith/client";
import { parseSubjectRef } from "@vistalith/client";
import { useSelection } from "../state/selection.ts";

/**
 * C4 lens: renders the projected C4 view (IMPLEMENT-NOW item 12). Elements
 * are selectable by SubjectRef — the same identity the graph and details
 * lenses use, so selection propagates across lenses.
 */
export function C4ViewPanel({ view }: { view: C4View }) {
  const toggle = useSelection((s) => s.toggle);
  const selected = useSelection((s) => s.selected);
  const selectedIdentity = selected
        ? `${selected.namespace}:${selected.kind}:${selected.id}`
        : null;

  const renderGroup = (title: string, elements: C4View["containers"]) =>
    elements.length > 0 ? (
      <section className="c4-group">
        <h3>{title}</h3>
        <ul>
          {elements.map((element) => (
            <li key={element.identity}>
              <button
                type="button"
                className={[
                  "c4-element",
                  `authority-${element.authority}`,
                  element.identity === selectedIdentity ? "c4-element-selected" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => toggle(parseSubjectRef(element.identity))}
                data-identity={element.identity}
                title={element.description ?? element.identity}
              >
                <span className="c4-name">{element.name}</span>
                <span className="c4-identity">{element.identity}</span>
                {element.deprecated ? (
                  <span className="badge badge-deprecated">deprecated</span>
                ) : null}
              </button>
            </li>
          ))}
        </ul>
      </section>
    ) : null;

  return (
    <div className="c4-view" data-testid="c4-view">
      <p className="c4-revision">
        C4 projection · graph revision {view.revision}
      </p>
      {renderGroup("Systems", view.systems)}
      {renderGroup("Containers", view.containers)}
      {renderGroup("Components", view.components)}
      {view.relationships.length > 0 ? (
        <section className="c4-group">
          <h3>Relationships</h3>
          <ul className="c4-relationships">
            {view.relationships.map((relationship) => (
              <li
                key={`${relationship.source}-${relationship.kind}-${relationship.target}`}
              >
                <span className={`edge-chip edge-${relationship.authority}`}>
                  {relationship.authority}
                </span>
                <code>{relationship.source}</code>{" "}
                <strong>-{relationship.kind}-&gt;</strong>{" "}
                <code>{relationship.target}</code>
              </li>
            ))}
          </ul>
        </section>
      ) : (
        <p className="empty">no relationships between architecture subjects</p>
      )}
    </div>
  );
}
