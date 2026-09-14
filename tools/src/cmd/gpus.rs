//! `caelestia-tools gpus` — what the per-app GPU picker chooses from.
//!
//! One entry per render node: a short marketing name, the kernel driver, the
//! PCI slot and the node path. The slot is the stable key the shell stores
//! per app; the driver decides which offload variables the launcher injects.

use redcommon::json::{self, Json};

pub fn run() -> i32 {
    let mut gpus = Vec::new();

    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        println!("[]");
        return 0;
    };
    let mut nodes: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("renderD"))
        })
        .collect();
    nodes.sort();

    for node in nodes {
        let Some(basename) = node.file_name().and_then(|n| n.to_str()) else { continue };
        let Ok(uevent) = std::fs::read_to_string(node.join("device/uevent")) else {
            continue; // a node without a device behind it is not a GPU we can use
        };

        let mut slot = String::new();
        let mut driver = String::new();
        for line in uevent.lines() {
            match line.split_once('=') {
                Some(("PCI_SLOT_NAME", value)) => slot = value.trim().to_string(),
                Some(("DRIVER", value)) => driver = value.trim().to_string(),
                _ => {}
            }
        }

        let name = if slot.is_empty() {
            basename.to_string()
        } else {
            short_name(&slot)
        };

        gpus.push(json::obj([
            ("name", json::s(name)),
            ("driver", json::s(driver)),
            ("slot", json::s(slot)),
            ("node", json::s(format!("/dev/dri/{basename}"))),
        ]));
    }

    println!("{}", Json::Arr(gpus).dump());
    0
}

/// lspci describes a card at length; the picker has room for a name. Prefer
/// the last bracketed part, which is the model ("Radeon 680M"), over the
/// vendor tag that comes first ("AMD/ATI").
fn short_name(slot: &str) -> String {
    let Ok(output) = std::process::Command::new("lspci").args(["-s", slot]).output() else {
        return slot.to_string();
    };
    if !output.status.success() {
        return slot.to_string();
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if line.is_empty() {
        return slot.to_string();
    }

    let described = line.split_once(": ").map(|(_, rest)| rest).unwrap_or(&line);
    let described = strip_revision(described);

    match last_bracketed(described) {
        Some(model) => model.to_string(),
        None => described.to_string(),
    }
}

/// Drop a trailing " (rev c5)".
fn strip_revision(text: &str) -> &str {
    match text.rfind(" (rev ") {
        Some(at) if text.ends_with(')') => text[..at].trim_end(),
        _ => text,
    }
}

fn last_bracketed(text: &str) -> Option<&str> {
    let close = text.rfind(']')?;
    let open = text[..close].rfind('[')?;
    Some(&text[open + 1..close])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_model_over_the_vendor_tag() {
        let line = "c1:00.0 VGA compatible controller: Advanced Micro Devices, Inc. [AMD/ATI] Rembrandt [Radeon 680M] (rev c5)";
        let described = strip_revision(line.split_once(": ").unwrap().1);
        assert_eq!(last_bracketed(described), Some("Radeon 680M"));
    }

    #[test]
    fn a_card_with_no_brackets_keeps_its_description() {
        let described = strip_revision("Intel Corporation HD Graphics 620 (rev 02)");
        assert_eq!(described, "Intel Corporation HD Graphics 620");
        assert_eq!(last_bracketed(described), None);
    }

    #[test]
    fn a_revision_only_goes_when_it_is_one() {
        assert_eq!(strip_revision("Thing (rev 01)"), "Thing");
        assert_eq!(strip_revision("Thing (not a rev)"), "Thing (not a rev)");
        assert_eq!(strip_revision("Thing"), "Thing");
    }
}
