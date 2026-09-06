import type {
  ExcalidrawImportReport,
  VistalithClient,
} from "@vistalith/client";
import { useState } from "react";
import { client as defaultClient } from "../api.ts";

/**
 * Excalidraw bindings (slice 20, SPK-009, ADR-014): export the canvas
 * primitives as an Excalidraw scene (identity travels in
 * `customData.vistalith`) and import scenes back as durable bindings.
 * Shape ids are never semantic identity — unchanged content re-imports as
 * a no-op even after ids changed or customData was stripped.
 */
export function ExcalidrawPanel({
  client = defaultClient,
  onGraphChanged,
}: {
  client?: VistalithClient;
  onGraphChanged: () => void;
}) {
  const [sceneText, setSceneText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<ExcalidrawImportReport | null>(null);
  const [exported, setExported] = useState<string | null>(null);

  const exportScene = async () => {
    setBusy(true);
    setError(null);
    try {
      setExported(JSON.stringify(await client.canvasScene(), null, 2));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const importScene = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      // after an export the textarea shows the exported scene
      const parsed: unknown = JSON.parse(exported ?? sceneText);
      const result = await client.importCanvasScene(parsed, {
        createMissing: false,
      });
      setReport(result);
      onGraphChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="excalidraw-panel" data-testid="excalidraw-panel">
      <h3>excalidraw bindings</h3>
      <div className="excalidraw-actions">
        <button
          type="button"
          onClick={() => void exportScene()}
          disabled={busy}
        >
          export scene
        </button>
        <button
          type="button"
          onClick={() => void importScene()}
          disabled={
            busy || (exported === null && sceneText.trim().length === 0)
          }
        >
          import scene
        </button>
      </div>
      <textarea
        aria-label="Excalidraw scene JSON"
        className="excalidraw-source"
        spellCheck={false}
        rows={8}
        placeholder='paste an Excalidraw scene {"elements":[…]} here'
        value={exported ?? sceneText}
        onChange={(event) => {
          setExported(null);
          setSceneText(event.target.value);
        }}
      />
      {report ? (
        <p className="excalidraw-report" data-testid="excalidraw-report">
          bound {report.bound.length} · created{" "}
          {report.created_primitives.length} · unchanged{" "}
          {report.skipped_bindings.length} · unknown{" "}
          {report.unknown_subjects.length} · unbound{" "}
          {report.unbound_elements.length}
        </p>
      ) : null}
      {error ? <p className="excalidraw-error">{error}</p> : null}
    </section>
  );
}
