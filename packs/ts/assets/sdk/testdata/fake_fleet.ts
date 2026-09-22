// The fake fleet binary the verb arms run against: `event …` goes to the real
// binary, so the steps a verb records and replays travel the writer and the
// reader a run uses; every other verb logs its argv to `calls.jsonl` under the
// fake's directory, appends the stream lines its canned file declares, prints
// the canned stdout and exits the canned code.
//
// argv: <fake dir> <real binary> <fleet args…>; the canned file is
// `<fake dir>/<verb>.json` — `{ stdout, code, append?: [{ type, payload }],
// cwd? }` — and an appended line's actor is the `--by` the call carried. A
// canned `cwd` is the directory the verb must have been started from: the fake
// exits 71 naming both when its own is another, before the canned answer.

import { append } from "./stream.ts";

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

await Deno.writeTextFile(`${dir}/calls.jsonl`, `${JSON.stringify(args)}\n`, {
  append: true,
});

interface Canned {
  stdout: string;
  code: number;
  append?: { type: string; payload: Record<string, unknown> }[];
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
const stream = `${Deno.env.get("FLEET_DIR")}/events.jsonl`;
for (const line of canned.append ?? []) {
  await append(stream, line.type, by, line.payload);
}
if (canned.stdout !== "") {
  await Deno.stdout.write(new TextEncoder().encode(canned.stdout));
}
Deno.exit(canned.code);
