# Screenshot And Demo Capture

README uses the brand assets in `images/` and the preview screenshots in
`images/screenshots/`. Native Windows `.ico` embedding is later installer
polish for archive-free distribution; derive it from `images/galaxy.svg` when
that packaging stage starts so the repo icon and binary icon stay consistent.
Before tagging a public release, refresh visual proof of the actual app with
these captures:

- Windows Terminal TUI browser on a small fixture directory.
- Reclaim view showing review-oriented wording, not a destructive action.
- Text report output from `spacescan . --no-tui`.
- Benchmark output from `spacescan . --bench 3 --bench-warmup 0`.

Use deterministic fixture folders for screenshots where possible. Do not
capture personal paths, private filenames, usernames, or full-drive listings in
public screenshots or recordings.

Suggested Windows capture flow:

```powershell
cargo build --release
.\target\release\spacescan.exe . --no-tui
.\target\release\spacescan.exe .
.\target\release\spacescan.exe . --bench 3 --bench-warmup 0
```
