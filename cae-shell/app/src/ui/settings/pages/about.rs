//! About: what this is, and what it is running on.

use cae_core::about::{self, Machine, Software};
use gpui::{AppContext, Context, FontWeight, IntoElement, Render, Window, div, prelude::*, px, svg};

use super::super::rows::{fact, page, rule, section_title};
use crate::theme;
use crate::ui::rsx;

pub struct About {
    machine: Machine,
    /// Nothing until the programs that are asked have answered.
    software: Option<Software>,
}

impl About {
    pub fn new(cx: &mut Context<Self>) -> About {
        cx.spawn(async move |page, cx| {
            let software = cx.background_spawn(async { about::software() }).await;
            let _ = page.update(cx, |page: &mut About, cx| {
                page.software = Some(software);
                cx.notify();
            });
        })
        .detach();
        About { machine: about::machine(), software: None }
    }
}

/// A fact that may not be in yet, or may never be: a machine with no
/// firmware to name, a program that is not installed.
fn known(value: &str) -> String {
    if value.is_empty() { "Unknown".to_string() } else { value.to_string() }
}

impl Render for About {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let machine = &self.machine;
        let software = self.software.clone().unwrap_or_default();
        let version = match software.release.as_str() {
            "" => format!("{} at {}", env!("CARGO_PKG_VERSION"), known(&software.revision)),
            release => format!("{} ({release})", env!("CARGO_PKG_VERSION")),
        };

        rsx! {
            <div base={page()}>
                <div class="flex flex-none items-center gap-[16px] pt-[4px] pb-[10px]">
                    <svg src="marks/caelestia.svg" class="flex-none size-[44px]" text_color={theme::accent()} />
                    <div class="flex flex-col gap-[3px]">
                        <div text_size={px(17.)} font_weight={FontWeight::MEDIUM}>{"Caelestia++"}</div>
                        <div text_size={px(12.)} text_color={theme::text_faint()}>{"The desktop shell, drawn by cae"}</div>
                    </div>
                </div>

                {section_title("This machine", false)}
                {fact("Name", known(&machine.hostname))}
                {rule()}
                {fact("Device", known(&machine.device))}
                {rule()}
                {fact("System", known(&machine.distro))}
                {rule()}
                {fact("Kernel", known(&machine.kernel))}
                {rule()}
                {fact("Firmware", known(&machine.firmware))}

                {section_title("Software", false)}
                {fact("Shell", version)}
                {rule()}
                {fact("Compositor", if software.compositor.is_empty() { known("") } else { format!("Hyprland {}", software.compositor) })}
                {rule()}
                {fact("Command line", known(&software.cli))}
            </div>
        }
    }
}
