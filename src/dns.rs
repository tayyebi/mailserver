//! Minimal DNS client for the admin toolbox.
//!
//! Sends one question over UDP to the servers listed in /etc/resolv.conf,
//! retries over TCP when the reply is truncated, and decodes the answer
//! section for the record types the toolbox needs (A, AAAA, MX, TXT).
//!
//! This avoids shelling out to `dig` or `nslookup`, which the container image
//! does not provide.

use std::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);
const CLASS_IN: u16 = 1;
const FLAG_RESPONSE: u16 = 0x8000;
const FLAG_TRUNCATED: u16 = 0x0200;
const RCODE_NXDOMAIN: u8 = 3;
const MAX_POINTER_JUMPS: usize = 32;
const HEADER_LEN: usize = 12;

pub const TYPE_A: u16 = 1;
pub const TYPE_MX: u16 = 15;
pub const TYPE_TXT: u16 = 16;
pub const TYPE_AAAA: u16 = 28;

#[derive(Debug, Clone, PartialEq)]
pub enum RData {
    A(Ipv4Addr),
    Aaaa(Ipv6Addr),
    Mx { preference: u16, exchange: String },
    Txt(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DnsError {
    NoServers,
    Network(String),
    Malformed(String),
    ServerFailure(u8),
}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsError::NoServers => write!(f, "no DNS servers configured"),
            DnsError::Network(e) => write!(f, "network error: {}", e),
            DnsError::Malformed(e) => write!(f, "malformed DNS response: {}", e),
            DnsError::ServerFailure(code) => write!(f, "server returned rcode {}", code),
        }
    }
}

fn malformed(msg: &str) -> DnsError {
    DnsError::Malformed(msg.to_string())
}

fn net_err(e: std::io::Error) -> DnsError {
    DnsError::Network(e.to_string())
}

// ── Public lookups ──

/// TXT strings published at `name`. NXDOMAIN and empty answers yield an empty list.
pub fn lookup_txt(name: &str) -> Result<Vec<String>, DnsError> {
    Ok(query(name, TYPE_TXT)?
        .into_iter()
        .filter_map(|r| match r {
            RData::Txt(t) => Some(t),
            _ => None,
        })
        .collect())
}

pub fn lookup_a(name: &str) -> Result<Vec<Ipv4Addr>, DnsError> {
    Ok(query(name, TYPE_A)?
        .into_iter()
        .filter_map(|r| match r {
            RData::A(ip) => Some(ip),
            _ => None,
        })
        .collect())
}

pub fn lookup_aaaa(name: &str) -> Result<Vec<Ipv6Addr>, DnsError> {
    Ok(query(name, TYPE_AAAA)?
        .into_iter()
        .filter_map(|r| match r {
            RData::Aaaa(ip) => Some(ip),
            _ => None,
        })
        .collect())
}

/// MX exchange host names, lowest preference value first.
pub fn lookup_mx(name: &str) -> Result<Vec<String>, DnsError> {
    let mut mx: Vec<(u16, String)> = query(name, TYPE_MX)?
        .into_iter()
        .filter_map(|r| match r {
            RData::Mx {
                preference,
                exchange,
            } => Some((preference, exchange)),
            _ => None,
        })
        .collect();
    mx.sort_by_key(|(pref, _)| *pref);
    Ok(mx.into_iter().map(|(_, host)| host).collect())
}

/// Sends a query for `name`/`qtype` to each configured server until one answers.
pub fn query(name: &str, qtype: u16) -> Result<Vec<RData>, DnsError> {
    let id: u16 = rand::random();
    let packet = build_query(id, name, qtype)?;
    let mut last_err = DnsError::NoServers;
    for server in system_resolvers() {
        let response = match exchange_udp(server, &packet) {
            Ok(bytes)
                if bytes.len() >= HEADER_LEN
                    && u16::from_be_bytes([bytes[2], bytes[3]]) & FLAG_TRUNCATED != 0 =>
            {
                // Truncated over UDP: repeat the question over TCP.
                exchange_tcp(server, &packet)
            }
            other => other,
        };
        match response.and_then(|bytes| parse_response(&bytes, id, qtype)) {
            Ok(records) => return Ok(records),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Nameservers from /etc/resolv.conf, or localhost when none are listed.
pub fn system_resolvers() -> Vec<SocketAddr> {
    let mut servers = Vec::new();
    if let Ok(conf) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in conf.lines() {
            if let Some(rest) = line.trim().strip_prefix("nameserver") {
                if let Ok(ip) = rest.trim().parse::<IpAddr>() {
                    servers.push(SocketAddr::new(ip, 53));
                }
            }
        }
    }
    if servers.is_empty() {
        servers.push(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 53));
    }
    servers
}

// ── Wire format: queries ──

fn build_query(id: u16, name: &str, qtype: u16) -> Result<Vec<u8>, DnsError> {
    let mut buf = Vec::with_capacity(HEADER_LEN + name.len() + 6);
    buf.extend_from_slice(&id.to_be_bytes());
    buf.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: recursion desired
    buf.extend_from_slice(&1u16.to_be_bytes()); // one question
    buf.extend_from_slice(&[0u8; 6]); // no answer, authority or additional records
    encode_name(name, &mut buf)?;
    buf.extend_from_slice(&qtype.to_be_bytes());
    buf.extend_from_slice(&CLASS_IN.to_be_bytes());
    Ok(buf)
}

fn encode_name(name: &str, buf: &mut Vec<u8>) -> Result<(), DnsError> {
    let name = name.trim_end_matches('.');
    if name.is_empty() || name.len() > 253 {
        return Err(malformed("name length out of range"));
    }
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(malformed("label length out of range"));
        }
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0);
    Ok(())
}

// ── Transport ──

fn exchange_udp(server: SocketAddr, packet: &[u8]) -> Result<Vec<u8>, DnsError> {
    let bind_addr = if server.is_ipv4() {
        SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)
    } else {
        SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0)
    };
    let socket = UdpSocket::bind(bind_addr).map_err(net_err)?;
    socket.set_read_timeout(Some(TIMEOUT)).map_err(net_err)?;
    socket.set_write_timeout(Some(TIMEOUT)).map_err(net_err)?;
    socket.connect(server).map_err(net_err)?;
    socket.send(packet).map_err(net_err)?;
    let mut buf = vec![0u8; 4096];
    let n = socket.recv(&mut buf).map_err(net_err)?;
    buf.truncate(n);
    Ok(buf)
}

fn exchange_tcp(server: SocketAddr, packet: &[u8]) -> Result<Vec<u8>, DnsError> {
    let mut stream = TcpStream::connect_timeout(&server, TIMEOUT).map_err(net_err)?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(net_err)?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(net_err)?;
    let len = u16::try_from(packet.len()).map_err(|_| malformed("query too long"))?;
    stream.write_all(&len.to_be_bytes()).map_err(net_err)?;
    stream.write_all(packet).map_err(net_err)?;
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).map_err(net_err)?;
    let mut body = vec![0u8; u16::from_be_bytes(len_buf) as usize];
    stream.read_exact(&mut body).map_err(net_err)?;
    Ok(body)
}

// ── Wire format: responses ──

fn parse_response(msg: &[u8], id: u16, qtype: u16) -> Result<Vec<RData>, DnsError> {
    let header = msg
        .get(..HEADER_LEN)
        .ok_or_else(|| malformed("message shorter than header"))?;
    if u16::from_be_bytes([header[0], header[1]]) != id {
        return Err(malformed("transaction id mismatch"));
    }
    let flags = u16::from_be_bytes([header[2], header[3]]);
    if flags & FLAG_RESPONSE == 0 {
        return Err(malformed("message is not a response"));
    }
    let rcode = (flags & 0x000f) as u8;
    if rcode == RCODE_NXDOMAIN {
        return Ok(Vec::new());
    }
    if rcode != 0 {
        return Err(DnsError::ServerFailure(rcode));
    }
    let qdcount = u16::from_be_bytes([header[4], header[5]]) as usize;
    let ancount = u16::from_be_bytes([header[6], header[7]]) as usize;

    let mut pos = HEADER_LEN;
    for _ in 0..qdcount {
        let (_, next) = read_name(msg, pos)?;
        // Skip the question's type and class.
        pos = next
            .checked_add(4)
            .filter(|&p| p <= msg.len())
            .ok_or_else(|| malformed("truncated question"))?;
    }

    let mut out = Vec::new();
    for _ in 0..ancount {
        let (_, next) = read_name(msg, pos)?;
        let fixed = msg
            .get(next..next + 10)
            .ok_or_else(|| malformed("truncated resource record"))?;
        let rtype = u16::from_be_bytes([fixed[0], fixed[1]]);
        let rdlen = u16::from_be_bytes([fixed[8], fixed[9]]) as usize;
        let rdata_start = next + 10;
        let rdata_end = rdata_start + rdlen;
        if rdata_end > msg.len() {
            return Err(malformed("rdata past end of message"));
        }
        if rtype == qtype {
            if let Some(record) = decode_rdata(msg, rdata_start, rdlen, rtype)? {
                out.push(record);
            }
        }
        pos = rdata_end;
    }
    Ok(out)
}

fn decode_rdata(
    msg: &[u8],
    start: usize,
    len: usize,
    rtype: u16,
) -> Result<Option<RData>, DnsError> {
    let data = &msg[start..start + len];
    Ok(match rtype {
        TYPE_A => {
            let octets: [u8; 4] = data
                .try_into()
                .map_err(|_| malformed("A record is not 4 bytes"))?;
            Some(RData::A(Ipv4Addr::from(octets)))
        }
        TYPE_AAAA => {
            let octets: [u8; 16] = data
                .try_into()
                .map_err(|_| malformed("AAAA record is not 16 bytes"))?;
            Some(RData::Aaaa(Ipv6Addr::from(octets)))
        }
        TYPE_MX => {
            if len < 3 {
                return Err(malformed("MX record too short"));
            }
            let preference = u16::from_be_bytes([data[0], data[1]]);
            let (exchange, _) = read_name(msg, start + 2)?;
            Some(RData::Mx {
                preference,
                exchange,
            })
        }
        TYPE_TXT => {
            // One or more length-prefixed character strings, concatenated.
            let mut text = Vec::with_capacity(len);
            let mut i = 0;
            while i < data.len() {
                let chunk_len = data[i] as usize;
                let chunk = data
                    .get(i + 1..i + 1 + chunk_len)
                    .ok_or_else(|| malformed("TXT string past end of rdata"))?;
                text.extend_from_slice(chunk);
                i += 1 + chunk_len;
            }
            Some(RData::Txt(String::from_utf8_lossy(&text).into_owned()))
        }
        _ => None,
    })
}

/// Reads a (possibly compressed) domain name at `start`. Returns the name and
/// the offset just past the name's bytes at `start`, not past any pointer target.
fn read_name(msg: &[u8], start: usize) -> Result<(String, usize), DnsError> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = start;
    let mut resume: Option<usize> = None;
    let mut jumps = 0usize;
    loop {
        let len = *msg
            .get(pos)
            .ok_or_else(|| malformed("name runs past end of message"))?;
        if len & 0xC0 == 0xC0 {
            let second = *msg
                .get(pos + 1)
                .ok_or_else(|| malformed("truncated compression pointer"))?;
            if resume.is_none() {
                resume = Some(pos + 2);
            }
            jumps += 1;
            if jumps > MAX_POINTER_JUMPS {
                return Err(malformed("too many compression pointers"));
            }
            pos = (((len & 0x3F) as usize) << 8) | second as usize;
            continue;
        }
        if len & 0xC0 != 0 {
            return Err(malformed("unsupported label type"));
        }
        if len == 0 {
            pos += 1;
            break;
        }
        let label = msg
            .get(pos + 1..pos + 1 + len as usize)
            .ok_or_else(|| malformed("label runs past end of message"))?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        pos += 1 + len as usize;
    }
    Ok((labels.join("."), resume.unwrap_or(pos)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a response with one question for `name`/`qtype` and the given
    /// answer records. Each answer is (rtype, rdata bytes).
    fn response(id: u16, flags: u16, name: &str, qtype: u16, answers: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&id.to_be_bytes());
        buf.extend_from_slice(&flags.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&(answers.len() as u16).to_be_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        encode_name(name, &mut buf).unwrap();
        buf.extend_from_slice(&qtype.to_be_bytes());
        buf.extend_from_slice(&CLASS_IN.to_be_bytes());
        for (rtype, rdata) in answers {
            // Name: compression pointer back to the question name at offset 12.
            buf.extend_from_slice(&[0xC0, 0x0C]);
            buf.extend_from_slice(&rtype.to_be_bytes());
            buf.extend_from_slice(&CLASS_IN.to_be_bytes());
            buf.extend_from_slice(&300u32.to_be_bytes());
            buf.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
            buf.extend_from_slice(rdata);
        }
        buf
    }

    #[test]
    fn encode_name_builds_labels() {
        let mut buf = Vec::new();
        encode_name("a.bc.", &mut buf).unwrap();
        assert_eq!(buf, vec![1, b'a', 2, b'b', b'c', 0]);
    }

    #[test]
    fn encode_name_rejects_empty_labels() {
        let mut buf = Vec::new();
        assert!(encode_name("a..b", &mut buf).is_err());
        assert!(encode_name("", &mut buf).is_err());
    }

    #[test]
    fn parses_txt_answer() {
        let txt = [&b"\x05hello"[..], &b"\x06 world"[..]].concat();
        let msg = response(0x1234, 0x8180, "example.com", TYPE_TXT, &[(TYPE_TXT, txt)]);
        let records = parse_response(&msg, 0x1234, TYPE_TXT).unwrap();
        assert_eq!(records, vec![RData::Txt("hello world".to_string())]);
    }

    #[test]
    fn parses_a_and_mx_answers() {
        let msg = response(
            7,
            0x8180,
            "example.com",
            TYPE_A,
            &[(TYPE_A, vec![192, 0, 2, 10])],
        );
        assert_eq!(
            parse_response(&msg, 7, TYPE_A).unwrap(),
            vec![RData::A(Ipv4Addr::new(192, 0, 2, 10))]
        );

        // MX exchange written as "mail" followed by a pointer to the question name.
        let mx = vec![0, 10, 4, b'm', b'a', b'i', b'l', 0xC0, 0x0C];
        let msg = response(8, 0x8180, "example.com", TYPE_MX, &[(TYPE_MX, mx)]);
        assert_eq!(
            parse_response(&msg, 8, TYPE_MX).unwrap(),
            vec![RData::Mx {
                preference: 10,
                exchange: "mail.example.com".to_string()
            }]
        );
    }

    #[test]
    fn nxdomain_is_empty_not_error() {
        let msg = response(9, 0x8183, "missing.example", TYPE_TXT, &[]);
        assert_eq!(parse_response(&msg, 9, TYPE_TXT).unwrap(), Vec::new());
    }

    #[test]
    fn rejects_id_mismatch() {
        let msg = response(1, 0x8180, "example.com", TYPE_TXT, &[]);
        assert!(matches!(
            parse_response(&msg, 2, TYPE_TXT),
            Err(DnsError::Malformed(_))
        ));
    }

    #[test]
    fn server_failure_is_reported() {
        let msg = response(3, 0x8182, "example.com", TYPE_TXT, &[]);
        assert_eq!(
            parse_response(&msg, 3, TYPE_TXT),
            Err(DnsError::ServerFailure(2))
        );
    }

    #[test]
    fn compression_loop_is_bounded() {
        // A pointer that points to itself must not hang the parser.
        let mut msg = vec![0u8; HEADER_LEN];
        msg.extend_from_slice(&[0xC0, 0x0C]);
        assert!(read_name(&msg, HEADER_LEN).is_err());
    }
}
