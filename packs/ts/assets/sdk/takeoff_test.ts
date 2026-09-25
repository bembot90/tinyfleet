// tiny's takeoff workflow against the fake fleet binary `verbs_test.ts` runs
// the verbs on: each arm reads the steps the flight records, in order, the
// argv the fake saw per verb, and the exit a re-run takes — so a Waiting at an
// item's delivery spawns nothing twice, a hold's letter is the verdict, and a
// `review=accept` flight lands only on the one licence it asks for first.
//
// The records the flight reads are planted in the fake's store, the way the
// verbs' suite plants them: an item's delivery by the canned dispatch, as the
// builder it rang would write it, or by the arm itself for an item delivered
// before the flight; a hold by the canned `hold`, on the run's own record; a
// clearance by the arm, as a person's `fleet clear` writes it.
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
  LICENCE,
  NOT_TESTED,
  policyOf,
  REPORT,
  takeoff,
  TICK,
} from "../../../tiny/workflows/takeoff.ts";
import { replay, RETAKEN } from "./mod.ts";
import {
  closes,
  enter,
  fakeOf,
  lines,
  plant,
  type Scratch,
  scratch as bare,
} from "./testdata/rig.ts";
import { type Body, cleared, delivered, held } from "./testdata/store.ts";

const here = import.meta.dirname!;

/** The question file a review=hold flight writes for it-1 at aaa1111: the
 * JSON `fleet hold --question` reads, each of the flight's OPTIONS taken
 * apart into its letter and its text, and about it-1 at that commit, which A
 * licenses the run to land. */
const ACCEPT_IT_1 = JSON.stringify({
  question: "Accept it-1 at aaa1111?",
  options: [
    { letter: "A", text: "accept and land" },
    { letter: "B", text: "return to the builder" },
  ],
  about: { items: ["it-1"], commit: "aaa1111", licenses: "A" },
});

/** The question a review=accept flight of it-1 and it-2 asks before anything
 * else: one hold about both, naming no commit, whose A licenses each landing. */
const LICENSE_BOTH = "Land what this flight's review accepts? it-1, it-2";

/** The step that hold records, by the name the SDK gives a hold's step. */
const LICENCE_STEP = `hold ${LICENSE_BOTH}`;

interface Faked extends Scratch {
  fake: string;
}

/** What the canned dispatch answers. */
const DISPATCHED = {
  item: "x",
  state: "ordered",
  seat: {
    id: "01a0d1f1-0aec-765f-9abe-00007e3fa2c0",
    kind: "agent",
  },
};

/** The person who clears a hold. */
const A_PERSON = { kind: "seat", id: "a-person" };

/** The scratch rig with the fake binary in front of the real one, the three
 * verbs a landing flight calls canned to answer, and a record in the store for
 * the run and each item the arms fly, with nothing on it yet. */
async function scratch(): Promise<Faked> {
  const s = await bare();
  const fake = fakeOf(s);
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
  for (const item of [t.env.runId, "it-1", "it-2"]) await plant(t, item);
  await can(t, "dispatch", DISPATCHED);
  await can(t, "review", { item: "x", state: "reviewed", verdict: "accepted" });
  await can(t, "land", { item: "x", state: "landed", sha: "fedcba9" });
  return t;
}

async function can(
  s: Faked,
  verb: string,
  data: unknown,
  plant?: Record<string, Body[]>,
): Promise<void> {
  await Deno.writeTextFile(
    `${s.fake}/${verb}.json`,
    JSON.stringify({
      code: 0,
      stdout: `${JSON.stringify({ ok: true, verb, data })}\n`,
      plant,
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

/** A hold canned under the id given: the verb answers it, and the held entry
 * it writes lands on the run's own record by the run. */
async function holds(s: Faked, hold: string): Promise<void> {
  const runId = s.env.runId;
  await can(s, "hold", { item: runId, state: "held", hold }, {
    [runId]: [held(hold, "ask")],
  });
}

/** A person's clearance of a hold on the run's record, with a letter, or a
 * cancel where none is given. */
async function clears(s: Faked, hold: string, letter?: string): Promise<void> {
  await enter(s, s.env.runId, cleared(hold, letter), A_PERSON);
}

/** The Waiting a hold on the run's record throws. */
function onClearance(s: Faked): unknown {
  return { items: [s.env.runId], kinds: ["cleared"], since: s.env.streamSeq };
}

/** The flight's licence hold canned as `licence-1` and, with a letter, cleared
 * by a person with it. */
async function licensed(s: Faked, letter?: string): Promise<void> {
  await holds(s, "licence-1");
  if (letter !== undefined) await clears(s, "licence-1", letter);
}

/** The dispatch canned to deliver each item named at its commit: the builder
 * the dispatch rang writes its delivery on the item's record. An item not
 * named is dispatched and never delivered. */
async function builds(
  s: Faked,
  commits: Record<string, string>,
): Promise<void> {
  const plant: Record<string, Body[]> = {};
  for (const [item, commit] of Object.entries(commits)) {
    plant[item] = [delivered(commit)];
  }
  await can(s, "dispatch", DISPATCHED, plant);
}

/** An item's delivery written on its record by the arm: one delivered before
 * the flight began, or by a builder after the flight stopped waiting. */
async function deliver(s: Faked, item: string, commit: string): Promise<void> {
  await enter(s, item, delivered(commit), { kind: "seat", id: "a-builder" });
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
 * the four inputs, the flight's licence, then per item spawn, until, review
 * and land with the second spawn fed by the first landing at width 1, then the
 * report and the tick. */
const FIFTEEN = [
  ...INPUTS,
  LICENCE_STEP,
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

Deno.test("AC1 takeoff — two items through the licence, then spawn, until, review and land: fifteen steps in order, the report and the tick in the run directory, and a replay that spawns nothing", async () => {
  const s = await scratch();
  await licensed(s, "A");
  await builds(s, { "it-1": "aaa1111", "it-2": "bbb2222" });
  const stdin = pinned("it-1,it-2", "review=accept");

  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.n), FIFTEEN.map((_, i) => i + 1));
  assertEquals(recorded.map((l) => l.payload.name), FIFTEEN);
  assertEquals(recorded.length, 15, "the flight's own steps, counted");

  const all = await calls(s);
  assertEquals(all[0][0], "hold", "the flight's licence is asked first");
  const seen = all.slice(1);
  assertEquals(verbs(seen), [
    "dispatch",
    "review",
    "land",
    "dispatch",
    "review",
    "land",
  ]);
  assertEquals(seen[0], [
    "dispatch",
    "it-1",
    "--by",
    `run:${s.env.runId}`,
    "--json",
  ]);
  assertEquals(seen[1], [
    "review",
    "it-1",
    "--land",
    "--by",
    `run:${s.env.runId}`,
    "--json",
  ]);
  assertEquals(
    seen[2],
    ["land", "it-1", "aaa1111", "--by", `run:${s.env.runId}`, "--json"],
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
  assertEquals(recorded[13].payload.result, {
    path: `${s.env.runDir}/${REPORT}`,
    landed: 2,
    returned: 0,
  });
  assertEquals(recorded[14].payload.result, {
    path: `${s.env.runDir}/${TICK}`,
    ticked: ["it-1", "it-2"],
  });

  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  assertEquals(closes(await lines(s)).length, 15, "a replay records nothing");
  assertEquals((await calls(s)).length, 7, "and spawns nothing");
});

Deno.test("AC2 re-run — Waiting at the second item's until: exit 2 naming it, the re-run spawns nothing (the fake's spawn counter stays at 2), and the flight closes once the delivery lands", async () => {
  const s = await scratch();
  await licensed(s, "A");
  await builds(s, { "it-1": "aaa1111" });
  const stdin = pinned('["it-1", "it-2"]', "review=accept");
  const onIt2 = {
    code: 2 as const,
    waiting: { items: ["it-2"], kinds: ["delivered"], since: s.env.streamSeq },
  };

  assertEquals(await replay(takeoff, s.env, stdin), onIt2);
  const spawns = (seen: string[][]) =>
    verbs(seen).filter((v) => v === "dispatch").length;
  assertEquals(
    spawns(await calls(s)),
    2,
    "both items are spawned before the wait",
  );
  assertEquals(
    closes(await lines(s)).map((l) => l.payload.name),
    FIFTEEN.slice(0, 10),
  );

  assertEquals(await replay(takeoff, s.env, stdin), onIt2);
  assertEquals(spawns(await calls(s)), 2, "the re-run spawns nothing");
  assertEquals(
    (await calls(s)).length,
    5,
    "nor asks, reviews or lands again",
  );
  assertEquals(closes(await lines(s)).length, 10, "and records no step");

  await deliver(s, "it-2", "bbb2222");
  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  assertEquals(spawns(await calls(s)), 2);
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), FIFTEEN);
});

/** The question file the licence hold writes: LICENCE taken apart, and about
 * both items with no commit. */
const LICENSE_BOTH_FILE = JSON.stringify({
  question: LICENSE_BOTH,
  options: [
    { letter: "A", text: "land each accepted delivery" },
    { letter: "B", text: "land nothing — end the flight" },
  ],
  about: { items: ["it-1", "it-2"], licenses: "A" },
});

Deno.test("AC6 review=accept — one hold before any review, about every item and naming no commit: the flight waits on it, A reviews and lands each item", async () => {
  const s = await scratch();
  await licensed(s);
  await builds(s, { "it-1": "aaa1111", "it-2": "bbb2222" });
  const stdin = pinned("it-1,it-2", "review=accept");

  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: onClearance(s),
  });
  assertEquals(
    await Deno.readTextFile(`${s.env.runDir}/holds/1.json`),
    LICENSE_BOTH_FILE,
  );
  assertEquals(
    LICENCE.map((o) => o.slice(0, 1)),
    ["A", "B"],
    "the licence's own letters",
  );
  assertEquals(
    verbs(await calls(s)),
    ["hold"],
    "the licence is asked before anything is spawned or reviewed",
  );
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), INPUTS);

  await clears(s, "licence-1", "A");
  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  assertEquals(verbs(await calls(s)), [
    "hold",
    "dispatch",
    "review",
    "land",
    "dispatch",
    "review",
    "land",
  ], "one ask, then each item reviewed and landed");
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), FIFTEEN);
});

Deno.test("AC6 review=accept — B fails the run with the declined message, and a cancel of the licence fails it as cancelled; either way nothing is spawned, reviewed or landed", async () => {
  for (
    const [letter, reason] of [
      [
        "B",
        "takeoff: the person declined to license this flight's landings (answered B)",
      ],
      [undefined, "takeoff: the flight's licence hold was cancelled"],
    ]
  ) {
    const s = await scratch();
    await licensed(s);
    await clears(s, "licence-1", letter);

    assertEquals(
      await replay(takeoff, s.env, pinned("it-1,it-2", "review=accept")),
      { code: 1, reason },
    );
    assertEquals(verbs(await calls(s)), ["hold"], "only the licence was asked");
    assertEquals(closes(await lines(s)).map((l) => l.payload.name), [
      ...INPUTS,
      LICENCE_STEP,
    ]);
  }
});

Deno.test("AC1 hold — under review=hold every verdict is a hold: the flight waits on the clearance, holds once across the re-runs, lands on A and returns on B with the findings file", async () => {
  const s = await scratch();
  await builds(s, { "it-1": "aaa1111" });
  const runId = s.env.runId;
  await holds(s, "hold-1");
  const stdin = pinned("it-1");

  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: onClearance(s),
  });
  assertEquals(
    await Deno.readTextFile(`${s.env.runDir}/holds/1.json`),
    ACCEPT_IT_1,
  );
  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: onClearance(s),
  });
  assertEquals(
    verbs(await calls(s)),
    ["dispatch", "hold"],
    "one ask across the re-runs",
  );

  await clears(s, "hold-1", "B");
  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  const findings = `${s.env.runDir}/${FINDINGS_DIR}/it-1.json`;
  assertEquals(await calls(s), [
    ["dispatch", "it-1", "--by", `run:${runId}`, "--json"],
    [
      "hold",
      "--item",
      runId,
      "--question",
      `${s.env.runDir}/holds/1.json`,
      "--by",
      `run:${runId}`,
      "--json",
    ],
    ["review", "it-1", "--return", findings, "--by", `run:${runId}`, "--json"],
  ], "B is a return, and nothing lands");
  // The findings file is the JSON `fleet review --return` reads: one finding,
  // naming the letter. The SDK's fake binary never reads it, so the bytes are
  // also the fixture the binary's own suite feeds the real verb.
  const written = await Deno.readTextFile(findings);
  const { findings: found } = JSON.parse(written);
  assertEquals(found.length, 1, written);
  assertMatch(found[0].text, /^The person answered B at the run's hold: B\. /);
  assertEquals(
    written,
    await Deno.readTextFile(`${here}/testdata/takeoff_findings_b.json`),
    "the file cli/tests/deliver.rs feeds `fleet review --return`",
  );
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), [
    ...INPUTS,
    "spawn it-1",
    "until delivered it-1",
    "hold Accept it-1 at aaa1111?",
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

  // The same hold cleared A: the review is --land and the item lands.
  const t = await scratch();
  await builds(t, { "it-1": "aaa1111" });
  await holds(t, "hold-2");
  assertEquals(await replay(takeoff, t.env, stdin), {
    code: 2,
    waiting: onClearance(t),
  });
  await clears(t, "hold-2", "A");
  assertEquals(await replay(takeoff, t.env, stdin), { code: 0 });
  assertEquals(verbs(await calls(t)), ["dispatch", "hold", "review", "land"]);
  assertEquals((await calls(t))[2][2], "--land");
});

Deno.test("AC1 an item taken back — its record carries a delivery no return and no landing followed: the spawn step closes on `already delivered at <commit>` and calls no dispatch, and the item holds and lands on that commit", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  // Delivered before the flight began, which is the item a run that failed
  // after a delivery left behind: it carries an order already, so a dispatch
  // for it is refused. Every other arm here is delivered by the dispatch it
  // makes, which is the same read's other answer.
  await deliver(s, "it-1", "aaa1111");
  await holds(s, "hold-1");
  const stdin = pinned("it-1");

  assertEquals(await replay(takeoff, s.env, stdin), {
    code: 2,
    waiting: onClearance(s),
  });
  assertEquals(
    verbs(await calls(s)),
    ["hold"],
    "the delivered item reaches the hold without a dispatch",
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
    await Deno.readTextFile(`${s.env.runDir}/holds/1.json`),
    ACCEPT_IT_1,
    "the hold carries the delivery's own commit",
  );

  await clears(s, "hold-1", "A");
  assertEquals(await replay(takeoff, s.env, stdin), { code: 0 });
  assertEquals(verbs(await calls(s)), ["hold", "review", "land"]);
  assertEquals(
    (await calls(s))[2],
    ["land", "it-1", "aaa1111", "--by", `run:${runId}`, "--json"],
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
  await licensed(s, "A");
  await builds(s, { "it-1": "aaa1111" });
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

Deno.test("AC1 the bundle — takeoff bundled by the pack's bundle line and run by its run line reaches its first hold: the note carries the question and both options, and the wrapper exits 2 waiting on its clearance", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await builds(s, { "it-1": "aaa1111" });
  await holds(s, "hold-1");

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
    [2, JSON.stringify(onClearance(s))],
    `the bundled workflow ended ${ran.code} on \`${last}\`\n${stderr}`,
  );
  assertEquals(
    await Deno.readTextFile(`${s.env.runDir}/holds/1.json`),
    ACCEPT_IT_1,
    "the note the person reads, written by the bundle and not by an import",
  );
  assertEquals(
    verbs(await calls(s)),
    ["dispatch", "hold"],
    "the flight spawned the item and raised its first hold, and stopped there",
  );
});

Deno.test("the inputs — items as a JSON array or a separated list, the policy's two keys and their defaults, and the refusals", () => {
  assertEquals(itemsOf("it-1,it-2"), ["it-1", "it-2"]);
  assertEquals(itemsOf(" it-1 it-2 "), ["it-1", "it-2"]);
  assertEquals(itemsOf('["it-1", "it-2"]'), ["it-1", "it-2"]);
  assertEquals(policyOf(null), { review: "hold", width: 1 });
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
    assertMatch(thrown, /is not one of review=hold, review=accept, width=<n>/);
  }
});
