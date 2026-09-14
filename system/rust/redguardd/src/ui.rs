//! redguard's end of the UI protocol: what a freshly-connected bar is told,
//! and what each message it sends back means.
//!
//! The socket itself, its authorisation and its framing are shared with
//! redwall and live in `redcommon::ui_sock`. The two daemons deliberately
//! speak the same dialect, so the bar's two tabs are the same code twice.

use std::sync::Arc;

use redcommon::json::{self, Json};
use redcommon::ui_sock::{Clients, Handler};

use crate::guard::{Answer, Guard, Policy};

pub struct Ui {
    guard: Arc<Guard>,
}

impl Ui {
    pub fn new(guard: Arc<Guard>) -> Ui {
        Ui { guard }
    }
}

impl Handler for Ui {
    /// Catch a freshly-launched bar up: current rules, current state, and any
    /// process frozen right now waiting on an answer.
    fn on_connect(&self, clients: &Clients, client: u64) {
        clients.send_to(
            client,
            &json::obj([("t", json::s("rules")), ("rules", self.guard.rules_snapshot())]),
        );
        clients.send_to(
            client,
            &json::obj([
                ("t", json::s("state")),
                ("enabled", Json::Bool(self.guard.enabled())),
            ]),
        );
        for ask in self.guard.waiting_asks() {
            clients.send_to(client, &ask);
        }
    }

    fn on_message(&self, msg: &Json) {
        match msg.str_field("t") {
            Some("verdict") => {
                let Some(id) = msg.get("id").and_then(Json::as_u64) else { return };
                // A verdict with no action is a block: the prompt's own
                // default, and the side that cannot be undone by waiting.
                let answer = Answer::from_ui(msg.str_field("action").unwrap_or("block"));
                self.guard
                    .apply_verdict(id, answer, msg.bool_field("remember", true));
            }
            Some("setrule") => {
                let Some(exe) = msg.str_field("exe") else { return };
                let policy = Policy::from_ui(msg.str_field("action").unwrap_or("block"));
                self.guard.set_rule(exe, policy, msg.str_field("name"));
            }
            Some("delrule") => {
                if let Some(exe) = msg.str_field("exe") {
                    self.guard.delete_rule(exe);
                }
            }
            Some("getrules") => self.guard.push_rules(),
            Some("setenabled") => self.guard.set_enabled(msg.bool_field("enabled", true)),
            // Only ever answered under --simulate, where nothing is frozen.
            Some("simdetect") => self.guard.inject(msg),
            _ => {}
        }
    }

    /// No UI left means nobody can answer, and a frozen process the user
    /// cannot see is a hang they cannot explain. The timeout would catch it a
    /// minute later; letting go now is the same decision, sooner.
    fn on_idle(&self) {
        self.guard.release_all("no UI connected");
    }
}
