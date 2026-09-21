//! Asking the web for something, which the shell does a few times an hour.
//!
//! Each question is a `curl` run to its end and thrown away. A TLS stack kept
//! in a process that is up all day, for a forecast twice an hour and the
//! words to a song, would cost more than every one of those processes put
//! together; and `curl` is on every machine this shell is.

use serde_json::Value;

/// Who is asking. OpenStreetMap's usage policy requires one that says, and
/// it is only polite to the others.
const AGENT: &str = "caelestia-plus-plus (+https://github.com/vermingov/caelestia-plus-plus)";

/// What is at `url`, as JSON. Nothing when the network is not there, when
/// the answer is not JSON, or when the far end says no. `referer` is for the
/// services that will not answer a request that does not look like it came
/// from their own pages.
pub fn json(url: &str, referer: Option<&str>) -> Option<Value> {
    let mut asking = std::process::Command::new("curl");
    asking.args(["-fsS", "--max-time", "12", "-A", AGENT]);
    if let Some(referer) = referer {
        asking.args(["-e", referer]);
    }
    let output = asking.arg(url).output().ok()?;
    output.status.success().then(|| serde_json::from_slice(&output.stdout).ok()).flatten()
}

/// Fetches what is at `url` into `file`. For a picture: a cover, which a
/// player names by where it is on the web.
pub fn download(url: &str, file: &std::path::Path) -> bool {
    let fetched = std::process::Command::new("curl").args(["-fsS", "--max-time", "12", "-A", AGENT, "-o"]).arg(file).arg(url).status();
    fetched.is_ok_and(|status| status.success())
}

/// `text` as it goes in a URL's query.
pub fn encoded(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::encoded;

    #[test]
    fn what_a_url_cannot_carry_is_escaped() {
        assert_eq!(encoded("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(encoded("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encoded("rock & roll?"), "rock%20%26%20roll%3F");
    }
}
