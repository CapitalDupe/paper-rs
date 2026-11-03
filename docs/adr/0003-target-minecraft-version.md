# ADR 0003: Target Minecraft Java version

- Status: Accepted (one verification gate open; see "Verification gate")
- Date: 2026-11-03

## Context

The first vertical slice needs exactly one Minecraft Java release as its protocol target (see `docs/COMPATIBILITY.md`). The choice affects protocol work, the plugin ecosystem we can test against, and which Paper API version the shim must match.

Facts gathered on 2026-11-03 (third-party sources; re-check before relying on them):

- Mojang moved to year-based numbering. The recent releases are 26.1 (2026-03-24), 26.2 (2026-06-16) and 26.3 (2026-09-15); 26.1.1 and 26.1.2 were hotfixes ([release history](https://minecrafthub.io/articles/minecraft-version-history)).
- 26.3 is the newest release found. A hosting guide says 26.4 is planned for Q4 2026 ([guide](https://hostvalues.net/en/blog/minecraft-26-3-server-guide)).
- Paper lists 26.3 as its latest stable release, with Java 25 required. Every older version on Paper's download list shows as a final build, so older ecosystems are no longer moving ([Paper builds](https://mcjarhub.com/jar/paper)).
- One source reports the 26.3 protocol version as 777 and says a 26.2 client cannot join a 26.3 server ([guide](https://hostvalues.net/en/blog/minecraft-26-3-server-guide)). No second source confirmed this number.

## Options

1. **Latest stable release.** Newest features and clients, but the plugin ecosystem may lag and the protocol is still moving.
2. **A widely used release the plugin ecosystem supports well.** Easier to find reference plugins, but older clients and a protocol further from current.
3. **Latest first, add a second version later through explicit protocol mappings.** More work up front; deferred until one version is solid.

## Decision

Target **Minecraft Java Edition 26.3** only, following option 3: latest first, with any second version added later through explicit protocol mappings and a new ADR.

Reasons:

- Paper has already moved its development to 26.3 and treats older versions as final, so the plugin API we shim will not move backwards.
- Choosing an older version now would mean implementing a protocol and API that Paper itself has stopped updating.
- One version keeps the first vertical slice small and testable.

Consequences:

- The Java shim (`api/`) targets the Paper API for 26.3. The exact artifact version is recorded when it is chosen, after the license check required by ADR 0001.
- The JVM bridge (ADR 0002) must support Java 25, the version Paper 26.3 requires. Open question 1 in ADR 0002 now has a starting answer.
- If 26.4 ships before the first playable milestone (M3), we stay on 26.3 and revisit with a new ADR. We do not chase releases mid-milestone.
- Supporting 26.3 means older clients are rejected with a clear disconnect message, never a silent failure.

## Verification gate

The protocol version number is not confirmed. Before any packet code depends on it (roadmap M1 handshake and status work):

1. Confirm the number against a second, authoritative source for 26.3.
2. Confirm it from a real 26.3 client by capturing its handshake packet, and keep the capture as a golden test fixture.
3. Record the verified number in `docs/COMPATIBILITY.md` and change this section to "Closed".

If the number differs from 777, only this ADR's context and `COMPATIBILITY.md` change; the decision itself stands.

## Still open

- Client-side tooling for CI. A handwritten test client for status and login is expected; whether a headless real client can run in CI has not been investigated.
- The exact Paper API artifact version for the shim, pending the ADR 0001 license check.
