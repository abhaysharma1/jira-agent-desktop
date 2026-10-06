//! Local crash capture for Phase 27.
//!
//! Real crash *reporting* (uploading) is deferred; what this adds is the local
//! half — an uncaught panic writes one `panic` line into the Phase 26
//! structured log before the process dies. The release profile uses
//! `panic = "abort"`, but the panic hook still runs first, so the last record a
//! crashed install leaves behind is readable in `~/.jira-agent/logs/`.

use std::panic::PanicHookInfo;
use std::sync::Once;

use serde_json::{json, Value};

static INSTALL: Once = Once::new();

/// Installs the crash-capturing panic hook once per process. Chains to the
/// previous hook so the usual stderr message is still printed.
pub fn install() {
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = panic_message(info);
            let thread = std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_string();
            let location = info.location().map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            });
            let backtrace = capture_backtrace();
            crate::logging::error(
                "panic",
                "uncaught panic",
                build_fields(&message, &thread, location.as_deref(), backtrace.as_deref()),
            );
            previous(info);
        }));
    });
}

/// Pure field builder, kept separate so it can be unit-tested without having to
/// trigger a real panic.
pub fn build_fields(
    message: &str,
    thread: &str,
    location: Option<&str>,
    backtrace: Option<&str>,
) -> Value {
    let mut fields = json!({ "message": message, "thread": thread });
    if let Some(location) = location {
        fields["location"] = Value::String(location.to_string());
    }
    if let Some(backtrace) = backtrace.filter(|value| !value.is_empty()) {
        fields["backtrace"] = Value::String(backtrace.to_string());
    }
    fields
}

fn panic_message(info: &PanicHookInfo) -> String {
    if let Some(message) = info.payload().downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Only captured when `RUST_BACKTRACE` asks for it: release binaries are
/// stripped, so an unsymbolized backtrace would be misleading noise.
fn capture_backtrace() -> Option<String> {
    match std::env::var("RUST_BACKTRACE") {
        Ok(value) if !value.is_empty() && value != "0" => {
            Some(std::backtrace::Backtrace::force_capture().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_fields_with_location() {
        let fields = build_fields("boom", "main", Some("src/lib.rs:10:5"), None);
        assert_eq!(fields["message"], "boom");
        assert_eq!(fields["thread"], "main");
        assert_eq!(fields["location"], "src/lib.rs:10:5");
        assert!(fields.get("backtrace").is_none());
    }

    #[test]
    fn includes_backtrace_when_provided() {
        let fields = build_fields("boom", "main", None, Some("frame 1"));
        assert_eq!(fields["backtrace"], "frame 1");
    }

    #[test]
    fn installs_a_hook_that_survives_a_panic() {
        install();
        let result = std::panic::catch_unwind(|| panic!("phase-27 test panic"));
        assert!(result.is_err());
    }
}
