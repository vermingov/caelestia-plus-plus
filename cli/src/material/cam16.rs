//! CAM16, the appearance model HCT's hue and chroma come from.
//!
//! Ported from materialyoucolor's `hct/cam16.py`.

use super::colour::{blue_from_argb, green_from_argb, linearized, red_from_argb};
use super::math::{sanitize_degrees_double, signum};
use super::viewing::{self, ViewingConditions};

#[derive(Debug, Clone, Copy)]
pub struct Cam16 {
    pub hue: f64,
    pub chroma: f64,
    pub j: f64,
    pub q: f64,
    pub m: f64,
    pub s: f64,
    pub jstar: f64,
    pub astar: f64,
    pub bstar: f64,
}

impl Cam16 {
    pub fn from_int(argb: u32) -> Cam16 {
        Cam16::from_int_in(argb, viewing::standard())
    }

    pub fn from_int_in(argb: u32, vc: &ViewingConditions) -> Cam16 {
        let r = linearized(red_from_argb(argb));
        let g = linearized(green_from_argb(argb));
        let b = linearized(blue_from_argb(argb));
        let x = 0.41233895 * r + 0.35762064 * g + 0.18051042 * b;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = 0.01932141 * r + 0.11916382 * g + 0.95034478 * b;
        Cam16::from_xyz_in(x, y, z, vc)
    }

    pub fn from_xyz_in(x: f64, y: f64, z: f64, vc: &ViewingConditions) -> Cam16 {
        let r_c = 0.401288 * x + 0.650173 * y - 0.051461 * z;
        let g_c = -0.250268 * x + 1.204414 * y + 0.045854 * z;
        let b_c = -0.002079 * x + 0.048952 * y + 0.953127 * z;

        let r_d = vc.rgb_d[0] * r_c;
        let g_d = vc.rgb_d[1] * g_c;
        let b_d = vc.rgb_d[2] * b_c;

        let r_af = ((vc.fl * r_d.abs()) / 100.0).powf(0.42);
        let g_af = ((vc.fl * g_d.abs()) / 100.0).powf(0.42);
        let b_af = ((vc.fl * b_d.abs()) / 100.0).powf(0.42);

        let r_a = (signum(r_d) * 400.0 * r_af) / (r_af + 27.13);
        let g_a = (signum(g_d) * 400.0 * g_af) / (g_af + 27.13);
        let b_a = (signum(b_d) * 400.0 * b_af) / (b_af + 27.13);

        let a = (11.0 * r_a + -12.0 * g_a + b_a) / 11.0;
        let b = (r_a + g_a - 2.0 * b_a) / 9.0;
        let u = (20.0 * r_a + 20.0 * g_a + 21.0 * b_a) / 20.0;
        let p2 = (40.0 * r_a + 20.0 * g_a + b_a) / 20.0;

        let hue = sanitize_degrees_double(b.atan2(a).to_degrees());
        let hue_radians = hue.to_radians();

        let ac = p2 * vc.nbb;
        let j = 100.0 * (ac / vc.aw).powf(vc.c * vc.z);
        let q = (4.0 / vc.c) * (j / 100.0).sqrt() * (vc.aw + 4.0) * vc.f_l_root;

        // The hue wheel is cut at 20.14 degrees, not at zero.
        let hue_prime = if hue < 20.14 { hue + 360.0 } else { hue };
        let e_hue = 0.25 * ((hue_prime.to_radians() + 2.0).cos() + 3.8);
        let p1 = (50000.0 / 13.0) * e_hue * vc.nc * vc.ncb;
        let t = (p1 * (a * a + b * b).sqrt()) / (u + 0.305);
        let alpha = t.powf(0.9) * (1.64 - 0.29f64.powf(vc.n)).powf(0.73);
        let c = alpha * (j / 100.0).sqrt();
        let m = c * vc.f_l_root;
        let s = 50.0 * ((alpha * vc.c) / (vc.aw + 4.0)).sqrt();

        let jstar = ((1.0 + 100.0 * 0.007) * j) / (1.0 + 0.007 * j);
        let mstar = (1.0 / 0.0228) * (1.0 + 0.0228 * m).ln();

        Cam16 {
            hue,
            chroma: c,
            j,
            q,
            m,
            s,
            jstar,
            astar: mstar * hue_radians.cos(),
            bstar: mstar * hue_radians.sin(),
        }
    }

    /// CAM16-UCS distance, which is what blending interpolates along.
    pub fn distance(&self, other: &Cam16) -> f64 {
        let d_j = self.jstar - other.jstar;
        let d_a = self.astar - other.astar;
        let d_b = self.bstar - other.bstar;
        let d_e_prime = (d_j * d_j + d_a * d_a + d_b * d_b).sqrt();
        1.41 * d_e_prime.powf(0.63)
    }
}
