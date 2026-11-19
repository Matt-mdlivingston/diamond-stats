## Summary

<!-- What changed and why. -->

## Testing

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (no network)
- [ ] Live `ingest` / `serve` checked, if the change touches the Stats API client or the quickstart

## Data

- [ ] No fetched responses, cache files, or Parquet outputs are committed
