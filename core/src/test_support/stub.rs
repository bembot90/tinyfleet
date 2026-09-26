//! The store contract answered by an executable over [`FakeStore`]:
//! `fleet-store-stub <verb>`, the request on stdin and the answer on stdout,
//! one process per call as [`crate::store::exec::Exec`] runs one — so every
//! `Exec` path, and the conformance suite through it, runs with no store
//! installed on the box.
//!
//! THE STORE LIVES IN A FILE, [`STATE_FILE`] under the request's root: each call
//! loads the fake's [`State`] from it, answers the verb from `impl Store for
//! FakeStore`, and writes the state back, all under [`LOCK_FILE`] — so two calls
//! on one root at once run one after the other and neither loses the other's
//! write. A root with no state file holds no store, and every verb that reads
//! one is could not tell there: that is the store a check asks for "not there".
//!
//! THE EXIT IS THE CONTRACT'S TABLE. 0 is the verb's response; 1 the fake's
//! typed refusal as `{"refused": …}`; 2 an unknown verb, a request that does
//! not decode as its verb's fields, or a request the contract calls malformed;
//! 3 a store that could not tell, as `{"error": …}`. Nothing is written on
//! stderr but a usage line, because `Exec` carries stderr into every refusal
//! it reads.
//!
//! Two knobs, each read for the one call that carries it: [`DEAF`] makes the
//! fake record every write and apply none ([`FakeStore::ignore_writes`]), and
//! [`SLOW`] sleeps before the call is answered, outside the lock.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{FakeStore, State};
use crate::entry::{self, Read};
use crate::seat::actor::Actor;
use crate::store::types::{
    Answered, Appended, Capabilities, Created, Exported, Listed, OpenHolds, Raised, Refusal,
    Resolved, Scratched, Shown, CONTRACT_VERSION,
};
use crate::store::{
    first_value, validated_new, writable, Filter, HoldId, ItemId, NewItem, Order, RunRecord, Store,
    StoreError, Update, Version, WithdrawFence,
};

/// Where a root's store is kept, relative to it: beside the fake's export, in
/// the directory the fake declares as the store's own.
pub const STATE_FILE: &str = ".store/state.json";

/// The file every call on a root holds an exclusive lock on, from before the
/// state is read until after it is written back.
pub const LOCK_FILE: &str = ".store/state.lock";

/// `1` in the environment: this call's writes are recorded and not applied.
pub const DEAF: &str = "FLEET_STUB_DEAF";

/// Seconds, in the environment: how long this call sleeps before it is
/// answered, for an arm about a store call's bound.
pub const SLOW: &str = "FLEET_STUB_SLOW";

/// How long a call waits on another call's lock before it answers could not
/// tell: well inside the store call's own 60 s bound, and far past any one
/// call's hold, which is a load, a verb and a save.
const LOCK_WAIT: Duration = Duration::from_secs(30);

/// How a call ends, by the row of the contract's exit table it answers.
enum Answer {
    /// Exit 0: the verb's response body, without `schema_version`.
    Body(Map<String, Value>),
    /// Exit 1: the fake's typed refusal.
    Refused(Refusal),
    /// Exit 2: a request this stub does not speak, and why.
    Usage(String),
    /// Exit 3: the store could not tell, and why.
    Untold(String),
}

impl Answer {
    fn body<T: Serialize>(body: T) -> Answer {
        match serde_json::to_value(body).expect("a response body is JSON") {
            Value::Object(body) => Answer::Body(body),
            other => unreachable!("every response body is an object, and this is {other}"),
        }
    }
}

/// One call: the verb off argv, the request off stdin, the answer on stdout and
/// the exit by the contract's table.
pub fn main() -> ExitCode {
    let verb = std::env::args().nth(1).unwrap_or_default();
    let mut request = String::new();
    let answer = match std::io::stdin().read_to_string(&mut request) {
        Err(e) => Answer::Usage(format!("the request could not be read off stdin: {e}")),
        Ok(_) => match slow() {
            Err(why) => Answer::Usage(why),
            Ok(wait) => {
                std::thread::sleep(wait);
                let deaf = std::env::var(DEAF).is_ok_and(|value| value == "1");
                answered(&verb, &request, deaf)
            }
        },
    };
    let version = Value::from(CONTRACT_VERSION);
    let (code, printed) = match answer {
        Answer::Body(mut body) => {
            body.insert(String::from("schema_version"), version);
            (0, Value::Object(body))
        }
        Answer::Refused(refusal) => (1, json!({"schema_version": version, "refused": refusal})),
        Answer::Usage(why) => {
            eprintln!("fleet-store-stub {verb}: {why}");
            return ExitCode::from(2);
        }
        Answer::Untold(why) => (3, json!({"schema_version": version, "error": why})),
    };
    println!("{printed}");
    ExitCode::from(code)
}

/// [`SLOW`]'s wait, none where it is unset.
fn slow() -> Result<Duration, String> {
    match std::env::var(SLOW) {
        Err(_) => Ok(Duration::ZERO),
        Ok(secs) => secs
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
            .ok_or_else(|| format!("{SLOW} is `{secs}`, which is no number of seconds")),
    }
}

/// Every verb of the contract, which is every verb this stub answers.
const VERBS: [&str; 18] = [
    "version",
    "capabilities",
    "resolve",
    "show",
    "list",
    "timeline",
    "create",
    "update",
    "append",
    "order.set",
    "order.withdraw",
    "run.set",
    "hold.raise",
    "hold.clear",
    "holds.open",
    "close",
    "export",
    "scratch",
];

/// The verb answered from the request's text.
fn answered(verb: &str, request: &str, deaf: bool) -> Answer {
    if !VERBS.contains(&verb) {
        return Answer::Usage(format!("`{verb}` is no verb of the store contract"));
    }
    let fields = match envelope(request) {
        Ok(fields) => fields,
        Err(answer) => return answer,
    };
    let root = match field::<String>(&fields, "root") {
        Ok(root) => PathBuf::from(root),
        Err(answer) => return answer,
    };
    let result = match verb {
        "version" => Ok(Answer::body(Version {
            name: String::from("stub"),
            version: String::from("0"),
        })),
        "capabilities" => capabilities(&root),
        "scratch" => field::<String>(&fields, "into").and_then(|into| {
            scratch(Path::new(&into)).map_err(Answer::Untold)?;
            Ok(Answer::body(Scratched { root: into }))
        }),
        _ => stored(verb, &fields, &root, deaf),
    };
    result.unwrap_or_else(|answer| answer)
}

/// The request's fields, held to the envelope: one JSON object at
/// `schema_version` 1.
fn envelope(request: &str) -> Result<Map<String, Value>, Answer> {
    let Some(Value::Object(fields)) = first_value(request) else {
        return Err(Answer::Usage(String::from(
            "the request is not one JSON object",
        )));
    };
    match fields.get("schema_version") {
        Some(version) if version.as_u64() == Some(CONTRACT_VERSION) => Ok(fields),
        other => Err(Answer::Usage(format!(
            "the request's schema_version is {}; this stub speaks {CONTRACT_VERSION}",
            other.map_or_else(|| String::from("missing"), Value::to_string)
        ))),
    }
}

/// One of the request's fields, read as the contract's type for it.
fn field<T: DeserializeOwned>(fields: &Map<String, Value>, key: &str) -> Result<T, Answer> {
    let value = fields
        .get(key)
        .cloned()
        .ok_or_else(|| Answer::Usage(format!("the request carries no `{key}`")))?;
    serde_json::from_value(value)
        .map_err(|why| Answer::Usage(format!("the request's `{key}` does not read: {why}")))
}

/// The whole request read as one of the contract's changes, whose fields sit
/// beside the id and the actor rather than under a key of their own.
fn whole<T: DeserializeOwned>(fields: &Map<String, Value>, what: &str) -> Result<T, Answer> {
    serde_json::from_value(Value::Object(fields.clone()))
        .map_err(|why| Answer::Usage(format!("the request's {what} does not read: {why}")))
}

/// What the fake declares, with the prefix its hashed ids carry: read off the
/// root's store where one is there, and the fake's defaults where none is —
/// a caller asks before it has made one.
fn capabilities(root: &Path) -> Result<Answer, Answer> {
    let declared = if root.join(STATE_FILE).is_file() {
        with_state(root, |store| store.capabilities()).map_err(Answer::Untold)?
    } else {
        FakeStore::default().capabilities()
    };
    let declared = declared.map_err(|why| Answer::Untold(why.to_string()))?;
    Ok(Answer::body(Capabilities {
        item_prefix: Some(String::from("fx")),
        ..declared
    }))
}

/// A verb that reads or writes the root's store, answered from the fake with
/// the lock held from the load to the save.
fn stored(
    verb: &str,
    fields: &Map<String, Value>,
    root: &Path,
    deaf: bool,
) -> Result<Answer, Answer> {
    if !root.join(STATE_FILE).is_file() {
        return Err(Answer::Untold(absent(root)));
    }
    let _held = locked(root).map_err(Answer::Untold)?;
    let store = load(root).map_err(Answer::Untold)?;
    if deaf {
        store.ignore_writes();
    }
    let answer = asked(&store, verb, fields);
    save(root, &store).map_err(Answer::Untold)?;
    answer
}

/// One verb asked of the fake, its request decoded as the contract's fields.
fn asked(store: &FakeStore, verb: &str, fields: &Map<String, Value>) -> Result<Answer, Answer> {
    let id = || field::<ItemId>(fields, "id");
    let by = || field::<Actor>(fields, "by");
    let taken = |done: Result<(), StoreError>| done.map(|()| Answer::body(Answered {}));
    let answer = match verb {
        "resolve" => store
            .resolve(&field::<String>(fields, "id")?)
            .map(|id| Answer::body(Resolved { id })),
        "show" => store
            .show(&field::<String>(fields, "id")?)
            .map(|item| Answer::body(Shown { item })),
        "list" => store
            .list(&field::<Filter>(fields, "filter")?)
            .map(|items| Answer::body(Listed { items })),
        "timeline" => store.timeline(&id()?).map(|entries| {
            let entries: Vec<Value> = entries.iter().map(entry::to_json).collect();
            Answer::body(json!({ "entries": entries }))
        }),
        "create" => {
            let item = field::<NewItem>(fields, "item")?;
            validated_new(&item).map_err(|why| Answer::Usage(why.to_string()))?;
            store
                .create(&item, &by()?)
                .map(|id| Answer::body(Created { id }))
        }
        "update" => {
            let change = whole::<Update>(fields, "change")?;
            writable(&change).map_err(|why| Answer::Usage(why.to_string()))?;
            taken(store.update(&id()?, &change, &by()?))
        }
        "append" => {
            let text = field::<Value>(fields, "entry")?.to_string();
            let body = match entry::decode(&text) {
                Read::Entry(body) => body,
                Read::NotAnEntry => {
                    return Err(Answer::Usage(format!(
                        "the request's entry carries no {}: {text}",
                        entry::KEY
                    )))
                }
                Read::Unreadable(why) => {
                    return Err(Answer::Usage(format!(
                        "the request's entry does not read: {why}"
                    )))
                }
            };
            store
                .append(&id()?, &body, &by()?)
                .map(|entry| Answer::body(Appended { entry }))
        }
        "order.set" => taken(store.order_set(&id()?, &field::<Order>(fields, "order")?, &by()?)),
        "order.withdraw" => {
            let fence = whole::<WithdrawFence>(fields, "fence")?;
            taken(store.order_withdraw(&id()?, &fence, &by()?))
        }
        "run.set" => taken(store.run_set(&id()?, &field::<RunRecord>(fields, "run")?, &by()?)),
        "hold.raise" => store
            .hold_raise(&id()?, &field::<String>(fields, "reason")?, &by()?)
            .map(|hold| Answer::body(Raised { hold })),
        "hold.clear" => taken(store.hold_clear(&field::<HoldId>(fields, "hold")?, &by()?)),
        "holds.open" => store
            .holds_open()
            .map(|holds| Answer::body(OpenHolds { holds })),
        "close" => taken(store.close(&id()?, &field::<String>(fields, "reason")?, &by()?)),
        "export" => {
            if store.no_export {
                return Err(Answer::Usage(String::from("this store declares no export")));
            }
            store
                .export(Path::new(&field::<String>(fields, "into")?))
                .map(|file| {
                    Answer::body(Exported {
                        file: file.display().to_string(),
                    })
                })
        }
        other => unreachable!("`{other}` is one of the verbs `stored` is handed"),
    };
    answer.map_err(|error| refusal(store, error))
}

/// A store error as the row it answers: the fake's typed refusal behind a
/// Refused or Moved, usage, or could not tell.
fn refusal(store: &FakeStore, error: StoreError) -> Answer {
    match error {
        StoreError::Refused(text) | StoreError::Moved(text) => {
            let typed = store
                .refusal
                .lock()
                .expect("the refusal is not poisoned")
                .take();
            match typed {
                Some(typed) if typed.message == text => Answer::Refused(typed),
                _ => Answer::Untold(format!(
                    "the fake refused with no typed refusal behind it: {text}"
                )),
            }
        }
        StoreError::Usage(why) => Answer::Usage(why),
        StoreError::Unreadable(why) => Answer::Untold(why),
    }
}

/// What a root with no store answers.
fn absent(root: &Path) -> String {
    format!(
        "no store at {}: {} is not there",
        root.display(),
        root.join(STATE_FILE).display()
    )
}

/// The root's lock, held until the file is dropped, or why it could not be
/// taken inside [`LOCK_WAIT`].
///
/// AN OS LOCK AND NOT A LOCKFILE'S PRESENCE: the kernel lets it go when the
/// holder dies, so a call killed at the store's bound leaves no lock behind.
fn locked(root: &Path) -> Result<File, String> {
    let path = root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{} could not be opened: {e}", path.display()))?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(format!(
                    "the store at {} was held by another call for {}s",
                    root.display(),
                    LOCK_WAIT.as_secs()
                ))
            }
            Err(TryLockError::Error(e)) => {
                return Err(format!("{} could not be locked: {e}", path.display()))
            }
        }
    }
}

/// The fake the root's state file describes, minting hashed ids.
fn load(root: &Path) -> Result<FakeStore, String> {
    let path = root.join(STATE_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => absent(root),
        _ => format!("{} could not be read: {e}", path.display()),
    })?;
    let state: State = serde_json::from_str(&text).map_err(|why| {
        format!(
            "{} does not read as the stub's state: {why}",
            path.display()
        )
    })?;
    Ok(FakeStore {
        hashed: true,
        ..FakeStore::from_state(state, root)
    })
}

/// The fake's state written over the root's file whole: to a file of its own
/// first and renamed into place, so no reader ever meets half of one.
fn save(root: &Path, store: &FakeStore) -> Result<(), String> {
    let path = root.join(STATE_FILE);
    let text = serde_json::to_string_pretty(&store.state()).expect("the state is JSON");
    let staged = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&staged, text)
        .and_then(|()| std::fs::rename(&staged, &path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&staged);
            format!("{} could not be written: {e}", path.display())
        })
}

/// An empty store made under `into`, whatever stood there before.
pub fn scratch(into: &Path) -> Result<(), String> {
    let dir = into.join(STATE_FILE);
    let dir = dir.parent().expect("the state file sits in a directory");
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    let _held = locked(into)?;
    save(into, &FakeStore::default())
}

/// `act` on the root's store, under its lock, and the store written back after
/// it: the way in for a rig putting the store in a state no verb of the
/// contract reaches — a label, a blocker, another writer's keys — as
/// [`super::Board`]'s own rig reaches the fake in memory.
pub fn with_state<T>(root: &Path, act: impl FnOnce(&FakeStore) -> T) -> Result<T, String> {
    if !root.join(STATE_FILE).is_file() {
        return Err(absent(root));
    }
    let _held = locked(root)?;
    let store = load(root)?;
    let answer = act(&store);
    save(root, &store)?;
    Ok(answer)
}
