# Contributing

Thanks for your interest in paper-rs. The project is early-stage, so design feedback and small, well-scoped contributions are especially useful.

## Before starting

- Check existing issues and open pull requests.
- For a substantial feature or architecture change, open an issue to discuss the approach first.
- Keep pull requests focused and explain the motivation and trade-offs.
- Do not submit code copied from other projects unless its license and provenance have been reviewed and documented.

## Development setup

Install a stable Rust toolchain, then run:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The current workspace is a scaffold; passing these commands does not imply Minecraft server compatibility.

## Code expectations

- Prefer clear, idiomatic Rust and avoid unsafe code.
- Validate untrusted network input and place explicit limits on allocations and parsing work.
- Add unit or integration tests for behavior changes.
- Document public APIs and non-obvious invariants.
- Do not add performance claims without a reproducible benchmark.
- Keep logs useful and never include secrets.
- Update the roadmap and compatibility notes when project status changes.

## Pull requests

Include:
1. What changed and why.
2. Tests and commands run.
3. Any protocol/game-version impact.
4. Security, performance, or migration considerations.
5. Follow-up work or known limitations.

Be respectful and constructive in issues, reviews, and discussions.
