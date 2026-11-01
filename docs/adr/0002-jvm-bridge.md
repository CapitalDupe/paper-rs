# ADR 0002: JVM bridge for Paper plugin compatibility

- Status: Proposed
- Date: 2026-11-01

## Context

Paper plugins are Java bytecode written against the Bukkit/Paper API. They cannot run without a JVM.

## Decision

Embed a JVM in the Rust server through the `jni` crate (new crate `paper-jvm`). A Java shim (`api/`) implements the Bukkit/Paper interfaces and calls back into Rust for server state. Plugins load unmodified from `plugins/`, each in an isolated class loader, with `plugin.yml` parsing and a managed lifecycle.

## Consequences

- A JVM becomes a runtime dependency; packaging, version, and memory settings need documenting.
- Rust-to-Java calls and Java native callbacks cross an FFI boundary. The workspace sets `unsafe_code = "forbid"`; `paper-jvm` may need a narrowly scoped, reviewed exception, isolated to that crate.
- The authoritative world stays in Rust. Plugin calls are marshalled onto the tick thread, so threading rules must be specified before the scheduler and event bus.
- A plugin crash or exception must not take down the server (error isolation).
- API coverage is incremental and tracked in a conformance matrix against reference plugins.

## Alternatives considered

- Rust-native extension API only: simpler, but no existing plugins run.
- WASM guest runtime: sandboxed, but plugins would need recompiling or porting.
- Separate JVM process over IPC: stronger isolation, but higher latency on hot API calls.

## Open questions

- Which JVM versions to support and how to ship or locate one.
- Exact licensing terms of the API artifacts we implement or depend on (see ADR 0001).
