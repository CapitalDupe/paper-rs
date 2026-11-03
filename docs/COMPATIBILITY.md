# Compatibility policy

## Current status

There is no playable server implementation yet, so no Minecraft client version or Paper/Bukkit/Spigot plugin compatibility is currently claimed.

## Initial target

The first target is **Minecraft Java Edition 26.3**, chosen in [ADR 0003](adr/0003-target-minecraft-version.md). Its protocol version is 777 according to two independent sources, but it has **not yet been confirmed against a real client**; treat it as unconfirmed until ADR 0003's verification gate is closed. Packet ids, layouts and their provenance are in [PROTOCOL.md](PROTOCOL.md). No other version is targeted.

The first protocol milestone documents:
- Protocol version and packet coverage.
- Status, login, encryption, and compression behavior.
- Authentication mode and required configuration.
- Client test matrix and known unsupported features.

Choosing one release first keeps the initial vertical slice testable. Additional releases can be added with explicit protocol mappings and regression tests.

## Paper and plugin compatibility

Paper has a mature Java ecosystem and APIs that cannot be assumed to map directly onto Rust. Compatibility with existing Java plugins is a separate, substantial project requiring an explicit design and security review. It is not an initial promise.

The project may eventually provide a Rust-native extension API. Any bridge to existing plugin ecosystems would need to define lifecycle behavior, threading, error isolation, versioning, and licensing clearly.

## Conformance

Compatibility claims must be backed by automated tests, documented client versions, and reproducible results. Avoid describing the project as a drop-in replacement until that claim is demonstrably true.
