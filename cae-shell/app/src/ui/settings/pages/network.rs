//! The network: what is plugged in, what is in range, and the radio.

use std::time::Duration;

use cae_core::network::{self, Addressing, Ipv4, Link};
use cae_core::services;
use gpui::{AnyElement, AppContext, Context, Entity, Focusable, IntoElement, Render, SharedString, Window, div, prelude::*, px};

use super::super::Page;
use super::super::frame::{Commit, Reach};
use super::super::rows::{button, chosen_mark, fact, leads, nothing, page, pick, press_twice, pressable, row, rule, section_title, typed};
use super::super::store::shell;
use crate::theme;
use crate::ui::controls::{chip, switch};
use crate::ui::field::Field;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// Nearly everything asked of NetworkManager has not happened yet when the
/// asking returns.
const SETTLED: Duration = Duration::from_millis(1200);

fn bars(strength: i64) -> &'static str {
    match strength {
        71.. => "network_wifi",
        46..=70 => "network_wifi_3_bar",
        21..=45 => "network_wifi_2_bar",
        _ => "network_wifi_1_bar",
    }
}

pub struct Network {
    reach: Reach,
    radio: bool,
    networks: Vec<services::Wifi>,
    wired: Vec<services::Ethernet>,
    scanning: bool,
    /// The network a password is being asked for, and the box it goes in.
    asking: Option<(String, Entity<Field>)>,
    joining: String,
    /// The network whose bin has been pressed once.
    forgetting: String,
    failure: String,
}

/// How long a bin pressed once stays armed.
const ARMED: Duration = Duration::from_secs(4);

impl Network {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Network {
        let mut network = Network {
            reach: reach.clone(),
            radio: true,
            networks: Vec::new(),
            wired: Vec::new(),
            scanning: false,
            asking: None,
            joining: String::new(),
            forgetting: String::new(),
            failure: String::new(),
        };
        network.glance(cx);
        network.keep_scanning(cx);
        network
    }

    /// What is already known, at once: the proper look can wait seconds on
    /// the radio, and a page that opens empty in a room full of networks
    /// reads as broken.
    fn glance(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |network, cx| {
            let found = cx
                .background_spawn(async { (network::wifi_enabled(), services::networks_at_hand(), services::ethernet()) })
                .await;
            let _ = network.update(cx, |network: &mut Network, cx| {
                (network.radio, network.networks, network.wired) = found;
                cx.notify();
            });
        })
        .detach();
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |network, cx| {
            cx.background_executor().timer(after).await;
            let found =
                cx.background_spawn(async { (network::wifi_enabled(), services::networks(), services::ethernet()) }).await;
            let _ = network.update(cx, |network: &mut Network, cx| {
                (network.radio, network.networks, network.wired) = found;
                network.scanning = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Scans for as long as the page is open, as often as the settings say.
    fn keep_scanning(&mut self, cx: &mut Context<Self>) {
        let every = self.reach.store.read(cx).number(shell("nexus.networkRescanInterval"), 15_000.).max(5000.);
        cx.spawn(async move |network, cx| {
            loop {
                let scanned = network.update(cx, |network: &mut Network, cx| network.rescan(cx));
                if scanned.is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(every as u64)).await;
            }
        })
        .detach();
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning || !self.radio {
            return;
        }
        self.scanning = true;
        cx.notify();
        cx.background_spawn(async { services::rescan() }).detach();
        self.look(Duration::from_millis(2200), cx);
    }

    fn toggle_radio(&mut self, cx: &mut Context<Self>) {
        let on = !self.radio;
        self.radio = on;
        if !on {
            self.networks.clear();
        }
        cx.background_spawn(async move { services::set_wifi(on) }).detach();
        self.look(SETTLED, cx);
        cx.notify();
    }

    /// A network joined before, or an open one, needs nothing from anybody.
    /// Any other is asked about in its own row.
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wifi) = self.networks.get(index).filter(|wifi| !wifi.active) else { return };
        self.failure.clear();
        if wifi.secured && !wifi.known {
            let password = cx.new(|cx| Field::new("Password", cx).secret());
            window.focus(&password.focus_handle(cx), cx);
            self.asking = Some((wifi.ssid.clone(), password));
            return cx.notify();
        }
        self.join(wifi.ssid.clone(), String::new(), cx);
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some((ssid, password)) = &self.asking else { return };
        let said = password.read(cx).text().to_string();
        if !said.is_empty() {
            self.join(ssid.clone(), said, cx);
        }
    }

    fn join(&mut self, ssid: String, password: String, cx: &mut Context<Self>) {
        self.joining = ssid.clone();
        cx.notify();
        cx.spawn(async move |network, cx| {
            let joined = cx.background_spawn(async move { services::join(&ssid, &password) }).await;
            let _ = network.update(cx, |network: &mut Network, cx| {
                network.joining.clear();
                match joined {
                    Ok(()) => network.asking = None,
                    Err(why) => network.failure = why,
                }
                network.look(Duration::ZERO, cx);
            });
        })
        .detach();
    }

    /// Forgetting throws a password away, so the bin is pressed twice: once
    /// to say which, and again to mean it.
    fn forget(&mut self, ssid: String, cx: &mut Context<Self>) {
        if self.forgetting != ssid {
            self.forgetting = ssid.clone();
            cx.notify();
            cx.spawn(async move |network, cx| {
                cx.background_executor().timer(ARMED).await;
                let _ = network.update(cx, |network: &mut Network, cx| {
                    if network.forgetting == ssid {
                        network.forgetting.clear();
                        cx.notify();
                    }
                });
            })
            .detach();
            return;
        }
        self.forgetting.clear();
        cx.spawn(async move |network, cx| {
            let forgotten = cx.background_spawn(async move { network::forget(&ssid) }).await;
            let _ = network.update(cx, |network: &mut Network, cx| {
                network.failure = forgotten.err().unwrap_or_default();
                network.look(Duration::from_millis(400), cx);
            });
        })
        .detach();
    }

    fn wireless(&self, index: usize, wifi: &services::Wifi, cx: &mut Context<Self>) -> AnyElement {
        let asking = self.asking.as_ref().filter(|(ssid, _)| *ssid == wifi.ssid);
        let note = match () {
            _ if self.joining == wifi.ssid => "Joining",
            _ if wifi.active => "Connected",
            _ if wifi.known => "Saved",
            _ if wifi.secured => "Secured",
            _ => "Open",
        };
        let ssid = wifi.ssid.clone();

        rsx! {
            <div class="flex flex-col flex-none">
                <div
                    base={pick(bars(wifi.strength), wifi.ssid.clone(), note, wifi.active)}
                    id={("wifi", index)}
                    onClick={cx.listener(move |network, _, window, cx| network.choose(index, window, cx))}
                >
                    {...(wifi.known).then(|| rsx! {
                        <div
                            base={press_twice("delete", self.forgetting == wifi.ssid, "Forget")}
                            id={("forget", index)}
                            onClick={cx.listener(move |network, _, _, cx| {
                                cx.stop_propagation();
                                network.forget(ssid.clone(), cx);
                            })}
                        />
                    })}
                    {...wifi.active.then(chosen_mark)}
                </div>
                {...asking.map(|(_, password)| rsx! {
                    <div class="flex flex-none items-center justify-end gap-[8px] pb-[10px]">
                        {typed(password, password.read(cx).is_focused())}
                        <div base={button("login", "Join", true)} id="join" onClick={cx.listener(|network, _, _, cx| network.submit(cx))} />
                    </div>
                })}
            </div>
        }
        .into_any_element()
    }
}

impl Render for Network {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = self.reach.nav.clone();
        let wired: Vec<AnyElement> = self
            .wired
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let name = if device.connection.is_empty() { device.interface.clone() } else { device.connection.clone() };
                let says = if device.connected { format!("Connected on {}", device.interface) } else { format!("{} is not connected", device.interface) };
                let (nav, to) = (nav.clone(), Page::Ethernet { interface: device.interface.clone(), connection: device.connection.clone() });
                rsx! {
                    <div class="flex flex-col flex-none">
                        {...(index > 0).then(rule)}
                        <div base={leads("lan", name, says)} id={("wired", index)} onClick={move |_, window, cx| nav.go(to.clone(), window, cx)} />
                    </div>
                }
                .into_any_element()
            })
            .collect();
        let has_wired = !wired.is_empty();

        let wireless: Vec<AnyElement> = self
            .networks
            .iter()
            .enumerate()
            .flat_map(|(index, wifi)| [rule().into_any_element(), self.wireless(index, wifi, cx)])
            .collect();

        rsx! {
            <div
                base={page()}
                on_action={cx.listener(|network, _: &Commit, _, cx| network.submit(cx))}
            >
                {...has_wired.then(|| section_title("Wired", true))}
                {...wired}

                {section_title("Wireless", !has_wired)}
                <div
                    base={pressable(row("Wi-Fi", if self.scanning { "Looking for networks" } else { "" }, true))}
                    id="radio"
                    onClick={cx.listener(|network, _, _, cx| network.toggle_radio(cx))}
                >
                    {switch(self.radio)}
                </div>
                {...wireless}
                {...(self.radio && self.networks.is_empty()).then(|| nothing("wifi_find", "Nothing in range yet"))}
                {...(!self.radio).then(|| nothing("signal_wifi_off", "Wi-Fi is off"))}
                {...(!self.failure.is_empty()).then(|| rsx! {
                    <div class="pt-[10px]" text_size={px(12.)} text_color={theme::alert()}>{SharedString::from(self.failure.clone())}</div>
                })}
            </div>
        }
    }
}

/// One wired device: what it was given, and how its profile gets an address.
pub struct Ethernet {
    interface: String,
    connection: String,
    connected: bool,
    link: Link,
    /// As the profile has it, and as it is being edited. Nothing until read.
    kept: Option<Ipv4>,
    addressing: Addressing,
    address: Entity<Field>,
    gateway: Entity<Field>,
    dns: Entity<Field>,
    saving: bool,
    failure: String,
}

impl Ethernet {
    pub fn new(interface: String, connection: String, cx: &mut Context<Self>) -> Ethernet {
        let field = |placeholder: &'static str, cx: &mut Context<Self>| {
            let field = cx.new(|cx| Field::new(placeholder, cx));
            cx.observe(&field, |_, _, cx| cx.notify()).detach();
            field
        };
        let mut page = Ethernet {
            address: field("192.168.1.50/24", cx),
            gateway: field("192.168.1.1", cx),
            dns: field("1.1.1.1, 9.9.9.9", cx),
            interface,
            connection,
            connected: false,
            link: Link::default(),
            kept: None,
            addressing: Addressing::Automatic,
            saving: false,
            failure: String::new(),
        };
        page.look(Duration::ZERO, cx);
        page
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        let (interface, connection) = (self.interface.clone(), self.connection.clone());
        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(after).await;
            let found = cx
                .background_spawn(async move {
                    let device = services::ethernet().into_iter().find(|device| device.interface == interface);
                    let connection = device.as_ref().map_or(connection, |device| device.connection.clone());
                    let kept = (!connection.is_empty()).then(|| network::ipv4(&connection)).flatten();
                    (device, network::link(&interface), kept, connection)
                })
                .await;
            let _ = page.update(cx, |page: &mut Ethernet, cx| {
                let (device, link, kept, connection) = found;
                page.connected = device.is_some_and(|device| device.connected);
                (page.link, page.connection) = (link, connection);
                // What is being typed is not written over by a look that
                // was only asked for to see whether the cable is in.
                if page.kept != kept {
                    if let Some(kept) = &kept {
                        page.addressing = kept.addressing;
                        page.address.update(cx, |field, cx| field.set_text(kept.address.clone(), cx));
                        page.gateway.update(cx, |field, cx| field.set_text(kept.gateway.clone(), cx));
                        page.dns.update(cx, |field, cx| field.set_text(kept.dns.clone(), cx));
                    }
                    page.kept = kept;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        let (interface, connect) = (self.interface.clone(), !self.connected);
        cx.background_spawn(async move { services::set_ethernet(&interface, connect) }).detach();
        self.look(SETTLED, cx);
    }

    fn edited(&self, cx: &Context<Self>) -> Ipv4 {
        let said = |field: &Entity<Field>| field.read(cx).text().trim().to_string();
        Ipv4 { addressing: self.addressing, address: said(&self.address), gateway: said(&self.gateway), dns: said(&self.dns) }
    }

    /// What is wrong with what has been typed, if anything is.
    fn objection(&self, edited: &Ipv4) -> Option<&'static str> {
        let manual = edited.addressing == Addressing::Manual;
        if manual && !network::is_address_with_prefix(&edited.address) {
            return Some("The address needs its prefix, as in 192.168.1.50/24");
        }
        if manual && !edited.gateway.is_empty() && !network::is_address(&edited.gateway) {
            return Some("The gateway is not an address");
        }
        let own_dns = edited.addressing != Addressing::Automatic;
        if own_dns && !edited.dns.is_empty() && !network::is_address_list(&edited.dns) {
            return Some("The name servers are addresses with commas between them");
        }
        (edited.addressing == Addressing::AutomaticWithDns && edited.dns.is_empty()).then_some("Name at least one name server")
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let edited = self.edited(cx);
        if self.saving || self.objection(&edited).is_some() {
            return;
        }
        self.saving = true;
        self.failure.clear();
        cx.notify();
        let connection = self.connection.clone();
        cx.spawn(async move |page, cx| {
            let applied = cx.background_spawn(async move { network::set_ipv4(&connection, &edited) }).await;
            let _ = page.update(cx, |page: &mut Ethernet, cx| {
                page.saving = false;
                page.failure = applied.err().unwrap_or_default();
                page.look(Duration::from_millis(600), cx);
            });
        })
        .detach();
    }
}

impl Render for Ethernet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let known = |value: &str| if value.is_empty() { "None".to_string() } else { value.to_string() };
        let link = &self.link;
        let edited = self.edited(cx);
        let changed = self.kept.as_ref().is_some_and(|kept| *kept != edited);
        let objection = changed.then(|| self.objection(&edited)).flatten();
        let ways = [(Addressing::Automatic, "Automatic"), (Addressing::AutomaticWithDns, "Own name servers"), (Addressing::Manual, "Manual")];
        let typed_row = |label: &'static str, note: &'static str, field: &Entity<Field>| {
            rsx! { <div base={row(label, note, true)}>{typed(field, field.read(cx).is_focused())}</div> }
        };

        rsx! {
            <div base={page()} on_action={cx.listener(|page, _: &Commit, _, cx| page.apply(cx))}>
                <div class="flex flex-none items-center gap-[14px] pt-[4px] pb-[14px]">
                    <div class="flex-none" text_color={if self.connected { theme::text() } else { theme::text_faint() }}>{glyph("lan", px(24.))}</div>
                    <div class="flex-1" text_size={px(15.)}>{if self.connected { "Connected" } else { "Not connected" }}</div>
                    <div
                        base={button(if self.connected { "link_off" } else { "link" }, if self.connected { "Disconnect" } else { "Connect" }, false)}
                        id="link"
                        onClick={cx.listener(|page, _, _, cx| page.toggle(cx))}
                    />
                </div>

                {section_title("This link", true)}
                {fact("Interface", self.interface.clone())}
                {rule()}
                {fact("Address", known(&link.address))}
                {rule()}
                {fact("Gateway", known(&link.gateway))}
                {rule()}
                {fact("Name servers", known(&link.dns.join(", ")))}
                {rule()}
                {fact("Hardware address", known(&link.mac))}
                {...(!link.speed.is_empty()).then(|| rsx! { <div class="flex flex-col flex-none">{rule()}{fact("Speed", link.speed.clone())}</div> })}
                {...(self.connected && !link.carried.is_empty()).then(|| rsx! {
                    <div class="flex flex-col flex-none">{rule()}{fact("Carried since startup", link.carried.clone())}</div>
                })}

                {...self.kept.is_some().then(|| rsx! {
                    <div class="flex flex-col flex-none">
                        {section_title("IPv4", false)}
                        <div base={row("Address from", "", true)}>
                            <div class="flex flex-none items-center gap-[4px]">
                                {for (index, (way, label)) in ways.into_iter().enumerate() {
                                    <div
                                        base={chip(label, self.addressing == way)}
                                        id={("way", index)}
                                        onClick={cx.listener(move |page, _, _, cx| {
                                            page.addressing = way;
                                            cx.notify();
                                        })}
                                    />
                                }}
                            </div>
                        </div>
                        {...(self.addressing == Addressing::Manual).then(|| rsx! {
                            <div class="flex flex-col flex-none">
                                {rule()}
                                {typed_row("Address", "With its prefix", &self.address)}
                                {rule()}
                                {typed_row("Gateway", "", &self.gateway)}
                            </div>
                        })}
                        {...(self.addressing != Addressing::Automatic).then(|| rsx! {
                            <div class="flex flex-col flex-none">
                                {rule()}
                                {typed_row("Name servers", "Commas between them", &self.dns)}
                            </div>
                        })}
                        {...changed.then(|| rsx! {
                            <div class="flex flex-none items-center justify-end gap-[14px] pt-[12px]">
                                {...objection.map(|why| rsx! { <div text_size={px(12.)} text_color={theme::alert()}>{why}</div> })}
                                <div
                                    base={button("check", if self.saving { "Applying" } else { "Apply" }, true)}
                                    id="apply"
                                    when={(objection.is_some() || self.saving, |button| button.opacity(0.45))}
                                    onClick={cx.listener(|page, _, _, cx| page.apply(cx))}
                                />
                            </div>
                        })}
                    </div>
                })}
                {...(!self.failure.is_empty()).then(|| rsx! {
                    <div class="pt-[10px]" text_size={px(12.)} text_color={theme::alert()}>{SharedString::from(self.failure.clone())}</div>
                })}
            </div>
        }
    }
}
