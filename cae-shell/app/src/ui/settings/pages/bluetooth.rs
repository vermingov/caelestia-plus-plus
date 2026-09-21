//! Bluetooth: what is paired, one device in full, and pairing another.

use std::time::Duration;

use cae_core::bluetooth::{self, Adapter, Info};
use cae_core::services;
use gpui::{AnyElement, AppContext, Context, IntoElement, Render, SharedString, Window, div, prelude::*, px};

use super::super::Page;
use super::super::frame::Reach;
use super::super::rows::{button, chosen_mark, fact, leads, nothing, page, pick, press, press_twice, pressable, row, rule, section_title};
use crate::theme;
use crate::ui::controls::{meter, switch};
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// BlueZ answers a command before the device has.
const SETTLED: Duration = Duration::from_millis(1500);
/// How often the list of what is nearby is read while something is looking.
const LOOK: Duration = Duration::from_secs(2);
const ARMED: Duration = Duration::from_secs(4);

/// The glyph for what BlueZ says a device is.
fn glyph_for(kind: &str) -> &'static str {
    match kind {
        "audio-headset" | "audio-headphones" => "headphones",
        "audio-card" => "speaker",
        "input-keyboard" => "keyboard",
        "input-mouse" => "mouse",
        "input-gaming" => "sports_esports",
        "input-tablet" => "tablet",
        "phone" => "smartphone",
        "computer" => "computer",
        _ => "bluetooth",
    }
}

fn status(device: &Info) -> String {
    match (device.connected, device.battery) {
        (true, Some(battery)) => format!("Connected, battery {battery}%"),
        (true, None) => "Connected".to_string(),
        _ => "Paired".to_string(),
    }
}

fn failure_line(failure: &str) -> Option<AnyElement> {
    (!failure.is_empty()).then(|| {
        rsx! { <div class="pt-[10px]" text_size={px(12.)} text_color={theme::alert()}>{SharedString::from(failure.to_string())}</div> }
            .into_any_element()
    })
}

pub struct Bluetooth {
    reach: Reach,
    paired: Vec<Info>,
    adapter: Adapter,
    /// Devices a connection has been asked of and not yet answered for.
    busy: Vec<String>,
}

impl Bluetooth {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Bluetooth {
        cx.observe(&reach.feeds.services, |_, _, cx| cx.notify()).detach();
        let mut page = Bluetooth { reach: reach.clone(), paired: Vec::new(), adapter: Adapter::default(), busy: Vec::new() };
        page.look(Duration::ZERO, cx);
        page
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(after).await;
            let found = cx
                .background_spawn(async {
                    let mut paired: Vec<Info> = services::devices().iter().map(|device| bluetooth::info(&device.address)).collect();
                    paired.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
                    (paired, bluetooth::adapter())
                })
                .await;
            let _ = page.update(cx, |page: &mut Bluetooth, cx| {
                (page.paired, page.adapter) = found;
                page.busy.clear();
                cx.notify();
            });
        })
        .detach();
    }

    fn power(&mut self, on: bool, cx: &mut Context<Self>) {
        cx.background_spawn(async move { services::set_bluetooth(on) }).detach();
        self.look(SETTLED, cx);
    }

    fn connect(&mut self, address: String, connect: bool, cx: &mut Context<Self>) {
        self.busy.push(address.clone());
        cx.notify();
        cx.spawn(async move |page, cx| {
            cx.background_spawn(async move { services::connect_device(&address, connect) }).await;
            let _ = page.update(cx, |page: &mut Bluetooth, cx| page.look(Duration::from_millis(300), cx));
        })
        .detach();
    }

    fn show_itself(&mut self, discoverable: bool, on: bool, cx: &mut Context<Self>) {
        if discoverable { self.adapter.discoverable = on } else { self.adapter.pairable = on }
        cx.background_spawn(async move { if discoverable { bluetooth::set_discoverable(on) } else { bluetooth::set_pairable(on) } })
            .detach();
        self.look(SETTLED, cx);
        cx.notify();
    }
}

impl Render for Bluetooth {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let powered = self.reach.feeds.services.read(cx).value.bluetooth.powered;
        let nav = self.reach.nav.clone();
        let adapter = self.adapter;

        let devices: Vec<AnyElement> = self
            .paired
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let busy = self.busy.contains(&device.address);
                let (address, connected) = (device.address.clone(), device.connected);
                let (nav, to) = (nav.clone(), Page::Device { address: device.address.clone(), name: device.name.clone() });
                rsx! {
                    <div class="flex flex-col flex-none">
                        {rule()}
                        <div
                            base={pick(glyph_for(&device.kind), device.name.clone(), if busy { "Working on it".to_string() } else { status(device) }, connected)}
                            id={("device", index)}
                            when={(busy, |row| row.opacity(0.55))}
                            onClick={cx.listener(move |page, _, _, cx| if !busy { page.connect(address.clone(), !connected, cx) })}
                        >
                            <div
                                base={press("settings", false)}
                                id={("about", index)}
                                onClick={move |_, window, cx| {
                                    cx.stop_propagation();
                                    nav.go(to.clone(), window, cx);
                                }}
                            />
                            {...connected.then(chosen_mark)}
                        </div>
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div base={page()}>
                <div base={pressable(row("Bluetooth", "", true))} id="power" onClick={cx.listener(move |page, _, _, cx| page.power(!powered, cx))}>
                    {switch(powered)}
                </div>
                {...powered.then(|| devices).into_iter().flatten()}
                {...(powered && self.paired.is_empty()).then(|| nothing("devices_other", "Nothing is paired"))}
                {...(!powered).then(|| nothing("bluetooth_disabled", "Bluetooth is off"))}

                {...powered.then(|| {
                    let nav = self.reach.nav.clone();
                    rsx! {
                        <div class="flex flex-col flex-none">
                            <div class="flex-none h-[18px]" />
                            <div base={leads("add", "Pair a new device", "")} id="pair" onClick={move |_, window, cx| nav.go(Page::Pairing, window, cx)} />
                            {section_title("To other devices", false)}
                            <div
                                base={pressable(row("Discoverable", "Devices nearby can find this computer", true))}
                                id="discoverable"
                                onClick={cx.listener(move |page, _, _, cx| page.show_itself(true, !adapter.discoverable, cx))}
                            >
                                {switch(adapter.discoverable)}
                            </div>
                            {rule()}
                            <div
                                base={pressable(row("Pairable", "Devices nearby can ask to pair with it", true))}
                                id="pairable"
                                onClick={cx.listener(move |page, _, _, cx| page.show_itself(false, !adapter.pairable, cx))}
                            >
                                {switch(adapter.pairable)}
                            </div>
                        </div>
                    }
                })}
            </div>
        }
    }
}

/// One paired device.
pub struct Device {
    reach: Reach,
    device: Info,
    busy: bool,
    forgetting: bool,
    failure: String,
}

impl Device {
    pub fn new(address: String, name: String, reach: &Reach, cx: &mut Context<Self>) -> Device {
        let mut page =
            Device { reach: reach.clone(), device: Info { address, name, ..Info::default() }, busy: false, forgetting: false, failure: String::new() };
        page.look(Duration::ZERO, cx);
        page
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        let address = self.device.address.clone();
        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(after).await;
            let device = cx.background_spawn(async move { bluetooth::info(&address) }).await;
            let _ = page.update(cx, |page: &mut Device, cx| {
                (page.device, page.busy) = (device, false);
                cx.notify();
            });
        })
        .detach();
    }

    /// Does one thing to the device and reads it again, saying why if the
    /// thing could not be done.
    fn ask(&mut self, work: impl FnOnce(&str) -> Result<(), String> + Send + 'static, cx: &mut Context<Self>) {
        let address = self.device.address.clone();
        self.failure.clear();
        cx.spawn(async move |page, cx| {
            let asked = cx.background_spawn(async move { work(&address) }).await;
            let _ = page.update(cx, |page: &mut Device, cx| {
                page.failure = asked.err().unwrap_or_default();
                page.look(Duration::from_millis(250), cx);
            });
        })
        .detach();
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let connect = !self.device.connected;
        self.busy = true;
        cx.notify();
        self.ask(
            move |address| {
                services::connect_device(address, connect);
                Ok(())
            },
            cx,
        );
    }

    fn forget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.forgetting {
            self.forgetting = true;
            cx.notify();
            cx.spawn(async move |page, cx| {
                cx.background_executor().timer(ARMED).await;
                let _ = page.update(cx, |page: &mut Device, cx| {
                    page.forgetting = false;
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let address = self.device.address.clone();
        cx.background_spawn(async move { services::forget_device(&address) }).detach();
        self.reach.nav.back(window, cx);
    }
}

impl Render for Device {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let device = self.device.clone();
        let says = if self.busy { "Working on it".to_string() } else if device.paired { status(&device) } else { "Not paired".to_string() };
        let flip = |label: &'static str, note: &'static str, id: &'static str, on: bool, set: fn(&str, bool) -> Result<(), String>, cx: &mut Context<Self>| {
            rsx! {
                <div base={pressable(row(label, note, true))} id={id} onClick={cx.listener(move |page, _, _, cx| page.ask(move |address| set(address, !on), cx))}>
                    {switch(on)}
                </div>
            }
        };

        rsx! {
            <div base={page()}>
                <div class="flex flex-none items-center gap-[14px] pt-[4px] pb-[16px]">
                    <div class="flex-none" text_color={if device.connected { theme::text() } else { theme::text_faint() }}>
                        {glyph(glyph_for(&device.kind), px(24.))}
                    </div>
                    <div class="flex-1 min-w-[0px] truncate" text_size={px(15.)}>{says}</div>
                    <div base={press_twice("delete", self.forgetting, "Forget")} id="forget" onClick={cx.listener(|page, _, window, cx| page.forget(window, cx))} />
                    <div
                        base={button(if device.connected { "link_off" } else { "link" }, if device.connected { "Disconnect" } else { "Connect" }, !device.connected)}
                        id="connect"
                        when={(self.busy, |button| button.opacity(0.5))}
                        onClick={cx.listener(|page, _, _, cx| if !page.busy { page.connect(cx) })}
                    />
                </div>

                {flip("Trusted", "May connect by itself", "trusted", device.trusted, bluetooth::set_trusted, cx)}
                {rule()}
                {flip("Blocked", "May not connect at all", "blocked", device.blocked, bluetooth::set_blocked, cx)}
                {rule()}
                {flip("Wakes this computer", "", "wakes", device.wakes, bluetooth::set_wakes, cx)}

                {section_title("About it", false)}
                {...device.battery.map(|battery| rsx! {
                    <div class="flex flex-col flex-none">
                        {fact("Battery", format!("{battery}%"))}
                        <div class="pb-[10px]">{meter(f64::from(battery))}</div>
                        {rule()}
                    </div>
                })}
                {fact("Address", device.address.clone())}
                {...failure_line(&self.failure)}
            </div>
        }
    }
}

/// Looking for something to pair with, for as long as this page is open.
pub struct Pairing {
    reach: Reach,
    nearby: Vec<(String, String)>,
    pairing: String,
    failure: String,
}

impl Pairing {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Pairing {
        cx.background_spawn(async { services::set_discovering(true) }).detach();
        // Scanning is a mode the adapter stays in, and costs battery on both
        // sides: it ends with the page that asked for it.
        cx.on_release(|_, cx| cx.background_spawn(async { services::set_discovering(false) }).detach()).detach();

        cx.spawn(async move |page, cx| {
            loop {
                let nearby = cx.background_spawn(async { bluetooth::nearby() }).await;
                let looked = page.update(cx, |page: &mut Pairing, cx| {
                    if page.nearby != nearby {
                        page.nearby = nearby;
                        cx.notify();
                    }
                });
                if looked.is_err() {
                    break;
                }
                cx.background_executor().timer(LOOK).await;
            }
        })
        .detach();
        Pairing { reach: reach.clone(), nearby: Vec::new(), pairing: String::new(), failure: String::new() }
    }

    fn pair(&mut self, address: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pairing.is_empty() {
            return;
        }
        self.pairing = address.clone();
        self.failure.clear();
        cx.notify();
        cx.spawn_in(window, async move |page, cx| {
            let paired = cx.background_spawn(async move { bluetooth::pair(&address) }).await;
            let _ = page.update_in(cx, |page: &mut Pairing, window, cx| {
                page.pairing.clear();
                match paired {
                    Ok(()) => page.reach.nav.back(window, cx),
                    Err(why) => {
                        page.failure = why;
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for Pairing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        rsx! {
            <div base={page()}>
                {section_title("Nearby, and ready to pair", true)}
                {for (index, (address, name)) in self.nearby.iter().enumerate() {
                    <div class="flex flex-col flex-none" key={index}>
                        {...(index > 0).then(rule)}
                        <div
                            base={pick("bluetooth", name.clone(), if self.pairing == *address { "Pairing".to_string() } else { address.clone() }, false)}
                            id={("nearby", index)}
                            when={(!self.pairing.is_empty(), |row| row.opacity(0.55))}
                            onClick={cx.listener({
                                let address = address.clone();
                                move |page, _, window, cx| page.pair(address.clone(), window, cx)
                            })}
                        />
                    </div>
                }}
                {...self.nearby.is_empty().then(|| nothing("bluetooth_searching", "Looking. Put the device in its pairing mode"))}
                {...failure_line(&self.failure)}
            </div>
        }
    }
}
