//! Updates: whether a newer release is out, and the way to it.

use cae_core::about;
use cae_core::updates::{self, Found};
use gpui::{AppContext, Context, IntoElement, Render, SharedString, Window, div, prelude::*, px};

use super::super::frame::Reach;
use super::super::rows::{button, fact, page, rule, section_title};
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

enum Looked {
    Looking,
    Found(Found),
    Failed(String),
}

pub struct Updates {
    reach: Reach,
    looked: Looked,
    revision: String,
}

impl Updates {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Updates {
        let mut page = Updates { reach: reach.clone(), looked: Looked::Looking, revision: String::new() };
        page.look(cx);
        page
    }

    fn look(&mut self, cx: &mut Context<Self>) {
        self.looked = Looked::Looking;
        cx.notify();
        cx.spawn(async move |page, cx| {
            let answer = cx
                .background_spawn(async { (updates::check(), about::git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default()) })
                .await;
            let _ = page.update(cx, |page: &mut Updates, cx| {
                let (found, revision) = answer;
                page.looked = found.map_or_else(Looked::Failed, Looked::Found);
                page.revision = revision;
                cx.notify();
            });
        })
        .detach();
    }

    /// The updater, in a terminal: it rebuilds what the release needs and
    /// may ask for a password, and both are things to be watched rather than
    /// done behind a button.
    fn update(&mut self, cx: &mut Context<Self>) {
        let terminal = self.reach.store.read(cx).terminal();
        cx.background_spawn(async move {
            let run = "cae; printf '\\nEnter closes this window. '; read -r _";
            let _ = std::process::Command::new("setsid").arg("-f").args(&terminal).args(["sh", "-c", run]).status();
        })
        .detach();
    }
}

impl Render for Updates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (symbol, says, behind): (_, SharedString, _) = match &self.looked {
            Looked::Looking => ("sync", "Looking for a newer release".into(), false),
            Looked::Failed(why) => ("cloud_off", why.clone().into(), false),
            Looked::Found(found) if found.behind > 0 => ("deployed_code_update", format!("{} is out", found.release).into(), true),
            Looked::Found(found) if found.release.is_empty() => ("check_circle", "Nothing has been published yet".into(), false),
            Looked::Found(_) => ("check_circle", "Caelestia++ is up to date".into(), false),
        };
        let looking = matches!(self.looked, Looked::Looking);
        let changes: Vec<String> = match &self.looked {
            Looked::Found(found) if found.behind > 0 => found.changes.clone(),
            _ => Vec::new(),
        };
        let release = match &self.looked {
            Looked::Found(found) if found.behind == 0 && !found.release.is_empty() => found.release.clone(),
            Looked::Found(found) if found.behind > 0 => format!("Behind {}", found.release),
            _ => "Between releases".to_string(),
        };

        rsx! {
            <div base={page()}>
                <div class="flex flex-none items-center gap-[14px] pt-[6px] pb-[16px]">
                    <div class="flex-none" text_color={if behind { theme::accent() } else { theme::text_dim() }}>{glyph(symbol, px(26.))}</div>
                    <div class="flex-1 min-w-[0px]" text_size={px(15.)}>{says}</div>
                    {...behind.then(|| rsx! {
                        <div base={button("download", "Update and restart", true)} id="update" onClick={cx.listener(|page, _, _, cx| page.update(cx))} />
                    })}
                    <div
                        base={button("refresh", "Check again", false)}
                        id="check"
                        when={(looking, |button| button.opacity(0.5))}
                        onClick={cx.listener(|page, _, _, cx| if !matches!(page.looked, Looked::Looking) { page.look(cx) })}
                    />
                </div>

                {...(!changes.is_empty()).then(|| section_title("What is new", true))}
                {for (index, change) in changes.into_iter().enumerate() {
                    <div class="flex flex-col flex-none" key={index}>
                        {...(index > 0).then(rule)}
                        <div class="py-[9px]" text_size={px(12.5)} line_height={px(17.)} text_color={theme::text_dim()}>{change}</div>
                    </div>
                }}

                {section_title("Installed", false)}
                {fact("Release", release)}
                {rule()}
                {fact("Revision", if self.revision.is_empty() { "Unknown".to_string() } else { self.revision.clone() })}
            </div>
        }
    }
}
