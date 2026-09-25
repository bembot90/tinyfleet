// The six verbs against a fake fleet binary on FLEET_BIN — a script that
// answers every verb from a canned envelope, answers `item show` from the
// records an arm plants, and hands `event …` to the real binary — so each arm
// reads the argv the verb spawned, the step it recorded with the envelope's
// data as its result, and the exit the wrapper takes.
//
// The records `until`, `spawn` and `hold` read are planted by the arm itself,
// entry by entry in the shape `fleet item show --json` prints (`rig.ts`'s
// `plant` and `enter`), or by a canned verb the way the verb writes its entry.
// The stream lines `start` reads — a child run's start and its end — are
// appended in the stored shape, the way `rig.ts` seeds the opening line.

import { assert, assertEquals, assertMatch } from "jsr:@std/assert@1";
import {
  type Dispatched,
  HOLDS_DIR,
  KINDS_OF,
  replay,
  RETAKEN,
  type Reviewed,
  type Run,
  type Seat,
  Waiting,
} from "./mod.ts";
import {
  closes,
  enter,
  fakeOf,
  lines,
  plant,
  type Scratch,
  scratch as bare,
  starts,
} from "./testdata/rig.ts";
import {
  type Body,
  cleared,
  delivered,
  held,
  landed,
  reviewed,
} from "./testdata/store.ts";
import { append, typed } from "./testdata/stream.ts";

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

/** The person who clears a hold. */
const A_PERSON = "seat:a-person";

/** The scratch rig with the fake binary in front of the real one. */
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
    plant?: Record<string, Body[]>;
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
  const data: Dispatched = { item: "it-1", state: "ordered", seat: SPAWNED };
  const seat: Seat = data.seat;
  assertEquals(seat.kind, "agent");
  assertEquals(seat.name, undefined, "a spawned seat carries no name");
  // @ts-expect-error — a seat is `{id, name?, kind}` and never a bare name.
  const named: Dispatched = { item: "it-1", state: "ordered", seat: "tr-1" };
  assertEquals(typeof named.seat, "string");
  await plant(s, "it-1");
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

Deno.test("spawn — an item whose standing delivery no landing followed is carried: the step closes on RETAKEN<commit> with no dispatch, and a delivery after a return is the one carried", async () => {
  const fn = (run: Run) => run.spawn({ role: "builder", item: "it-1" });
  const s = await scratch();
  await enter(s, "it-1", delivered("aaa1111"));
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(await calls(s), [], "a carried delivery calls no dispatch");
  assertEquals(
    closes(await lines(s))[0].payload.result,
    `${RETAKEN}aaa1111`,
  );

  const t = await scratch();
  await enter(t, "it-1", delivered("aaa1111"));
  await enter(t, "it-1", reviewed("returned", "aaa1111"));
  await enter(t, "it-1", delivered("bbb2222"));
  assertEquals(await replay(fn, t.env, "{}"), { code: 0 });
  assertEquals(await calls(t), []);
  assertEquals(
    closes(await lines(t))[0].payload.result,
    `${RETAKEN}bbb2222`,
    "the delivery the return did not judge",
  );
});

Deno.test("spawn — a delivery a verdict returned, or a landing closed, is not carried: an item delivered then returned is dispatched, and so is one delivered then landed", async () => {
  const data: Dispatched = { item: "it-1", state: "ordered", seat: SPAWNED };
  for (
    const after of [
      reviewed("returned", "aaa1111"),
      landed("fedcba9", "aaa1111"),
    ]
  ) {
    const s = await scratch();
    await can(s, "dispatch", { stdout: envelope("dispatch", data) });
    await enter(s, "it-1", delivered("aaa1111"));
    await enter(s, "it-1", after);
    let got: unknown;
    const outcome = await replay(
      async (run) => {
        got = await run.spawn({ role: "builder", item: "it-1" });
      },
      s.env,
      "{}",
    );
    assertEquals(outcome, { code: 0 });
    assertEquals(got, data, `a delivery then ${after.kind} is dispatched`);
    assertEquals(
      await calls(s),
      [["dispatch", "it-1", "--by", `run:${s.env.runId}`, "--json"]],
      `a delivery then ${after.kind} is no RETAKEN`,
    );
  }
});

Deno.test("until delivered — fleet-45m over the store: delivered then returned waits on the item's delivered entries from the start seq, delivered again answers that entry, and a landing does not withdraw it", async () => {
  const s = await scratch();
  // A start position of its own, so the wake's `since` is read off this
  // execution's FLEET_STREAM_SEQ and not off anything the stream holds.
  const env = { ...s.env, streamSeq: 5 };
  let got: Record<string, unknown> | undefined;
  const fn = async (run: Run) => {
    got = await run.until(["it-1"], "delivered");
  };
  await enter(s, "it-1", delivered("aaa1111"));
  await enter(s, "it-1", reviewed("returned", "aaa1111"));
  assertEquals(
    await replay(fn, env, "{}"),
    {
      code: 2,
      waiting: { items: ["it-1"], kinds: ["delivered"], since: 5 },
    },
    "a returned delivery is still outstanding",
  );
  const again = await enter(s, "it-1", delivered("bbb2222"));
  assertEquals(await replay(fn, env, "{}"), { code: 0 });
  assertEquals(got, { "it-1": again }, "the answer is the entry, whole");
  assertEquals((got!["it-1"] as { commit: string }).commit, "bbb2222");

  const t = await scratch();
  const first = await enter(t, "it-1", delivered("aaa1111"));
  await enter(t, "it-1", landed("fedcba9", "aaa1111"));
  assertEquals(
    await replay(fn, t.env, "{}"),
    { code: 0 },
    "a landed item answers its delivery rather than waiting for good",
  );
  assertEquals(got, { "it-1": first });
  assertEquals(await calls(t), [], "until reads the store and writes nothing");
});

Deno.test("until held — an item with no open hold waits on its held entries, and answers the open hold once one is on its record; a cleared hold is no open hold", async () => {
  const s = await scratch();
  await plant(s, "it-1");
  let got: Record<string, unknown> | undefined;
  const fn = async (run: Run) => {
    got = await run.until(["it-1"], "held");
  };
  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: { items: ["it-1"], kinds: ["held"], since: 1 },
  });
  const hold = await enter(s, "it-1", held("hold-1", "ask"));
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(got, { "it-1": hold });

  const t = await scratch();
  await enter(t, "it-1", held("hold-1", "ask"));
  await enter(t, "it-1", cleared("hold-1", "A"));
  assertEquals(await replay(fn, t.env, "{}"), {
    code: 2,
    waiting: { items: ["it-1"], kinds: ["held"], since: 1 },
  });
});

/** Core's entry kinds as `core/src/entry.rs` spells them, the kinds a wake may
 * name as `controller/src/runs.rs` spells them, and the kinds core's verbs
 * signal, off each `signal(…, "<kind>")` call in `core/src/item`: the SDK
 * reaches neither crate but through the binary, so its suite reads their
 * source. */
async function rustTables(): Promise<{
  kinds: string[];
  wakeable: string[];
  signalled: Set<string>;
}> {
  const workspace = `${here}/../../../..`;
  const listed = (text: string, name: string, file: string): string[] => {
    const declared = new RegExp(
      `pub const ${name}: \\[&str; \\d+\\] = \\[([^\\]]*)\\];`,
    ).exec(text);
    assert(declared !== null, `${file} declares ${name}`);
    return [...declared[1].matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
  };
  const entry = await Deno.readTextFile(`${workspace}/core/src/entry.rs`);
  const kinds = listed(entry, "KINDS", "core/src/entry.rs");
  const runs = await Deno.readTextFile(`${workspace}/controller/src/runs.rs`);
  const wakeable = listed(runs, "ENTRY_KINDS", "controller/src/runs.rs");
  const signalled = new Set<string>();
  for (
    const verb of ["dispatch", "deliver", "review", "hold", "run", "land"]
  ) {
    const source = await Deno.readTextFile(
      `${workspace}/core/src/item/${verb}.rs`,
    );
    for (const call of source.matchAll(/signal\([^;]*?"([a-z_]+)",?\s*\)/g)) {
      signalled.add(call[1]);
    }
  }
  return { kinds, wakeable, signalled };
}

Deno.test("KINDS_OF — every state waits on entry kinds core defines, the controller's wake reads and a verb signals, and so does a hold's clearance", async () => {
  const { kinds, wakeable, signalled } = await rustTables();
  assertEquals(kinds.length, 7, `core's seven entry kinds: ${kinds}`);
  assertEquals(wakeable, kinds, "the controller reads core's entry kinds");
  assertEquals(
    [...signalled].sort(),
    ["cleared", "delivered", "held", "landed", "ordered", "reviewed"],
    "the six kinds the verbs signal: a withdrawn order is signalled by none",
  );
  const states = Object.keys(KINDS_OF);
  assertEquals(states.length, 6, `every state has a row: ${states}`);
  for (
    const [state, named] of [...Object.entries(KINDS_OF), ["(hold)", [
      "cleared",
    ]]] as [string, string[]][]
  ) {
    assert(named.length > 0, `${state} names no kind`);
    for (const kind of named) {
      assert(kinds.includes(kind), `${state} waits on ${kind}, no entry kind`);
      assert(
        signalled.has(kind),
        `${state} waits on ${kind}, and no verb signals it`,
      );
    }
  }
});

Deno.test("a read the store refuses — item show on an item with no record — is a thrown Refusal: exit 1 with the refusal's code in the reason, the step started and not closed", async () => {
  const s = await scratch();
  const outcome = await replay(
    (run) => run.until(["no-such-item"], "delivered"),
    s.env,
    "{}",
  );
  assertEquals(outcome, {
    code: 1,
    reason: {
      verb: "item show",
      code: "refused",
      why: "no-such-item: no issues found matching the provided IDs",
    },
  });
  const all = await lines(s);
  assertEquals(starts(all).map((l) => l.payload.n), [1]);
  assertEquals(closes(all), []);
  assertEquals(await calls(s), []);
});

Deno.test("AC1 review — accepted is --land and { returned } is --return <file>, each the reviewed entry it wrote and its verdict", async () => {
  const s = await scratch();
  const accepted: Reviewed = {
    item: "it-1",
    state: "reviewed",
    verdict: "accepted",
  };
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
  const returned: Reviewed = {
    item: "it-2",
    state: "reviewed",
    verdict: "returned",
  };
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

/** A hold on the run's record, canned: the verb answers `hold`, and the held
 * entry it writes lands on the run's record by the run, the way `fleet hold`
 * writes it. */
async function asks(s: Faked, hold: string): Promise<void> {
  const runId = s.env.runId;
  await can(s, "hold", {
    stdout: envelope("hold", { item: runId, state: "held", hold }),
    plant: { [runId]: [held(hold, "ask")] },
  });
}

/** The Waiting a hold on the run's record throws: the record, its clearance,
 * and the position the execution started from. */
function clearanceOf(s: Faked): unknown {
  return { items: [s.env.runId], kinds: ["cleared"], since: s.env.streamSeq };
}

Deno.test("AC2 hold — the question file as JSON, fleet hold --question on the run's record item, exit 2 waiting on the record's clearance, one hold across the re-runs, then the letter", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await plant(s, runId);
  await asks(s, "hold-7");
  const letters: string[] = [];
  const fn = async (run: Run) => {
    await run.step("count", () => 1);
    letters.push(await run.hold("Ship the report?", ["A. yes", "B. hold"]));
  };

  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: clearanceOf(s),
  });
  const file = `${s.env.runDir}/${HOLDS_DIR}/1.json`;
  assertEquals(
    await Deno.readTextFile(file),
    '{"question":"Ship the report?","options":[{"letter":"A","text":"yes"},{"letter":"B","text":"hold"}]}',
    "the question file is JSON of the shape assets/question.schema.json",
  );
  assertEquals(await calls(s), [[
    "hold",
    "--item",
    runId,
    "--question",
    file,
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

  // Nobody has cleared: the re-run finds its ask on the record, waits on the
  // same clearance and asks nothing.
  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: clearanceOf(s),
  });
  assertEquals((await calls(s)).length, 1, "the re-run does not ask twice");

  await enter(s, runId, cleared("hold-7", "B"), A_PERSON);
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

Deno.test("hold — the k-th hold is the k-th ask this run wrote on its record: a second hold asks once and never again, and neither the controller's max_crashes hold nor another run's ask on the same record is one of this run's", async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await plant(s, runId);
  const fn = async (run: Run) => {
    await run.hold("First?", ["A. yes", "B. no"]);
    await run.hold("Second?", ["A. yes", "B. no"]);
  };

  await asks(s, "hold-1");
  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: clearanceOf(s),
  });
  await enter(s, runId, cleared("hold-1", "A"), A_PERSON);
  // Two holds on the same record, neither of them this run's ask, between its
  // first and its second.
  await enter(s, runId, held("crash-1", "max_crashes"), "controller:a-machine");
  await enter(s, runId, held("other-1", "ask"), "run:fleet-run-another");
  await asks(s, "hold-2");

  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: clearanceOf(s),
  });
  assertEquals(
    (await calls(s)).map((argv) => argv[0]),
    ["hold", "hold"],
    "the second hold asked: no other hold on the record is this run's second ask",
  );
  assertEquals(await replay(fn, s.env, "{}"), {
    code: 2,
    waiting: clearanceOf(s),
  });
  assertEquals((await calls(s)).length, 2, "and the re-run found it");

  await enter(s, runId, cleared("hold-2", "B"), A_PERSON);
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals((await calls(s)).length, 2);
  assertEquals(
    closes(await lines(s)).map((l) => [l.payload.name, l.payload.result]),
    [["hold First?", "A"], ["hold Second?", "B"]],
    "each hold closes on its own clearance's letter",
  );
});

Deno.test('hold — a cancel\'s clearance answers "cancelled" and not the string "null": the cleared entry says how it was cleared, where the stream\'s line only names it', async () => {
  const s = await scratch();
  const runId = s.env.runId;
  await plant(s, runId);
  await asks(s, "hold-9");
  // What `fleet cancel` leaves behind: the clearance on the record, and the
  // stream's signal for it, which names the entry and carries no letter.
  const clearance = await enter(s, runId, cleared("hold-9"), A_PERSON);
  await append(s.env.stream, "item.entry", typed(A_PERSON), {
    item: runId,
    entry: clearance.id,
    kind: "cleared",
  });
  let got: string | undefined;
  assertEquals(
    await replay(
      async (run) => {
        got = await run.hold("Ship the report?", ["A. yes", "B. hold"]);
      },
      s.env,
      "{}",
    ),
    { code: 0 },
  );
  assertEquals(got, "cancelled");
  assertEquals(closes(await lines(s))[0].payload.result, "cancelled");
});

Deno.test("AC2 hold — the about licence rides in the question file, and an option with no letter throws before the binary is asked", async () => {
  const s = await scratch();
  await plant(s, s.env.runId);
  await asks(s, "hold-8");
  const about = {
    items: ["it-1"],
    commit: "0123456789abcdef0123456789abcdef01234567",
    licenses: "A",
  };
  assertEquals(
    await replay(
      (run) => run.hold("Land it-1?", ["A. land it", "B. not yet"], about),
      s.env,
      "{}",
    ),
    { code: 2, waiting: clearanceOf(s) },
  );
  assertEquals(
    JSON.parse(await Deno.readTextFile(`${s.env.runDir}/${HOLDS_DIR}/1.json`)),
    {
      question: "Land it-1?",
      options: [
        { letter: "A", text: "land it" },
        { letter: "B", text: "not yet" },
      ],
      about,
    },
  );

  const t = await scratch();
  await plant(t, t.env.runId);
  await asks(t, "x");
  const unlettered = await replay(
    (run) => run.hold("Ship the report?", ["yes", "B. hold"]),
    t.env,
    "{}",
  );
  assertEquals(unlettered.code, 1);
  assertMatch(
    String((unlettered as { reason: unknown }).reason),
    /hold: the option yes is not <letter>\. <text>/,
  );
  assertEquals(await calls(t), [], "the binary was never asked");
});

Deno.test("AC3 until — Waiting names exactly the outstanding items, in the order given, with the state's entry kinds, and closes once the last item's record answers", async () => {
  const s = await scratch();
  const items = ["it-a", "it-b", "it-c"];
  for (const item of items) await plant(s, item);
  let got: Record<string, unknown> | undefined;
  const fn = async (run: Run) => {
    got = await run.until(items, "landed");
  };
  const waiting = (outstanding: string[]) => ({
    code: 2 as const,
    waiting: { items: outstanding, kinds: ["landed"], since: 1 },
  });

  assertEquals(await replay(fn, s.env, "{}"), waiting(items));
  await enter(s, "it-b", landed("b0b", "b1b"));
  await enter(s, "it-a", delivered("a0a"));
  assertEquals(
    await replay(fn, s.env, "{}"),
    waiting(["it-a", "it-c"]),
    "a delivery is not a landing",
  );
  await enter(s, "it-c", landed("c0c", "c1c"));
  assertEquals(await replay(fn, s.env, "{}"), waiting(["it-a"]));
  await enter(s, "it-a", landed("a0a", "a1a"));
  assertEquals(await replay(fn, s.env, "{}"), { code: 0 });
  assertEquals(Object.keys(got!), items);
  assertEquals((got!["it-b"] as { sha: string }).sha, "b0b");
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.name), [
    "until landed it-a it-b it-c",
  ]);
  assertEquals(recorded[0].payload.result, got);
  assertEquals(await calls(s), [], "until reads the store and writes nothing");

  const t = await scratch();
  const unknown = await replay(
    (run) => run.until(items, "shipped" as "landed"),
    t.env,
    "{}",
  );
  assertEquals(unknown.code, 1);
  assertMatch(
    String((unknown as { reason: unknown }).reason),
    /until: no item state is named shipped; the states are dispatched, delivered, reviewed, returned, landed, held/,
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
  const data: Dispatched = { item: "it-1", state: "ordered", seat: SPAWNED };
  await plant(s, "it-1");
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
  await plant(t, "it-1");
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
