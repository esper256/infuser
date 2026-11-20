//! Peer addresses the relay must not contact. Same ranges as the coturn
//! example: loopback, private, link-local, shared, multicast, and the rest
//! of the non-global space. The relay's own public address is allowed so two
//! allocations on this host can still reach each other.

use std::net::{IpAddr, Ipv6Addr};

const V4: &[(u32, u32)] = &[
    (0x0000_0000, 0x00ff_ffff),
    (0x0a00_0000, 0x0aff_ffff),
    (0x6440_0000, 0x647f_ffff),
    (0x7f00_0000, 0x7fff_ffff),
    (0xa9fe_0000, 0xa9fe_ffff),
    (0xac10_0000, 0xac1f_ffff),
    (0xc0a8_0000, 0xc0a8_ffff),
    (0xe000_0000, 0xffff_ffff),
];

pub fn denied(ip: IpAddr, public: IpAddr) -> bool {
    if ip == public {
        return false;
    }
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            V4.iter()
                .any(|(start, end)| (start..=end).contains(&&value))
        }
        IpAddr::V6(ip) => denied_v6(ip),
    }
}

fn denied_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn private_and_special_ranges_are_denied_and_the_public_relay_is_not() {
        let public = v4(203, 0, 113, 10);
        for ip in [
            v4(0, 0, 0, 0),
            v4(0, 255, 255, 255),
            v4(10, 0, 0, 1),
            v4(10, 255, 255, 255),
            v4(100, 64, 0, 1),
            v4(100, 127, 255, 255),
            v4(127, 0, 0, 1),
            v4(169, 254, 1, 1),
            v4(172, 16, 0, 1),
            v4(172, 31, 255, 255),
            v4(192, 168, 1, 1),
            v4(224, 0, 0, 1),
            v4(255, 255, 255, 255),
        ] {
            assert!(denied(ip, public), "{ip} should be denied");
        }
        assert!(!denied(public, public));
        assert!(!denied(v4(203, 0, 113, 11), public));
        assert!(!denied(v4(8, 8, 8, 8), public));
        assert!(!denied(v4(172, 32, 0, 1), public));
        assert!(!denied(v4(100, 128, 0, 1), public));
        assert!(denied(IpAddr::V6(Ipv6Addr::LOCALHOST), public));
        assert!(denied("fd12::1".parse().unwrap(), public));
        assert!(denied("fe80::1".parse().unwrap(), public));
        assert!(!denied("2001:db8::1".parse().unwrap(), public));
        // A loopback public address is the relay itself, so hairpin stays possible.
        assert!(!denied(v4(127, 0, 0, 1), v4(127, 0, 0, 1)));
        assert!(denied(v4(127, 0, 0, 2), v4(127, 0, 0, 1)));
    }
}
