# paper-rs

**A Rust-native, high-performance Minecraft Java server inspired by the goals and feature set of PaperMC.**

> **Status: early-stage project / design scaffold.** This repository is the beginning of an independent reimplementation, not a working or compatible replacement yet.

PaperMC demonstrates how much a server can improve through performance engineering, operational tooling, and a rich plugin ecosystem. `paper-rs` explores those goals with Rust: predictable resource use, memory safety, explicit concurrency, and a maintainable architecture.

## Goals

- Build a Minecraft: Java Edition dedicated server in Rust.
- Prioritize correctness and protocol compatibility before optimizations.
- Design for low-latency ticking and efficient networking.
- Provide robust configuration, logging, metrics, and graceful shutdown.
- Explore a documented extension API after core server behavior stabilizes.
- Develop in small, testable milestones with reproducible benchmarks.

## Non-goals (for now)

- Claiming drop-in compatibility with Paper, Bukkit, or Spigot plugins.
- Reusing PaperMC implementation code without an explicit, compatible license basis.
- Promising production readiness or a specific performance advantage before benchmarks exist.
- Implementing every Minecraft version at once.

## Project status

The repository currently contains the project plan and an initial Rust workspace scaffold. The server is **not yet ready to host players**. Protocol handling, authentication, world simulation, persistence, and compatibility work remain future milestones.

See [ROADMAP.md](ROADMAP.md), [ARCHITECTURE.md](docs/ARCHITECTURE.md), and [COMPATIBILITY.md](docs/COMPATIBILITY.md).

## Planned architecture

```text
Minecraft Java client
        |
   Network / protocol
        |
  Session & authentication
        |
   Server tick scheduler
        |
 World, entities, chunks, plugins
        |
 Persistence / configuration
```

The exact module boundaries will evolve as prototypes and measurements reveal the right design.

## Getting started

Requirements: a stable Rust toolchain.

```sh
git clone https://github.com/CapitalDupe/paper-rs.git
cd paper-rs
cargo check --workspace
cargo test --workspace
```

These commands validate the current scaffold; they do not launch a playable Minecraft server yet.

## Contributing

Issues, design discussions, tests, benchmarks, and documentation are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. For large architectural changes, open an issue first so the design can be discussed.

## Project name and relationship to PaperMC

This is an independent project inspired by the server software ecosystem and goals associated with PaperMC. It is **not affiliated with, endorsed by, or a continuation of PaperMC**. Minecraft is a trademark of Microsoft/Mojang; PaperMC is a separate project. This repository does not imply compatibility with Paper plugins.

## License

Licensed under the MIT License; see [LICENSE](LICENSE). Any third-party protocol specifications, assets, or dependencies remain subject to their respective terms.
