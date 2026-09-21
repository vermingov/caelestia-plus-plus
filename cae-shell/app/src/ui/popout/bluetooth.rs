//! Bluetooth: the adapter, whether it is looking, and what it knows.

use std::time::Duration;

use cae_core::services;
use gpui::{AppContext, Context, IntoElement, Render, Window, prelude::*};

use super::pieces::{caption, column, detail, entry, ghost, headline, list, row, toggle, wide};
use crate::feeds::Feeds;
use crate::ui::rsx;

pub struct Bluetooth {
    feeds: Feeds,
    devices: Vec<services::Device>,
}

impl Bluetooth {
    pub fn new(feeds: &Feeds, cx: &mut Context<Self>) -> Bluetooth {
        cx.observe(&feeds.services, |_, _, cx| cx.notify()).detach();
        let mut bluetooth = Bluetooth { feeds: feeds.clone(), devices: Vec::new() };
        bluetooth.look(Duration::ZERO, cx);
        bluetooth
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |bluetooth, cx| {
            cx.background_executor().timer(after).await;
            let devices = cx.background_spawn(async { services::devices() }).await;
            let _ = bluetooth.update(cx, |bluetooth, cx| {
                bluetooth.devices = devices;
                cx.notify();
            });
        })
        .detach();
    }

    /// Does something to the adapter or a device, and looks again once it
    /// has had time to take.
    fn ask(&mut self, wait: u64, work: impl FnOnce() + Send + 'static, cx: &mut Context<Self>) {
        cx.background_spawn(async move { work() }).detach();
        self.look(Duration::from_millis(wait), cx);
    }
}

impl Render for Bluetooth {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let adapter = self.feeds.services.read(cx).value.bluetooth.clone();
        let (powered, discovering) = (adapter.powered, adapter.discovering);

        let count = self.devices.len();
        let mut found = format!("{count} device{} available", if count == 1 { "" } else { "s" });
        if adapter.connected > 0 {
            found.push_str(&format!(" ({} connected)", adapter.connected));
        }

        rsx! {
            <div base={column()}>
                <div base={row()}>
                    {headline("Bluetooth")}
                    <div
                        base={toggle(powered)}
                        id="power"
                        onClick={cx.listener(move |bluetooth, _, _, cx| {
                            bluetooth.ask(900, move || services::set_bluetooth(!powered), cx)
                        })}
                    />
                </div>
                <div base={row()}>
                    {detail("Discovering")}
                    <div
                        base={toggle(discovering)}
                        id="discover"
                        onClick={cx.listener(move |bluetooth, _, _, cx| {
                            bluetooth.ask(900, move || services::set_discovering(!discovering), cx)
                        })}
                    />
                </div>
                {caption(found)}
                <div base={list()} id="devices" class="overflow-y-scroll">
                    {for (index, device) in self.devices.iter().enumerate() {
                        <div
                            base={entry(
                                if device.connected { "bluetooth_connected" } else { "bluetooth" },
                                device.name.clone(),
                                device.connected,
                            )}
                            id={("device", index)}
                            onClick={cx.listener({
                                let (address, connect) = (device.address.clone(), !device.connected);
                                move |bluetooth, _, _, cx| {
                                    let address = address.clone();
                                    bluetooth.ask(900, move || services::connect_device(&address, connect), cx)
                                }
                            })}
                        >
                            // Forgetting is deliberate, so it is its own
                            // target rather than something the row does.
                            <div
                                base={ghost("close")}
                                id={("forget", index)}
                                onClick={cx.listener({
                                    let address = device.address.clone();
                                    move |bluetooth, _, _, cx| {
                                        cx.stop_propagation();
                                        let address = address.clone();
                                        bluetooth.ask(600, move || services::forget_device(&address), cx)
                                    }
                                })}
                            />
                        </div>
                    }}
                    {...self.devices.is_empty().then(|| detail("Nothing paired").px(gpui::px(8.)))}
                </div>
                <div base={wide("settings", "Open settings")} id="settings" onClick={|_, _, cx| super::open_settings("bluetooth", cx)} />
            </div>
        }
    }
}
