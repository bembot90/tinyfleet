// tiny's takeoff workflow against the fake fleet binary `verbs_test.ts` runs
// the verbs on: each arm reads the steps the flight records, in order, the
// argv the fake saw per verb, and the exit a re-run takes — so a Waiting at an
// item's delivery spawns nothing twice, and a gate's letter is the verdict.
//
// The deliveries the flight waits on are appended by the arm in the stored
// shape, the way the verbs' suite appends a park or a landing.
//
// The last arm is the only one that runs the workflow the way a run does —
// through the pack's own bundle and run lines, as a process — so a module
// whose entry line sits above a const the workflow reads is caught here and
// nowhere else: an import initialises every const before the workflow runs.

import { assert, assertEquals, assertMatch } from "jsr:@std/assert@1";
import {
  commandOf,
  FINDINGS_DIR,
  itemsOf,
  NOT_TESTED,
  OPTIONS,
  policyOf,
  REPORT,
  takeoff,
  TICK,
} from "../../../tiny/workflows/takeoff.ts";
import { replay, RETAKEN } from "./mod.ts";
import {
  closes,
  lines,
  type Scratch,
  scratch as bare,
} from "./testdata/rig.ts";
import { append } from "./testdata/stream.ts";

const here = import.meta.dirname!;

interface Faked extends Scratch {
  fake: string;
}

/** The scratch rig with the fake binary in front of the real one, and the
 * three verbs a landing flight calls canned to answer. */
async function scratch(): Promise<Faked> {
  const s = await bare();
  const fake = `${s.root}/fake`;
  await Deno.mkdir(fake);
  const script = `${s.root}/fake-fleet`;
  await Deno.writeTextFile(
    script,
    `#!/bin/sh\nexec ${JSON.stringify(Deno.execPath())} run --allow-all ${
      JSON.stringify(`${here}/testdata/fake_fleet.ts`)
    } ${JSON.stringify(fake)} ${JSON.stringify(s.env.bin)} "$@"\n`,
  );
  await Deno.chmod(script, 0o755);
  const t = { ...s, fake, env: { ...s.env, bin: script } };
  await can(t, "dispatch", { item: "x", state: "dispatched", seat: "tr-1" });
  await can(t, "review", { item: "x", state: "reviewed" });
  await can(t, "land", { item: "x", state: "landed", sha: "fedcba9" });
  return t;
}

async function can(
  s: Faked,
  verb: string,
  data: unknown,
  append?: { type: string; payload: Record<string, unknown> }[],
): Promise<void> {
  await Deno.writeTextFile(
    `${s.fake}/${verb}.json`,
    JSON.stringify({
      code: 0,
      stdout: `${JSON.stringify({ ok: true, verb, data })}\n`,
      append,
    }),
  );
}

/** Every argv the fake answered, in call order. */
async function calls(s: Faked): Promise<string[][]> {
  try {
    const body = await Deno.readTextFile(`${s.fake}/calls.jsonl`);
    return body.split("\n").filter((l) => l !== "").map((l) => JSON.parse(l));
  } catch {
    return [];
  }
}

function verbs(seen: string[][]): string[] {
  return seen.map((argv) => argv[0]);
}

async function delivered(
  s: Faked,
  item: string,
  commit: string,
): Promise<number> {
  return await append(s.env.stream, "item.delivered", "a-builder", {
    item,
    commit,
    branch: `w/${item}`,
    base: "0",
  });
}

function pinned(
  items: string,
  policy?: string,
  more: { inputs?: Record<string, string>; config?: Record<string, string> } =
    {},
): string {
  return JSON.stringify({
    inputs: {
      items,
      ...(policy === undefined ? {} : { policy }),
      ...(more.inputs ?? {}),
    },
    ...(more.config === undefined ? {} : { config: more.config }),
  });
}

/** The four inputs every flight reads first, in order. */
const INPUTS = ["input items", "input policy", "input test", "input touched"];

/** The step names a two-item flight records under `review=accept`, in order:
 * the four inputs, then per item spawn, until, review and land with the second
 * spawn fed by the first landing at width 1, then the report and the tick. */
const FOURTEEN = [
  ...INPUTS,
  "spawn it-1",
  "until delivered it-1",
  "review it-1",
  "land it-1",
  "spawn it-2",
  "until delivered it-2",
  "review it-2",
  "land it-2",
  "report",
  "tick",
];

Deno.test("AC1 takeoff — two items through spawn, until, review and land: fourteen steps in order, the report and the tick in the run directory, and a replay that spawns nothing", async () => {
  const s = await scratch();
  await delivered(s, "it-1", "aaa1111");
  await delivered(s, "it-2", "bbb2222");
  const stdin = pinned("it-1,it-2", "review=accept");

  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.n), FOURTEEN.map((_, i) => i + 1));
  assertEquals(recorded.map((l) => l.payload.name), FOURTEEN);
  assertEquals(recorded.length, 14, "the flight's own steps, counted");

  const seen = await calls(s);
  assertEquals(verbs(seen), [
    "dispatch",
    "review",
    "land",
    "dispatch",
    "review",
    "land",
  ]);
  assertEquals(seen[0], ["dispatch", "it-1", "--by", s.env.runId, "--json"]);
  assertEquals(seen[1], [
    "review",
    "it-1",
    "--land",
    "--by",
    s.env.runId,
    "--json",
  ]);
  assertEquals(
    seen[2],
    ["land", "it-1", "aaa1111", "--by", s.env.runId, "--json"],
    "the landed sha is the delivery's commit",
  );
  assertEquals(seen[5][2], "bbb2222");

  const report = await Deno.readTextFile(`${s.env.runDir}/${REPORT}`);
  assertMatch(report, new RegExp(`^# Flight ${s.env.runId}$`, "m"));
  assertMatch(report, /\| it-1 \| landed \| fedcba9 \|/);
  assertMatch(report, /\| it-2 \| landed \| fedcba9 \|/);
  assertMatch(report, /items 2, landed 2, returned 0, decisions 0/);
  const tick = await Deno.readTextFile(`${s.env.runDir}/${TICK}`);
  assertMatch(tick, /^- ☑ it-1 fedcba9$/m);
  assertMatch(tick, /^- ☑ it-2 fedcba9$/m);
  assertEquals(recorded[12].payload.result, {
    path: `${s.env.runDir}/${REPORT}`,
    landed: 2,
    returned: 0,
  });
  assertEquals(recorded[13].payload.result, {
    path: `${s.env.runDir}/${TICK}`,
    ticked: ["it-1", "it-2"],
  });

  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  assertEquals(closes(await lines(s)).length, 14, "a replay records nothing");
  assertEquals((await calls(s)).length, 6, "and spawns nothing");
});

Deno.test("AC2 re-run — Waiting at the second item's until: exit 2 naming it, the re-run spawns nothing (the fake's spawn counter stays at 2), and the flight closes once the delivery lands", async () => {
  const s = await scratch();
  await delivered(s, "it-1", "aaa1111");
  const stdin = pinned('["it-1", "it-2"]', "review=accept");

  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: ["it-2"],
  });
  const spawns = (seen: string[][]) =>
    verbs(seen).filter((v) => v === "dispatch").length;
  assertEquals(
    spawns(await calls(s)),
    2,
    "both items are spawned before the wait",
  );
  assertEquals(
    closes(await lines(s)).map((l) => l.payload.name),
    FOURTEEN.slice(0, 9),
  );

  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: ["it-2"],
  });
  assertEquals(spawns(await calls(s)), 2, "the re-run spawns nothing");
  assertEquals((await calls(s)).length, 4, "nor reviews or lands again");
  assertEquals(closes(await lines(s)).length, 9, "and records no step");

  await delivered(s, "it-2", "bbb2222");
  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  assertEquals(spawns(await calls(s)), 2);
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), FOURTEEN);
});

Deno.test("AC1 gate — under review=gate every verdict is a gate: the flight waits on the gate id, asks once across the re-runs, lands on A and returns on B with the findings file", async () => {
  const s = await scratch();
  await delivered(s, "it-1", "aaa1111");
  const runId = s.env.runId;
  await can(s, "ask", { item: runId, state: "parked", gate: "gate-1" }, [{
    type: "item.parked",
    payload: {
      item: runId,
      reason: "ask",
      branch: "",
      commit: "",
      gate: "gate-1",
    },
  }]);
  const stdin = pinned("it-1");

  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: "gate-1",
  });
  assertEquals(
    await Deno.readTextFile(`${s.env.runDir}/gates/1.md`),
    `QUESTION Accept it-1 at aaa1111?\n${OPTIONS.join("\n")}\n`,
  );
  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: "gate-1",
  });
  assertEquals(
    verbs(await calls(s)),
    ["dispatch", "ask"],
    "one ask across the re-runs",
  );

  await append(s.env.stream, "gate.resolved", "a-person", {
    item: runId,
    gate: "gate-1",
    letter: "B",
  });
  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  const findings = `${s.env.runDir}/${FINDINGS_DIR}/it-1.md`;
  assertEquals(await calls(s), [
    ["dispatch", "it-1", "--by", runId, "--json"],
    [
      "ask",
      "--item",
      runId,
      "--note",
      `${s.env.runDir}/gates/1.md`,
      "--by",
      runId,
      "--json",
    ],
    ["review", "it-1", "--return", findings, "--by", runId, "--json"],
  ], "B is a return, and nothing lands");
  assertMatch(await Deno.readTextFile(findings), /^RETURNED it-1 at aaa1111$/m);
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), [
    ...INPUTS,
    "spawn it-1",
    "until delivered it-1",
    "gate Accept it-1 at aaa1111?",
    "review it-1",
    "report",
    "tick",
  ]);
  const report = await Deno.readTextFile(`${s.env.runDir}/${REPORT}`);
  assertMatch(
    report,
    new RegExp(
      `^- \`${runId}-D1\` it-1 — Accept it-1 at aaa1111\\? Answered B\\.$`,
      "m",
    ),
  );
  assertMatch(report, /\| it-1 \| returned \| — \|/);
  assertMatch(
    await Deno.readTextFile(`${s.env.runDir}/${TICK}`),
    /^# Board tick/,
  );
  assertEquals(
    (await Deno.readTextFile(`${s.env.runDir}/${TICK}`)).includes("☑"),
    false,
    "a returned item is not ticked",
  );

  // The same gate answered A: the review is --land and the item lands.
  const t = await scratch();
  await delivered(t, "it-1", "aaa1111");
  await can(t, "ask", { item: t.env.runId, state: "parked", gate: "gate-2" }, [{
    type: "item.parked",
    payload: {
      item: t.env.runId,
      reason: "ask",
      branch: "",
      commit: "",
      gate: "gate-2",
    },
  }]);
  assertEquals(await replay(takeoff, t.env, stdin), {
    code: 2,
    waiting: "gate-2",
  });
  await append(t.env.stream, "gate.resolved", "a-person", {
    item: t.env.runId,
    gate: "gate-2",
    letter: "A",
  });
  assertEquals(await replay(takeoff, t.env, stdin), { code: 0 });
  assertEquals(verbs(await calls(t)), ["dispatch", "ask", "review", "land"]);
  assertEquals((await calls(t))[2][2], "--land");
});

Deno.test("AC1 an item taken back — its delivery sits at or below the seq the run started from: the spawn step closes on `already delivered at <commit>` and calls no dispatch, and the item gates and lands on that commit", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  const seq = await delivered(s, "it-1", "aaa1111");
  // The run starts ABOVE the delivery, which is the item a run that failed
  // after a delivery left behind: it carries an order already, so a dispatch
  // for it is refused. Every other arm here delivers above its start seq and
  // dispatches, which is the same read's other answer.
  const env = { ...s.env, streamSeq: seq };
  await can(s, "ask", { item: runId, state: "parked", gate: "gate-1" }, [{
    type: "item.parked",
    payload: {
      item: runId,
      reason: "ask",
      branch: "",
      commit: "",
      gate: "gate-1",
    },
  }]);
  const stdin = pinned("it-1");

  assertEquals(await replay(takeoff, env, stdin), {
    code: 2,
    waiting: "gate-1",
  });
  assertEquals(
    verbs(await calls(s)),
    ["ask"],
    "the delivered item reaches the gate without a dispatch",
  );
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.name), [
    ...INPUTS,
    "spawn it-1",
    "until delivered it-1",
  ], "the steps a spawned item records, in the same places");
  assertEquals(
    recorded[4].payload.result,
    `${RETAKEN}aaa1111`,
    "the spawn step closes on the delivery it found",
  );
  assertEquals(
    await Deno.readTextFile(`${env.runDir}/gates/1.md`),
    `QUESTION Accept it-1 at aaa1111?\n${OPTIONS.join("\n")}\n`,
    "the gate carries the delivery's own commit",
  );

  await append(env.stream, "gate.resolved", "a-person", {
    item: runId,
    gate: "gate-1",
    letter: "A",
  });
  assertEquals(await replay(takeoff, env, stdin), { code: 0 });
  assertEquals(verbs(await calls(s)), ["ask", "review", "land"]);
  assertEquals(
    (await calls(s))[2],
    ["land", "it-1", "aaa1111", "--by", runId, "--json"],
    "the item lands on the commit it was delivered at, with no builder cut",
  );
});

/** A one-item flight under `review=accept`, run to its close over the pinned
 * document the arm hands it: the argv each verb was called with, and the
 * report the flight wrote. */
async function flown(
  more: { inputs?: Record<string, string>; config?: Record<string, string> },
): Promise<{ dispatch: string[]; land: string[]; report: string }> {
  const s = await scratch();
  await delivered(s, "it-1", "aaa1111");
  assertEquals(
    await replay(takeoff, s.env, pinned("it-1", "review=accept", more)),
    { code: 0 },
  );
  const seen = await calls(s);
  const find = (verb: string) => {
    const argv = seen.find((a) => a[0] === verb);
    if (argv === undefined) throw new Error(`no ${verb} among ${seen}`);
    return argv;
  };
  return {
    dispatch: find("dispatch"),
    land: find("land"),
    report: await Deno.readTextFile(`${s.env.runDir}/${REPORT}`),
  };
}

Deno.test("AC3 the test commands — `--input test=` wins over [packs.tiny] takeoff.test and reaches the landing as `--test`; takeoff.touched reaches the dispatch as `--touched`; the report says what was run", async () => {
  const both = await flown({
    inputs: { test: "make from-the-input" },
    config: {
      "takeoff.test": "make from-the-fleet",
      "takeoff.touched": "make touched-from-the-fleet",
    },
  });
  assertEquals(
    both.land.slice(0, 5),
    ["land", "it-1", "aaa1111", "--test", "make from-the-input"],
    "the input's command, never the fleet's, reaches the landing",
  );
  assertEquals(
    both.dispatch.slice(0, 4),
    ["dispatch", "it-1", "--touched", "make touched-from-the-fleet"],
    "the fleet's touched command reaches the builder's dispatch",
  );
  assert(
    both.report.startsWith("# Flight "),
    `a tested flight's report opens on its heading:\n${both.report}`,
  );
  assert(!both.report.includes("NOT TESTED"), both.report);
  assertMatch(both.report, /`make from-the-input`/);

  // The fleet's setting alone is the one used, and a touched input wins over
  // the fleet's touched the same way.
  const fleet = await flown({
    inputs: { touched: "make touched-from-the-input" },
    config: {
      "takeoff.test": "make from-the-fleet",
      "takeoff.touched": "make touched-from-the-fleet",
    },
  });
  assertEquals(fleet.land.slice(3, 5), ["--test", "make from-the-fleet"]);
  assertEquals(fleet.dispatch.slice(2, 4), [
    "--touched",
    "make touched-from-the-input",
  ]);
});

Deno.test("AC3 no test command — with neither an input nor [packs.tiny] takeoff.test the flight still flies and lands, the report's FIRST line says NOT TESTED, and no landing is handed `--test`, so each landing note says NOT TESTED too", async () => {
  const none = await flown({});
  assertEquals(
    none.land.includes("--test"),
    false,
    `the landing is handed no test, which is what writes NOT TESTED on its note: ${none.land}`,
  );
  assertEquals(
    none.dispatch.includes("--touched"),
    false,
    `nor the dispatch a touched command: ${none.dispatch}`,
  );
  const first = none.report.split("\n")[0];
  assertEquals(first, NOT_TESTED, "the report's first line");
  assert(first.startsWith("NOT TESTED"), first);
  assertMatch(none.report, /\| it-1 \| landed \| fedcba9 \|/);

  // A blank value names nothing, on either side.
  const blank = await flown({
    inputs: { test: "  " },
    config: { "takeoff.test": "" },
  });
  assertEquals(blank.land.includes("--test"), false, `${blank.land}`);
  assertEquals(blank.report.split("\n")[0], NOT_TESTED);
});

Deno.test("commandOf — the input over the setting, a blank value names nothing, and neither is undefined", () => {
  assertEquals(commandOf("make a", "make b"), "make a");
  assertEquals(commandOf(null, "make b"), "make b");
  assertEquals(commandOf(" ", "make b"), "make b");
  assertEquals(commandOf(null, undefined), undefined);
  assertEquals(commandOf("", ""), undefined);
});

/** One [runtime] line of the ts pack's manifest, as the manifest stores it:
 * the read `run_line_test.ts` makes of the run line, by key. */
async function runtimeLine(key: string): Promise<string> {
  const manifest = `${here}/../../pack.toml`;
  const text = await Deno.readTextFile(manifest);
  const runtime = text.slice(text.indexOf("[runtime]"));
  const m = runtime.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"\\s*$`, "m"));
  if (!m) throw new Error(`${manifest} holds no [runtime] ${key} line`);
  return m[1];
}

/** The PATH a run's child gets: this process's own, with the directory the
 * pinned runtime was resolved from appended, never prepended. */
function childPath(): string {
  const exe = Deno.execPath();
  return `${Deno.env.get("PATH") ?? ""}:${exe.slice(0, exe.lastIndexOf("/"))}`;
}

/** What core copies from its own environment into a run's child, less the
 * one that names a claude tree, which no arm here runs. */
function passedThrough(): Record<string, string> {
  const env: Record<string, string> = {};
  for (const name of ["HOME", "USER", "TMPDIR", "LANG"]) {
    const value = Deno.env.get(name);
    if (value !== undefined) env[name] = value;
  }
  return env;
}

Deno.test("AC1 the bundle — takeoff bundled by the pack's bundle line and run by its run line reaches its first gate: the note carries the question and both options, and the wrapper exits 2 waiting on the gate", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await delivered(s, "it-1", "aaa1111");
  await can(s, "ask", { item: runId, state: "parked", gate: "gate-1" }, [{
    type: "item.parked",
    payload: {
      item: runId,
      reason: "ask",
      branch: "",
      commit: "",
      gate: "gate-1",
    },
  }]);

  const path = childPath();
  const bundle = `${s.root}/bundle.js`;
  const bundleLine = (await runtimeLine("bundle"))
    .replaceAll("{bundle}", bundle)
    .replaceAll("{entry}", `${here}/../../../tiny/workflows/takeoff.ts`);
  assert(
    !bundleLine.includes("{"),
    `a placeholder is left standing in: ${bundleLine}`,
  );
  const bundled = await new Deno.Command("sh", {
    args: ["-c", bundleLine],
    cwd: s.root,
    env: { PATH: path },
    stdout: "piped",
    stderr: "piped",
  }).output();
  assertEquals(
    bundled.code,
    0,
    `\`${bundleLine}\` exited ${bundled.code}:\n${
      new TextDecoder().decode(bundled.stderr)
    }`,
  );

  const runLine = (await runtimeLine("run"))
    .replaceAll("{fleet}", s.env.bin)
    .replaceAll("{run_dir}", s.env.runDir)
    .replaceAll("{bundle}", bundle);
  assert(
    !runLine.includes("{"),
    `a placeholder is left standing in: ${runLine}`,
  );
  const child = new Deno.Command("sh", {
    args: ["-c", runLine],
    cwd: s.env.runDir,
    clearEnv: true,
    env: {
      ...passedThrough(),
      PATH: path,
      FLEET_DIR: s.env.dir ?? "",
      FLEET_RUN_ID: runId,
      FLEET_STREAM: s.env.stream,
      FLEET_STREAM_SEQ: `${s.env.streamSeq}`,
      FLEET_RUN_DIR: s.env.runDir,
      FLEET_BIN: s.env.bin,
      FLEET_PROJECT: s.env.project,
    },
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  }).spawn();
  const writer = child.stdin.getWriter();
  await writer.write(new TextEncoder().encode(pinned("it-1")));
  await writer.close();
  const ran = await child.output();
  const stdout = new TextDecoder().decode(ran.stdout);
  const stderr = new TextDecoder().decode(ran.stderr);
  const last = stdout.trimEnd().split("\n").pop() ?? "";

  assertEquals(
    [ran.code, last],
    [2, JSON.stringify("gate-1")],
    `the bundled workflow ended ${ran.code} on \`${last}\`\n${stderr}`,
  );
  assertEquals(
    await Deno.readTextFile(`${s.env.runDir}/gates/1.md`),
    `QUESTION Accept it-1 at aaa1111?\n${OPTIONS.join("\n")}\n`,
    "the note the person reads, written by the bundle and not by an import",
  );
  assertEquals(
    verbs(await calls(s)),
    ["dispatch", "ask"],
    "the flight spawned the item and asked its first gate, and stopped there",
  );
});

Deno.test("the inputs — items as a JSON array or a separated list, the policy's two keys and their defaults, and the refusals", () => {
  assertEquals(itemsOf("it-1,it-2"), ["it-1", "it-2"]);
  assertEquals(itemsOf(" it-1 it-2 "), ["it-1", "it-2"]);
  assertEquals(itemsOf('["it-1", "it-2"]'), ["it-1", "it-2"]);
  assertEquals(policyOf(null), { review: "gate", width: 1 });
  assertEquals(policyOf("review=accept,width=3"), {
    review: "accept",
    width: 3,
  });
  for (
    const [bad, why] of [
      [null, /no `items` input/],
      [",", /names no item/],
      ["it-1,it-1", /listed twice/],
    ] as [unknown, RegExp][]
  ) {
    let thrown = "";
    try {
      itemsOf(bad);
    } catch (e) {
      thrown = String((e as Error).message);
    }
    assertMatch(thrown, why);
  }
  for (const bad of ["review=maybe", "width=0", "depth=2"]) {
    let thrown = "";
    try {
      policyOf(bad);
    } catch (e) {
      thrown = String((e as Error).message);
    }
    assertMatch(thrown, /is not one of review=gate, review=accept, width=<n>/);
  }
});
