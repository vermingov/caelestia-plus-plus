//! Pictures read from files, kept decoded for as long as a view shows them.
//!
//! GPUI's `RetainAllImageCache` keeps everything it has drawn until the cache
//! itself goes, which for a view that lives as long as the shell is never:
//! every cover of every song played, decoded, megabytes each. This one lets a
//! picture go as soon as the view no longer has it to show.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{App, Entity, Resource, RetainAllImageCache, Window};

pub struct Pictures {
    cache: Entity<RetainAllImageCache>,
    held: HashSet<PathBuf>,
}

impl Pictures {
    pub fn new(cx: &mut App) -> Pictures {
        Pictures { cache: RetainAllImageCache::new(cx), held: HashSet::new() }
    }

    /// What an `image_cache` attribute is given.
    pub fn cache(&self) -> Entity<RetainAllImageCache> {
        self.cache.clone()
    }

    /// Lets go of every picture that is not among `shown`. Called while
    /// drawing, before the pictures that stay are asked for.
    pub fn keep_only(&mut self, shown: impl IntoIterator<Item = PathBuf>, window: &mut Window, cx: &mut App) {
        let shown: HashSet<PathBuf> = shown.into_iter().collect();
        let gone: Vec<PathBuf> = self.held.difference(&shown).cloned().collect();
        if !gone.is_empty() {
            self.cache.update(cx, |cache, cx| {
                for path in gone {
                    cache.remove(&Resource::Path(path.into()), window, cx);
                }
            });
        }
        self.held = shown;
    }
}
