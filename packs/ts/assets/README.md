# The ts pack

The TypeScript layer of a fleet: the pack that pins the workflow runtime and
the pack the SDK lives in. tiny imports it, and any other opinion pack imports
it the same way. It imports nothing itself, so it sits between the opinion
pack above it and the binary's own defaults below.

This file lives under `assets/` because a pack's top level holds the manifest
and the eight slots and nothing else; `fleet pack check` refuses a tenth name.

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
  step is exit 2 with `{"waiting": …}` on stdout's last line; any other
  throw is exit 1 with `{"reason": …}`; a name mismatch at n is exit 1 with
  `replay diverged at step n`. Under `assets/` for the reason this file is.
  Its suite is `assets/sdk/mod_test.ts`, under Deno's own runner: `make
  fleet-ts-test`, a test-tools row of its own beside fleet-test.

## The verbs

The run handle carries seven verbs beside `step`, `now`, `random` and
`input`. Each is one numbered step whose exec runs the fleet binary with
`--json` and records the envelope's `data` as the step's result, so a re-run
returns what the first run was told and spawns nothing. Every act is
attributed `--by` the run id — the run's child starts with a cleared
environment, and the run is the one name every act under it can carry. A
refusal envelope (`ok: false`) is a thrown `Refusal` carrying the verb, the
refusal code and its why; the wrapper turns it into exit 1 with
`{"reason": {"verb", "code", "why"}}` on stdout's last line.

Two verbs leave something open across a Waiting exit and must not repeat it
on the re-run: a gate's ask and a start's child run. Each finds its own record
on the stream by the actor — the k-th `item.parked` and the k-th `run.started`
this run raised are its k-th `gate` and k-th `start` call — so neither asks
nor starts twice. Their suite is `assets/sdk/verbs_test.ts`, against a fake
binary on `FLEET_BIN` that answers `event …` with the real one.

- `spawn({ role, item, model? })` — a builder on an item, over
  `fleet dispatch <item> --json`: the verb that cuts a transient seat, writes
  the order and rings it. `fleet seat spawn` alone carries neither an item
  nor a role, and a seat with no order could never deliver. The one role is
  `"builder"` (a reviewer's spawn is the flight's own), and a hand-run
  dispatch pins no model, so a `model` given is refused rather than dropped.

  ```ts
  const { seat } = await run.spawn({ role: "builder", item: "item-12" });
  ```

- `deliver(item, note)` — `fleet deliver --item <item> --note <file> --json`;
  the note is a file in the delivery-note grammar, written under the run
  directory, which is the one place a workflow may write.

  ```ts
  const { commit } = await run.deliver("item-12", `${run.env.runDir}/note.md`);
  ```

- `review(item, verdict)` — `fleet review <item> --json` with `--land` for
  `"accepted"` or `--return <file>` for `{ returned: file }`; the result's
  `state` is the one the verdict moved the item to.

  ```ts
  await run.review("item-12", "accepted");
  await run.review("item-13", { returned: `${run.env.runDir}/findings.md` });
  ```

- `land(item, sha)` — `fleet land <item> <sha> --json`; the result carries the
  landed sha, read by the verb from the push's own range line.

  ```ts
  const { sha } = await run.land("item-12", "0123abc");
  ```

- `gate(question, options)` — a question for a person on the run's own record
  item: the note `gates/<k>.md` in the question grammar (`QUESTION <text>`,
  then the lettered options one per line), `fleet ask --item <run> --note
  <file> --json`, then Waiting with the gate id as the condition. Once
  `gate.resolved` for that gate is on the stream the re-run closes the step
  with the answer's letter.

  ```ts
  const letter = await run.gate("Ship the report?", ["A. yes", "B. hold"]);
  ```

- `until(items, state)` — reads `item.<state>` events off the stream and
  throws Waiting whose condition is exactly the outstanding items, in the
  order given; once every item has one it returns each item's event payload.
  The states are the item kinds' last words: `dispatched`, `held`,
  `delivered`, `reviewed`, `returned`, `landed`, `parked`.

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
  await run.gate("Ship the report?", ["A. yes", "B. hold"]);
});
```

Deno is installed per box by its own installer into `~/.deno/bin`, and the
toolchain export every seat evals appends that directory to PATH; the pin
here is the version that installer wrote on the box this pack was measured on.
