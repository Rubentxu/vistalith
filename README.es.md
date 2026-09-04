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
| 5 | Spike de SurrealDB (con puerta — **puerta cerrada**, `docs/SURREALDB-SPIKE.md`), fork de hilos + diff/Time Travel del grafo (SPEC-011), shell de escritorio Tauri | hecho |
| 6 | Cliente MCP (rmcp, stdio + Streamable HTTP), catálogo unificado de tools con grants de permisos con scope (SPEC-009, TOOLS-PERMISSIONS) | hecho |

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
6. Los forks (SPEC-011) son estado advisory de exploración: un hilo bifurcado
   copia sus items con bindings `forked_of` hacia los originales y una
   relación `forked_from` hacia su fuente; el time travel
   (`graph?at_revision=R`) es un replay estricto del prefijo del log, y los
   diffs estructurales son deterministas. La promoción a SDDK sigue siendo
   explícita y gobernada.
7. Las tools (nativas y MCP) proyectan en un único catálogo (SPEC-009). Los
   resultados de permiso son deny / allow / ask: las tools read-only
   ejecutan libres, las de clase write necesitan un grant temporal con scope
   (por llamada, consumible, revocable), y los denies explícitos siempre
   ganan. Toda llamada — concedida o rechazada — es un evento durable
   `ToolInvoked` que lleva la fuente de la tool. Los permisos de Vistalith
   restringen; nunca debilitan la policy de SDDK.

## Estructura del repositorio

```text
crates/
├── vistalith-domain         # SubjectRef, VEvent, tipos de patch, clases de autoridad
├── vistalith-graph          # SWG en memoria, proyección de eventos, patches, vista C4, replay, diff
├── vistalith-agent-runtime  # motor de conversación, contratos de proveedor, cliente MCP, tools unificadas
├── vistalith-server         # `vistalithd` — servidor axum sobre el log de eventos + SWG
└── vistalith-spike-surrealdb  # spike SPK-003 de la puerta de almacenamiento (toolchain propia; excluido)
packages/
└── client             # @vistalith/client — espejo TS del protocolo + cliente HTTP tipado
apps/
├── web                # lente de grafo React/Vite (subjects/edges, selección por SubjectRef)
└── desktop            # shell Tauri 2 que envuelve la lente web + ciclo de vida de vistalithd
dev/                   # checkout de SDDK fijado + binario sddk fijado (gitignored)
docs/DEPENDENCIES.md   # pins de dependencias y política de pinning
docs/SURREALDB-SPIKE.md  # informe y veredicto de la puerta SPK-003
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
# Núcleo Rust + servidor
cargo test
cargo run -p vistalith-server --bin vistalithd \
  --fixture crates/vistalith-graph/tests/fixtures/sample-world.json --port 7420

# Workspace TypeScript
pnpm install
pnpm build && pnpm test && pnpm lint
pnpm dev:web        # http://localhost:5173 → habla con vistalithd en :7420

# Shell de escritorio (Tauri 2; envuelve la misma lente web y puede lanzar vistalithd)
pnpm install
pnpm desktop:dev    # requiere cabeceras devel de WebKit/GTK — ver scripts/tauri-env.sh

# Spike de almacenamiento SurrealDB (SPK-003; aislado: toolchain nightly, lockfile propio)
cd crates/vistalith-spike-surrealdb
cargo test --features file-engine
cargo run --release --features file-engine -- --engine surrealkv --nodes 50000
```

API de `vistalithd`: `GET /health`, `GET /graph` (con `?at_revision=R`
opcional para time travel), `GET /diff?from=A[&to=B]` (diff estructural del
grafo), `GET /subjects`, `GET /subjects/{namespace}/{kind}/{id}`,
`GET|POST /events`, `POST /patches` (aplicado → `200`, rechazado → `409`;
los rechazos son eventos durables), `POST|GET /threads`, `GET /threads/{id}`,
`POST /threads/{id}/messages` (un turno de proveedor por mensaje),
`POST /threads/{id}/fork` (SPEC-011: copia items hasta un turno con bindings
`forked_of` y enlaza el fork con `forked_from`),
`POST|GET /intents`, `GET /intents/{id}`, `POST /intents/{id}/promote`,
`POST /intents/{id}/discard` (ciclo de vida SPEC-006), `GET /views/c4`,
`GET /tools` (catálogo unificado: tools nativas + MCP con decisiones de
permiso y estado de grants), `POST /tools/{id}/grant` /
`POST /tools/{id}/revoke` (grants temporales con scope — las tools de clase
write solo ejecutan mientras un grant tenga llamadas restantes), y
`GET|POST /mcp/servers`, `DELETE /mcp/servers/{name}` (SPEC-009:
conectar/desconectar servidores MCP por stdio o Streamable HTTP; las tools
descubiertas entran al catálogo unificado con consecuencias clasificadas
desde las anotaciones MCP — los servidores silenciosos obtienen el
conservador `write`).

El cliente web tiene tres lentes sobre las mismas identidades: **Graph**
(sujetos/aristas, con selector de time travel y diff estructural al ver una
revisión pasada), **C4** (vista proyectada) y **Chat** (hilos, con acción de
fork por hilo; los items copiados se marcan `⎇ forked`, y el panel de tools
muestra el catálogo unificado donde las tools ask se conceden o revocan).
Seleccionar un sujeto en cualquier lente propaga el mismo `SubjectRef`.

MCP: conecta un servidor de tools en runtime —
`POST /mcp/servers {"name":"echo","command":"./target/debug/mcp-echo"}`
(stdio) o `{"name":"docs","url":"http://localhost:8100/mcp"}` (Streamable
HTTP). El binario fixture `mcp-echo` viene en el workspace para demos y
tests. `--provider fake --fake-tool TOOL_ID --fake-args '{...}'` scripted un
round de tool determinista para demos offline.

## Decisión de almacenamiento (SPK-003)

El spike de SurrealDB ejecutó la puerta completa de
`technology/GRAPH-STORAGE-DECISION.md` y **la puerta queda cerrada**:
surrealdb 3.x (tanto la línea 3.2.x del baseline como 3.1.6) no compila con
ninguna toolchain estable Rust que use este proyecto (su dependencia
`diskann`, fijada exacta, dispara rust-lang/rust#100013); en nightly el
motor midió bien (rebuild determinista, traversal de 3 saltos con p95
0,35 ms a 1M de relaciones, reapertura durable, digest idéntico a la
proyección del SWG), pero adoptarlo exigiría bifurcar la toolchain. El
almacenamiento sigue siendo el **Candidato B**: log de eventos durable en
JSON + proyección estricta en memoria. Evidencia completa y reproducción:
[`docs/SURREALDB-SPIKE.md`](docs/SURREALDB-SPIKE.md).

Proveedores: `--provider fake` (offline, por defecto) o `--provider
anthropic --model claude-haiku-4-5` con `VISTALITH_ANTHROPIC_API_KEY` (se lee
una vez y nunca se devuelve a ninguna superficie de render — SPEC-008).
`VITE_VISTALITHD_URL` apunta el cliente web a otro `vistalithd`.

## Licencia

[MIT](LICENSE)
