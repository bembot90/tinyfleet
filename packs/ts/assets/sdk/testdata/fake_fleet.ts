// The fake fleet binary the verb arms run against: `event …` goes to the real
// binary, so the steps a verb records and replays travel the writer and the
// reader a run uses; `item show <id> --json` answers the record planted at
// `<fake dir>/store/<id>.json` (`store.ts`), and refuses an id with none the
// way the real verb refuses an unknown item, exit 1; every other verb logs its
// argv to `calls.jsonl` under the fake's directory, appends the stream lines
// and the entries its canned file declares, prints the canned stdout and exits
// the canned code.
//
// argv: <fake dir> <real binary> <fleet args…>; the canned file is
// `<fake dir>/<verb>.json` — `{ stdout, code, append?: [{ type, payload }],
// plant?: { <item>: [{ kind, …fields }] }, cwd? }` — and an appended line's
// actor is the `--by` the call carried, typed as the stream stores it, and a
// planted entry's author is the same actor as its one string, `<kind>:<id>`,
// as `fleet item show --json` prints it. `plant` is keyed by the item the call names —
// `--item`'s value, else the verb's first argument — so one canned dispatch
// can deliver each item it is called for, and a call for an item it does not
// name plants nothing. A canned `cwd` is the directory the verb must have been
// started from: the fake exits 71 naming both when its own is another, before
// the canned answer.

import { append, typed } from "./stream.ts";
import { type Body, enter, storeOf } from "./store.ts";

const [dir, real, ...args] = Deno.args;

if (args[0] === "event") {
  const ran = await new Deno.Command(real, {
    args,
    stdin: "null",
    stdout: "inherit",
    stderr: "inherit",
  }).output();
  Deno.exit(ran.code);
}

// A READ AND NOT AN ACT, so it is not logged: `calls.jsonl` is what the
// workflow asked the store to do, and an arm counting dispatches or holds
// counts those alone.
if (args[0] === "item" && args[1] === "show") {
  const id = args[2];
  let data: unknown;
  try {
    data = JSON.parse(await Deno.readTextFile(`${storeOf(dir)}/${id}.json`));
  } catch {
    const why = `${id}: no issues found matching the provided IDs`;
    console.error(`fleet item show: ${why}`);
    console.log(JSON.stringify({
      ok: false,
      verb: "item show",
      refusal: { code: "refused", why },
    }));
    Deno.exit(1);
  }
  console.log(JSON.stringify({ ok: true, verb: "item show", data }));
  Deno.exit(0);
}

await Deno.writeTextFile(`${dir}/calls.jsonl`, `${JSON.stringify(args)}\n`, {
  append: true,
});

interface Canned {
  stdout: string;
  code: number;
  append?: { type: string; payload: Record<string, unknown> }[];
  plant?: Record<string, Body[]>;
  cwd?: string;
}

let canned: Canned;
try {
  canned = JSON.parse(await Deno.readTextFile(`${dir}/${args[0]}.json`));
} catch (e) {
  console.error(`fake fleet: no canned ${args[0]}: ${e}`);
  Deno.exit(70);
}

if (canned.cwd !== undefined) {
  const [expected, actual] = await Promise.all([
    Deno.realPath(canned.cwd),
    Deno.realPath(Deno.cwd()),
  ]);
  if (expected !== actual) {
    console.error(
      `fake fleet: ${
        args[0]
      } ran from ${actual}, not the project root ${expected}`,
    );
    Deno.exit(71);
  }
}

const by = args[args.indexOf("--by") + 1] ?? "nobody";
const actor = typed(by);
const stream = `${Deno.env.get("FLEET_DIR")}/events.jsonl`;
for (const line of canned.append ?? []) {
  await append(stream, line.type, actor, line.payload);
}
const named = args.includes("--item")
  ? args[args.indexOf("--item") + 1]
  : args[1];
for (const body of canned.plant?.[named] ?? []) {
  await enter(dir, named, body, `${actor.kind}:${actor.id}`);
}
if (canned.stdout !== "") {
  await Deno.stdout.write(new TextEncoder().encode(canned.stdout));
}
Deno.exit(canned.code);
