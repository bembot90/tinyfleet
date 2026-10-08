//! The event seam over the machine directory's own stream: core states the kind, the actor and the payload, and the line is appended here.

use std::path::PathBuf;

use fleet_core::item::run as workflow_run;
use fleet_core::item::Events;
use fleet_core::seat::actor::Actor;

/// The event seam: core states the kind, the actor and the payload, and the
/// stream is opened HERE, over the machine directory's own file.
///
/// The log is opened per append rather than held, because core's seam takes
/// `&self` and the writer's own sequence is re-read off the file on every
/// append anyway — so a held handle would buy nothing and would make the
/// sequence a fact two processes could each believe.
pub struct StreamEvents {
    path: PathBuf,
}

impl StreamEvents {
    pub fn at(path: PathBuf) -> StreamEvents {
        StreamEvents { path }
    }
}

impl Events for StreamEvents {
    fn append(&self, kind: &str, actor: &Actor, payload: serde_json::Value) -> Result<(), String> {
        crate::events::EventLog::open(&self.path)
            .append(kind, &stream_actor(actor), payload)
            .map_err(|e| format!("{} could not be appended to: {e}", self.path.display()))
    }
}

/// The typed actor as the stream stores it: core's kind word and its id, as
/// the `{kind, id}` object.
pub fn stream_actor(actor: &Actor) -> crate::events::ActorRef {
    crate::events::ActorRef::new(actor.kind.as_str(), actor.id.clone())
}

/// The reading side of the file the appends go to — one type for both, so the
/// stream a run's child is told about and the stream its events land on cannot
/// come to be two files.
impl workflow_run::Stream for StreamEvents {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn seq(&self) -> u64 {
        crate::events::EventLog::open(&self.path).seq()
    }
}
