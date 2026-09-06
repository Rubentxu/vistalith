import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { client } from "./api.ts";
import { C4ViewPanel } from "./components/C4ViewPanel.tsx";
import { ChatPanel } from "./components/ChatPanel.tsx";
import { DecisionLensPanel } from "./components/DecisionLensPanel.tsx";
import { FramesPanel } from "./components/FramesPanel.tsx";
import { GraphView } from "./components/GraphView.tsx";
import { ImpactPanel } from "./components/ImpactPanel.tsx";
import { IntentComposer } from "./components/IntentComposer.tsx";
import { LikeC4Panel } from "./components/LikeC4Panel.tsx";
import { SubjectDetails } from "./components/SubjectDetails.tsx";
import { SubjectList } from "./components/SubjectList.tsx";
import { ThinkingPanel } from "./components/ThinkingPanel.tsx";
import { HistoryDiff, TimeTravelBar } from "./components/TimeTravel.tsx";
import { ToolsPanel } from "./components/ToolsPanel.tsx";
import { useGraph, useHealth } from "./hooks.ts";
import { useSelection } from "./state/selection.ts";

type Lens = "graph" | "c4" | "chat" | "frames" | "decisions" | "thinking";

const LENSES: { id: Lens; label: string }[] = [
  { id: "graph", label: "Graph" },
  { id: "c4", label: "C4" },
  { id: "chat", label: "Chat" },
  { id: "frames", label: "Frames" },
  { id: "decisions", label: "Decisions" },
  { id: "thinking", label: "Thinking" },
];

export function App() {
  const health = useHealth();
  const graph = useGraph();
  const queryClient = useQueryClient();
  const selected = useSelection((s) => s.selected);
  const [lens, setLens] = useState<Lens>("graph");
  // SPEC-011 time travel: when set, the graph lens renders that revision.
  const [asOf, setAsOf] = useState<number | null>(null);

  const c4 = useQuery({
    queryKey: ["c4"],
    queryFn: () => client.c4View(),
    refetchInterval: 2_000,
    retry: 1,
  });

  const history = useQuery({
    queryKey: ["graph-at", asOf],
    queryFn: () => client.graphAt(asOf as number),
    enabled: asOf !== null,
    retry: 1,
  });
  const diff = useQuery({
    queryKey: ["graph-diff", asOf],
    queryFn: () => client.diff(asOf as number),
    enabled: asOf !== null,
    retry: 1,
    refetchInterval: 5_000,
  });

  const shownGraph =
    asOf !== null && history.data ? history.data : (graph ?? undefined);

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
            <SubjectList graph={shownGraph ?? graph} />
          </aside>
          <section className="panel graph-panel">
            <TimeTravelBar
              currentRevision={graph.revision}
              asOf={asOf}
              onSelect={setAsOf}
            />
            {shownGraph ? (
              <GraphView graph={shownGraph} />
            ) : (
              <p className="empty">replaying revision {asOf}…</p>
            )}
          </section>
          <aside className="panel">
            <SubjectDetails graph={shownGraph ?? graph} />
            <ImpactPanel selected={selected} />
            {asOf !== null && diff.data ? (
              <HistoryDiff diff={diff.data} from={asOf} />
            ) : null}
            <IntentComposer
              client={client}
              selected={selected}
              onGraphChanged={() => {
                void queryClient.invalidateQueries({ queryKey: ["graph"] });
                void queryClient.invalidateQueries({ queryKey: ["c4"] });
              }}
            />
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
            <LikeC4Panel
              client={client}
              onGraphChanged={() => {
                void queryClient.invalidateQueries({ queryKey: ["graph"] });
                void queryClient.invalidateQueries({ queryKey: ["c4"] });
              }}
            />
            <IntentComposer
              client={client}
              selected={selected}
              onGraphChanged={() => {
                void queryClient.invalidateQueries({ queryKey: ["graph"] });
                void queryClient.invalidateQueries({ queryKey: ["c4"] });
              }}
            />
          </aside>
        </main>
      ) : lens === "thinking" ? (
        <main className="app-main">
          <section className="panel decisions-lens-host">
            <ThinkingPanel client={client} />
          </section>
        </main>
      ) : lens === "decisions" ? (
        <main className="app-main">
          <section className="panel decisions-lens-host">
            <DecisionLensPanel client={client} />
          </section>
        </main>
      ) : lens === "frames" ? (
        <main className="app-main">
          <section className="panel chat-panel-host">
            <FramesPanel client={client} />
          </section>
          <aside className="panel chat-tools-host">
            <ToolsPanel client={client} />
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
          <aside className="panel chat-tools-host">
            <ToolsPanel client={client} />
          </aside>
        </main>
      )}
    </div>
  );
}
