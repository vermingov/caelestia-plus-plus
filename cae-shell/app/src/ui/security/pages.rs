//! The four pages: what the guards are doing, what they remember, and what
//! starts itself.

use cae_core::guards::Detail;
use cae_core::startup;
use gpui::{AnyElement, App, AppContext, Context, IntoElement, SharedString, Styled, div, prelude::*, px};
use serde_json::Value;

use super::pane::{Pane, Reach};
use super::Tab;
use crate::theme;
use crate::ui::controls::{chip, round, switch};
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// How a guard is doing, in one line.
fn how(daemon: Option<&Detail>) -> (&'static str, String) {
    match daemon {
        None => ("gpp_maybe", "Not set up".to_string()),
        Some(daemon) if !daemon.connected => ("gpp_maybe", "Not running".to_string()),
        Some(daemon) if !daemon.enabled => ("gpp_bad", "Switched off".to_string()),
        Some(daemon) => {
            let rules = daemon.rules.len();
            ("verified_user", format!("On, {rules} rule{}", if rules == 1 { "" } else { "s" }))
        }
    }
}

fn daemon<'a>(guards: &'a [Detail], name: &str) -> Option<&'a Detail> {
    guards.iter().find(|daemon| daemon.name == name)
}

/// A heading above a list.
fn heading(text: impl Into<SharedString>) -> gpui::Div {
    rsx! { <div class="flex-none pt-[6px] pb-[8px]" text_size={px(12.)} text_color={theme::text_faint()}>{text.into()}</div> }
}

/// One row of a list: a mark, what it is, what it is doing, and whatever
/// goes at the end.
fn row(symbol: &'static str, name: impl Into<SharedString>, said: impl Into<SharedString>, end: Vec<AnyElement>) -> gpui::Div {
    rsx! {
        <div
            class="flex flex-none items-center gap-[12px] p-[12px]"
            rounded={px(12.)}
            bg={theme::white(0.04)}
        >
            <div
                class="flex flex-none items-center justify-center size-[34px] rounded-full"
                bg={theme::white(0.07)}
                text_color={theme::text_dim()}
            >
                {glyph(symbol, px(18.))}
            </div>
            <div class="flex flex-col flex-1 min-w-[0px] gap-[1px]">
                <div class="flex-none truncate" text_size={px(13.)}>{name.into()}</div>
                <div class="flex-none truncate" text_size={px(11.5)} text_color={theme::text_dim()}>{said.into()}</div>
            </div>
            {...end}
        </div>
    }
}

/// The first page: how things stand, and a way into each of the others.
pub fn overview(reach: &Reach, cx: &mut Context<Pane>) -> AnyElement {
    let guards = reach.feeds.guards.read(cx).value.clone();
    let starts = reach.starts.read(cx).len();
    let waiting: usize = guards.iter().map(|daemon| daemon.pending.len()).sum();
    let both_on = ["protection", "firewall"]
        .iter()
        .all(|name| daemon(&guards, name).is_some_and(|daemon| daemon.connected && daemon.enabled));

    let (mark, headline, said) = if waiting > 0 {
        ("gpp_bad", "Action needed", format!("{waiting} waiting for a word from you"))
    } else if both_on {
        ("verified_user", "Protected", "Both guards are on".to_string())
    } else {
        ("gpp_maybe", "Worth a look", "One of the guards is not running".to_string())
    };

    let page = |tab: Tab| {
        move |pane: &mut Pane, _: &gpui::ClickEvent, _: &mut gpui::Window, cx: &mut Context<Pane>| {
            pane.turn(tab, cx);
        }
    };

    rsx! {
        <div id="overview" class="flex flex-col flex-1 min-h-[0px] gap-[8px] overflow-y-scroll">
            <div
                class="flex flex-none items-center gap-[16px] p-[18px]"
                rounded={px(16.)}
                bg={theme::white(0.05)}
            >
                <div
                    class="flex flex-none items-center justify-center size-[52px] rounded-full"
                    bg={if waiting > 0 || !both_on { theme::accent() } else { theme::white(0.1) }}
                    text_color={if waiting > 0 || !both_on { theme::on_accent() } else { theme::text() }}
                >
                    {glyph(mark, px(28.))}
                </div>
                <div class="flex flex-col flex-1 min-w-[0px] gap-[2px]">
                    <div class="flex-none" text_size={px(17.)}>{headline}</div>
                    <div class="flex-none" text_size={px(12.5)} text_color={theme::text_dim()}>{said}</div>
                </div>
            </div>

            {...["protection", "firewall"].into_iter().map(|name| {
                let (mark, said) = how(daemon(&guards, name));
                let tab = if name == "protection" { Tab::Protection } else { Tab::Firewall };
                row(mark, if name == "protection" { "Protection" } else { "Firewall" }, said, vec![
                    rsx! {
                        <div base={chip("Open", false)} id={name} onClick={cx.listener(page(tab))} />
                    }
                    .into_any_element(),
                ])
                .into_any_element()
            }).collect::<Vec<_>>()}

            {row("rocket_launch", "Startup", format!("{starts} start with the desktop"), vec![
                rsx! {
                    <div base={chip("Open", false)} id="open-startup" onClick={cx.listener(page(Tab::Startup))} />
                }
                .into_any_element(),
            ])}
        </div>
    }
    .into_any_element()
}

/// One guard: whether it is on, and everything it remembers.
pub fn guard(tab: Tab, reach: &Reach, cx: &mut Context<Pane>) -> AnyElement {
    let Some(name) = tab.daemon() else { return div().into_any_element() };
    let guards = reach.feeds.guards.read(cx).value.clone();
    let here = daemon(&guards, name).cloned();
    let watcher = reach.feeds.watcher.clone();

    let Some(here) = here.filter(|daemon| daemon.connected) else {
        return rsx! {
            <div class="flex flex-col flex-1 items-center justify-center gap-[10px]" text_color={theme::text_dim()}>
                {glyph("gpp_maybe", px(34.))}
                <div text_size={px(13.)}>{format!("The {name} daemon is not running")}</div>
                <div text_size={px(11.5)} text_color={theme::text_faint()}>
                    "Its privileged half installs from the settings"
                </div>
            </div>
        }
        .into_any_element();
    };

    let (enabled, rules) = (here.enabled, here.rules.clone());
    let which = name.to_string();
    let switching = watcher.clone();
    let head = row(
        if enabled { "verified_user" } else { "gpp_bad" },
        if name == "protection" { "Exploit guard" } else { "Firewall" },
        if enabled { "Watching" } else { "Not watching" },
        vec![
            rsx! {
                <div
                    id="enabled"
                    class="flex-none cursor-pointer"
                    onClick={move |_, _: &mut gpui::Window, cx: &mut App| {
                        let (switching, which) = (switching.clone(), which.clone());
                        cx.background_spawn(async move { switching.set_enabled(&which, !enabled) }).detach();
                    }}
                >
                    {switch(enabled)}
                </div>
            }
            .into_any_element(),
        ],
    );

    let rows: Vec<AnyElement> = rules
        .iter()
        .enumerate()
        .map(|(at, rule)| {
            let word = |key: &str| rule.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
            let (exe, action) = (word("exe"), word("action"));
            let shown = if word("name").is_empty() { exe.rsplit('/').next().unwrap_or_default().to_string() } else { word("name") };
            let allowed = action == "allow";
            let (forgetting, which) = (watcher.clone(), name.to_string());
            let (turning, turning_which, turning_exe, turning_name) = (watcher.clone(), name.to_string(), exe.clone(), shown.clone());
            row(
                if allowed { "check_circle" } else { "block" },
                shown.clone(),
                exe.clone(),
                vec![
                    rsx! {
                        <div
                            base={chip(if allowed { "Allowed" } else { "Blocked" }, allowed)}
                            id={("rule", at)}
                            onClick={move |_, _: &mut gpui::Window, cx: &mut App| {
                                let (turning, which, exe, name) = (turning.clone(), turning_which.clone(), turning_exe.clone(), turning_name.clone());
                                let other = if allowed { "deny" } else { "allow" };
                                cx.background_spawn(async move { turning.set_rule(&which, &exe, other, &name) }).detach();
                            }}
                        />
                    }
                    .into_any_element(),
                    rsx! {
                        <div
                            base={round("delete", false)}
                            id={("forget", at)}
                            onClick={move |_, _: &mut gpui::Window, cx: &mut App| {
                                let (forgetting, which, exe) = (forgetting.clone(), which.clone(), exe.clone());
                                cx.background_spawn(async move { forgetting.delete_rule(&which, &exe) }).detach();
                            }}
                        />
                    }
                    .into_any_element(),
                ],
            )
            .into_any_element()
        })
        .collect();

    rsx! {
        <div id="guard" class="flex flex-col flex-1 min-h-[0px] gap-[8px] overflow-y-scroll">
            {head}
            {heading(format!("{} remembered", rows.len()))}
            {...nothing_when(rows.is_empty())}
            {...rows}
        </div>
    }
    .into_any_element()
}

/// What a list says when there is nothing in it.
fn nothing_when(empty: bool) -> Option<gpui::Div> {
    empty.then(|| {
        rsx! {
            <div class="flex-none py-[14px] text-center" text_size={px(12.)} text_color={theme::text_faint()}>
                "Nothing yet"
            </div>
        }
    })
}

/// What starts itself when the desktop does.
pub fn startup(reach: &Reach, cx: &mut Context<Pane>) -> AnyElement {
    let entries = reach.starts.read(cx).clone();
    let rows: Vec<AnyElement> = entries
        .iter()
        .enumerate()
        .map(|(at, entry)| {
            let (source, key, on) = (entry.source.clone(), entry.key.clone(), entry.enabled);
            let (removing_source, removing_key) = (source.clone(), key.clone());
            row(
                if entry.source == "systemd" { "settings_applications" } else { "rocket_launch" },
                entry.name.clone(),
                entry.exec.clone(),
                vec![
                    rsx! {
                        <div
                            id={("start", at)}
                            class="flex-none cursor-pointer"
                            onClick={move |_, _: &mut gpui::Window, cx: &mut App| {
                                let (source, key) = (source.clone(), key.clone());
                                cx.background_spawn(async move { startup::set_enabled(&source, &key, !on) }).detach();
                            }}
                        >
                            {switch(on)}
                        </div>
                    }
                    .into_any_element(),
                    rsx! {
                        <div
                            base={round("delete", false)}
                            id={("drop", at)}
                            onClick={move |_, _: &mut gpui::Window, cx: &mut App| {
                                let (source, key) = (removing_source.clone(), removing_key.clone());
                                cx.background_spawn(async move { startup::remove(&source, &key) }).detach();
                            }}
                        />
                    }
                    .into_any_element(),
                ],
            )
            .into_any_element()
        })
        .collect();

    rsx! {
        <div id="startup" class="flex flex-col flex-1 min-h-[0px] gap-[8px] overflow-y-scroll">
            {heading(format!("{} start with the desktop", rows.len()))}
            {...nothing_when(rows.is_empty())}
            {...rows}
        </div>
    }
    .into_any_element()
}
