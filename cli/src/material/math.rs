//! The small numeric helpers the colour maths leans on.
//!
//! Ported from materialyoucolor's `utils/math_utils.py`. Two of them differ
//! from the obvious Rust spelling and the difference is load-bearing:
//! Python's `%` on a negative float returns a positive remainder, and
//! Python's `round` breaks ties to even.

pub fn signum(value: f64) -> f64 {
    if value < 0.0 {
        -1.0
    } else if value == 0.0 {
        0.0
    } else {
        1.0
    }
}

pub fn lerp(start: f64, stop: f64, amount: f64) -> f64 {
    (1.0 - amount) * start + amount * stop
}

pub fn clamp_double(min: f64, max: f64, value: f64) -> f64 {
    value.clamp(min, max)
}

pub fn clamp_int(min: i64, max: i64, value: i64) -> i64 {
    value.clamp(min, max)
}

/// Python's `%` on floats, which never returns a negative remainder.
pub fn sanitize_degrees_double(degrees: f64) -> f64 {
    degrees.rem_euclid(360.0)
}

pub fn sanitize_degrees_int(degrees: i64) -> i64 {
    degrees.rem_euclid(360)
}

pub fn rotation_direction(from: f64, to: f64) -> f64 {
    if sanitize_degrees_double(to - from) <= 180.0 {
        1.0
    } else {
        -1.0
    }
}

pub fn difference_degrees(a: f64, b: f64) -> f64 {
    180.0 - ((a - b).abs() - 180.0).abs()
}

pub fn matrix_multiply(row: [f64; 3], matrix: &[[f64; 3]; 3]) -> [f64; 3] {
    [
        row[0] * matrix[0][0] + row[1] * matrix[0][1] + row[2] * matrix[0][2],
        row[0] * matrix[1][0] + row[1] * matrix[1][1] + row[2] * matrix[1][2],
        row[0] * matrix[2][0] + row[1] * matrix[2][1] + row[2] * matrix[2][2],
    ]
}

/// Python's `round`: halfway cases go to the even neighbour, where Rust's
/// `f64::round` goes away from zero. One unit of red is a visible difference
/// when it lands on a surface colour.
pub fn round_half_to_even(value: f64) -> f64 {
    let rounded = value.round();
    if (value - value.trunc()).abs() == 0.5 && rounded % 2.0 != 0.0 {
        rounded - signum(value)
    } else {
        rounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_wrap_the_way_python_wraps_them() {
        assert_eq!(sanitize_degrees_double(-10.0), 350.0);
        assert_eq!(sanitize_degrees_double(370.0), 10.0);
        assert_eq!(sanitize_degrees_double(0.0), 0.0);
        assert_eq!(sanitize_degrees_int(-10), 350);
    }

    #[test]
    fn ties_round_to_even() {
        assert_eq!(round_half_to_even(0.5), 0.0);
        assert_eq!(round_half_to_even(1.5), 2.0);
        assert_eq!(round_half_to_even(2.5), 2.0);
        assert_eq!(round_half_to_even(-0.5), 0.0);
        assert_eq!(round_half_to_even(-1.5), -2.0);
        assert_eq!(round_half_to_even(2.4), 2.0);
        assert_eq!(round_half_to_even(2.6), 3.0);
    }

    #[test]
    fn the_short_way_round_is_the_direction() {
        assert_eq!(rotation_direction(0.0, 90.0), 1.0);
        assert_eq!(rotation_direction(0.0, 270.0), -1.0);
        assert_eq!(difference_degrees(350.0, 10.0), 20.0);
        assert_eq!(difference_degrees(10.0, 350.0), 20.0);
    }
}
