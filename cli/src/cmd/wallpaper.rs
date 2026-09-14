//! `caelestia wallpaper` — set one, pick one at random, or say what is set.
//!
//! Setting a wallpaper records the path, points the thumbnail link at its
//! cached thumbnail, decides the scheme's mode and variant from what the
//! image looks like, and pushes the resulting colours out to everything
//! themed.
//!
//! Making a thumbnail is the one part that still belongs to the Python CLI:
//! it needs to decode whatever format the wallpaper is in. When a thumbnail
//! is already cached — which it is for every wallpaper the machine has seen
//! before — nothing here needs the image at all, and the whole command runs
//! from the cache.

use std::path::{Path, PathBuf};

use crate::material::hct::Hct;
use crate::material::jpeg;
use crate::{image, paths, proc, scheme, sha256, theme};

pub struct Args {
    /// `-p [PATH]`: print a wallpaper's scheme rather than setting it. The
    /// inner option is argparse's `nargs="?"` — present, with or without a
    /// value.
    pub print: Option<Option<String>>,
    pub random: Option<Option<String>>,
    pub file: Option<String>,
    pub no_filter: bool,
    pub threshold: f64,
    pub no_smart: bool,
}

fn wallpaper_path_file() -> PathBuf {
    paths::caelestia_state_dir().join("wallpaper/path.txt")
}

fn wallpaper_link() -> PathBuf {
    paths::caelestia_state_dir().join("wallpaper/current")
}

fn wallpapers_cache_dir() -> PathBuf {
    paths::caelestia_cache_dir().join("wallpapers")
}

fn wallpapers_dir() -> PathBuf {
    match std::env::var_os("CAELESTIA_WALLPAPERS_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => paths::pictures_dir().join("Wallpapers"),
    }
}

pub fn current() -> Option<String> {
    std::fs::read_to_string(wallpaper_path_file()).ok().filter(|p| !p.is_empty())
}

pub fn run(args: &Args, argv: &[String]) -> i32 {
    // The order the Python CLI checks these in; an empty value is no value.
    if let Some(print) = &args.print {
        let target = print.clone().or_else(current).filter(|p| !p.is_empty());
        if let Some(target) = target {
            return print_scheme(Path::new(&target), args, argv);
        }
    }
    if let Some(file) = args.file.as_deref().filter(|f| !f.is_empty()) {
        return set(Path::new(file), args, argv);
    }
    if let Some(random) = &args.random {
        let dir = random.clone().unwrap_or_else(|| wallpapers_dir().to_string_lossy().into_owned());
        if !dir.is_empty() {
            return set_random(Path::new(&dir), args, argv);
        }
    }
    println!("{}", current().unwrap_or_else(|| "No wallpaper set".to_string()));
    0
}

/// Where a wallpaper's thumbnail and smart options live: one directory per
/// image, named by the hash of its contents.
fn cache_for(wall: &Path) -> Option<PathBuf> {
    Some(wallpapers_cache_dir().join(sha256::file_hex(wall)?))
}

/// A GIF is thumbnailed from its first frame, which the Python CLI extracts
/// to a PNG beside the thumbnail.
fn thumbnail_source(wall: &Path) -> PathBuf {
    let is_gif = wall.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("gif"));
    if is_gif != Some(true) {
        return wall.to_path_buf();
    }
    match cache_for(wall) {
        Some(cache) => cache.join("first_frame.png"),
        None => wall.to_path_buf(),
    }
}

fn set(wall: &Path, args: &Args, argv: &[String]) -> i32 {
    let Ok(wall) = std::fs::canonicalize(wall) else {
        eprintln!("caelestia: \"{}\" is not a valid image", wall.display());
        return 1;
    };
    if !image::is_wallpaper(&wall) {
        eprintln!("caelestia: \"{}\" is not a valid image", wall.display());
        return 1;
    }

    let source = thumbnail_source(&wall);
    let (Some(cache), true) = (cache_for(&source), source.exists()) else {
        return crate::hand_over(argv); // the first frame has not been extracted yet
    };
    let thumbnail = cache.join("thumbnail.jpg");
    if !thumbnail.is_file() {
        return crate::hand_over(argv); // making one needs a decoder for this format
    }

    let state = paths::caelestia_state_dir().join("wallpaper");
    let _ = std::fs::create_dir_all(&state);
    if let Err(e) = std::fs::write(wallpaper_path_file(), wall.to_string_lossy().as_bytes()) {
        eprintln!("caelestia: cannot record the wallpaper: {e}");
        return 1;
    }
    relink(&wallpaper_link(), &wall);
    relink(&paths::wallpaper_thumbnail_path(), &thumbnail);

    let mut current = match scheme::Scheme::load() {
        Ok(scheme) => scheme,
        Err(e) => {
            eprintln!("caelestia: {e}");
            return 1;
        }
    };

    // A generated scheme follows the wallpaper: how colourful it is picks the
    // variant, how bright it is picks light or dark.
    if current.name == scheme::DYNAMIC && !args.no_smart {
        if let Some(smart) = smart_options(&thumbnail, &cache) {
            current.mode = smart.mode;
            current.variant = smart.variant;
        }
    }
    if let Err(e) = current.update_colours() {
        eprintln!("caelestia: {e}");
        return 1;
    }

    theme::apply_colours(&current.colours, &current.mode);
    run_post_hook(&wall, &thumbnail, &current);
    0
}

fn print_scheme(wall: &Path, args: &Args, argv: &[String]) -> i32 {
    let Ok(wall) = std::fs::canonicalize(wall) else {
        eprintln!("caelestia: \"{}\" is not a valid image", wall.display());
        return 1;
    };
    let source = thumbnail_source(&wall);
    let (Some(cache), true) = (cache_for(&source), source.exists()) else {
        return crate::hand_over(argv);
    };
    let thumbnail = cache.join("thumbnail.jpg");
    if !thumbnail.is_file() {
        return crate::hand_over(argv);
    }

    let Ok(current) = scheme::Scheme::load() else { return crate::hand_over(argv) };
    let (mut mode, mut variant) = (current.mode.clone(), current.variant.clone());
    if !args.no_smart {
        if let Some(smart) = smart_options(&thumbnail, &cache) {
            mode = smart.mode;
            variant = smart.variant;
        }
    }

    let colours = match scheme::dynamic_colours_for(&thumbnail, &variant, &current.flavour, &mode) {
        Ok(colours) => colours,
        Err(e) => {
            eprintln!("caelestia: {e}");
            return 1;
        }
    };
    let body: Vec<String> =
        colours.iter().map(|(name, colour)| format!("\"{name}\": \"{colour}\"")).collect();
    println!(
        "{{\"name\": \"{}\", \"flavour\": \"{}\", \"mode\": \"{mode}\", \"variant\": \"{variant}\", \"colours\": {{{}}}}}",
        scheme::DYNAMIC,
        current.flavour,
        body.join(", ")
    );
    0
}

fn set_random(dir: &Path, args: &Args, argv: &[String]) -> i32 {
    let mut candidates = Vec::new();
    collect_images(dir, &mut candidates);

    if !args.no_filter {
        if let Some((width, height)) = smallest_monitor() {
            let wanted = (width as f64 * args.threshold, height as f64 * args.threshold);
            candidates.retain(|path| match image::dimensions(path) {
                Some((w, h)) => w as f64 >= wanted.0 && h as f64 >= wanted.1,
                None => false,
            });
        }
    }
    if candidates.is_empty() {
        eprintln!("caelestia: no valid wallpapers found");
        return 1;
    }

    // Never pick the one already set, unless it is the only one.
    if let Some(current) = current() {
        let current = PathBuf::from(current);
        if candidates.len() > 1 {
            candidates.retain(|path| *path != current);
        }
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    let chosen = candidates[nanos % candidates.len()].clone();
    set(&chosen, args, argv)
}

fn collect_images(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_images(&path, out);
        } else if image::is_wallpaper(&path) {
            out.push(path);
        }
    }
}

fn smallest_monitor() -> Option<(i64, i64)> {
    let monitors = crate::hypr::monitors();
    if monitors.is_empty() {
        return None;
    }
    let width = monitors.iter().filter_map(|m| m.get("width")?.as_u64()).min()?;
    let height = monitors.iter().filter_map(|m| m.get("height")?.as_u64()).min()?;
    Some((width as i64, height as i64))
}

fn relink(link: &Path, target: &Path) {
    if let Some(parent) = link.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(link);
    if let Err(e) = std::os::unix::fs::symlink(target, link) {
        eprintln!("caelestia: cannot link {}: {e}", link.display());
    }
}

struct Smart {
    mode: String,
    variant: String,
}

/// What the image looks like, cached beside its thumbnail: the variant from
/// how colourful it is, the mode from how bright it is.
fn smart_options(thumbnail: &Path, cache: &Path) -> Option<Smart> {
    let cached = cache.join("smart.json");
    if let Some(parsed) = std::fs::read_to_string(&cached).ok().and_then(|t| redcommon::json::parse(&t)) {
        if let (Some(mode), Some(variant)) = (parsed.str_field("mode"), parsed.str_field("variant")) {
            return Some(Smart { mode: mode.to_string(), variant: variant.to_string() });
        }
    }

    let bytes = std::fs::read(thumbnail).ok()?;
    let image = jpeg::decode(&bytes).ok()?;
    let variant = variant_for(&image);
    let mode = if mean_tone(&image) > 60.0 { "light" } else { "dark" };

    let body = format!("{{\"variant\": \"{variant}\", \"mode\": \"{mode}\"}}");
    if let Err(e) = paths::atomic_write(&cached, &body) {
        eprintln!("caelestia: cannot cache {}: {e}", cached.display());
    }
    Some(Smart { mode: mode.to_string(), variant: variant.to_string() })
}

/// Hasler and Süsstrunk's colourfulness: how far the pixels spread along the
/// red-green and yellow-blue axes, plus a little of how far they sit from
/// grey on average.
fn colourfulness(image: &jpeg::Image) -> f64 {
    let pixels = image.rgb.len() / 3;
    if pixels == 0 {
        return 0.0;
    }
    let (mut sum_rg, mut sum_yb) = (0.0f64, 0.0f64);
    for px in image.rgb.chunks_exact(3) {
        let (r, g, b) = (px[0] as f64, px[1] as f64, px[2] as f64);
        sum_rg += (r - g).abs();
        sum_yb += (0.5 * (r + g) - b).abs();
    }
    let (mean_rg, mean_yb) = (sum_rg / pixels as f64, sum_yb / pixels as f64);

    let (mut var_rg, mut var_yb) = (0.0f64, 0.0f64);
    for px in image.rgb.chunks_exact(3) {
        let (r, g, b) = (px[0] as f64, px[1] as f64, px[2] as f64);
        var_rg += ((r - g).abs() - mean_rg).powi(2);
        var_yb += ((0.5 * (r + g) - b).abs() - mean_yb).powi(2);
    }
    let std_rg = (var_rg / pixels as f64).sqrt();
    let std_yb = (var_yb / pixels as f64).sqrt();

    (std_rg * std_rg + std_yb * std_yb).sqrt() + 0.3 * (mean_rg * mean_rg + mean_yb * mean_yb).sqrt()
}

fn variant_for(image: &jpeg::Image) -> &'static str {
    match colourfulness(image) {
        c if c < 10.0 => "neutral",
        c if c < 20.0 => "content",
        _ => "tonalspot",
    }
}

/// The tone of the image's average colour. The Python CLI gets this by
/// scaling the thumbnail to a single pixel, which is the same average by
/// another name.
fn mean_tone(image: &jpeg::Image) -> f64 {
    let pixels = image.rgb.len() / 3;
    if pixels == 0 {
        return 0.0;
    }
    let mut sums = [0u64; 3];
    for px in image.rgb.chunks_exact(3) {
        for channel in 0..3 {
            sums[channel] += px[channel] as u64;
        }
    }
    let average = |channel: usize| {
        // Rounded, because the pixel the Python averages into is an integer.
        ((sums[channel] as f64 / pixels as f64).round() as u32).min(255)
    };
    let argb = 0xff00_0000 | (average(0) << 16) | (average(1) << 8) | average(2);
    Hct::from_int(argb).tone()
}

fn run_post_hook(wall: &Path, thumbnail: &Path, current: &scheme::Scheme) {
    let config = crate::config::user_config();
    let Some(hook) = config
        .get("wallpaper")
        .and_then(|w| w.str_field("postHook"))
        .filter(|h| !h.is_empty())
    else {
        return;
    };

    let body: Vec<String> =
        current.colours.iter().map(|(name, colour)| format!("\"{name}\": \"{colour}\"")).collect();
    let _ = std::process::Command::new("sh")
        .args(["-c", hook])
        .env("WALLPAPER_PATH", wall)
        .env("SCHEME_NAME", &current.name)
        .env("SCHEME_FLAVOUR", &current.flavour)
        .env("SCHEME_MODE", &current.mode)
        .env("SCHEME_VARIANT", &current.variant)
        .env("SCHEME_COLOURS", format!("{{{}}}", body.join(", ")))
        .env("THUMBNAIL_PATH", thumbnail)
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_of(pixels: &[[u8; 3]]) -> jpeg::Image {
        jpeg::Image {
            width: pixels.len(),
            height: 1,
            rgb: pixels.iter().flatten().copied().collect(),
        }
    }

    #[test]
    fn a_grey_image_is_never_colourful() {
        let grey = image_of(&[[10, 10, 10], [128, 128, 128], [240, 240, 240]]);
        assert!(colourfulness(&grey) < 1.0);
        assert_eq!(variant_for(&grey), "neutral");
    }

    #[test]
    fn a_saturated_image_is() {
        let loud = image_of(&[[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]]);
        assert!(colourfulness(&loud) > 20.0, "{}", colourfulness(&loud));
        assert_eq!(variant_for(&loud), "tonalspot");
    }

    #[test]
    fn brightness_decides_the_mode() {
        assert!(mean_tone(&image_of(&[[250, 250, 250]])) > 60.0);
        assert!(mean_tone(&image_of(&[[20, 20, 20]])) < 60.0);
    }

    /// The values the Python produces for the images in the corpus, over the
    /// same pixels — and, separately, that the decision comes out the same as
    /// the Python's own even though it decodes the JPEG with libjpeg and this
    /// decodes it with stb, which disagree by a level here and there.
    #[test]
    fn colourfulness_matches_the_python() {
        use redcommon::json::Json;

        let vectors = redcommon::json::parse(include_str!("../../tests/smart-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(images)) = vectors.get("images") else { panic!("no images") };

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpeg");
        for image in images {
            let name = image.str_field("name").expect("image name");
            let bytes = std::fs::read(format!("{dir}/{name}")).expect("test image is readable");
            let decoded = jpeg::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));

            let Some(Json::Num(expected)) = image.get("colourfulness") else { panic!("a number") };
            let ours = colourfulness(&decoded);
            assert!((ours - expected).abs() < 1e-9, "{name}: {ours} not {expected}");
            assert_eq!(variant_for(&decoded), image.str_field("variant").unwrap(), "{name}: variant");
            let mode = if mean_tone(&decoded) > 60.0 { "light" } else { "dark" };
            assert_eq!(mode, image.str_field("mode").unwrap(), "{name}: mode");
            assert_eq!(
                variant_for(&decoded),
                image.str_field("pilVariant").unwrap(),
                "{name}: the two decoders disagree about the variant"
            );
            assert_eq!(
                mode,
                image.str_field("pilMode").unwrap(),
                "{name}: the two decoders disagree about the mode"
            );
        }
        assert!(images.len() >= 40, "corpus shrank");
    }
}
