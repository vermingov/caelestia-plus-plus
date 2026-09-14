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

fn parse_clipboard(argv: &[String]) -> Option<Command> {
    let mut delete = false;
    for arg in argv {
        match arg.as_str() {
            "-d" | "--delete" => delete = true,
            _ => return None,
        }
    }
    Some(Command::Clipboard { delete })
}

fn parse_emoji(argv: &[String]) -> Option<Command> {
    let mut picker = false;
    for arg in argv {
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
        ] {
            assert!(is_ours(&words), "{words:?} should not need the Python CLI");
        }
    }

    #[test]
    fn anything_unknown_goes_to_the_python_cli() {
        for words in [
            vec!["scheme", "get"],
            vec!["wallpaper", "-f", "/tmp/x.png"],
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
