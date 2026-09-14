//! redwall's end of the UI protocol: what a freshly-connected bar is told,
//! and what each message it sends back means.
//!
//! The socket itself, its authorisation and its framing are shared with
//! redguard and live in `redcommon::ui_sock`.

use std::sync::Arc;

use redcommon::json::{self, Json};
use redcommon::ui_sock::{Clients, Handler};

use crate::firewall::{Action, Firewall};

pub struct Ui {
    fw: Arc<Firewall>,
}

impl Ui {
    pub fn new(fw: Arc<Firewall>) -> Ui {
        Ui { fw }
    }
}

impl Handler for Ui {
    /// Catch a freshly-launched bar up: current rules, current state, and any
    /// prompt already waiting for an answer.
    fn on_connect(&self, clients: &Clients, client: u64) {
        clients.send_to(
            client,
            &json::obj([("t", json::s("rules")), ("rules", self.fw.rules_snapshot())]),
        );
        clients.send_to(
            client,
            &json::obj([
                ("t", json::s("state")),
                ("enabled", Json::Bool(self.fw.enabled())),
            ]),
        );
        for ask in self.fw.waiting_asks() {
            clients.send_to(client, &ask);
        }
    }

    fn on_message(&self, msg: &Json) {
        match msg.str_field("t") {
            Some("verdict") => {
                let Some(id) = msg.get("id").and_then(Json::as_u64) else { return };
                let action = Action::from_ui(msg.str_field("action").unwrap_or("deny"));
                self.fw
                    .apply_verdict(id, action, msg.bool_field("remember", true));
            }
            Some("setrule") => {
                let Some(exe) = msg.str_field("exe") else { return };
                let action = Action::from_ui(msg.str_field("action").unwrap_or("deny"));
                self.fw.set_rule(exe, action, msg.str_field("name"));
            }
            Some("delrule") => {
                if let Some(exe) = msg.str_field("exe") {
                    self.fw.delete_rule(exe);
                }
            }
            Some("getrules") => self.fw.push_rules(),
            // Only ever answered under --simulate, where no packet is held.
            Some("simconnect") => self.fw.inject(msg),
            Some("setenabled") => self.fw.set_enabled(msg.bool_field("enabled", true)),
            _ => {}
        }
    }

    /// No UI left means nobody can answer, and new packets already pass
    /// straight through in that state — so anything still held has to go
    /// through too, or a shell reload would strand it.
    fn on_idle(&self) {
        self.fw.release_all("no UI connected");
    }
}
