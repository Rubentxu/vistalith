import { useQuery } from "@tanstack/react-query";
import type {
  SemanticContextView,
  SubjectRef,
  VistalithClient,
} from "@vistalith/client";
import { subjectRefToString } from "@vistalith/client";
import { useState } from "react";
import { client as defaultClient } from "../api.ts";

/**
 * Algorithmic answers for the selected subject (slice 7, SPK-004 + SPEC-005):
 * the transitive impact set (who depends on it) and, on demand, a bounded
 * semantic context view whose provenance explains every inclusion.
 */
export function ImpactPanel({
  selected,
  client = defaultClient,
}: {
  selected: SubjectRef | null;
  client?: VistalithClient;
}) {
  const [context, setContext] = useState<SemanticContextView | null>(null);
  const identity = selected ? subjectRefToString(selected) : null;

  const impact = useQuery({
    queryKey: ["impact-full", identity],
    queryFn: () =>
      client.impactFull(
        (selected as SubjectRef).namespace,
        (selected as SubjectRef).kind,
        (selected as SubjectRef).id,
      ),
    enabled: identity !== null,
    retry: 0,
  });

  // M9: the why-path — what supports this subject, with the evidence
  // backbone highlighted.
  const why = useQuery({
    queryKey: ["why", identity],
    queryFn: () =>
      client.why(
        (selected as SubjectRef).namespace,
        (selected as SubjectRef).kind,
        (selected as SubjectRef).id,
      ),
    enabled: identity !== null,
    retry: 0,
  });

  if (!selected) return null;

  const buildContext = async () => {
    const view = await client.contextView({
      roots: [identity as string],
      max_depth: 1,
      token_budget: 4_000,
    });
    setContext(view);
  };

  return (
    <div className="impact-panel" data-testid="impact-panel">
      {why.data && why.data.links?.length > 0 ? (
        <div className="why-panel" data-testid="why-panel">
          <h3>Why</h3>
          <ul className="why-list">
            {why.data.links.map((link) => (
              <li key={`${link.depth}-${link.kind}-${link.from}`}>
                <code>{link.from}</code>
                <span className="context-reason">
                  {link.kind}
                  {link.kind === "provides_evidence_for" ||
                  link.kind === "verifies"
                    ? " · evidence"
                    : ""}{" "}
                  (depth {link.depth})
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      <h3>Impact</h3>
      {impact.data ? (
        impact.data.transitively_impacted?.length === 0 &&
        impact.data.direct_dependents.length === 0 ? (
          <p className="empty">nothing depends on this subject</p>
        ) : (
          <>
            <p className="impact-count">
              {impact.data.direct_dependents?.length ?? 0} direct ·{" "}
              {impact.data.transitively_impacted?.length ?? 0} transitive
            </p>
            <ul className="impact-list">
              {impact.data.transitively_impacted.map((subject) => (
                <li key={subject}>
                  <code>{subject}</code>
                </li>
              ))}
            </ul>
            {impact.data.affected_tests.length > 0 ? (
              <p className="impact-section">
                tests: {impact.data.affected_tests?.length}
              </p>
            ) : null}
            {(impact.data.stale_evidence?.length ?? 0) > 0 ? (
              <p className="impact-section">
                stale evidence: {impact.data.stale_evidence?.length}
              </p>
            ) : null}
            {(impact.data.decisions_potentially_invalidated?.length ?? 0) >
            0 ? (
              <p className="impact-section">
                decisions possibly invalidated:{" "}
                {impact.data.decisions_potentially_invalidated?.length}
              </p>
            ) : null}
            {(impact.data.unknown_impact?.length ?? 0) > 0 ? (
              <p className="impact-section">
                unknown impact: {impact.data.unknown_impact?.length}
              </p>
            ) : null}
          </>
        )
      ) : impact.isError ? (
        <p className="empty">subject not in the graph</p>
      ) : (
        <p className="empty">measuring impact…</p>
      )}

      <button
        type="button"
        className="impact-context-button"
        aria-label="build context view"
        onClick={() => void buildContext()}
      >
        context view (depth 1)
      </button>
      {context ? (
        <div className="context-view" data-testid="context-view">
          <p className="impact-count">
            {context.items.length} item
            {context.items.length === 1 ? "" : "s"} · ~
            {context.estimated_tokens}/{context.token_budget} tokens
            {context.truncated ? " · truncated" : ""}
          </p>
          <ul className="context-items">
            {context.items.map((item) => (
              <li key={item.subject}>
                <code>{item.subject}</code>
                <span className="context-reason">
                  {item.reason.reason === "root"
                    ? "root"
                    : `via ${item.reason.kind} (depth ${item.reason.depth})`}
                  {" · "}
                  {item.estimated_tokens} tok
                </span>
              </li>
            ))}
          </ul>
          {context.exclusions.length > 0 ? (
            <details>
              <summary>
                {context.exclusions.length} exclusion
                {context.exclusions.length === 1 ? "" : "s"}
              </summary>
              <ul className="context-exclusions">
                {context.exclusions.map((exclusion) => (
                  <li key={exclusion.subject}>
                    <code>{exclusion.subject}</code>
                    <span className="context-reason">
                      {exclusion.exclusion.reason}
                    </span>
                  </li>
                ))}
              </ul>
            </details>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
