# Security policy

FM1-NTT is a one-maintainer hobby project with no releases yet: only the latest commit on `main` gets fixes.

## Reporting a problem

Report a vulnerability **privately**: the repository's **Security** tab, **Report a vulnerability** (GitHub private vulnerability
reporting). Do not open a public issue or pull request for it. Say what you found, how to reproduce it and what it lets someone do.
Best effort, about a week for a reply. If there is no such button, open a public issue that says only "I have a security report".

## What counts

- A firmware image (`.fwsc`, `.bin`, `.elf`, a save) that makes the core read or write outside its own memory, hang without bound,
  or allocate without bound. Images are untrusted input: the core must answer them with an error, never a crash (CLAUDE.md rule 5).
- The host or the web shell writing anywhere but the folder it was given, or talking to a network it was not asked to.
- A firmware, key or personal data committed to the repository or its history (docs/FIRMWARE.md).
- A build or CI weakness: an unpinned download, a missing checksum, a workflow with more access than it needs.

## What does not

- What a firmware does inside the emulated machine (it is the user's code, in a sandbox that is the program itself).
- Flashing a real FM-1: that is the firmware's and the device's matter (docs/fm-1_reference-kit.md, section 9).
- Weaknesses in SDL, Rust, Debian or the R36S's kernel: report those upstream.
