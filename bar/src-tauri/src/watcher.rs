//! The tray's registry: `org.kde.StatusNotifierWatcher`.
//!
//! An application with a tray icon tells the watcher it has one, and a host
//! — the bar — asks the watcher which there are. Quickshell was this
//! desktop's watcher, and when it was retired nothing was: applications had
//! nobody to tell, and the tray stayed empty on every machine without some
//! other watcher of its own. So the shell is the watcher itself.
//!
//! Only where nobody else already is, or another offers to let go: the name
//! is queued for, and the shell becomes the watcher the moment one leaves.

use std::sync::{Arc, Mutex, PoisonError};

use zbus::fdo::RequestNameFlags;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

pub const NAME: &str = "org.kde.StatusNotifierWatcher";
pub const PATH: &str = "/StatusNotifierWatcher";

/// The items as hosts read them: a bus name, or a bus name and the path the
/// item is at, run together (`:1.42/org/ayatana/NotificationItem/steam`).
type Items = Arc<Mutex<Vec<String>>>;

struct Watcher {
    items: Items,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    /// `service` is a bus name, or — as libappindicator sends it — the path
    /// of the item on the caller's own connection.
    async fn register_status_notifier_item(
        &self,
        service: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|sender| sender.to_string()).unwrap_or_default();
        let entry = entry_for(&service, &sender);
        let new = {
            let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
            let new = !items.contains(&entry);
            if new {
                items.push(entry.clone());
            }
            new
        };
        if new {
            let _ = Self::status_notifier_item_registered(&emitter, &entry).await;
        }
    }

    /// The shell is the host, and says so for itself; another host asking is
    /// told the same as ever.
    async fn register_status_notifier_host(&self, _service: String, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) {
        let _ = Self::status_notifier_host_registered(&emitter).await;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// There is always a host while this watcher is: it is the shell's. An
    /// application asks this before registering, and one told no falls back
    /// to an old kind of tray that nothing here shows.
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// What an item registered as `service` by `sender` is listed as.
fn entry_for(service: &str, sender: &str) -> String {
    if service.starts_with('/') { format!("{sender}{service}") } else { service.to_string() }
}

/// The bus name an entry is reached at: everything before its path.
fn owner_of(entry: &str) -> &str {
    entry.split_once('/').map_or(entry, |(name, _)| name)
}

/// Serves the watcher and queues for its name. Returns once the name has
/// been asked for, so that a host registering straight after finds it where
/// it can; a thread of its own then keeps the list and the connection.
pub fn serve() {
    let items: Items = Arc::default();
    let built = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.serve_at(PATH, Watcher { items: items.clone() }))
        .and_then(|builder| builder.build());
    let connection = match built {
        Ok(connection) => connection,
        Err(error) => return eprintln!("cae: the tray's watcher could not start: {error}"),
    };
    // Without `DoNotQueue`: where another watcher holds the name and will
    // not hand it over, this one waits in line and takes over if it goes.
    if let Err(error) = connection.request_name_with_flags(NAME, RequestNameFlags::ReplaceExisting.into()) {
        return eprintln!("cae: the tray's watcher could not ask for its name: {error}");
    }
    std::thread::Builder::new()
        .name("tray-watcher".to_string())
        .spawn(move || forget_the_departed(&connection, &items))
        .ok();
}

/// Drops the items of every application that leaves the bus, for as long as
/// the connection lasts: an application that quits or crashes does not say
/// so first.
fn forget_the_departed(connection: &zbus::blocking::Connection, items: &Items) {
    let rule = "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged'";
    let Ok(changes) = zbus::blocking::MessageIterator::for_match_rule(rule, connection, Some(64)) else { return };
    for change in changes.filter_map(Result::ok) {
        let Ok((name, _, owner)) = change.body().deserialize::<(String, String, String)>() else { continue };
        if !owner.is_empty() {
            continue;
        }
        let gone: Vec<String> = {
            let mut items = items.lock().unwrap_or_else(PoisonError::into_inner);
            let (gone, kept) = items.drain(..).partition(|entry| owner_of(entry) == name);
            *items = kept;
            gone
        };
        let Ok(watcher) = connection.object_server().interface::<_, Watcher>(PATH) else { continue };
        for entry in gone {
            let _ = zbus::block_on(Watcher::status_notifier_item_unregistered(watcher.signal_emitter(), &entry));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_listed_by_name_or_by_its_path_on_the_caller() {
        assert_eq!(entry_for("org.kde.StatusNotifierItem-1234-1", ":1.42"), "org.kde.StatusNotifierItem-1234-1");
        assert_eq!(entry_for(":1.7", ":1.7"), ":1.7");
        // libappindicator sends a path, which is on the sender's connection.
        assert_eq!(entry_for("/org/ayatana/NotificationItem/steam", ":1.42"), ":1.42/org/ayatana/NotificationItem/steam");
    }

    #[test]
    fn an_entry_belongs_to_the_name_before_its_path() {
        assert_eq!(owner_of(":1.42/org/ayatana/NotificationItem/steam"), ":1.42");
        assert_eq!(owner_of("org.kde.StatusNotifierItem-1234-1"), "org.kde.StatusNotifierItem-1234-1");
    }
}
