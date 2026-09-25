// An item's record as the fake binary keeps it: the document `fleet item show
// --json` answers under `data`, one file per item under the fake's `store/`.
// An arm plants a record whole, or appends one entry to it the way a verb
// appends one to the store; a canned verb appends through the same function.
//
// The bodies below carry every field core's entry kinds carry, in the shapes
// core's `entry::to_json` prints them, so a fold the SDK makes over a planted
// record reads the fields a real record would hand it.

/** One entry as `fleet item show --json` prints it: the store's id and time,
 * who wrote it as the actor's one string `<kind>:<id>`, its kind, and the
 * kind's own fields beside them. */
export interface Entry {
  id: string;
  at: string;
  by: string;
  kind: string;
  [field: string]: unknown;
}

/** An entry's kind and its fields, before the store gives it an id, a time
 * and an author. */
export interface Body {
  kind: string;
  [field: string]: unknown;
}

/** Who writes an entry where an arm names nobody: a seat. */
export const A_SEAT = "seat:a-seat";

/** The directory the fake answers `item show` from, under its own. */
export function storeOf(fake: string): string {
  return `${fake}/store`;
}

/** One record written whole, as `item show`'s data: the id and an open status
 * unless `data` says otherwise, and no entries unless it names them. */
export async function write(
  fake: string,
  item: string,
  data: Record<string, unknown>,
): Promise<void> {
  await Deno.mkdir(storeOf(fake), { recursive: true });
  await Deno.writeTextFile(
    `${storeOf(fake)}/${item}.json`,
    JSON.stringify({ id: item, status: "open", timeline: [], ...data }),
  );
}

/** One entry appended to the item's record — made open and empty first where
 * there is none — by `by`, under an id and a time of its own. */
export async function enter(
  fake: string,
  item: string,
  body: Body,
  by: string = A_SEAT,
): Promise<Entry> {
  let data: Record<string, unknown> = {};
  try {
    data = JSON.parse(await Deno.readTextFile(`${storeOf(fake)}/${item}.json`));
  } catch {
    // no record yet: the write below plants one
  }
  const entry: Entry = {
    ...body,
    id: crypto.randomUUID(),
    at: "2026-09-24T00:00:00Z",
    by,
  };
  const timeline = Array.isArray(data.timeline) ? data.timeline : [];
  await write(fake, item, { ...data, timeline: [...timeline, entry] });
  return entry;
}

/** A dispatch's order, to whichever seat the spawner opens. */
export function ordered(): Body {
  return { kind: "ordered", order: "dispatch" };
}

/** A seat's handoff at `commit`. */
export function delivered(commit: string): Body {
  return {
    kind: "delivered",
    commit,
    branch: "a-seat/feat/the-work",
    base: "0000000",
    files: ["the-work.txt"],
    checks: [],
    suite: { not_tested: "a fixture runs no suite" },
    spec_corrections: [],
    not_proven: [{ surface: "the whole of it", command: "none" }],
    decisions: [],
    covers: [],
  };
}

/** A verdict on `commit`: an accept, or a return carrying one finding. */
export function reviewed(
  verdict: "accepted" | "returned",
  commit: string,
): Body {
  return {
    kind: "reviewed",
    verdict,
    commit,
    size: {
      files: 1,
      added: 1,
      deleted: 0,
      binary: 0,
      tests: false,
      executable: false,
      base: "0000000",
    },
    walk: [],
    ...(verdict === "returned" ? { findings: [{ text: "a finding" }] } : {}),
  };
}

/** A landing of `squash` as the trunk's new `sha`. */
export function landed(sha: string, squash: string): Body {
  return {
    kind: "landed",
    sha,
    old: "0000000",
    squash_of: squash,
    test: { not_tested: "a fixture runs no suite" },
    checks: [{ check: "AC1", verdict: "green", evidence: "a fixture" }],
    work_branch: { classification: "not_given" },
  };
}

/** A hold on a run's record: a run's own question, or the controller's stop at
 * `[core.run] max_crashes`. */
export function held(hold: string, reason: "ask" | "max_crashes"): Body {
  return {
    kind: "held",
    hold,
    reason,
    question: "Ship the report?",
    options: [{ letter: "A", text: "yes" }, { letter: "B", text: "hold" }],
    run_hash: "0",
  };
}

/** A hold's clearance: answered with a letter, or cancelled where none is
 * given. */
export function cleared(hold: string, letter?: string): Body {
  return letter === undefined
    ? { kind: "cleared", hold, how: "cancel" }
    : { kind: "cleared", hold, how: "answer", letter };
}
