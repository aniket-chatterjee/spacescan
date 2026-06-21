# Roadmap

This roadmap keeps public work easy to understand without promising dates.

## v0.1.0

- Windows-first CLI and TUI release with safe delete guards.
- Portable walker as the only production scan engine.
- Reproducible benchmark JSON for performance reports.
- Brand assets from `images/` used in the README and release archives; derive
  any future Windows executable icon from `images/galaxy.svg`.
- Windows and Linux archive packages, with CI smoke checks for both package
  scripts.

## Next

- Continue profiling and tuning the portable walker before adding broader
  feature work.
- Add refreshed TUI screenshots or an asciinema demo once the v0.1 interface is
  stable.
- Keep improving reclaim explanations and selected-item details without making
  delete actions easier to trigger accidentally.

## Later

- Add installer formats after archive packages are reliable.
- Expand Linux and macOS smoke coverage as contributors can verify platform
  behavior.
