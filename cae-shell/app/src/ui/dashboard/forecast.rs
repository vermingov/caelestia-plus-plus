//! The last forecast, kept for as long as the shell runs.
//!
//! A forecast is three requests and a few seconds. The dashboard is open for
//! a few seconds at a time. So what it shows is what was fetched last, at
//! once, and the network is only gone to when that has gone stale, while the
//! old one is still on the screen.

use cae_core::{config, weather};
use gpui::{AppContext, Context};

#[derive(Default)]
pub struct Forecast {
    pub weather: Option<weather::Weather>,
    asking: bool,
}

impl Forecast {
    /// Makes sure what is kept is worth showing: read from the disk if
    /// nothing is in hand, and fetched again if it is old.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.asking || self.weather.as_ref().is_some_and(weather::Weather::is_fresh) {
            return;
        }
        self.asking = true;
        let in_hand = self.weather.is_some();
        cx.spawn(async move |forecast, cx| {
            if !in_hand {
                let kept = cx.background_spawn(async { weather::kept() }).await;
                let fresh = kept.as_ref().is_some_and(weather::Weather::is_fresh);
                let _ = forecast.update(cx, |forecast: &mut Forecast, cx| {
                    forecast.weather = kept;
                    forecast.asking = !fresh;
                    cx.notify();
                });
                if fresh {
                    return;
                }
            }
            let fetched = cx
                .background_spawn(async {
                    let shell = config::read(config::File::Shell);
                    let place = config::lookup(&shell, "services.weatherLocation").and_then(serde_json::Value::as_str).unwrap_or_default();
                    weather::fetch(place)
                })
                .await;
            let _ = forecast.update(cx, |forecast: &mut Forecast, cx| {
                forecast.asking = false;
                // A fetch that failed leaves the old forecast, which is
                // still a better thing to show than nothing.
                if fetched.is_some() {
                    forecast.weather = fetched;
                }
                cx.notify();
            });
        })
        .detach();
    }
}
