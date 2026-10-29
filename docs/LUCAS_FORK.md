# Lucas Jcode fork

The fork tracks `1jehuang/jcode:master` using merges, preserving the upstream
history and the original authors of local changes. Do not replace it with an
official release binary or force-reset the fork to upstream.

The October 2026 integration preserves these changes:

- Project-scoped resume, including subdirectories and sibling worktrees. Upstream
  now supports this natively through `display.resume_project_scope`; the fork
  launcher enables `JCODE_RESUME_PROJECT_SCOPE=1` while retaining the new async
  session loader and the user's explicit filter selection.
- Maestri terminal image probing, full-color iTerm2 image output when its image
  addon advertises sixel, and the probed terminal cell size.
- Cancellation of blocking control/API turns, including provider opening,
  silent streams and foreground tools. Busy sessions do not block connection
  readers. The integration retains upstream background handoff, recovery
  deduplication and worker output handling.

## Installing a validated build

Build with a recorded source commit and retain a SHA-256 checksum. Use a Linux
build environment compatible with every target, rather than assuming a binary
built on a rolling distribution will run on older servers. The installer needs
Python 3.11+ and Linux; it manages the standard per-user Jcode installation.

```sh
python3 scripts/install_lucas_fork.py /path/to/jcode \
  --sha256 ACTUAL_SHA256 --source-commit BUILD_COMMIT --label UNIQUE_BUILD_LABEL
# Apply only after the focused unit and isolated runtime checks passed:
python3 scripts/install_lucas_fork.py /path/to/jcode \
  --sha256 ACTUAL_SHA256 --source-commit BUILD_COMMIT --label UNIQUE_BUILD_LABEL \
  --apply --reload
```

The first invocation only checks the artifact and reports the plan. Installation
keeps old immutable versions and writes private backups/receipts under
`~/.local/state/jcode-fork/`. It compares local rules, configuration and MCP
files by hash after installation. It does not copy these files between machines
or change their contents. Existing preference migrations are preserved; absent
migration markers are marked complete so the upgrade keeps the user's choices.

The launcher pins project resume and disables official auto-update with
`JCODE_CHECK_UPDATES=0`, so an official download cannot silently remove local
patches. An existing `JCODE_DISABLE_CLAUDE_MCP` launcher policy is retained.
Promotion and reload use the native Jcode commands; a running shared daemon is
verified through its actual `/proc` executable and checksum, not just symlinks.

## Updating the fork again

Use Chronosync to inventory and snapshot Jcode source/WIP across reachable
machines before integrating upstream. Report offline machines explicitly and
recheck their private commits/WIP when they return. Never infer ancestry from
file timestamps or install over unreviewed machine-specific source changes.

Merge upstream and relevant custom branches, preferring equivalent native
configuration options when available. Validate cancellation through terminal
and API, background handoff, reload recovery, project resume, Maestri protocol
selection and instruction loading. An isolated daemon with a synthetic HTTP
provider exercises runtime behavior without real model requests. Do not run
fuzzing or restart unrelated sessions/services to obtain evidence.

Promote the validated integration to the fork's primary branch and distribute
the same checked artifact to machines already using Jcode. Do not install a
watchdog, timer or recurring update job as part of this manual update.
