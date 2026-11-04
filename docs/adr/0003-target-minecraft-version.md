# ADR 0003: Target Minecraft Java version

- Status: Accepted
- Date: 2026-11-04

## Context

The server needs exactly one Minecraft Java release as its protocol target (see `docs/COMPATIBILITY.md`). The choice drives the protocol work, the block and packet data, and which Paper API version the plugin layer must match.

Since 2026 Mojang numbers releases year.drop.hotfix and ships roughly three feature drops a year (26.1, 26.2, 26.3). Each drop can change the protocol.

## Decision

Target **Java Edition 26.3** ("Wilderness Bound", released 2026-09-15), the latest stable release when this was decided.

- **Java 25** is required: Mojang's server needs it for 26.1 and newer, and Paper's 26.3 builds need it too. CI and the embedded JVM use Java 25.
- The **protocol number** is not recorded here on purpose. It will be read from the official server jar's `version.json` by the data pipeline, not copied from a wiki.
- Packets, registries, and block states must come from a **repeatable generator** that runs Mojang's official data reports against the official server jar. Hand-edited tables are not acceptable, because the data changes every drop.
- Moving to a newer release later is a new ADR, not a quiet edit.

## Consequences

- The login and status packet layouts written so far follow the 1.20.2-and-later form and are **unverified for 26.x**; they must be checked against generated packet data before play packets are built.
- Valence targets 1.20.1 and is in maintenance mode, so it is a reference only, not a dependency.
- The data pipeline derives from Mojang's game. Server jars are never committed. Whether generated data files may be committed is still open and is governed by ADR 0001 until reviewed.
- The Java API shim must match the Paper API for 26.3; its exact version string is still to be confirmed.

## Alternatives considered

- **1.20.1, reusing Valence's packets and data:** fastest start and a large plugin ecosystem, but years behind current clients and Valence is no longer actively developed.
- **Several versions at once:** more work up front; deferred until one version is solid.

## Open questions

- The 26.3 protocol number (from the server jar).
- The Paper API version string for the Java shim.
- Whether to follow every drop or pin to a release for a while.
