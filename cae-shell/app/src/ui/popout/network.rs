//! The network: what is in range, what is plugged in, and the radio.

use std::time::Duration;

use cae_core::services;
use gpui::{AppContext, Context, EventEmitter, IntoElement, Render, Window, prelude::*};

use super::pieces::{caption, column, detail, entry, headline, list, meter, row, toggle, trailing, wide};
use super::{Finished, Place, join};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::rsx;

pub struct Network {
    feeds: Feeds,
    place: Place,
    /// Scanning costs a process, so the lists are read when the panel opens
    /// and when something in it has been pressed, and not kept otherwise.
    networks: Vec<services::Wifi>,
    wired: Vec<services::Ethernet>,
    scanning: bool,
    failure: String,
}

impl EventEmitter<Finished> for Network {}

impl Network {
    pub fn new(feeds: &Feeds, place: Place, cx: &mut Context<Self>) -> Network {
        cx.observe(&feeds.system, |_, _, cx| cx.notify()).detach();
        let mut network =
            Network { feeds: feeds.clone(), place, networks: Vec::new(), wired: Vec::new(), scanning: false, failure: String::new() };
        network.glance(cx);
        network.look(Duration::ZERO, cx);
        network
    }

    /// What is already known, at once. The proper look that follows can wait
    /// several seconds on the radio, and a panel that opens saying nothing is
    /// in range, in a room full of networks, reads as a broken panel.
    fn glance(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |network, cx| {
            let found = cx.background_spawn(async { (services::networks_at_hand(), services::ethernet()) }).await;
            let _ = network.update(cx, |network, cx| {
                // The proper look may have come back first, and knows better.
                if network.networks.is_empty() {
                    (network.networks, network.wired) = found;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Reads both lists again, after a wait: nearly everything asked of
    /// NetworkManager has not happened yet when the asking returns.
    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |network, cx| {
            cx.background_executor().timer(after).await;
            let found = cx.background_spawn(async { (services::networks(), services::ethernet()) }).await;
            let _ = network.update(cx, |network, cx| {
                (network.networks, network.wired) = found;
                network.scanning = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// A scan is asked for and returns at once, so the list is read a beat
    /// later rather than immediately: otherwise the button appears to do
    /// nothing.
    fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.notify();
        cx.background_spawn(async { services::rescan() }).detach();
        self.look(Duration::from_millis(2200), cx);
    }

    /// A network joined before, or an open one, needs nothing from anybody.
    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(wifi) = self.networks.get(index).filter(|wifi| !wifi.active) else { return };
        if wifi.secured && !wifi.known {
            // A popout cannot be typed into: it is on the screen because the
            // pointer is, and has never been given the keyboard. The question
            // is put by a window that has, in the place this one is.
            let Place { at, display } = self.place;
            cx.emit(Finished);
            return join::ask(wifi.ssid.clone(), at, display, cx);
        }
        self.failure.clear();
        let ssid = wifi.ssid.clone();
        cx.spawn(async move |network, cx| {
            let joined = cx.background_spawn(async move { services::join(&ssid, "") }).await;
            let _ = network.update(cx, |network, cx| {
                network.failure = joined.err().unwrap_or_default();
                network.look(Duration::ZERO, cx);
            });
        })
        .detach();
    }

    fn toggle_wired(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(device) = self.wired.get(index) else { return };
        let (interface, connect) = (device.interface.clone(), !device.connected);
        cx.background_spawn(async move { services::set_ethernet(&interface, connect) }).detach();
        self.look(Duration::from_millis(1200), cx);
    }

    fn toggle_radio(&mut self, on: bool, cx: &mut Context<Self>) {
        cx.background_spawn(async move { services::set_wifi(on) }).detach();
        self.look(Duration::from_millis(900), cx);
    }
}

fn plural(count: usize, one: &str) -> String {
    format!("{count} {one}{} available", if count == 1 { "" } else { "s" })
}

impl Render for Network {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let link = self.feeds.system.read(cx).value.network.clone();
        let up = link.kind == "wifi" || link.kind == "ethernet";
        let name = match link.kind.as_str() {
            "ethernet" => "Wired",
            "wifi" => "Wireless",
            _ => "No connection",
        };

        rsx! {
            <div base={column()}>
                <div base={row()}>
                    {headline(name)}
                    <div base={toggle(up)} id="radio" onClick={cx.listener(move |network, _, _, cx| network.toggle_radio(!up, cx))} />
                </div>
                {...(link.kind == "wifi").then(|| meter(link.strength as f64))}
                {caption(plural(self.networks.len(), "network"))}
                <div base={list()} id="networks" class="overflow-y-scroll">
                    {for (index, wifi) in self.networks.iter().enumerate() {
                        <div
                            base={entry(if wifi.secured { "wifi_lock" } else { "wifi" }, wifi.ssid.clone(), wifi.active)}
                            id={("wifi", index)}
                            onClick={cx.listener(move |network, _, _, cx| network.choose(index, cx))}
                        >
                            {trailing(format!("{}%", wifi.strength))}
                        </div>
                    }}
                    {...self.networks.is_empty().then(|| detail("Nothing in range").px(gpui::px(8.)))}
                </div>
                <div
                    base={wide("refresh", if self.scanning { "Scanning…" } else { "Rescan networks" })}
                    id="rescan"
                    when={(self.scanning, |button| button.opacity(0.7))}
                    onClick={cx.listener(|network, _, _, cx| network.rescan(cx))}
                />
                // Wired devices, under the same roof: a dock or a USB adapter
                // is something connected and disconnected like anything else.
                {...(!self.wired.is_empty()).then(|| headline("Ethernet"))}
                {...(!self.wired.is_empty()).then(|| caption(plural(self.wired.len(), "device")))}
                {...(!self.wired.is_empty()).then(|| rsx! {
                    <div base={list()} id="wired" class="overflow-y-scroll">
                        {for (index, device) in self.wired.iter().enumerate() {
                            <div
                                base={entry(
                                    "lan",
                                    if device.connection.is_empty() { device.interface.clone() } else { device.connection.clone() },
                                    device.connected,
                                )}
                                id={("wired", index)}
                                onClick={cx.listener(move |network, _, _, cx| network.toggle_wired(index, cx))}
                            >
                                {trailing(if device.connected { "Connected" } else { "Off" })}
                            </div>
                        }}
                    </div>
                })}
                {...(!self.failure.is_empty()).then(|| detail(self.failure.clone()).text_color(theme::alert()))}
            </div>
        }
    }
}
