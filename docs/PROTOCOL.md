# Protocol reference

What `paper-protocol` implements for the target version (Minecraft Java 26.3, see
[ADR 0003](adr/0003-target-minecraft-version.md)), where each fact came from, and how
far it has been verified.

## Verification status

Nothing here has been checked against a real 26.3 client yet. Until a handshake capture
exists as a test fixture, treat every layout and number below as *documented but
unconfirmed*. All byte fixtures in the tests are built by hand from the layouts below.

| Fact | Value | Sources | Status |
|------|-------|---------|--------|
| Protocol version for 26.3 | 777 | Two independent sources: a hosting guide, and the data tables of another Rust server implementation (which also lists 26.2 as 776) | Two sources agree; real-client capture still open |
| Packet ids below | see tables | The same implementation's generated id tables; consistent with the long-standing ids of these states | Unconfirmed against a client |
| Field layouts below | see tables | Protocol documentation for the 1.21 line | Unconfirmed for 26.3 |

## Provenance

Protocol numbers and packet ids are interoperability facts, not creative expression.
They were read from the data tables of Pumpkin (GPL-3.0), a third-party server, as a
cross-check. **No code, data file, or generated table was copied from it.** Pumpkin's
own contributor guidance says it is developed against decompiled vanilla sources, so it
is treated strictly as a source of facts to verify, never of code, in line with
[ADR 0001](adr/0001-license-and-mojang-code-boundary.md) and `CONTRIBUTING.md`. Every
implementation in this repository is written independently from the layouts documented
here.

## Framing

- A frame is a `VarInt` length followed by that many bytes (packet id, then body).
- The length prefix is at most 3 bytes, so a frame is at most 2,097,151 bytes.
- A zero-length frame is invalid.
- With compression enabled, the frame body starts with a `VarInt` data length: `0` means
  the rest is uncompressed; otherwise it is the uncompressed size and the rest is zlib
  data. A compressed packet must be at least the threshold, and at most 8 MiB, when
  uncompressed.
- After login encryption, all bytes in both directions pass through AES-128 in CFB8
  mode, with the shared secret as both key and IV.

## Handshaking state

| Id | Direction | Packet | Fields |
|----|-----------|--------|--------|
| `0x00` | Serverbound | Handshake | protocol version (`VarInt`), server address (string, 255), port (`u16`), next state (`VarInt`: 1 status, 2 login, 3 transfer) |

## Status state

| Id | Direction | Packet | Fields |
|----|-----------|--------|--------|
| `0x00` | Serverbound | Status request | none |
| `0x01` | Serverbound | Ping request | payload (`i64`) |
| `0x00` | Clientbound | Status response | JSON (string, 32767) |
| `0x01` | Clientbound | Pong response | payload (`i64`) |

## Login state

| Id | Direction | Packet | Fields |
|----|-----------|--------|--------|
| `0x00` | Serverbound | Login start | name (string, 16), UUID (16 bytes) |
| `0x01` | Serverbound | Encryption response | shared secret (bytes), verify token (bytes) |
| `0x03` | Serverbound | Login acknowledged | none |
| `0x00` | Clientbound | Login disconnect | reason (JSON string, 262144) |
| `0x01` | Clientbound | Encryption request | server id (string, 20), public key (bytes), verify token (bytes), should authenticate (bool) |
| `0x02` | Clientbound | Login success | UUID, name (string, 16), properties (up to 16: name, value, optional signature) |
| `0x03` | Clientbound | Set compression | threshold (`VarInt`; negative disables) |

Strings are limited in UTF-16 code units and encoded as a `VarInt` UTF-8 byte length
followed by the bytes. Every length, count and string limit is checked before data is
read.

## Not yet implemented

The configuration and play states, cookie and custom-query login packets, the legacy
pre-1.7 server list ping, and the online-mode session check with Mojang's servers.

## Closing the verification gate

1. Capture a real 26.3 client's handshake, status and login packets.
2. Add them as golden fixtures and replace the hand-built ones in the tests.
3. Update the table above and ADR 0003's gate to "Closed".
