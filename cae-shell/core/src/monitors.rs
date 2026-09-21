//! The screens, as the compositor has them, and the arithmetic of arranging
//! them.
//!
//! Hyprland says what each monitor is and where it is. Where it should be is
//! the person's to say, by dragging a picture of it, and a picture dragged
//! freely ends up overlapping its neighbour or adrift from it. What keeps an
//! arrangement one that a compositor can use is here, apart from anything
//! that draws, so that it can be tested without a screen.

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    #[serde(default = "one")]
    pub scale: f64,
    /// Quarter turns and flips, as Hyprland numbers them: the odd ones lie
    /// on their side.
    #[serde(default)]
    pub transform: i64,
    #[serde(default)]
    pub refresh_rate: f64,
    #[serde(default)]
    pub disabled: bool,
    /// `1920x1200@60.00Hz`, every one the panel offers.
    #[serde(default)]
    pub available_modes: Vec<String>,
}

fn one() -> f64 {
    1.
}

impl Monitor {
    /// The mode it is in, the way the config writes one.
    pub fn mode(&self) -> String {
        format!("{}x{}@{}", self.width, self.height, self.refresh_rate.round())
    }

    /// Where it is and how much room it takes among the others, which is
    /// its pixels less its scale, and turned if it is.
    pub fn placed(&self) -> Placed {
        let (across, down) = if self.transform % 2 == 1 { (self.height, self.width) } else { (self.width, self.height) };
        let scale = if self.scale > 0. { self.scale } else { 1. };
        Placed {
            x: self.x,
            y: self.y,
            w: (across as f64 / scale).round() as i64,
            h: (down as f64 / scale).round() as i64,
        }
    }
}

/// A mode as the config writes it, from one as the panel lists it:
/// `1920x1200@60.00Hz` is `1920x1200@60`.
pub fn mode_from(listed: &str) -> String {
    let plain = listed.trim_end_matches("Hz");
    match plain.split_once('@') {
        Some((size, rate)) => format!("{size}@{}", rate.parse::<f64>().map_or_else(|_| rate.to_string(), |rate| rate.round().to_string())),
        None => plain.to_string(),
    }
}

/// Every monitor the compositor knows, the ones switched off included.
pub fn list() -> Vec<Monitor> {
    let listed = std::process::Command::new("hyprctl").args(["-j", "monitors", "all"]).output();
    listed.ok().and_then(|output| serde_json::from_slice(&output.stdout).ok()).unwrap_or_default()
}

/// A monitor's place among the others, in the compositor's own units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Placed {
    fn overlaps(&self, other: &Placed) -> bool {
        self.x < other.x + other.w && self.x + self.w > other.x && self.y < other.y + other.h && self.y + self.h > other.y
    }
}

/// Positions snap to this, so that two monitors can be lined up by hand.
const GRID: i64 = 10;

/// Where a monitor dragged to `wanted` comes to rest among `others`, having
/// started the drag at `start`.
///
/// On the grid; pushed out of any monitor it would overlap, by the shorter
/// way out that does not cross the other one; and kept within reach of the
/// rest, so that it cannot be lost a desert away from them.
pub fn settle(dragged: Placed, start: (i64, i64), wanted: (i64, i64), others: &[Placed]) -> (i64, i64) {
    let snap = |value: i64| (value as f64 / GRID as f64).round() as i64 * GRID;
    let (x, y) = push_out(dragged, start, (snap(wanted.0), snap(wanted.1)), others);
    keep_near(dragged, (x, y), others)
}

fn push_out(dragged: Placed, start: (i64, i64), wanted: (i64, i64), others: &[Placed]) -> (i64, i64) {
    let (mut x, mut y) = wanted;
    let Placed { w, h, .. } = dragged;
    for other in others {
        if !(Placed { x, y, w, h }).overlaps(other) {
            continue;
        }
        // Which side of the other it started on is the side it goes back to:
        // pushed out the other way it would have jumped across.
        let beside = start.0 + w <= other.x || start.0 >= other.x + other.w;
        let above_or_below = start.1 + h <= other.y || start.1 >= other.y + other.h;
        let left_of_centre = x + w / 2 < other.x + other.w / 2;
        let above_centre = y + h / 2 < other.y + other.h / 2;

        let sideways = if left_of_centre { (other.x - w, y, x + w - other.x) } else { (other.x + other.w, y, other.x + other.w - x) };
        let up_or_down = if above_centre { (x, other.y - h, y + h - other.y) } else { (x, other.y + other.h, other.y + other.h - y) };

        let (to_x, to_y, _) = match (beside, above_or_below) {
            (true, true) => if sideways.2 <= up_or_down.2 { sideways } else { up_or_down },
            (true, false) => sideways,
            (false, true) => up_or_down,
            // It started on top of the other, which only a saved layout
            // gone wrong does: out by whichever way is nearer, measured
            // against the sizes so that a wide monitor is not always "up".
            (false, false) => {
                let across = (x + w / 2 - other.x - other.w / 2).abs() * (h + other.h);
                let down = (y + h / 2 - other.y - other.h / 2).abs() * (w + other.w);
                if across >= down { sideways } else { up_or_down }
            }
        };
        (x, y) = (to_x, to_y);
    }
    (x, y)
}

fn keep_near(dragged: Placed, at: (i64, i64), others: &[Placed]) -> (i64, i64) {
    if others.is_empty() {
        return at;
    }
    let reach_across = (others.iter().map(|other| other.w).sum::<i64>() + dragged.w) * 3;
    let reach_down = (others.iter().map(|other| other.h).sum::<i64>() + dragged.h) * 3;
    let left = others.iter().map(|other| other.x).min().unwrap_or(0);
    let top = others.iter().map(|other| other.y).min().unwrap_or(0);
    let right = others.iter().map(|other| other.x + other.w).max().unwrap_or(0);
    let bottom = others.iter().map(|other| other.y + other.h).max().unwrap_or(0);

    let x = at.0.clamp(left.min(right - reach_across), right.max(left + reach_across) - dragged.w);
    let y = at.1.clamp(top.min(bottom - reach_down), bottom.max(top + reach_down) - dragged.h);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: Placed = Placed { x: 0, y: 0, w: 1920, h: 1200 };
    const EXTERNAL: Placed = Placed { x: 1920, y: 0, w: 2560, h: 1440 };

    #[test]
    fn a_monitor_takes_the_room_its_scale_and_turn_leave_it() {
        let panel: Monitor = serde_json::from_str(
            r#"{"name": "eDP-1", "x": 0, "y": 0, "width": 2880, "height": 1800, "scale": 1.5, "transform": 0, "refreshRate": 59.999,
                "availableModes": ["2880x1800@60.00Hz"]}"#,
        )
        .unwrap();
        assert_eq!(panel.placed(), Placed { x: 0, y: 0, w: 1920, h: 1200 });
        assert_eq!(panel.mode(), "2880x1800@60");

        let on_its_side = Monitor { transform: 1, ..panel };
        assert_eq!((on_its_side.placed().w, on_its_side.placed().h), (1200, 1920));
    }

    #[test]
    fn a_listed_mode_is_written_the_way_the_config_writes_one() {
        assert_eq!(mode_from("1920x1200@60.00Hz"), "1920x1200@60");
        assert_eq!(mode_from("2560x1440@143.91Hz"), "2560x1440@144");
        assert_eq!(mode_from("preferred"), "preferred");
    }

    #[test]
    fn a_drag_lands_on_the_grid() {
        assert_eq!(settle(EXTERNAL, (1920, 0), (1923, 247), &[LAPTOP]), (1920, 250));
    }

    #[test]
    fn a_monitor_dropped_on_another_is_pushed_back_out_the_side_it_came_from() {
        // Dragged left from the laptop's right, a long way into it.
        assert_eq!(settle(EXTERNAL, (1920, 0), (400, 0), &[LAPTOP]), (1920, 0));
        // From below it, up into it: back down, not out sideways.
        assert_eq!(settle(EXTERNAL, (0, 1200), (0, 300), &[LAPTOP]), (0, 1200));
    }

    #[test]
    fn a_monitor_cannot_be_dragged_out_of_reach_of_the_rest() {
        let (x, y) = settle(EXTERNAL, (1920, 0), (900_000, -900_000), &[LAPTOP]);
        assert!(x < 20_000 && y > -20_000, "it went to {x}, {y}");
        // A monitor on its own has nothing to stay near.
        assert_eq!(settle(LAPTOP, (0, 0), (5000, 5000), &[]), (5000, 5000));
    }
}
