import type { FrameSummary, VistalithClient } from "@vistalith/client";
import { useCallback, useEffect, useState } from "react";
import { client as defaultClient } from "../api.ts";

function frameIdOf(identity: string): string {
  return identity.split(":")[2] ?? identity;
}

/**
 * Frames lens (slice 8): bounded execution contexts. Each frame owns a
 * thread, runs turns against a restricted catalog under hard budgets, and
 * closes automatically (or explicitly) with a durable outcome.
 */
export function FramesPanel({
  client = defaultClient,
}: {
  client?: VistalithClient;
}) {
  const [frames, setFrames] = useState<FrameSummary[]>([]);
  const [activeFrame, setActiveFrame] = useState<string | null>(null);
  const [messages, setMessages] = useState<
    { id: string; role: string; content: string }[]
  >([]);
  const [goal, setGoal] = useState("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reloadFrames = useCallback(async () => {
    try {
      setFrames(await client.frames());
    } catch {
      setFrames([]);
    }
  }, [client]);

  useEffect(() => {
    void reloadFrames();
  }, [reloadFrames]);

  const openFrame = useCallback(
    async (identity: string) => {
      setActiveFrame(identity);
      try {
        const view = await client.frame(frameIdOf(identity));
        setMessages(
          view.messages.map((m) => ({
            id: m.message,
            role: m.role,
            content: m.content,
          })),
        );
      } catch (err) {
        setError(String(err));
      }
    },
    [client],
  );

  const create = async () => {
    const trimmed = goal.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await client.createFrame({ goal: trimmed, max_turns: 5 });
      setGoal("");
      await reloadFrames();
      await openFrame(created.frame);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const sendTurn = async () => {
    const content = draft.trim();
    if (!content || !activeFrame || busy) return;
    setDraft("");
    setBusy(true);
    setError(null);
    try {
      const reply = await client.frameTurn(frameIdOf(activeFrame), content);
      setMessages((current) => [
        ...current,
        { id: `local-user-${reply.turn}`, role: "user", content },
        {
          id: `local-assistant-${reply.turn}`,
          role: "assistant",
          content: `(turn ${reply.turn})`,
        },
      ]);
      await openFrame(activeFrame);
      await reloadFrames();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const close = async (identity: string, outcome: "completed" | "aborted") => {
    setBusy(true);
    setError(null);
    try {
      await client.closeFrame(frameIdOf(identity), outcome);
      await reloadFrames();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="frames-panel" data-testid="frames-panel">
      <div className="frames-list">
        <div className="frames-create">
          <input
            value={goal}
            placeholder="frame goal…"
            aria-label="frame goal"
            onChange={(event) => setGoal(event.target.value)}
          />
          <button
            type="button"
            className="frames-new"
            onClick={() => void create()}
            disabled={busy || !goal.trim()}
          >
            + frame
          </button>
        </div>
        <ul>
          {frames.map((frame) => (
            <li key={frame.frame}>
              <button
                type="button"
                className={[
                  "frames-item",
                  frame.frame === activeFrame ? "frames-item-active" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => openFrame(frame.frame)}
              >
                <span className="frames-goal">{frame.goal}</span>
                <span className={`badge frames-status-${frame.status}`}>
                  {frame.status} · {frame.turns}/{frame.max_turns} turns
                </span>
              </button>
              {frame.status === "open" ? (
                <span className="frames-actions">
                  <button
                    type="button"
                    aria-label={`complete frame ${frame.goal}`}
                    disabled={busy}
                    onClick={() => void close(frame.frame, "completed")}
                  >
                    ✓
                  </button>
                  <button
                    type="button"
                    aria-label={`abort frame ${frame.goal}`}
                    disabled={busy}
                    onClick={() => void close(frame.frame, "aborted")}
                  >
                    ✕
                  </button>
                </span>
              ) : null}
            </li>
          ))}
          {frames.length === 0 ? (
            <li className="empty">no frames yet</li>
          ) : null}
        </ul>
      </div>

      <div className="frames-main">
        <div className="frames-messages" data-testid="frames-messages">
          {messages.map((message) => (
            <div
              key={message.id}
              className={`chat-message chat-message-${message.role}`}
            >
              <span className="chat-role">{message.role}</span>
              <p>{message.content}</p>
            </div>
          ))}
          {messages.length === 0 ? (
            <p className="empty">select a frame and run a bounded turn</p>
          ) : null}
        </div>
        {error ? <p className="chat-error">{error}</p> : null}
        <form
          className="chat-composer"
          onSubmit={(event) => {
            event.preventDefault();
            void sendTurn();
          }}
        >
          <input
            value={draft}
            placeholder={
              activeFrame ? "run a bounded turn…" : "create or select a frame"
            }
            disabled={!activeFrame || busy}
            onChange={(event) => setDraft(event.target.value)}
            aria-label="frame turn"
          />
          <button
            type="submit"
            disabled={!activeFrame || busy || !draft.trim()}
          >
            {busy ? "…" : "turn"}
          </button>
        </form>
      </div>
    </div>
  );
}
