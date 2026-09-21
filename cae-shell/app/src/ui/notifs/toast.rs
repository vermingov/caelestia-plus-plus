//! One toast, and the row of buttons a notification comes with.

use cae_core::notifs::{Notification, reason};
use gpui::{
    AnyElement, App, Bounds, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, Pixels, SharedString, Styled, Window,
    canvas, div, prelude::*, px,
};

use super::column::{ASIDE, Column, GAP, TOAST, Toast, What};
use super::parts::{self, CRITICAL};
use super::markup;
use crate::theme;
use crate::ui::controls::round;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

impl Column {
    /// The close button, the sender's own buttons, and copy.
    pub(super) fn actions(&self, notif: &Notification, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = notif.id;
        let (closing, copying) = (self.feeds.server.clone(), notif.clone());
        let copied = self.copied.contains_key(&id);
        // A press on a button is not the start of a drag of what it is on.
        let own = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation();

        rsx! {
            <div class="flex flex-none gap-[6px] pt-[10px]">
                <div
                    base={round("close", false)}
                    id={("dismiss", id as usize)}
                    onMouseDown={(MouseButton::Left, own)}
                    onClick={move |_, _, _| closing.close(id, reason::DISMISSED)}
                />
                {for (index, action) in notif.actions.iter().enumerate() {
                    <div
                        base={parts::action(&action.text)}
                        id={("action", id as usize * 64 + index)}
                        onMouseDown={(MouseButton::Left, own)}
                        onClick={{
                            let (server, pressed) = (self.feeds.server.clone(), action.identifier.clone());
                            move |_, _, _| server.invoke(id, &pressed)
                        }}
                    />
                }}
                <div
                    base={round(if copied { "inventory" } else { "content_copy" }, false)}
                    id={("copy", id as usize)}
                    onMouseDown={(MouseButton::Left, own)}
                    onClick={cx.listener(move |column, _, _, cx| column.copy(&copying, cx))}
                />
            </div>
        }
    }

    pub(super) fn toast(&self, toast: &Toast, cx: &mut Context<Self>) -> AnyElement {
        let notif = &toast.notif;
        let (id, what) = (notif.id, What::Toast(notif.id));
        let preview = markup::plain(&notif.body);
        let critical = notif.urgency == CRITICAL;

        let height = toast.height.value();
        let natural = toast.natural.get().max(1.);
        let aside = toast.aside.value() * ASIDE * if toast.leftwards { -1. } else { 1. };
        let measure = toast.natural.clone();

        let (holding, letting_go) = (self.feeds.server.clone(), self.feeds.server.clone());
        let (closing, leaving) = (self.feeds.server.clone(), toast.leaving);

        let content = rsx! {
            <div class="relative flex flex-none items-start gap-[12px] pt-[12px] pr-[10px] pb-[12px] pl-[14px]">
                <canvas
                    class="absolute size-full"
                    prepaint={move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                        // How tall it wants to be is only known once it has
                        // been laid out, and what it is given follows a frame
                        // behind, which is what lets a change of height be a
                        // movement rather than a jump.
                        let height = f32::from(bounds.size.height);
                        if (measure.replace(height) - height).abs() > 0.5 {
                            window.request_animation_frame();
                        }
                    }}
                    paint={|_, _, _, _| ()}
                />
                {parts::avatar(notif, &notif.image, &notif.app_icon, notif.urgency, false)}
                <div class="flex flex-col flex-1 min-w-[0px] pt-[2px]">
                    // Who sent it is only worth a line once there is room.
                    {...toast.expanded.then(|| rsx! {
                        <div class="truncate pb-[2px]" text_size={px(11.)} text_color={theme::text_faint()}>
                            {SharedString::from(notif.app_name.clone())}
                        </div>
                    })}
                    <div class="flex items-baseline gap-[8px] min-w-[0px]">
                        <div
                            class="flex-1 min-w-[0px] font-medium"
                            text_size={px(13.)}
                            when={(!toast.expanded, |summary| summary.truncate())}
                        >
                            {SharedString::from(notif.summary.clone())}
                        </div>
                        {parts::time(notif.time, self.now)}
                    </div>
                    {...(!toast.expanded && !preview.is_empty()).then(|| rsx! {
                        <div class="truncate pt-[2px]" text_size={px(12.)} text_color={theme::text_dim()}>
                            {SharedString::from(preview.clone())}
                        </div>
                    })}
                    {...(toast.expanded && !notif.body.is_empty()).then(|| parts::body(id, &notif.body))}
                    {...toast.expanded.then(|| self.actions(notif, cx))}
                </div>
                <div
                    id={("chevron", id as usize)}
                    class="flex flex-none items-center justify-center size-[24px] rounded-full cursor-pointer"
                    text_color={theme::text_dim()}
                    hover={|style| style.bg(theme::white(0.08)).text_color(theme::text())}
                    onMouseDown={(MouseButton::Left, |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation())}
                    onClick={cx.listener(move |column, _, _, cx| {
                        if let Some(toast) = column.toasts.iter_mut().find(|toast| toast.notif.id == id) {
                            toast.expanded = !toast.expanded;
                        }
                        cx.notify();
                    })}
                >
                    {glyph(if toast.expanded { "expand_less" } else { "expand_more" }, px(18.))}
                </div>
            </div>
        };

        rsx! {
            // The room it takes, which closes up behind it as it leaves and
            // opens ahead of it as it arrives, so that the others move over
            // rather than jump.
            <div class="relative flex-none w-full" h={px(height + GAP * (height / natural).min(1.))}>
                {self.reachable()}
                <div
                    id={("toast", id as usize)}
                    class="absolute flex flex-col w-full overflow-hidden"
                    top={px(0.)}
                    left={px(aside + self.pulled(&what))}
                    h={px(height)}
                    opacity={toast.shown.value()}
                    rounded={theme::RADIUS}
                    bg={theme::panel()}
                    shadow={if critical { theme::critical_shadows() } else { theme::panel_shadows() }}
                    text_color={theme::text()}
                    // Under the pointer it keeps its place: its clock stops
                    // when the pointer arrives and starts again, from the
                    // top, when it goes.
                    onHover={move |hovered: &bool, _: &mut Window, _: &mut App| {
                        if !leaving {
                            if *hovered { holding.hold(id) } else { letting_go.release(id) }
                        }
                    }}
                    onMouseDown={(MouseButton::Left, cx.listener(move |column, _, _, cx| {
                        column.press(What::Toast(id), f32::from(TOAST));
                        cx.notify();
                    }))}
                    // The middle button throws it away outright.
                    onMouseDown={(MouseButton::Middle, move |_: &MouseDownEvent, _: &mut Window, _: &mut App| {
                        closing.close(id, reason::DISMISSED)
                    })}
                >
                    {content}
                </div>
            </div>
        }
        .into_any_element()
    }
}
