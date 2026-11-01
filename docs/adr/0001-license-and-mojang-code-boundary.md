# ADR 0001: License and Mojang-code boundary

- Status: Accepted
- Date: 2026-11-01

## Context

paper-rs aims to run existing Paper plugins. Paper and Bukkit are GPL-licensed. The repository started under MIT with a rule against reusing Paper code, which would prevent following Paper's own structure.

## Decision

The project is licensed **GPL-3.0-only**. Paper's original code may be followed or reused, with provenance (source, commit, license) recorded in the pull request.

The boundary: Mojang's proprietary server code, decompiled or deobfuscated Minecraft sources, and Paper patches that modify that code must **not** be copied. Mojang-derived behavior is reimplemented from protocol documentation, observed behavior, and tests.

Before bundling or depending on any Bukkit/Paper API artifact, its exact license terms are checked and recorded.

## Consequences

- All contributions and dependencies must be GPL-3.0-compatible.
- Pull requests carry a provenance checklist.
- This is an engineering policy, not legal advice; get legal review before any release.

## Alternatives considered

- Stay MIT with a clean-room reimplementation of the API: safer for relicensing, but forbids following Paper's structure and code.
