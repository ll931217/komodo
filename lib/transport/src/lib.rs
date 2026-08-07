use std::{
  net::{IpAddr, SocketAddr},
  str::FromStr,
};

pub mod auth;
pub mod channel;
pub mod timeout;
pub mod websocket;

/// - Fixes ws addresses:
///   - `11.11.11.11:9120` => `ws://11.11.11.11:9120`
///   - `server.domain` => `wss://server.domain`
///   - `http://server.domain` => `ws://server.domain`
///   - `https://server.domain` => `wss://server.domain`
///
/// Anything resolving to `ws://` is unencrypted, and that matters more here
/// than the scheme suggests: the Noise handshake this connection performs is
/// authentication only (the transport state is never promoted with
/// `into_transport_mode`), so TLS is the *only* thing providing
/// confidentiality. Core sends fully-interpolated secrets — and the
/// `(secret_value, name)` replacer pairs — as plain JSON over it. So the
/// downgrade is logged rather than left silent; see [warn_if_unencrypted].
pub fn fix_ws_address(address: &str) -> String {
  let fixed = fix_ws_address_inner(address);
  warn_if_unencrypted(&fixed);
  fixed
}

fn fix_ws_address_inner(address: &str) -> String {
  if address.starts_with("ws://") || address.starts_with("wss://") {
    return address.to_string();
  }
  if address.starts_with("http://") {
    return address.replace("http://", "ws://");
  }
  if address.starts_with("https://") {
    return address.replace("https://", "wss://");
  }
  // When using direct IPs, always use ws://
  if SocketAddr::from_str(address).is_ok()
    || IpAddr::from_str(address).is_ok()
  {
    return format!("ws://{address}");
  }
  format!("wss://{address}")
}

/// Loopback is the one `ws://` that is not a finding — nothing leaves the
/// host, and it is the normal shape for a Periphery running beside Core.
fn is_loopback(address: &str) -> bool {
  let host = address.trim_start_matches("ws://");
  let host = host.split('/').next().unwrap_or(host);
  let bare = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
  let bare = bare.trim_start_matches('[').trim_end_matches(']');
  bare == "localhost"
    || IpAddr::from_str(bare)
      .map(|ip| ip.is_loopback())
      .unwrap_or(false)
}

/// Warn once per connection attempt that traffic is going out in the clear.
///
/// Deliberately a log and not an error: existing deployments legitimately
/// run `ws://` over a trusted network, and hard-failing them on upgrade
/// would be worse than the exposure. The point is that the downgrade stops
/// being invisible.
pub fn warn_if_unencrypted(address: &str) {
  if !address.starts_with("ws://") || is_loopback(address) {
    return;
  }
  tracing::warn!(
    address,
    "Connection is UNENCRYPTED (ws://). Komodo's Noise handshake authenticates \
     but does not encrypt, so secrets interpolated into Deployments, Stacks, \
     Repos and Clusters cross this link in plaintext. Use a hostname or an \
     https:// address to get wss:// — a bare IP always resolves to ws://."
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scheme_resolution_is_unchanged() {
    assert_eq!(
      fix_ws_address_inner("11.11.11.11:9120"),
      "ws://11.11.11.11:9120"
    );
    assert_eq!(
      fix_ws_address_inner("server.domain"),
      "wss://server.domain"
    );
    assert_eq!(fix_ws_address_inner("http://a.b"), "ws://a.b");
    assert_eq!(fix_ws_address_inner("https://a.b"), "wss://a.b");
    assert_eq!(fix_ws_address_inner("ws://a.b"), "ws://a.b");
  }

  /// A Periphery on the same host is the expected `ws://`, and warning on it
  /// would train people to ignore the warning that matters.
  #[test]
  fn loopback_is_not_warned() {
    assert!(is_loopback("ws://localhost:8120"));
    assert!(is_loopback("ws://127.0.0.1:8120"));
    assert!(is_loopback("ws://[::1]:8120"));
    assert!(!is_loopback("ws://11.11.11.11:9120"));
    assert!(!is_loopback("ws://server.domain"));
  }
}
