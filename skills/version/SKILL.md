---
name: version
description: Report the version of the fleet binary this session resolves, by running fleet --version.
---

# version

Run `fleet --version` in the shell and report the line it printed, verbatim.

If the command is not found, or exits non-zero, say so and quote what it
printed instead — the answer to this skill is what the session's own `fleet`
did, never a version read from anywhere else.
