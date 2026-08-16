//! DNS-SD advertisement for the HTTP server.
//!
//! The reference uses `bonjour-service` to publish an `http` service named
//! `opencode-{port}`, with host `opencode.local` and TXT `path=/`. The default
//! Rust build provides the small responder needed for that contract without a
//! native or registry dependency. Builds with `--no-default-features` retain a
//! deliberate no-op boundary for environments where multicast is unavailable.

#[cfg(feature = "mdns")]
mod implementation {
    use std::fmt;
    use std::io;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock, Weak};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    const MDNS_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353);
    const SERVICE_TYPE: &str = "_http._tcp.local.";
    const ENUMERATION_NAME: &str = "_services._dns-sd._udp.local.";
    const DEFAULT_HOST: &str = "opencode.local";
    const TTL: u32 = 120;

    #[derive(Clone)]
    pub struct Advertisement {
        inner: Arc<AdvertisementInner>,
    }

    impl fmt::Debug for Advertisement {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("Advertisement")
                .field("port", &self.inner.port)
                .field("host", &self.inner.host)
                .finish()
        }
    }

    struct AdvertisementInner {
        port: u16,
        service_name: String,
        host: String,
        local_ipv4: Option<Ipv4Addr>,
        socket: UdpSocket,
        stopped: AtomicBool,
        worker: Mutex<Option<JoinHandle<()>>>,
    }

    impl Advertisement {
        /// Stop answering mDNS queries and send a DNS-SD goodbye packet.
        pub fn unpublish(&self) {
            if self.inner.stopped.swap(true, Ordering::AcqRel) {
                return;
            }

            let _ = send_packet(&self.inner.socket, &self.inner, 0);
            if let Some(worker) = self.inner.worker.lock().unwrap().take() {
                let _ = worker.join();
            }

            let clear_current = current_advertisement()
                .lock()
                .unwrap()
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|current| Arc::ptr_eq(&current, &self.inner));
            if clear_current {
                *current_advertisement().lock().unwrap() = None;
            }
        }
    }

    impl Drop for Advertisement {
        fn drop(&mut self) {
            self.unpublish();
        }
    }

    /// Publish an HTTP DNS-SD service, swallowing setup failures like the
    /// reference implementation. The caller owns the returned guard.
    pub fn publish(port: u16, domain: Option<&str>) -> Option<Advertisement> {
        let host = normalize_name(domain.unwrap_or(DEFAULT_HOST))?;
        let service_name = format!("opencode-{port}.{SERVICE_TYPE}");

        let previous = {
            let mut current = current_advertisement().lock().unwrap();
            current.take().and_then(|weak| weak.upgrade())
        };
        if let Some(previous) = previous {
            if previous.port == port && !previous.stopped.load(Ordering::Acquire) {
                let advertisement = Advertisement { inner: previous };
                *current_advertisement().lock().unwrap() =
                    Some(Arc::downgrade(&advertisement.inner));
                return Some(advertisement);
            }
            Advertisement { inner: previous }.unpublish();
        }

        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 5353)).ok()?;
        socket
            .set_read_timeout(Some(Duration::from_millis(250)))
            .ok()?;
        socket.set_multicast_loop_v4(true).ok()?;
        socket.set_multicast_ttl_v4(255).ok()?;
        let local_ipv4 = local_ipv4();
        socket
            .join_multicast_v4(
                &Ipv4Addr::new(224, 0, 0, 251),
                &local_ipv4.unwrap_or(Ipv4Addr::UNSPECIFIED),
            )
            .ok()?;

        let inner = Arc::new(AdvertisementInner {
            port,
            service_name,
            host,
            local_ipv4,
            socket,
            stopped: AtomicBool::new(false),
            worker: Mutex::new(None),
        });
        let worker_inner = Arc::clone(&inner);
        let worker = thread::Builder::new()
            .name("opencode-mdns".to_string())
            .spawn(move || responder_loop(worker_inner))
            .ok()?;
        *inner.worker.lock().unwrap() = Some(worker);
        *current_advertisement().lock().unwrap() = Some(Arc::downgrade(&inner));

        let advertisement = Advertisement { inner };
        let _ = send_packet(&advertisement.inner.socket, &advertisement.inner, TTL);
        Some(advertisement)
    }

    /// Remove the process-wide advertisement, if one exists.
    pub fn unpublish() {
        let current = current_advertisement()
            .lock()
            .unwrap()
            .take()
            .and_then(|weak| weak.upgrade());
        if let Some(current) = current {
            Advertisement { inner: current }.unpublish();
        }
    }

    fn current_advertisement() -> &'static Mutex<Option<Weak<AdvertisementInner>>> {
        static CURRENT: OnceLock<Mutex<Option<Weak<AdvertisementInner>>>> = OnceLock::new();
        CURRENT.get_or_init(|| Mutex::new(None))
    }

    fn responder_loop(advertisement: Arc<AdvertisementInner>) {
        let mut query = [0_u8; 1500];
        while !advertisement.stopped.load(Ordering::Acquire) {
            match advertisement.socket.recv_from(&mut query) {
                Ok((length, _)) => {
                    if let Some(response) = build_response(&query[..length], &advertisement) {
                        let _ = advertisement.socket.send_to(&response, MDNS_ADDR);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
    }

    fn send_packet(
        socket: &UdpSocket,
        advertisement: &AdvertisementInner,
        ttl: u32,
    ) -> io::Result<()> {
        let packet = build_announcement(advertisement, ttl).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid mDNS service name")
        })?;
        socket.send_to(&packet, MDNS_ADDR).map(|_| ())
    }

    fn build_announcement(advertisement: &AdvertisementInner, ttl: u32) -> Option<Vec<u8>> {
        let records = records_for_service(advertisement, ttl);
        build_dns_response(0, &[], &records)
    }

    fn build_response(query: &[u8], advertisement: &AdvertisementInner) -> Option<Vec<u8>> {
        if query.len() < 12 || query[2] & 0x80 != 0 {
            return None;
        }
        let question_count = u16::from_be_bytes([query[4], query[5]]) as usize;
        let mut offset = 12;
        let mut wants_service = false;
        let mut wants_type = false;
        let mut wants_enumeration = false;
        for _ in 0..question_count {
            let (name, next) = read_name(query, offset)?;
            offset = next.checked_add(4)?;
            if offset > query.len() {
                return None;
            }
            let qtype = u16::from_be_bytes([query[next], query[next + 1]]);
            let qclass = u16::from_be_bytes([query[next + 2], query[next + 3]]) & 0x7fff;
            if qclass != 1 && qclass != 255 {
                continue;
            }
            match name.as_str() {
                ENUMERATION_NAME => wants_enumeration = true,
                SERVICE_TYPE => wants_type = qtype == 12 || qtype == 255,
                name if name == advertisement.service_name => {
                    wants_service = matches!(qtype, 16 | 33 | 255)
                }
                name if name == advertisement.host => {
                    wants_service = qtype == 1 || qtype == 255;
                }
                _ => {}
            }
        }
        if !wants_enumeration && !wants_type && !wants_service {
            return None;
        }

        let mut records = Vec::new();
        if wants_enumeration {
            records.push(Record::ptr(ENUMERATION_NAME, SERVICE_TYPE, TTL));
        }
        if wants_type {
            records.extend(records_for_service(advertisement, TTL));
        } else if wants_service {
            records.extend(records_for_service(advertisement, TTL));
        }
        build_dns_response(0, &[], &records)
    }

    fn records_for_service(advertisement: &AdvertisementInner, ttl: u32) -> Vec<Record> {
        let mut records = vec![Record::ptr(SERVICE_TYPE, &advertisement.service_name, ttl)];
        records.push(Record::srv(
            &advertisement.service_name,
            &advertisement.host,
            advertisement.port,
            ttl,
        ));
        records.push(Record::txt(&advertisement.service_name, b"path=/", ttl));
        if let Some(address) = advertisement.local_ipv4 {
            records.push(Record::a(&advertisement.host, address, ttl));
        }
        records
    }

    #[derive(Debug)]
    enum Record {
        Ptr {
            name: String,
            target: String,
            ttl: u32,
        },
        Srv {
            name: String,
            host: String,
            port: u16,
            ttl: u32,
        },
        Txt {
            name: String,
            value: Vec<u8>,
            ttl: u32,
        },
        A {
            name: String,
            address: Ipv4Addr,
            ttl: u32,
        },
    }

    impl Record {
        fn ptr(name: &str, target: &str, ttl: u32) -> Self {
            Self::Ptr {
                name: name.to_string(),
                target: target.to_string(),
                ttl,
            }
        }

        fn srv(name: &str, host: &str, port: u16, ttl: u32) -> Self {
            Self::Srv {
                name: name.to_string(),
                host: host.to_string(),
                port,
                ttl,
            }
        }

        fn txt(name: &str, value: &[u8], ttl: u32) -> Self {
            Self::Txt {
                name: name.to_string(),
                value: value.to_vec(),
                ttl,
            }
        }

        fn a(name: &str, address: Ipv4Addr, ttl: u32) -> Self {
            Self::A {
                name: name.to_string(),
                address,
                ttl,
            }
        }
    }

    fn build_dns_response(id: u16, questions: &[u8], records: &[Record]) -> Option<Vec<u8>> {
        let mut packet = Vec::with_capacity(512);
        packet.extend_from_slice(&id.to_be_bytes());
        packet.extend_from_slice(&0x8400_u16.to_be_bytes());
        packet.extend_from_slice(&(if questions.is_empty() { 0 } else { 1_u16 }).to_be_bytes());
        packet.extend_from_slice(&(records.len() as u16).to_be_bytes());
        packet.extend_from_slice(&0_u16.to_be_bytes());
        packet.extend_from_slice(&0_u16.to_be_bytes());
        packet.extend_from_slice(questions);
        for record in records {
            append_record(&mut packet, record)?;
        }
        Some(packet)
    }

    fn append_record(packet: &mut Vec<u8>, record: &Record) -> Option<()> {
        let (name, kind, ttl) = match record {
            Record::Ptr { name, ttl, .. } => (name, 12_u16, *ttl),
            Record::Srv { name, ttl, .. } => (name, 33_u16, *ttl),
            Record::Txt { name, ttl, .. } => (name, 16_u16, *ttl),
            Record::A { name, ttl, .. } => (name, 1_u16, *ttl),
        };
        append_name(packet, name)?;
        packet.extend_from_slice(&kind.to_be_bytes());
        packet.extend_from_slice(&(if kind == 12 { 1_u16 } else { 0x8001_u16 }).to_be_bytes());
        packet.extend_from_slice(&ttl.to_be_bytes());
        let length_offset = packet.len();
        packet.extend_from_slice(&0_u16.to_be_bytes());
        let data_start = packet.len();
        match record {
            Record::Ptr { target, .. } => append_name(packet, target)?,
            Record::Srv { host, port, .. } => {
                packet.extend_from_slice(&0_u16.to_be_bytes());
                packet.extend_from_slice(&0_u16.to_be_bytes());
                packet.extend_from_slice(&port.to_be_bytes());
                append_name(packet, host)?;
            }
            Record::Txt { value, .. } => {
                if value.len() > 255 {
                    return None;
                }
                packet.push(value.len() as u8);
                packet.extend_from_slice(value);
            }
            Record::A { address, .. } => packet.extend_from_slice(&address.octets()),
        }
        let length = u16::try_from(packet.len() - data_start).ok()?;
        packet[length_offset..length_offset + 2].copy_from_slice(&length.to_be_bytes());
        Some(())
    }

    fn read_name(packet: &[u8], start: usize) -> Option<(String, usize)> {
        let mut position = start;
        let mut next = start;
        let mut jumped = false;
        let mut labels = Vec::new();
        for _ in 0..packet.len() {
            let length = *packet.get(position)?;
            if length == 0 {
                if !jumped {
                    next = position + 1;
                }
                return Some((format!("{}.", labels.join(".")).to_ascii_lowercase(), next));
            }
            if length & 0xc0 == 0xc0 {
                let pointer =
                    (((length as usize) & 0x3f) << 8) | (*packet.get(position + 1)? as usize);
                if !jumped {
                    next = position + 2;
                    jumped = true;
                }
                position = pointer;
                continue;
            }
            if length & 0xc0 != 0 || length > 63 {
                return None;
            }
            let begin = position + 1;
            let end = begin.checked_add(length as usize)?;
            labels.push(std::str::from_utf8(packet.get(begin..end)?).ok()?);
            position = end;
        }
        None
    }

    fn append_name(packet: &mut Vec<u8>, name: &str) -> Option<()> {
        let name = normalize_name(name)?;
        for label in name.trim_end_matches('.').split('.') {
            let bytes = label.as_bytes();
            if bytes.is_empty() || bytes.len() > 63 {
                return None;
            }
            packet.push(bytes.len() as u8);
            packet.extend_from_slice(bytes);
        }
        packet.push(0);
        Some(())
    }

    fn normalize_name(name: &str) -> Option<String> {
        let name = name.trim().trim_end_matches('.');
        if name.is_empty() || name.len() > 253 || name.contains("..") {
            return None;
        }
        let normalized = format!("{}.", name.to_ascii_lowercase());
        if normalized
            .trim_end_matches('.')
            .split('.')
            .any(|label| label.is_empty() || label.len() > 63)
        {
            return None;
        }
        Some(normalized)
    }

    fn local_ipv4() -> Option<Ipv4Addr> {
        let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        probe.connect(MDNS_ADDR).ok()?;
        match probe.local_addr().ok()?.ip() {
            std::net::IpAddr::V4(address) if !address.is_unspecified() => Some(address),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn query(name: &str, qtype: u16) -> Vec<u8> {
            let mut packet = vec![0; 12];
            packet[4..6].copy_from_slice(&1_u16.to_be_bytes());
            packet.extend_from_slice(&name_to_bytes(name));
            packet.extend_from_slice(&qtype.to_be_bytes());
            packet.extend_from_slice(&1_u16.to_be_bytes());
            packet
        }

        fn name_to_bytes(name: &str) -> Vec<u8> {
            let mut bytes = Vec::new();
            append_name(&mut bytes, name).unwrap();
            bytes
        }

        fn test_advertisement() -> AdvertisementInner {
            AdvertisementInner {
                port: 4096,
                service_name: "opencode-4096._http._tcp.local.".into(),
                host: "opencode.local.".into(),
                local_ipv4: Some(Ipv4Addr::new(192, 168, 1, 10)),
                socket: UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap(),
                stopped: AtomicBool::new(false),
                worker: Mutex::new(None),
            }
        }

        #[test]
        fn normalizes_and_validates_dns_names() {
            assert_eq!(
                normalize_name(" opencode.local. "),
                Some("opencode.local.".into())
            );
            assert!(normalize_name("opencode..local").is_none());
            assert!(normalize_name(&"a".repeat(64)).is_none());
        }

        #[test]
        fn answers_service_browse_with_dns_sd_records() {
            let advertisement = test_advertisement();
            let response = build_response(&query(SERVICE_TYPE, 12), &advertisement).unwrap();
            assert_eq!(&response[2..4], &0x8400_u16.to_be_bytes());
            assert_eq!(u16::from_be_bytes([response[6], response[7]]), 4);
            assert!(response
                .windows(b"opencode-4096".len())
                .any(|window| window == b"opencode-4096"));
            assert!(response
                .windows(b"path=/".len())
                .any(|window| window == b"path=/"));
        }

        #[test]
        fn ignores_unrelated_queries() {
            let advertisement = test_advertisement();
            assert!(build_response(&query("_ssh._tcp.local.", 12), &advertisement).is_none());
        }
    }
}

#[cfg(not(feature = "mdns"))]
mod implementation {
    use std::fmt;

    /// No-op marker used when the optional multicast feature is disabled.
    #[derive(Clone, Debug)]
    pub struct Advertisement;

    impl Advertisement {
        pub fn unpublish(&self) {}
    }

    pub fn publish(_port: u16, _domain: Option<&str>) -> Option<Advertisement> {
        None
    }

    pub fn unpublish() {}

    impl fmt::Display for Advertisement {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("disabled")
        }
    }
}

pub use implementation::{publish, unpublish, Advertisement};
