import type { CanvasSubject, VistalithClient } from "@vistalith/client";
import { useCallback, useEffect, useState } from "react";
import { client as defaultClient } from "../api.ts";
import { ExcalidrawPanel } from "./ExcalidrawPanel.tsx";

/**
 * Thinking lens (slice 17, VISUAL-THINKING.md): free-form primitives —
 * note, question, hypothesis, option — as advisory semantic subjects.
 * Sketching is first-class: a primitive is already semantic (step 1),
 * attaches to a subject by mention (step 2), and formalizes into a
 * VisualIntent draft on demand (steps 3-4, SPEC-006).
 */
export function ThinkingPanel({
  client = defaultClient,
}: {
  client?: VistalithClient;
}) {
  const [subjects, setSubjects] = useState<CanvasSubject[]>([]);
  const [kind, setKind] = useState("note");
  const [content, setContent] = useState("");
  const [relatesTo, setRelatesTo] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [drafted, setDrafted] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSubjects(await client.canvasSubjects());
    } catch {
      setSubjects([]);
    }
  }, [client]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const create = async () => {
    const trimmed = content.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    setError(null);
    try {
      const input: Parameters<typeof client.createCanvasSubject>[0] = {
        kind: kind as Parameters<typeof client.createCanvasSubject>[0]["kind"],
        content: trimmed,
      };
      if (relatesTo.trim()) input.relates_to = relatesTo.trim();
      await client.createCanvasSubject(input);
      setContent("");
      setRelatesTo("");
      await reload();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const promote = async (subject: string) => {
    const id = subject.split(":")[2] ?? subject;
    const ns = subject.split(":")[0] ?? "vistalith";
    const k = subject.split(":")[1] ?? "note";
    setBusy(true);
    setError(null);
    try {
      const result = await client.promoteCanvasSubject(ns, k, id);
      setDrafted(result.intent);
      await reload();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="thinking-panel" data-testid="thinking-panel">
      <div className="thinking-create">
        {" "}
        <h3>thinking canvas</h3>
        <select
          aria-label="primitive kind"
          value={kind}
          onChange={(e) => setKind(e.target.value)}
        >
          <option value="note">note</option>
          <option value="question">question</option>
          <option value="hypothesis">hypothesis</option>
          <option value="option">option</option>
        </select>
        <input
          value={content}
          placeholder="sketch a thought…"
          aria-label="primitive content"
          onChange={(e) => setContent(e.target.value)}
        />
        <input
          value={relatesTo}
          placeholder="relates to (ns:kind:id, optional)"
          aria-label="relates to"
          onChange={(e) => setRelatesTo(e.target.value)}
        />
        <button
          type="button"
          onClick={() => void create()}
          disabled={busy || !content.trim()}
        >
          sketch
        </button>
      </div>
      {error ? <p className="chat-error">{error}</p> : null}
      {drafted ? (
        <p className="context-reason" data-testid="drafted-intent">
          drafted {drafted} — promote it from the visual intent composer
        </p>
      ) : null}
      <ul className="thinking-list">
        {subjects.map((subject) => (
          <li key={subject.subject} className="thinking-item">
            <div className="tools-row">
              <span className={`badge badge-advisory`}>{subject.kind}</span>
              <p className="thinking-content">{subject.content}</p>
            </div>
            {subject.relates_to ? (
              <p className="context-reason">↳ {subject.relates_to}</p>
            ) : null}
            {subject.relates_to ? (
              <button
                type="button"
                className="thinking-promote"
                disabled={busy}
                aria-label={`promote ${subject.kind} ${subject.subject}`}
                onClick={() => void promote(subject.subject)}
              >
                promote to intent
              </button>
            ) : null}
          </li>
        ))}
        {subjects.length === 0 ? (
          <li className="empty">no primitives yet — sketch one</li>
        ) : null}
      </ul>
      <ExcalidrawPanel client={client} onGraphChanged={reload} />
    </div>
  );
}
