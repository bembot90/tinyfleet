// The seven verbs against a fake fleet binary on FLEET_BIN — a script that
// answers every verb from a canned envelope and hands `event …` to the real
// binary — so each arm reads the argv the verb spawned, the step it recorded
// with the envelope's data as its result, and the exit the wrapper takes.
//
// The stream lines a verb's re-run reads — a park, a cleared hold, an item's
// landing, a child run's close — are appended by the arm itself in the stored
// shape, the way `rig.ts` seeds the opening line.

import { assert, assertEquals, assertMatch } from "jsr:@std/assert@1";
import {
  type Dispatched,
  HOLDS_DIR,
  replay,
  RETAKEN,
  type Run,
  type Seat,
  Waiting,
} from "./mod.ts";
import {
  closes,
  lines,
  type Scratch,
  scratch as bare,
  starts,
} from "./testdata/rig.ts";
import { append } from "./testdata/stream.ts";

const here = import.meta.dirname!;

interface Faked extends Scratch {
  fake: string;
}

/** The seat a dispatch's document names: a spawned seat is an agent with no
 * name, so its object carries none. */
const SPAWNED: Seat = {
  id: "01a0d1f1-0aec-765f-9abe-00007e3fa2c0",
  kind: "agent",
};

/** The scratch rig with the fake binary in front of the real one. */
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
  return { ...s, fake, env: { ...s.env, bin: script } };
}

function envelope(verb: string, data: unknown): string {
  return `${JSON.stringify({ ok: true, verb, data })}\n`;
}

function refusal(verb: string, code: string, why: string): string {
  return `${JSON.stringify({ ok: false, verb, refusal: { code, why } })}\n`;
}

async function can(
  s: Faked,
  verb: string,
  canned: {
    stdout: string;
    code?: number;
    append?: { type: string; payload: Record<string, unknown> }[];
    cwd?: string;
  },
): Promise<void> {
  await Deno.writeTextFile(
    `${s.fake}/${verb}.json`,
    JSON.stringify({ code: 0, ...canned }),
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

/** One verb's arm: the workflow run twice, the argv the fake saw, the step's
 * recorded result equal to the envelope's data, and no second call on the
 * replay. */
async function oneStep<T>(
  s: Faked,
  name: string,
  data: T,
  call: (run: Run) => Promise<T>,
): Promise<string[][]> {
  const got: T[] = [];
  const fn = async (run: Run) => {
    got.push(await call(run));
  };
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(got, [data], "the verb returns the envelope's data");
  const recorded = closes(await lines(s));
  assertEquals(recorded.length, 1);
  assertEquals(recorded[0].payload.n, 1);
  assertEquals(recorded[0].payload.name, name);
  assertEquals(
    recorded[0].payload.result,
    data,
    "the step's result is the data",
  );
  const seen = await calls(s);
  assertEquals(seen.length, 1, "one spawn of the binary for the verb");

  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(got, [data, data], "the replay returns the recorded data");
  assertEquals((await calls(s)).length, 1, "the replay spawns nothing");
  return seen;
}

Deno.test("AC1 spawn — fleet dispatch <item> --by <run> --json, the seat in the step's result; a reviewer or a model is refused", async () => {
  const s = await scratch();
  // The seat is its object, typed: a name where the object belongs is a type
  // error, which the directive below asserts at check time.
  const data: Dispatched = { item: "it-1", state: "dispatched", seat: SPAWNED };
  const seat: Seat = data.seat;
  assertEquals(seat.kind, "agent");
  assertEquals(seat.name, undefined, "a spawned seat carries no name");
  // @ts-expect-error — a seat is `{id, name?, kind}` and never a bare name.
  const named: Dispatched = { item: "it-1", state: "dispatched", seat: "tr-1" };
  assertEquals(typeof named.seat, "string");
  await can(s, "dispatch", { stdout: envelope("dispatch", data) });
  const seen = await oneStep(
    s,
    "spawn it-1",
    data,
    (run) => run.spawn({ role: "builder", item: "it-1" }),
  );
  assertEquals(seen, [[
    "dispatch",
    "it-1",
    "--by",
    `run:${s.env.runId}`,
    "--json",
  ]]);

  const t = await scratch();
  await can(t, "dispatch", { stdout: envelope("dispatch", data) });
  const modelled = await replay(
    (run) => run.spawn({ role: "builder", item: "it-1", model: "a-model" }),
    t.env,
    "{}",
  );
  assertEquals(modelled.code, 1);
  assertMatch(
    String((modelled as { reason: unknown }).reason),
    /pins no model/,
  );
  const reviewer = await replay(
    (run) => run.spawn({ role: "reviewer" as "builder", item: "it-1" }),
    t.env,
    "{}",
  );
  assertEquals(reviewer.code, 1);
  assertMatch(
    String((reviewer as { reason: unknown }).reason),
    /no verb cuts a reviewer/,
  );
  assertEquals(await calls(t), [], "neither refusal spawned the binary");
});

/** One line of an item's record on the stream, in the stored shape the
 * binary's own verb writes it. */
function recorded(
  s: Faked,
  kind: "delivered" | "returned" | "landed",
  item: string,
  commit: string,
): Promise<number> {
  const payload = kind === "delivered"
    ? { item, commit, branch: `w/${item}`, base: "0" }
    : kind === "returned"
    ? { item, commit, findings: "f.md" }
    : { item, sha: commit, base: "0", squash_of: commit };
  return append(
    s.env.stream,
    `item.${kind}`,
    { kind: "seat", id: "a-seat" },
    payload,
  );
}

Deno.test("spawn — a delivery a verdict returned, or a landing closed, is not carried: an item delivered then returned below the start seq is dispatched, and so is one delivered then landed", async () => {
  const data: Dispatched = { item: "it-1", state: "dispatched", seat: SPAWNED };
  for (const after of ["returned", "landed"] as const) {
    const s = await scratch();
    await can(s, "dispatch", { stdout: envelope("dispatch", data) });
    await recorded(s, "delivered", "it-1", "aaa1111");
    const seq = await recorded(s, after, "it-1", "aaa1111");
    let got: unknown;
    const outcome = await replay(
      async (run) => {
        got = await run.spawn({ role: "builder", item: "it-1" });
      },
      { ...s.env, streamSeq: seq },
      "{}",
    );
    assertEquals(outcome, { code: 0 });
    assertEquals(got, data, `a delivery then ${after} is dispatched`);
    assertEquals(
      await calls(s),
      [["dispatch", "it-1", "--by", `run:${s.env.runId}`, "--json"]],
      `a delivery then ${after} is no RETAKEN`,
    );
  }
});

Deno.test("spawn — the fence: a delivery nobody judged at or below the start seq closes on RETAKEN, and so does one whose return sits above that seq", async () => {
  const s = await scratch();
  const seq = await recorded(s, "delivered", "it-1", "aaa1111");
  const env = { ...s.env, streamSeq: seq };
  const fn = (run: Run) => run.spawn({ role: "builder", item: "it-1" });
  assertEquals(await replay(fn, env, "{}"), { code: 0 });
  assertEquals(await calls(s), [], "a carried delivery calls no dispatch");
  assertEquals(
    closes(await lines(s))[0].payload.result,
    `${RETAKEN}aaa1111`,
  );

  const t = await scratch();
  const at = await recorded(t, "delivered", "it-1", "bbb2222");
  await recorded(t, "returned", "it-1", "bbb2222");
  assertEquals(await replay(fn, { ...t.env, streamSeq: at }, "{}"), {
    code: 0,
  });
  assertEquals(await calls(t), [], "a return above the start seq is unread");
  assertEquals(
    closes(await lines(t))[0].payload.result,
    `${RETAKEN}bbb2222`,
  );
});

Deno.test("until delivered — each item's latest delivery that no return follows: delivered then returned waits, delivered again answers the second commit, and a landing does not withdraw it", async () => {
  const s = await scratch();
  let got: Record<string, unknown> | undefined;
  const fn = async (run: Run) => {
    got = await run.until(["it-1"], "delivered");
  };
  await recorded(s, "delivered", "it-1", "aaa1111");
  await recorded(s, "returned", "it-1", "aaa1111");
  assertEquals(
    await replay(fn, s.env, "{}"),
    { code: 2, waiting: ["it-1"] },
    "a returned delivery is still outstanding",
  );
  await recorded(s, "delivered", "it-1", "bbb2222");
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals((got!["it-1"] as { commit: string }).commit, "bbb2222");

  const t = await scratch();
  await recorded(t, "delivered", "it-1", "aaa1111");
  await recorded(t, "landed", "it-1", "aaa1111");
  assertEquals(
    await replay(fn, t.env, "{}"),
    { code: 0 },
    "a landed item answers its delivery rather than waiting for good",
  );
  assertEquals((got!["it-1"] as { commit: string }).commit, "aaa1111");
});

Deno.test("AC1 deliver — fleet deliver --item <item> --delivery <file> --by <run> --json, the commit in the step's result", async () => {
  const s = await scratch();
  const data = { item: "it-1", state: "delivered", commit: "0123abc" };
  await can(s, "deliver", { stdout: envelope("deliver", data) });
  const delivery = `${s.env.runDir}/delivery.json`;
  const seen = await oneStep(
    s,
    "deliver it-1",
    data,
    (run) => run.deliver("it-1", delivery),
  );
  assertEquals(seen, [[
    "deliver",
    "--item",
    "it-1",
    "--delivery",
    delivery,
    "--by",
    `run:${s.env.runId}`,
    "--json",
  ]]);
});

Deno.test("AC1 review — accepted is --land and { returned } is --return <file>, each the state its verdict moved the item to", async () => {
  const s = await scratch();
  const accepted = { item: "it-1", state: "reviewed" };
  await can(s, "review", { stdout: envelope("review", accepted) });
  await oneStep(
    s,
    "review it-1",
    accepted,
    (run) => run.review("it-1", "accepted"),
  );
  assertEquals((await calls(s))[0], [
    "review",
    "it-1",
    "--land",
    "--by",
    `run:${s.env.runId}`,
    "--json",
  ]);

  const t = await scratch();
  const returned = { item: "it-2", state: "returned" };
  await can(t, "review", { stdout: envelope("review", returned) });
  const findings = `${t.env.runDir}/findings.md`;
  await oneStep(
    t,
    "review it-2",
    returned,
    (run) => run.review("it-2", { returned: findings }),
  );
  assertEquals((await calls(t))[0], [
    "review",
    "it-2",
    "--return",
    findings,
    "--by",
    `run:${t.env.runId}`,
    "--json",
  ]);
});

Deno.test("AC1 land — fleet land <item> <sha> --by <run> --json, the landed sha in the step's result", async () => {
  const s = await scratch();
  const data = { item: "it-1", state: "landed", sha: "fedcba9" };
  await can(s, "land", { stdout: envelope("land", data) });
  const seen = await oneStep(
    s,
    "land it-1",
    data,
    (run) => run.land("it-1", "abc1234"),
  );
  assertEquals(seen, [[
    "land",
    "it-1",
    "abc1234",
    "--by",
    `run:${s.env.runId}`,
    "--json",
  ]]);
});

Deno.test("AC2 hold — the question note, fleet hold on the run's record item, exit 2 with the hold id, one hold across the re-runs, then the letter", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await can(s, "hold", {
    stdout: envelope("hold", { item: runId, state: "held", hold: "hold-7" }),
    append: [{
      type: "item.held",
      payload: {
        item: runId,
        reason: "ask",
        branch: "",
        commit: "",
        hold: "hold-7",
      },
    }],
  });
  const letters: string[] = [];
  const fn = async (run: Run) => {
    await run.step("count", () => 1);
    letters.push(await run.hold("Ship the report?", ["A. yes", "B. not yet"]));
  };

  assertEquals(await replay(fn, s.env, "{}"), { code: 2, waiting: "hold-7" });
  const note = `${s.env.runDir}/${HOLDS_DIR}/1.md`;
  assertEquals(
    await Deno.readTextFile(note),
    "QUESTION Ship the report?\nA. yes\nB. not yet\n",
    "the note is in the question grammar",
  );
  assertEquals(await calls(s), [[
    "hold",
    "--item",
    runId,
    "--note",
    note,
    "--by",
    `run:${runId}`,
    "--json",
  ]]);
  let all = await lines(s);
  assertEquals(
    closes(all).map((l) => l.payload.n),
    [1],
    "the hold is started and not closed",
  );
  assertEquals(starts(all).map((l) => l.payload.n), [1, 2]);

  // The stream has not moved past the park: the re-run waits on the same hold
  // and asks nothing — the park on the stream is the record it reads.
  assertEquals(await replay(fn, s.env, "{}"), { code: 2, waiting: "hold-7" });
  assertEquals((await calls(s)).length, 1, "the re-run does not ask twice");

  await append(s.env.stream, "hold.cleared", { kind: "seat", id: "a-person" }, {
    item: runId,
    hold: "hold-7",
    letter: "B",
  });
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(letters, ["B"]);
  assertEquals((await calls(s)).length, 1);
  all = await lines(s);
  const hold = closes(all)[1];
  assertEquals(hold.payload.name, "hold Ship the report?");
  assertEquals(hold.payload.result, "B", "the step closes with the letter");

  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(letters, ["B", "B"], "and replays it");
  assertEquals(closes(await lines(s)).length, 2);
});

Deno.test("AC3 until — Waiting names exactly the outstanding items, in the order given, and closes when the last event lands", async () => {
  const s = await scratch();
  const items = ["it-a", "it-b", "it-c"];
  let got: Record<string, unknown> | undefined;
  const fn = async (run: Run) => {
    got = await run.until(items, "landed");
  };

  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: ["it-a", "it-b", "it-c"],
  });
  await append(s.env.stream, "item.landed", { kind: "seat", id: "kite" }, {
    item: "it-b",
    sha: "b0b",
    base: "0",
    squash_of: "x",
  });
  await append(s.env.stream, "item.delivered", { kind: "seat", id: "pell" }, {
    item: "it-a",
    commit: "a0a",
    branch: "w",
    base: "0",
  });
  assertEquals(
    await replay(fn, s.env, "{}"),
    { code: 2, waiting: ["it-a", "it-c"] },
    "a delivery is not a landing",
  );
  await append(s.env.stream, "item.landed", { kind: "seat", id: "kite" }, {
    item: "it-c",
    sha: "c0c",
  });
  assertEquals(await replay(fn, s.env, "{}"), { code: 2, waiting: ["it-a"] });
  await append(s.env.stream, "item.landed", { kind: "seat", id: "kite" }, {
    item: "it-a",
    sha: "a0a",
  });
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(Object.keys(got!), items);
  assertEquals((got!["it-b"] as { sha: string }).sha, "b0b");
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.name), [
    "until landed it-a it-b it-c",
  ]);
  assertEquals(recorded[0].payload.result, got);
  assertEquals(await calls(s), [], "until reads the stream and spawns no verb");

  const t = await scratch();
  const unknown = await replay(
    (run) => run.until(items, "shipped" as "landed"),
    t.env,
    "{}",
  );
  assertEquals(unknown.code, 1);
  assertMatch(
    String((unknown as { reason: unknown }).reason),
    /no item event is named item\.shipped/,
  );
});

Deno.test("AC1 start — fleet run <name> --by <run> --input k=v, Waiting on the child's run id, one run across the re-runs, then { run } on run.closed; a failed child is exit 1", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await can(s, "run", {
    stdout: "fleet-run-child — deadbeef\nfleet-run-child — waiting\n",
    append: [
      {
        type: "run.started",
        payload: {
          run: "fleet-run-child",
          hash: "deadbeef",
          workflow: "child",
        },
      },
      {
        type: "run.waiting",
        payload: { run: "fleet-run-child", wake: { until: "x" }, seq: 3 },
      },
    ],
  });
  let got: { run: string } | undefined;
  const fn = async (run: Run) => {
    got = await run.start("child", { city: "Lisbon", n: 2 });
  };

  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: "fleet-run-child",
  });
  assertEquals(await calls(s), [[
    "run",
    "child",
    "--by",
    `run:${runId}`,
    "--input",
    "city=Lisbon",
    "--input",
    "n=2",
  ]]);
  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: "fleet-run-child",
  });
  assertEquals(
    (await calls(s)).length,
    1,
    "the re-run does not start a second child",
  );

  await append(s.env.stream, "run.closed", {
    kind: "run",
    id: "fleet-run-child",
  }, {
    run: "fleet-run-child",
  });
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(got, { run: "fleet-run-child" });
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.name), ["start child"]);
  assertEquals(recorded[0].payload.result, { run: "fleet-run-child" });

  const t = await scratch();
  await can(t, "run", {
    stdout: "fleet-run-bad — deadbeef\nfleet-run-bad — failed\n",
    code: 1,
    append: [
      {
        type: "run.started",
        payload: { run: "fleet-run-bad", hash: "deadbeef", workflow: "child" },
      },
      {
        type: "run.failed",
        payload: { run: "fleet-run-bad", reason: "the child said no" },
      },
    ],
  });
  const failed = await replay((run) => run.start("child"), t.env, "{}");
  assertEquals(failed.code, 1);
  assertMatch(String((failed as { reason: unknown }).reason), /exited 1/);
  const again = await replay((run) => run.start("child"), t.env, "{}");
  assertEquals(again.code, 1, "the re-run reads run.failed and starts nothing");
  assertMatch(
    String((again as { reason: unknown }).reason),
    /run fleet-run-bad failed: "the child said no"/,
  );
  assertEquals((await calls(t)).length, 1);
});

Deno.test("start — a child a person cancelled fails the step naming it cancelled, and the re-run starts no second child", async () => {
  const s = await scratch();
  await can(s, "run", {
    stdout: "fleet-run-gone — deadbeef\nfleet-run-gone — waiting\n",
    append: [
      {
        type: "run.started",
        payload: { run: "fleet-run-gone", hash: "deadbeef", workflow: "child" },
      },
      {
        type: "run.waiting",
        payload: { run: "fleet-run-gone", wake: { until: "x" }, seq: 3 },
      },
    ],
  });
  const fn = (run: Run) => run.start("child");
  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: "fleet-run-gone",
  });

  await append(
    s.env.stream,
    "run.cancelled",
    { kind: "seat", id: "a-person" },
    {
      run: "fleet-run-gone",
    },
  );
  const cancelled = await replay(fn, s.env, "{}");
  assertEquals(cancelled.code, 1, "a cancelled child ends the parent's wait");
  assertMatch(
    String((cancelled as { reason: unknown }).reason),
    /run fleet-run-gone was cancelled by seat:a-person/,
  );
  assertEquals((await calls(s)).length, 1, "and no second child is started");
});

Deno.test("AC4 refusal — a refusal envelope on land is exit 1 with the refusal's code in the reason, the step started and not closed", async () => {
  const s = await scratch();
  await can(s, "land", {
    stdout: refusal("land", "refused", "the trunk moved under the landing"),
    code: 1,
  });
  const outcome = await replay(
    async (run) => {
      await run.step("count", () => 1);
      await run.land("it-1", "abc1234");
    },
    s.env,
    "{}",
  );
  assertEquals(outcome, {
    code: 1,
    reason: {
      verb: "land",
      code: "refused",
      why: "the trunk moved under the landing",
    },
  });
  const all = await lines(s);
  assertEquals(closes(all).map((l) => l.payload.n), [1]);
  assertEquals(starts(all).map((l) => l.payload.n), [1, 2]);

  // The same through the wrapper as a process: exit 1, the code on the last line.
  const t = await scratch();
  await can(t, "land", {
    stdout: refusal("land", "refused", "the trunk moved under the landing"),
    code: 1,
  });
  const child = await new Deno.Command(Deno.execPath(), {
    args: [
      "run",
      "--allow-read",
      "--allow-write",
      "--allow-run",
      "--allow-env",
      `${here}/testdata/lands.ts`,
    ],
    cwd: t.env.runDir,
    env: {
      FLEET_RUN_ID: t.env.runId,
      FLEET_STREAM: t.env.stream,
      FLEET_STREAM_SEQ: "1",
      FLEET_RUN_DIR: t.env.runDir,
      FLEET_BIN: t.env.bin,
      FLEET_PROJECT: t.project,
      FLEET_DIR: t.machine,
    },
    stdin: "null",
    stdout: "piped",
    stderr: "piped",
  }).output();
  const printed = new TextDecoder().decode(child.stdout).split("\n").filter((
    l,
  ) => l !== "");
  assertEquals(child.code, 1, new TextDecoder().decode(child.stderr));
  assertEquals(
    JSON.parse(printed[printed.length - 1]),
    {
      verb: "land",
      code: "refused",
      why: "the trunk moved under the landing",
    },
  );

  // A step whose exec throws Waiting from inside a verb is still the wrapper's
  // exit 2: the refusal path and the waiting path do not share a class.
  assert(!(new Waiting("x") instanceof Error));
});

/** The wrapper as a process from the RUN DIRECTORY, the way core starts a
 * run's child: `spawns.ts` under `deno run` with the six names and FLEET_DIR
 * in its environment, the fake binary in front. */
async function fromTheRunDirectory(
  s: Faked,
): Promise<{ code: number; last: string; stderr: string }> {
  const child = await new Deno.Command(Deno.execPath(), {
    args: [
      "run",
      "--allow-read",
      "--allow-write",
      "--allow-run",
      "--allow-env",
      `${here}/testdata/spawns.ts`,
    ],
    cwd: s.env.runDir,
    env: {
      FLEET_RUN_ID: s.env.runId,
      FLEET_STREAM: s.env.stream,
      FLEET_STREAM_SEQ: "1",
      FLEET_RUN_DIR: s.env.runDir,
      FLEET_BIN: s.env.bin,
      FLEET_PROJECT: s.project,
      FLEET_DIR: s.machine,
    },
    stdin: "null",
    stdout: "piped",
    stderr: "piped",
  }).output();
  const printed = new TextDecoder().decode(child.stdout).split("\n").filter((
    l,
  ) => l !== "");
  return {
    code: child.code,
    last: printed[printed.length - 1] ?? "",
    stderr: new TextDecoder().decode(child.stderr),
  };
}

Deno.test("a verb runs from the project root under a run-directory cwd: the fake binary, canned to refuse any other cwd, answers dispatch from FLEET_PROJECT while the wrapper's own cwd is the run directory", async () => {
  const s = await scratch();
  const data: Dispatched = { item: "it-1", state: "dispatched", seat: SPAWNED };
  await can(s, "dispatch", {
    stdout: envelope("dispatch", data),
    cwd: s.project,
  });
  const ran = await fromTheRunDirectory(s);
  assertEquals(ran.code, 0, `${ran.last}\n${ran.stderr}`);
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.name), ["spawn it-1"]);
  assertEquals(recorded[0].payload.result, data);
  assertEquals(await calls(s), [
    ["dispatch", "it-1", "--by", `run:${s.env.runId}`, "--json"],
  ]);

  // The control: the same fake canned to expect the run directory instead
  // refuses, which is the fake's cwd check observed firing — so the green
  // above is the verb's cwd and not a check that never ran.
  const t = await scratch();
  await can(t, "dispatch", {
    stdout: envelope("dispatch", data),
    cwd: t.env.runDir,
  });
  const refused = await fromTheRunDirectory(t);
  assertEquals(refused.code, 1, refused.stderr);
  assertMatch(
    JSON.parse(refused.last),
    /exited 71: fake fleet: dispatch ran from .*, not the project root .*/,
  );
  assertEquals(closes(await lines(t)).length, 0, "the step never closed");
});
