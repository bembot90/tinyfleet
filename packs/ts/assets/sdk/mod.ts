// The SDK's core: the workflow wrapper and the replay contract (fleet-layers
// § The replay contract).
//
// Every call a workflow makes through the run handle is a numbered step. On
// start the wrapper reads the run's closed steps off the stream, and at step n
// it returns the recorded result when a closed step n exists under the same
// name; otherwise it records `step.started`, runs the step, records
// `step.closed` with the result, and returns it. A name mismatch at n is the
// code changed under a live run: the run fails naming the step rather than
// guessing. Time, randomness and the pinned inputs are steps too, so a re-run
// sees the values the first run saw.
//
// Waiting is an exit and not a sleep: a step that cannot close yet throws
// `Waiting` with its condition, the wrapper prints the condition as stdout's
// last line and exits 2, and the controller re-runs the bundle once a line
// the condition names reaches the stream — any line, for a condition it
// cannot read. Replay carries the re-run back to the same step. Any other
// throw prints its reason the same way and exits 1.
//
// The stream is reached through the fleet binary alone: `fleet event step`
// writes the pair and `fleet event tail --json` reads it back. The wrapper
// opens the stream file for neither, so what it records and what it replays
// cannot be two files.
//
// AN ITEM IS READ OFF THE STORE AND NEVER OFF THE STREAM: `until`, `spawn` and
// `hold` read `fleet item show <id> --json` and fold the item's timeline, the
// typed entries its store keeps. A stream line's payload decides nothing; the
// line is the controller's signal that an entry was written. So a step that
// cannot close waits on `{ items, kinds, since }` — the items to read again,
// the entry kinds any of which could satisfy it, and FLEET_STREAM_SEQ, where
// this execution started — and the controller re-runs the bundle on a line
// for one of those items and kinds above that position.
//
// The verbs — spawn, review, land, hold, until and start — are each one step
// whose exec runs the binary with `--json` and records the envelope's `data`;
// a refusal envelope is a thrown `Refusal`, exit 1 with the code in the
// reason, and so is a read the store refuses. Every act a verb takes is
// attributed `--by run:<id>`, the run typed as an actor, and the two verbs that
// leave something open across a Waiting exit find their own record on a re-run
// by that actor: the k-th ask this run held on its own record is its k-th
// `hold`, and the k-th run it started on the stream its k-th `start`, so
// neither holds nor starts twice.

/// The six names the run exports to its child (core's run module), and the
/// machine directory the binary resolves the stream under.
export interface Env {
  runId: string;
  stream: string;
  streamSeq: number;
  runDir: string;
  bin: string;
  /** FLEET_PROJECT, the project root the run was started inside. Every fleet
   * process this handle starts runs from it: the wrapper's own cwd is the run
   * directory, which sits under the machine and above no `fleet.toml`, so a
   * verb started there would resolve no project. */
  project: string;
  /** FLEET_DIR, handed to every fleet process this handle starts when set. */
  dir?: string;
}

/** The exit the wrapper takes, and the last line it prints for it. */
export type Outcome =
  | { code: 0 }
  | { code: 1; reason: unknown }
  | { code: 2; waiting: unknown };

/** Thrown from a step that cannot close yet. The condition is the wake the
 * controller records, carried whole. */
export class Waiting {
  constructor(readonly condition: unknown) {}
}

/** Thrown at step n when the recorded name is not the one the code gives. */
export class Divergence extends Error {
  constructor(
    readonly n: number,
    readonly expected: string,
    readonly found: string,
  ) {
    super(`replay diverged at step ${n}: expected ${expected} got ${found}`);
  }
}

/** Thrown when a verb's envelope reads `ok: false`: the refusal's code, which
 * names the exit table's row, and its why. The wrapper's reason carries the
 * three fields whole. */
export class Refusal extends Error {
  constructor(
    readonly verb: string,
    readonly code: string,
    readonly why: string,
  ) {
    super(`fleet ${verb} refused (${code}): ${why}`);
  }
}

/** What `spawn` takes. The one role a verb cuts is a builder — a reviewer's
 * spawn is the flight's own — and a hand-run dispatch pins no model, so a
 * model given is refused rather than dropped. */
export interface Spawn {
  role: "builder";
  item: string;
  model?: string;
  /** The builder's checks, handed to `fleet dispatch --touched`: the command
   * its brief names for the seat to run over its own diff. Absent, the brief
   * names the absence instead. */
  touched?: string;
}

/** What `land` takes beyond the item and the commit. */
export interface LandOptions {
  /** The command `fleet land --test` runs on the rebased tree before the
   * push. Absent, the landing runs nothing and says NOT TESTED. */
  test?: string;
}

/** What `review` writes: the accept, or the return with its findings file —
 * a JSON file of the shape core's `assets/findings.schema.json` gives,
 * `{"findings": [{"text": "..."}]}`, which the verb numbers `F1`, `F2` in its
 * order. A file that does not read is refused, exit 2, before anything is
 * written. */
export type Verdict = "accepted" | { returned: string };

/** The states `until` waits on an item to reach, each read off its record. */
export type ItemState =
  | "dispatched"
  | "delivered"
  | "reviewed"
  | "returned"
  | "landed"
  | "held";

/** The entry kinds each state is read off: any one of them written on the
 * item could satisfy a wait on the state, so they are the kinds its Waiting
 * names for the controller to wake it on. A verdict is one kind, accepted or
 * returned. */
export const KINDS_OF: Record<ItemState, string[]> = {
  dispatched: ["ordered"],
  delivered: ["delivered"],
  reviewed: ["reviewed"],
  returned: ["reviewed"],
  landed: ["landed"],
  held: ["held"],
};

const ITEM_STATES = Object.keys(KINDS_OF) as readonly ItemState[];

/** A seat, as every machine-readable document names one: its full id, its
 * own name where it has one — the key is absent, never null, where it has
 * none — and its kind. Key on the id; the name is free to change. */
export interface Seat {
  id: string;
  name?: string;
  kind: "agent" | "human";
}

/** `fleet dispatch --json`'s data. Each verb's `state` is the kind of the
 * entry it wrote: `ordered` here. */
export interface Dispatched {
  item: string;
  state: string;
  seat: Seat;
}

/** `fleet review --json`'s data: the one entry kind either verdict writes,
 * `reviewed`, and which verdict it was. */
export interface Reviewed {
  item: string;
  state: string;
  verdict: "accepted" | "returned";
}

/** `fleet land --json`'s data. */
export interface Landed {
  item: string;
  state: string;
  sha: string;
}

interface Held {
  item: string;
  state: string;
  hold: string;
}

/** What a hold's answer licenses: the items, the commit where one is named,
 * and the letter of the option that licenses them. */
export interface About {
  items: string[];
  commit?: string;
  licenses: string;
}

/** The handle a workflow runs against. */
export interface Run {
  readonly id: string;
  readonly env: Env;
  /** The pinned inputs, as the document the run wrote to stdin. */
  readonly document: Record<string, unknown>;
  /** One numbered step: the recorded result, or `exec` run and recorded. */
  step<T>(name: string, exec: () => T | Promise<T>): Promise<T>;
  /** The clock, as a step: an ISO-8601 stamp. */
  now(): Promise<string>;
  /** The runtime's random source, as a step: a number in [0, 1). */
  random(): Promise<number>;
  /** One pinned input, as a step: `inputs.<key>` of the document, null when
   * the run pinned none under that key. */
  input(key: string): Promise<unknown>;
  /** One setting of the pack that carries this workflow: the value fleet.toml
   * sets under `[packs.<name>]`, else the default the pack's manifest
   * declares, else undefined. Read off `config` in the pinned document, which
   * core resolved and pinned when the run was opened, so a re-run reads the
   * value the first run read whatever fleet.toml says since.
   *
   * NOT A STEP: the document is pinned and hashed with the bundle, so there is
   * nothing a step would add, and a recorded result cannot carry undefined. */
  config(key: string): unknown;
  /** A builder on an item: `fleet dispatch <item> [--touched <command>]
   * --json`, which cuts a transient seat, orders it and rings it — unless the
   * item is carried, which is an item a failed run delivered and left behind:
   * dispatch refuses an item that already carries an order, so the step closes
   * on `${RETAKEN}<commit>` without a dispatch and the flight reads the
   * delivery already there.
   *
   * AN ITEM WHOSE STANDING DELIVERY NO LANDING FOLLOWED IS CARRIED, read off
   * its record. A delivery a return followed is a verdict's return, and one a
   * landing followed closed the item: neither is a delivery to review, so the
   * item is dispatched as any other. The landing is spawn's alone: `until`
   * reads the return and not the landing. A spawn this run already recorded
   * replays its result and reads nothing, so its own builder's delivery is
   * never taken for one left behind. */
  spawn(order: Spawn): Promise<Dispatched | string>;
  /** `fleet review <item> --json`: `--land` for the accept, `--return <file>`
   * for the return, the file JSON in the shape {@link Verdict} names. */
  review(item: string, verdict: Verdict): Promise<Reviewed>;
  /** `fleet land <item> <sha> [--test <command>] --json`. */
  land(item: string, sha: string, options?: LandOptions): Promise<Landed>;
  /** A question for a person, held on the run's own record item: the
   * question file, JSON of the shape `assets/question.schema.json`, each
   * option `<letter>. <text>` taken apart into its letter and its text, then
   * `fleet hold --question <file> --json` — unless this run's k-th ask is
   * already on its record, for its k-th hold. Then the hold's clearance, read
   * off the record: the letter it was answered with, `"cancelled"` where it
   * was cancelled, or Waiting on the record's `cleared` entries. `about` names
   * the items the answer licenses and the letter that licenses them. */
  hold(question: string, options: string[], about?: About): Promise<string>;
  /** Waiting until every item's record answers `state`, naming the
   * outstanding items and the state's entry kinds ({@link KINDS_OF}); then
   * each item's answering entry, as `fleet item show --json` prints it:
   * `dispatched` its current order; `delivered` its standing delivery, the
   * latest no return follows — so an item whose delivery was returned is
   * outstanding until it is delivered again, and a landing does not withdraw
   * it, so a landed item answers the delivery it landed rather than waiting
   * for one that will never come; `reviewed` and `returned` its last verdict,
   * where it is that verdict and no delivery follows it; `held` its open
   * hold; `landed` its last landing. */
  until(items: string[], state: ItemState): Promise<Record<string, unknown>>;
  /** A child run: `fleet run <name> --input k=v …`, then `{ run }` once its
   * `run.closed` is on the stream, a failure once its `run.failed` or its
   * `run.cancelled` is, else Waiting on the child's run id. */
  start(
    name: string,
    inputs?: Record<string, unknown>,
  ): Promise<{ run: string }>;
}

/** The kinds the wrapper and `start` read off the stream. Spelled here because
 * the SDK reaches core through the binary alone. */
const STEP_CLOSED = "step.closed";
const RUN_STARTED = "run.started";
const RUN_CLOSED = "run.closed";
const RUN_FAILED = "run.failed";
const RUN_CANCELLED = "run.cancelled";

/** Where a hold's question file goes under the run directory. */
export const HOLDS_DIR = "holds";

/** What a spawn step closes on where the item is carried — its record holds a
 * delivery no return and no landing followed — the delivery's commit
 * appended. */
export const RETAKEN = "already delivered at ";

/** A result over this many bytes goes to the run directory and the event
 * carries its sha256 instead. */
export const RESULT_CAP = 64 * 1024;

/** Where the results over the cap live under the run directory. */
export const STEPS_DIR = "steps";

interface Closed {
  name: string;
  result?: unknown;
  sha?: string;
}

/** The core, with its environment and its stdin handed in: the process
 * wrapper below reads both and exits on the outcome, and a test reads the
 * outcome without a process. */
export async function replay(
  fn: (run: Run) => unknown | Promise<unknown>,
  env: Env,
  stdin: string,
): Promise<Outcome> {
  try {
    const document = parseDocument(stdin);
    const closed = await recordedSteps(env);
    const handle = new Handle(env, document, closed);
    await fn(handle);
    return { code: 0 };
  } catch (thrown) {
    if (thrown instanceof Waiting) {
      return { code: 2, waiting: thrown.condition };
    }
    return { code: 1, reason: reasonOf(thrown) };
  }
}

/** The process wrapper: the environment, stdin, the outcome's last line on
 * stdout, and the exit. Never returns. */
export async function workflow(
  fn: (run: Run) => unknown | Promise<unknown>,
): Promise<never> {
  let outcome: Outcome;
  try {
    const env = envFromProcess();
    const stdin = await readAll(Deno.stdin.readable);
    outcome = await replay(fn, env, stdin);
  } catch (thrown) {
    outcome = { code: 1, reason: reasonOf(thrown) };
  }
  // THE CONDITION AND THE REASON ALONE, not wrapped: core stores the last line
  // whole as the event's `wake` or `reason`, so a key added here is a second
  // wrapper on the stream and a status row that prints `{"reason":…}` where
  // the text belongs. A condition of undefined prints null, as a reason does.
  if (outcome.code === 2) {
    console.log(JSON.stringify(outcome.waiting ?? null));
  }
  if (outcome.code === 1) {
    console.log(JSON.stringify(outcome.reason ?? null));
  }
  Deno.exit(outcome.code);
}

class Handle implements Run {
  readonly id: string;
  /** The run as an actor, typed: what every verb is attributed `--by`, and
   * what its own lines on the stream carry. */
  private readonly actor: string;
  private n = 0;
  private holds = 0;
  private starts = 0;

  constructor(
    readonly env: Env,
    readonly document: Record<string, unknown>,
    private readonly closed: Map<number, Closed>,
  ) {
    this.id = env.runId;
    this.actor = `run:${env.runId}`;
  }

  async step<T>(name: string, exec: () => T | Promise<T>): Promise<T> {
    const n = ++this.n;
    const recorded = this.closed.get(n);
    if (recorded !== undefined) {
      if (recorded.name !== name) throw new Divergence(n, recorded.name, name);
      return (await this.resolve(n, recorded)) as T;
    }
    await this.fleet([
      "event",
      "step",
      "started",
      "--run",
      this.id,
      "--n",
      `${n}`,
      "--name",
      name,
    ]);
    const result = await exec();
    const text = JSON.stringify(result === undefined ? null : result);
    const carry = new TextEncoder().encode(text).byteLength > RESULT_CAP
      ? ["--sha", await this.spill(n, text)]
      : ["--result", text];
    await this.fleet([
      "event",
      "step",
      "closed",
      "--run",
      this.id,
      "--n",
      `${n}`,
      "--name",
      name,
      ...carry,
    ]);
    return result;
  }

  now(): Promise<string> {
    return this.step("now", () => new Date().toISOString());
  }

  random(): Promise<number> {
    return this.step("random", () => Math.random());
  }

  input(key: string): Promise<unknown> {
    return this.step(`input ${key}`, () => {
      const inputs = this.document.inputs;
      if (inputs === null || typeof inputs !== "object") return null;
      const value = (inputs as Record<string, unknown>)[key];
      return value === undefined ? null : value;
    });
  }

  config(key: string): unknown {
    const config = this.document.config;
    if (config === null || typeof config !== "object") return undefined;
    return Object.hasOwn(config, key)
      ? (config as Record<string, unknown>)[key]
      : undefined;
  }

  spawn(order: Spawn): Promise<Dispatched | string> {
    return this.step(`spawn ${order.item}`, async () => {
      if (order.role !== "builder") {
        throw new Error(
          `spawn: no verb cuts a ${order.role}; a builder is the one role fleet dispatch spawns`,
        );
      }
      if (order.model !== undefined) {
        throw new Error(
          "spawn: fleet dispatch pins no model; the policy's default is the one a hand-run dispatch takes",
        );
      }
      const delivery = carried((await this.read(order.item)).timeline);
      if (delivery !== undefined) {
        return `${RETAKEN}${String(delivery.commit)}`;
      }
      return this.verb<Dispatched>([
        "dispatch",
        order.item,
        ...given("--touched", order.touched),
      ]);
    });
  }

  review(item: string, verdict: Verdict): Promise<Reviewed> {
    return this.step(`review ${item}`, () => {
      const mode = verdict === "accepted"
        ? ["--land"]
        : ["--return", verdict.returned];
      return this.verb<Reviewed>(["review", item, ...mode]);
    });
  }

  land(item: string, sha: string, options: LandOptions = {}): Promise<Landed> {
    return this.step(
      `land ${item}`,
      () =>
        this.verb<Landed>([
          "land",
          item,
          sha,
          ...given("--test", options.test),
        ]),
    );
  }

  hold(question: string, options: string[], about?: About): Promise<string> {
    const k = ++this.holds;
    return this.step(`hold ${question}`, async () => {
      // Taken apart before anything is read or written: an option the verb
      // could not letter is the workflow's mistake, not the person's.
      const choices = options.map((o) => {
        const lettered = /^([A-Z])\. (.+)$/.exec(o);
        if (lettered === null) {
          throw new Error(`hold: the option ${o} is not <letter>. <text>`);
        }
        return { letter: lettered[1], text: lettered[2] };
      });
      // THIS RUN'S OWN ASKS, AND NO OTHER HOLD ON ITS RECORD: the controller
      // holds the same record at the crash cap, and that hold is no question
      // this run put.
      const asks = (await this.read(this.id)).timeline.filter((e) =>
        e.kind === "held" && e.reason === "ask" && e.by === `run:${this.id}`
      );
      let hold: string;
      if (asks.length >= k) {
        hold = String(asks[k - 1].hold);
      } else {
        const file = `${this.env.runDir}/${HOLDS_DIR}/${k}.json`;
        await Deno.mkdir(`${this.env.runDir}/${HOLDS_DIR}`, {
          recursive: true,
        });
        await Deno.writeTextFile(
          file,
          JSON.stringify({
            question,
            options: choices,
            ...(about ? { about } : {}),
          }),
        );
        const held = await this.verb<Held>([
          "hold",
          "--item",
          this.id,
          "--question",
          file,
        ]);
        hold = held.hold;
      }
      const clearance = clearanceOf((await this.read(this.id)).timeline, hold);
      if (clearance === undefined) {
        throw new Waiting({
          items: [this.id],
          kinds: ["cleared"],
          since: this.env.streamSeq,
        });
      }
      // A CANCEL TAKES NO OPTION, so it has no letter to answer with: the word
      // says what happened, and no option's letter is a word.
      if (clearance.how === "cancel") return "cancelled";
      return String(clearance.letter);
    });
  }

  until(items: string[], state: ItemState): Promise<Record<string, unknown>> {
    return this.step(`until ${state} ${items.join(" ")}`, async () => {
      if (!ITEM_STATES.includes(state)) {
        throw new Error(
          `until: no item state is named ${state}; the states are ${
            ITEM_STATES.join(", ")
          }`,
        );
      }
      const seen = new Map<string, Entry>();
      for (const item of items) {
        const answer = answerOf((await this.read(item)).timeline, state);
        if (answer !== undefined) seen.set(item, answer);
      }
      const outstanding = items.filter((item) => !seen.has(item));
      if (outstanding.length > 0) {
        throw new Waiting({
          items: outstanding,
          kinds: KINDS_OF[state],
          since: this.env.streamSeq,
        });
      }
      return Object.fromEntries(items.map((item) => [item, seen.get(item)]));
    });
  }

  start(
    name: string,
    inputs: Record<string, unknown> = {},
  ): Promise<{ run: string }> {
    const k = ++this.starts;
    return this.step(`start ${name}`, async () => {
      let mine = await this.tail({ type: RUN_STARTED, actor: this.actor });
      if (mine.length < k) {
        const args = ["run", name, "--by", this.actor];
        for (const [key, value] of Object.entries(inputs)) {
          const text = typeof value === "string"
            ? value
            : JSON.stringify(value);
          args.push("--input", `${key}=${text}`);
        }
        await this.fleet(args);
        mine = await this.tail({ type: RUN_STARTED, actor: this.actor });
        if (mine.length < k) {
          throw new Error(
            `start: fleet run ${name} exited 0 and no ${RUN_STARTED} by ${this.actor} followed on the stream`,
          );
        }
      }
      const child = String(mine[k - 1].payload.run);
      const closed = (await this.tail({ type: RUN_CLOSED }))
        .some((r) => r.payload.run === child);
      if (closed) return { run: child };
      const failed = (await this.tail({ type: RUN_FAILED }))
        .find((r) => r.payload.run === child);
      if (failed !== undefined) {
        throw new Error(
          `start: run ${child} failed: ${
            JSON.stringify(failed.payload.reason)
          }`,
        );
      }
      // A CANCEL IS AN ENDING: nothing executes a cancelled child again, so a
      // parent that kept waiting on it would wait for good.
      const cancelled = (await this.tail({ type: RUN_CANCELLED }))
        .find((r) => r.payload.run === child);
      if (cancelled !== undefined) {
        throw new Error(
          `start: run ${child} was cancelled${
            cancelled.actor === null
              ? ""
              : ` by ${cancelled.actor.kind}:${cancelled.actor.id}`
          }`,
        );
      }
      throw new Waiting(child);
    });
  }

  /** One verb under `--json`, `--by run:<id>`: the envelope's data, or the
   * refusal thrown. */
  private verb<T>(args: string[]): Promise<T> {
    return answered<T>(this.env, [...args, "--by", this.actor, "--json"]);
  }

  /** One item's record: `fleet item show <item> --json`, which writes nothing
   * and so is attributed to nobody. An item the store does not know is the
   * verb's refusal, thrown. */
  private async read(item: string): Promise<Shown> {
    const args = ["item", "show", item, "--json"];
    const data = await answered<Record<string, unknown>>(this.env, args);
    if (
      data === null || typeof data !== "object" || !Array.isArray(data.timeline)
    ) {
      throw new Error(
        `${this.env.bin} ${args.join(" ")} answered no timeline: ${
          JSON.stringify(data)
        }`,
      );
    }
    return data as unknown as Shown;
  }

  private tail(filter: Filter): Promise<Record_[]> {
    return tail(this.env, filter);
  }

  /** A recorded result: inline, or read from the run directory and checked
   * against the sha the event carries. */
  private async resolve(n: number, recorded: Closed): Promise<unknown> {
    if (recorded.sha === undefined) return recorded.result;
    const path = this.stepFile(n);
    const bytes = await Deno.readFile(path);
    const sha = await sha256(bytes);
    if (sha !== recorded.sha) {
      throw new Error(
        `step ${n}'s result file ${path} hashes to ${sha}, not the ${recorded.sha} its event carries`,
      );
    }
    return JSON.parse(new TextDecoder().decode(bytes));
  }

  private async spill(n: number, text: string): Promise<string> {
    const bytes = new TextEncoder().encode(text);
    await Deno.mkdir(`${this.env.runDir}/${STEPS_DIR}`, { recursive: true });
    await Deno.writeFile(this.stepFile(n), bytes);
    return await sha256(bytes);
  }

  private stepFile(n: number): string {
    return `${this.env.runDir}/${STEPS_DIR}/${n}.json`;
  }

  private fleet(args: string[]): Promise<string> {
    return fleet(this.env, args);
  }
}

/** A flag and its command, or nothing where no command was given: a blank one
 * is none, and a verb handed `--test ""` would read a command that is not
 * there. */
function given(flag: string, command: string | undefined): string[] {
  return command === undefined || command.trim() === "" ? [] : [flag, command];
}

/** One fleet process whose stdout ends on the `--json` envelope: its data, or
 * its refusal thrown as a `Refusal`; a non-zero exit that printed no refusal
 * is the exit, thrown. */
async function answered<T>(env: Env, args: string[]): Promise<T> {
  const ran = await spawnFleet(env, args);
  const envelope = lastDocument(ran.stdout);
  if (envelope !== null && envelope.ok === false) {
    const refusal = (envelope.refusal ?? {}) as Record<string, unknown>;
    throw new Refusal(
      String(envelope.verb),
      String(refusal.code),
      String(refusal.why),
    );
  }
  if (ran.code !== 0) throw exited(env, args, ran);
  if (envelope === null || envelope.ok !== true) {
    throw new Error(
      `${env.bin} ${args.join(" ")} printed no envelope: ${ran.stdout.trim()}`,
    );
  }
  return envelope.data as T;
}

// ---- an item's record --------------------------------------------------------

/** One entry of an item's timeline as `fleet item show --json` prints it: the
 * store's id and time, who wrote it as the actor's one string `<kind>:<id>`,
 * its kind — one of core's entry kinds — and the kind's own fields beside
 * them. */
interface Entry {
  id: string;
  at: string;
  by: string;
  kind: string;
  [field: string]: unknown;
}

/** `fleet item show --json`'s data, as far as the verbs read it: the entries
 * in the store's order. */
interface Shown {
  id: string;
  status: string;
  timeline: Entry[];
}

// THE FOLDS BELOW ARE CORE'S TIMELINE'S (`fleet_core::entry::Timeline`), by
// position in the store's order, so the SDK and the verbs answer one question
// the same way.

/** The position of the last entry `is` takes, or -1. */
function lastAt(timeline: Entry[], is: (e: Entry) => boolean): number {
  for (let at = timeline.length - 1; at >= 0; at--) {
    if (is(timeline[at])) return at;
  }
  return -1;
}

/** Whether any entry after position `at` is one `is` takes. */
function after(
  timeline: Entry[],
  at: number,
  is: (e: Entry) => boolean,
): boolean {
  return timeline.slice(at + 1).some(is);
}

function isVerdict(verdict: "accepted" | "returned"): (e: Entry) => boolean {
  return (e) => e.kind === "reviewed" && e.verdict === verdict;
}

/** The last entry of `kind`. */
function lastOf(timeline: Entry[], kind: string): Entry | undefined {
  const at = lastAt(timeline, (e) => e.kind === kind);
  return at < 0 ? undefined : timeline[at];
}

/** The last order, where no withdrawal came after it. */
function currentOrder(timeline: Entry[]): Entry | undefined {
  const at = lastAt(timeline, (e) => e.kind === "ordered");
  if (at < 0 || after(timeline, at, (e) => e.kind === "order_withdrawn")) {
    return undefined;
  }
  return timeline[at];
}

/** The position of the last delivery no return came after, or -1. */
function standingAt(timeline: Entry[]): number {
  const at = lastAt(timeline, (e) => e.kind === "delivered");
  return at < 0 || after(timeline, at, isVerdict("returned")) ? -1 : at;
}

/** The last delivery no return came after. A landing does not clear it
 * (fleet-45m): what was landed still stood when it was. */
function standing(timeline: Entry[]): Entry | undefined {
  const at = standingAt(timeline);
  return at < 0 ? undefined : timeline[at];
}

/** The standing delivery, where no landing came after it: the work the item
 * still carries. */
function carried(timeline: Entry[]): Entry | undefined {
  const at = standingAt(timeline);
  if (at < 0 || after(timeline, at, (e) => e.kind === "landed")) {
    return undefined;
  }
  return timeline[at];
}

/** The last verdict, where it is `verdict` and no delivery came after it: a
 * delivery after a verdict is new work nobody has judged. */
function lastVerdict(
  timeline: Entry[],
  verdict: "accepted" | "returned",
): Entry | undefined {
  const at = lastAt(timeline, (e) => e.kind === "reviewed");
  if (at < 0 || !isVerdict(verdict)(timeline[at])) return undefined;
  if (after(timeline, at, (e) => e.kind === "delivered")) return undefined;
  return timeline[at];
}

/** The last hold no clearance naming it came after. */
function openHold(timeline: Entry[]): Entry | undefined {
  for (let at = timeline.length - 1; at >= 0; at--) {
    const e = timeline[at];
    const clears = (c: Entry) => c.kind === "cleared" && c.hold === e.hold;
    if (e.kind === "held" && !after(timeline, at, clears)) return e;
  }
  return undefined;
}

/** The first clearance naming `hold`, whether or not a hold came before it: a
 * clearance answers by the id it names. */
function clearanceOf(timeline: Entry[], hold: string): Entry | undefined {
  return timeline.find((e) => e.kind === "cleared" && e.hold === hold);
}

/** The entry that answers `until` for `state`, where the record holds one. */
function answerOf(timeline: Entry[], state: ItemState): Entry | undefined {
  switch (state) {
    case "dispatched":
      return currentOrder(timeline);
    case "delivered":
      return standing(timeline);
    case "reviewed":
      return lastVerdict(timeline, "accepted");
    case "returned":
      return lastVerdict(timeline, "returned");
    case "held":
      return openHold(timeline);
    case "landed":
      return lastOf(timeline, "landed");
  }
}

/** Every closed step of this run, by number, off `fleet event tail --json`. */
async function recordedSteps(env: Env): Promise<Map<number, Closed>> {
  const closed = new Map<number, Closed>();
  for (const record of await tail(env, { type: STEP_CLOSED })) {
    const payload = record.payload;
    if (payload.run !== env.runId) continue;
    if (typeof payload.n !== "number" || typeof payload.name !== "string") {
      continue;
    }
    const entry: Closed = { name: payload.name };
    if (typeof payload.sha === "string") entry.sha = payload.sha;
    else entry.result = payload.result ?? null;
    closed.set(payload.n, entry);
  }
  return closed;
}

/** The filters `fleet event tail` takes: the kind, and the typed actor that
 * wrote the line, `<kind>:<id>`, under `--actor`. */
interface Filter {
  type: string;
  actor?: string;
}

/** Who wrote a line, as the stream stores it. */
interface Actor {
  kind: string;
  id: string;
}

/** One stored event as the tail's envelope carries it. */
interface Record_ {
  seq: number;
  kind: string;
  actor: Actor | null;
  payload: Record<string, unknown>;
}

/** The stream through `fleet event tail --json`, filtered. `--since 0` is the
 * whole stream: what a re-run replays or waits on sits below the position this
 * execution started from. */
async function tail(env: Env, filter: Filter): Promise<Record_[]> {
  const args = [
    "event",
    "tail",
    "--json",
    "--since",
    "0",
    "--type",
    filter.type,
  ];
  if (filter.actor !== undefined) args.push("--actor", filter.actor);
  const printed = await fleet(env, args);
  const records: Record_[] = [];
  for (const line of printed.split("\n")) {
    if (line.trim() === "") continue;
    const envelope = JSON.parse(line);
    if (envelope.ok !== true) {
      throw new Error(
        `fleet event tail refused: ${JSON.stringify(envelope.refusal)}`,
      );
    }
    const data = envelope.data ?? {};
    const payload = data.payload;
    records.push({
      seq: Number(data.seq),
      kind: String(data.kind),
      actor: actorOf(data.actor),
      payload: payload !== null && typeof payload === "object" ? payload : {},
    });
  }
  return records;
}

/** The envelope's actor, where it is the typed object carrying a string kind
 * and a string id; anything else is no actor. */
function actorOf(actor: unknown): Actor | null {
  if (actor === null || typeof actor !== "object") return null;
  const { kind, id } = actor as Record<string, unknown>;
  return typeof kind === "string" && typeof id === "string"
    ? { kind, id }
    : null;
}

interface Ran {
  code: number;
  stdout: string;
  stderr: string;
}

/** One fleet process, its exit and both streams read. */
async function spawnFleet(env: Env, args: string[]): Promise<Ran> {
  const command = new Deno.Command(env.bin, {
    args,
    cwd: env.project,
    stdin: "null",
    stdout: "piped",
    stderr: "piped",
    env: env.dir === undefined ? {} : { FLEET_DIR: env.dir },
  });
  const ran = await command.output();
  return {
    code: ran.code,
    stdout: new TextDecoder().decode(ran.stdout),
    stderr: new TextDecoder().decode(ran.stderr),
  };
}

/** One fleet process: stdout on a 0, the refusal on anything else. */
async function fleet(env: Env, args: string[]): Promise<string> {
  const ran = await spawnFleet(env, args);
  if (ran.code !== 0) throw exited(env, args, ran);
  return ran.stdout;
}

function exited(env: Env, args: string[], ran: Ran): Error {
  return new Error(
    `${env.bin} ${args.join(" ")} exited ${ran.code}: ${
      ran.stderr.trim() || ran.stdout.trim()
    }`,
  );
}

/** Stdout's last non-empty line as a JSON object, or null where it is not
 * one: a verb's envelope is one document on one line. */
function lastDocument(stdout: string): Record<string, unknown> | null {
  const printed = stdout.split("\n").filter((l) => l.trim() !== "");
  const last = printed[printed.length - 1];
  if (last === undefined) return null;
  try {
    const parsed = JSON.parse(last);
    return parsed !== null && typeof parsed === "object" &&
        !Array.isArray(parsed)
      ? parsed
      : null;
  } catch {
    return null;
  }
}

function envFromProcess(): Env {
  const read = (name: string): string => {
    const value = Deno.env.get(name);
    if (value === undefined || value === "") {
      throw new Error(`${name} is not set`);
    }
    return value;
  };
  const seq = Number(read("FLEET_STREAM_SEQ"));
  if (!Number.isInteger(seq) || seq < 0) {
    throw new Error(
      `FLEET_STREAM_SEQ is not a sequence: ${Deno.env.get("FLEET_STREAM_SEQ")}`,
    );
  }
  return {
    runId: read("FLEET_RUN_ID"),
    stream: read("FLEET_STREAM"),
    streamSeq: seq,
    runDir: read("FLEET_RUN_DIR"),
    bin: read("FLEET_BIN"),
    project: read("FLEET_PROJECT"),
    dir: Deno.env.get("FLEET_DIR") || undefined,
  };
}

/** The pinned inputs, as the document the run writes to stdin; a workflow
 * handed nothing reads an empty document. */
function parseDocument(stdin: string): Record<string, unknown> {
  if (stdin.trim() === "") return {};
  const parsed = JSON.parse(stdin);
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("stdin is not a JSON object");
  }
  return parsed as Record<string, unknown>;
}

function reasonOf(thrown: unknown): unknown {
  if (thrown instanceof Refusal) {
    return { verb: thrown.verb, code: thrown.code, why: thrown.why };
  }
  if (thrown instanceof Error) return thrown.message;
  return thrown === undefined ? null : thrown;
}

async function readAll(stream: ReadableStream<Uint8Array>): Promise<string> {
  const chunks: Uint8Array[] = [];
  for await (const chunk of stream) chunks.push(chunk);
  const whole = new Uint8Array(chunks.reduce((n, c) => n + c.byteLength, 0));
  let at = 0;
  for (const chunk of chunks) {
    whole.set(chunk, at);
    at += chunk.byteLength;
  }
  return new TextDecoder().decode(whole);
}

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(
    new Uint8Array(digest),
    (b) => b.toString(16).padStart(2, "0"),
  ).join("");
}
