//! The desktop's settings, as the two files they are kept in.
//!
//! `shell.json` is upstream's: every option its shell has, written only where
//! it differs from the default, which is why a file with thirty lines in it
//! describes a shell with four hundred options. `prefs.json` is this fork's,
//! for what upstream's schema has no place for. The shell that is still QML
//! watches both and picks a change up as it lands, so writing one here is how
//! the two shells are told the same thing at once.
//!
//! Written a key at a time, into whatever the file holds at that moment. The
//! file is never rebuilt from what this process believes is in it: somebody
//! else writes it too, and what they wrote a second ago is not ours to undo.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum File {
    Shell,
    Prefs,
}

/// One setting: which file, and where in it, as `bar.status.showAudio`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub file: File,
    pub path: &'static str,
}

pub const fn shell(path: &'static str) -> Key {
    Key { file: File::Shell, path }
}

pub const fn prefs(path: &'static str) -> Key {
    Key { file: File::Prefs, path }
}

/// Both files, as they were when last read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub shell: Value,
    pub prefs: Value,
}

impl Snapshot {
    pub fn read() -> Snapshot {
        Snapshot { shell: read(File::Shell), prefs: read(File::Prefs) }
    }

    /// What the file says, which for most keys on most machines is nothing:
    /// the default is whoever asks's to know.
    pub fn get(&self, key: Key) -> Option<&Value> {
        lookup(if key.file == File::Shell { &self.shell } else { &self.prefs }, key.path)
    }

    /// The same change `write` makes to the file, made to this copy, so that
    /// a switch shows where it was put without waiting for the disk.
    pub fn set(&mut self, key: Key, value: Value) {
        put(if key.file == File::Shell { &mut self.shell } else { &mut self.prefs }, key.path, value);
    }
}

pub fn path(file: File) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let (variable, fallback, name) = match file {
        File::Shell => ("XDG_CONFIG_HOME", ".config", "shell.json"),
        File::Prefs => ("XDG_STATE_HOME", ".local/state", "prefs.json"),
    };
    let base = std::env::var(variable).map(PathBuf::from).unwrap_or_else(|_| Path::new(&home).join(fallback));
    Some(base.join("caelestia").join(name))
}

/// A file that is missing, empty or not JSON reads as nothing at all, and
/// everything in it is then at its default.
pub fn read(file: File) -> Value {
    path(file).map(|path| read_at(&path)).unwrap_or(Value::Null)
}

fn read_at(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

/// Why a settings file is not being read: it is there, it has something in
/// it, and none of it is JSON. Nothing when the file is fine, missing or
/// empty — a file nobody has written is not a complaint, it is a default.
///
/// Worth saying out loud, because the failure is silent otherwise: the file
/// reads as nothing and every setting in it quietly goes back to what it
/// shipped as.
pub fn complaint(file: File) -> Option<String> {
    complaint_at(&path(file)?)
}

fn complaint_at(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    let why = serde_json::from_str::<Value>(&text).err()?;
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    Some(format!("{name}: {why}"))
}

pub fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |node, name| node.get(name))
}

/// Sets one key, making whatever objects lie between the root and it. What
/// is in the way and is not an object is replaced: a path through it is what
/// was asked for.
pub fn put(root: &mut Value, path: &str, value: Value) {
    let mut node = root;
    for name in path.split('.') {
        if !node.is_object() {
            *node = Value::Object(Map::new());
        }
        node = node.as_object_mut().expect("just made one").entry(name).or_insert(Value::Null);
    }
    *node = value;
}

/// Two writes at once would each read the file, change their own key and
/// write it back, and the second would undo the first.
static WRITING: Mutex<()> = Mutex::new(());

pub fn write(key: Key, value: Value) -> Result<(), String> {
    let path = path(key.file).ok_or("no home directory to keep settings in")?;
    write_at(&path, key.path, value)
}

fn write_at(file: &Path, path: &str, value: Value) -> Result<(), String> {
    let _one_at_a_time = WRITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut root = match std::fs::read_to_string(file) {
        Ok(text) if text.trim().is_empty() => Value::Null,
        // A file somebody broke by hand is still theirs. Written over, the
        // one key set here would be all that was left of it.
        Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{} is not valid JSON: {error}", file.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => return Err(format!("cannot read {}: {error}", file.display())),
    };
    put(&mut root, path, value);

    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("cannot make {}: {error}", parent.display()))?;
    }
    // Beside it and then moved over it: a shell killed mid-write leaves the
    // old file, not the first half of the new one.
    let beside = file.with_extension("json.new");
    std::fs::write(&beside, indented(&root)).map_err(|error| format!("cannot write {}: {error}", beside.display()))?;
    std::fs::rename(&beside, file).map_err(|error| format!("cannot replace {}: {error}", file.display()))
}

/// Four spaces, which is how the QML shell writes the same file: the two
/// take turns at it, and a file that was reformatted every time the other
/// one saved would be unreadable in a diff.
fn indented(root: &Value) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    root.serialize(&mut serializer).expect("a JSON value always serialises");
    out.push(b'\n');
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A file of its own under the system's temporary directory, gone when
    /// the test is.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("cae-config-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Scratch(dir.join("caelestia").join("shell.json"))
        }

        fn holding(name: &str, text: &str) -> Scratch {
            let scratch = Scratch::new(name);
            std::fs::create_dir_all(scratch.0.parent().unwrap()).unwrap();
            std::fs::write(&scratch.0, text).unwrap();
            scratch
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap().parent().unwrap());
        }
    }

    #[test]
    fn only_a_file_with_something_unreadable_in_it_is_complained_about() {
        // Each name is one test's own: two `Scratch`es of the same name in
        // the same process are the same directory, and the first to finish
        // takes it away from the other.
        let unreadable = Scratch::holding("complaint-unreadable", "{ \"bar\": }");
        let why = complaint_at(&unreadable.0).expect("a file that is not JSON is worth saying");
        assert!(why.starts_with("shell.json: "), "the complaint names the file: {why}");

        assert_eq!(complaint_at(&Scratch::holding("complaint-fine", "{}").0), None);
        assert_eq!(complaint_at(&Scratch::holding("complaint-blank", "  \n").0), None, "an empty file is a default, not a fault");
        assert_eq!(complaint_at(&Scratch::new("complaint-absent").0), None, "a file nobody has written is not a complaint");
    }

    #[test]
    fn a_key_is_found_by_its_dotted_path() {
        let root = json!({"bar": {"status": {"showAudio": true}}});
        assert_eq!(lookup(&root, "bar.status.showAudio"), Some(&json!(true)));
        assert_eq!(lookup(&root, "bar.status.showWifi"), None);
        assert_eq!(lookup(&root, "bar.status.showAudio.deeper"), None);
        assert_eq!(lookup(&Value::Null, "bar"), None);
    }

    #[test]
    fn setting_a_key_makes_the_objects_on_the_way_to_it() {
        let mut root = Value::Null;
        put(&mut root, "dashboard.performance.showGpu", json!(false));
        assert_eq!(root, json!({"dashboard": {"performance": {"showGpu": false}}}));
    }

    #[test]
    fn writing_one_key_leaves_every_other_alone() {
        let scratch = Scratch::holding("others", r#"{"bar": {"entries": [{"id": "clock"}]}, "unknownToUs": 7}"#);
        write_at(&scratch.0, "bar.clock.showDate", json!(true)).unwrap();

        let after = read_at(&scratch.0);
        assert_eq!(after["bar"]["clock"]["showDate"], json!(true));
        assert_eq!(after["bar"]["entries"], json!([{"id": "clock"}]), "a neighbour was lost");
        assert_eq!(after["unknownToUs"], json!(7), "a key this shell has never heard of was lost");
    }

    #[test]
    fn a_file_that_is_not_there_is_made() {
        let scratch = Scratch::new("missing");
        write_at(&scratch.0, "launcher.maxShown", json!(9)).unwrap();
        assert_eq!(read_at(&scratch.0), json!({"launcher": {"maxShown": 9}}));
    }

    /// The case that matters most. The file is somebody's hand-edited config
    /// with a comma missing; "fixing" it by writing the one key we know about
    /// would throw the rest of it away.
    #[test]
    fn a_file_that_does_not_parse_is_refused_and_not_written_over() {
        let broken = r#"{"bar": {"persistent": true} "launcher": {}}"#;
        let scratch = Scratch::holding("broken", broken);

        let refused = write_at(&scratch.0, "bar.persistent", json!(false));
        assert!(refused.is_err(), "a broken file was written over");
        assert_eq!(std::fs::read_to_string(&scratch.0).unwrap(), broken);
    }

    #[test]
    fn what_is_written_is_indented_the_way_the_other_shell_writes_it() {
        let scratch = Scratch::new("format");
        write_at(&scratch.0, "border.thickness", json!(0)).unwrap();
        assert_eq!(std::fs::read_to_string(&scratch.0).unwrap(), "{\n    \"border\": {\n        \"thickness\": 0\n    }\n}\n");
    }

    #[test]
    fn a_snapshot_changed_in_memory_says_what_the_file_will() {
        let mut snapshot = Snapshot::default();
        snapshot.set(prefs("barShowGpu"), json!(false));
        snapshot.set(shell("bar.status.showAudio"), json!(true));
        assert_eq!(snapshot.get(prefs("barShowGpu")), Some(&json!(false)));
        assert_eq!(snapshot.get(shell("bar.status.showAudio")), Some(&json!(true)));
        assert_eq!(snapshot.get(shell("barShowGpu")), None, "the two files are not one namespace");
    }
}
