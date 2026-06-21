# Contributing

Thanks for helping make `spacescan` easier to understand and safer to use.

## Code Standards

- Use constants, not magic literals. Reusable strings, labels, defaults, thresholds, filenames, and routing values belong in `src/constants.rs`.
- Prefer intention-revealing helper boundaries. Long methods should become small helpers that each do one step and can be tested independently.
- Keep control flow shallow. Use early returns and keep nesting depth at 2 whenever practical.
- Prefer names that read with parameters while staying idiomatic Rust `snake_case`, such as `build_options_from`, `render_header_for`, or `parse_size_from`.
- Separate domain decisions, mapping, parsing, rendering, and side effects.
- When touching existing code, move it toward these rules instead of preserving avoidable complexity.
- Write code for junior developers first: plain names, small modules, focused comments, and tests that describe behavior.

## Verification

Run these before opening a pull request:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo bench --no-run
```

For local Windows scanner timing:

```powershell
spacescan . --bench 3 --bench-warmup 0 --bench-json benchmark-results\repo.json
```

Treat local timing as a directional signal. Benchmarks that depend on machine
state should be documented clearly and should not be used as release gates.

For release prep:

```powershell
cargo package --list
cargo publish --dry-run
$metadata = cargo metadata --format-version 1 | ConvertFrom-Json; $metadata.packages | Sort-Object name | Select-Object name, version, license
```

`cargo package --list` and `cargo publish --dry-run` are release gates and
should run from a clean, tracked worktree. Before the first public commit, use
`cargo package --list --allow-dirty` only to preview package contents; do not
publish from a dirty tree. Cargo may list a generated `Cargo.toml.orig` in the
crate package; that is expected and is not a local privacy artifact.

The tag-driven release workflow repeats the release gate before packaging.

## Benchmarks

Benchmarks should use deterministic generated fixtures where possible. If a benchmark depends on local machine state, document that clearly and do not use it as a regression gate.
