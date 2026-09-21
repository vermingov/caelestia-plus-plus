//! Which pieces of the desktop are this shell's to draw yet.
//!
//! Each piece cae takes from Quickshell has a moment when both could draw it
//! (see `cae_core::handover`), so each asks here first. The answer is kept:
//! a Quickshell only changes it by being restarted, and the asking is a
//! program started and waited for. A piece that is not ours yet is asked
//! about again every so often, and one that is ours is never asked again.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cae_core::handover::{self, Answer};
use gpui::{App, AppContext, Global};

/// How often a Quickshell that has not stood a piece down is asked again.
const ASK_AGAIN: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Claim {
    ours: bool,
    asked: Option<Instant>,
}

#[derive(Default)]
struct Claims(HashMap<&'static str, Claim>);

impl Global for Claims {}

/// Whether `piece` is this shell's, as far as is known, asking again in the
/// background if it is not and has not been asked about lately. The first
/// time anything asks, the answer is no: a piece that waits a tick to appear
/// is better than a piece that appears twice.
pub fn is_ours(piece: &'static str, cx: &mut App) -> bool {
    let claim = cx.default_global::<Claims>().0.entry(piece).or_default();
    if claim.ours {
        return true;
    }
    if claim.asked.is_some_and(|asked| asked.elapsed() < ASK_AGAIN) {
        return false;
    }
    claim.asked = Some(Instant::now());

    cx.spawn(async move |cx| {
        let answer = cx.background_spawn(async move { handover::stood_down(piece) }).await;
        cx.update(|cx| cx.default_global::<Claims>().0.entry(piece).or_default().ours = answer == Answer::Ours);
    })
    .detach();
    false
}

/// Does `then` once it is known whose `piece` is, asking now if it is not
/// known to be ours. For a key or a press, which is somebody waiting: the
/// kept answer may be a no from before Quickshell had started answering.
///
/// This is also what keeps the two shells from passing a key back and forth.
/// A Quickshell that has stood a piece down passes its keys here; what is
/// passed back to Quickshell is only ever what it has just said is its own.
pub fn when_known(piece: &'static str, cx: &mut App, then: impl FnOnce(bool, &mut App) + 'static) {
    if cx.default_global::<Claims>().0.entry(piece).or_default().ours {
        return then(true, cx);
    }
    cx.spawn(async move |cx| {
        let answer = cx.background_spawn(async move { handover::stood_down(piece) }).await;
        cx.update(|cx| {
            let claim = cx.default_global::<Claims>().0.entry(piece).or_default();
            (claim.ours, claim.asked) = (answer == Answer::Ours, Some(Instant::now()));
            then(claim.ours, cx);
        });
    })
    .detach();
}
