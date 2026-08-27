//! Named pipe transport (Windows), ACL-restricted to the current user (spec §6.4, §14.5).

use std::ffi::c_void;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::sync::Arc;

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

use crate::server::{handle_connection, RequestHandler};

const PIPE_CONNECT_TIMEOUT_MS: u32 = 500;

pub fn pipe_name(user_identifier: &str) -> String {
    format!(r"\\.\pipe\lcp-{user_identifier}")
}

/// Best-effort current-user identifier used both to scope the pipe name and (via the SID form)
/// to build its ACL. Falls back to `USERNAME` if the SID lookup fails for any reason -- the
/// pipe still ends up user-scoped by name even in that fallback case.
///
/// When `LCP_PROFILE_DIR` is set (the dev/test escape hatch for running multiple local
/// "machines", see `lcp-core::config::app_dir`), a short suffix derived from it is appended so
/// two profiles never collide on the same pipe name. The ACL itself is unaffected: both
/// profiles are still the same real OS user, so the same SID-based access grant is correct for
/// each independently.
pub fn current_user_identifier() -> String {
    let base = current_user_sid().unwrap_or_else(|| {
        std::env::var("USERNAME").unwrap_or_else(|_| "unknown-user".to_string())
    });
    match std::env::var("LCP_PROFILE_DIR") {
        Ok(dir) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            dir.hash(&mut hasher);
            format!("{base}-{:x}", hasher.finish())
        }
        Err(_) => base,
    }
}

fn current_user_sid() -> Option<String> {
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?.trim();
    let sid = line.rsplit(',').next()?.trim().trim_matches('"');
    sid.starts_with("S-1-").then(|| sid.to_string())
}

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

struct SecurityDescriptorGuard(*mut c_void);

impl Drop for SecurityDescriptorGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(self.0);
            }
        }
    }
}

/// Builds a security descriptor granting only `user_sid` access (`D:P(A;;GA;;;<sid>)` -- a
/// protected DACL with one full-access ACE) and creates the first pipe instance with it.
fn create_first_instance(addr: &str, user_sid: &str) -> io::Result<NamedPipeServer> {
    let sddl = to_wide_null(&format!("D:P(A;;GA;;;{user_sid})"));
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    let ok = unsafe {
        windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1, // SDDL_REVISION_1
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let _guard = SecurityDescriptorGuard(descriptor);

    let mut attrs = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };

    let mut options = ServerOptions::new();
    options.first_pipe_instance(true);
    unsafe {
        options.create_with_security_attributes_raw(addr, &mut attrs as *mut _ as *mut c_void)
    }
}

/// Creates the pipe under the current user's name/SID. Fails (rather than falling back to an
/// unrestricted pipe) if the SID can't be determined, since an ACL we can't verify is worse
/// than a clear startup error.
pub fn bind_first_instance() -> io::Result<(NamedPipeServer, String)> {
    let user_sid = current_user_sid()
        .ok_or_else(|| io::Error::other("could not determine current user SID for pipe ACL"))?;
    let addr = pipe_name(&current_user_identifier());
    let server = create_first_instance(&addr, &user_sid)?;
    Ok((server, addr))
}

pub async fn serve<H: RequestHandler>(first: NamedPipeServer, addr: String, handler: Arc<H>) {
    let mut current = first;
    loop {
        if let Err(e) = current.connect().await {
            tracing::warn!(error = %e, "named pipe connect failed");
            break;
        }
        let next = match ServerOptions::new().create(&addr) {
            Ok(server) => server,
            Err(e) => {
                tracing::warn!(error = %e, "failed to create next named pipe instance");
                break;
            }
        };
        let connected = std::mem::replace(&mut current, next);
        let handler = handler.clone();
        tokio::spawn(async move {
            handle_connection(connected, handler).await;
        });
    }
}

pub async fn connect(user_identifier: &str) -> io::Result<NamedPipeClient> {
    let addr = pipe_name(user_identifier);
    let wide = to_wide_null(&addr);
    let pipe_available = unsafe { WaitNamedPipeW(wide.as_ptr(), PIPE_CONNECT_TIMEOUT_MS) };
    if pipe_available == 0 {
        return Err(io::Error::last_os_error());
    }
    ClientOptions::new().open(&addr)
}
