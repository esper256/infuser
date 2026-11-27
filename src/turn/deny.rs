//! Peer addresses the relay must not contact. Same ranges as the coturn
//! example, plus the benchmarking range, with IPv4-mapped IPv6 folded back to
//! IPv4. Documentation ranges such as 203.0.113.0/24 stay reachable so tests
//! can use them as a stand-in public address. The public address is allowed
//! only on the relay port range, so the listener and the game ports stay
//! unreachable.

use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Mutex,
};

const V4: &[(u32, u32)] = &[
    (0x0000_0000, 0x00ff_ffff),
    (0x0a00_0000, 0x0aff_ffff),
    (0x6440_0000, 0x647f_ffff),
    (0x7f00_0000, 0x7fff_ffff),
    (0xa9fe_0000, 0xa9fe_ffff),
    (0xac10_0000, 0xac1f_ffff),
    (0xc000_0000, 0xc000_00ff),
    (0xc0a8_0000, 0xc0a8_ffff),
    (0xc612_0000, 0xc613_ffff),
    (0xe000_0000, 0xffff_ffff),
];

static EXCEPTIONS: Mutex<Vec<(IpAddr, u16, u16)>> = Mutex::new(Vec::new());

pub fn install(public: IpAddr, relay_start: u16, relay_end: u16) {
    let mut exceptions = EXCEPTIONS.lock().unwrap();
    let exception = (public, relay_start, relay_end);
    if !exceptions.contains(&exception) {
        exceptions.push(exception);
    }
    drop(exceptions);
    turn::server::set_peer_filter(filtered);
}

fn filtered(addr: SocketAddr) -> bool {
    let ip = addr.ip().to_canonical();
    let exceptions = EXCEPTIONS.lock().unwrap();
    let ranges: Vec<_> = exceptions
        .iter()
        .filter(|(public, _, _)| *public == ip)
        .collect();
    if !ranges.is_empty() {
        return !ranges
            .iter()
            .any(|(_, start, end)| (*start..=*end).contains(&addr.port()));
    }
    ip_denied(ip)
}

pub fn denied(addr: SocketAddr, public: IpAddr, relay_start: u16, relay_end: u16) -> bool {
    let ip = addr.ip().to_canonical();
    if ip == public {
        return !(relay_start..=relay_end).contains(&addr.port());
    }
    ip_denied(ip)
}

fn ip_denied(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            V4.iter()
                .any(|(start, end)| (start..=end).contains(&&value))
        }
        IpAddr::V6(ip) => denied_v6(ip),
    }
}

fn denied_v6(ip: Ipv6Addr) -> bool {
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
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

    fn at(ip: IpAddr, port: u16) -> SocketAddr {
        SocketAddr::new(ip, port)
    }

    #[test]
    fn private_ranges_are_denied_and_only_the_public_relay_ports_are_open() {
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
            v4(192, 0, 0, 1),
            v4(192, 168, 1, 1),
            v4(198, 18, 0, 1),
            v4(198, 19, 255, 255),
            v4(224, 0, 0, 1),
            v4(255, 255, 255, 255),
        ] {
            assert!(
                denied(at(ip, 8800), public, 8800, 8899),
                "{ip} should be denied"
            );
        }
        assert!(!denied(at(public, 8800), public, 8800, 8899));
        assert!(denied(at(public, 8700), public, 8800, 8899));
        assert!(denied(at(public, 443), public, 8800, 8899));
        assert!(!denied(at(v4(203, 0, 113, 11), 9), public, 8800, 8899));
        assert!(!denied(at(v4(8, 8, 8, 8), 9), public, 8800, 8899));
        assert!(!denied(at(v4(172, 32, 0, 1), 9), public, 8800, 8899));
        assert!(!denied(at(v4(100, 128, 0, 1), 9), public, 8800, 8899));
        assert!(denied(
            at(IpAddr::V6(Ipv6Addr::LOCALHOST), 9),
            public,
            8800,
            8899
        ));
        assert!(denied(
            at(Ipv6Addr::UNSPECIFIED.into(), 9),
            public,
            8800,
            8899
        ));
        assert!(denied(
            at("ff02::1".parse().unwrap(), 9),
            public,
            8800,
            8899
        ));
        assert!(denied(
            at("fd12::1".parse().unwrap(), 9),
            public,
            8800,
            8899
        ));
        assert!(denied(
            at("fe80::1".parse().unwrap(), 9),
            public,
            8800,
            8899
        ));
        assert!(!denied(
            at("2001:db8::1".parse().unwrap(), 9),
            public,
            8800,
            8899
        ));
        let mapped: IpAddr = "::ffff:10.1.2.3".parse().unwrap();
        assert!(denied(at(mapped, 9), public, 8800, 8899));
        assert!(!denied(
            at(v4(127, 0, 0, 1), 8801),
            v4(127, 0, 0, 1),
            8800,
            8899
        ));
        assert!(denied(
            at(v4(127, 0, 0, 1), 8700),
            v4(127, 0, 0, 1),
            8800,
            8899
        ));
        assert!(denied(
            at(v4(127, 0, 0, 2), 8801),
            v4(127, 0, 0, 1),
            8800,
            8899
        ));
    }
}
