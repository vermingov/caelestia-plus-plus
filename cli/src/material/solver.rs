//! Going the other way: from a hue, chroma and tone to the sRGB colour that
//! has them, or the closest one that exists.
//!
//! Ported from materialyoucolor's `hct/hct_solver.py`. Most requests are
//! answered by Newton's method on J; when the colour asked for is outside the
//! gamut the cube is bisected instead, first to the edge that carries the
//! hue, then along it to the boundary.

use super::colour::{argb_from_linrgb, argb_from_lstar, y_from_lstar};
use super::math::{matrix_multiply, sanitize_degrees_double, signum};
use super::tables::{
    CRITICAL_PLANES, LINRGB_FROM_SCALED_DISCOUNT, SCALED_DISCOUNT_FROM_LINRGB, Y_FROM_LINRGB,
};
use super::viewing;

fn sanitize_radians(angle: f64) -> f64 {
    (angle + std::f64::consts::PI * 8.0).rem_euclid(std::f64::consts::PI * 2.0)
}

/// Like `delinearized`, but without the rounding and clamping: the solver
/// needs the exact position between two channel values.
fn true_delinearized(component: f64) -> f64 {
    let normalized = component / 100.0;
    let value = if normalized <= 0.0031308 {
        normalized * 12.92
    } else {
        1.055 * normalized.powf(1.0 / 2.4) - 0.055
    };
    value * 255.0
}

fn chromatic_adaptation(component: f64) -> f64 {
    let af = component.abs().powf(0.42);
    signum(component) * 400.0 * af / (af + 27.13)
}

fn hue_of(linrgb: [f64; 3]) -> f64 {
    let scaled = matrix_multiply(linrgb, &SCALED_DISCOUNT_FROM_LINRGB);
    let r_a = chromatic_adaptation(scaled[0]);
    let g_a = chromatic_adaptation(scaled[1]);
    let b_a = chromatic_adaptation(scaled[2]);
    let a = (11.0 * r_a + -12.0 * g_a + b_a) / 11.0;
    let b = (r_a + g_a - 2.0 * b_a) / 9.0;
    b.atan2(a)
}

fn are_in_cyclic_order(a: f64, b: f64, c: f64) -> bool {
    sanitize_radians(b - a) < sanitize_radians(c - a)
}

fn intercept(source: f64, mid: f64, target: f64) -> f64 {
    (mid - source) / (target - source)
}

fn lerp_point(source: [f64; 3], t: f64, target: [f64; 3]) -> [f64; 3] {
    [
        source[0] + (target[0] - source[0]) * t,
        source[1] + (target[1] - source[1]) * t,
        source[2] + (target[2] - source[2]) * t,
    ]
}

fn set_coordinate(source: [f64; 3], coordinate: f64, target: [f64; 3], axis: usize) -> [f64; 3] {
    lerp_point(
        source,
        intercept(source[axis], coordinate, target[axis]),
        target,
    )
}

fn is_bounded(x: f64) -> bool {
    (0.0..=100.0).contains(&x)
}

/// The nth vertex of the plane of constant Y through the RGB cube, or a
/// sentinel when that vertex is outside it.
fn nth_vertex(y: f64, n: i32) -> [f64; 3] {
    let [kr, kg, kb] = Y_FROM_LINRGB;
    let coord_a = if n % 4 <= 1 { 0.0 } else { 100.0 };
    let coord_b = if n % 2 == 0 { 0.0 } else { 100.0 };

    if n < 4 {
        let (g, b) = (coord_a, coord_b);
        let r = (y - g * kg - b * kb) / kr;
        if is_bounded(r) {
            return [r, g, b];
        }
    } else if n < 8 {
        let (b, r) = (coord_a, coord_b);
        let g = (y - r * kr - b * kb) / kg;
        if is_bounded(g) {
            return [r, g, b];
        }
    } else {
        let (r, g) = (coord_a, coord_b);
        let b = (y - r * kr - g * kg) / kb;
        if is_bounded(b) {
            return [r, g, b];
        }
    }
    [-1.0, -1.0, -1.0]
}

/// The edge of the constant-Y plane that the target hue crosses.
fn bisect_to_segment(y: f64, target_hue: f64) -> ([f64; 3], [f64; 3]) {
    let mut left = [-1.0, -1.0, -1.0];
    let mut right = left;
    let mut left_hue = 0.0;
    let mut right_hue = 0.0;
    let mut initialized = false;
    let mut uncut = true;

    for n in 0..12 {
        let mid = nth_vertex(y, n);
        if mid[0] < 0.0 {
            continue;
        }
        let mid_hue = hue_of(mid);

        if !initialized {
            left = mid;
            right = mid;
            left_hue = mid_hue;
            right_hue = mid_hue;
            initialized = true;
            continue;
        }

        if uncut || are_in_cyclic_order(left_hue, mid_hue, right_hue) {
            uncut = false;
            if are_in_cyclic_order(left_hue, target_hue, mid_hue) {
                right = mid;
                right_hue = mid_hue;
            } else {
                left = mid;
                left_hue = mid_hue;
            }
        }
    }
    (left, right)
}

fn midpoint(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        (a[0] + b[0]) / 2.0,
        (a[1] + b[1]) / 2.0,
        (a[2] + b[2]) / 2.0,
    ]
}

fn critical_plane_below(x: f64) -> i64 {
    (x - 0.5).floor() as i64
}

fn critical_plane_above(x: f64) -> i64 {
    (x - 0.5).ceil() as i64
}

/// Walk that edge to the gamut boundary, stepping between the planes where a
/// channel crosses an integer value.
fn bisect_to_limit(y: f64, target_hue: f64) -> [f64; 3] {
    let (mut left, mut right) = bisect_to_segment(y, target_hue);
    let mut left_hue = hue_of(left);

    for axis in 0..3 {
        if left[axis] == right[axis] {
            continue;
        }
        let (mut l_plane, mut r_plane) = if left[axis] < right[axis] {
            (
                critical_plane_below(true_delinearized(left[axis])),
                critical_plane_above(true_delinearized(right[axis])),
            )
        } else {
            (
                critical_plane_above(true_delinearized(left[axis])),
                critical_plane_below(true_delinearized(right[axis])),
            )
        };

        for _ in 0..8 {
            if (r_plane - l_plane).abs() <= 1 {
                break;
            }
            let m_plane = ((l_plane + r_plane) as f64 / 2.0).floor() as i64;
            let coordinate = CRITICAL_PLANES[m_plane.clamp(0, 254) as usize];
            let mid = set_coordinate(left, coordinate, right, axis);
            let mid_hue = hue_of(mid);

            if are_in_cyclic_order(left_hue, target_hue, mid_hue) {
                right = mid;
                r_plane = m_plane;
            } else {
                left = mid;
                left_hue = mid_hue;
                l_plane = m_plane;
            }
        }
    }
    midpoint(left, right)
}

fn inverse_chromatic_adaptation(adapted: f64) -> f64 {
    let magnitude = adapted.abs();
    let base = (27.13 * magnitude / (400.0 - magnitude)).max(0.0);
    signum(adapted) * base.powf(1.0 / 0.42)
}

/// Newton's method on J. Returns 0 when the colour does not exist in sRGB,
/// which is the caller's signal to bisect instead.
fn find_result_by_j(hue_radians: f64, chroma: f64, y: f64) -> u32 {
    let vc = viewing::standard();
    let mut j = y.sqrt() * 11.0;
    let t_inner_coeff = 1.0 / (1.64 - 0.29f64.powf(vc.n)).powf(0.73);
    let e_hue = 0.25 * ((hue_radians + 2.0).cos() + 3.8);
    let p1 = e_hue * (50000.0 / 13.0) * vc.nc * vc.ncb;
    let h_sin = hue_radians.sin();
    let h_cos = hue_radians.cos();

    for round in 0..5 {
        let j_normalized = j / 100.0;
        let alpha = if chroma == 0.0 || j == 0.0 {
            0.0
        } else {
            chroma / j_normalized.sqrt()
        };
        let t = (alpha * t_inner_coeff).powf(1.0 / 0.9);
        let ac = vc.aw * j_normalized.powf(1.0 / vc.c / vc.z);
        let p2 = ac / vc.nbb;
        let gamma = 23.0 * (p2 + 0.305) * t / (23.0 * p1 + 11.0 * t * h_cos + 108.0 * t * h_sin);
        let a = gamma * h_cos;
        let b = gamma * h_sin;
        let r_a = (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0;
        let g_a = (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0;
        let b_a = (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0;

        let linrgb = matrix_multiply(
            [
                inverse_chromatic_adaptation(r_a),
                inverse_chromatic_adaptation(g_a),
                inverse_chromatic_adaptation(b_a),
            ],
            &LINRGB_FROM_SCALED_DISCOUNT,
        );

        if linrgb[0] < 0.0 || linrgb[1] < 0.0 || linrgb[2] < 0.0 {
            return 0;
        }

        let [kr, kg, kb] = Y_FROM_LINRGB;
        let fnj = kr * linrgb[0] + kg * linrgb[1] + kb * linrgb[2];
        if fnj <= 0.0 {
            return 0;
        }

        if round == 4 || (fnj - y).abs() < 0.002 {
            if linrgb[0] > 100.01 || linrgb[1] > 100.01 || linrgb[2] > 100.01 {
                return 0;
            }
            return argb_from_linrgb(linrgb);
        }

        j -= (fnj - y) * j / (2.0 * fnj);
    }
    0
}

/// The colour with this hue, chroma and tone — or, when there is none, the
/// closest one sRGB can hold.
pub fn solve_to_int(hue_degrees: f64, chroma: f64, lstar: f64) -> u32 {
    if chroma < 0.0001 || lstar < 0.0001 || lstar > 99.9999 {
        return argb_from_lstar(lstar);
    }
    let hue_radians = sanitize_degrees_double(hue_degrees) / 180.0 * std::f64::consts::PI;
    let y = y_from_lstar(lstar);

    let exact = find_result_by_j(hue_radians, chroma, y);
    if exact != 0 {
        return exact;
    }
    argb_from_linrgb(bisect_to_limit(y, hue_radians))
}
