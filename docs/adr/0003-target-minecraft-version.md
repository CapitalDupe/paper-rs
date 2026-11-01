# ADR 0003: Target Minecraft Java version

- Status: Draft (decision pending)
- Date: 2026-11-01

## Context

The first vertical slice needs exactly one Minecraft Java release as its protocol target (see `docs/COMPATIBILITY.md`). The choice affects protocol work, the plugin ecosystem we can test against, and which Paper API version the shim must match.

## Options

1. **Latest stable release.** Newest features and clients, but the plugin ecosystem may lag and the protocol is still moving.
2. **A widely used release the plugin ecosystem supports well.** Easier to find reference plugins, but older clients and a protocol further from current.
3. **Latest first, add a second version later through explicit protocol mappings.** More work up front; deferred until one version is solid.

## To decide

- The exact release and protocol version number (verify against the current Minecraft release and the Paper versions currently supported before filling in).
- The matching Bukkit/Paper API version for the Java shim.
- Whether any client-side tooling exists to test that version in CI.

## Decision

Pending.
