//! Enough of RFC 5389 to notice Binding messages and Allocate usernames.

pub const BINDING_REQUEST: u16 = 0x0001;
pub const BINDING_SUCCESS: u16 = 0x0101;
const ALLOCATE_REQUEST: u16 = 0x0003;
const MAGIC: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const USERNAME: u16 = 0x0006;

pub struct Stun<'a> {
    pub class_type: u16,
    pub transaction: &'a [u8],
}

pub fn parse(packet: &[u8]) -> Option<Stun<'_>> {
    if packet.len() < 20 || packet[4..8] != MAGIC {
        return None;
    }
    let class_type = u16::from_be_bytes([packet[0], packet[1]]);
    let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if packet.len() < 20 + length {
        return None;
    }
    Some(Stun {
        class_type,
        transaction: &packet[8..20],
    })
}

pub fn allocate_username(packet: &[u8]) -> Option<&str> {
    let stun = parse(packet)?;
    if stun.class_type != ALLOCATE_REQUEST {
        return None;
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let mut attributes = &packet[20..20 + length];
    while attributes.len() >= 4 {
        let kind = u16::from_be_bytes([attributes[0], attributes[1]]);
        let size = u16::from_be_bytes([attributes[2], attributes[3]]) as usize;
        let padded = size.checked_add(3)? & !3;
        if attributes.len() < 4 + padded {
            return None;
        }
        if kind == USERNAME {
            return std::str::from_utf8(&attributes[4..4 + size]).ok();
        }
        attributes = &attributes[4 + padded..];
    }
    None
}

const MAX_FRAME: usize = 65535;

/// Client TURN-over-TCP framing used by browsers and coturn.
///
/// The top two bits select the header, which is the same test RFC 6544 uses
/// to separate STUN from other TCP bytes. A STUN or ChannelData message is
/// delimited by its own length field. Any other leading byte is an RFC 4571
/// length prefix. A short RFC 4571 prefix is indistinguishable from STUN, so
/// this listener does not expect that prefix on the client connection.
pub fn tcp_frame_len(header: &[u8; 4]) -> Option<usize> {
    let marker = header[0] & 0xc0;
    let total = if marker == 0x00 {
        20 + u16::from_be_bytes([header[2], header[3]]) as usize
    } else if marker == 0x40 {
        let length = u16::from_be_bytes([header[2], header[3]]) as usize;
        (4 + length + 3) & !3
    } else {
        let length = u16::from_be_bytes([header[0], header[1]]) as usize;
        if length < 2 {
            return None;
        }
        2 + length
    };
    (total <= MAX_FRAME).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_allocate_username_and_binding_ids() {
        let mut packet = vec![0x00, 0x03, 0x00, 0x10, 0x21, 0x12, 0xa4, 0x42];
        packet.extend_from_slice(&[9; 12]);
        packet.extend_from_slice(&[0x00, 0x06, 0x00, 0x0c]);
        packet.extend_from_slice(b"4600:peer123");
        assert_eq!(allocate_username(&packet), Some("4600:peer123"));
        let stun = parse(&packet).unwrap();
        assert_eq!(stun.transaction, &[9; 12]);
        packet[0] = 0x00;
        packet[1] = 0x01;
        assert!(allocate_username(&packet).is_none());
        assert_eq!(parse(&packet).unwrap().class_type, BINDING_REQUEST);
    }

    #[test]
    fn frames_stun_channel_data_and_length_prefix() {
        assert_eq!(tcp_frame_len(&[0x00, 0x01, 0x00, 0x08]), Some(28));
        assert_eq!(tcp_frame_len(&[0x40, 0x01, 0x00, 0x05]), Some(12));
        assert_eq!(tcp_frame_len(&[0x80, 0x00, 0x00, 0x00]), Some(2 + 0x8000));
        assert!(tcp_frame_len(&[0x00, 0x01, 0xff, 0xff]).is_none());
    }
}
