//! smoltcp tabanlı asgari ağ yığını: DHCP ile IP al, DNS ile çöz, TCP ile
//! HTTP/1.0 GET yap. Hepsi yoklamalı (kesmesiz) tek bir bloklayan akışta.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::dns::{self, GetQueryResultError};
use smoltcp::socket::{dhcpv4, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    DnsQueryType, EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address,
};

use alloc::sync::Arc;

use rustls::client::{ClientConnectionData, UnbufferedClientConnection};
use rustls::pki_types::ServerName;
use rustls::unbuffered::{
    ConnectionState, EncodeError, EncryptError, UnbufferedStatus, WriteTraffic,
};
use rustls::version::TLS13;
use rustls::{ClientConfig, RootCertStore};

/// e1000'i smoltcp'nin `Device` arayüzüne bağlayan ince katman.
struct E1000Phy;

struct RxTok(Vec<u8>);
struct TxTok;

impl Device for E1000Phy {
    type RxToken<'a> = RxTok;
    type TxToken<'a> = TxTok;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps
    }

    fn receive(&mut self, _t: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut buf = [0u8; 2048];
        crate::e1000::recv(&mut buf).map(|n| (RxTok(buf[..n].to_vec()), TxTok))
    }

    fn transmit(&mut self, _t: Instant) -> Option<Self::TxToken<'_>> {
        Some(TxTok)
    }
}

impl RxToken for RxTok {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl TxToken for TxTok {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0u8; len];
        let r = f(&mut buf);
        crate::e1000::send(&buf);
        r
    }
}

fn now() -> Instant {
    Instant::from_millis(crate::time::millis() as i64)
}

/// DHCP ile IP/gateway/DNS alıp hazır bir arayüz/soket kümesi döndürür.
fn setup() -> Result<(Interface, E1000Phy, SocketSet<'static>, Ipv4Address), &'static str> {
    if !crate::e1000::ready() {
        return Err("ağ kartı yok (e1000)");
    }

    let mac = crate::e1000::mac();
    let mut device = E1000Phy;

    let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
    config.random_seed = crate::time::millis().wrapping_mul(2654435761).wrapping_add(1);
    let mut iface = Interface::new(config, &mut device, now());

    let mut sockets = SocketSet::new(vec![]);

    let dhcp = dhcpv4::Socket::new();
    let dhcp_h = sockets.add(dhcp);

    let mut dns_server: Option<Ipv4Address> = None;
    let deadline = crate::time::millis() + 8000;
    let mut configured = false;
    while crate::time::millis() < deadline {
        iface.poll(now(), &mut device, &mut sockets);
        let event = sockets.get_mut::<dhcpv4::Socket>(dhcp_h).poll();
        if let Some(dhcpv4::Event::Configured(cfg)) = event {
            iface.update_ip_addrs(|a| {
                a.clear();
                let _ = a.push(IpCidr::Ipv4(cfg.address));
            });
            if let Some(router) = cfg.router {
                let _ = iface.routes_mut().add_default_ipv4_route(router);
            }
            dns_server = cfg.dns_servers.first().copied();
            configured = true;
            break;
        }
        core::hint::spin_loop();
    }
    if !configured {
        return Err("DHCP başarısız (IP alınamadı)");
    }
    sockets.remove(dhcp_h);

    let server = dns_server.unwrap_or(Ipv4Address::new(8, 8, 8, 8));
    Ok((iface, device, sockets, server))
}

/// Tam bir URL'yi (şema dahil) ayrıştırıp HTTP veya HTTPS ile çeker.
/// `http://`/`https://` ön ekini, host ve yolu ayırır; varsayılan yol `/`.
/// Kullanıcı alanındaki `SYS_FETCH` sistem çağrısı bunu kullanır.
pub fn fetch_url(raw: &str) -> Result<String, &'static str> {
    let secure = raw.starts_with("https://");
    let url = raw
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let (host, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };
    if secure {
        fetch_https(host, path)
    } else {
        fetch(host, path)
    }
}

/// `host` için `path`'i HTTP/1.0 ile çeker. Tüm yanıtı (başlıklar dahil) döndürür.
pub fn fetch(host: &str, path: &str) -> Result<String, &'static str> {
    let (mut iface, mut device, mut sockets, server) = setup()?;

    let ip = match parse_ipv4(host) {
        Some(ip) => IpAddress::Ipv4(ip),
        None => resolve(&mut iface, &mut device, &mut sockets, server, host)?,
    };

    http_get(&mut iface, &mut device, &mut sockets, ip, host, path)
}

fn resolve(
    iface: &mut Interface,
    device: &mut E1000Phy,
    sockets: &mut SocketSet,
    server: Ipv4Address,
    host: &str,
) -> Result<IpAddress, &'static str> {
    let servers = [IpAddress::Ipv4(server)];
    let dns_sock = dns::Socket::new(&servers, vec![]);
    let h = sockets.add(dns_sock);

    let query = sockets
        .get_mut::<dns::Socket>(h)
        .start_query(iface.context(), host, DnsQueryType::A)
        .map_err(|_| "DNS sorgusu başlatılamadı")?;

    let deadline = crate::time::millis() + 8000;
    let result = loop {
        if crate::time::millis() >= deadline {
            break Err("DNS zaman aşımı");
        }
        iface.poll(now(), device, sockets);
        match sockets.get_mut::<dns::Socket>(h).get_query_result(query) {
            Ok(addrs) => {
                break addrs
                    .iter()
                    .copied()
                    .find(|a| matches!(a, IpAddress::Ipv4(_)))
                    .ok_or("DNS: IPv4 adresi yok");
            }
            Err(GetQueryResultError::Pending) => {}
            Err(_) => break Err("DNS sorgusu başarısız"),
        }
        core::hint::spin_loop();
    };
    sockets.remove(h);
    result
}

fn http_get(
    iface: &mut Interface,
    device: &mut E1000Phy,
    sockets: &mut SocketSet,
    ip: IpAddress,
    host: &str,
    path: &str,
) -> Result<String, &'static str> {
    let rx = tcp::SocketBuffer::new(vec![0u8; 8192]);
    let tx = tcp::SocketBuffer::new(vec![0u8; 2048]);
    let h = sockets.add(tcp::Socket::new(rx, tx));

    let local_port = 49152 + (crate::time::millis() as u16 & 0x3FFF);
    sockets
        .get_mut::<tcp::Socket>(h)
        .connect(iface.context(), (ip, 80), local_port)
        .map_err(|_| "TCP bağlanılamadı")?;

    let mut out: Vec<u8> = Vec::new();
    let mut request_sent = false;
    let mut was_active = false;
    let deadline = crate::time::millis() + 15000;

    let res = loop {
        if crate::time::millis() >= deadline {
            break Err("HTTP zaman aşımı");
        }
        iface.poll(now(), device, sockets);
        let sock = sockets.get_mut::<tcp::Socket>(h);

        if sock.is_active() {
            was_active = true;
        } else if was_active {
            break Ok(()); // sunucu bağlantıyı kapattı
        }

        if !request_sent && sock.can_send() {
            let mut req = String::new();
            req.push_str("GET ");
            req.push_str(path);
            req.push_str(" HTTP/1.0\r\nHost: ");
            req.push_str(host);
            req.push_str("\r\nUser-Agent: MinOS/0.1\r\nConnection: close\r\n\r\n");
            let _ = sock.send_slice(req.as_bytes());
            request_sent = true;
        }

        if sock.can_recv() {
            let _ = sock.recv(|data| {
                let take = core::cmp::min(data.len(), (64 * 1024usize).saturating_sub(out.len()));
                out.extend_from_slice(&data[..take]);
                (data.len(), ())
            });
            if out.len() >= 64 * 1024 {
                break Ok(());
            }
        }

        // Karşı taraf veriyi bitirip bağlantıyı kapattıysa (FIN) çık.
        if request_sent && was_active && !sock.may_recv() {
            break Ok(());
        }
        core::hint::spin_loop();
    };

    sockets.get_mut::<tcp::Socket>(h).close();
    iface.poll(now(), device, sockets);
    sockets.remove(h);

    res?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

// ---------------------------------------------------------------------------
// HTTPS — rustls (TLS 1.3) + RustCrypto sağlayıcı + webpki-roots doğrulaması
// ---------------------------------------------------------------------------

/// `host` için `path`'i HTTPS ile çeker. Sertifika, gömülü Mozilla kök
/// sertifikalarıyla (webpki-roots) DOĞRULANIR. rustls'in bloklamasız
/// (unbuffered) API'si smoltcp TCP soketi üzerinde elle sürülür.
pub fn fetch_https(host: &str, path: &str) -> Result<String, &'static str> {
    let (mut iface, mut device, mut sockets, server) = setup()?;

    let ip = match parse_ipv4(host) {
        Some(ip) => IpAddress::Ipv4(ip),
        None => resolve(&mut iface, &mut device, &mut sockets, server, host)?,
    };
    let handle = connect_tcp(&mut iface, &mut sockets, ip, 443)?;

    // Kök sertifikalar (Mozilla) — gerçek sunucu kimlik doğrulaması için.
    let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = ClientConfig::builder_with_details(
        Arc::new(rustls_rustcrypto::provider()),
        Arc::new(RtcTime),
    )
    .with_protocol_versions(&[&TLS13])
    .map_err(|_| "TLS yapılandırma hatası")?
    .with_root_certificates(root_store)
    .with_no_client_auth();

    let server_name: ServerName<'static> = ServerName::try_from(host)
        .map_err(|_| "geçersiz sunucu adı")?
        .to_owned();
    let mut conn = UnbufferedClientConnection::new(Arc::new(config), server_name)
        .map_err(|_| "TLS bağlantı kurulamadı")?;

    let request = {
        let mut r = String::new();
        r.push_str("GET ");
        r.push_str(path);
        r.push_str(" HTTP/1.0\r\nHost: ");
        r.push_str(host);
        r.push_str("\r\nUser-Agent: MinOS/0.1\r\nConnection: close\r\n\r\n");
        r
    };

    let mut incoming = vec![0u8; 32 * 1024];
    let mut outgoing = vec![0u8; 8 * 1024];
    let mut in_used = 0usize;
    let mut out_used = 0usize;
    let mut response: Vec<u8> = Vec::new();
    let mut sent_request = false;

    let deadline = crate::time::millis() + 30000;
    'outer: loop {
        if crate::time::millis() > deadline {
            return Err("TLS zaman aşımı");
        }

        let UnbufferedStatus { mut discard, state } =
            conn.process_tls_records(&mut incoming[..in_used]);

        match state.map_err(rustls_err)? {
            ConnectionState::ReadTraffic(mut st) => {
                while let Some(res) = st.next_record() {
                    let rec = res.map_err(|_| "TLS kayıt çözme hatası")?;
                    discard += rec.discard;
                    let take = core::cmp::min(
                        rec.payload.len(),
                        (256 * 1024usize).saturating_sub(response.len()),
                    );
                    response.extend_from_slice(&rec.payload[..take]);
                }
                if response.len() >= 256 * 1024 {
                    break 'outer;
                }
            }

            ConnectionState::EncodeTlsData(mut st) => loop {
                match st.encode(&mut outgoing[out_used..]) {
                    Ok(n) => {
                        out_used += n;
                        break;
                    }
                    Err(EncodeError::InsufficientSize(is)) => {
                        let new_len = out_used + is.required_size;
                        outgoing.resize(new_len, 0);
                    }
                    Err(_) => return Err("TLS kodlama hatası"),
                }
            },

            ConnectionState::TransmitTlsData(mut st) => {
                if let Some(mut enc) = st.may_encrypt_app_data() {
                    if !sent_request {
                        out_used += encrypt_into(&mut enc, request.as_bytes(), &mut outgoing, out_used)?;
                        sent_request = true;
                    }
                }
                send_tls(&mut iface, &mut device, &mut sockets, handle, &outgoing[..out_used])?;
                out_used = 0;
                st.done();
            }

            ConnectionState::BlockedHandshake { .. } => {
                let n = recv_tls(&mut iface, &mut device, &mut sockets, handle, &mut incoming[in_used..])?;
                if n == 0 {
                    return Err("TLS: el sıkışma sırasında bağlantı koptu");
                }
                in_used += n;
            }

            ConnectionState::WriteTraffic(mut enc) => {
                if !sent_request {
                    out_used += encrypt_into(&mut enc, request.as_bytes(), &mut outgoing, out_used)?;
                    sent_request = true;
                    send_tls(&mut iface, &mut device, &mut sockets, handle, &outgoing[..out_used])?;
                    out_used = 0;
                }
                let n = recv_tls(&mut iface, &mut device, &mut sockets, handle, &mut incoming[in_used..])?;
                if n == 0 {
                    break 'outer; // karşı taraf kapattı; yanıt tamam
                }
                in_used += n;
            }

            ConnectionState::PeerClosed | ConnectionState::Closed => break 'outer,
            _ => break 'outer,
        }

        if discard != 0 {
            incoming.copy_within(discard..in_used, 0);
            in_used -= discard;
        }
    }

    Ok(String::from_utf8_lossy(&response).into_owned())
}

/// Uygulama verisini `outgoing[out_used..]`'e şifreler; tampon küçükse büyütür.
fn encrypt_into(
    enc: &mut WriteTraffic<'_, ClientConnectionData>,
    plaintext: &[u8],
    outgoing: &mut Vec<u8>,
    out_used: usize,
) -> Result<usize, &'static str> {
    loop {
        match enc.encrypt(plaintext, &mut outgoing[out_used..]) {
            Ok(n) => return Ok(n),
            Err(EncryptError::InsufficientSize(is)) => {
                let new_len = out_used + is.required_size;
                outgoing.resize(new_len, 0);
            }
            Err(_) => return Err("TLS şifreleme hatası"),
        }
    }
}

/// rustls hatasını kullanıcıya anlaşılır bir mesaja çevirir (ayrıntı COM1'e).
fn rustls_err(e: rustls::Error) -> &'static str {
    crate::serial::write_str("[tls] ");
    crate::serial::write_str(&alloc::format!("{e:?}\n"));
    match e {
        rustls::Error::InvalidCertificate(_) => "TLS: sertifika doğrulanamadı (kök/zincir/tarih)",
        rustls::Error::PeerIncompatible(_) => "TLS: sunucu uyumsuz",
        rustls::Error::PeerMisbehaved(_) => "TLS: sunucu hatalı davrandı",
        _ => "TLS el sıkışma başarısız",
    }
}

/// outgoing tamponunu smoltcp soketi üzerinden tamamen gönderir.
fn send_tls(
    iface: &mut Interface,
    device: &mut E1000Phy,
    sockets: &mut SocketSet,
    handle: SocketHandle,
    data: &[u8],
) -> Result<(), &'static str> {
    let mut sent = 0;
    let deadline = crate::time::millis() + 20000;
    while sent < data.len() {
        iface.poll(now(), device, sockets);
        let s = sockets.get_mut::<tcp::Socket>(handle);
        if s.can_send() {
            match s.send_slice(&data[sent..]) {
                Ok(n) => sent += n,
                Err(_) => return Err("TCP gönderim hatası"),
            }
        }
        if s.state() == tcp::State::Closed {
            return Err("TCP kapandı");
        }
        if crate::time::millis() >= deadline {
            return Err("TCP gönderim zaman aşımı");
        }
        core::hint::spin_loop();
    }
    iface.poll(now(), device, sockets);
    Ok(())
}

/// Sokete gelen TLS baytlarını `buf`'a okur. 0 = karşı taraf kapattı (EOF).
fn recv_tls(
    iface: &mut Interface,
    device: &mut E1000Phy,
    sockets: &mut SocketSet,
    handle: SocketHandle,
    buf: &mut [u8],
) -> Result<usize, &'static str> {
    if buf.is_empty() {
        return Err("TLS: gelen tampon doldu");
    }
    let deadline = crate::time::millis() + 20000;
    loop {
        iface.poll(now(), device, sockets);
        let s = sockets.get_mut::<tcp::Socket>(handle);
        if s.can_recv() {
            if let Ok(n) = s.recv_slice(buf) {
                if n > 0 {
                    return Ok(n);
                }
            }
        }
        let st = s.state();
        if st == tcp::State::Closed {
            return Ok(0);
        }
        if st != tcp::State::SynSent && st != tcp::State::SynReceived && !s.may_recv() {
            return Ok(0);
        }
        if crate::time::millis() >= deadline {
            return Err("TCP alım zaman aşımı");
        }
        core::hint::spin_loop();
    }
}

/// Yeni bir TCP soketi açıp `ip:port`'a bağlanır, handle döndürür.
fn connect_tcp(
    iface: &mut Interface,
    sockets: &mut SocketSet,
    ip: IpAddress,
    port: u16,
) -> Result<SocketHandle, &'static str> {
    let rx = tcp::SocketBuffer::new(vec![0u8; 16384]);
    let tx = tcp::SocketBuffer::new(vec![0u8; 8192]);
    let h = sockets.add(tcp::Socket::new(rx, tx));

    let local_port = 49152 + (crate::time::millis() as u16 & 0x3FFF);
    sockets
        .get_mut::<tcp::Socket>(h)
        .connect(iface.context(), (ip, port), local_port)
        .map_err(|_| "TCP bağlanılamadı")?;
    Ok(h)
}

// --- rustls için zaman sağlayıcı: CMOS RTC -> Unix zaman ---
// Sertifika geçerlilik (notBefore/notAfter) kontrolü için gereklidir.

#[derive(Debug)]
struct RtcTime;

impl rustls::time_provider::TimeProvider for RtcTime {
    fn current_time(&self) -> Option<rustls::pki_types::UnixTime> {
        let dt = crate::rtc::now();
        Some(rustls::pki_types::UnixTime::since_unix_epoch(
            core::time::Duration::from_secs(unix_secs(&dt)),
        ))
    }
}

/// Y/A/G S:D:S -> 1970'ten beri saniye (proleptik Gregoryen, Howard Hinnant).
fn unix_secs(dt: &crate::rtc::DateTime) -> u64 {
    let y = if dt.month <= 2 {
        dt.year as i64 - 1
    } else {
        dt.year as i64
    };
    let m = dt.month as i64;
    let d = dt.day as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + dt.hour as i64 * 3600 + dt.min as i64 * 60 + dt.sec as i64;
    if secs < 0 {
        0
    } else {
        secs as u64
    }
}

// --- getrandom 0.2 "custom" backend: çekirdeğin donanım RNG'sine bağlanır ---
// rustls-rustcrypto'nun SecureRandom'u rand_core::OsRng -> getrandom -> buraya
// düşer. register_custom_getrandom! makrosu gerekli sembolü üretir.
fn kernel_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    HwRng::new().fill_bytes(buf);
    Ok(())
}
getrandom::register_custom_getrandom!(kernel_getrandom);

// --- Rastgelelik kaynağı: RDRAND (varsa) + TSC ile tohumlanmış xorshift ---

struct HwRng {
    state: u64,
    rdrand: bool,
}

impl HwRng {
    fn new() -> Self {
        let seed = rdtsc() ^ 0x9E37_79B9_7F4A_7C15;
        HwRng {
            state: seed | 1,
            rdrand: has_rdrand(),
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        let mut r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        if self.rdrand {
            if let (Some(a), Some(b)) = (rdrand32(), rdrand32()) {
                r ^= ((a as u64) << 32) | (b as u64);
            }
        }
        r
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut i = 0;
        while i < dest.len() {
            let v = self.next_u64().to_le_bytes();
            let n = core::cmp::min(8, dest.len() - i);
            dest[i..i + n].copy_from_slice(&v[..n]);
            i += n;
        }
    }
}

fn rdtsc() -> u64 {
    let hi: u32;
    let lo: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | (lo as u64)
}

fn has_rdrand() -> bool {
    let r = core::arch::x86::__cpuid(1);
    (r.ecx >> 30) & 1 == 1
}

fn rdrand32() -> Option<u32> {
    let val: u32;
    let ok: u8;
    unsafe {
        core::arch::asm!(
            "rdrand {v:e}",
            "setc {s}",
            v = out(reg) val,
            s = out(reg_byte) ok,
            options(nomem, nostack),
        );
    }
    if ok != 0 {
        Some(val)
    } else {
        None
    }
}

fn parse_ipv4(s: &str) -> Option<Ipv4Address> {
    let mut parts = [0u8; 4];
    let mut i = 0;
    for p in s.split('.') {
        if i >= 4 {
            return None;
        }
        parts[i] = p.parse::<u8>().ok()?;
        i += 1;
    }
    if i == 4 {
        Some(Ipv4Address::new(parts[0], parts[1], parts[2], parts[3]))
    } else {
        None
    }
}
