//! smoltcp tabanlı asgari ağ yığını: DHCP ile IP al, DNS ile çöz, TCP ile
//! HTTP/1.0 GET yap. Hepsi yoklamalı (kesmesiz) tek bir bloklayan akışta.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::dns::{self, GetQueryResultError};
use smoltcp::socket::{dhcpv4, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    DnsQueryType, EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address,
};

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

/// `host` için `path`'i HTTP/1.0 ile çeker. Tüm yanıtı (başlıklar dahil) döndürür.
pub fn fetch(host: &str, path: &str) -> Result<String, &'static str> {
    if !crate::e1000::ready() {
        return Err("ağ kartı yok (e1000)");
    }

    let mac = crate::e1000::mac();
    let mut device = E1000Phy;

    let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
    config.random_seed = crate::time::millis().wrapping_mul(2654435761).wrapping_add(1);
    let mut iface = Interface::new(config, &mut device, now());

    let mut sockets = SocketSet::new(vec![]);

    // --- 1) DHCP ile IP/gateway/DNS al ---
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

    // --- 2) Adı IP'ye çöz (gerekiyorsa) ---
    let server = dns_server.unwrap_or(Ipv4Address::new(8, 8, 8, 8));
    let ip = match parse_ipv4(host) {
        Some(ip) => IpAddress::Ipv4(ip),
        None => resolve(&mut iface, &mut device, &mut sockets, server, host)?,
    };

    // --- 3) TCP bağlan + HTTP GET ---
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
