//! `fleet-store-stub`, the board held in memory answering the store contract
//! as an adapter executable: the exits it answers, the schema every answer
//! passes, its lock under calls at once, and the two knobs a suite turns on
//! it — a deaf call and a slow one.
//!
//! The contract's checks themselves are asked of it in `contract.rs`, beside
//! the other two stores; what is here is the stub's own.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Barrier;
use std::time::Duration;

use common::Fixture;
use fleet_core::store::conformance::{self, Ctx, Passed, CHECKS};
use fleet_core::store::exec::Exec;
use fleet_core::store::types;
use fleet_core::store::{schema, Filter, ItemId, NewItem, Store, StoreError, Update};
use fleet_core::test_support::stub::{self as stubbed, STATE_FILE};
use fleet_core::test_support::{stub_path, the_test, FakeStore};
use serde_json::{json, Value};

/// A root with the stub's empty store made in it, through `Exec` as a caller
/// asks for one.
struct Stubbed {
    dir: Fixture,
}

impl Stubbed {
    fn new(label: &str) -> Stubbed {
        let dir = Fixture::new(&format!("stub-{label}"));
        let root = Exec::at(&stub_path(), &dir.root)
            .scratch(&dir.root)
            .unwrap_or_else(|e| panic!("the stub makes a store in {}: {e}", dir.root.display()));
        assert_eq!(root, dir.root, "the root is the directory it was handed");
        Stubbed { dir }
    }

    fn root(&self) -> &Path {
        &self.dir.root
    }

    fn exec(&self) -> Exec {
        Exec::at(&stub_path(), self.root())
    }

    /// The store over an adapter that runs the stub with `env` set, written
    /// as a `#!/bin/sh` wrapper beside the store: the knob is the one call's,
    /// and no other arm's.
    fn with_env(&self, name: &str, env: &str) -> Exec {
        let bin = self.dir.path(name);
        std::fs::write(
            &bin,
            format!("#!/bin/sh\n{env} exec '{}' \"$@\"\n", stub_path().display()),
        )
        .expect("the wrapper is written");
        executable(&bin);
        Exec::at(&bin, self.root())
    }

    /// One call of the stub run by hand: its exit, its stdout and its stderr.
    fn call(&self, verb: &str, fields: Value) -> (Option<i32>, String, String) {
        let Value::Object(fields) = fields else {
            panic!("a request's fields are an object");
        };
        let request = types::request(fields, self.root()).to_string();
        raw(verb, &request)
    }
}

/// The stub run on `request` as it stands, envelope and all.
fn raw(verb: &str, request: &str) -> (Option<i32>, String, String) {
    let mut child = Command::new(stub_path())
        .arg(verb)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the stub runs");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(request.as_bytes())
        .expect("the request is written");
    let out = child.wait_with_output().expect("the stub answers");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn executable(bin: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin, std::fs::Permissions::from_mode(0o755))
        .expect("the file is executable");
}

fn a_task(title: &str) -> NewItem {
    NewItem {
        title: title.to_string(),
        description: String::from("an item the stub's arms filed"),
        item_type: String::from("task"),
        labels: vec![String::from("stub")],
        priority: None,
    }
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("`{text}` is JSON: {e}"))
}

/// Whether `answer` passes the schema at `pointer` in the contract's document.
fn passes(pointer: &str, answer: &Value) {
    let document = schema::document();
    if let Err(why) = schema::check(&document, pointer, answer) {
        panic!("{answer} does not pass {pointer}: {why}");
    }
}

/// cargo's own path to the stub is the one the library answers for this
/// crate's tests.
#[test]
fn stub_path_is_the_executable_cargo_built() {
    assert_eq!(
        stub_path().canonicalize().expect("the stub is there"),
        Path::new(env!("CARGO_BIN_EXE_fleet-store-stub"))
            .canonicalize()
            .expect("cargo built the stub"),
    );
}

/// The exit table, row by row: a refusal by its reason, with the ids an
/// ambiguity names; usage for an unknown verb, a request that does not decode
/// and a change the contract calls malformed, with nothing on stdout; could
/// not tell at a root holding no store. Every answer passes the document's
/// schema for its row.
#[test]
fn each_row_of_the_exit_table_is_answered_with_its_body() {
    let stub = Stubbed::new("exits");
    let exec = stub.exec();
    let by = the_test().to_string();
    let item = exec
        .create(&a_task("an item the exits name"), &the_test())
        .expect("the item is filed");

    let refused = |verb: &str, fields: Value, reason: &str| {
        let (code, stdout, stderr) = stub.call(verb, fields);
        assert_eq!(code, Some(1), "{verb}: {stdout}{stderr}");
        assert_eq!(stderr, "", "{verb}: a refusal says nothing on stderr");
        let answer = json_of(&stdout);
        passes("/refusal", &answer);
        assert_eq!(answer["refused"]["reason"], reason, "{verb}: {answer}");
        answer
    };
    refused("show", json!({"id": "nothing-by-this-id"}), "missing");
    refused(
        "update",
        json!({"id": "fx-zzz", "by": by, "title": "t"}),
        "missing",
    );
    let seat = fleet_core::seat::identity::SeatId::mint();
    let answer = refused(
        "update",
        json!({"id": item, "by": by, "title": "t", "if_assignee": seat.to_string()}),
        "moved",
    );
    assert!(
        answer["refused"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(item.as_str())),
        "a moved names the item: {answer}"
    );
    exec.close(&item, "done", &the_test())
        .expect("the item closes");
    refused(
        "close",
        json!({"id": item, "by": by, "reason": "again"}),
        "already",
    );

    // Two items whose ids open with one character, for text naming both.
    let mut filed = vec![item.clone()];
    let (one, two) = loop {
        let next = exec
            .create(&a_task("an item an ambiguity names"), &the_test())
            .expect("the item is filed");
        let opens = |id: &ItemId| id.as_str().chars().nth(3);
        if let Some(twin) = filed.iter().find(|held| opens(held) == opens(&next)) {
            break (twin.clone(), next);
        }
        filed.push(next);
    };
    let fragment = &one.as_str()[3..4];
    let answer = refused("resolve", json!({ "id": fragment }), "ambiguous");
    let candidates: Vec<&str> = answer["refused"]["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        candidates.contains(&one.as_str()) && candidates.contains(&two.as_str()),
        "the ambiguity names both: {answer}"
    );

    let usage = |verb: &str, request: String, says: &str| {
        let (code, stdout, stderr) = raw(verb, &request);
        assert_eq!(code, Some(2), "{verb}: {stdout}{stderr}");
        assert_eq!(stdout, "", "{verb}: usage prints nothing on stdout");
        assert!(stderr.contains(says), "{verb}: {stderr}");
    };
    let root = stub.root().display().to_string();
    let request = |fields: Value| {
        let Value::Object(fields) = fields else {
            unreachable!("an object")
        };
        types::request(fields, stub.root()).to_string()
    };
    usage(
        "rename",
        request(json!({})),
        "`rename` is no verb of the store contract",
    );
    usage("show", String::from("not json"), "not one JSON object");
    usage(
        "show",
        json!({"schema_version": 2, "root": root, "id": "x"}).to_string(),
        "schema_version is 2",
    );
    usage("show", request(json!({})), "carries no `id`");
    usage(
        "create",
        request(json!({"item": {"title": "t"}, "by": by})),
        "`item` does not read",
    );
    usage(
        "update",
        request(json!({"id": item, "by": by})),
        "an update names no title, assignee or status",
    );
    usage(
        "update",
        request(json!({"id": item, "by": by, "status": "closed"})),
        "sets a status only to open",
    );
    usage(
        "append",
        request(json!({"id": item, "by": by, "entry": {"kind": "ordered"}})),
        "carries no fleet.entry",
    );

    let nowhere = Fixture::new("stub-nowhere");
    let (code, stdout, _) = raw(
        "show",
        &types::request(
            json!({"id": "x"}).as_object().cloned().unwrap_or_default(),
            &nowhere.root,
        )
        .to_string(),
    );
    assert_eq!(code, Some(3), "{stdout}");
    let answer = json_of(&stdout);
    passes("/error", &answer);
    assert!(
        answer["error"]
            .as_str()
            .is_some_and(|error| error.contains("no store at")),
        "{answer}"
    );
    assert!(
        !nowhere.root.join(".store").exists(),
        "a read at a root holding no store makes nothing there"
    );
}

/// EVERY ANSWER THE STUB GIVES PASSES ITS VERB'S SCHEMA: the whole conformance
/// table run through an adapter that records each call's verb, exit and
/// stdout before handing them on, and each answer then held to the
/// document — a response to `/verbs/<verb>/response` and decoded through
/// [`types::answer`], a refusal to `/refusal`, an error to `/error` — with
/// every verb of the document answered at least once.
#[test]
fn every_answer_the_stub_gives_passes_its_verbs_schema() {
    let stub = Stubbed::new("schema");
    let log = stub.dir.path("answers.log");
    let recorder = stub.dir.path("recorder");
    std::fs::write(
        &recorder,
        format!(
            "#!/bin/sh\n\
             out=$('{stub}' \"$@\")\n\
             code=$?\n\
             printf '%s\\t%s\\t%s\\n' \"$1\" \"$code\" \"$out\" >> '{log}'\n\
             printf '%s\\n' \"$out\"\n\
             exit $code\n",
            stub = stub_path().display(),
            log = log.display(),
        ),
    )
    .expect("the recorder is written");
    executable(&recorder);
    let nowhere = Fixture::new("stub-schema-absent");
    let store = Exec::at(&recorder, stub.root());
    let absent = Exec::at(&recorder, &nowhere.root);
    let ctx = Ctx {
        store: &store,
        root: stub.root(),
        absent: &absent,
        another_writer: None,
    };
    let failed: Vec<String> = conformance::run(&ctx)
        .filter_map(|(name, answer)| match answer {
            Ok(Passed::Pass | Passed::Skip(_)) => None,
            Err(why) => Some(format!("{name}: {why}")),
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    // The scratch this store was made by is answered too.
    Exec::at(&recorder, stub.root())
        .scratch(&stub.dir.path("again"))
        .expect("a second scratch is made");

    let document = schema::document();
    let mut answered: Vec<String> = Vec::new();
    let text = std::fs::read_to_string(&log).expect("the recorder logged its calls");
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (verb, code, stdout) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        let answer = json_of(stdout);
        let pointer = match code {
            "0" => {
                types::answer::<Value>(stdout)
                    .unwrap_or_else(|why| panic!("{verb}'s answer does not decode: {why}"));
                answered.push(verb.to_string());
                format!("/verbs/{verb}/response")
            }
            "1" => String::from("/refusal"),
            "3" => String::from("/error"),
            other => panic!("{verb} exited {other} under the conformance run: {stdout}"),
        };
        if let Err(why) = schema::check(&document, &pointer, &answer) {
            panic!("{verb}'s answer {answer} does not pass {pointer}: {why}");
        }
    }
    answered.sort_unstable();
    answered.dedup();
    let verbs: Vec<&str> = document["verbs"]
        .as_object()
        .expect("verbs")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(answered, verbs, "every verb was answered at least once");
}

/// CALLS AT ONCE ON ONE ROOT RUN ONE AFTER THE OTHER: every create of sixteen
/// callers, started together, is in the store afterwards, each under an id of
/// its own.
///
/// RED-PROOF: with the lock taken away, two calls load the same state, mint
/// the same id from it, and the later save drops the earlier's item — 22
/// distinct ids of 48, measured.
#[test]
fn calls_at_once_on_one_root_lose_no_write() {
    const CALLERS: usize = 16;
    const EACH: usize = 3;
    let stub = Stubbed::new("race");
    let start = Barrier::new(CALLERS);
    let ids: Vec<ItemId> = std::thread::scope(|scope| {
        let callers: Vec<_> = (0..CALLERS)
            .map(|caller| {
                let (stub, start) = (&stub, &start);
                scope.spawn(move || {
                    let exec = stub.exec();
                    start.wait();
                    (0..EACH)
                        .map(|n| {
                            exec.create(&a_task(&format!("caller {caller}, item {n}")), &the_test())
                                .expect("the create is answered")
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        callers
            .into_iter()
            .flat_map(|caller| caller.join().expect("the caller ends"))
            .collect()
    });
    let mut distinct = ids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), CALLERS * EACH, "each create its own id");
    let mut listed: Vec<ItemId> = stub
        .exec()
        .list(&Filter::Label(String::from("stub")))
        .expect("the listing reads")
        .into_iter()
        .map(|row| row.id)
        .collect();
    listed.sort_unstable();
    assert_eq!(listed, distinct, "every create is in the store");
}

/// A DEAF CALL IS THE IN-PROCESS `ignore_writes`, for one call: the write is
/// answered and not applied, so its read-back disagrees with it — the same
/// disagreement the board held in memory gives, and the same red the
/// conformance check that reads a create back gives on both. The call after
/// it writes again.
#[test]
fn a_deaf_write_is_answered_and_its_read_back_disagrees_as_in_process() {
    let stub = Stubbed::new("deaf");
    let exec = stub.exec();
    let deaf = stub.with_env("deaf", "FLEET_STUB_DEAF=1");
    let item = exec
        .create(&a_task("the title it was filed under"), &the_test())
        .expect("the item is filed");
    let retitle = |title: &str| Update::title(title.to_string());

    let memory = FakeStore::default();
    let held = memory
        .create(&a_task("the title it was filed under"), &the_test())
        .expect("the item is filed");
    memory.ignore_writes();
    assert_eq!(
        memory.update(&held, &retitle("a title nothing kept"), &the_test()),
        Ok(())
    );
    assert_eq!(
        deaf.update(&item, &retitle("a title nothing kept"), &the_test()),
        Ok(()),
        "the deaf write is answered as taken"
    );
    let in_memory = memory.show(&held).expect("the item reads").title;
    let on_stub = exec.show(&item).expect("the item reads").title;
    assert_eq!(on_stub, in_memory, "both read back what was there before");
    assert_eq!(on_stub, "the title it was filed under");

    exec.update(&item, &retitle("a title the next call kept"), &the_test())
        .expect("the next write is taken");
    assert_eq!(
        exec.show(&item).expect("the item reads").title,
        "a title the next call kept",
        "the knob was that one call's"
    );

    let check = CHECKS
        .iter()
        .find(|(name, _)| *name == "create then show")
        .map(|(_, check)| *check)
        .expect("the check is on the table");
    let nowhere = Fixture::new("stub-deaf-absent");
    let absent = FakeStore {
        unreadable: Some(String::from("not there")),
        ..FakeStore::default()
    };
    let deaf_memory = FakeStore::default();
    deaf_memory.ignore_writes();
    for (which, store) in [
        ("in memory", &deaf_memory as &dyn Store),
        ("the stub", &deaf as &dyn Store),
    ] {
        let why = check(&Ctx {
            store,
            root: &nowhere.root,
            absent: &absent,
            another_writer: None,
        })
        .expect_err("a create nothing kept does not read back");
        assert!(
            why.starts_with("show fx-") && why.contains("answered Refused (fx-"),
            "{which}: the read-back is refused, naming the item: {why}"
        );
    }
}

/// A slow call outruns the store call's bound and is killed, as a store that
/// does not answer is — and the write it carried is never made, because the
/// wait comes before the store is read.
#[test]
fn a_slow_call_is_killed_at_the_bound_and_writes_nothing() {
    let stub = Stubbed::new("slow");
    let slow = stub
        .with_env("slow", "FLEET_STUB_SLOW=5")
        .with_timeout(Duration::from_secs(1));
    match slow.create(&a_task("an item too slow to file"), &the_test()) {
        Err(StoreError::Unreadable(why)) => {
            assert!(why.contains("did not answer within 1s"), "{why}")
        }
        other => panic!("a slow call is Unreadable at the bound: {other:?}"),
    }
    assert_eq!(
        stub.exec().list(&Filter::Ready).expect("the listing reads"),
        Vec::new(),
        "nothing was filed"
    );
}

/// A rig reaches the store where no verb of the contract does — a label, a
/// blocker, another writer's keys — under the same lock, and the next call
/// answers what it planted.
#[test]
fn a_rig_puts_the_store_in_a_state_the_next_call_answers() {
    let stub = Stubbed::new("rig");
    let exec = stub.exec();
    let item = exec
        .create(&a_task("an item a rig moves"), &the_test())
        .expect("the item is filed");
    stubbed::with_state(stub.root(), |store| {
        store.amend(&item, |held| held.labels.push(String::from("planted")));
        store.plant_metadata(&item, r#"{"sprint":4}"#);
    })
    .expect("the rig reaches the store");
    let rows = exec
        .list(&Filter::Label(String::from("planted")))
        .expect("the listing reads");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, item);
    assert_eq!(rows[0].foreign, ["sprint"]);

    let nowhere = Fixture::new("stub-rig-absent");
    assert!(
        stubbed::with_state(&nowhere.root, |_| ()).is_err(),
        "a root holding no store is no store to move"
    );
}

/// A scratch over a root that holds a store empties it.
#[test]
fn a_scratch_over_a_store_empties_it() {
    let stub = Stubbed::new("rescratch");
    let exec = stub.exec();
    exec.create(&a_task("an item a scratch forgets"), &the_test())
        .expect("the item is filed");
    assert_eq!(exec.scratch(stub.root()), Ok(PathBuf::from(stub.root())));
    assert_eq!(exec.list(&Filter::Ready).expect("it reads"), Vec::new());
    assert!(stub.root().join(STATE_FILE).is_file());
}
