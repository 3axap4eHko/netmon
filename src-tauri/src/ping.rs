use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

#[cfg(windows)]
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
};

pub struct IcmpPinger {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

// SAFETY: The Windows ICMP handle is thread-safe — IcmpSendEcho can be called
// concurrently from multiple threads on the same handle.
unsafe impl Send for IcmpPinger {}
unsafe impl Sync for IcmpPinger {}

/// Result from an ICMP ping.
#[derive(Debug)]
pub struct PingResult {
    pub ip: String,
    pub latency_ms: Option<f64>,
    pub ttl_expired: bool,
}

// On 64-bit Windows, ICMP_ECHO_REPLY contains pointers that are 8 bytes.
// We define the struct to match the exact Windows layout.
#[cfg(windows)]
#[repr(C)]
struct IcmpEchoReply {
    address: u32,                       // IPAddr (4 bytes)
    status: u32,                        // ULONG (4 bytes)
    round_trip_time: u32,               // ULONG (4 bytes)
    data_size: u16,                     // USHORT (2 bytes)
    reserved: u16,                      // USHORT (2 bytes)
    data: usize,                        // PVOID — use usize to match pointer width
    // Embedded IP_OPTION_INFORMATION:
    options_ttl: u8,
    options_tos: u8,
    options_flags: u8,
    options_options_size: u8,
    options_options_data: usize,        // PUCHAR — use usize to match pointer width
}

/// IP_OPTION_INFORMATION passed to IcmpSendEcho to control TTL.
#[cfg(windows)]
#[repr(C)]
struct IpOptionInfo {
    ttl: u8,
    tos: u8,
    flags: u8,
    options_size: u8,
    options_data: usize,                // PUCHAR — use usize to match pointer width
}

#[cfg(windows)]
impl IcmpPinger {
    pub fn new() -> io::Result<Self> {
        let handle = unsafe { IcmpCreateFile() }
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("IcmpCreateFile failed: {}", e)))?;
        Ok(IcmpPinger { handle })
    }

    /// Ping an IP address with a specific TTL. Returns the responding IP and latency.
    pub fn ping(&self, ip: &str, timeout_ms: u32, ttl: u8) -> Option<PingResult> {
        let resolved_ip = resolve_ipv4(ip)?;
        let addr = parse_ipv4_literal(&resolved_ip)?;
        let send_data: [u8; 32] = [0u8; 32];

        let options = IpOptionInfo {
            ttl,
            tos: 0,
            flags: 0,
            options_size: 0,
            options_data: 0, // null pointer as usize
        };

        // Allocate a generous reply buffer (at least sizeof(ICMP_ECHO_REPLY) + 8 + data)
        let reply_size = std::mem::size_of::<IcmpEchoReply>() + send_data.len() + 64;
        let mut reply_buf: Vec<u8> = vec![0u8; reply_size];

        let ret = unsafe {
            IcmpSendEcho(
                self.handle,
                addr,
                send_data.as_ptr() as *const _,
                send_data.len() as u16,
                Some(&options as *const IpOptionInfo as *const _),
                reply_buf.as_mut_ptr() as *mut _,
                reply_buf.len() as u32,
                timeout_ms,
            )
        };

        if ret == 0 {
            // On Vista+, the reply buffer may still contain valid data even when ret==0.
            // Check the status field in the reply buffer.
            let reply = unsafe { &*(reply_buf.as_ptr() as *const IcmpEchoReply) };

            // IP_TTL_EXPIRED_TRANSIT = 11013
            if reply.status == 11013 {
                let reply_ip = ipv4_to_string(reply.address);
                return Some(PingResult {
                    ip: reply_ip,
                    latency_ms: Some(reply.round_trip_time as f64),
                    ttl_expired: true,
                });
            }

            // Actual timeout or error
            return None;
        }

        let reply = unsafe { &*(reply_buf.as_ptr() as *const IcmpEchoReply) };
        let reply_ip = ipv4_to_string(reply.address);

        match reply.status {
            0 => Some(PingResult {
                ip: reply_ip,
                latency_ms: Some(reply.round_trip_time as f64),
                ttl_expired: false,
            }),
            11013 => Some(PingResult {
                ip: reply_ip,
                latency_ms: Some(reply.round_trip_time as f64),
                ttl_expired: true,
            }),
            _ => None,
        }
    }

    /// Simple ping (TTL=128, just get latency).
    pub fn ping_host(&self, ip: &str, timeout_ms: u32) -> Option<f64> {
        let result = self.ping(ip, timeout_ms, 128)?;
        if result.ttl_expired {
            return None;
        }
        result.latency_ms
    }

    /// Trace a single hop by pinging target with specific TTL.
    pub fn trace_hop(&self, target: &str, ttl: u8) -> Option<String> {
        let result = self.ping(target, 1000, ttl)?;
        Some(result.ip)
    }
}

#[cfg(windows)]
impl Drop for IcmpPinger {
    fn drop(&mut self) {
        unsafe {
            let _ = IcmpCloseHandle(self.handle);
        }
    }
}

// Fallback for non-Windows
#[cfg(not(windows))]
impl IcmpPinger {
    pub fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ICMP pinger only supported on Windows",
        ))
    }

    pub fn ping(&self, _ip: &str, _timeout_ms: u32, _ttl: u8) -> Option<PingResult> {
        None
    }

    pub fn ping_host(&self, _ip: &str, _timeout_ms: u32) -> Option<f64> {
        None
    }

    pub fn trace_hop(&self, _target: &str, _ttl: u8) -> Option<String> {
        None
    }
}

fn ipv4_to_string(addr: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        addr & 0xFF,
        (addr >> 8) & 0xFF,
        (addr >> 16) & 0xFF,
        (addr >> 24) & 0xFF
    )
}

/// Resolve a target string into an IPv4 address string.
/// Accepts IPv4 literals directly and hostnames via DNS A records.
pub fn resolve_ipv4(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }

    if let Some(addr) = parse_ipv4_literal(target) {
        return Some(ipv4_to_string(addr));
    }

    let mut addrs = (target, 0).to_socket_addrs().ok()?;
    addrs.find_map(|addr| match addr {
        SocketAddr::V4(v4) => Some(v4.ip().to_string()),
        SocketAddr::V6(_) => None,
    })
}

fn parse_ipv4_literal(ip: &str) -> Option<u32> {
    if let Ok(parsed) = ip.parse::<IpAddr>() {
        if let IpAddr::V4(v4) = parsed {
            let octets = v4.octets();
            return Some(
                (octets[0] as u32)
                    | ((octets[1] as u32) << 8)
                    | ((octets[2] as u32) << 16)
                    | ((octets[3] as u32) << 24),
            );
        }
    }
    None
}
