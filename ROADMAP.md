# Roadmap

This roadmap is directional. Items are not complete unless reflected in code, tests, and documentation.

## M0 — Repository and engineering foundations
- [x] Create Rust workspace and crate boundaries.
- [x] Document goals, constraints, and proposed architecture.
- [x] Add CI for formatting, linting, and tests.
- [ ] Establish issue labels and design decision records.

## M1 — Protocol research and vertical slice
- [x] Choose and document the first target Minecraft Java version (26.3, ADR 0003).
- [ ] Verify the 26.3 protocol version against a real client capture (ADR 0003 gate; two independent sources already agree on 777).
- [x] Document protocol references and compatibility policy (`docs/PROTOCOL.md`, `docs/COMPATIBILITY.md`).
- [x] Implement bounded packet framing and codec primitives (also handshake, status and login packets, compression framing and CFB8).
- [ ] Add golden tests and fuzz/property tests for malformed packets.
- [ ] Build a minimal status-ping response.

## M2 — Login and connection lifecycle
- [ ] Implement connection/session state machine and timeouts.
- [ ] Implement online-mode authentication safely.
- [ ] Add protocol-level integration tests.
- [ ] Document supported client versions and known gaps.

## M3 — Minimal playable server
- [ ] Join a world with a player.
- [ ] Implement initial chunk and player synchronization.
- [ ] Add a basic tick loop and graceful shutdown.
- [ ] Persist enough world state to survive a restart.
- [ ] Run repeatable end-to-end smoke tests.

## M4 — World simulation
- [ ] Define block, chunk, entity, and player models.
- [ ] Implement movement and a narrow set of core game mechanics.
- [ ] Add safe chunk loading/saving and recovery behavior.
- [ ] Profile tick time under controlled load.

## M5 — Administration and operations
- [ ] Configuration with validation and useful errors.
- [ ] Structured logs and health/diagnostic output.
- [ ] Metrics for tick duration, memory, connections, and chunk I/O.
- [ ] Document deployment, backups, upgrades, and troubleshooting.

## M6 — Extension model and performance
- [ ] Design a stable, versioned extension API.
- [ ] Evaluate native Rust extensions versus a sandboxed scripting/guest runtime.
- [ ] Publish compatibility and security boundaries.
- [ ] Benchmark against clearly described reference workloads.
- [ ] Optimize demonstrated bottlenecks without sacrificing correctness.

## M7 — JVM bridge and Bukkit/Paper API compatibility
- [ ] Embed a JVM from Rust and smoke-test startup (`paper-jvm`).
- [ ] Load plugin JARs from `plugins/` with `plugin.yml` parsing and isolated class loaders.
- [ ] Implement plugin lifecycle (load, enable, disable) with error isolation.
- [ ] Implement the Bukkit/Paper API shim in Java, backed by Rust calls (core interfaces, events, commands, permissions, scheduler).
- [ ] Build a reference-plugin test harness and a conformance matrix of supported API calls.
- [ ] Document supported plugins, API coverage, and known gaps.

## Definition of done

A feature is done when it has tests, error handling, documentation, and a clear statement of supported versions and limitations. Network-facing features additionally require malformed-input and resource-exhaustion considerations.
