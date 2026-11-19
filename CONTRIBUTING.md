# Contributing

diamond-stats is dual-licensed under MIT OR Apache-2.0. By submitting a contribution you agree that it may be distributed under those terms.

## What belongs here

Code, tests, and documentation. Do not commit fetched MLB Stats API responses, the on-disk cache, or Parquet output. Tests use the trimmed fixtures already in the repo; add a new fixture only when it is a small excerpt of a real response, and say so in `crates/core/fixtures/README.md`.

Do not add a scraper for FanGraphs or Baseball-Reference.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

`cargo test` does not use the network. The DuckDB client downloads a prebuilt shared library at build time; `cargo test` and `cargo run` put that library on the loader path.

## Pull requests

Open a PR against `main`. Describe the behavior change and how you tested it. Keep the diff focused.
