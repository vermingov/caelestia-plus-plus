//! The centre: everything that has arrived and not been dealt with, under
//! the name of whoever sent it.

use cae_core::notifs::{Notification, reason};
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, SharedString, Styled, Window, div,
    prelude::*, px,
};

use super::column::{ASIDE, CENTRE, Column, What};
use super::parts::{self, CRITICAL};
use super::markup;
use crate::theme;
use crate::ui::controls::round;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

impl Column {
    fn row(&self, notif: &Notification, expanded: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = notif.id;
        let excerpt = markup::plain(&notif.body);
        let closing = self.feeds.server.clone();
        rsx! {
            <div
                id={("row", id as usize)}
                class="relative flex flex-col flex-none ml-[-8px]"
                left={px(self.pulled(&What::Row(id)))}
                rounded={px(9.)}
                when={(expanded, |row| row.mb(px(5.)).pt(px(9.)).px(px(10.)).pb(px(10.)).bg(theme::white(0.045)))}
                when={(!expanded, |row| row.py(px(3.)).px(px(8.)))}
                onMouseDown={(MouseButton::Left, cx.listener(move |column, _, _, cx| {
                    column.press(What::Row(id), f32::from(CENTRE));
                    cx.notify();
                }))}
                onMouseDown={(MouseButton::Middle, move |_: &MouseDownEvent, _: &mut Window, _: &mut App| {
                    closing.close(id, reason::DISMISSED)
                })}
            >
                <div class="flex items-baseline gap-[8px] min-w-[0px]" text_size={px(12.5)}>
                    <div
                        class="min-w-[0px]"
                        text_color={if notif.urgency == CRITICAL { theme::alert() } else { theme::text() }}
                        when={(expanded, |summary| summary.flex_1().font_weight(FontWeight::MEDIUM))}
                        when={(!expanded, |summary| summary.flex_none().max_w(gpui::relative(0.62)).truncate())}
                    >
                        {SharedString::from(notif.summary.clone())}
                    </div>
                    {...(!expanded && !excerpt.is_empty()).then(|| rsx! {
                        <div class="flex-1 min-w-[0px] truncate" text_color={theme::text_faint()}>
                            {SharedString::from(excerpt.clone())}
                        </div>
                    })}
                    {...expanded.then(|| parts::time(notif.time, self.now))}
                </div>
                {...(expanded && !notif.body.is_empty()).then(|| parts::body(id, &notif.body))}
                {...expanded.then(|| self.actions(notif, cx))}
            </div>
        }
        .into_any_element()
    }

    /// Everything one application sent, under its name. Newest first.
    fn group(&self, index: usize, app: &str, notifs: &[&Notification], cx: &mut Context<Self>) -> AnyElement {
        let newest = notifs[0];
        let expanded = self.centre.unfolded.contains(app);
        let listed = if expanded { notifs.len() } else { (self.config.group_preview_num.max(1) as usize).min(notifs.len()) };

        // The group wears the most urgent face of anything in it, and
        // whichever pictures its notifications brought most recently.
        let urgency = notifs.iter().map(|notif| notif.urgency).max().unwrap_or(1);
        let image = notifs.iter().find(|notif| !notif.image.is_empty()).map_or("", |notif| notif.image.as_str());
        let app_icon = notifs.iter().find(|notif| !notif.app_icon.is_empty()).map_or("", |notif| notif.app_icon.as_str());

        let name = if app.is_empty() { "Unknown" } else { app };
        let (pressed, flipped, toggled, closing) = (app.to_string(), app.to_string(), app.to_string(), self.feeds.server.clone());
        let thrown = app.to_string();
        let flip = |column: &mut Column, app: &str| {
            if !column.centre.unfolded.remove(app) {
                column.centre.unfolded.insert(app.to_string());
            }
        };

        rsx! {
            <div class="flex flex-col flex-none">
                <div
                    id={("group", index)}
                    class="relative flex items-center gap-[10px]"
                    left={px(self.pulled(&What::Group(app.to_string())))}
                    // Swiping the heading throws away everything the
                    // application sent.
                    onMouseDown={(MouseButton::Left, cx.listener(move |column, _, _, cx| {
                        column.press(What::Group(pressed.clone()), f32::from(CENTRE));
                        cx.notify();
                    }))}
                    onMouseDown={(MouseButton::Middle, move |_: &MouseDownEvent, _: &mut Window, _: &mut App| {
                        closing.close_app(&thrown)
                    })}
                    onMouseDown={(MouseButton::Right, cx.listener(move |column, _, _, cx| {
                        flip(column, &flipped);
                        cx.notify();
                    }))}
                >
                    {parts::avatar(newest, image, app_icon, urgency, true)}
                    <div class="flex-1 min-w-[0px] truncate" text_size={px(12.)} text_color={theme::text_dim()}>
                        {SharedString::from(name.to_string())}
                    </div>
                    {parts::time(newest.time, self.now)}
                    <div
                        id={("count", index)}
                        class="flex flex-none items-center gap-[1px] h-[22px] pl-[9px] pr-[3px] rounded-full cursor-pointer"
                        bg={theme::white(0.07)}
                        text_size={px(11.5)}
                        font_features={theme::tabular()}
                        text_color={theme::text_dim()}
                        hover={|style| style.bg(theme::white(0.13)).text_color(theme::text())}
                        onMouseDown={(MouseButton::Left, |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation())}
                        onClick={cx.listener(move |column, _, _, cx| {
                            flip(column, &toggled);
                            cx.notify();
                        })}
                    >
                        {notifs.len().to_string()}
                        {glyph(if expanded { "expand_less" } else { "expand_more" }, px(18.))}
                    </div>
                </div>
                // Under the name rather than under the face, so that a group
                // reads as one thing with a face and the rows as what it said.
                <div class="flex flex-col gap-[1px] pt-[3px] pl-[38px]">
                    {for notif in &notifs[..listed] {
                        {self.row(notif, expanded, cx)}
                    }}
                </div>
            </div>
        }
        .into_any_element()
    }

    pub(super) fn centre(&self, cx: &mut Context<Self>) -> AnyElement {
        let feed = self.feeds.notifs.read(cx).value.clone();
        let count = feed.list.len();
        let title = match count {
            0 => "Notifications".to_string(),
            1 => "1 notification".to_string(),
            many => format!("{many} notifications"),
        };

        // One group for each application, in the order each last spoke. The
        // list is newest first, so the first sight of an application is its
        // newest.
        let mut groups: Vec<(&str, Vec<&Notification>)> = Vec::new();
        for notif in &feed.list {
            match groups.iter_mut().find(|(app, _)| *app == notif.app_name) {
                Some((_, notifs)) => notifs.push(notif),
                None => groups.push((notif.app_name.as_str(), vec![notif])),
            }
        }

        let (dnd, quietening, clearing) = (feed.dnd, self.feeds.server.clone(), self.feeds.server.clone());
        let aside = self.centre.aside.map_or(0., |aside| aside.value()) * ASIDE;
        rsx! {
            <div
                id="centre"
                class="absolute flex flex-col gap-[12px] pt-[16px] px-[16px] pb-[10px]"
                top={px(6.)}
                bottom={px(12.)}
                right={px(12. - aside)}
                w={CENTRE}
                opacity={self.centre.shown.map_or(1., |shown| shown.value())}
                rounded={px(14.)}
                bg={theme::panel()}
                shadow={theme::panel_shadows()}
                text_color={theme::text()}
                onHover={cx.listener(|column, hovered: &bool, _, cx| column.centre_hovered(*hovered, cx))}
            >
                {self.reachable()}
                <div class="flex flex-none items-center gap-[6px] min-h-[28px]">
                    <div class="flex-1 min-w-[0px] font-medium" text_size={px(14.)}>{title}</div>
                    <div
                        base={round(if dnd { "do_not_disturb_on" } else { "do_not_disturb_off" }, dnd)}
                        id="dnd"
                        onClick={move |_, _, _| quietening.set_dnd(!dnd)}
                    />
                    {...(count > 0).then(|| rsx! {
                        <div base={round("clear_all", false)} id="clear" onClick={move |_, _, _| clearing.clear()} />
                    })}
                </div>
                {...dnd.then(|| rsx! {
                    <div class="flex-none" text_size={px(12.)} line_height={px(17.4)} text_color={theme::text_faint()}>
                        {"Do not disturb is on. New notifications land here without popping up."}
                    </div>
                })}
                {if count == 0 {
                    rsx! {
                        <div class="flex flex-1 items-center justify-center" text_size={px(13.)} text_color={theme::text_faint()}>
                            {"All caught up"}
                        </div>
                    }
                    .into_any_element()
                } else {
                    rsx! {
                        // Room at the sides for a row to be pulled sideways
                        // without the scroller clipping it against the text.
                        <div id="groups" class="flex flex-col flex-1 gap-[16px] min-h-[0px] mx-[-8px] px-[8px] pt-[2px] pb-[8px] overflow-y-scroll">
                            {for (index, (app, notifs)) in groups.iter().enumerate() {
                                {self.group(index, app, notifs, cx)}
                            }}
                        </div>
                    }
                    .into_any_element()
                }}
            </div>
        }
        .into_any_element()
    }
}
