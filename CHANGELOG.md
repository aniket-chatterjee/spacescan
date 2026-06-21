# Changelog

## 0.0.1 - 2026-06-21

- Added the Windows-first `spacescan` CLI/TUI for scanning disk usage, browsing
  large directories, exporting JSON/CSV, and reviewing reclaimable space.
- Kept the production scanner on the proven parallel filesystem walker and
  removed experimental alternate scanner paths.
- Added centralized constants for defaults, UI labels, report/export headers,
  benchmark values, and platform commands.
- Refactored binary orchestration into a runner module with testable scan,
  export, report, benchmark, reclaim, and TUI handoff helpers.
- Added characterization coverage for reports, exports, TUI state, CLI
  behavior, reclaim logic, delete guards, and benchmark summaries.
- Added machine-readable benchmark JSON with warmup control, sample count
  stability, throughput, memory, represented tree storage, and directory shape
  telemetry.
- Added deterministic Criterion workloads for file-only, directory-only,
  fanout, nested fanout, wide, deep, mixed, export, report, reclaim, filtering,
  and sorting behavior.
- Added benchmark JSON, license metadata checks, and Windows/Linux archive
  packaging through CI release workflows.
- Added open-source project files, release workflow checks, README, license,
  security policy, roadmap, screenshot notes, and Windows/Linux archive
  packaging.
- Added `--exclude <PATH>` and `--prune-zero-size` scan-shaping options with
  benchmark JSON recording the selected profile.
- Reduced scan-tree memory with boxed names, boxed child collections, and
  `ThinVec<Node>`, while preserving public report/export behavior.
- Added `mimalloc` for the release binary after local benchmarking improved
  scanner throughput.
