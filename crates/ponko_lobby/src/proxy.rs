//! Reverse proxies the server believes: a browser connection that comes
//! from one of them counts as the client address the proxy wrote into its
//! `X-Forwarded-For` header ([`ponko::net::WebPeer::client_ip`]).

use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

/// An address or a range of addresses (`10.0.0.0/8`, `fd00::/8`) of
/// reverse proxies in front of the server's WebSocket port, for
/// [`LobbyLimits::trusted_proxies`](crate::LobbyLimits::trusted_proxies).
///
/// ```
/// use ponko_lobby::TrustedProxy;
/// let range: TrustedProxy = "10.0.0.0/8".parse().unwrap();
/// assert!(range.contains("10.1.2.3".parse().unwrap()));
/// assert!(!range.contains("192.0.2.1".parse().unwrap()));
/// let one: TrustedProxy = "127.0.0.1".parse().unwrap();
/// assert_eq!(one.to_string(), "127.0.0.1/32");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrustedProxy {
    network: IpAddr,
    prefix: u8,
}

impl TrustedProxy {
    /// The addresses whose first `prefix` bits are `network`'s (all bits for
    /// one address: 32, or 128 for IPv6).
    ///
    /// ```
    /// let local = ponko_lobby::TrustedProxy::new("127.0.0.1".parse().unwrap(), 8).unwrap();
    /// assert!(local.contains("127.9.9.9".parse().unwrap()));
    /// ```
    ///
    /// # Errors
    ///
    /// A message when `prefix` is longer than the address.
    pub fn new(network: IpAddr, prefix: u8) -> Result<Self, String> {
        let network = network.to_canonical();
        let bits = if network.is_ipv4() { 32 } else { 128 };
        if prefix > bits {
            return Err(format!("{network}/{prefix}: the prefix is longer than {bits} bits"));
        }
        Ok(Self {
            network: mask(network, prefix),
            prefix,
        })
    }

    /// Whether `ip` is in the range (IPv4 addresses mapped into IPv6 count
    /// as IPv4).
    /// ```
    /// let any6: ponko_lobby::TrustedProxy = "::/0".parse().unwrap();
    /// assert!(any6.contains("2001:db8::1".parse().unwrap()));
    /// assert!(!any6.contains("192.0.2.1".parse().unwrap()));
    /// ```
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        ip.is_ipv4() == self.network.is_ipv4() && mask(ip, self.prefix) == self.network
    }

    /// Reads a comma-separated list (`LOBBY_TRUSTED_PROXIES`), skipping
    /// blanks.
    /// ```
    /// let list = ponko_lobby::TrustedProxy::parse_list("127.0.0.1, 10.0.0.0/8").unwrap();
    /// assert_eq!(list.len(), 2);
    /// assert!(ponko_lobby::TrustedProxy::parse_list("").unwrap().is_empty());
    /// ```
    ///
    /// # Errors
    ///
    /// The first entry that isn't an address or a range.
    pub fn parse_list(list: &str) -> Result<Vec<Self>, String> {
        list.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .collect()
    }
}

fn mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let keep = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
            IpAddr::from(std::net::Ipv4Addr::from(u32::from(v4) & keep))
        }
        IpAddr::V6(v6) => {
            let keep = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
            IpAddr::from(std::net::Ipv6Addr::from(u128::from(v6) & keep))
        }
    }
}

impl FromStr for TrustedProxy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (ip, prefix) = match s.split_once('/') {
            Some((ip, prefix)) => (ip, Some(prefix)),
            None => (s, None),
        };
        let ip: IpAddr = ip
            .trim()
            .parse()
            .map_err(|_| format!("{s}: not an IP address or range"))?;
        let ip = ip.to_canonical();
        let prefix = match prefix {
            Some(p) => p.trim().parse().map_err(|_| format!("{s}: not a prefix length"))?,
            None if ip.is_ipv4() => 32,
            None => 128,
        };
        Self::new(ip, prefix)
    }
}

impl fmt::Display for TrustedProxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_match_their_prefix_only() {
        let r: TrustedProxy = "192.168.4.0/22".parse().unwrap();
        assert!(r.contains("192.168.7.255".parse().unwrap()));
        assert!(!r.contains("192.168.8.0".parse().unwrap()));
        let mapped: IpAddr = "::ffff:192.168.5.1".parse().unwrap();
        assert!(r.contains(mapped), "a mapped IPv4 address is IPv4");
        let all4: TrustedProxy = "0.0.0.0/0".parse().unwrap();
        assert!(all4.contains("203.0.113.1".parse().unwrap()));
        assert!(!all4.contains("2001:db8::1".parse().unwrap()));
        let v6: TrustedProxy = "fd00::/8".parse().unwrap();
        assert!(v6.contains("fd12::1".parse().unwrap()));
        assert!(!v6.contains("fe80::1".parse().unwrap()));
    }

    #[test]
    fn bad_entries_are_refused_with_the_entry() {
        for bad in ["10.0.0.0/33", "::/129", "example.org", "10.0.0.0/x", "10.0.0.0/"] {
            let e = bad.parse::<TrustedProxy>().unwrap_err();
            assert!(e.starts_with(bad), "{e}");
        }
        assert!(TrustedProxy::parse_list("127.0.0.1, nope").is_err());
    }
}
