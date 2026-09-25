// packs/tiny/workflows/takeoff.ts — a flight's middle as code. Pre-flight,
// the preboard skill with the person present, hands this run its items, its
// policy and the test command its landings run; the run spawns a builder per
// item, waits on each delivery, reviews it — every verdict a hold when the
// policy says so, and one hold licensing the whole flight when it does not —
// lands the accepted ones on the test it was handed, and ends on the report
// and the board tick as its last two steps. Every act is a numbered step over
// the SDK, so a re-run after a Waiting exit replays to where it stopped and
// spawns nothing twice.
import { type Run, workflow } from "../../ts/assets/sdk/mod.ts";

export async function takeoff(run: Run): Promise<void> {
  const items = itemsOf(await run.input("items"));
  const policy = policyOf(await run.input("policy"));
  // The two test commands: the run's own input over the fleet's setting. With
  // neither, the flight still flies — and says NOT TESTED where it cannot be
  // missed.
  const test = commandOf(await run.input("test"), run.config("takeoff.test"));
  const touched = commandOf(
    await run.input("touched"),
    run.config("takeoff.touched"),
  );
  const rows: Row[] = [];
  const decisions: Decision[] = [];

  // THE FLIGHT'S LICENCE under `review=accept`: a run lands an item only on
  // the reviewer's clearance of a hold about it, so one hold about every item,
  // asked before anything is spawned or reviewed, is what lets each accepted
  // delivery land. Anything but A ends the flight with nothing reviewed.
  if (policy.review === "accept") {
    const licence = await run.hold(
      `Land what this flight's review accepts? ${items.join(", ")}`,
      LICENCE,
      { items, licenses: "A" },
    );
    if (licence === "cancelled") {
      throw new Error("takeoff: the flight's licence hold was cancelled");
    }
    if (licence !== "A") {
      throw new Error(
        `takeoff: the person declined to license this flight's landings (answered ${licence})`,
      );
    }
  }

  // The flight, `policy.width` items in the air at once: the first wave is
  // spawned up front and every landing or return feeds the next unspawned item.
  let next = 0;
  for (; next < Math.min(policy.width, items.length); next++) {
    await run.spawn({ role: "builder", item: items[next], touched });
  }
  for (const item of items) {
    const delivery = (await run.until([item], "delivered"))[item] as Delivery;
    const commit = String(delivery.commit);
    let letter = "A";
    if (policy.review === "hold") {
      const question = `Accept ${item} at ${commit}?`;
      letter = await run.hold(question, OPTIONS, {
        items: [item],
        commit,
        licenses: "A",
      });
      decisions.push({ n: decisions.length + 1, item, question, letter });
    }
    if (letter === "A") {
      await run.review(item, "accepted");
      const landed = await run.land(item, commit, { test });
      rows.push({ item, outcome: "landed", sha: landed.sha });
    } else {
      // The findings file `fleet review --return` reads: JSON of the shape
      // core's findings schema gives, one finding naming the letter. The verb
      // numbers it, so the file numbers nothing.
      const findings = `${run.env.runDir}/${FINDINGS_DIR}/${item}.json`;
      await Deno.mkdir(`${run.env.runDir}/${FINDINGS_DIR}`, {
        recursive: true,
      });
      const option = OPTIONS.find((o) => o.startsWith(letter)) ?? letter;
      const text =
        `The person answered ${letter} at the run's hold: ${option}.`;
      await Deno.writeTextFile(
        findings,
        JSON.stringify({ findings: [{ text }] }, null, 2) + "\n",
      );
      await run.review(item, { returned: findings });
      rows.push({ item, outcome: "returned", sha: "" });
    }
    if (next < items.length) {
      await run.spawn({ role: "builder", item: items[next++], touched });
    }
  }

  await run.step("report", () => writeReport(run, rows, decisions, test));
  await run.step("tick", () => writeTick(run, rows));
}

/** The hold every verdict is read from under `review=hold`; the letter the
 * person clears it with is the verdict, and A is the letter that licenses the
 * run's landing of that item at that commit. */
export const OPTIONS = ["A. accept and land", "B. return to the builder"];

/** The one hold a `review=accept` flight raises before anything else, about
 * every item it flies: A licenses the run's landing of each delivery its
 * review accepts, and B ends the flight. */
export const LICENCE = [
  "A. land each accepted delivery",
  "B. land nothing — end the flight",
];

/** The flight report's markdown, written into the run directory. */
export const REPORT = "report.md";
/** The board tick, written into the run directory: the rows the landing hand
 * ticks on the departure board. A workflow writes the run directory and nothing
 * else, so the edit itself is the hand's, `--also` on its landing. */
export const TICK = "board-tick.md";
/** Where a returned item's findings file goes under the run directory, as
 * `<item>.json`. */
export const FINDINGS_DIR = "findings";

/** The report's first line where the flight was handed no test command: every
 * landing it made ran nothing, and says so on its own landed entry too. */
export const NOT_TESTED =
  "NOT TESTED — this flight was handed no test command, so every landing ran nothing and stands on the review alone. Set `takeoff.test` under [packs.tiny] in fleet.toml, or pass `--input test=<command>`.";

/** What the flight pins. `items` is the ids in board order, as a JSON array or
 * a comma-separated list. `policy` is `key=value` pairs, comma-separated:
 * `review` is `hold` (every verdict asked of the person, the default: one hold
 * per delivery, about that item at its commit, whose A lands it) or `accept`
 * (every delivery landed on one licence: a single hold before anything is
 * spawned, about every item, whose A licenses each landing and whose B fails
 * the flight with nothing reviewed); `width` is how many items fly at once,
 * 1 unless named. A run's landing stands on the `[core] reviewer`'s clearance
 * of a hold about the item, so the person clears either kind as that seat.
 * `test` is the command each landing runs on the rebased tree and `touched`
 * the one each builder's brief names; each is read from the run's input, else
 * from `takeoff.test` / `takeoff.touched` under [packs.tiny] in fleet.toml,
 * and the input wins. */
export interface Policy {
  review: "hold" | "accept";
  width: number;
}

/** The delivered entry `until` answers for an item, as `fleet item show
 * --json` prints it: the commit is what the flight holds, reviews and lands. */
interface Delivery {
  kind: "delivered";
  commit: string;
  branch: string;
  base: string;
}

interface Row {
  item: string;
  outcome: "landed" | "returned";
  sha: string;
}

interface Decision {
  n: number;
  item: string;
  question: string;
  letter: string;
}

export function itemsOf(pinned: unknown): string[] {
  if (pinned === null || pinned === undefined) {
    throw new Error(
      "takeoff: no `items` input — the flight has nothing to fly",
    );
  }
  const text = String(pinned).trim();
  let list: unknown = text;
  if (text.startsWith("[")) list = JSON.parse(text);
  const items = Array.isArray(list)
    ? list.map((x) => String(x).trim())
    : text.split(/[\s,]+/);
  const kept = items.filter((x) => x !== "");
  if (kept.length === 0) {
    throw new Error("takeoff: the `items` input names no item");
  }
  if (new Set(kept).size !== kept.length) {
    throw new Error(`takeoff: an item is listed twice: ${kept.join(", ")}`);
  }
  return kept;
}

/** A test command: the run's input where it names one, else the fleet's
 * setting, else undefined. A blank value names nothing, on either side. */
export function commandOf(
  input: unknown,
  setting: unknown,
): string | undefined {
  for (const value of [input, setting]) {
    if (typeof value === "string" && value.trim() !== "") return value.trim();
  }
  return undefined;
}

export function policyOf(pinned: unknown): Policy {
  const policy: Policy = { review: "hold", width: 1 };
  if (pinned === null || pinned === undefined) return policy;
  for (const pair of String(pinned).split(",")) {
    if (pair.trim() === "") continue;
    const [key, value] = pair.split("=").map((s) => s.trim());
    if (key === "review" && (value === "hold" || value === "accept")) {
      policy.review = value;
    } else if (key === "width" && /^[1-9][0-9]*$/.test(value ?? "")) {
      policy.width = Number(value);
    } else {
      throw new Error(
        `takeoff: the policy pair \`${pair.trim()}\` is not one of review=hold, review=accept, width=<n>`,
      );
    }
  }
  return policy;
}

async function writeReport(
  run: Run,
  rows: Row[],
  decisions: Decision[],
  test: string | undefined,
): Promise<{ path: string; landed: number; returned: number }> {
  const landed = rows.filter((r) => r.outcome === "landed").length;
  const returned = rows.length - landed;
  const lines = [
    // The FIRST line, above the heading, so a reader who reads one line of
    // this file reads that nothing was tested.
    ...(test === undefined ? [NOT_TESTED, ""] : []),
    `# Flight ${run.id}`,
    "",
    test === undefined
      ? "Tested: nothing — no test command reached this flight."
      : `Tested: every landing ran \`${test}\` on its rebased tree before the push.`,
    "",
    "## Decisions",
    "",
    ...(decisions.length === 0
      ? ["None: every delivery was accepted by policy."]
      : decisions.map(
        (d) =>
          `- \`${run.id}-D${d.n}\` ${d.item} — ${d.question} Answered ${d.letter}.`,
      )),
    "",
    "## The flight",
    "",
    "| Item | Outcome | Landed |",
    "|---|---|---|",
    ...rows.map((r) => `| ${r.item} | ${r.outcome} | ${r.sha || "—"} |`),
    "",
    "## Numbers",
    "",
    `- items ${rows.length}, landed ${landed}, returned ${returned}, decisions ${decisions.length}`,
    "",
  ];
  const path = `${run.env.runDir}/${REPORT}`;
  await Deno.writeTextFile(path, lines.join("\n"));
  return { path, landed, returned };
}

async function writeTick(
  run: Run,
  rows: Row[],
): Promise<{ path: string; ticked: string[] }> {
  const ticked = rows.filter((r) => r.outcome === "landed");
  const lines = [
    `# Board tick — flown ${run.id}`,
    "",
    "Tick each row below on the departure board and write the run id in the",
    "flight heading; the edit rides the landing hand's `--also`.",
    "",
    ...ticked.map((r) => `- ☑ ${r.item} ${r.sha}`),
    "",
  ];
  const path = `${run.env.runDir}/${TICK}`;
  await Deno.writeTextFile(path, lines.join("\n"));
  return { path, ticked: ticked.map((r) => r.item) };
}

// Last: a bundle hoists the consts as `var`, so the entry runs after them.
if (import.meta.main) await workflow(takeoff);
