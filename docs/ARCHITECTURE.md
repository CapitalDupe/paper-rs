# Architecture

This document describes the intended architecture, not completed functionality. Treat module boundaries as provisional until validated by prototypes and benchmarks.

## Design principles

1. **Correctness first.** Packet parsing, game rules, and persistence need tests before performance tuning.
2. **Measure before optimizing.** Track tick duration, allocations, network throughput, and memory use with repeatable workloads.
3. **Explicit ownership.** Prefer Rust's ownership and type system over shared mutable state by default.
4. **Keep the tick predictable.** Work that can safely happen off-thread should be separated from state mutations that must be serialized.
5. **Version compatibility is deliberate.** Protocol and game-version support must be documented and tested.
6. **Small crates with clear contracts.** Keep protocol, server runtime, world simulation, persistence, and extension APIs independently testable.

## Proposed components

- **paper-protocol** — packet framing, codecs, packet models, protocol version tables, and fuzz/property tests.
- **paper-server** — process lifecycle, configuration, logging, networking integration, and orchestration.
- **session/auth** — login state machine, online-mode authentication, timeouts, and session security.
- **tick runtime** — fixed-step scheduling, task queues, timing metrics, and graceful overload behavior.
- **world** — chunks, block state, entities, players, and deterministic game-rule logic.
- **storage** — world format, chunk loading/saving, crash safety, and background I/O.
- **paper-jvm and the Java API shim (`api/`)** — an embedded JVM and a Bukkit/Paper API compatibility layer for running existing Paper plugins, as proposed in [ADR 0002](adr/0002-jvm-bridge.md). Plugin isolation and API coverage are open questions.
- **extensions** — a possible Rust-native extension API alongside plugin compatibility.
- **observability** — structured logs, health status, profiling hooks, and server metrics.

Only the protocol and server crates exist as early scaffolding today.

## Concurrency model to investigate

Start with a simple, correct tick loop and asynchronous socket I/O. Keep authoritative world mutations on a clearly defined execution context initially. Introduce parallel work only where ownership boundaries and ordering semantics can be specified and tested. Avoid promising a multi-threaded world model before benchmarks and correctness tests justify it.

## Security considerations

- Bound packet sizes and decoding work.
- Apply timeouts and rate limits to unauthenticated sessions.
- Never log access tokens or authentication secrets.
- Validate persisted data and configuration.
- Fuzz protocol parsers and test malformed input.
- Keep online-mode authentication enabled by default when authentication is implemented.

## Performance methodology

Every optimization should include a reproducible workload and report hardware, operating system, Rust version, game version, player/entity counts, and measurement methodology. Compare equivalent workloads and publish both throughput and latency/tick-time distributions. No performance claims are made until results can be reproduced.

## Decision records

Significant decisions are recorded in `docs/adr/`:

- [ADR 0001](adr/0001-license-and-mojang-code-boundary.md): GPL-3.0 license and the Mojang-code boundary.
- [ADR 0002](adr/0002-jvm-bridge.md): JVM bridge for Paper plugin compatibility (proposed).
- [ADR 0003](adr/0003-target-minecraft-version.md): target Minecraft version (draft).
