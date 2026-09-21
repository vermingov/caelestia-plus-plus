//! Whether that is the password.
//!
//! PAM, which is what every other thing on the machine that asks for a
//! password uses: the same rules, the same fingerprint reader if there is
//! one, the same lockout after too many tries. The alternative — reading a
//! hash out of `/etc/shadow` — needs the shell to be privileged, and a shell
//! that is privileged is a much worse thing to have.
//!
//! Blocking, and slow on purpose: PAM makes a wrong answer take a second or
//! two. Never on the thread that draws.

/// The services to ask, in order. A machine has `system-auth` where the
/// distribution composes one stack for everything; `login` is there on all
/// of them.
const SERVICES: [&str; 2] = ["system-auth", "login"];

/// Who is at the machine.
pub fn whoever() -> String {
    std::env::var("USER").ok().filter(|name| !name.is_empty()).unwrap_or_else(|| {
        // SAFETY: the pointer is only read, and only while it is valid.
        unsafe {
            let passwd = libc::getpwuid(libc::getuid());
            if passwd.is_null() {
                return String::new();
            }
            std::ffi::CStr::from_ptr((*passwd).pw_name).to_string_lossy().into_owned()
        }
    })
}

/// Whether `password` is the password of whoever is at the machine.
pub fn accepted(password: &str) -> bool {
    let user = whoever();
    if user.is_empty() {
        return false;
    }
    for service in SERVICES {
        let Ok(mut asking) = pam::Client::with_password(service) else { continue };
        asking.conversation_mut().set_credentials(user.as_str(), password);
        // A service that is not there fails differently from a password that
        // is wrong, but not in a way worth telling apart: either way this one
        // did not let anybody in, and the next is tried.
        if asking.authenticate().is_ok() {
            return true;
        }
    }
    false
}
