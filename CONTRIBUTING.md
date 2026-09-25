# Branching, commits and releases

Kept simple on purpose. Three kinds of branch, one commit shape, and one way
work reaches `main`.

## Branches

| Branch | Cut from | What lands on it | How it leaves |
| --- | --- | --- | --- |
| `main` | — | release squashes and hotfix squashes, nothing else | never |
| `release/<x.y.z>` | `main` | one squash commit per item, for that release | squash-merged into `main` |
| `hotfix/<x.y.z>` | `main` | the fix, as one or more item squashes | squash-merged into `main` |
| work branches (a seat's worktree, `worktree-agent-*`) | the open release branch | anything | squashed onto the release branch, then deleted |

- **Cut the release branch first.** Before any work for a release starts,
  `release/<x.y.z>` is cut from `main`, and its first commit sets the version
  in every crate's `Cargo.toml` to `x.y.z`.
- **Work lands on the release branch, never on `main`.** A builder's branch is
  cut from the open release branch, and the reviewer squash-lands it there.
- **One release branch open at a time.** Anything for a later version waits
  on its bead until the next release branch is cut.
- **Nobody pushes to `main` directly.** `main` changes only by the squash
  merge of a release or hotfix branch.

## Commits

Every commit on a release or hotfix branch is **one item, squashed**:

    <full item id>: <what changed, in the present tense>

    <why, and anything a reviewer needs; wrapped at 72>

    Co-Authored-By: …

- The subject starts with the item's **full id** (`fleet-cq6`, never `cq6`).
  The release changelog is built from these subjects, so write them for a
  reader of the changelog.
- One item per commit. No merge commits, no "fix review comments" commits:
  the squash hides the work branch's history.
- The item's own bead records the verdict and the landing; the commit does
  not repeat them.
- Board state (`.beads/`) is committed on the branch the work is on, as its
  own commit (`beads: …`).

## Supported versions

The releases of the tools fleet runs on that it supports are declared in one
place, `core/src/supported.rs`:

| Tool | Constant | Where it is declared | Checked by |
| --- | --- | --- | --- |
| bd | `PINNED_BD` | `core/src/store/mod.rs`, named again in `supported.rs` | `doctor/bd-version`, `fleet prime`'s second line |
| Claude Code | `PINNED_CLAUDE_CODE` | `core/src/supported.rs` | `doctor/claude-code-version`, the controller's `substrate.moved` when a fleet pins none |

Deno is a pack's, not the binary's: `packs/ts/pack.toml`'s `[runtime]` table,
measured by `doctor/runtime-version`. git carries no pin.

**A supported-version move is one constant plus a re-measure**, as one item:

1. Change the constant, and the one copy of it in its doctor check's `run.sh`
   (`PINNED=` or `SUPPORTED=`). The core suite fails until the two agree.
   The install pointers follow the copy: Claude Code's check names
   `claude install <version>` and the native installer at that version, and
   bd's check and `fleet prime`'s second line link beads' installation page
   at the pin's tag
   (`github.com/gastownhall/beads/blob/v<pin>/docs/getting-started/installation.md`).
   For bd, confirm that page exists at the new tag before the move lands; no
   suite can.
2. Re-measure every claim that rests on the old release, on the new one: for
   bd, every "measured on" comment in `core/src/store/mod.rs`; for Claude Code,
   the version-scoped entries in `brain/lessons/claude-code.md` the adapter
   reads. Restate each one that held, and change the code where a behaviour
   moved.
3. The version is also in the "What fleet runs on" table of
   `docs/getting-started.md`, which only the docs skill edits: ask for the
   edit on the item's bead, or the release pass catches it up.

## Releasing

1. **The docs pass.** When the branch's work is done, run the docs skill's
   release pass (`.claude/skills/docs/SKILL.md` § 8) over the items landed on
   the branch. It lands as the branch's last item, subject
   `<item id>: docs release pass for <x.y.z>`.
2. **Open the release PR** from `release/<x.y.z>` to `main`.
3. **Squash-merge it.** The squash commit's subject is `release: v<x.y.z>`.
4. **The release workflow runs on that push to `main`**
   (`.github/workflows/release.yml`). It:
   - reads the version from `Cargo.toml` and refuses if the tag `v<x.y.z>`
     already exists or the version does not match the merged branch's name;
   - refuses if the branch carries no docs release pass;
   - tags `v<x.y.z>`;
   - builds the changelog from the release branch's commit subjects, grouped
     by item, and creates the GitHub release with it;
   - publishes `docs/` for that version (the renderer is not chosen yet).
5. **Delete the release branch**, and cut the next one when work for it
   starts.

## Hotfixes

A fix that cannot wait for the open release:

1. Cut `hotfix/<x.y.z+1>` from `main`, bump the patch version as its first
   commit, and land the fix as an item squash.
2. Squash-merge it into `main` as `release: v<x.y.z+1>`. The release workflow
   runs the same way (a hotfix's docs pass may be a no-op when no behaviour a
   user sees has changed; it still lands, saying so).
3. Cherry-pick the fix onto the open release branch, so the next release does
   not undo it.
