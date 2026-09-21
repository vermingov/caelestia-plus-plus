//! Argument parsing for the subcommands this front-end implements.
//!
//! The rule that makes the whole design safe: anything not understood here —
//! an unknown subcommand, an unknown flag, a missing argument — parses as
//! `None` and the call goes to the Python CLI untouched. A new option landing
//! there keeps working the day it is added; the worst this binary can do is
//! not be used.

use crate::cmd;

pub enum Command {
    Shell(cmd::shell::Args),
    Toggle(String),
    Clipboard { delete: bool },
    Emoji { picker: bool },
    Screenshot(cmd::screenshot::Args),
    Record(cmd::record::Args),
    SchemeGet(cmd::scheme::GetArgs),
    SchemeList(cmd::scheme::ListArgs),
    SchemeSet(cmd::scheme::SetArgs),
    Wallpaper(cmd::wallpaper::Args),
}

pub fn parse(argv: &[String]) -> Option<Command> {
    let (name, rest) = argv.split_first()?;
    match name.as_str() {
        "shell" => parse_shell(rest),
        "toggle" => parse_toggle(rest),
        "clipboard" => parse_clipboard(rest),
        "emoji" => parse_emoji(rest),
        "screenshot" => parse_screenshot(rest),
        "record" => parse_record(rest),
        "scheme" => parse_scheme(rest),
        "wallpaper" => parse_wallpaper(rest),
        _ => None,
    }
}

fn parse_shell(argv: &[String]) -> Option<Command> {
    let mut args = cmd::shell::Args {
        message: Vec::new(),
        daemon: false,
        show: false,
        log: false,
        kill: false,
        log_rules: None,
    };
    let mut rest = argv.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-d" | "--daemon" => args.daemon = true,
            "-s" | "--show" => args.show = true,
            "-l" | "--log" => args.log = true,
            "-k" | "--kill" => args.kill = true,
            "--log-rules" => args.log_rules = Some(rest.next()?.clone()),
            // The message is positional and may be several words, but a flag
            // this parser does not know means the Python CLI should have it.
            other if other.starts_with('-') => return None,
            other => args.message.push(other.to_string()),
        }
    }
    Some(Command::Shell(args))
}

fn parse_toggle(argv: &[String]) -> Option<Command> {
    match argv {
        [workspace] if !workspace.starts_with('-') => Some(Command::Toggle(workspace.clone())),
        _ => None,
    }
}

/// argparse lets short flags bundle: `-nfv` is `-n -f -v`, and the launcher
/// calls `scheme get -nfv`. Expanded here so each matcher only ever sees one
/// flag at a time. Only used for commands whose short flags never take a
/// value — where one does, a bundle is ambiguous and falls through instead.
fn expand_short_bundles(argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for arg in argv {
        let bundled = arg.len() > 2
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg[1..].chars().all(|c| c.is_ascii_alphabetic());
        if bundled {
            out.extend(arg[1..].chars().map(|c| format!("-{c}")));
        } else {
            out.push(arg.clone());
        }
    }
    out
}

fn parse_clipboard(argv: &[String]) -> Option<Command> {
    let mut delete = false;
    for arg in &expand_short_bundles(argv) {
        match arg.as_str() {
            "-d" | "--delete" => delete = true,
            _ => return None,
        }
    }
    Some(Command::Clipboard { delete })
}

fn parse_emoji(argv: &[String]) -> Option<Command> {
    let mut picker = false;
    for arg in &expand_short_bundles(argv) {
        match arg.as_str() {
            "-p" | "--picker" => picker = true,
            // Fetching rewrites the data file from two remote sources; that
            // belongs to the CLI that owns the file.
            _ => return None,
        }
    }
    Some(Command::Emoji { picker })
}

fn parse_screenshot(argv: &[String]) -> Option<Command> {
    let mut args = cmd::screenshot::Args {
        region: None,
        freeze: false,
        clipboard: false,
    };
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "-r" | "--region" => {
                // Optional value: `-r` alone means "let the user select",
                // which is what argparse's const does on the other side.
                let value = argv.get(i + 1).filter(|v| !v.starts_with('-'));
                match value {
                    Some(region) => {
                        args.region = Some(region.clone());
                        i += 1;
                    }
                    None => args.region = Some("slurp".to_string()),
                }
            }
            "-f" | "--freeze" => args.freeze = true,
            "-c" | "--clipboard" => args.clipboard = true,
            _ => return None,
        }
        i += 1;
    }
    Some(Command::Screenshot(args))
}

fn parse_record(argv: &[String]) -> Option<Command> {
    let mut args = cmd::record::Args {
        region: None,
        sound: false,
        pause: false,
        clipboard: false,
    };
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "-r" | "--region" => {
                let value = argv.get(i + 1).filter(|v| !v.starts_with('-'));
                match value {
                    Some(region) => {
                        args.region = Some(region.clone());
                        i += 1;
                    }
                    None => args.region = Some("slurp".to_string()),
                }
            }
            "-s" | "--sound" => args.sound = true,
            "-p" | "--pause" => args.pause = true,
            "-c" | "--clipboard" => args.clipboard = true,
            _ => return None,
        }
        i += 1;
    }
    Some(Command::Record(args))
}

/// `wallpaper`. Two of its options take a value or not — argparse's
/// `nargs="?"` — so a following word is the value only when it does not look
/// like another option.
fn parse_wallpaper(argv: &[String]) -> Option<Command> {
    let mut args = cmd::wallpaper::Args {
        print: None,
        random: None,
        file: None,
        no_filter: false,
        threshold: 0.8,
        no_smart: false,
    };
    let flags = expand_short_bundles(argv);
    let mut i = 0;
    while i < flags.len() {
        let optional_value = |i: &mut usize| -> Option<String> {
            let next = flags.get(*i + 1)?;
            (!next.starts_with('-')).then(|| {
                *i += 1;
                next.clone()
            })
        };
        match flags[i].as_str() {
            "-p" | "--print" => args.print = Some(optional_value(&mut i)),
            "-r" | "--random" => args.random = Some(optional_value(&mut i)),
            "-f" | "--file" => {
                i += 1;
                args.file = Some(flags.get(i)?.clone());
            }
            "-t" | "--threshold" => {
                i += 1;
                args.threshold = flags.get(i)?.parse().ok()?;
            }
            "-n" | "--no-filter" => args.no_filter = true,
            "-N" | "--no-smart" => args.no_smart = true,
            _ => return None,
        }
        i += 1;
    }
    Some(Command::Wallpaper(args))
}

/// `scheme` has subcommands of its own: `get` and `list` read files, `set`
/// regenerates the palette and pushes it out to every themed application.
fn parse_scheme(argv: &[String]) -> Option<Command> {
    let (which, flags) = argv.split_first()?;
    let flags = &expand_short_bundles(flags);
    match which.as_str() {
        "get" => {
            let mut args = cmd::scheme::GetArgs {
                name: false,
                flavour: false,
                mode: false,
                variant: false,
            };
            for flag in flags {
                match flag.as_str() {
                    "-n" | "--name" => args.name = true,
                    "-f" | "--flavour" => args.flavour = true,
                    "-m" | "--mode" => args.mode = true,
                    "-v" | "--variant" => args.variant = true,
                    _ => return None,
                }
            }
            // Bare `scheme get` prints a formatted block; that is the other
            // CLI's wording to own, and nothing calls it on a hot path.
            args.any().then_some(Command::SchemeGet(args))
        }
        "list" => {
            let mut args = cmd::scheme::ListArgs {
                names: false,
                flavours: false,
                modes: false,
                variants: false,
            };
            for flag in flags {
                match flag.as_str() {
                    "-n" | "--names" => args.names = true,
                    "-f" | "--flavours" => args.flavours = true,
                    "-m" | "--modes" => args.modes = true,
                    "-v" | "--variants" => args.variants = true,
                    _ => return None,
                }
            }
            Some(Command::SchemeList(args))
        }
        "set" => {
            let mut args = cmd::scheme::SetArgs {
                name: None,
                flavour: None,
                mode: None,
                variant: None,
                random: false,
                notify: false,
            };
            let mut rest = flags.iter();
            while let Some(flag) = rest.next() {
                match flag.as_str() {
                    "-n" | "--name" => args.name = Some(rest.next()?.clone()),
                    "-f" | "--flavour" => args.flavour = Some(rest.next()?.clone()),
                    "-m" | "--mode" => args.mode = Some(rest.next()?.clone()),
                    "-v" | "--variant" => args.variant = Some(rest.next()?.clone()),
                    "-r" | "--random" => args.random = true,
                    "--notify" => args.notify = true,
                    _ => return None,
                }
            }
            Some(Command::SchemeSet(args))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn is_ours(words: &[&str]) -> bool {
        parse(&argv(words)).is_some()
    }

    #[test]
    fn the_keybinds_in_this_config_are_all_handled_here() {
        for words in [
            vec!["toggle", "specialws"],
            vec!["toggle", "sysmon"],
            vec!["screenshot"],
            vec!["clipboard"],
            vec!["clipboard", "-d"],
            vec!["emoji", "-p"],
            vec!["shell", "-d"],
            vec!["shell", "ipc", "call", "picker", "open"],
            vec!["record"],
            vec!["record", "-s"],
            vec!["record", "-p"],
            vec!["record", "-r"],
            vec!["scheme", "get", "-nfv"],
            vec!["scheme", "list"],
            vec!["scheme", "list", "-n"],
            vec!["scheme", "set", "-n", "catppuccin"],
            vec!["scheme", "set", "-n", "dynamic", "-f", "hard", "-m", "dark", "-v", "vibrant"],
            vec!["scheme", "set", "-r"],
            vec!["scheme", "set", "--notify", "--mode", "light"],
            vec!["wallpaper"],
            vec!["wallpaper", "-f", "/tmp/x.png"],
            vec!["wallpaper", "-r"],
            vec!["wallpaper", "-r", "/tmp/walls"],
            vec!["wallpaper", "-p"],
            vec!["wallpaper", "-rN"],
            vec!["wallpaper", "--random", "--no-filter", "--threshold", "0.5"],
        ] {
            assert!(is_ours(&words), "{words:?} should not need the Python CLI");
        }
    }

    #[test]
    fn anything_unknown_goes_to_the_python_cli() {
        for words in [
            vec!["scheme", "set", "--what"],
            vec!["scheme", "set", "-n"],  // the name is missing
            vec!["scheme", "get"],
            vec!["scheme", "get", "--colours"],
            vec!["wallpaper", "--what"],
            vec!["wallpaper", "-f"],  // the path is missing
            vec!["install"],
            vec!["--version"],
            vec!["-h"],
            vec!["emoji", "-f"],          // fetching is not ours
            vec!["toggle"],               // missing the workspace
            vec!["toggle", "a", "b"],     // more than one
            vec!["shell", "--new-flag"],  // an option added on the other side
            vec!["clipboard", "--what"],
            vec!["screenshot", "--what"],
            vec!["record", "--what"],
        ] {
            assert!(!is_ours(&words), "{words:?} should fall through");
        }
        assert!(parse(&[]).is_none(), "no subcommand at all");
    }

    #[test]
    fn short_flags_may_be_bundled_the_way_argparse_allows() {
        let Some(Command::SchemeGet(args)) = parse(&argv(&["scheme", "get", "-nfv"])) else {
            panic!("the launcher's own call did not parse")
        };
        assert!(args.name && args.flavour && args.variant);
        assert!(!args.mode, "only what was asked for");

        let Some(Command::SchemeGet(spread)) = parse(&argv(&["scheme", "get", "-n", "-f", "-v"])) else {
            panic!("not parsed")
        };
        assert_eq!(
            (spread.name, spread.flavour, spread.mode, spread.variant),
            (args.name, args.flavour, args.mode, args.variant),
            "bundled and spread mean the same thing"
        );

        let Some(Command::Clipboard { delete }) = parse(&argv(&["clipboard", "-d"])) else {
            panic!("not parsed")
        };
        assert!(delete);
    }

    #[test]
    fn a_region_takes_its_value_or_stands_for_the_picker() {
        let Some(Command::Screenshot(args)) = parse(&argv(&["screenshot", "-r"])) else {
            panic!("not parsed")
        };
        assert_eq!(args.region.as_deref(), Some("slurp"));

        let Some(Command::Screenshot(args)) = parse(&argv(&["screenshot", "-r", "10x10+0+0"])) else {
            panic!("not parsed")
        };
        assert_eq!(args.region.as_deref(), Some("10x10+0+0"));

        let Some(Command::Screenshot(args)) = parse(&argv(&["screenshot", "-r", "-f"])) else {
            panic!("not parsed")
        };
        assert_eq!(args.region.as_deref(), Some("slurp"), "a flag is not a region");
        assert!(args.freeze);
    }

    #[test]
    fn a_shell_message_keeps_its_words_in_order() {
        let Some(Command::Shell(args)) = parse(&argv(&["shell", "ipc", "call", "picker", "open"])) else {
            panic!("not parsed")
        };
        assert_eq!(args.message, ["ipc", "call", "picker", "open"]);
        assert!(!args.daemon);

        let Some(Command::Shell(args)) = parse(&argv(&["shell", "--log-rules", "*=false"])) else {
            panic!("not parsed")
        };
        assert_eq!(args.log_rules.as_deref(), Some("*=false"));
        assert!(parse(&argv(&["shell", "--log-rules"])).is_none(), "a flag with no value");
    }
}
