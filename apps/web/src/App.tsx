import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { client } from "./api.ts";
import { C4ViewPanel } from "./components/C4ViewPanel.tsx";
import { ChatPanel } from "./components/ChatPanel.tsx";
import { GraphView } from "./components/GraphView.tsx";
import { SubjectDetails } from "./components/SubjectDetails.tsx";
import { SubjectList } from "./components/SubjectList.tsx";
import { useGraph, useHealth } from "./hooks.ts";

type Lens = "graph" | "c4" | "chat";

const LENSES: { id: Lens; label: string }[] = [
  { id: "graph", label: "Graph" },
  { id: "c4", label: "C4" },
  { id: "chat", label: "Chat" },
];

export function App() {
  const health = useHealth();
  const graph = useGraph();
  const queryClient = useQueryClient();
  const [lens, setLens] = useState<Lens>("graph");

  const c4 = useQuery({
    queryKey: ["c4"],
    queryFn: () => client.c4View(),
    refetchInterval: 2_000,
    retry: 1,
  });

  return (
    <div className="app">
      <header className="app-header">
        <h1>Vistalith</h1>
        <span className="subtitle">semantic world graph</span>
        <nav className="lens-tabs" aria-label="lenses">
          {LENSES.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              className={`lens-tab ${lens === id ? "lens-tab-active" : ""}`}
              onClick={() => setLens(id)}
            >
              {label}
            </button>
          ))}
        </nav>
        <span
          className={`badge ${health?.status === "ok" ? "badge-live" : "badge-offline"}`}
          data-testid="health-badge"
        >
          {health
            ? `vistalithd · ${health.provider} · revision ${health.graph_revision} · ${health.events} events`
            : "vistalithd offline"}
        </span>
      </header>

      {!graph ? (
        <main className="app-main app-main-empty">
          <p>
            waiting for <code>vistalithd</code>… (start it with{" "}
            <code>
              cargo run -p vistalith-server --bin vistalithd -- --fixture
              crates/vistalith-graph/tests/fixtures/sample-world.json
            </code>
            )
          </p>
        </main>
      ) : lens === "graph" ? (
        <main className="app-main">
          <aside className="panel">
            <SubjectList graph={graph} />
          </aside>
          <section className="panel graph-panel">
            <GraphView graph={graph} />
          </section>
          <aside className="panel">
            <SubjectDetails graph={graph} />
          </aside>
        </main>
      ) : lens === "c4" ? (
        <main className="app-main">
          <section className="panel c4-panel">
            {c4.data ? (
              <C4ViewPanel view={c4.data} />
            ) : (
              <p className="empty">waiting for the C4 projection…</p>
            )}
          </section>
          <aside className="panel">
            <SubjectDetails graph={graph} />
          </aside>
        </main>
      ) : (
        <main className="app-main">
          <section className="panel chat-panel-host">
            <ChatPanel
              client={client}
              onGraphChanged={() => {
                void queryClient.invalidateQueries({ queryKey: ["graph"] });
                void queryClient.invalidateQueries({ queryKey: ["c4"] });
              }}
            />
          </section>
        </main>
      )}
    </div>
  );
}
