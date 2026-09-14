//! `caelestia scheme get` and `caelestia scheme list` — the two the launcher
//! calls every time its scheme picker opens.
//!
//! Both are file reads dressed up as a command, and both were costing an
//! interpreter start plus a walk of every scheme on disk: 142 ms warm, 414 ms
//! cold, twice, before the picker could draw anything.
//!
//! `scheme set` is not here. Changing a scheme regenerates colours through
//! the Material pipeline, which still lives in the Python CLI.

use redcommon::json::Json;

use crate::scheme;

pub struct GetArgs {
    pub name: bool,
    pub flavour: bool,
    pub mode: bool,
    pub variant: bool,
}

pub struct ListArgs {
    pub names: bool,
    pub flavours: bool,
    pub modes: bool,
    pub variants: bool,
}

impl GetArgs {
    pub fn any(&self) -> bool {
        self.name || self.flavour || self.mode || self.variant
    }
}

impl ListArgs {
    pub fn any(&self) -> bool {
        self.names || self.flavours || self.modes || self.variants
    }

    fn count(&self) -> usize {
        [self.names, self.flavours, self.modes, self.variants]
            .iter()
            .filter(|on| **on)
            .count()
    }
}

pub fn get(args: &GetArgs) -> i32 {
    let Some(current) = scheme::current() else {
        eprintln!("caelestia: no scheme has been set yet");
        return 1;
    };
    // Printed in the order the fields are declared, not the order the flags
    // were given — the same as the Python CLI, whose callers split on lines.
    for (wanted, value) in [
        (args.name, &current.name),
        (args.flavour, &current.flavour),
        (args.mode, &current.mode),
        (args.variant, &current.variant),
    ] {
        if wanted {
            println!("{value}");
        }
    }
    0
}

pub fn list(args: &ListArgs) -> i32 {
    if !args.any() {
        return list_everything();
    }

    // With more than one list asked for, each gets a label so the output is
    // still readable; with one, it is a plain list callers can split.
    let labelled = args.count() > 1;
    let sections: [(bool, &str, Vec<String>); 4] = [
        (args.names, "Names:", scheme::names()),
        (args.flavours, "Flavours:", current_flavours()),
        (args.modes, "Modes:", current_modes()),
        (
            args.variants,
            "Variants:",
            scheme::VARIANTS.iter().map(|v| v.to_string()).collect(),
        ),
    ];

    for (wanted, label, values) in sections {
        if !wanted {
            continue;
        }
        if labelled {
            println!("{label} {}", values.join(" "));
        } else {
            for value in values {
                println!("{value}");
            }
        }
    }
    0
}

/// Every scheme, flavour and colour as one JSON object. Falls back to the
/// Python CLI when a generated palette is not in the cache, because working
/// it out is that CLI's job.
fn list_everything() -> i32 {
    match scheme::all_colours() {
        Some(Json::Obj(map)) => {
            println!("{}", Json::Obj(map).dump());
            0
        }
        _ => crate::hand_over(&["scheme".to_string(), "list".to_string()]),
    }
}

fn current_flavours() -> Vec<String> {
    match scheme::current() {
        Some(current) => scheme::flavours(&current.name),
        None => Vec::new(),
    }
}

fn current_modes() -> Vec<String> {
    match scheme::current() {
        Some(current) => scheme::modes(&current.name, &current.flavour),
        None => Vec::new(),
    }
}
