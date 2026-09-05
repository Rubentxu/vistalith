import type {
  DecisionsLens,
  UatScenarioView,
  VistalithClient,
} from "@vistalith/client";
import { useCallback, useEffect, useState } from "react";
import { client as defaultClient } from "../api.ts";

/**
 * Decision lens (slice 13, `visual/DECISIONS-TIME.md`, milestone M9): every
 * decision in the graph with its question, the option that won, rejected
 * alternatives, the requirement that motivated it and the evidence that
 * supports it — read straight from the SWG's typed relations.
 */
export function DecisionLensPanel({
  client = defaultClient,
}: {
  client?: VistalithClient;
}) {
  const [lens, setLens] = useState<DecisionsLens | null>(null);
  const [uat, setUat] = useState<UatScenarioView[]>([]);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setLens(await client.decisionsLens());
      setUat((await client.uatLens()).scenarios);
    } catch (err) {
      setError(String(err));
    }
  }, [client]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return (
    <div className="decisions-panel" data-testid="decisions-panel">
      <h3>decisions</h3>
      {error ? <p className="chat-error">{error}</p> : null}
      <ul className="decisions-list">
        {(lens?.decisions ?? []).map((decision) => (
          <li key={decision.decision} className="decision-entry">
            <code className="decision-id">{decision.decision}</code>
            {decision.deprecated ? (
              <span className="badge badge-deprecated">deprecated</span>
            ) : null}
            <dl className="decision-facts">
              {decision.question ? (
                <>
                  <dt>question:</dt>
                  <dd>
                    <code>{decision.question}</code>
                  </dd>
                </>
              ) : null}
              {decision.selected ? (
                <>
                  <dt>selected:</dt>
                  <dd>
                    <code>{decision.selected}</code>
                  </dd>
                </>
              ) : null}
              {decision.rejected.length > 0 ? (
                <>
                  <dt>rejected:</dt>
                  <dd>
                    {decision.rejected.map((r) => (
                      <span key={r.option} className="decision-rejected">
                        <code>{r.option}</code>
                      </span>
                    ))}
                  </dd>
                </>
              ) : null}
              {decision.motivated_by.length > 0 ? (
                <>
                  <dt>motivated by:</dt>
                  <dd>{decision.motivated_by.join(", ")}</dd>
                </>
              ) : null}
              {decision.evidence.length > 0 ? (
                <>
                  <dt>evidence:</dt>
                  <dd>
                    {decision.evidence.map((e) => (
                      <span key={e} className="decision-evidence">
                        <code>{e}</code>
                      </span>
                    ))}
                  </dd>
                </>
              ) : null}
              {decision.contradicts.length > 0 ? (
                <>
                  <dt>contradicted by:</dt>
                  <dd>{decision.contradicts.join(", ")}</dd>
                </>
              ) : null}
              {decision.revisits.length > 0 ? (
                <>
                  <dt>revisits:</dt>
                  <dd>{decision.revisits.join(", ")}</dd>
                </>
              ) : null}
            </dl>
          </li>
        ))}
        {lens && lens.decisions.length === 0 ? (
          <li className="empty">no decisions in the graph</li>
        ) : null}
        {!lens ? <li className="empty">loading decisions…</li> : null}
      </ul>

      {uat.length > 0 ? (
        <div className="uat-panel" data-testid="uat-panel">
          <h3>uat</h3>
          <ul className="uat-list">
            {uat.map((scenario) => (
              <li key={scenario.scenario}>
                <code>{scenario.title || scenario.scenario}</code>
                <span
                  className={`badge ${
                    scenario.latest_verdict === "pass"
                      ? "badge-live"
                      : scenario.latest_verdict === "fail"
                        ? "badge-deprecated"
                        : "badge-advisory"
                  }`}
                >
                  {scenario.latest_verdict} · {scenario.checks.length} check
                  {scenario.checks.length === 1 ? "" : "s"}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
