//! A store adapter executable that answers from files and records every
//! request, for the arms that ask what a verb SENDS over the contract.
//!
//! The in-memory board answers a write the way bd does, and the verbs reach
//! both through methods no adapter out of process sees. An arm asking what one
//! of them sends to an adapter drives the verb over [`Exec`], the store an
//! adapter is reached through, and reads the requests off disk.
//!
//! EACH VERB ANSWERS IN TURN: the k-th call of a verb gets the k-th answer the
//! arm gave that verb, and every call past the last gets the last. A verb given
//! no answer is could not tell, so a call the arm did not expect reads as a
//! red and never as a success.

use std::path::PathBuf;

use fleet_core::store::exec::Exec;

use super::Fixture;

pub struct Adapter {
    dir: Fixture,
    bin: PathBuf,
}

impl Adapter {
    pub fn new(label: &str) -> Adapter {
        let dir = Fixture::new(label);
        dir.dir("answers");
        let bin = dir.path("adapter");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n\
                 dir='{dir}'\n\
                 printf '%s\\n' \"$1\" >> \"$dir/argv\"\n\
                 k=$(grep -cxF -- \"$1\" \"$dir/argv\")\n\
                 cat > \"$dir/request-$1.$k.json\"\n\
                 while [ \"$k\" -gt 0 ] && [ ! -f \"$dir/answers/$1.$k\" ]; do k=$((k-1)); done\n\
                 [ \"$k\" -gt 0 ] || {{ echo \"no answer for $1\" >&2; exit 3; }}\n\
                 cat \"$dir/answers/$1.$k\"\n\
                 exit \"$(cat \"$dir/answers/$1.$k.code\")\"\n",
                dir = dir.root.display(),
            ),
        )
        .expect("the adapter is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("the adapter is executable");
        Adapter { dir, bin }
    }

    /// `verb`'s answers, in the order its calls are to get them: each the
    /// response's JSON and the exit it answers with.
    pub fn answers(&self, verb: &str, answers: &[(&str, i32)]) -> &Adapter {
        for (k, (json, code)) in answers.iter().enumerate() {
            let at = format!("answers/{verb}.{}", k + 1);
            self.dir.file(&at, json);
            self.dir.file(&format!("{at}.code"), &code.to_string());
        }
        self
    }

    /// The store over this adapter, scoped to the fixture's own root.
    pub fn exec(&self) -> Exec {
        Exec::at(&self.bin, &self.dir.root)
    }

    /// Every verb it was called with, in order.
    pub fn verbs(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path("argv"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The request the k-th call of `verb` carried, counted from 1.
    pub fn request(&self, verb: &str, k: usize) -> serde_json::Value {
        let text = std::fs::read_to_string(self.dir.path(&format!("request-{verb}.{k}.json")))
            .unwrap_or_else(|e| panic!("call {k} of {verb} was recorded: {e}"));
        serde_json::from_str(&text).expect("the request is one JSON value")
    }
}
