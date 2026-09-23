// The SDK's core against the shipped binary: every arm runs the real `fleet`
// over the scratch rig in `testdata/rig.ts`, so what a step records and what a
// re-run replays travel the same path they do under a run.

import { assert, assertEquals, assertMatch } from "jsr:@std/assert@1";
import {
  Divergence,
  replay,
  RESULT_CAP,
  type Run,
  STEPS_DIR,
  Waiting,
} from "./mod.ts";
import {
  closes,
  counters,
  lines,
  type Scratch,
  scratch,
  starts,
} from "./testdata/rig.ts";

const here = import.meta.dirname!;

Deno.test("AC1 — a three-step workflow run twice: the second run makes no exec call and closes", async () => {
  const s = await scratch();
  const first = counters();
  const three = (c: ReturnType<typeof counters>) => async (run: Run) => {
    const a = await run.step("fetch", c.exec("fetch", { rows: [1, 2] }));
    const b = await run.step("count", c.exec("count", a.rows.length));
    await run.step("report", c.exec("report", `${b} rows`));
  };

  assertEquals(await replay(three(first), s.env, "{}"), { code: 0 });
  assertEquals(first.calls, { fetch: 1, count: 1, report: 1 });
  const recorded = closes(await lines(s));
  assertEquals(recorded.map((l) => l.payload.n), [1, 2, 3]);
  assertEquals(recorded.map((l) => l.payload.name), [
    "fetch",
    "count",
    "report",
  ]);
  assertEquals(recorded[2].payload.result, "2 rows");
  assertEquals(starts(await lines(s)).length, 3);

  const second = counters();
  assertEquals(await replay(three(second), s.env, "{}"), { code: 0 });
  assertEquals(
    second.calls,
    {},
    "the fake exec's counter reads 0 on the second run",
  );
  assertEquals(
    closes(await lines(s)).length,
    3,
    "a replayed step writes nothing",
  );
  assertEquals(starts(await lines(s)).length, 3);
});

Deno.test("AC2 — step 2 renamed between runs: exit 1 with the divergence naming n, the expected and the found name", async () => {
  const s = await scratch();
  const named = (second: string) => async (run: Run) => {
    await run.step("fetch", () => 1);
    await run.step(second, () => 2);
    await run.step("report", () => 3);
  };
  assertEquals(await replay(named("count"), s.env, "{}"), { code: 0 });

  const diverged = await replay(named("tally"), s.env, "{}");
  assertEquals(diverged, {
    code: 1,
    reason: "replay diverged at step 2: expected count got tally",
  });
  assertEquals(
    closes(await lines(s)).length,
    3,
    "a diverged run records nothing",
  );

  // The same shape when a caller catches it: the error carries the three.
  const thrown = new Divergence(2, "count", "tally");
  assertEquals([thrown.n, thrown.expected, thrown.found], [
    2,
    "count",
    "tally",
  ]);
});

Deno.test("AC3 — Waiting thrown at step 2: exit 2 with the condition, then the re-run replays steps 1 and 2 and closes", async () => {
  const s = await scratch();
  let met = false;
  const condition = { until: "the gate is answered", seq: 1 };
  const waits = (c: ReturnType<typeof counters>) => async (run: Run) => {
    await run.step("fetch", c.exec("fetch", "rows"));
    await run.step("gate", () => {
      c.calls.gate = (c.calls.gate ?? 0) + 1;
      if (!met) throw new Waiting(condition);
      return "A";
    });
    await run.step("report", c.exec("report", "done"));
  };

  const first = counters();
  assertEquals(await replay(waits(first), s.env, "{}"), {
    code: 2,
    waiting: condition,
  });
  assertEquals(first.calls, { fetch: 1, gate: 1 });
  let all = await lines(s);
  assertEquals(
    closes(all).map((l) => l.payload.n),
    [1],
    "the waiting step is started and not closed",
  );
  assertEquals(starts(all).map((l) => l.payload.n), [1, 2]);

  met = true;
  const second = counters();
  assertEquals(await replay(waits(second), s.env, "{}"), { code: 0 });
  assertEquals(
    second.calls,
    { gate: 1, report: 1 },
    "step 1 replayed; the gate ran again and closed",
  );
  all = await lines(s);
  assertEquals(closes(all).map((l) => l.payload.n), [1, 2, 3]);
  assertEquals(closes(all)[1].payload.result, "A");
  assertEquals(
    starts(all).map((l) => l.payload.n),
    [1, 2, 2, 3],
    "the gate started twice, once per attempt",
  );

  const third = counters();
  assertEquals(await replay(waits(third), s.env, "{}"), { code: 0 });
  assertEquals(third.calls, {}, "steps 1 and 2 replayed");
});

Deno.test("AC4 — a 100 KiB result: the file in the run directory, the sha on the event, and the replay of it", async () => {
  const s = await scratch();
  const big = "x".repeat(100 * 1024);
  assert(big.length > RESULT_CAP);
  const large = (c: ReturnType<typeof counters>) => async (run: Run) => {
    const got = await run.step("blob", c.exec("blob", big));
    await run.step("length", c.exec("length", got.length));
  };

  const first = counters();
  assertEquals(await replay(large(first), s.env, "{}"), { code: 0 });
  const file = `${s.env.runDir}/${STEPS_DIR}/1.json`;
  const bytes = await Deno.readFile(file);
  assertEquals(JSON.parse(new TextDecoder().decode(bytes)), big);
  const digest = Array.from(
    new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (b) => b.toString(16).padStart(2, "0"),
  ).join("");
  const [blob, length] = closes(await lines(s));
  assertEquals(blob.payload.sha, digest, "the event carries the file's sha256");
  assertEquals(blob.payload.result, undefined, "and no inline result");
  assertEquals(
    length.payload.result,
    big.length,
    "a small result stays inline",
  );

  const second = counters();
  assertEquals(await replay(large(second), s.env, "{}"), { code: 0 });
  assertEquals(second.calls, {});

  // The file moved under the run: the replay refuses rather than returning it.
  await Deno.writeTextFile(file, JSON.stringify("y".repeat(100 * 1024)));
  const moved = await replay(large(counters()), s.env, "{}");
  assertEquals(moved.code, 1);
  assertMatch(
    String((moved as { reason: unknown }).reason),
    /step 1's result file .* hashes to/,
  );
});

Deno.test("now, random and input are steps: a re-run sees the first run's values", async () => {
  const s = await scratch();
  const seen: Record<string, unknown>[] = [];
  const reads = async (run: Run) => {
    seen.push({
      now: await run.now(),
      random: await run.random(),
      city: await run.input("city"),
      absent: await run.input("absent"),
    });
  };
  const document = JSON.stringify({
    workflow: "w",
    inputs: { city: "Lisbon" },
  });
  assertEquals(await replay(reads, s.env, document), { code: 0 });
  assertEquals(await replay(reads, s.env, document), { code: 0 });
  assertEquals(seen.length, 2);
  assertEquals(
    seen[1],
    seen[0],
    "the clock, the random source and the inputs replay",
  );
  assertMatch(String(seen[0].now), /^\d{4}-\d\d-\d\dT/);
  assert(typeof seen[0].random === "number");
  assertEquals(seen[0].city, "Lisbon");
  assertEquals(seen[0].absent, null);
  assertEquals(
    closes(await lines(s)).map((l) => l.payload.name),
    ["now", "random", "input city", "input absent"],
  );
});

Deno.test("config reads the pack's pinned settings: the set value, the resolved default, undefined for neither, and no step", async () => {
  const s = await scratch();
  const seen: Record<string, unknown>[] = [];
  const reads = (run: Run) => {
    seen.push({
      test: run.config("takeoff.test"),
      width: run.config("width"),
      touched: run.config("takeoff.touched"),
      inherited: run.config("toString"),
    });
  };
  const document = JSON.stringify({
    workflow: "w",
    inputs: {},
    config: { "takeoff.test": "cargo nextest run --workspace", width: 1 },
  });
  assertEquals(await replay(reads, s.env, document), { code: 0 });
  assertEquals(seen[0], {
    test: "cargo nextest run --workspace",
    width: 1,
    touched: undefined,
    inherited: undefined,
  });
  assertEquals(
    closes(await lines(s)),
    [],
    "reading a setting records no step: the document is already pinned",
  );

  // A document pinned before settings existed carries no `config` at all.
  assertEquals(
    await replay(reads, s.env, JSON.stringify({ workflow: "w", inputs: {} })),
    { code: 0 },
  );
  assertEquals(seen[1].test, undefined);
});

Deno.test("any other throw: exit 1 with the reason; a step that threw is started and not closed", async () => {
  const s = await scratch();
  const outcome = await replay(
    async (run: Run) => {
      await run.step("fetch", () => 1);
      await run.step("explode", () => {
        throw new Error("the remote said no");
      });
    },
    s.env,
    "{}",
  );
  assertEquals(outcome, { code: 1, reason: "the remote said no" });
  const all = await lines(s);
  assertEquals(closes(all).map((l) => l.payload.n), [1]);
  assertEquals(starts(all).map((l) => l.payload.n), [1, 2]);
});

/** The wrapper as a process: the fixture under `deno run`, with the run's
 * environment and the pinned document on stdin. */
async function process(
  s: Scratch,
  stdin: string,
  env: Record<string, string> = {},
): Promise<{ code: number; last: string; stdout: string }> {
  const vars: Record<string, string> = {
    FLEET_RUN_ID: s.env.runId,
    FLEET_STREAM: s.env.stream,
    FLEET_STREAM_SEQ: `${s.env.streamSeq}`,
    FLEET_RUN_DIR: s.env.runDir,
    FLEET_BIN: s.env.bin,
    FLEET_PROJECT: s.project,
    FLEET_DIR: s.machine,
    ...env,
  };
  const command = new Deno.Command(Deno.execPath(), {
    args: [
      "run",
      "--allow-read",
      "--allow-write",
      "--allow-run",
      "--allow-env",
      `${here}/testdata/waits.ts`,
    ],
    cwd: s.env.runDir,
    env: vars,
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });
  const child = command.spawn();
  const writer = child.stdin.getWriter();
  await writer.write(new TextEncoder().encode(stdin));
  await writer.close();
  const ran = await child.output();
  const stdout = new TextDecoder().decode(ran.stdout);
  const stderr = new TextDecoder().decode(ran.stderr);
  const printed = stdout.split("\n").filter((l) => l.trim() !== "");
  return {
    code: ran.code,
    last: printed[printed.length - 1] ?? "",
    stdout: `${stdout}\n--- stderr ---\n${stderr}`,
  };
}

// THE LAST LINE IS THE CONDITION OR THE REASON ALONE, never wrapped: core
// stores that line whole as the event's `wake` or `reason`, so a wrapper
// printed here is a second one on the stream, and `fleet status` prints a
// failure as `{"reason":"…"}` where it should print the text.
Deno.test("the process wrapper: the wake condition on stdout's last line and exit 2, then exit 0 on the re-run, and exit 1 with the reason", async () => {
  const s = await scratch();
  const waiting = await process(s, "{}");
  assertEquals(waiting.code, 2, waiting.stdout);
  assertEquals(waiting.last, JSON.stringify({ until: "answered" }));
  assert(
    waiting.stdout.startsWith("a line before the last one\n"),
    "earlier lines stay above it",
  );

  await Deno.writeTextFile(`${s.env.runDir}/answered`, "");
  const closed = await process(s, "{}");
  assertEquals(closed.code, 0, closed.stdout);
  assertEquals(
    closed.last,
    "a line before the last one",
    "a close prints no last line of its own",
  );
  assertEquals(closes(await lines(s)).map((l) => l.payload.name), [
    "count",
    "gate",
    "input boom",
  ]);

  const t = await scratch();
  await Deno.writeTextFile(`${t.env.runDir}/answered`, "");
  const failed = await process(t, JSON.stringify({ inputs: { boom: "yes" } }));
  assertEquals(failed.code, 1, failed.stdout);
  assertEquals(failed.last, JSON.stringify("boom: yes"));

  const unset = await process(t, "{}", { FLEET_RUN_ID: "" });
  assertEquals(unset.code, 1, unset.stdout);
  assertEquals(unset.last, JSON.stringify("FLEET_RUN_ID is not set"));
});
