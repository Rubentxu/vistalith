import type {
  ThreadMessage,
  ThreadSummary,
  VistalithClient,
} from "@vistalith/client";
import { useCallback, useEffect, useState } from "react";
import { client as defaultClient } from "../api.ts";

function threadIdOf(identity: string): string {
  return identity.split(":")[2] ?? identity;
}

/**
 * Chat lens (ADR-010: chat is the primary interaction door, not the
 * authority). Threads are durable Vistalith state; sending a message runs a
 * turn against the configured provider and the projected graph updates
 * through the same event log — chat binds subjects, it never overwrites them.
 */
export function ChatPanel({
  client = defaultClient,
  onGraphChanged,
}: {
  client?: VistalithClient;
  onGraphChanged?: () => void;
}) {
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [activeThread, setActiveThread] = useState<string | null>(null);
  const [messages, setMessages] = useState<ThreadMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reloadThreads = useCallback(async () => {
    try {
      setThreads(await client.threads());
    } catch {
      setThreads([]);
    }
  }, [client]);

  useEffect(() => {
    void reloadThreads();
  }, [reloadThreads]);

  const openThread = useCallback(
    async (identity: string) => {
      setActiveThread(identity);
      try {
        const view = await client.thread(threadIdOf(identity));
        setMessages(view.messages);
      } catch (err) {
        setError(String(err));
      }
    },
    [client],
  );

  const newThread = async () => {
    setError(null);
    try {
      const identity = await client.createThread(
        `chat ${new Date().toLocaleString()}`,
      );
      await reloadThreads();
      await openThread(identity);
    } catch (err) {
      setError(String(err));
    }
  };

  // SPEC-011: forking a thread copies its durable items up to a turn with
  // `forked_of` bindings preserved; the fork becomes a live thread.
  const forkThread = async (identity: string) => {
    setError(null);
    try {
      const fork = await client.forkThread(threadIdOf(identity), {});
      await reloadThreads();
      await openThread(fork.fork);
    } catch (err) {
      setError(String(err));
    }
  };

  const send = async () => {
    const content = draft.trim();
    if (!content || !activeThread || busy) return;
    setDraft("");
    setError(null);
    setBusy(true);
    const nextTurn = (messages.at(-1)?.turn ?? 0) + 1;
    setMessages((current) => [
      ...current,
      { message: "local", role: "user", content, turn: nextTurn },
    ]);
    try {
      // Streamed turn: deltas render live; the refetch reconciles with the
      // durable state.
      await client.sendMessageStream(threadIdOf(activeThread), content, {
        onDelta: (delta) => {
          setMessages((current) => {
            const last = current.at(-1);
            if (last && last.message === "streaming") {
              return [
                ...current.slice(0, -1),
                { ...last, content: last.content + delta },
              ];
            }
            return [
              ...current,
              {
                message: "streaming",
                role: "assistant",
                content: delta,
                turn: nextTurn,
              },
            ];
          });
        },
      });
      onGraphChanged?.();
      const view = await client.thread(threadIdOf(activeThread));
      setMessages(view.messages);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="chat-panel" data-testid="chat-panel">
      <div className="chat-threads">
        <button type="button" className="chat-new-thread" onClick={newThread}>
          + new thread
        </button>
        <ul>
          {threads.map((thread) => (
            <li key={thread.thread}>
              <button
                type="button"
                className={[
                  "chat-thread-item",
                  thread.thread === activeThread ? "chat-thread-active" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => openThread(thread.thread)}
              >
                <span className="chat-thread-title">
                  {thread.forked_from ? "⎇ " : ""}
                  {thread.title}
                </span>
                <span className="chat-thread-turns">{thread.turns} turns</span>
              </button>
              <button
                type="button"
                className="chat-thread-fork"
                aria-label={`fork thread ${thread.title}`}
                title="fork this thread (SPEC-011)"
                onClick={() => void forkThread(thread.thread)}
              >
                fork
              </button>
            </li>
          ))}
          {threads.length === 0 ? (
            <li className="empty">no threads yet</li>
          ) : null}
        </ul>
      </div>

      <div className="chat-main">
        <div className="chat-messages" data-testid="chat-messages">
          {messages.map((message) => (
            <div
              key={message.message}
              className={`chat-message chat-message-${message.role}`}
            >
              <span className="chat-role">
                {message.role}
                {message.forked_of ? (
                  <em
                    className="chat-forked-of"
                    title={`copied from ${message.forked_of}`}
                  >
                    {" "}
                    ⎇ forked
                  </em>
                ) : null}
              </span>
              <p>{message.content}</p>
            </div>
          ))}
          {messages.length === 0 ? (
            <p className="empty">
              start or select a thread, then say something
            </p>
          ) : null}
        </div>

        {error ? <p className="chat-error">{error}</p> : null}

        <form
          className="chat-composer"
          onSubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <input
            value={draft}
            placeholder={
              activeThread ? "message the model…" : "create a thread first"
            }
            disabled={!activeThread || busy}
            onChange={(event) => setDraft(event.target.value)}
            aria-label="chat message"
          />
          <button
            type="submit"
            disabled={!activeThread || busy || !draft.trim()}
          >
            {busy ? "…" : "send"}
          </button>
        </form>
      </div>
    </div>
  );
}
