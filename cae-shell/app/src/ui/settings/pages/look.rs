//! How the desktop looks: the wallpaper, the colours taken from it, and what
//! is behind the windows when there is no wallpaper.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cae_core::launcher::config::Config;
use cae_core::launcher::{schemes, variants, wallpapers};
use cae_core::{scheme, thumbs};
use gpui::{
    AnyElement, AppContext, Context, Entity, FontWeight, IntoElement, ObjectFit, PathPromptOptions, Pixels, Render,
    RetainAllImageCache, SharedString, Window, div, img, prelude::*, px, rgb, rgba,
};

use super::super::Page;
use super::super::frame::Reach;
use super::super::list::ListPage;
use super::super::rows::{button, chosen_mark, nothing, page, pick, pressable, row, rule, section_title};
use super::super::schema;
use crate::theme;
use crate::ui::controls::switch;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

/// A scheme is a dozen files rewritten by another program.
const SETTLED: Duration = Duration::from_millis(1600);
/// How many small copies are made at once: decoding is a core each, and the
/// machine is somebody's desktop while it happens.
const AT_ONCE: usize = 4;

const GAP: Pixels = px(12.);
const ACROSS: usize = 3;

fn tile_width() -> Pixels {
    (super::super::rows::COLUMN - GAP * (ACROSS - 1) as f32) / ACROSS as f32
}

fn file_name(path: &str) -> String {
    Path::new(path).file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

const ROUNDED: Pixels = px(10.);

/// A picture with its corners rounded, or the glass it would sit on while
/// there is no small copy of it yet. The picture rounds its own corners: a
/// box clips what is in it to its edges, and not to the curve of them.
fn picture(source: Option<&PathBuf>, width: Pixels, height: Pixels) -> gpui::Div {
    rsx! {
        <div class="flex flex-none items-center justify-center" w={width} h={height} rounded={ROUNDED} bg={theme::white(0.04)} shadow={theme::edge(0.05)}>
            {...source.map(|source| rsx! {
                <img src={source.clone()} class="flex-none size-full" rounded={ROUNDED} object_fit={ObjectFit::Cover} />
            })}
        </div>
    }
}

pub struct Look {
    reach: Reach,
    rest: Entity<ListPage>,
    wallpaper: Option<String>,
    preview: Option<PathBuf>,
    set: scheme::Current,
    images: Entity<RetainAllImageCache>,
}

impl Look {
    pub fn new(reach: &Reach, window: &mut Window, cx: &mut Context<Self>) -> Look {
        cx.observe(&reach.store, |_, _, cx| cx.notify()).detach();
        let rest = cx.new(|cx| ListPage::new(schema::LOOK, reach, window, cx));
        let mut look = Look {
            reach: reach.clone(),
            rest,
            wallpaper: None,
            preview: None,
            set: scheme::current(),
            images: RetainAllImageCache::new(cx),
        };
        look.look(Duration::ZERO, cx);
        look
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |look, cx| {
            cx.background_executor().timer(after).await;
            let found = cx
                .background_spawn(async {
                    let wallpaper = wallpapers::current();
                    let preview = wallpaper.as_ref().and_then(|path| thumbs::thumbnail(Path::new(path)));
                    (wallpaper, preview, scheme::current())
                })
                .await;
            let _ = look.update(cx, |look: &mut Look, cx| {
                (look.wallpaper, look.preview, look.set) = found;
                cx.notify();
            });
        })
        .detach();
    }

    fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.set.mode = if dark { "dark" } else { "light" }.to_string();
        cx.background_spawn(async move { scheme::set_dark(dark) }).detach();
        self.look(SETTLED, cx);
        cx.notify();
    }
}

impl Render for Look {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.reach.store.read(cx).flag(super::super::store::shell("background.wallpaperEnabled"), true);
        let name = match (&self.wallpaper, shown) {
            (_, false) => "The wallpaper is switched off".to_string(),
            (Some(path), true) => file_name(path),
            (None, true) => "No wallpaper has been chosen".to_string(),
        };
        let scheme = if self.set.flavour == "default" { self.set.name.clone() } else { format!("{} {}", self.set.name, self.set.flavour) };
        let colours = format!("{scheme}, {}", self.set.variant);
        let preview = self.preview.as_ref().filter(|_| shown);
        let (dark, nav) = (self.set.is_dark(), self.reach.nav.clone());
        let to_colours = nav.clone();

        rsx! {
            <div base={page()} class="pb-[0px]" image_cache={self.images.clone()}>
                <div class="flex flex-none items-center gap-[22px] pt-[4px] pb-[18px]">
                    <div base={picture(preview, px(288.), px(180.))} text_color={theme::text_faint()}>
                        {...preview.is_none().then(|| glyph("hide_image", px(24.)))}
                    </div>
                    <div class="flex flex-col flex-1 gap-[5px] min-w-[0px]">
                        <div class="truncate" text_size={px(15.)} font_weight={FontWeight::MEDIUM}>{name}</div>
                        <div class="truncate pb-[10px]" text_size={px(12.)} text_color={theme::text_faint()}>{colours}</div>
                        <div class="flex items-center gap-[8px]">
                            <div base={button("wallpaper", "Wallpapers", false)} id="wallpapers" onClick={move |_, window, cx| nav.go(Page::Wallpapers, window, cx)} />
                            <div base={button("palette", "Colours", false)} id="colours" onClick={move |_, window, cx| to_colours.go(Page::Colours, window, cx)} />
                        </div>
                    </div>
                </div>
                <div base={pressable(row("Dark theme", "", true))} id="dark" onClick={cx.listener(move |look, _, _, cx| look.set_dark(!dark, cx))}>
                    {switch(dark)}
                </div>
                {rule()}
                {self.rest.clone()}
            </div>
        }
    }
}

/// What a folder of wallpapers shows: the pictures loose in it, and a tile
/// for each folder under it.
enum Tile {
    Picture(wallpapers::Wallpaper),
    Folder { name: String, holds: usize, cover: String },
}

pub struct Wallpapers {
    reach: Reach,
    /// The folder under the wallpaper directory this page is, which is
    /// nothing for the directory itself.
    folder: String,
    tiles: Vec<Tile>,
    current: Option<String>,
    /// The small copy of each picture, as each is made.
    small: HashMap<String, PathBuf>,
    looked: bool,
    images: Entity<RetainAllImageCache>,
}

impl Wallpapers {
    pub fn new(folder: String, reach: &Reach, cx: &mut Context<Self>) -> Wallpapers {
        let within = folder.clone();
        cx.spawn(async move |page, cx| {
            let (tiles, current) = cx.background_spawn(async move { (tiles_of(&within), wallpapers::current()) }).await;
            let wanted: Vec<String> = tiles
                .iter()
                .map(|tile| match tile {
                    Tile::Picture(wallpaper) => wallpaper.path.clone(),
                    Tile::Folder { cover, .. } => cover.clone(),
                })
                .collect();
            let listed = page.update(cx, |page: &mut Wallpapers, cx| {
                (page.tiles, page.current, page.looked) = (tiles, current, true);
                cx.notify();
            });
            if listed.is_err() {
                return;
            }

            // A few at a time, and shown as they come: the first row of a
            // large folder is there in a moment, the last in its own time.
            for batch in wanted.chunks(AT_ONCE) {
                let made = batch.iter().cloned().map(|path| {
                    cx.background_spawn(async move {
                        let small = thumbs::thumbnail(Path::new(&path));
                        (path, small)
                    })
                });
                let made = futures::future::join_all(made).await;
                let kept = page.update(cx, |page: &mut Wallpapers, cx| {
                    page.small.extend(made.into_iter().filter_map(|(path, small)| Some((path, small?))));
                    cx.notify();
                });
                if kept.is_err() {
                    return;
                }
            }
        })
        .detach();

        Wallpapers {
            reach: reach.clone(),
            folder,
            tiles: Vec::new(),
            current: None,
            small: HashMap::new(),
            looked: false,
            images: RetainAllImageCache::new(cx),
        }
    }

    /// A picture is put up; a folder is opened.
    fn press(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.tiles.get(index) {
            Some(Tile::Picture(wallpaper)) => self.choose(wallpaper.path.clone(), cx),
            Some(Tile::Folder { name, .. }) => {
                let within = if self.folder.is_empty() { name.clone() } else { format!("{}/{name}", self.folder) };
                self.reach.nav.go(Page::WallpaperFolder(within), window, cx);
            }
            None => {}
        }
    }

    fn choose(&mut self, path: String, cx: &mut Context<Self>) {
        self.current = Some(path.clone());
        cx.background_spawn(async move { wallpapers::set(&path) }).detach();
        cx.notify();
    }

    fn random(&mut self, cx: &mut Context<Self>) {
        let pictures: Vec<&String> = self.tiles.iter().filter_map(|tile| match tile {
            Tile::Picture(wallpaper) => Some(&wallpaper.path),
            Tile::Folder { .. } => None,
        }).collect();
        if pictures.is_empty() {
            return;
        }
        // Any of them but the one that is up, picked by the clock: nothing
        // here needs randomness of a better kind than that.
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.subsec_nanos());
        let others: Vec<&&String> = pictures.iter().filter(|path| Some(**path) != self.current.as_ref()).collect();
        let pool = if others.is_empty() { pictures.iter().collect() } else { others };
        self.choose((**pool[nanos as usize % pool.len()]).clone(), cx);
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let asked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Use as wallpaper".into()) });
        cx.spawn(async move |page, cx| {
            let Ok(Ok(Some(paths))) = asked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = page.update(cx, |page: &mut Wallpapers, cx| page.choose(path.to_string_lossy().into_owned(), cx));
        })
        .detach();
    }
}

/// What is directly in `folder` under the wallpaper directory, folders
/// first.
fn tiles_of(folder: &str) -> Vec<Tile> {
    let root = Config::load().wallpaper_dir;
    let mut folders: Vec<(String, usize, String)> = Vec::new();
    let mut pictures = Vec::new();

    for wallpaper in wallpapers::load(&root) {
        if wallpaper.category == folder {
            pictures.push(Tile::Picture(wallpaper));
            continue;
        }
        let under = if folder.is_empty() { Some(wallpaper.category.as_str()) } else { wallpaper.category.strip_prefix(&format!("{folder}/")) };
        let Some(name) = under.and_then(|under| under.split('/').next()).filter(|name| !name.is_empty()) else { continue };
        match folders.iter_mut().find(|(known, ..)| known == name) {
            Some((_, holds, _)) => *holds += 1,
            None => folders.push((name.to_string(), 1, wallpaper.path.clone())),
        }
    }
    folders.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    let folders = folders.into_iter().map(|(name, holds, cover)| Tile::Folder { name, holds, cover });
    folders.chain(pictures).collect()
}

impl Render for Wallpapers {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = tile_width();
        let height = width * 10. / 16.;
        let tiles: Vec<AnyElement> = self
            .tiles
            .iter()
            .enumerate()
            .map(|(index, tile)| {
                let (name, note, source, current) = match tile {
                    Tile::Picture(wallpaper) => {
                        (wallpaper.name.clone(), String::new(), self.small.get(&wallpaper.path), Some(&wallpaper.path) == self.current.as_ref())
                    }
                    Tile::Folder { name, holds, cover } => (name.clone(), format!("{holds} inside"), self.small.get(cover), false),
                };
                rsx! {
                    <div id={("tile", index)} class="relative flex flex-col flex-none gap-[7px] cursor-pointer" w={width} onClick={cx.listener(move |page, _, window, cx| page.press(index, window, cx))}>
                        {picture(source, width, height).when(current, |picture| picture.shadow(theme::tile_lifted()))}
                        <div class="flex items-center gap-[6px]" text_size={px(12.)}>
                            {...matches!(tile, Tile::Folder { .. }).then(|| glyph("folder", px(15.)).text_color(theme::text_faint()))}
                            <div class="flex-1 min-w-[0px] truncate" text_color={if current { theme::text() } else { theme::text_dim() }}>{name}</div>
                            <div class="flex-none" text_color={theme::text_faint()}>{note}</div>
                        </div>
                        {...current.then(|| rsx! {
                            <div
                                class="absolute flex items-center justify-center size-[22px] rounded-full"
                                top={px(7.)}
                                right={px(7.)}
                                bg={rgba(0x0000008c)}
                                text_color={theme::white(1.)}
                            >
                                {glyph("check", px(15.))}
                            </div>
                        })}
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div base={page()} image_cache={self.images.clone()}>
                <div class="flex flex-none items-center gap-[8px] pt-[2px] pb-[18px]">
                    <div base={button("shuffle", "Random", false)} id="random" onClick={cx.listener(|page, _, _, cx| page.random(cx))} />
                    <div base={button("folder_open", "Choose a file", false)} id="browse" onClick={cx.listener(|page, _, _, cx| page.browse(cx))} />
                </div>
                <div class="flex flex-wrap" gap={GAP}>{...tiles}</div>
                {...(self.looked && self.tiles.is_empty()).then(|| nothing("hide_image", "No pictures in the wallpaper folder"))}
            </div>
        }
    }
}

/// The schemes there are and the variants of them, with the ones that are
/// set marked.
pub struct Colours {
    schemes: Vec<schemes::Scheme>,
    set: scheme::Current,
    looked: bool,
}

impl Colours {
    pub fn new(cx: &mut Context<Self>) -> Colours {
        cx.spawn(async move |page, cx| {
            // Slow: the CLI works the dynamic scheme's palette out to list it.
            let found = cx.background_spawn(async { schemes::load() }).await;
            let _ = page.update(cx, |page: &mut Colours, cx| {
                (page.schemes, page.looked) = (found, true);
                cx.notify();
            });
        })
        .detach();
        Colours { schemes: Vec::new(), set: scheme::current(), looked: false }
    }

    fn settle(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        cx.spawn(async move |page, cx| {
            cx.background_executor().timer(SETTLED).await;
            let set = cx.background_spawn(async { scheme::current() }).await;
            let _ = page.update(cx, |page: &mut Colours, cx| {
                page.set = set;
                cx.notify();
            });
        })
        .detach();
    }

    fn choose(&mut self, name: String, flavour: String, cx: &mut Context<Self>) {
        (self.set.name, self.set.flavour) = (name.clone(), flavour.clone());
        cx.background_spawn(async move { schemes::apply(&name, &flavour) }).detach();
        self.settle(cx);
    }

    fn vary(&mut self, variant: &'static str, cx: &mut Context<Self>) {
        self.set.variant = variant.to_string();
        cx.background_spawn(async move { schemes::apply_variant(variant) }).detach();
        self.settle(cx);
    }
}

fn swatches(colours: &[String]) -> gpui::Div {
    rsx! {
        <div class="flex flex-none items-center gap-[3px]">
            {for colour in colours {
                <div
                    class="flex-none size-[14px] rounded-full"
                    bg={rgb(u32::from_str_radix(colour.trim_start_matches('#'), 16).unwrap_or(0))}
                    shadow={theme::edge(0.12)}
                />
            }}
        </div>
    }
}

impl Render for Colours {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let schemes: Vec<AnyElement> = self
            .schemes
            .iter()
            .enumerate()
            .map(|(index, scheme)| {
                let chosen = scheme.name == self.set.name && scheme.flavour == self.set.flavour;
                let (name, flavour) = (scheme.name.clone(), scheme.flavour.clone());
                let label = if scheme.flavour == "default" { scheme.name.clone() } else { format!("{} {}", scheme.name, scheme.flavour) };
                rsx! {
                    <div class="flex flex-col flex-none">
                        {...(index > 0).then(rule)}
                        <div
                            base={pick("palette", label, "", chosen)}
                            id={("scheme", index)}
                            onClick={cx.listener(move |page, _, _, cx| page.choose(name.clone(), flavour.clone(), cx))}
                        >
                            {swatches(&scheme.swatches)}
                            {...chosen.then(chosen_mark)}
                        </div>
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div base={page()}>
                {section_title("Variant", true)}
                {for (index, variant) in variants::ALL.iter().enumerate() {
                    <div class="flex flex-col flex-none" key={index}>
                        {...(index > 0).then(rule)}
                        <div
                            base={pick(variant.icon, variant.name, variant.description, variant.id == self.set.variant)}
                            id={("variant", index)}
                            onClick={cx.listener(move |page, _, _, cx| page.vary(variant.id, cx))}
                        >
                            {...(variant.id == self.set.variant).then(chosen_mark)}
                        </div>
                    </div>
                }}

                {section_title("Scheme", false)}
                {...schemes}
                {...(!self.looked).then(|| rsx! {
                    <div class="py-[14px]" text_size={px(12.5)} text_color={theme::text_faint()}>{SharedString::from("Asking for the schemes")}</div>
                })}
            </div>
        }
    }
}
