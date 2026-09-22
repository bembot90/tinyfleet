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
// `Waiting` with its condition, the wrapper prints `{"waiting": <condition>}`
// as stdout's last line and exits 2, and the controller re-runs the bundle
// when the stream moves past the position it recorded. Replay carries the
// re-run back to the same step.
//
// The stream is reached through the fleet binary alone: `fleet event step`
// writes the pair and `fleet event tail --json` reads it back. The wrapper
// opens the stream file for neither, so what it records and what it replays
// cannot be two files.
//
// The verbs — spawn, deliver, review, land, gate, until and start — are each
// one step whose exec runs the binary with `--json` and records the envelope's
// `data`; a refusal envelope is a thrown `Refusal`, exit 1 with the code in the
// reason. Every act a verb takes is attributed `--by` the run id, and the two
// verbs that leave something open across a Waiting exit — a gate's ask, a
// start's child run — find their own record on a re-run by that actor: the k-th
// park or the k-th run this run raised is the k-th gate or start call, so
// neither asks nor starts twice.

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
}

/** What `review` writes: the accept, or the return with its findings file. */
export type Verdict = "accepted" | { returned: string };

/** The item events `until` waits on, by their last word. */
export type ItemState =
  | "dispatched"
  | "held"
  | "delivered"
  | "reviewed"
  | "returned"
  | "landed"
  | "parked";

const ITEM_STATES: readonly ItemState[] = [
  "dispatched",
  "held",
  "delivered",
  "reviewed",
  "returned",
  "landed",
  "parked",
];

/** `fleet dispatch --json`'s data. */
export interface Dispatched {
  item: string;
  state: string;
  seat: string;
}

/** `fleet deliver --json`'s data. */
export interface Delivered {
  item: string;
  state: string;
  commit: string;
}

/** `fleet review --json`'s data. */
export interface Reviewed {
  item: string;
  state: string;
}

/** `fleet land --json`'s data. */
export interface Landed {
  item: string;
  state: string;
  sha: string;
}

interface Asked {
  item: string;
  state: string;
  gate: string;
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
  /** A builder on an item: `fleet dispatch <item> --json`, which cuts a
   * transient seat, orders it and rings it — unless the item's delivery is
   * already on the stream at or below the position this execution started
   * from, which is an item a failed run delivered and left behind: dispatch
   * refuses an item that already carries an order, so the step closes on
   * `${RETAKEN}<commit>` without a dispatch and the flight reads the delivery
   * already there. A delivery ABOVE that position is this run's own builder
   * answering a spawn this run has already recorded. */
  spawn(order: Spawn): Promise<Dispatched | string>;
  /** `fleet deliver --json` for the item, with the note file in the
   * delivery-note grammar. */
  deliver(item: string, note: string): Promise<Delivered>;
  /** `fleet review <item> --json`: `--land` for the accept, `--return <file>`
   * for the return. */
  review(item: string, verdict: Verdict): Promise<Reviewed>;
  /** `fleet land <item> <sha> --json`. */
  land(item: string, sha: string): Promise<Landed>;
  /** A question for a person, asked on the run's own record item: `fleet ask
   * --json` with the lettered options, then Waiting on the gate id; the
   * answer's letter once `gate.resolved` is on the stream. */
  gate(question: string, options: string[]): Promise<string>;
  /** Waiting until every item's `item.<state>` is on the stream, naming the
   * outstanding ones; then each item's event payload. */
  until(items: string[], state: ItemState): Promise<Record<string, unknown>>;
  /** A child run: `fleet run <name> --input k=v …`, then `{ run }` once its
   * `run.closed` is on the stream, else Waiting on the child's run id. */
  start(
    name: string,
    inputs?: Record<string, unknown>,
  ): Promise<{ run: string }>;
}

/** The kinds the verbs read off the stream. Spelled here because the SDK
 * reaches core through the binary alone. */
const STEP_CLOSED = "step.closed";
const ITEM_DELIVERED = "item.delivered";
const ITEM_PARKED = "item.parked";
const GATE_RESOLVED = "gate.resolved";
const RUN_STARTED = "run.started";
const RUN_CLOSED = "run.closed";
const RUN_FAILED = "run.failed";

/** Where a gate's question note goes under the run directory. */
export const GATES_DIR = "gates";

/** What a spawn step closes on where the item was already delivered before
 * this execution began, the delivery's commit appended. */
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
  if (outcome.code === 2) {
    console.log(JSON.stringify({ waiting: outcome.waiting }));
  }
  if (outcome.code === 1) {
    console.log(JSON.stringify({ reason: outcome.reason }));
  }
  Deno.exit(outcome.code);
}

class Handle implements Run {
  readonly id: string;
  private n = 0;
  private gates = 0;
  private starts = 0;

  constructor(
    readonly env: Env,
    readonly document: Record<string, unknown>,
    private readonly closed: Map<number, Closed>,
  ) {
    this.id = env.runId;
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
      const carried = (await this.tail({ type: ITEM_DELIVERED }))
        .filter((r) =>
          r.payload.item === order.item && r.seq <= this.env.streamSeq
        )
        .at(-1);
      if (carried !== undefined) {
        return `${RETAKEN}${String(carried.payload.commit)}`;
      }
      return this.verb<Dispatched>(["dispatch", order.item]);
    });
  }

  deliver(item: string, note: string): Promise<Delivered> {
    return this.step(
      `deliver ${item}`,
      () => this.verb<Delivered>(["deliver", "--item", item, "--note", note]),
    );
  }

  review(item: string, verdict: Verdict): Promise<Reviewed> {
    return this.step(`review ${item}`, () => {
      const mode = verdict === "accepted"
        ? ["--land"]
        : ["--return", verdict.returned];
      return this.verb<Reviewed>(["review", item, ...mode]);
    });
  }

  land(item: string, sha: string): Promise<Landed> {
    return this.step(
      `land ${item}`,
      () => this.verb<Landed>(["land", item, sha]),
    );
  }

  gate(question: string, options: string[]): Promise<string> {
    const k = ++this.gates;
    return this.step(`gate ${question}`, async () => {
      const parks = (await this.tail({ type: ITEM_PARKED, seat: this.id }))
        .filter((r) => r.payload.item === this.id);
      let gate: string;
      if (parks.length >= k) {
        gate = String(parks[k - 1].payload.gate);
      } else {
        const note = `${this.env.runDir}/${GATES_DIR}/${k}.md`;
        await Deno.mkdir(`${this.env.runDir}/${GATES_DIR}`, {
          recursive: true,
        });
        await Deno.writeTextFile(
          note,
          `QUESTION ${question}\n${options.join("\n")}\n`,
        );
        const asked = await this.verb<Asked>([
          "ask",
          "--item",
          this.id,
          "--note",
          note,
        ]);
        gate = asked.gate;
      }
      const resolved = (await this.tail({ type: GATE_RESOLVED }))
        .find((r) => r.payload.gate === gate);
      if (resolved === undefined) throw new Waiting(gate);
      return String(resolved.payload.letter);
    });
  }

  until(items: string[], state: ItemState): Promise<Record<string, unknown>> {
    return this.step(`until ${state} ${items.join(" ")}`, async () => {
      if (!ITEM_STATES.includes(state)) {
        throw new Error(
          `until: no item event is named item.${state}; the states are ${
            ITEM_STATES.join(", ")
          }`,
        );
      }
      const seen = new Map<string, unknown>();
      for (const r of await this.tail({ type: `item.${state}` })) {
        const item = r.payload.item;
        if (typeof item === "string" && items.includes(item)) {
          seen.set(item, r.payload);
        }
      }
      const outstanding = items.filter((item) => !seen.has(item));
      if (outstanding.length > 0) throw new Waiting(outstanding);
      return Object.fromEntries(items.map((item) => [item, seen.get(item)]));
    });
  }

  start(
    name: string,
    inputs: Record<string, unknown> = {},
  ): Promise<{ run: string }> {
    const k = ++this.starts;
    return this.step(`start ${name}`, async () => {
      let mine = await this.tail({ type: RUN_STARTED, seat: this.id });
      if (mine.length < k) {
        const args = ["run", name, "--by", this.id];
        for (const [key, value] of Object.entries(inputs)) {
          const text = typeof value === "string"
            ? value
            : JSON.stringify(value);
          args.push("--input", `${key}=${text}`);
        }
        await this.fleet(args);
        mine = await this.tail({ type: RUN_STARTED, seat: this.id });
        if (mine.length < k) {
          throw new Error(
            `start: fleet run ${name} exited 0 and no ${RUN_STARTED} by ${this.id} followed on the stream`,
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
      throw new Waiting(child);
    });
  }

  /** One verb under `--json`, `--by` this run: the envelope's data, or the
   * refusal thrown. */
  private async verb<T>(args: string[]): Promise<T> {
    const full = [...args, "--by", this.id, "--json"];
    const ran = await spawnFleet(this.env, full);
    const envelope = lastDocument(ran.stdout);
    if (envelope !== null && envelope.ok === false) {
      const refusal = (envelope.refusal ?? {}) as Record<string, unknown>;
      throw new Refusal(
        String(envelope.verb),
        String(refusal.code),
        String(refusal.why),
      );
    }
    if (ran.code !== 0) throw exited(this.env, full, ran);
    if (envelope === null || envelope.ok !== true) {
      throw new Error(
        `${this.env.bin} ${
          full.join(" ")
        } printed no envelope: ${ran.stdout.trim()}`,
      );
    }
    return envelope.data as T;
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

/** The filters `fleet event tail` takes: the kind, and the actor it stores
 * under `--seat`. */
interface Filter {
  type: string;
  seat?: string;
}

/** One stored event as the tail's envelope carries it. */
interface Record_ {
  seq: number;
  kind: string;
  actor: string | null;
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
  if (filter.seat !== undefined) args.push("--seat", filter.seat);
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
      actor: typeof data.actor === "string" ? data.actor : null,
      payload: payload !== null && typeof payload === "object" ? payload : {},
    });
  }
  return records;
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
