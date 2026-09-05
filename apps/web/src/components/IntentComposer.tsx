import type {
  IntentSummary,
  PromotionOutcome,
  SubjectRef,
} from "@vistalith/client";
import { subjectRefToString, type VistalithClient } from "@vistalith/client";
import { useState } from "react";
import { client as defaultClient } from "../api.ts";

/**
 * Visual intent composer (SPEC-006): a gesture drafts only; the preview is
 * stale-aware and execution requires the explicit Promote act. When the
 * target is SDDK-owned, promotion routes to SDDK governance instead of
 * applying (SPEC-001 invariant 4).
 */
export function IntentComposer({
  client = defaultClient,
  selected,
  onGraphChanged,
}: {
  client?: VistalithClient;
  selected: SubjectRef | null;
  onGraphChanged?: () => void;
}) {
  const [gesture, setGesture] = useState<"rename" | "annotate">("rename");
  const [name, setName] = useState("");
  const [draft, setDraft] = useState<IntentSummary | null>(null);
  const [outcome, setOutcome] = useState<PromotionOutcome | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const targetIdentity = selected ? subjectRefToString(selected) : null;

  if (!selected || !targetIdentity) {
    return null;
  }

  const propose = async () => {
    setError(null);
    setOutcome(null);
    const operations =
      gesture === "rename"
        ? [
            {
              op: "upsert-subject",
              subject: {
                namespace: selected.namespace,
                kind: selected.kind,
                id: selected.id,
              },
              authority: "authoritative",
              provenance: { source: "web:intent" },
              properties: { name },
            },
          ]
        : [];
    if (gesture === "rename" && !name.trim()) {
      setError("give the new name first");
      return;
    }
    setBusy(true);
    try {
      setDraft(
        await client.draftIntent({
          target: targetIdentity,
          gesture,
          change: { operations },
        }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const promote = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const result = await client.promoteIntent(
        draft.intent.split(":")[2] ?? draft.intent,
        { actor: "user:web" },
      );
      setOutcome(result);
      setDraft(null);
      if (result.outcome === "applied") onGraphChanged?.();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const discard = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      await client.discardIntent(draft.intent.split(":")[2] ?? draft.intent);
      setDraft(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="intent-composer" data-testid="intent-composer">
      <h3>Visual intent</h3>

      {!draft ? (
        <div className="intent-form">
          <select
            value={gesture}
            aria-label="intent gesture"
            onChange={(e) =>
              setGesture(e.target.value as "rename" | "annotate")
            }
          >
            <option value="rename">rename</option>
            <option value="annotate">annotate (no-op draft)</option>
          </select>
          {gesture === "rename" ? (
            <input
              value={name}
              placeholder="new name…"
              aria-label="intent new name"
              onChange={(e) => setName(e.target.value)}
            />
          ) : null}
          <button type="button" disabled={busy} onClick={propose}>
            {busy ? "…" : "propose draft"}
          </button>
        </div>
      ) : (
        <div className="intent-preview" data-testid="intent-preview">
          <p>
            <strong>{draft.gesture}</strong> → <code>{targetIdentity}</code>
          </p>
          <p className="revision">
            base revision {draft.base_revision}
            {draft.stale ? (
              <span className="badge badge-deprecated">stale</span>
            ) : (
              <span className="badge badge-live">fresh</span>
            )}
          </p>
          <div className="intent-actions">
            <button type="button" disabled={busy} onClick={promote}>
              promote
            </button>
            <button type="button" disabled={busy} onClick={discard}>
              discard
            </button>
          </div>
        </div>
      )}

      {outcome ? (
        <p className="intent-outcome" data-testid="intent-outcome">
          {outcome.outcome === "applied" &&
            `applied at revision ${outcome.revision}`}
          {outcome.outcome === "sddk-governed" &&
            `routed to SDDK governance: ${outcome.subject}`}
          {outcome.outcome === "submitted-to-sddk" &&
            `submitted to SDDK (${outcome.decision})${
              outcome.receipt_id ? ` · receipt ${outcome.receipt_id}` : ""
            }: ${outcome.subject}`}
          {outcome.outcome === "stale" &&
            "preview is stale — refresh and re-draft"}
          {outcome.outcome === "rejected" && `rejected: ${outcome.reason}`}
        </p>
      ) : null}
      {error ? <p className="chat-error">{error}</p> : null}
    </div>
  );
}
