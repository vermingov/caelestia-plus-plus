//! How hard the machine is working: the processor and the graphics card,
//! memory, the disks, the network and the battery.
//!
//! Read while this page is up and not otherwise. The bar already keeps the
//! loads and the memory; the temperatures, the disks and the traffic are
//! this page's own, and cost nothing when nobody is looking at them.

use std::collections::VecDeque;
use std::time::Duration;

use cae_core::{config, gpus, machine};
use gpui::{
    AnyElement, AppContext, Bounds, Context, FontWeight, Hsla, IntoElement, PathBuilder, Pixels, Render, SharedString, Styled, Window,
    canvas, div, point, prelude::*, px, relative,
};

use super::pane::Reach;
use crate::theme;
use crate::ui::dial::{Dial, dial};
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// How many readings of the network are kept: a minute of them, at the rate
/// they are usually taken.
const HISTORY: usize = 60;
/// How often the disks are looked at. They fill at the pace of a download,
/// not of a frame.
const DISKS_EVERY: Duration = Duration::from_secs(10);

/// Which of the page's parts the settings leave on it.
struct Shown {
    cpu: bool,
    gpu: bool,
    memory: bool,
    storage: bool,
    network: bool,
    battery: bool,
    fahrenheit: bool,
    every: Duration,
}

impl Shown {
    fn read() -> Shown {
        let shell = config::read(config::File::Shell);
        let flag = |path: &str, otherwise: bool| config::lookup(&shell, path).and_then(serde_json::Value::as_bool).unwrap_or(otherwise);
        let every = config::lookup(&shell, "dashboard.resourceUpdateInterval").and_then(serde_json::Value::as_u64).unwrap_or(1000);
        Shown {
            cpu: flag("dashboard.performance.showCpu", true),
            gpu: flag("dashboard.performance.showGpu", true),
            memory: flag("dashboard.performance.showMemory", true),
            storage: flag("dashboard.performance.showStorage", true),
            network: flag("dashboard.performance.showNetwork", true),
            battery: flag("dashboard.performance.showBattery", true),
            fahrenheit: flag("services.useFahrenheitPerformance", false),
            every: Duration::from_millis(every.clamp(250, 10_000)),
        }
    }
}

/// What is read on every tick.
struct Reading {
    cpu_celsius: Option<f64>,
    gpu: machine::Gpu,
    traffic: machine::Traffic,
}

pub struct Performance {
    reach: Reach,
    shown: Shown,
    cpu_name: String,
    gpu_name: String,
    reading: Option<Reading>,
    disks: Vec<machine::Disk>,
    /// Which disk the storage part is about, by where it is mounted.
    disk: String,
    down: VecDeque<f64>,
    up: VecDeque<f64>,
}

impl Performance {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Performance {
        cx.observe(&reach.feeds.system, |_, _, cx| cx.notify()).detach();
        let shown = Shown::read();
        let every = shown.every;

        cx.spawn(async move |page, cx| {
            let names = cx.background_spawn(async { (machine::cpu_name(), gpus::list().into_iter().next().map(|gpu| gpu.name)) }).await;
            let _ = page.update(cx, |page: &mut Performance, cx| {
                (page.cpu_name, page.gpu_name) = (names.0, names.1.unwrap_or_default());
                cx.notify();
            });
        })
        .detach();

        cx.spawn(async move |page, cx| {
            // The meter lives with the loop that reads it: a rate is the
            // difference between two readings, and these are them.
            let mut meter = Some(machine::TrafficMeter::new());
            loop {
                cx.background_executor().timer(every).await;
                let mut held = meter.take().unwrap_or_default();
                let (reading, held) = cx
                    .background_spawn(async move {
                        let reading = Reading { cpu_celsius: machine::cpu_celsius(), gpu: machine::gpu(), traffic: held.read() };
                        (reading, held)
                    })
                    .await;
                meter = Some(held);
                let landed = page.update(cx, |page: &mut Performance, cx| {
                    for (history, rate) in [(&mut page.down, reading.traffic.down), (&mut page.up, reading.traffic.up)] {
                        history.push_back(rate);
                        while history.len() > HISTORY {
                            history.pop_front();
                        }
                    }
                    page.reading = Some(reading);
                    cx.notify();
                });
                if landed.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |page, cx| {
            loop {
                let disks = cx.background_spawn(async { machine::disks() }).await;
                let landed = page.update(cx, |page: &mut Performance, cx| {
                    if !disks.iter().any(|disk| disk.mount == page.disk) {
                        page.disk = disks.first().map(|disk| disk.mount.clone()).unwrap_or_default();
                    }
                    page.disks = disks;
                    cx.notify();
                });
                if landed.is_err() {
                    break;
                }
                cx.background_executor().timer(DISKS_EVERY).await;
            }
        })
        .detach();

        Performance {
            reach: reach.clone(),
            shown,
            cpu_name: String::new(),
            gpu_name: String::new(),
            reading: None,
            disks: Vec::new(),
            disk: String::new(),
            down: VecDeque::new(),
            up: VecDeque::new(),
        }
    }

    fn degrees(&self, celsius: Option<f64>) -> String {
        match celsius {
            Some(celsius) if self.shown.fahrenheit => format!("{}°F", (celsius * 1.8 + 32.).round() as i64),
            Some(celsius) => format!("{}°C", celsius.round() as i64),
            None => "No sensor".to_string(),
        }
    }

    /// The processor, or the graphics card: what it is, how busy, how hot.
    fn worker(&self, symbol: &'static str, what: &'static str, name: &str, busy: Option<f64>, celsius: Option<f64>) -> gpui::Div {
        // Hot is where a laptop starts to slow itself down to cool off.
        let hot = celsius.is_some_and(|celsius| celsius >= 90.);
        let warmth = (celsius.unwrap_or(0.) / 100.).clamp(0., 1.) as f32;
        rsx! {
            <div class="flex flex-1 items-center gap-[20px] min-w-[0px] px-[24px] py-[20px]">
                <div class="relative flex flex-none items-center justify-center size-[92px]">
                    <div class="absolute">{dial(busy.unwrap_or(0.), Dial::gauge(px(92.), px(6.)))}</div>
                    <div class="flex flex-col items-center">
                        <div text_size={px(20.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>
                            {busy.map_or("--".to_string(), |busy| format!("{}%", busy.round() as i64))}
                        </div>
                    </div>
                </div>
                <div class="flex flex-col flex-1 gap-[6px] min-w-[0px]">
                    <div class="flex items-center gap-[8px]" text_size={px(14.)} font_weight={FontWeight::MEDIUM}>
                        <div text_color={theme::text_dim()}>{glyph(symbol, px(18.))}</div>
                        {what}
                    </div>
                    <div class="truncate" text_size={px(12.)} text_color={theme::text_faint()}>{SharedString::from(name.to_string())}</div>
                    <div class="flex items-center gap-[8px] pt-[6px]" text_size={px(12.)} text_color={if hot { theme::alert() } else { theme::text_dim() }}>
                        {glyph(if hot { "thermometer_alert" } else { "thermometer" }, px(16.))}
                        <div font_features={theme::tabular()}>{self.degrees(celsius)}</div>
                    </div>
                    <div class="flex-none h-[4px] rounded-full overflow-hidden" bg={theme::white(0.07)}>
                        <div class="h-full rounded-full" w={relative(warmth)} bg={if hot { theme::alert() } else { theme::white(0.45) }} />
                    </div>
                </div>
            </div>
        }
    }

    fn memory(&self, cx: &Context<Self>) -> gpui::Div {
        let system = &self.reach.feeds.system.read(cx).value;
        rsx! {
            <div class="flex flex-col flex-1 items-center justify-center gap-[10px] min-w-[0px] py-[16px]">
                <div class="relative flex flex-none items-center justify-center size-[84px]">
                    <div class="absolute">{dial(system.memory, Dial::gauge(px(84.), px(6.)))}</div>
                    <div text_size={px(18.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>
                        {format!("{}%", system.memory.round() as i64)}
                    </div>
                </div>
                <div class="flex items-center gap-[7px]" text_size={px(13.)}>
                    <div text_color={theme::text_dim()}>{glyph("memory_alt", px(16.))}</div>
                    {"Memory"}
                </div>
                <div text_size={px(12.)} text_color={theme::text_faint()} font_features={theme::tabular()}>
                    {format!("{:.1} of {:.1} GB", system.memory_used_gb, system.memory_total_gb)}
                </div>
            </div>
        }
    }

    fn storage(&self, cx: &mut Context<Self>) -> gpui::Div {
        let chosen = self.disks.iter().find(|disk| disk.mount == self.disk);
        let (percent, amount) = match chosen {
            Some(disk) => (disk.percent(), format!("{} of {}", machine::bytes(disk.used as f64), machine::bytes(disk.total as f64))),
            None => (0., "No disks".to_string()),
        };
        let others: Vec<AnyElement> = self
            .disks
            .iter()
            .enumerate()
            .map(|(index, disk)| {
                let (mount, is_chosen) = (disk.mount.clone(), disk.mount == self.disk);
                // The last part of where it is mounted is what it is called:
                // the whole path of a plugged-in disk is mostly its owner.
                let name = if disk.mount == "/" { "System".to_string() } else { disk.mount.rsplit('/').next().unwrap_or_default().to_string() };
                rsx! {
                    <div
                        id={("disk", index)}
                        class="flex flex-none items-center h-[22px] px-[9px] rounded-full cursor-pointer max-w-[110px]"
                        text_size={px(11.)}
                        bg={theme::white(if is_chosen { 0.16 } else { 0.06 })}
                        text_color={if is_chosen { theme::text() } else { theme::text_dim() }}
                        onClick={cx.listener(move |page, _, _, cx| {
                            page.disk = mount.clone();
                            cx.notify();
                        })}
                    >
                        <div class="truncate">{name}</div>
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div class="flex flex-col flex-1 items-center justify-center gap-[10px] min-w-[0px] py-[16px]">
                <div class="relative flex flex-none items-center justify-center size-[84px]">
                    <div class="absolute">{dial(percent, Dial::gauge(px(84.), px(6.)))}</div>
                    <div text_size={px(18.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>
                        {format!("{}%", percent.round() as i64)}
                    </div>
                </div>
                <div class="flex items-center gap-[7px]" text_size={px(13.)}>
                    <div text_color={theme::text_dim()}>{glyph("hard_drive", px(16.))}</div>
                    {"Storage"}
                </div>
                <div text_size={px(12.)} text_color={theme::text_faint()} font_features={theme::tabular()}>{amount}</div>
                {...(self.disks.len() > 1).then(|| rsx! { <div class="flex flex-wrap justify-center gap-[4px] px-[10px]">{...others}</div> })}
            </div>
        }
    }

    fn network(&self) -> gpui::Div {
        let traffic = self.reading.as_ref().map(|reading| reading.traffic).unwrap_or_default();
        let (down, up): (Vec<f64>, Vec<f64>) = (self.down.iter().copied().collect(), self.up.iter().copied().collect());
        let rate = |symbol: &'static str, name: &'static str, amount: String, colour: Hsla| {
            rsx! {
                <div class="flex items-center gap-[8px]" text_size={px(12.)}>
                    <div text_color={colour}>{glyph(symbol, px(15.))}</div>
                    <div class="flex-1" text_color={theme::text_dim()}>{name}</div>
                    <div text_color={theme::text()} font_features={theme::tabular()}>{amount}</div>
                </div>
            }
        };
        rsx! {
            <div class="flex flex-col flex-1 gap-[10px] min-w-[0px] px-[22px] py-[18px]">
                <div class="flex items-center gap-[7px]" text_size={px(13.)}>
                    <div text_color={theme::text_dim()}>{glyph("swap_vert", px(16.))}</div>
                    {"Network"}
                </div>
                <canvas
                    class="flex-1 w-full min-h-[0px]"
                    prepaint={|_, _, _| ()}
                    paint={move |bounds, _, window, _| paint_traffic(bounds, &down, &up, window)}
                />
                {rate("download", "Down", format!("{}/s", machine::bytes(traffic.down)), theme::accent())}
                {rate("upload", "Up", format!("{}/s", machine::bytes(traffic.up)), theme::white(0.6))}
                {rate("history", "Since startup", format!("{} down, {} up", machine::bytes(traffic.received as f64), machine::bytes(traffic.sent as f64)), theme::text_faint())}
            </div>
        }
    }

    fn battery(&self, cx: &Context<Self>) -> Option<gpui::Div> {
        let battery = self.reach.feeds.system.read(cx).value.battery.clone()?;
        let says = match (battery.charging, battery.on_mains, battery.minutes) {
            _ if battery.level >= 100 => "Full".to_string(),
            (true, _, Some(minutes)) if minutes > 0 => format!("Full in {}", hours(minutes)),
            (true, ..) => "Charging".to_string(),
            (false, true, _) => "Plugged in".to_string(),
            (false, false, Some(minutes)) => format!("{} left", hours(minutes)),
            (false, false, None) => "On battery".to_string(),
        };
        let low = battery.level <= 15 && !battery.on_mains;
        Some(rsx! {
            <div class="flex flex-col flex-none items-center justify-center gap-[10px] w-[150px] py-[16px]">
                <div class="relative flex flex-none items-center justify-center size-[84px]">
                    <div class="absolute">
                        {dial(battery.level as f64, Dial::gauge(px(84.), px(6.)).coloured(if low { theme::alert() } else { theme::accent() }))}
                    </div>
                    <div text_size={px(18.)} font_weight={FontWeight::SEMIBOLD} font_features={theme::tabular()}>{format!("{}%", battery.level)}</div>
                </div>
                <div class="flex items-center gap-[7px]" text_size={px(13.)}>
                    <div text_color={theme::text_dim()}>{glyph(if battery.charging { "battery_charging_full" } else { "battery_full" }, px(16.))}</div>
                    {"Battery"}
                </div>
                <div text_size={px(12.)} text_color={theme::text_faint()}>{says}</div>
            </div>
        })
    }
}

fn hours(minutes: i64) -> String {
    if minutes >= 60 { format!("{} h {} min", minutes / 60, minutes % 60) } else { format!("{minutes} min") }
}

/// The last minute of traffic as two lines, down over up, to the scale of
/// whichever was the busier.
fn paint_traffic(bounds: Bounds<Pixels>, down: &[f64], up: &[f64], window: &mut Window) {
    // A floor under the scale, so that a quiet network is a flat line and
    // not its own noise blown up to fill the height.
    let most = down.iter().chain(up).copied().fold(64. * 1024., f64::max);
    let line = |readings: &[f64]| {
        if readings.len() < 2 {
            return None;
        }
        let step = bounds.size.width / (HISTORY - 1) as f32;
        // Against the right-hand edge: what is newest is where the eye ends.
        let first = bounds.right() - step * (readings.len() - 1) as f32;
        let at = |index: usize, reading: f64| {
            let up = (reading / most).clamp(0., 1.) as f32;
            point(first + step * index as f32, bounds.bottom() - px(1.) - (bounds.size.height - px(2.)) * up)
        };
        let mut path = PathBuilder::stroke(px(1.5));
        path.move_to(at(0, readings[0]));
        for (index, reading) in readings.iter().enumerate().skip(1) {
            path.line_to(at(index, *reading));
        }
        path.build().ok()
    };
    if let Some(sent) = line(up) {
        window.paint_path(sent, theme::white(0.5));
    }
    if let Some(received) = line(down) {
        window.paint_path(received, theme::accent());
    }
}

impl Render for Performance {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let system = self.reach.feeds.system.read(cx).value.clone();
        let reading = self.reading.as_ref();
        let has_gpu = self.shown.gpu && (system.gpu.is_some() || !self.gpu_name.is_empty());
        let down_rule = || rsx! { <div class="flex-none w-[1px] h-full" bg={theme::white(0.06)} /> };

        let workers: Vec<AnyElement> = [
            self.shown.cpu.then(|| self.worker("memory", "Processor", &self.cpu_name, Some(system.cpu), reading.and_then(|reading| reading.cpu_celsius))),
            has_gpu.then(|| {
                let gpu = reading.map(|reading| reading.gpu.clone()).unwrap_or_default();
                self.worker("desktop_windows", "Graphics", &self.gpu_name, gpu.busy.or(system.gpu), gpu.celsius)
            }),
        ]
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(index, part)| [(index > 0).then(|| down_rule().into_any_element()), Some(part.into_any_element())])
        .flatten()
        .collect();

        let battery = self.shown.battery.then(|| self.battery(cx)).flatten();
        let lower: Vec<AnyElement> = [
            self.shown.memory.then(|| self.memory(cx)),
            self.shown.storage.then(|| self.storage(cx)),
            self.shown.network.then(|| self.network()),
            battery,
        ]
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(index, part)| [(index > 0).then(|| down_rule().into_any_element()), Some(part.into_any_element())])
        .flatten()
        .collect();

        if workers.is_empty() && lower.is_empty() {
            return rsx! {
                <div class="flex flex-col size-full items-center justify-center gap-[8px]" text_color={theme::text_faint()}>
                    {glyph("tune", px(26.))}
                    <div text_size={px(13.)}>{"Every part of this page is switched off in the settings"}</div>
                </div>
            };
        }
        let both = !workers.is_empty() && !lower.is_empty();
        rsx! {
            <div class="flex flex-col size-full">
                {...(!workers.is_empty()).then(|| rsx! { <div class="flex flex-none">{...workers}</div> })}
                {...both.then(|| rsx! { <div class="flex-none h-[1px] w-full" bg={theme::white(0.06)} /> })}
                {...(!lower.is_empty()).then(|| rsx! { <div class="flex flex-1 min-h-[0px]">{...lower}</div> })}
            </div>
        }
    }
}
