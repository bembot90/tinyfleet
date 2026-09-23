# Guards

A guard is a hook that refuses a class of mistake in a Bash command before
the command runs. fleet has four guard classes: shell-trap, record,
release-ref and production-write. You meet them in a Claude Code session with
the fleet plugin loaded, where every class judges every Bash command, and a
refusal says what is wrong, what to write instead, and how to run it anyway.
You switch classes off, and name what the last two refuse on, in
`fleet.toml`.

## Terms

- **Guard class**: one of the four, named as you type it after `fleet guard`.
  Each class judges the whole command text on its own.
- **Check**: one kind of mistake inside a class. A class runs its checks in a
  fixed order and refuses on the first that hits, so one class gives at most
  one refusal per command.
- **Target**: the setting a check needs before it can refuse anything: your
  item prefix, a ref glob, or a list of production names. A check whose
  target is not set refuses nothing.
- **Escape**: an assignment such as `FLEET_TRAP_OK=1` written at the front of
  the command, which lets that one command through the checks it names.

## How a guard judges a command

The fleet plugin wires four pre-tool hooks on Claude Code's Bash tool, one
per class, in this order: `fleet guard shell-trap`, `fleet guard record`,
`fleet guard release-ref`, `fleet guard production-write`. Each one reads
the payload Claude Code hands a pre-tool hook on standard input.

When a class refuses, it prints one JSON object on standard output: a
`PreToolUse` decision of `deny`, with the reason as its text. When it lets
the command through, it prints nothing. You can hand it a payload yourself:

```sh
$ echo '{"tool_name":"Bash","tool_input":{"command":"for b in $BRANCHES; do echo $b; done"},"cwd":"."}' | fleet guard shell-trap
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"fleet guard shell-trap: UNSPLIT VARIABLE — $BRANCHES → pass the items as explicit arguments, or read them one line at a time: printf '%s\\n' \"${BRANCHES}\" | while read -r item; do ...; done — and where ONE argument really is intended, quote it: \"${BRANCHES}\"; FLEET_TRAP_OK=1 <the same command> runs it anyway. The reason: the shell passes an unquoted variable as ONE argument, so this iterates or matches once over the whole newline-joined string and reads exactly like absence."}}
```

It exits 0. The reason always has the same parts, in this order:

```text
fleet guard <class>: <WHAT IT FOUND> — <the part of the command> → <what to write instead>; <the escape>. The reason: <why it is a mistake>.
```

The judging path exits 0 on every route: a refusal, a command let through,
and a payload with nothing to judge. A payload that is not JSON, is not a
Bash call, or carries a blank command prints nothing and exits 0.

An escape counts only when it is among the assignments that open the command
text, and it covers every statement after them: `FLEET_TRAP_OK=1 ls; for b in $LIST;
do :; done` is let through, and `cd <dir> && FLEET_TRAP_OK=1 <command>` is
judged as if the escape were not there. Any value turns an escape on except
an empty one, `0`, `false` and `no`.

A command a class cannot read, such as one with a quote left open, is let
through by shell-trap and release-ref. Record and production-write fall back
to a plain text match for it, described under each class, and say so in the
refusal: `(this command could not be read, so the conservative text match
applied)`.

The plugin runs the binary through its own `bin/fleet`. When that finds no
built binary, it prints `fleet: no built binary under <root>/target — run
cargo build --release in <root>, or set FLEET_BIN` on standard error and
exits 127, and no class judges the command.

## Where a guard reads its settings

A guard starts from the command's working directory, as the payload gives
it, and walks up to the first directory that has either file:

- **A directory with `.fleet/project.toml`** is a standalone project, even
  where a `fleet.toml` sits beside it. The on and off switches come from the
  fleet's own `fleet.toml`, the one the machine directory names. The targets
  come from the project's `.fleet/project.toml`.
- **A directory with `fleet.toml`** is an embedded fleet. The switches and
  the targets both come from that file.
- **Neither, all the way up**: the switches come from the fleet's own
  `fleet.toml`, and no target is set. With no machine directory naming a
  `fleet.toml`, every class is on.

The targets live in two tables:

| Key | Class | Check |
| --- | --- | --- |
| `[project] item_prefix` | record | bare-id |
| `[gates] release_ref_glob` | release-ref | push-target |
| `[gates] prod_buckets` | production-write | bucket |
| `[gates] prod_projects` | production-write | project |
| `[gates] prod_apps` | production-write | app |
| `[gates] prod_make_goals` | production-write | make-goal |
| `[gates] prod_dagger_functions` | production-write | module-function |
| `[gates] prod_workflow_refs` | production-write | workflow-ref |

`release_ref_glob` is a string. The other five `prod_` keys are lists of
strings. An empty string, an empty list and an absent key all leave the check
refusing nothing.

A file that cannot be read or parsed counts as an empty one: it switches no
class off and sets no target, and nothing is printed about it.

## Switching a class off

Every class is on unless its table says `enabled = false`:

```toml
[guards]
release-ref = { enabled = false }
production-write = { enabled = false }
```

Only the boolean `false` switches a class off. An absent table, an absent
key, and a value of any other type (`"no"`, `0`) leave it on. In a
standalone project the switches are read from the fleet's `fleet.toml` only;
a `[guards]` table in `.fleet/project.toml` or in a `fleet.toml` beside it
changes nothing.

`fleet prime`, which a session runs at its start, ends its first line with
each class and whether it is on, for example `guards: shell-trap off, record
on, release-ref on, production-write on`. "on" says the class runs, not that
its targets are set.

## The shell-trap class

Shell-trap refuses shell shapes that give a plausible wrong answer rather
than an error. It needs no target. Its escape is `FLEET_TRAP_OK=1`. Its five
checks, in the order it runs them:

| Check | Refuses | Write instead |
| --- | --- | --- |
| `record-backtick` | a backtick inside double quotes in text a `bd` command stores (see below) | the text in a file written through a quoted heredoc (`<<'EOF'`), passed as `"$(cat <file>)"` |
| `modifier` | `$NAME:` followed by one of `a c e h l q r s t u A P Q &`, unbraced | `"${NAME}:..."` |
| `unsplit-variable` | a lone `$LIST` as the whole list of a `for ... in`, as an argument to `xargs` or `kill`, or after a bare `--` | explicit arguments, a `while read` loop, or `"${LIST}"` where one argument is meant |
| `pipe-rc` | reading `$?` right after a pipeline whose last stage only formats | the command without the pipe, output to a file, then its own `$?` |
| `false-alternative` | `A && B \|\| C` where C only reports (`echo`, `printf`, `true`, `:`, `print`) and A has a third answer | `if A; then B; else ...; exit 1; fi` |

The stored text `record-backtick` reads is the text argument of `bd note`
and `bd comment`, the title and `--description`, `-d`, `--notes` and
`--append-notes` of `bd create`, `--reason` and `-r` of `bd close`, and
`--title`, `--description`, `-d`, `--acceptance`, `--design` and
`--append-notes` of `bd update`. A backtick inside single quotes, and text
passed as `"$(cat <file>)"`, are let through.

The stages `pipe-rc` treats as formatting are `head`, `tail`, `cat`, `cut`,
`sed`, `awk`, `tr`, `sort`, `uniq`, `fold`, `tee`, `nl`, `rev`, `column`,
`wc`, `less`, `more`, `pr`, `expand`, `unexpand` and `tac`. A pipeline ending
in `grep`, `diff`, `cmp` or `test` reports its own answer, so
`cargo test | grep -q ok; echo $?` is let through.

The A commands `false-alternative` knows a third answer for are
`git merge-tree`, `git merge-base`, `grep`, `diff`, `cmp` and `curl`. Any
other A is let through, so `test -e p && echo EXISTS || echo MISSING` is
let through.
`for b in $A $B` names its items one by one and is let through.

## The record class

Record refuses writes to items that destroy text, skip the audit row, or
leave an id a later reader cannot resolve. It reads `bd` commands only, by
that name or by a path ending in it. Its three checks, in order, each with
its own escape:

| Check | Refuses | Write instead | Escape |
| --- | --- | --- | --- |
| `notes-replace` | `bd update` with `--notes`, which replaces the whole notes field | `bd note <id> <text>`, or `bd update <id> --append-notes <text>` | `FLEET_NOTES_REPLACE_OK=1` |
| `sql-write` | `bd sql` with a statement that writes | `bd update`, `bd note` or `bd close` | `FLEET_SQL_WRITE_OK=1` |
| `bare-id` | an item named by its suffix alone in stored text | the full id, `<prefix>-<suffix>` | `FLEET_BARE_ID_OK=1` |

Each escape lets through its own check and no other:
`FLEET_SQL_WRITE_OK=1 bd update <item> --notes "done"` is still refused as
`NOTES REPLACED`.

`sql-write` reads the first keyword of the statement: `UPDATE`, `DELETE`,
`INSERT`, `REPLACE`, `DROP`, `ALTER` or `TRUNCATE` is a write, and a `WITH`
statement is a write when any of them follows. Words inside quotes are
skipped, so `bd sql "SELECT 'DELETE' FROM issues"` is let through.

`bare-id` needs `[project] item_prefix` and refuses nothing without it. It
reads the same stored text as `record-backtick`, not ids given as arguments,
so `bd show a1b2` is let through. A suffix it refuses is exactly four
characters of lowercase letters and digits, with at least one of each,
optionally followed by child numbers such as `.1`. It does not read a run
that is part of a longer word, a path, a file name (`a1b2.md`) or an id
already written in full. With `item_prefix = "acme"`, the refusal names each
suffix it found and the full id it stands for: `a1b2 -> acme-a1b2, c3d4.1 ->
acme-c3d4.1`.

For a command it cannot read, `notes-replace` and `sql-write` refuse any
`bd update` carrying `--notes`, and any `bd sql` carrying a write keyword, by
plain text match. `bare-id` lets it through.

## The release-ref class

Release-ref refuses a `git push` whose destination matches
`[gates] release_ref_glob`. It has no escape: the refusal prints `no escape
at this layer — a release is cut by a person from their own shell` where the
escape would be.

```toml
[gates]
release_ref_glob = "refs/heads/release/*"
```

In the glob, `*` matches any run of characters, a slash included, and `?`
matches one character. Every other character, brackets included, matches
itself. The glob is matched against the full ref name: a destination written
without `refs/` is read as `refs/heads/<name>`, so `git push origin
release/v1` is refused under `refs/heads/release/*`, and a glob written as
`release/*` matches no push at all.

The destinations it reads are every refspec after the remote (the part after
a colon, with a leading `+` dropped), the refs after `--delete` or `-d`, and
the ref named in `--force-with-lease=<ref>`. It steps over git's own flags
before `push`, such as `-C <path>`. A push that names no ref, such as a bare
`git push`, `git push --all` or `git push --mirror`, is let through.

## The production-write class

Production-write refuses commands that write to a production target the
project lists. Each list is the whole target set: fleet refuses a name
because it is on a list, never because of how it is spelled. It reads
`gsutil`, `gcloud`, `firebase`, `fly`, `flyctl`, `make`, `gmake`, `dagger`
and `gh`. Its escape is `FLEET_PROD_WRITE_OK=1`, for all six checks.

### bucket

`prod_buckets` lists bucket names, matched against the host of a
`gs://<bucket>/...` argument. It refuses `gsutil cp`, `mv`, `rm`, `rsync`,
`rb` and `setmeta`, and `gsutil acl`, `iam`, `defacl`, `notification`,
`retention`, `versioning`, `web`, `cors` and `lifecycle` unless the next word
is `get`, `list` or `describe`. It refuses `gcloud storage cp`, `mv`, `rm`
and `rsync`, `gcloud storage buckets create`, `delete` and `update`, and
`gcloud storage objects delete` and `update`. Reads such as `gsutil ls` and
`gsutil cat` are let through.

For `cp`, `mv` and `rsync`, a listed bucket anywhere in the arguments
refuses, so a download from a listed bucket is refused too, with the label
`A LISTED BUCKET, AND THIS VERB READS ITS DIRECTION FROM ARGUMENT ORDER`.

### project

`prod_projects` lists cloud project ids. It refuses:

- `gcloud run deploy`, `gcloud functions deploy` and `gcloud app deploy`;
- `gcloud sql`, `gcloud compute` and `gcloud iam` commands whose verb is not
  a read (`list`, `describe`, `get`, `get-value`, `get-iam-policy`,
  `get-config`, `info`, `version`, `help`, `print-access-token`,
  `print-identity-token`), a verb fleet does not recognise included;
- `gcloud config set project <listed id>`;
- `firebase deploy`, `firebase hosting:channel:deploy` and
  `firebase functions:delete`.

The project is the one named by `--project` (or `-p` for `gcloud`, `-P` for
`firebase`). When a command that mentions `gcloud` names none, fleet asks
`gcloud config get-value project`, waits up to five seconds, and refuses when
the answer is listed. When that cannot answer, the command is let through, and
so is a `firebase` command with no project flag.

### app

`prod_apps` lists application names. It refuses `fly deploy`, `fly scale`,
`fly secrets set`, and `fly machines` with any sub-verb other than `list`,
`status` and `show`; `flyctl` the same. The application is the one named by
`-a` or `--app`, or else the `app` key of a `fly.toml` in the command's
working directory. With neither, the command is let through.

### make-goal

`prod_make_goals` lists goals that deploy. `make` or `gmake` with a listed
goal among its words is refused, unless the last assignment to `dry` on the
line is written exactly `dry=1`: `make deploy-prod dry=1` is let through,
and `make deploy-prod dry=1 dry=0` and `make deploy-prod dry:=1` are
refused.

### module-function

`prod_dagger_functions` lists functions that write production. `dagger` with
a listed function among its words is refused, and so is one named inside the
script given to `-c` or `--command`.

### workflow-ref

`prod_workflow_refs` lists entries written `<workflow>:<ref glob>`, such as
`deploy.yml:release/*`. `gh workflow run <workflow> --ref <ref>` (or `-r`) is
refused when an entry's workflow and glob both match. The glob is matched
against the ref as you typed it, so `release/*` matches `--ref release/v1`. A
dispatch with no `--ref` is let through.

### A command it cannot read

For a command it cannot read, production-write looks at each part that
starts with one of its tools and refuses when any argument contains a listed
bucket, project or app, read or write alike. `gsutil ls "gs://<bucket>` with
its quote left open is refused this way.

## Checking that the targets are set

A check whose target is not set refuses nothing, and says nothing about it
when it lets a command through. `fleet guard <class> --check` prints one line
per check, in the order the class runs them, and exits 1 when any target is
not set:

```sh
$ fleet guard release-ref --check
release-ref push-target: not configured — [gates] release_ref_glob
```

It exits 1. With every target set, it exits 0:

```sh
$ fleet guard record --check
record notes-replace: configured
record sql-write: configured
record bare-id: configured — [project] item_prefix
```

A check with no target always prints `configured`, so
`fleet guard shell-trap --check` always exits 0. `--check` walks up from the
directory you run it in, the same way a guard walks up from the command's.
It does not read the `[guards]` switches: a class you switched off still
reports its checks, and still exits 1 when a target is not set.

The defaults every fleet gets carry a doctor entry, `guards-installed`, that
runs `--check` for shell-trap and record. The tiny pack shadows it with one
that runs all four. Either prints every line and exits with the first
non-zero status among them.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| A guard refuses a Bash command | 0 | one JSON object on standard output, `"permissionDecision":"deny"`, with the reason | write what the reason says instead, or put the class's escape at the front of the command |
| `fleet guard` with no class | 2 | `error: the following required arguments were not provided:` and `<CLASS>` | name one of the four classes |
| `fleet guard` with a class it does not know | 2 | ``error: invalid value 'shell' for '<CLASS>': unknown class `shell` — one of shell-trap, record, release-ref, production-write`` | use one of the names it lists |
| `--check` finds a target not set | 1 | `<class> <check>: not configured — <key>` | set the key the line names, or leave it unset if the check does not apply to you |
| The plugin finds no built binary | 127 | `fleet: no built binary under <root>/target — run cargo build --release in <root>, or set FLEET_BIN` | build fleet, or set `FLEET_BIN` to the binary's absolute path |

## See also

- [Getting started](getting-started.md): installing fleet and loading the
  plugin that wires the guards into a session.
- [Packs](packs.md): the defaults and the tiny pack, which carry the
  `guards-installed` doctor entry, and how one pack's file shadows another's.
- [Items and the record](items.md): the notes and fields the record class
  protects.
- [The controller and seats](seats.md): the sessions the controller starts,
  and the plugin they load.
- [Exit codes and conventions](conventions.md): the exit table, and why items
  are named by their full id.
