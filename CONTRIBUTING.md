# Contributing

Read `CLAUDE.md` (the rules) and `ROADMAP.md` (what is next, in order) first.

- **Licence of what you add:** MIT. Do not paste code from a GPL, AGPL or LGPL emulator or from M-VAVE's FM1 Open (GPLv3), and do not
  commit firmware or vendor SDK files. `docs/LICENSING.md` says what may be read, what may be copied and what must be written fresh.
- **Check before a pull request:** `scripts/check.sh` (core tests, host build and self-test, web test, shellcheck). Say which tools
  were not installed rather than calling the run clean.
- **Say what you did not run on a real R36S or a real FM-1.** "Untested" is a normal word here.
- **Change `ROADMAP.md` and `CHANGELOG.md` in the same commit as the work.**
