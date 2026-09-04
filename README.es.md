# Vistalith

[English](README.md) | [Español](README.es.md)

**Vistalith es un espacio de trabajo de ingeniería visual y agéntico cuyo
núcleo es un Grafo Semántico Mundial (SWG), construido directamente sobre los
crates de
[SDDK](https://github.com/Rubentxu/software-development-decision-kernel).**
SDDK sigue siendo la autoridad en planificación, workflows, decisiones y
evidencia; Vistalith añade el plano de interacción agéntica (conversaciones,
proveedores, herramientas), el espacio de trabajo visual y el grafo semántico
transversal que conecta el conocimiento de ingeniería.

Este README es **normativo**: las reglas de abajo gobiernan cómo se construye
y evoluciona este repositorio. El baseline completo de planificación está en
[`vistalith-sddk-baseline-v5-graph-first-2026-09-04/`](vistalith-sddk-baseline-v5-graph-first-2026-09-04/START-HERE.md)
(orden de lectura normativo en su `START-HERE.md`).

## Estado

| Slice | Alcance | Estado |
|---|---|---|
| 1 | Workspace Rust, pin de SDDK, `SubjectRef`, `VEvent`, SWG en memoria, replay determinista, `vistalithd` | hecho |
| 2 | `@vistalith/client` + lente de grafo web con selección cross-lens por `SubjectRef` | hecho |
| 3 | Hilos de conversación, un proveedor vía Rig, proyección C4 | hecho |
| 4 | Herramienta nativa (`graph_search`) + ciclo de vida de VisualIntent (draft/preview/promoción) | hecho |
| 5 | Spike de SurrealDB (con puerta de decisión), fork/diff, escritorio Tauri | pendiente |

## Decisiones normativas del baseline

| # | Regla |
|---|---|
| B1 | **SDDK es el núcleo.** Vistalith consume los crates Rust de SDDK directamente; no se inventa frontera de red/proceso interna ni fachada `SddkPort`. Los errores de compilación al subir SDDK son evidencia de acoplamiento real, no algo que ocultar. |
| B2 | **SDDK permanece agnóstico.** Nada de chat, LLM, Rig, MCP, renderizado o cliente específico de Vistalith sube a SDDK. |
| B3 | **El runtime agéntico es Rust.** Proveedores, ensamblado de contexto, MCP, orquestación de herramientas, persistencia de conversaciones y tracing viven en crates Rust de Vistalith. |
| B4 | **TypeScript posee la experiencia humana.** Render de chat, superficies de control y lentes visuales son React/TypeScript. |
| B5 | **Grafo primero.** El conocimiento de ingeniería es un grafo semántico tipado con procedencia, revisión y metadatos de autoridad. |
| B6 | **Eventos primero.** Las transiciones de estado de Vistalith emiten eventos durables; toda vista materializada es reconstruible desde el log durable. |
| B7 | **Intención Visual.** Un gesto visual puede crear intención semántica; nunca ejecuta en silencio un efecto de ingeniería. |
| B8 | **Las innovaciones pueden bajar a SDDK** mediante evaluación explícita de pull-up. |
| B9 | **ActiveGraph es inspiración, no dependencia.** |
| B10 | **La arquitectura emerge de evidencia de UAT.** Sin marketplaces de plugins, CRDTs de colaboración, clustering de grafos ni servicios distribuidos antes de una necesidad medida. |

## Reparto de autoridad

**SDDK posee:** la verdad de planificación y work items, estado de
workflow/run, acciones siguientes legales, policy/gateway, evidencia y
recibos, memoria de decisiones, ciclo de vida determinista.

**Vistalith añade:** conversaciones, proveedores/modelos, Rig, MCP, runtime de
interacción agéntica, el espacio de trabajo visual, el Grafo Semántico
Mundial transversal, el tracing de uso de LLM, el protocolo de cliente y las
lentes de renderizado.

**Regla:** Vistalith nunca reimplementa una capacidad solo por evitar depender
de SDDK — si la capacidad pertenece a SDDK, se llama a SDDK directamente.

## Invariantes duros (forzados en código y tests)

1. Los IDs de nodos del renderer nunca son IDs semánticos; la selección
   propaga `SubjectRef`s (`namespace:kind:id`, con revisión pero sin
   incluirla en la identidad) a través de todas las lentes.
2. Los sujetos propiedad de SDDK nunca se mutan autoritativamente mediante un
   graph patch de Vistalith: esos patches se rechazan
   (`must-be-governed-by-sddk`) y deben convertirse en propuestas semánticas
   gobernadas por SDDK. Vistalith mantiene la verdad de SDDK como
   *observaciones derivadas* con procedencia.
3. Los graph patches llevan la revisión base (concurrencia optimista); las
   bases obsoletas se rechazan y los rechazos son eventos durables.
4. El grafo es una proyección: el log de eventos es la fuente durable de
   verdad, el replay es determinista (digest SHA-256 del grafo) y las
   reconstrucciones se verifican contra las revisiones almacenadas.
5. Todo hecho del grafo lleva fuente, revisión de la fuente, clase de
   autoridad y procedencia; los hechos advisory son distinguibles.

## Estructura del repositorio

```text
crates/
├── vistalith-domain         # SubjectRef, VEvent, tipos de patch, clases de autoridad
├── vistalith-graph          # SWG en memoria, proyección de eventos, patches, vista C4, replay
├── vistalith-agent-runtime  # motor de conversación + contratos de proveedor (Rig detrás)
└── vistalith-server         # `vistalithd` — servidor axum sobre el log de eventos + SWG
packages/
└── client             # @vistalith/client — espejo TS del protocolo + cliente HTTP tipado
apps/
└── web                # lente de grafo React/Vite (subjects/edges, selección por SubjectRef)
dev/                   # checkout de SDDK fijado + binario sddk fijado (gitignored)
docs/DEPENDENCIES.md   # pins de dependencias y política de pinning
vistalith-sddk-baseline-v5-graph-first-2026-09-04/  # baseline de planificación (docs)
```

## Política de fijación de dependencias

- **Toolchain:** Rust 1.91.0 (`rust-toolchain.toml`), Node ≥ 24, pnpm 12
  (`packageManager`), pins exactos en manifiestos y lockfiles
  (`.npmrc` → `save-exact=true`, `Cargo.lock` / `pnpm-lock.yaml` committeados).
- **SDDK** se fija a un tag/commit exacto, materializado en un checkout
  intermedio local de la máquina:

  ```bash
  scripts/bootstrap-dev.sh                 # clona + checkout de la revisión fijada
  scripts/bootstrap-dev.sh --pin v1.83.0   # mueve el pin (actualiza scripts/sddk-pin.env)
  ```

  Todos los crates de SDDK consumidos resuelven a esa única revisión; nunca
  se mezclan revisiones. Solo se fijan refs que existen en el origen de SDDK.
  El binario `sddk` fijado vive en `dev/bin/` con su manifiesto SHA-256. Una
  actualización de SDDK es una actualización de dependencia de primera
  clase: actualizar pin → compilar → tests de contrato/proyección → UAT
  maestro → diff semántico → aceptar o revertir.

## Compilar y ejecutar

```bash
# Núcleo Rust + servidor (24 tests)
cargo test
cargo run -p vistalith-server --bin vistalithd \
  --fixture crates/vistalith-graph/tests/fixtures/sample-world.json --port 7420

# Workspace TypeScript (24 tests)
pnpm install
pnpm build && pnpm test && pnpm lint
pnpm dev:web        # http://localhost:5173 → habla con vistalithd en :7420
```

API de `vistalithd`: `GET /health`, `GET /graph`, `GET /subjects`,
`GET /subjects/{namespace}/{kind}/{id}`, `GET|POST /events`, `POST /patches`
(aplicado → `200`, rechazado → `409`; los rechazos son eventos durables),
`POST|GET /threads`, `GET /threads/{id}`, `POST /threads/{id}/messages`
(un turno de proveedor por mensaje), `POST|GET /intents`,
`GET /intents/{id}`, `POST /intents/{id}/promote`,
`POST /intents/{id}/discard` (ciclo de vida SPEC-006) y `GET /views/c4`.

El cliente web tiene tres lentes sobre las mismas identidades: **Graph**
(sujetos/aristas), **C4** (vista proyectada) y **Chat** (hilos). Seleccionar
un sujeto en cualquier lente propaga el mismo `SubjectRef`.

Proveedores: `--provider fake` (offline, por defecto) o `--provider
anthropic --model claude-haiku-4-5` con `VISTALITH_ANTHROPIC_API_KEY` (se lee
una vez y nunca se devuelve a ninguna superficie de render — SPEC-008).
`VITE_VISTALITHD_URL` apunta el cliente web a otro `vistalithd`.

## Licencia

[MIT](LICENSE)
