//! The calculator mode, which is qalc with a timeout.
//!
//! The shell embeds libqalculate through its own plugin; there is no reason
//! to link it here when the binary is already on the machine and a single
//! evaluation costs a few milliseconds.

use std::process::Command;

/// Evaluates an expression, returning what to show and what to copy.
/// The two differ: the display carries the whole `1 + 1 = 2`, the clipboard
/// only wants `2`.
pub fn evaluate(expression: &str) -> Option<(String, String)> {
    let expression = expression.trim();
    if expression.is_empty() {
        return None;
    }

    let output = Command::new("qalc")
        // -t prints only the result, which is what goes on the clipboard.
        .args(["-t", "-s", "update_exchange_rates 0", expression])
        .output()
        .ok()?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if result.is_empty() {
        return None;
    }

    let display = format!("{expression} = {result}");
    Some((display, result))
}

/// qalc says so itself when it cannot make sense of something; the UI colours
/// those differently rather than pretending they are answers.
pub fn is_error(result: &str) -> bool {
    result.contains("error: ") || result.contains("warning: ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_arithmetic() {
        let (display, clipboard) = evaluate("2 + 2").expect("qalc answered");
        assert!(display.starts_with("2 + 2 = "), "{display}");
        assert_eq!(clipboard, "4");
    }

    #[test]
    fn handles_units_the_way_qalc_does() {
        let (_, clipboard) = evaluate("1 km to m").expect("qalc answered");
        assert!(clipboard.contains('m'), "{clipboard}");
    }

    #[test]
    fn an_empty_expression_has_no_answer() {
        assert!(evaluate("").is_none());
        assert!(evaluate("   ").is_none());
    }

    #[test]
    fn qalcs_own_complaints_are_recognised() {
        // qalc is forgiving — it reads `2 +* 2` as 4 — so what matters is
        // that when it does complain, the complaint is spotted rather than
        // shown as an answer.
        assert!(is_error("error: undefined symbol"));
        assert!(is_error("warning: assuming degrees"));
        assert!(!is_error("2 + 2 = 4"));
    }
}
