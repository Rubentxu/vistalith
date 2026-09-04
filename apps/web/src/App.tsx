import { GraphView } from "./components/GraphView.tsx";
import { SubjectDetails } from "./components/SubjectDetails.tsx";
import { SubjectList } from "./components/SubjectList.tsx";
import { useGraph, useHealth } from "./hooks.ts";

export function App() {
  const health = useHealth();
  const graph = useGraph();

  return (
    <div className="app">
      <header className="app-header">
        <h1>Vistalith</h1>
        <span className="subtitle">semantic world graph</span>
        <span
          className={`badge ${health?.status === "ok" ? "badge-live" : "badge-offline"}`}
          data-testid="health-badge"
        >
          {health
            ? `vistalithd · revision ${health.graph_revision} · ${health.events} events`
            : "vistalithd offline"}
        </span>
      </header>

      {graph ? (
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
      ) : (
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
      )}
    </div>
  );
}
