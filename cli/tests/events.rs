//! `fleet event tail | show` against the shipped binary, and `fleet event
//! step`, the step pair's writer.
//!
//! Every arm runs the built `fleet` with a scratch machine directory holding a
//! stream this file writes, and reads each exit from the child's own status.
//! Nothing here starts a controller: the two readers are read-only over the
//! file.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use fleet_controller::clock;

use common::hermetic::Hermetic;

/// The fixture's first stamp. Fixed, so every stamp this file computes is the
/// one the fixture carries and no arm reads the wall clock.
const START: u64 = 1_788_600_000;

/// The fixture's length, and the count a tail prints when nothing is asked for.
const LINES: usize = 200;
const DEFAULT: usize = 50;

/// The four types the fixture cycles through, so every `--type` has a count the
/// arms compute from the fixture rather than type.
const TYPES: [&str; 4] = [
    "seat.woke",
    "seat.resting",
    "seat.handed_off",
    "seat.exited",
];

/// The two actors it alternates between. Neither is a seat this fleet runs:
/// what the filter matches is the stored string.
const SEATS: [&str; 2] = ["builder-a", "builder-b"];

struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Rig {
        let root = std::env::temp_dir().join(format!("fleet-events-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig { root };
        std::fs::create_dir_all(rig.machine()).unwrap();
        rig
    }

    fn machine(&self) -> PathBuf {
        self.root.join("machine")
    }

    fn stream(&self) -> PathBuf {
        self.machine().join("events.jsonl")
    }

    fn write_stream(&self, lines: &[String]) {
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(self.stream(), body).unwrap();
    }

    /// Append raw bytes, which is what a writer at the other end of the stream
    /// does — and what a half-written line is made of.
    fn append(&self, bytes: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.stream())
            .unwrap();
        file.write_all(bytes.as_bytes()).unwrap();
        file.flush().unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fleet"));
        command
            .args(args)
            .hermetic(&self.root.join("home"), &self.machine(), None);
        command
    }

    fn fleet(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the built binary runs")
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> Option<i32> {
    output.status.code()
}

/// One stored line, in the shape and the key order the writer's own envelope
/// has. The id is `event_id`'s: hex nanoseconds, a hyphen, then hex sequence.
fn line(seq: usize) -> String {
    format!(
        "{{\"id\":\"{}\",\"seq\":{seq},\"ts\":\"{}\",\"type\":\"{}\",\"actor\":\"{}\",\"payload\":{{}}}}",
        id(seq),
        stamp(seq),
        TYPES[(seq - 1) % TYPES.len()],
        SEATS[(seq - 1) % SEATS.len()],
    )
}

fn id(seq: usize) -> String {
    format!(
        "{:032x}-{:08x}",
        1_788_600_000_000_000_000u128 + seq as u128,
        seq
    )
}

/// The stamp of line `seq`: one second per line from a fixed start.
fn stamp(seq: usize) -> String {
    clock::stamp_secs(START + seq as u64 - 1)
}

/// Two hundred lines, sequenced 1 through 200.
fn fixture() -> Vec<String> {
    (1..=LINES).map(line).collect()
}

/// The lines of the fixture the arm expects back, as stdout would carry them.
fn expected(seqs: &[usize]) -> String {
    let mut body = seqs
        .iter()
        .map(|seq| line(*seq))
        .collect::<Vec<String>>()
        .join("\n");
    body.push('\n');
    body
}

/// AC1 — the snapshot: the last fifty by sequence, byte-identical to the file's
/// own lines, and the cursor `--since` moves.
#[test]
fn the_tail_answers_the_last_fifty_lines_as_stored_and_the_cursor_moves_them() {
    let rig = Rig::new("tail");
    rig.write_stream(&fixture());

    let all = rig.fleet(&["event", "tail"]);
    assert_eq!(code(&all), Some(0), "{}", err(&all));
    let last_fifty: Vec<usize> = (LINES - DEFAULT + 1..=LINES).collect();
    assert_eq!(
        out(&all),
        expected(&last_fifty),
        "the default is the last fifty, as stored"
    );
    assert_eq!(out(&all).lines().count(), DEFAULT);
    assert_eq!(
        err(&all),
        "",
        "stdout carries the stream and stderr nothing"
    );

    // The same fifty, named by their cursor.
    let since_150 = rig.fleet(&["event", "tail", "--since", "150"]);
    assert_eq!(code(&since_150), Some(0));
    assert_eq!(out(&since_150), expected(&last_fifty));

    // The cursor is exclusive: above 199 is the last line alone.
    let since_199 = rig.fleet(&["event", "tail", "--since", "199"]);
    assert_eq!(code(&since_199), Some(0));
    assert_eq!(out(&since_199), expected(&[200]));

    // Past the end is nothing at all, and it is not a refusal.
    let since_200 = rig.fleet(&["event", "tail", "--since", "200"]);
    assert_eq!(code(&since_200), Some(0), "{}", err(&since_200));
    assert_eq!(out(&since_200), "");
    let since_far = rig.fleet(&["event", "tail", "--since", "9999"]);
    assert_eq!(code(&since_far), Some(0));
    assert_eq!(out(&since_far), "");
}

/// AC1 — the filters, and the ORDER they run in: filter first, then the last
/// fifty that survive.
///
/// The fixture is built so the two orders differ. `builder-a` says a hundred of
/// the two hundred lines, so filtering first answers fifty of that seat's own
/// lines and taking fifty first would answer twenty-five; a type says fifty, so
/// filtering first answers all fifty and taking fifty first would answer twelve
/// or thirteen. Each count is computed from the fixture here.
#[test]
fn the_filters_run_before_the_last_fifty_are_taken_and_combine_as_an_and() {
    let rig = Rig::new("filters");
    rig.write_stream(&fixture());

    let seat_lines: Vec<usize> = (1..=LINES)
        .filter(|seq| SEATS[(seq - 1) % 2] == "builder-a")
        .collect();
    assert_eq!(
        seat_lines.len(),
        100,
        "the fixture the assertion is read off"
    );
    let wanted: Vec<usize> = seat_lines[seat_lines.len() - DEFAULT..].to_vec();
    let by_seat = rig.fleet(&["event", "tail", "--seat", "builder-a"]);
    assert_eq!(code(&by_seat), Some(0), "{}", err(&by_seat));
    assert_eq!(
        out(&by_seat),
        expected(&wanted),
        "filtering after the take would answer twenty-five of these"
    );
    assert_eq!(out(&by_seat).lines().count(), DEFAULT);

    // A type: fifty lines in the fixture, so filtering first answers all fifty.
    let typed: Vec<usize> = (1..=LINES)
        .filter(|seq| TYPES[(seq - 1) % 4] == TYPES[1])
        .collect();
    assert_eq!(typed.len(), 50);
    let by_type = rig.fleet(&["event", "tail", "--type", TYPES[1]]);
    assert_eq!(code(&by_type), Some(0), "{}", err(&by_type));
    assert_eq!(out(&by_type), expected(&typed));

    // The two together are an AND. `seat.woke` falls on every fourth line from
    // the first, which is odd, so `builder-a` says all fifty of them and
    // `builder-b` says none — which is the reading a filter that ORed would
    // fail on both halves.
    let woke: Vec<usize> = (1..=LINES)
        .filter(|seq| TYPES[(seq - 1) % 4] == TYPES[0])
        .collect();
    let both = rig.fleet(&["event", "tail", "--seat", "builder-a", "--type", TYPES[0]]);
    assert_eq!(code(&both), Some(0), "{}", err(&both));
    assert_eq!(out(&both), expected(&woke));
    let neither = rig.fleet(&["event", "tail", "--seat", "builder-b", "--type", TYPES[0]]);
    assert_eq!(code(&neither), Some(0), "{}", err(&neither));
    assert_eq!(
        out(&neither),
        "",
        "no line is both, so the AND answers none"
    );
}

/// AC2 — a stamp given to `--since`: the sequence it resolves to is inclusive,
/// printed once on stderr so a script can pass it back, and stdout carries only
/// the stream.
#[test]
fn a_stamp_resolves_to_a_sequence_and_says_which_one_on_stderr() {
    let rig = Rig::new("stamps");
    rig.write_stream(&fixture());

    let at = rig.fleet(&["event", "tail", "--since", &stamp(120)]);
    assert_eq!(code(&at), Some(0), "{}", err(&at));
    let from_120: Vec<usize> = (120..=LINES).collect();
    assert_eq!(
        out(&at),
        expected(&from_120),
        "the event AT the stamp is one the caller asked for"
    );
    assert!(
        err(&at).contains("resolved to 120"),
        "the resolved sequence is on stderr: {}",
        err(&at)
    );

    // Past the end: nothing on stdout, `none` on stderr, and exit 0.
    let past = rig.fleet(&[
        "event",
        "tail",
        "--since",
        &clock::stamp_secs(START + 100_000),
    ]);
    assert_eq!(code(&past), Some(0), "{}", err(&past));
    assert_eq!(out(&past), "");
    assert!(err(&past).contains("resolved to none"), "{}", err(&past));

    // A value that is neither a sequence nor a stamp is a usage error, and the
    // line names the shape.
    for bad in ["yesterday", "2026-09-08", "2026-09-08T00:00:00"] {
        let refused = rig.fleet(&["event", "tail", "--since", bad]);
        assert_eq!(code(&refused), Some(2), "{bad}: {}", err(&refused));
        assert_eq!(out(&refused), "");
        assert!(
            err(&refused).contains("YYYY-MM-DDTHH:MM:SSZ"),
            "{bad}: {}",
            err(&refused)
        );
    }

    // A stamp BETWEEN two lines resolves to the later one. It takes a stream
    // with a gap in it: the fixture above is one second per line, where no
    // stamp falls between two of them.
    let gapped = Rig::new("gap");
    gapped.write_stream(&[
        format!(
            "{{\"id\":\"a\",\"seq\":1,\"ts\":\"{}\",\"type\":\"{}\",\"actor\":\"builder-a\"}}",
            clock::stamp_secs(START),
            TYPES[0]
        ),
        format!(
            "{{\"id\":\"b\",\"seq\":2,\"ts\":\"{}\",\"type\":\"{}\",\"actor\":\"builder-a\"}}",
            clock::stamp_secs(START + 10),
            TYPES[0]
        ),
    ]);
    let between = gapped.fleet(&["event", "tail", "--since", &clock::stamp_secs(START + 5)]);
    assert_eq!(code(&between), Some(0), "{}", err(&between));
    assert!(err(&between).contains("resolved to 2"), "{}", err(&between));
    assert_eq!(out(&between).lines().count(), 1);
    assert!(out(&between).contains("\"seq\":2"), "{}", out(&between));
}

/// AC3 — the follower: a line appears once its newline lands, an unterminated
/// tail is held rather than skipped, and an interrupt ends the follow with 0.
///
/// The first append is a READINESS probe and carries no bound worth reading: it
/// is waited for patiently, because until it arrives the child may still be
/// starting and a one-second window would be measuring this box's load. Every
/// assertion after it is the follower's own.
#[test]
fn a_follow_prints_a_line_when_its_newline_lands_and_ends_on_an_interrupt() {
    let rig = Rig::new("follow");
    rig.write_stream(&fixture());

    let mut child = rig
        .command(&["event", "tail", "--follow", "--since", "200"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built binary runs");
    let lines = reader(&mut child);

    rig.append(&format!("{}\n", line(201)));
    assert_eq!(
        lines.recv_timeout(Duration::from_secs(20)).ok(),
        Some(line(201)),
        "the follower never reached its loop"
    );

    // The bound the arm is named for: a complete line, inside one second.
    rig.append(&format!("{}\n", line(202)));
    assert_eq!(
        lines.recv_timeout(Duration::from_secs(1)).ok(),
        Some(line(202))
    );

    // Half a line, and nothing may be printed for it — the lower bound is a
    // whole second of nothing, which a follower that read the torn bytes would
    // spend printing them or losing them.
    let whole = line(203);
    let (half, rest) = whole.split_at(whole.len() / 2);
    rig.append(half);
    assert_eq!(
        lines.recv_timeout(Duration::from_secs(1)).err(),
        Some(RecvTimeoutError::Timeout),
        "the unterminated tail was read as a line"
    );

    // The writer finishes it, and the whole line appears.
    rig.append(&format!("{rest}\n"));
    assert_eq!(
        lines.recv_timeout(Duration::from_secs(1)).ok(),
        Some(whole),
        "the held line was lost when its newline landed"
    );

    // SIGINT ends the follow, and 0 is the exit: an interrupted follow is a
    // reader that finished, not one that failed.
    let killed = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success(), "the signal was not delivered");
    let ended = child.wait().expect("the follower is reaped");
    assert_eq!(ended.code(), Some(0), "an interrupted follow exits 0");
}

/// The child's stdout, one line per message, off a thread so the arm can put a
/// bound on how long it waits.
fn reader(child: &mut Child) -> Receiver<String> {
    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    rx
}

/// AC4 — `show`: one event pretty-printed, an absent id named, and a duplicate
/// id read as the corrupted stream it is.
#[test]
fn show_prints_the_one_event_and_names_an_absent_or_duplicated_id() {
    let rig = Rig::new("show");
    rig.write_stream(&fixture());

    let one = rig.fleet(&["event", "show", &id(42)]);
    assert_eq!(code(&one), Some(0), "{}", err(&one));
    let printed: serde_json::Value = serde_json::from_str(&out(&one)).expect("stdout is one event");
    let stored: serde_json::Value = serde_json::from_str(&line(42)).unwrap();
    assert_eq!(printed, stored, "the event is the fixture's own line");
    assert!(
        out(&one).contains("\n  \"seq\": 42"),
        "and it is pretty-printed: {}",
        out(&one)
    );

    let absent = rig.fleet(&["event", "show", "no-such-id"]);
    assert_eq!(code(&absent), Some(1), "{}", err(&absent));
    assert_eq!(out(&absent), "");
    assert!(err(&absent).contains("no-such-id"), "{}", err(&absent));

    // Two lines carrying one id: a corrupted stream, refused in the usage row
    // with both sequences named, rather than one of the two picked.
    let corrupt = Rig::new("show-corrupt");
    let mut lines = fixture();
    lines[8] = lines[8].replace(&id(9), &id(7));
    corrupt.write_stream(&lines);
    let twice = corrupt.fleet(&["event", "show", &id(7)]);
    assert_eq!(code(&twice), Some(2), "{}", err(&twice));
    assert_eq!(
        out(&twice),
        "",
        "no event is printed for a stream like that"
    );
    let said = err(&twice);
    assert!(said.contains(&id(7)), "{said}");
    assert!(
        said.contains("7, 9"),
        "both sequences the id is on are named: {said}"
    );
    assert!(
        said.lines().count() == 1,
        "one line names both sequences: {said}"
    );
}

/// AC5 — no stream at the resolved path: both readers refuse with exit 5, the
/// no-collector row, naming the path a caller would have to look at.
#[test]
fn a_missing_stream_refuses_both_readers_and_names_the_path() {
    let rig = Rig::new("no-stream");
    assert!(!rig.stream().exists(), "the rig writes no stream");
    let path = rig.stream().display().to_string();

    for args in [
        vec!["event", "tail"],
        vec!["event", "tail", "--follow"],
        vec!["event", "show", "any-id"],
    ] {
        let refused = rig.fleet(&args);
        assert_eq!(code(&refused), Some(5), "{args:?}: {}", err(&refused));
        assert_eq!(out(&refused), "");
        assert!(err(&refused).contains(&path), "{args:?}: {}", err(&refused));
        assert_eq!(err(&refused).lines().count(), 1, "{}", err(&refused));
    }

    // The control: with a stream at that path the same call answers.
    rig.write_stream(&fixture());
    let answered = rig.fleet(&["event", "tail", "--since", "199"]);
    assert_eq!(code(&answered), Some(0), "{}", err(&answered));
    assert_eq!(answered.stdout.len(), line(200).len() + 1);
}

/// AC6 — `--json`: the two readers the SDK replays from print the envelope, one
/// document per event for a tail and one for a show, each carrying the record
/// whole where the flagless tail carries the file's bytes.
#[test]
fn the_json_readers_print_one_envelope_per_event_carrying_the_record_whole() {
    let rig = Rig::new("json-read");
    let three: Vec<String> = (1..=3).map(line).collect();
    rig.write_stream(&three);

    let tailed = rig.fleet(&["event", "tail", "--json"]);
    assert_eq!(code(&tailed), Some(0), "{}", err(&tailed));
    let printed = out(&tailed);
    let documents: Vec<&str> = printed.lines().collect();
    assert_eq!(documents.len(), 3, "one line per record: {printed}");
    for (index, document) in documents.iter().enumerate() {
        let seq = index + 1;
        let parsed: serde_json::Value = serde_json::from_str(document)
            .unwrap_or_else(|e| panic!("line {seq}: {e}: {document}"));
        let stored: serde_json::Value = serde_json::from_str(&line(seq)).unwrap();
        assert_eq!(parsed["ok"], serde_json::Value::Bool(true), "{document}");
        assert_eq!(parsed["verb"], "event tail", "{document}");
        assert_eq!(parsed["data"]["seq"], seq as u64, "{document}");
        // Every field the record carries, and the id and the payload are the
        // two a `RawLine` does not: they are what the document reads the stored
        // line again for.
        assert_eq!(parsed["data"]["id"], stored["id"], "{document}");
        assert_eq!(parsed["data"]["ts"], stored["ts"], "{document}");
        assert_eq!(parsed["data"]["kind"], stored["type"], "{document}");
        assert_eq!(parsed["data"]["actor"], stored["actor"], "{document}");
        assert_eq!(parsed["data"]["payload"], stored["payload"], "{document}");
    }

    // The control the assertion above needs: without the flag the same three
    // lines come back as the file's own bytes, so the flag is what wraps them.
    let plain = rig.fleet(&["event", "tail"]);
    assert_eq!(code(&plain), Some(0), "{}", err(&plain));
    assert_eq!(out(&plain), expected(&[1, 2, 3]));

    let shown = rig.fleet(&["event", "show", &id(2), "--json"]);
    assert_eq!(code(&shown), Some(0), "{}", err(&shown));
    assert_eq!(
        out(&shown).lines().count(),
        1,
        "one document: {}",
        out(&shown)
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&out(&shown)).expect("stdout is one document");
    let stored: serde_json::Value = serde_json::from_str(&line(2)).unwrap();
    assert_eq!(parsed["ok"], serde_json::Value::Bool(true));
    assert_eq!(parsed["verb"], "event show");
    assert_eq!(parsed["data"]["seq"], 2u64);
    assert_eq!(parsed["data"]["id"], stored["id"]);
    assert_eq!(parsed["data"]["payload"], stored["payload"]);
}

/// AC6 — the refusal half: an id the stream does not carry, and a stream that
/// is not there, each answered as the envelope's refusal document on stdout
/// with the exit table's own row on `$?` — the flag changes the document and
/// never the number.
#[test]
fn the_json_readers_refuse_in_the_envelopes_refusal_shape() {
    let rig = Rig::new("json-refuse");
    let three: Vec<String> = (1..=3).map(line).collect();
    rig.write_stream(&three);

    let absent = rig.fleet(&["event", "show", "no-such-id", "--json"]);
    assert_eq!(code(&absent), Some(1), "{}", err(&absent));
    let parsed: serde_json::Value =
        serde_json::from_str(&out(&absent)).unwrap_or_else(|e| panic!("{e}: {}", out(&absent)));
    assert_eq!(parsed["ok"], serde_json::Value::Bool(false));
    assert_eq!(parsed["verb"], "event show");
    assert_eq!(parsed["refusal"]["code"], "refused");
    assert!(
        parsed["refusal"]["why"]
            .as_str()
            .expect("why is a string")
            .contains("no-such-id"),
        "the why names the id: {}",
        out(&absent)
    );
    // The person's line is printed under the flag too, so a run nobody is
    // parsing still says what happened.
    assert!(err(&absent).contains("no-such-id"), "{}", err(&absent));

    // The other reader's refusal, from the other row of the table: no stream at
    // the resolved path is exit 5 and the class `no_collector`.
    let empty = Rig::new("json-refuse-empty");
    assert!(!empty.stream().exists(), "the rig writes no stream");
    let none = empty.fleet(&["event", "tail", "--json"]);
    assert_eq!(code(&none), Some(5), "{}", err(&none));
    let parsed: serde_json::Value =
        serde_json::from_str(&out(&none)).unwrap_or_else(|e| panic!("{e}: {}", out(&none)));
    assert_eq!(parsed["ok"], serde_json::Value::Bool(false));
    assert_eq!(parsed["verb"], "event tail");
    assert_eq!(parsed["refusal"]["code"], "no_collector");
    assert!(
        parsed["refusal"]["why"]
            .as_str()
            .expect("why is a string")
            .contains(&empty.stream().display().to_string()),
        "the why names the path: {}",
        out(&none)
    );

    // The control: the flagless calls print nothing at all on stdout, so the
    // document above is the flag's and not something both forms said.
    let plain_absent = rig.fleet(&["event", "show", "no-such-id"]);
    assert_eq!(code(&plain_absent), Some(1), "{}", err(&plain_absent));
    assert_eq!(out(&plain_absent), "");
    let plain_none = empty.fleet(&["event", "tail"]);
    assert_eq!(code(&plain_none), Some(5), "{}", err(&plain_none));
    assert_eq!(out(&plain_none), "");
}

/// `fleet event step <started|closed>`: the one writer of the step pair, for a
/// workflow's own process. Both halves land on the stream with the run, the
/// number and the name; a close carries its result as one JSON value or the
/// sha of the result file; the actor is the run, typed `run:<id>`, where no
/// `FLEET_ACTOR` is set and the typed actor it names where one is; and the
/// SDK's own read — `event tail --json --type step.closed` — gets
/// every close back and nothing else.
#[test]
fn the_step_writer_appends_both_halves_on_the_run_and_the_json_tail_reads_the_closes_back() {
    let rig = Rig::new("step-writer");
    let run = "fleet-run-7";
    let stream = |seq: usize| -> serde_json::Value {
        let body = std::fs::read_to_string(rig.stream()).expect("the stream is written");
        let line = body
            .lines()
            .nth(seq - 1)
            .unwrap_or_else(|| panic!("line {seq}: {body}"));
        serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))
    };

    // The fallback actor: the run, typed, because the child a run starts
    // carries no actor variable. It is removed rather than assumed absent,
    // since the suite's own environment may set it.
    let started = rig
        .command(&[
            "event", "step", "started", "--run", run, "--n", "1", "--name", "fetch",
        ])
        .env_remove("FLEET_ACTOR")
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&started), Some(0), "{}", err(&started));
    assert!(
        out(&started).starts_with("step.started fleet-run-7 n=1 fetch — seq 1"),
        "{}",
        out(&started)
    );
    let first = stream(1);
    assert_eq!(first["type"], "step.started");
    assert_eq!(
        first["actor"],
        format!("run:{run}"),
        "the run is the actor: {first}"
    );
    assert_eq!(first["payload"]["run"], run);
    assert_eq!(first["payload"]["n"], 1);
    assert_eq!(first["payload"]["name"], "fetch");
    assert!(
        first["payload"].get("result").is_none(),
        "a start carries no result: {first}"
    );

    // The close with an inline result, and an actor the environment names.
    let closed = rig
        .command(&[
            "event",
            "step",
            "closed",
            "--run",
            run,
            "--n",
            "1",
            "--name",
            "fetch",
            "--result",
            "{\"rows\":[1,2],\"ok\":true}",
        ])
        .env("FLEET_ACTOR", "routine:x")
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&closed), Some(0), "{}", err(&closed));
    let second = stream(2);
    assert_eq!(second["type"], "step.closed");
    assert_eq!(second["actor"], "routine:x");
    assert_eq!(second["payload"]["n"], 1);
    assert_eq!(second["payload"]["name"], "fetch");
    assert_eq!(
        second["payload"]["result"]["rows"],
        serde_json::json!([1, 2])
    );
    assert_eq!(second["payload"]["result"]["ok"], true);

    // The close over the cap: the sha and no result.
    let sha = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    let hashed = rig.fleet(&[
        "event", "step", "closed", "--run", run, "--n", "2", "--name", "big", "--sha", sha,
    ]);
    assert_eq!(code(&hashed), Some(0), "{}", err(&hashed));
    let third = stream(3);
    assert_eq!(third["payload"]["sha"], sha);
    assert!(
        third["payload"].get("result").is_none(),
        "a hashed close carries no inline result: {third}"
    );

    // A number lands as the literal the workflow printed: the parser is the
    // correctly rounded one, so the double a re-run reads back is the double the
    // first run saw. This literal is one the default parser lands an ulp off.
    let literal = "0.9540002341648193";
    let number = rig.fleet(&[
        "event", "step", "closed", "--run", run, "--n", "3", "--name", "random", "--result",
        literal,
    ]);
    assert_eq!(code(&number), Some(0), "{}", err(&number));
    let stored = std::fs::read_to_string(rig.stream()).unwrap();
    let fourth = stored.lines().nth(3).expect("the fourth line");
    assert!(
        fourth.contains(&format!("\"result\":{literal}")),
        "the literal is stored as printed: {fourth}"
    );

    // The SDK's read: every close and only the closes, as envelopes.
    let tailed = rig.fleet(&[
        "event",
        "tail",
        "--json",
        "--since",
        "0",
        "--type",
        "step.closed",
    ]);
    assert_eq!(code(&tailed), Some(0), "{}", err(&tailed));
    let documents: Vec<serde_json::Value> = out(&tailed)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect();
    assert_eq!(
        documents.len(),
        3,
        "three closes, no start: {}",
        out(&tailed)
    );
    assert_eq!(documents[0]["data"]["payload"]["n"], 1);
    assert_eq!(documents[0]["data"]["payload"]["result"]["ok"], true);
    assert_eq!(documents[1]["data"]["payload"]["n"], 2);
    assert_eq!(documents[1]["data"]["payload"]["sha"], sha);
    assert!(
        out(&tailed)
            .lines()
            .nth(2)
            .unwrap()
            .contains(&format!("\"result\":{literal}")),
        "the envelope carries the literal too: {}",
        out(&tailed)
    );

    // The three usage refusals, each exit 2 and none of them a line.
    let lines_before = std::fs::read_to_string(rig.stream())
        .unwrap()
        .lines()
        .count();
    for args in [
        vec![
            "event", "step", "started", "--run", run, "--n", "3", "--name", "x", "--result", "1",
        ],
        vec![
            "event", "step", "closed", "--run", run, "--n", "3", "--name", "x",
        ],
        vec![
            "event", "step", "closed", "--run", run, "--n", "3", "--name", "x", "--result", "nope",
        ],
    ] {
        let refused = rig.fleet(&args);
        assert_eq!(code(&refused), Some(2), "{args:?}: {}", err(&refused));
        assert!(
            err(&refused).contains("fleet event step:"),
            "{args:?}: {}",
            err(&refused)
        );
    }
    // An actor variable that is not `<kind>:<id>` is the fourth: a bare name
    // is no actor a step can be written under.
    let bare = rig
        .command(&[
            "event", "step", "started", "--run", run, "--n", "4", "--name", "x",
        ])
        .env("FLEET_ACTOR", "the-workflow")
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&bare), Some(2), "{}", err(&bare));
    assert!(
        err(&bare).contains("fleet event step: FLEET_ACTOR the-workflow is not kind:id"),
        "{}",
        err(&bare)
    );
    assert_eq!(
        std::fs::read_to_string(rig.stream())
            .unwrap()
            .lines()
            .count(),
        lines_before,
        "a refused step writes nothing"
    );
}
