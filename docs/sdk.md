# Rust and Python SDK Guide

Psychevo exposes one Thread and Turn model through an in-process Rust
Framework and an async Python client. Both paths use the same
`psychevo::Application` authority.

## Package layout

The Rust SDK consists of three crates:

- `psychevo-ai` defines provider-neutral generation requests, responses, and
  transports.
- `psychevo-agent-core` defines the model-agnostic agent loop and tool
  interface.
- `psychevo` owns the high-level `Application`, `Client`, `Thread`, and
  `TurnHandle` API.

The default `psychevo` feature set exposes the Framework API. Psychevo's private
Gateway, ACP, and CLI crates opt into an `internal` feature for product
assembly. That feature is not part of the supported SDK interface.

Python uses three distributions with the same version:

- `psychevo` contains the async client and has an exact dependency on
  `psychevo-app-server-bin`.
- `psychevo-app-server-bin` contains the platform App Server executable.
- `psychevo-cli-bin` contains `pevo`, its TUI, and Workbench assets. Install it
  through the `psychevo[cli]` extra.

Binary distributions are wheel-only. The pure Python `psychevo` package also
provides an sdist.

## Rust

Add the Framework and Tokio to your application:

```toml
[dependencies]
psychevo = "0.1.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

An `Application` requires an explicit Psychevo home. It owns the state database
and accepted work until shutdown:

```rust,no_run
use psychevo::{
    Application, StartThreadRequest, ThreadListQuery, TurnEvent, TurnRequest,
};

#[tokio::main]
async fn main() -> psychevo::Result<()> {
    let application = Application::builder()
        .home("./.psychevo-sdk")
        .build()
        .await?;
    let client = application.client();

    let thread = client
        .start_thread(StartThreadRequest::new("."))
        .await?;
    let turn = thread
        .start_turn(TurnRequest::new("Find the main entrypoints"))
        .await?;

    let mut events = turn.events();
    while let Some(event) = events.next().await {
        if let TurnEvent::ReasoningDelta { text } = event {
            eprint!("{text}");
        }
    }

    let result = turn.wait().await?;
    println!("{}", result.final_answer);

    let snapshot = thread.snapshot().await?;
    assert_eq!(snapshot.id, thread.id());
    let _summaries = client.list_threads(ThreadListQuery::default()).await?;

    application.shutdown().await
}
```

Dropping a `Thread`, `TurnHandle`, event receiver, or client clone does not
cancel accepted work. Call `TurnHandle::interrupt` to stop a turn. Graceful
shutdown rejects new turns and waits for accepted work. Forced shutdown also
aborts active turns.

Each event receiver has a fixed capacity. A lagging receiver gets
`TurnEvent::ResyncRequired`; call `Thread::snapshot` to rebuild the projection
from durable items and pending interactions.

`ApplicationBuilder::provider`, `ApplicationBuilder::agent_session_adapter`,
and `TurnRequest::tool` are the supported extension points. They do not expose
the state store or the native run loop.

## Python

Install the SDK, or include the CLI wheel when the Python environment should
also provide `pevo`:

```bash
python -m pip install psychevo
python -m pip install "psychevo[cli]"
```

The API requires Python 3.11 or newer and is async-only:

```python
import asyncio
from pathlib import Path

import psychevo


async def main() -> None:
    async with psychevo.Client() as client:
        thread = await client.start_thread(cwd=Path.cwd())
        turn = await thread.start_turn("Find the main entrypoints")

        async for event in turn.events():
            if event.type == "reasoning_delta":
                print(event.data["text"], end="", flush=True)

        result = await turn.wait()
        print(result.final_answer)

        snapshot = await thread.snapshot()
        assert snapshot.id == thread.id


asyncio.run(main())
```

The default client imports `psychevo-app-server-bin`, verifies that its version
matches the SDK, and starts that exact executable over newline-delimited
JSON-RPC stdio. Protocol messages use stdout. App Server diagnostics use
stderr.

Ordinary RPCs use a 30-second deadline by default. Configure it with
`Client(request_timeout=...)`; `None` opts out. A timeout raises
`RequestTimeoutError` with the method, deadline, and whether delivery is
unknown. The SDK removes that request's correlation, discards any late
response, and never retries automatically. `TurnHandle.wait()` is deliberately
unbounded unless `timeout=` is passed because a Turn is a long-running
operation.

`Client(close_timeout=...)` defaults to 10 seconds and is one deadline across the
shutdown RPC, callback workers, reader, transport close, and local
terminate-to-kill fallback.

Use an explicit local executable when embedding a source build:

```python
async with psychevo.Client(
    executable="./target/release/psychevo-app-server",
    executable_args=("--home", "./.psychevo-sdk"),
) as client:
    ...
```

Use an explicit URL and bearer token for a remote App Server:

```python
async with psychevo.Client(
    remote_url="wss://agent.example.test/app-server",
    token=token,
) as client:
    ...
```

The client does not search `PATH`, install or download executables, discover a
daemon, connect to raw sockets, or load Rust through FFI.

## Controls and reconnects

`TurnHandle.steer` queues additional user input for an active turn.
`TurnHandle.interrupt` requests interruption. `TurnHandle.respond` resolves a
pending interaction. These methods return protocol errors when a connection or
turn cannot accept the operation.

Transport disconnect does not cancel accepted work. After reconnecting, call
`Client.resume_thread` for the authoritative snapshot and
`Client.resume_turn` with the accepted Turn id. A completed Turn returns its
durable result. A process restart cannot reconstruct an in-memory model call
that was still running.

## Client-hosted callbacks

Python can register async custom tools and approval or clarify handlers:

```python
import psychevo


async def read_build_id(call: psychevo.ToolCall) -> psychevo.ToolResult:
    return psychevo.ToolResult({"buildId": "local"})


async def approve(request: psychevo.ApprovalRequest) -> psychevo.ApprovalDecision:
    return psychevo.ApprovalDecision("deny")


tool = psychevo.Tool(
    name="read_build_id",
    description="Read the local build identifier",
    parameters={"type": "object", "properties": {}},
    handler=read_build_id,
)

async with psychevo.Client(tools=[tool], approval_handler=approve) as client:
    ...
```

Registrations belong to one connection. The App Server routes each callback to
the connection captured for that turn. Disconnect, timeout, malformed output,
or an unknown callback fails that invocation. Approval failures never grant
permission.

Each connection runs eight callback workers with a bounded backlog of 64.
When the backlog is full, callback requests receive an overload JSON-RPC error;
notification callbacks are reported to the asyncio loop exception handler.
Filesystem and MCP-startup approval details are exposed as
`FilesystemApprovalRequest` and `McpStartupApprovalRequest`, so handlers do not
need to interpret opaque dictionaries.

Clarify handlers receive a durable interaction event and may answer it for the
caller:

```python
async def clarify(request: psychevo.ClarifyRequest) -> list[list[str]] | None:
    return [["Use the existing API"]] if request.questions else None
```

If a handler does not answer, inspect `ThreadSnapshot.pending_interactions` and
respond through the corresponding `TurnHandle`.

## Protocol and compatibility

The App Server requires an `initialize` request followed by an `initialized`
notification. Client and server negotiate an independent protocol version;
product package versions do not replace that handshake.

Psychevo is pre-release. The SDK does not provide a compatibility facade for
the former `psychevo-runtime` crate. No SDK package or protocol capability
contains outbound telemetry.
