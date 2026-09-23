//! The system scan: what the shell needs from the machine, whether it is
//! there, and the offer to put it right.
//!
//! The findings are `cae_core::checkup`'s. What is here is the three states
//! a fix passes through and nothing else: described, confirmed, running.
//! Nothing runs until the exact commands have been on screen and somebody
//! has pressed the button under them.

use std::time::Duration;

use cae_core::checkup::{self, Fix, Report, Row, Status};
use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui::{AppContext, Context, Div, IntoElement, Render, SharedString, Styled, Window, div, prelude::*, px};

use super::super::rows::{button, nothing, page, rule, section_title};
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// How much of a fix's output is kept on screen. Enough to see what it is
/// doing and where it stopped; the rest is on the clipboard.
const SHOWN_LINES: usize = 16;

/// A fix with no reachable password prompt hangs for ever. Long installs are
/// fine, an unanswerable prompt is not.
const GIVE_UP_AFTER: Duration = Duration::from_secs(600);

/// A fix that has been described and is waiting for a yes.
struct Staged {
    what: SharedString,
    fix: Fix,
}

/// A fix that is running, or has just finished.
struct Running {
    what: SharedString,
    root: bool,
    /// The wrapper process, so a stuck fix can be stopped.
    pid: Option<u32>,
    log: Vec<String>,
    /// What it exited with, once it has.
    ended: Option<i32>,
    stopped: bool,
}

/// A fix somebody has already been offered somewhere else — the startup
/// prompt — and wants to see the whole of. Left here for the page to pick up
/// when it opens, so that the prompt does not have to describe it twice.
#[derive(Default)]
pub struct Waiting(pub Option<(SharedString, Fix)>);

impl gpui::Global for Waiting {}

/// Leaves a fix for the page to stage the moment it opens.
pub fn hand_over(what: impl Into<SharedString>, fix: Fix, cx: &mut gpui::App) {
    cx.set_global(Waiting(Some((what.into(), fix))));
}

pub struct Checkup {
    /// None while the first scan is still out.
    report: Option<Report>,
    scanning: bool,
    at: SharedString,
    staged: Option<Staged>,
    running: Option<Running>,
}

impl Checkup {
    pub fn new(cx: &mut Context<Self>) -> Checkup {
        let handed = cx.default_global::<Waiting>().0.take();
        let staged = handed.map(|(what, fix)| Staged { what, fix });
        let mut page = Checkup { report: None, scanning: false, at: SharedString::default(), staged, running: None };
        page.scan(cx);
        page
    }

    fn scan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.notify();
        cx.spawn(async move |page, cx| {
            let report = cx.background_spawn(async { checkup::scan(checkup::Pace::Asked) }).await;
            let _ = page.update(cx, |page: &mut Checkup, cx| {
                page.at = crate::clock::now().1.into();
                page.report = Some(report);
                page.scanning = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Describes a fix and waits. Nothing has run yet at this point.
    fn stage(&mut self, what: impl Into<SharedString>, fix: Fix, cx: &mut Context<Self>) {
        if self.running.as_ref().is_some_and(|running| running.ended.is_none()) {
            return;
        }
        self.staged = Some(Staged { what: what.into(), fix });
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(staged) = self.staged.take() else { return };
        // The updater is a program of its own, with its own window: it is
        // not a line of shell to be streamed into a card.
        if staged.fix.updates_the_shell {
            cx.notify();
            return cx.background_spawn(async { cae_core::session::run(&["cae".to_string()]) }).detach();
        }
        self.running = Some(Running { what: staged.what, root: staged.fix.root, pid: None, log: Vec::new(), ended: None, stopped: false });
        cx.notify();

        let (lines, mut heard) = unbounded::<String>();
        let (pids, mut pid_heard) = unbounded::<u32>();
        cx.spawn(async move |page, cx| {
            while let Some(pid) = pid_heard.next().await {
                let _ = page.update(cx, |page: &mut Checkup, _| {
                    if let Some(running) = &mut page.running {
                        running.pid = Some(pid);
                    }
                });
            }
        })
        .detach();
        cx.spawn(async move |page, cx| {
            while let Some(line) = heard.next().await {
                let _ = page.update(cx, |page: &mut Checkup, cx| {
                    if let Some(running) = &mut page.running {
                        running.log.push(line);
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        let fix = staged.fix;
        cx.spawn(async move |page, cx| {
            let code = cx
                .background_spawn(async move {
                    checkup::run(&fix, |pid| drop(pids.unbounded_send(pid)), |line| drop(lines.unbounded_send(line)))
                })
                .await;
            let _ = page.update(cx, |page: &mut Checkup, cx| {
                if let Some(running) = &mut page.running {
                    running.ended = Some(code);
                }
                // Whatever it did, the answer is what the machine says now.
                page.scan(cx);
            });
        })
        .detach();

        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(GIVE_UP_AFTER).await;
            let _ = page.update(cx, |page: &mut Checkup, cx| page.stop(cx));
        })
        .detach();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(running) = &mut self.running else { return };
        if running.ended.is_some() {
            return;
        }
        running.stopped = true;
        let pid = running.pid;
        cx.notify();
        if let Some(pid) = pid {
            cx.background_spawn(async move { checkup::stop(pid) }).detach();
        }
    }

    fn put_away(&mut self, cx: &mut Context<Self>) {
        self.running = None;
        cx.notify();
    }

    fn copy_log(&mut self, cx: &mut Context<Self>) {
        let Some(running) = &self.running else { return };
        let text = running.log.join("\n");
        cx.background_spawn(async move {
            let _ = std::process::Command::new("wl-copy").arg(text).status();
        })
        .detach();
    }
}

/// The colour and the mark a finding wears.
fn mark(status: Status) -> (&'static str, gpui::Hsla) {
    match status {
        Status::Fail => ("error", theme::alert()),
        Status::Warn => ("warning", theme::warn()),
        Status::Info => ("info", theme::text_dim()),
        Status::Ok => ("check_circle", theme::text_faint()),
    }
}

/// One finding: what it is, what it means, and the offer to mend it.
fn finding(row: &Row, mend: Option<impl IntoElement>) -> Div {
    let (symbol, colour) = mark(row.status);
    rsx! {
        <div class="flex flex-none items-start gap-[12px] py-[10px]">
            <div class="flex-none pt-[1px]" text_color={colour}>{glyph(symbol, px(17.))}</div>
            <div class="flex flex-col flex-1 gap-[3px] min-w-[0px]">
                <div text_size={px(13.)} text_color={theme::text()}>{row.name.clone()}</div>
                <div text_size={px(11.5)} line_height={px(16.)} text_color={theme::text_faint()}>{row.detail.clone()}</div>
            </div>
            {...mend}
        </div>
    }
}

/// The block of commands a fix will run, as they are written.
fn commands(fix: &Fix) -> Div {
    rsx! {
        <div class="flex flex-col flex-none gap-[4px] p-[11px]" rounded={px(9.)} bg={theme::black(0.22)}>
            {for command in fix.commands.iter().cloned() {
                <div text_size={px(11.)} line_height={px(16.)} text_color={theme::text_dim()} font_features={theme::tabular()}>
                    {command}
                </div>
            }}
        </div>
    }
}

impl Render for Checkup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let report = self.report.clone();
        let problems = report.as_ref().map_or(0, Report::problems);
        let checks = report.as_ref().map_or(0, |report| report.rows.len());
        let missing = report.as_ref().map(Report::missing_packages).unwrap_or_default();
        let needs_agent = report.as_ref().is_some_and(Report::polkit_missing);
        let says: SharedString = if self.scanning && report.is_none() {
            "Looking over the machine".into()
        } else if problems > 0 {
            let needs = if problems == 1 { "needs" } else { "need" };
            format!("{problems} of {checks} checks {needs} attention").into()
        } else {
            format!("All {checks} checks pass").into()
        };
        let bundle = checkup::install_all(&missing);
        let busy = self.running.as_ref().is_some_and(|running| running.ended.is_none());

        rsx! {
            <div base={page()}>
                <div class="flex flex-none items-center gap-[14px] pt-[6px] pb-[16px]">
                    <div class="flex-none" text_color={if problems > 0 { theme::warn() } else { theme::text_dim() }}>
                        {glyph(if problems > 0 { "troubleshoot" } else { "health_and_safety" }, px(26.))}
                    </div>
                    <div class="flex flex-col flex-1 gap-[2px] min-w-[0px]">
                        <div text_size={px(15.)}>{says}</div>
                        {...(!self.at.is_empty()).then(|| rsx! {
                            <div text_size={px(11.5)} text_color={theme::text_faint()}>{format!("Last looked at {}", self.at)}</div>
                        })}
                    </div>
                    {...bundle.clone().map(|bundle| {
                        let count = missing.len();
                        rsx! {
                            <div
                                base={button("download", format!("Install {count} missing"), true)}
                                id="install-all"
                                onClick={cx.listener(move |page, _, _, cx| page.stage("Install everything missing", bundle.clone(), cx))}
                            />
                        }
                    })}
                    <div
                        base={button("refresh", "Scan again", false)}
                        id="scan"
                        when={(self.scanning, |button| button.opacity(0.5))}
                        onClick={cx.listener(|page, _, _, cx| page.scan(cx))}
                    />
                </div>

                {...self.staged.as_ref().map(|staged| {
                    let fix = staged.fix.clone();
                    rsx! {
                        <div class="flex flex-col flex-none gap-[12px] p-[15px] mb-[10px]" rounded={px(13.)} bg={theme::white(0.05)}>
                            <div text_size={px(13.5)} text_color={theme::text()}>{staged.what.clone()}</div>
                            <div text_size={px(11.5)} line_height={px(17.)} text_color={theme::text_faint()}>{staged.fix.summary.clone()}</div>
                            {...(!staged.fix.commands.is_empty()).then(|| commands(&staged.fix))}
                            {...(staged.fix.root && needs_agent).then(|| rsx! {
                                <div class="flex flex-none items-center gap-[8px]" text_size={px(11.5)} text_color={theme::alert()}>
                                    {glyph("lock", px(15.))}
                                    "No polkit agent is running, so the password prompt cannot appear — this will hang"
                                </div>
                            })}
                            <div class="flex flex-none items-center gap-[9px]">
                                <div
                                    base={button(if staged.fix.root { "key" } else { "play_arrow" }, if staged.fix.root { "Run as root" } else { "Run" }, true)}
                                    id="confirm"
                                    onClick={cx.listener(|page, _, _, cx| page.confirm(cx))}
                                />
                                <div
                                    base={button("close", "Cancel", false)}
                                    id="cancel-staged"
                                    onClick={cx.listener(|page, _, _, cx| { page.staged = None; cx.notify(); })}
                                />
                                {...fix.root.then(|| rsx! {
                                    <div class="flex-1 min-w-[0px]" text_size={px(11.)} text_color={theme::text_faint()}>
                                        "Asks for your password once"
                                    </div>
                                })}
                            </div>
                        </div>
                    }
                })}

                {...self.running.as_ref().map(|running| {
                    let shown: Vec<String> = running.log.iter().rev().take(SHOWN_LINES).rev().cloned().collect();
                    let hidden = running.log.len().saturating_sub(shown.len());
                    let (symbol, headline): (_, SharedString) = match (running.stopped, running.ended) {
                        (true, _) => ("block", format!("{} — stopped", running.what).into()),
                        (_, None) => ("pending", running.what.clone()),
                        (_, Some(0)) => ("task_alt", format!("{} — done", running.what).into()),
                        (_, Some(126 | 127)) => ("lock", "Cancelled at the password prompt, or nothing answered it".into()),
                        (_, Some(code)) => ("error", format!("Stopped at exit {code} — the log shows where").into()),
                    };
                    rsx! {
                        <div class="flex flex-col flex-none gap-[10px] p-[15px] mb-[10px]" rounded={px(13.)} bg={theme::white(0.05)}>
                            <div class="flex flex-none items-center gap-[10px]">
                                <div class="flex-none" text_color={if running.ended == Some(0) { theme::accent() } else { theme::text_dim() }}>
                                    {glyph(symbol, px(18.))}
                                </div>
                                <div class="flex-1 min-w-[0px]" text_size={px(13.)}>{headline}</div>
                                {...(running.ended.is_none() && running.root).then(|| rsx! {
                                    <div class="flex-none" text_size={px(11.)} text_color={theme::text_faint()}>"Waiting on your password"</div>
                                })}
                            </div>
                            {...(hidden > 0).then(|| rsx! {
                                <div text_size={px(11.)} text_color={theme::text_faint()}>{format!("{hidden} earlier lines — copy for all of them")}</div>
                            })}
                            {...(!shown.is_empty()).then(|| rsx! {
                                <div class="flex flex-col flex-none gap-[2px] p-[11px]" rounded={px(9.)} bg={theme::black(0.22)}>
                                    {for line in shown.into_iter() {
                                        <div text_size={px(10.5)} line_height={px(15.)} text_color={theme::text_faint()} font_features={theme::tabular()}>
                                            {line}
                                        </div>
                                    }}
                                </div>
                            })}
                            <div class="flex flex-none items-center gap-[9px]">
                                {...(running.ended.is_none()).then(|| rsx! {
                                    <div base={button("stop", "Stop", false)} id="stop-fix" onClick={cx.listener(|page, _, _, cx| page.stop(cx))} />
                                })}
                                {...(running.ended.is_some()).then(|| rsx! {
                                    <div base={button("check", "Close", false)} id="close-fix" onClick={cx.listener(|page, _, _, cx| page.put_away(cx))} />
                                })}
                                <div base={button("content_copy", "Copy log", false)} id="copy-log" onClick={cx.listener(|page, _, _, cx| page.copy_log(cx))} />
                            </div>
                        </div>
                    }
                })}

                {...report.as_ref().map(|report| {
                    let (wrong, well): (Vec<Row>, Vec<Row>) = report.rows.iter().cloned().partition(|row| row.status != Status::Ok);
                    let (any_wrong, any_well) = (!wrong.is_empty(), !well.is_empty());
                    rsx! {
                        <div class="flex flex-col flex-none">
                            {...any_wrong.then(|| section_title("Worth a look", true))}
                            {for (index, row) in wrong.into_iter().enumerate() {
                                <div class="flex flex-col flex-none" key={index}>
                                    {...(index > 0).then(rule)}
                                    {finding(&row, row.fix.clone().map(|fix| {
                                        let what: SharedString = row.name.clone().into();
                                        rsx! {
                                            <div
                                                base={button("build", fix.label.clone(), false)}
                                                id={SharedString::from(row.id.clone())}
                                                when={(busy, |button| button.opacity(0.45))}
                                                onClick={cx.listener(move |page, _, _, cx| page.stage(what.clone(), fix.clone(), cx))}
                                            />
                                        }
                                    }))}
                                </div>
                            }}

                            {...any_well.then(|| section_title("Well", !any_wrong))}
                            {for (index, row) in well.into_iter().enumerate() {
                                <div class="flex flex-col flex-none" key={index}>
                                    {...(index > 0).then(rule)}
                                    {finding(&row, None::<Div>)}
                                </div>
                            }}
                        </div>
                    }
                })}

                {...(report.is_none() && !self.scanning).then(|| nothing("troubleshoot", "Nothing looked at yet"))}
            </div>
        }
    }
}
