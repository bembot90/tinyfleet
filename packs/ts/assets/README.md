# The ts pack

The TypeScript layer of a fleet: the pack that pins the workflow runtime and
the pack the SDK lives in. tiny imports it, and any other opinion pack imports
it the same way. It imports nothing itself, so it sits between the opinion
pack above it and the binary's own defaults below.

This file lives under `assets/` because a pack's top level holds the manifest
and the eight slots and nothing else; `fleet pack check` refuses a tenth name,
and reads past only the litter a file browser leaves, such as `.DS_Store`.

What it holds today:

- `pack.toml` — the `[runtime]` table: Deno, pinned by version, with the
  bundle and run lines core substitutes and execs. Core knows nothing else
  about a workflow's language; the pack owns the spelling of both lines.
- `doctor/deno-version` — the check that asks the resolved deno for its
  version and reads red when deno is absent or another version. It looks on
  PATH first and in the installer's own bin second — `$DENO_INSTALL/bin` when
  that root is set, `~/.deno/bin` otherwise, neither of which a session PATH
  carries on its own — and prints which it used.

- `assets/sdk/mod.ts` — the SDK's core: `workflow(fn)` and the run handle
  whose every call is a numbered step. On start it reads the run's closed
  steps off the stream through `fleet event tail --json`; at step n it
  returns the recorded result under the same name, or writes the pair
  through `fleet event step`, runs the step and records it. `now`, `random`
  and `input` are steps. A result over 64 KiB goes to `steps/<n>.json` in
  the run directory with its sha256 on the event. A `Waiting` thrown from a
  step is exit 2 with its condition, as JSON, on stdout's last line; any
  other throw is exit 1 with its reason there the same way; a name mismatch
  at n is exit 1 with `replay diverged at step n`. Under `assets/` for the reason this file is.
  Its suite is `assets/sdk/mod_test.ts`, under Deno's own runner: `make
  fleet-ts-test`, a test-tools row of its own beside fleet-test.

## The verbs

The run handle carries six verbs beside `step`, `now`, `random` and
`input`. Each is one numbered step whose exec runs the fleet binary with
`--json` and records the envelope's `data` as the step's result, so a re-run
returns what the first run was told and spawns nothing. Every act is
attributed `--by run:<id>`, the run typed as an actor — the run's child starts
with a cleared environment, and the run is the one name every act under it can
carry. A
refusal envelope (`ok: false`) is a thrown `Refusal` carrying the verb, the
refusal code and its why; the wrapper turns it into exit 1 with the reason
`{"verb", "code", "why"}` on stdout's last line.

An item is read off its store and never off the stream: `spawn`, `hold` and
`until` read `fleet item show <id> --json` and fold the timeline of typed
entries it prints; a read the store refuses is a `Refusal` like any verb's. A
step that cannot close waits on `{ items, kinds, since }` — the items to read
again, the entry kinds any of which could satisfy it, and `FLEET_STREAM_SEQ`,
where the execution started — and the controller re-runs the workflow on a
line for one of those items and kinds above that position.

Two verbs leave something open across a Waiting exit and must not repeat it
on the re-run: a hold's question and a start's child run. Each finds its own
record by the actor — the k-th ask this run held on its own record is its
k-th `hold`, and the k-th `run.started` it raised on the stream its k-th
`start` — so neither holds nor starts twice. Their suite is
`assets/sdk/verbs_test.ts`, against a fake binary on `FLEET_BIN` that answers
`item show` from the records an arm plants and `event …` with the real one.

- `spawn({ role, item, model?, touched? })` — a builder on an item, over
  `fleet dispatch <item> [--touched <command>] --json`: the verb that cuts a
  transient seat, writes the order and rings it. `fleet seat spawn` alone
  carries neither an item nor a role, and a seat with no order could never
  deliver. The one role is `"builder"` (a reviewer's spawn is the flight's
  own), and a hand-run dispatch pins no model, so a `model` given is refused
  rather than dropped. `touched` is the builder's checks its brief names;
  without one the brief names the absence. An item whose record carries a
  delivery no return and no landing followed is carried — delivered by a run
  that failed and left it behind — and the step closes on `already delivered
  at <commit>` without a dispatch.

  ```ts
  const { seat } = await run.spawn({ role: "builder", item: "item-12" });
  ```

- `review(item, verdict)` — `fleet review <item> --json` with `--land` for
  `"accepted"` or `--return <file>` for `{ returned: file }`; the result's
  `state` is the one the verdict moved the item to. The findings file is JSON
  of the shape core's `assets/findings.schema.json` gives, one entry per
  finding, written under the run directory; the verb numbers them `F1`, `F2`,
  and refuses a file that does not read with exit 2 before it writes anything.

  ```ts
  await run.review("item-12", "accepted");
  const findings = `${run.env.runDir}/findings.json`;
  await Deno.writeTextFile(
    findings,
    JSON.stringify({ findings: [{ text: "the one finding" }] }),
  );
  await run.review("item-13", { returned: findings });
  ```

- `land(item, sha, { test? })` — `fleet land <item> <sha> [--test <command>]
  --json`; the result carries the landed sha, read by the verb from the push's
  own range line. `test` is the command the landing runs on the rebased tree
  under its lock, before the push; without one the landing runs nothing and
  its note says NOT TESTED. A project's policy names no test command: the
  workflow hands it in.

  ```ts
  const { sha } = await run.land("item-12", "0123abc", { test: "make check" });
  ```

- `hold(question, options, about?)` — a question for a person on the run's
  own record item: the file `holds/<k>.json`, JSON of the shape core's
  `assets/question.schema.json` gives, each option `<letter>. <text>` taken
  apart into its `letter` and its `text` (an option of any other shape throws
  before anything is written), then `fleet hold --item <run> --question <file>
  --json`, then Waiting on the run's record's `cleared` entries. Once the
  hold's clearance is on the record the re-run closes the step with its
  letter, or with `"cancelled"` where the hold was cancelled. `about` —
  `{ items, commit?, licenses }` — names the items the answer licenses and
  the letter that licenses them, and rides in the file as the schema's
  `about`.

  ```ts
  const letter = await run.hold("Ship the report?", ["A. yes", "B. not yet"]);
  ```

- `until(items, state)` — reads each item's record and throws Waiting whose
  `items` are exactly the outstanding ones, in the order given; once every
  item's record answers it returns each item's answering entry. The states
  are `dispatched` (the current order), `delivered` (the latest delivery no
  return followed; a landing does not withdraw it), `reviewed` and `returned`
  (the last verdict, where it is that one and no delivery followed it),
  `landed` (the last landing) and `held` (the open hold).

  ```ts
  const landed = await run.until(["item-12", "item-13"], "landed");
  ```

- `start(name, inputs?)` — a child run over `fleet run <name> --input k=v …`
  (a string value as given, any other as JSON). It returns `{ run }` once the
  child's `run.closed` is on the stream, throws with the child's reason on
  `run.failed`, and otherwise waits on the child's run id.

  ```ts
  const { run: child } = await run.start("preboard", { items: ["item-12"] });
  ```

The takeoff workflow of fleet-layers.md, in these verbs:

```ts
// packs/tiny/workflows/takeoff.ts
import { workflow } from "fleet";

export default workflow(async (run) => {
  const items = await run.input("items");
  for (const item of items) await run.spawn({ role: "builder", item });
  await run.until(items, "landed");
  await run.hold("Ship the report?", ["A. yes", "B. not yet"]);
});
```

Deno is installed per box by its own installer into `~/.deno/bin`, and the
toolchain export every seat evals appends that directory to PATH; the pin
here is the version that installer wrote on the box this pack was measured on.
