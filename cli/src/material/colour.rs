//! sRGB, XYZ, L* and the conversions between them.
//!
//! Ported from materialyoucolor's `utils/color_utils.py`.

use super::math::{clamp_int, matrix_multiply, round_half_to_even};
use super::tables::{SRGB_TO_XYZ, XYZ_TO_SRGB};

pub fn argb_from_rgb(red: i64, green: i64, blue: i64) -> u32 {
    (255 << 24 | (red & 255) << 16 | (green & 255) << 8 | (blue & 255)) as u32
}

pub fn red_from_argb(argb: u32) -> i64 {
    ((argb >> 16) & 255) as i64
}

pub fn green_from_argb(argb: u32) -> i64 {
    ((argb >> 8) & 255) as i64
}

pub fn blue_from_argb(argb: u32) -> i64 {
    (argb & 255) as i64
}

/// sRGB channel to linear light, 0..100.
pub fn linearized(component: i64) -> f64 {
    let normalized = component as f64 / 255.0;
    if normalized <= 0.040449936 {
        normalized / 12.92 * 100.0
    } else {
        ((normalized + 0.055) / 1.055).powf(2.4) * 100.0
    }
}

/// Linear light back to an sRGB channel.
pub fn delinearized(component: f64) -> i64 {
    let normalized = component / 100.0;
    let value = if normalized <= 0.0031308 {
        normalized * 12.92
    } else {
        1.055 * normalized.powf(1.0 / 2.4) - 0.055
    };
    clamp_int(0, 255, round_half_to_even(value * 255.0) as i64)
}

pub fn argb_from_linrgb(linrgb: [f64; 3]) -> u32 {
    argb_from_rgb(
        delinearized(linrgb[0]),
        delinearized(linrgb[1]),
        delinearized(linrgb[2]),
    )
}

pub fn argb_from_xyz(x: f64, y: f64, z: f64) -> u32 {
    let m = &XYZ_TO_SRGB;
    argb_from_rgb(
        delinearized(m[0][0] * x + m[0][1] * y + m[0][2] * z),
        delinearized(m[1][0] * x + m[1][1] * y + m[1][2] * z),
        delinearized(m[2][0] * x + m[2][1] * y + m[2][2] * z),
    )
}

pub fn xyz_from_argb(argb: u32) -> [f64; 3] {
    matrix_multiply(
        [
            linearized(red_from_argb(argb)),
            linearized(green_from_argb(argb)),
            linearized(blue_from_argb(argb)),
        ],
        &SRGB_TO_XYZ,
    )
}

fn lab_f(t: f64) -> f64 {
    const E: f64 = 216.0 / 24389.0;
    const KAPPA: f64 = 24389.0 / 27.0;
    if t > E {
        t.cbrt()
    } else {
        (KAPPA * t + 16.0) / 116.0
    }
}

fn lab_inv_f(ft: f64) -> f64 {
    const E: f64 = 216.0 / 24389.0;
    const KAPPA: f64 = 24389.0 / 27.0;
    let ft3 = ft * ft * ft;
    if ft3 > E {
        ft3
    } else {
        (116.0 * ft - 16.0) / KAPPA
    }
}

pub fn y_from_lstar(lstar: f64) -> f64 {
    100.0 * lab_inv_f((lstar + 16.0) / 116.0)
}

pub fn lstar_from_y(y: f64) -> f64 {
    lab_f(y / 100.0) * 116.0 - 16.0
}

pub fn lstar_from_argb(argb: u32) -> f64 {
    116.0 * lab_f(xyz_from_argb(argb)[1] / 100.0) - 16.0
}

pub fn argb_from_lstar(lstar: f64) -> u32 {
    let component = delinearized(y_from_lstar(lstar));
    argb_from_rgb(component, component, component)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_round_trip_through_linear_light() {
        for value in [0i64, 1, 17, 128, 254, 255] {
            assert_eq!(delinearized(linearized(value)), value, "channel {value}");
        }
    }

    #[test]
    fn black_and_white_land_where_they_should() {
        assert_eq!(argb_from_lstar(0.0), 0xff000000);
        assert_eq!(argb_from_lstar(100.0), 0xffffffff);
        assert!((lstar_from_argb(0xffffffff) - 100.0).abs() < 1e-9);
        assert!(lstar_from_argb(0xff000000).abs() < 1e-9);
    }

    #[test]
    fn argb_is_packed_the_way_the_rest_of_the_world_packs_it() {
        assert_eq!(argb_from_rgb(0x12, 0x34, 0x56), 0xff123456);
        assert_eq!(red_from_argb(0xff123456), 0x12);
        assert_eq!(green_from_argb(0xff123456), 0x34);
        assert_eq!(blue_from_argb(0xff123456), 0x56);
    }
}
