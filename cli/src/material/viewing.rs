//! The viewing conditions CAM16 is computed under.
//!
//! One set, built once: D65, a mid-grey background, average surround. Every
//! colour in the shell is judged against these, so they are a constant rather
//! than a parameter.

use super::colour::y_from_lstar;
use super::math::lerp;
use super::tables::WHITE_POINT_D65;

pub struct ViewingConditions {
    pub n: f64,
    pub aw: f64,
    pub nbb: f64,
    pub ncb: f64,
    pub c: f64,
    pub nc: f64,
    pub rgb_d: [f64; 3],
    pub fl: f64,
    pub f_l_root: f64,
    pub z: f64,
}

impl ViewingConditions {
    pub fn standard() -> ViewingConditions {
        let adapting_luminance = (200.0 / std::f64::consts::PI) * y_from_lstar(50.0) / 100.0;
        ViewingConditions::make(WHITE_POINT_D65, adapting_luminance, 50.0, 2.0, false)
    }

    pub fn make(
        white_point: [f64; 3],
        adapting_luminance: f64,
        background_lstar: f64,
        surround: f64,
        discounting_illuminant: bool,
    ) -> ViewingConditions {
        let [x, y, z] = white_point;
        let r_w = x * 0.401288 + y * 0.650173 + z * -0.051461;
        let g_w = x * -0.250268 + y * 1.204414 + z * 0.045854;
        let b_w = x * -0.002079 + y * 0.048952 + z * 0.953127;

        let f = 0.8 + surround / 10.0;
        let c = if f >= 0.9 {
            lerp(0.59, 0.69, (f - 0.9) * 10.0)
        } else {
            lerp(0.525, 0.59, (f - 0.8) * 10.0)
        };

        let d = if discounting_illuminant {
            1.0
        } else {
            f * (1.0 - (1.0 / 3.6) * ((-adapting_luminance - 42.0) / 92.0).exp())
        };
        let d = d.clamp(0.0, 1.0);

        let rgb_d = [
            d * (100.0 / r_w) + 1.0 - d,
            d * (100.0 / g_w) + 1.0 - d,
            d * (100.0 / b_w) + 1.0 - d,
        ];

        let k = 1.0 / (5.0 * adapting_luminance + 1.0);
        let k4 = k * k * k * k;
        let k4_f = 1.0 - k4;
        let fl = k4 * adapting_luminance
            + 0.1 * k4_f * k4_f * (5.0 * adapting_luminance).powf(1.0 / 3.0);

        let n = y_from_lstar(background_lstar) / white_point[1];
        let z_factor = 1.48 + n.sqrt();
        let nbb = 0.725 / n.powf(0.2);

        let factors = [
            ((fl * rgb_d[0] * r_w) / 100.0).powf(0.42),
            ((fl * rgb_d[1] * g_w) / 100.0).powf(0.42),
            ((fl * rgb_d[2] * b_w) / 100.0).powf(0.42),
        ];
        let rgb_a = [
            (400.0 * factors[0]) / (factors[0] + 27.13),
            (400.0 * factors[1]) / (factors[1] + 27.13),
            (400.0 * factors[2]) / (factors[2] + 27.13),
        ];
        let aw = (2.0 * rgb_a[0] + rgb_a[1] + 0.05 * rgb_a[2]) * nbb;

        ViewingConditions {
            n,
            aw,
            nbb,
            ncb: nbb,
            c,
            nc: f,
            rgb_d,
            fl,
            f_l_root: fl.powf(0.25),
            z: z_factor,
        }
    }
}

/// Built once and shared: nothing here varies at runtime.
pub fn standard() -> &'static ViewingConditions {
    use std::sync::OnceLock;
    static VC: OnceLock<ViewingConditions> = OnceLock::new();
    VC.get_or_init(ViewingConditions::standard)
}
