//! Celebi: Wu's box-cutting, then weighted k-means over the result.
//!
//! Reduces a wallpaper to at most 128 colours with a population each, which is
//! what the scorer then picks the scheme's seed from.
//!
//! Ported from the C++ in materialyoucolor (`quantize/wu.cc`, `wsmeans.cc`,
//! `lab.cc`), which is Google's Material Color Utilities. Two details are not
//! optional: the Lab conversion here is the quantizer's own and rounds
//! differently from the rest of the pipeline, and the k-means starts from
//! cluster assignments drawn from glibc's `rand()` seeded with 42688 — so
//! that generator is reproduced rather than replaced.

use std::collections::{BTreeMap, HashMap};

const WHITE_POINT_D65: [f64; 3] = [95.047, 100.0, 108.883];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

impl Lab {
    /// Squared distance — the k-means never needs the root except to compare
    /// against the move threshold.
    fn delta_e(&self, other: &Lab) -> f64 {
        let (dl, da, db) = (self.l - other.l, self.a - other.a, self.b - other.b);
        dl * dl + da * da + db * db
    }
}

fn linearized(component: i32) -> f64 {
    let normalized = component as f64 / 255.0;
    if normalized <= 0.040449936 {
        normalized / 12.92 * 100.0
    } else {
        ((normalized + 0.055) / 1.055).powf(2.4) * 100.0
    }
}

/// Rounds half away from zero, like C's `round`, not like the banker's
/// rounding the rest of the pipeline inherits from Python.
fn delinearized(rgb_component: f64) -> i32 {
    let normalized = rgb_component / 100.0;
    let value = if normalized <= 0.0031308 {
        normalized * 12.92
    } else {
        1.055 * normalized.powf(1.0 / 2.4) - 0.055
    };
    ((value * 255.0).round() as i32).clamp(0, 255)
}

fn argb_from_rgb(red: i32, green: i32, blue: i32) -> u32 {
    0xff00_0000 | ((red as u32 & 0xff) << 16) | ((green as u32 & 0xff) << 8) | (blue as u32 & 0xff)
}

fn lab_from_int(argb: u32) -> Lab {
    let red = linearized(((argb & 0x00ff_0000) >> 16) as i32);
    let green = linearized(((argb & 0x0000_ff00) >> 8) as i32);
    let blue = linearized((argb & 0x0000_00ff) as i32);
    let x = 0.41233895 * red + 0.35762064 * green + 0.18051042 * blue;
    let y = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    let z = 0.01932141 * red + 0.11916382 * green + 0.95034478 * blue;

    const E: f64 = 216.0 / 24389.0;
    const KAPPA: f64 = 24389.0 / 27.0;
    let f = |normalized: f64| {
        if normalized > E {
            normalized.powf(1.0 / 3.0)
        } else {
            (KAPPA * normalized + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (
        f(x / WHITE_POINT_D65[0]),
        f(y / WHITE_POINT_D65[1]),
        f(z / WHITE_POINT_D65[2]),
    );
    Lab { l: 116.0 * fy - 16.0, a: 500.0 * (fx - fy), b: 200.0 * (fy - fz) }
}

fn int_from_lab(lab: Lab) -> u32 {
    const E: f64 = 216.0 / 24389.0;
    const KAPPA: f64 = 24389.0 / 27.0;
    const KE: f64 = 8.0;

    let fy = (lab.l + 16.0) / 116.0;
    let fx = lab.a / 500.0 + fy;
    let fz = fy - lab.b / 200.0;
    let fx3 = fx * fx * fx;
    let x_normalized = if fx3 > E { fx3 } else { (116.0 * fx - 16.0) / KAPPA };
    let y_normalized = if lab.l > KE { fy * fy * fy } else { lab.l / KAPPA };
    let fz3 = fz * fz * fz;
    let z_normalized = if fz3 > E { fz3 } else { (116.0 * fz - 16.0) / KAPPA };
    let x = x_normalized * WHITE_POINT_D65[0];
    let y = y_normalized * WHITE_POINT_D65[1];
    let z = z_normalized * WHITE_POINT_D65[2];

    // The plain sRGB matrix, not the higher-precision one the rest of the
    // pipeline uses; this is what the quantizer was written against.
    let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
    let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
    let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;
    argb_from_rgb(delinearized(r), delinearized(g), delinearized(b))
}

/// glibc's `rand`: the TYPE_3 additive-feedback generator, degree 31,
/// separation 3. The k-means seeds its starting assignment with it, so the
/// clustering only reproduces if this sequence does.
struct GlibcRand {
    /// The last 31 words of the feedback register, oldest first.
    window: [u32; 31],
    next: usize,
}

impl GlibcRand {
    fn new(seed: u32) -> GlibcRand {
        let mut r = [0u32; 344];
        r[0] = if seed == 0 { 1 } else { seed };
        for i in 1..31 {
            // Schrage's trick for 16807 * x mod 2^31-1 without overflowing.
            let previous = r[i - 1] as i64;
            let word = 16807 * (previous % 127773) - 2836 * (previous / 127773);
            r[i] = (if word < 0 { word + 2147483647 } else { word }) as u32;
        }
        for i in 31..34 {
            r[i] = r[i - 31];
        }
        // glibc runs ten cycles before handing anything out, so the seed stops
        // showing in the first draws.
        for i in 34..344 {
            r[i] = r[i - 31].wrapping_add(r[i - 3]);
        }
        let mut window = [0u32; 31];
        window.copy_from_slice(&r[313..344]);
        GlibcRand { window, next: 0 }
    }

    fn next(&mut self) -> i32 {
        // window[next] holds the word 31 back; the one three back is 28 later.
        let value = self.window[self.next].wrapping_add(self.window[(self.next + 28) % 31]);
        self.window[self.next] = value;
        self.next = (self.next + 1) % 31;
        (value >> 1) as i32
    }
}

// ---- Wu ------------------------------------------------------------------

const INDEX_BITS: usize = 5;
const INDEX_COUNT: usize = (1 << INDEX_BITS) + 1;
const TOTAL_SIZE: usize = INDEX_COUNT * INDEX_COUNT * INDEX_COUNT;
const MAX_COLOURS: usize = 256;

#[derive(Clone, Copy, Default)]
struct Box3 {
    r0: usize,
    r1: usize,
    g0: usize,
    g1: usize,
    b0: usize,
    b1: usize,
    vol: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Red,
    Green,
    Blue,
}

fn index_of(r: usize, g: usize, b: usize) -> usize {
    (r << (INDEX_BITS * 2)) + (r << (INDEX_BITS + 1)) + (g << INDEX_BITS) + r + g + b
}

struct Moments {
    weights: Vec<i64>,
    r: Vec<i64>,
    g: Vec<i64>,
    b: Vec<i64>,
    squared: Vec<f64>,
}

impl Moments {
    /// A 32-cube histogram of the image, then turned into 3D prefix sums so
    /// any box's totals are eight lookups rather than a scan.
    fn build(pixels: &[u32]) -> Moments {
        let mut m = Moments {
            weights: vec![0; TOTAL_SIZE],
            r: vec![0; TOTAL_SIZE],
            g: vec![0; TOTAL_SIZE],
            b: vec![0; TOTAL_SIZE],
            squared: vec![0.0; TOTAL_SIZE],
        };

        for pixel in pixels {
            let red = ((pixel & 0x00ff_0000) >> 16) as i64;
            let green = ((pixel & 0x0000_ff00) >> 8) as i64;
            let blue = (pixel & 0x0000_00ff) as i64;
            let bits_to_remove = 8 - INDEX_BITS;
            let index = index_of(
                (red as usize >> bits_to_remove) + 1,
                (green as usize >> bits_to_remove) + 1,
                (blue as usize >> bits_to_remove) + 1,
            );
            m.weights[index] += 1;
            m.r[index] += red;
            m.g[index] += green;
            m.b[index] += blue;
            m.squared[index] += (red * red + green * green + blue * blue) as f64;
        }

        for r in 1..INDEX_COUNT {
            let mut area = [0i64; INDEX_COUNT];
            let mut area_r = [0i64; INDEX_COUNT];
            let mut area_g = [0i64; INDEX_COUNT];
            let mut area_b = [0i64; INDEX_COUNT];
            let mut area_2 = [0f64; INDEX_COUNT];
            for g in 1..INDEX_COUNT {
                let (mut line, mut line_r, mut line_g, mut line_b) = (0i64, 0i64, 0i64, 0i64);
                let mut line_2 = 0f64;
                for b in 1..INDEX_COUNT {
                    let index = index_of(r, g, b);
                    line += m.weights[index];
                    line_r += m.r[index];
                    line_g += m.g[index];
                    line_b += m.b[index];
                    line_2 += m.squared[index];

                    area[b] += line;
                    area_r[b] += line_r;
                    area_g[b] += line_g;
                    area_b[b] += line_b;
                    area_2[b] += line_2;

                    let previous = index_of(r - 1, g, b);
                    m.weights[index] = m.weights[previous] + area[b];
                    m.r[index] = m.r[previous] + area_r[b];
                    m.g[index] = m.g[previous] + area_g[b];
                    m.b[index] = m.b[previous] + area_b[b];
                    m.squared[index] = m.squared[previous] + area_2[b];
                }
            }
        }
        m
    }
}

fn top(cube: &Box3, direction: Direction, position: usize, moment: &[i64]) -> i64 {
    match direction {
        Direction::Red => {
            moment[index_of(position, cube.g1, cube.b1)] - moment[index_of(position, cube.g1, cube.b0)]
                - moment[index_of(position, cube.g0, cube.b1)]
                + moment[index_of(position, cube.g0, cube.b0)]
        }
        Direction::Green => {
            moment[index_of(cube.r1, position, cube.b1)] - moment[index_of(cube.r1, position, cube.b0)]
                - moment[index_of(cube.r0, position, cube.b1)]
                + moment[index_of(cube.r0, position, cube.b0)]
        }
        Direction::Blue => {
            moment[index_of(cube.r1, cube.g1, position)] - moment[index_of(cube.r1, cube.g0, position)]
                - moment[index_of(cube.r0, cube.g1, position)]
                + moment[index_of(cube.r0, cube.g0, position)]
        }
    }
}

fn bottom(cube: &Box3, direction: Direction, moment: &[i64]) -> i64 {
    match direction {
        Direction::Red => {
            -moment[index_of(cube.r0, cube.g1, cube.b1)] + moment[index_of(cube.r0, cube.g1, cube.b0)]
                + moment[index_of(cube.r0, cube.g0, cube.b1)]
                - moment[index_of(cube.r0, cube.g0, cube.b0)]
        }
        Direction::Green => {
            -moment[index_of(cube.r1, cube.g0, cube.b1)] + moment[index_of(cube.r1, cube.g0, cube.b0)]
                + moment[index_of(cube.r0, cube.g0, cube.b1)]
                - moment[index_of(cube.r0, cube.g0, cube.b0)]
        }
        Direction::Blue => {
            -moment[index_of(cube.r1, cube.g1, cube.b0)] + moment[index_of(cube.r1, cube.g0, cube.b0)]
                + moment[index_of(cube.r0, cube.g1, cube.b0)]
                - moment[index_of(cube.r0, cube.g0, cube.b0)]
        }
    }
}

fn volume(cube: &Box3, moment: &[i64]) -> i64 {
    moment[index_of(cube.r1, cube.g1, cube.b1)] - moment[index_of(cube.r1, cube.g1, cube.b0)]
        - moment[index_of(cube.r1, cube.g0, cube.b1)]
        + moment[index_of(cube.r1, cube.g0, cube.b0)]
        - moment[index_of(cube.r0, cube.g1, cube.b1)]
        + moment[index_of(cube.r0, cube.g1, cube.b0)]
        + moment[index_of(cube.r0, cube.g0, cube.b1)]
        - moment[index_of(cube.r0, cube.g0, cube.b0)]
}

fn variance(cube: &Box3, m: &Moments) -> f64 {
    let dr = volume(cube, &m.r) as f64;
    let dg = volume(cube, &m.g) as f64;
    let db = volume(cube, &m.b) as f64;
    let s = &m.squared;
    let xx = s[index_of(cube.r1, cube.g1, cube.b1)] - s[index_of(cube.r1, cube.g1, cube.b0)]
        - s[index_of(cube.r1, cube.g0, cube.b1)]
        + s[index_of(cube.r1, cube.g0, cube.b0)]
        - s[index_of(cube.r0, cube.g1, cube.b1)]
        + s[index_of(cube.r0, cube.g1, cube.b0)]
        + s[index_of(cube.r0, cube.g0, cube.b1)]
        - s[index_of(cube.r0, cube.g0, cube.b0)];
    let hypotenuse = dr * dr + dg * dg + db * db;
    xx - hypotenuse / volume(cube, &m.weights) as f64
}

/// The cut position along one axis that splits the box into the two halves
/// with the most separated means.
#[allow(clippy::too_many_arguments)]
fn maximize(
    cube: &Box3,
    direction: Direction,
    first: usize,
    last: usize,
    cut: &mut i64,
    whole_w: i64,
    whole_r: i64,
    whole_g: i64,
    whole_b: i64,
    m: &Moments,
) -> f64 {
    let bottom_r = bottom(cube, direction, &m.r);
    let bottom_g = bottom(cube, direction, &m.g);
    let bottom_b = bottom(cube, direction, &m.b);
    let bottom_w = bottom(cube, direction, &m.weights);

    let mut max = 0.0f64;
    *cut = -1;

    for i in first..last {
        let mut half_r = bottom_r + top(cube, direction, i, &m.r);
        let mut half_g = bottom_g + top(cube, direction, i, &m.g);
        let mut half_b = bottom_b + top(cube, direction, i, &m.b);
        let mut half_w = bottom_w + top(cube, direction, i, &m.weights);
        if half_w == 0 {
            continue;
        }
        let mut temp = (half_r as f64 * half_r as f64
            + half_g as f64 * half_g as f64
            + half_b as f64 * half_b as f64)
            / half_w as f64;

        half_r = whole_r - half_r;
        half_g = whole_g - half_g;
        half_b = whole_b - half_b;
        half_w = whole_w - half_w;
        if half_w == 0 {
            continue;
        }
        temp += (half_r as f64 * half_r as f64
            + half_g as f64 * half_g as f64
            + half_b as f64 * half_b as f64)
            / half_w as f64;

        if temp > max {
            max = temp;
            *cut = i as i64;
        }
    }
    max
}

fn cut(box1: &mut Box3, box2: &mut Box3, m: &Moments) -> bool {
    let whole_r = volume(box1, &m.r);
    let whole_g = volume(box1, &m.g);
    let whole_b = volume(box1, &m.b);
    let whole_w = volume(box1, &m.weights);

    let (mut cut_r, mut cut_g, mut cut_b) = (0i64, 0i64, 0i64);
    let max_r = maximize(box1, Direction::Red, box1.r0 + 1, box1.r1, &mut cut_r, whole_w, whole_r, whole_g, whole_b, m);
    let max_g = maximize(box1, Direction::Green, box1.g0 + 1, box1.g1, &mut cut_g, whole_w, whole_r, whole_g, whole_b, m);
    let max_b = maximize(box1, Direction::Blue, box1.b0 + 1, box1.b1, &mut cut_b, whole_w, whole_r, whole_g, whole_b, m);

    let direction = if max_r >= max_g && max_r >= max_b {
        if cut_r < 0 {
            return false;
        }
        Direction::Red
    } else if max_g >= max_r && max_g >= max_b {
        Direction::Green
    } else {
        Direction::Blue
    };

    box2.r1 = box1.r1;
    box2.g1 = box1.g1;
    box2.b1 = box1.b1;

    match direction {
        Direction::Red => {
            box1.r1 = cut_r as usize;
            box2.r0 = cut_r as usize;
            box2.g0 = box1.g0;
            box2.b0 = box1.b0;
        }
        Direction::Green => {
            box2.r0 = box1.r0;
            box1.g1 = cut_g as usize;
            box2.g0 = cut_g as usize;
            box2.b0 = box1.b0;
        }
        Direction::Blue => {
            box2.r0 = box1.r0;
            box2.g0 = box1.g0;
            box1.b1 = cut_b as usize;
            box2.b0 = cut_b as usize;
        }
    }

    box1.vol = ((box1.r1 - box1.r0) * (box1.g1 - box1.g0) * (box1.b1 - box1.b0)) as i64;
    box2.vol = ((box2.r1 - box2.r0) * (box2.g1 - box2.g0) * (box2.b1 - box2.b0)) as i64;
    true
}

/// Wu's algorithm: repeatedly split whichever box holds the most variance,
/// then take each box's average colour.
pub fn quantize_wu(pixels: &[u32], max_colours: usize) -> Vec<u32> {
    if max_colours == 0 || max_colours > 256 || pixels.is_empty() {
        return Vec::new();
    }

    let m = Moments::build(pixels);
    let mut cubes = vec![Box3::default(); MAX_COLOURS];
    cubes[0].r1 = INDEX_COUNT - 1;
    cubes[0].g1 = INDEX_COUNT - 1;
    cubes[0].b1 = INDEX_COUNT - 1;

    let mut volume_variance = vec![0.0f64; MAX_COLOURS];
    let mut max_colours = max_colours;
    let mut next = 0usize;
    let mut i = 1usize;
    while i < max_colours {
        let (left, right) = cubes.split_at_mut(i.max(next + 1));
        let (a, b) = if next < i {
            (&mut left[next], &mut right[0])
        } else {
            unreachable!("the box being split always comes before the new one")
        };
        if cut(a, b, &m) {
            volume_variance[next] = if cubes[next].vol > 1 { variance(&cubes[next], &m) } else { 0.0 };
            volume_variance[i] = if cubes[i].vol > 1 { variance(&cubes[i], &m) } else { 0.0 };
        } else {
            volume_variance[next] = 0.0;
            i -= 1;
        }

        next = 0;
        let mut temp = volume_variance[0];
        for j in 1..=i {
            if volume_variance[j] > temp {
                temp = volume_variance[j];
                next = j;
            }
        }
        if temp <= 0.0 {
            max_colours = i + 1;
            break;
        }
        i += 1;
    }

    let mut out = Vec::new();
    for cube in cubes.iter().take(max_colours) {
        let weight = volume(cube, &m.weights);
        if weight > 0 {
            let red = (volume(cube, &m.r) / weight) as i32;
            let green = (volume(cube, &m.g) / weight) as i32;
            let blue = (volume(cube, &m.b) / weight) as i32;
            out.push(argb_from_rgb(red, green, blue));
        }
    }
    out
}

// ---- Wsmeans -------------------------------------------------------------

const MAX_ITERATIONS: usize = 100;
const MIN_DELTA_E: f64 = 3.0;

/// Weighted k-means in Lab, started from Wu's palette. Colours only move
/// cluster when the move is big enough to see, which is what stops it
/// oscillating between near-identical centres.
pub fn quantize_wsmeans(
    input_pixels: &[u32],
    starting_clusters: &[u32],
    max_colours: usize,
) -> BTreeMap<u32, u32> {
    if max_colours == 0 || input_pixels.is_empty() {
        return BTreeMap::new();
    }
    let max_colours = max_colours.min(256);

    let mut pixel_to_count: HashMap<u32, i32> = HashMap::new();
    let mut pixels: Vec<u32> = Vec::new();
    let mut points: Vec<Lab> = Vec::new();
    for pixel in input_pixels {
        match pixel_to_count.get_mut(pixel) {
            Some(count) => *count += 1,
            None => {
                pixels.push(*pixel);
                points.push(lab_from_int(*pixel));
                pixel_to_count.insert(*pixel, 1);
            }
        }
    }

    let mut cluster_count = max_colours.min(points.len());
    if !starting_clusters.is_empty() {
        cluster_count = cluster_count.min(starting_clusters.len());
    }

    let mut clusters: Vec<Lab> = starting_clusters.iter().map(|c| lab_from_int(*c)).collect();

    let mut rng = GlibcRand::new(42688);
    let mut cluster_indices: Vec<usize> =
        (0..points.len()).map(|_| (rng.next() as usize) % cluster_count).collect();

    let mut pixel_count_sums = [0i32; 256];
    let mut distances = vec![vec![0.0f64; cluster_count]; cluster_count];

    for iteration in 0..MAX_ITERATIONS {
        for i in 0..cluster_count {
            distances[i][i] = 0.0;
            for j in i + 1..cluster_count {
                let distance = clusters[i].delta_e(&clusters[j]);
                distances[j][i] = distance;
                distances[i][j] = distance;
            }
        }

        let mut colour_moved = false;
        for i in 0..points.len() {
            let point = points[i];
            let previous_cluster_index = cluster_indices[i];
            let previous_distance = point.delta_e(&clusters[previous_cluster_index]);
            let mut minimum_distance = previous_distance;
            let mut new_cluster_index: i32 = -1;

            for j in 0..cluster_count {
                // Clusters further from the current one than twice the
                // current distance cannot win, so they are never measured.
                if distances[previous_cluster_index][j] >= 4.0 * previous_distance {
                    continue;
                }
                let distance = point.delta_e(&clusters[j]);
                if distance < minimum_distance {
                    minimum_distance = distance;
                    new_cluster_index = j as i32;
                }
            }
            if new_cluster_index != -1 {
                // The C++ calls a bare `abs` on a double with <cstdlib> in
                // scope, which picks the *integer* overload and truncates the
                // difference. So the threshold is really "four or more", not
                // "more than three". Reproduced, not fixed: it decides which
                // cluster a colour lands in.
                let change = (minimum_distance.sqrt() - previous_distance.sqrt()).abs();
                let change = change.trunc();
                if change > MIN_DELTA_E {
                    colour_moved = true;
                    cluster_indices[i] = new_cluster_index as usize;
                }
            }
        }

        if !colour_moved && iteration != 0 {
            break;
        }

        let mut sum_l = [0.0f64; 256];
        let mut sum_a = [0.0f64; 256];
        let mut sum_b = [0.0f64; 256];
        for sum in pixel_count_sums.iter_mut().take(cluster_count) {
            *sum = 0;
        }

        for i in 0..points.len() {
            let index = cluster_indices[i];
            let point = points[i];
            let count = pixel_to_count[&pixels[i]];
            pixel_count_sums[index] += count;
            sum_l[index] += point.l * count as f64;
            sum_a[index] += point.a * count as f64;
            sum_b[index] += point.b * count as f64;
        }

        for i in 0..cluster_count {
            let count = pixel_count_sums[i];
            clusters[i] = if count == 0 {
                Lab::default()
            } else {
                Lab {
                    l: sum_l[i] / count as f64,
                    a: sum_a[i] / count as f64,
                    b: sum_b[i] / count as f64,
                }
            };
        }
    }

    let mut colour_to_count: BTreeMap<u32, u32> = BTreeMap::new();
    let mut swatches: Vec<(u32, i32)> = Vec::new();
    for i in 0..cluster_count {
        let colour = int_from_lab(clusters[i]);
        let count = pixel_count_sums[i];
        if count == 0 {
            continue;
        }
        match swatches.iter_mut().find(|(argb, _)| *argb == colour) {
            Some(swatch) => swatch.1 += count,
            None => swatches.push((colour, count)),
        }
    }
    for (colour, count) in swatches {
        colour_to_count.insert(colour, count as u32);
    }
    colour_to_count
}

/// The whole thing: an image's pixels to at most `max_colours` with a
/// population each.
pub fn quantize_celebi(pixels: &[u32], max_colours: usize) -> BTreeMap<u32, u32> {
    let max_colours = max_colours.min(256);
    let wu = quantize_wu(pixels, max_colours);
    quantize_wsmeans(pixels, &wu, max_colours)
}

/// The pixels of a decoded image, in the order the quantizer wants them:
/// row by row, alpha dropped.
pub fn pixels_of(image: &super::jpeg::Image) -> Vec<u32> {
    let mut out = Vec::with_capacity(image.width * image.height);
    for chunk in image.rgb.chunks_exact(3) {
        out.push(((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first values glibc's rand() produces for this seed, taken from a C
    /// program on this machine.
    #[test]
    fn the_random_sequence_is_glibcs() {
        let mut rng = GlibcRand::new(42688);
        let expected = [
            1057348992, 762201445, 1956717003, 540228760, 637815880, 1149054967, 495305702,
            2085833226, 2135573059, 97451330, 1503726733, 1090010579,
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(rng.next(), *want, "draw {i}");
        }
    }

    #[test]
    fn a_single_colour_quantises_to_itself() {
        let pixels = vec![0x0042_85f4u32; 500];
        let result = quantize_celebi(&pixels, 128);
        assert_eq!(result.len(), 1);
        let (colour, count) = result.iter().next().unwrap();
        assert_eq!(*colour & 0x00ff_ffff, 0x0042_85f4);
        assert_eq!(*count, 500);
    }

    #[test]
    fn lab_round_trips_through_the_quantisers_own_conversion() {
        for argb in [0xff00_0000u32, 0xffff_ffff, 0xff42_85f4, 0xfff4_b400, 0xff0f_9d58] {
            let back = int_from_lab(lab_from_int(argb));
            let channel = |c: u32, shift: u32| ((c >> shift) & 0xff) as i32;
            for shift in [16, 8, 0] {
                let delta = (channel(argb, shift) - channel(back, shift)).abs();
                assert!(delta <= 1, "{argb:08x} came back as {back:08x}");
            }
        }
    }

    /// The whole pipeline against the C++ it replaces: decode each image in
    /// the corpus and quantise it, colour for colour and count for count.
    #[test]
    fn images_quantise_exactly_as_the_library_does() {
        use redcommon::json::Json;

        let vectors = redcommon::json::parse(include_str!("../../tests/quantize-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(images)) = vectors.get("images") else { panic!("no images") };

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpeg");
        let mut checked = 0usize;
        for image in images {
            let name = image.str_field("name").expect("image name");
            let bytes = std::fs::read(format!("{dir}/{name}")).expect("test image is readable");
            let decoded = super::super::jpeg::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            let ours = quantize_celebi(&pixels_of(&decoded), 128);

            let Some(Json::Arr(expected)) = image.get("colours") else { panic!("no colours") };
            assert_eq!(ours.len(), expected.len(), "{name}: colour count");
            for (pair, (colour, count)) in expected.iter().zip(&ours) {
                let Json::Arr(pair) = pair else { panic!("pairs") };
                let (Some(their_colour), Some(their_count)) =
                    (pair[0].as_u64(), pair[1].as_u64())
                else {
                    panic!("pairs are numbers")
                };
                assert_eq!(*colour as u64, their_colour, "{name}: colour");
                assert_eq!(*count as u64, their_count, "{name}: count of {colour:08x}");
                checked += 1;
            }
        }
        assert!(checked > 4_000, "only {checked} colours checked");
    }
}
