//! Who owns a loopback listening socket (PRD 0036 re-validation, S-01).
//!
//! Plain HTTP on loopback has no server authentication: any program of any
//! local user may bind a port while the service that normally holds it is
//! down. Before a request that carries a password or a session goes to a
//! loopback address, the caller asks [`ListenerCheck::verdict`] who listens
//! on that port. Only the operating system's own socket table is read,
//! locally; nothing is sent anywhere.
//!
//! - **Linux:** the `LISTEN` rows of `/proc/net/tcp` and `/proc/net/tcp6`
//!   carry the owner's uid of every socket of this network namespace.
//! - **Windows:** `GetExtendedTcpTable` (IPv4 and IPv6) gives the owning
//!   process, and that process's token gives its user. A process whose
//!   token cannot be opened counts as foreign.
//! - **macOS:** there is no unprivileged way to name the owner of another
//!   user's socket, so the check is positive: one of the current user's own
//!   processes must hold a listening socket on that port (what `lsof` does
//!   without root, through `libproc`).
//!
//! Every listener the connection could reach counts: the exact loopback
//! address and the wildcard binds. One foreign listener among them refuses,
//! so a more specific bind over a wildcard one cannot win. Allowed owners
//! are the current user and the superuser (root, SYSTEM), which can already
//! do anything on the machine.
//!
//! What remains possible, and is documented: code running as the same user,
//! the superuser, and a socket that changes hands in the milliseconds
//! between this check and the connection (only while the port was free).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Who owns one listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    CurrentUser,
    /// root or SYSTEM.
    Superuser,
    /// Another user, or a process whose user could not be read.
    Other,
}

/// One row of the socket table: a TCP socket in the `LISTEN` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Listener {
    pub addr: IpAddr,
    pub port: u16,
    pub owner: Owner,
}

/// The answer for one loopback port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Every listener the connection could reach belongs to the current
    /// user or the superuser.
    Trusted,
    /// At least one of them belongs to someone else.
    Foreign,
    /// No listener could be found or the table could not be read.
    Unconfirmed,
}

/// The check a caller injects (the system one, or a fake in tests).
pub(crate) trait ListenerCheck: Send + Sync {
    /// Who listens on `port` of the loopback addresses `targets` (one host
    /// may resolve to both `127.0.0.1` and `::1`).
    fn verdict(&self, targets: &[IpAddr], port: u16) -> Verdict;
}

/// The operating system's socket table.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct System;

impl ListenerCheck for System {
    fn verdict(&self, targets: &[IpAddr], port: u16) -> Verdict {
        let started = std::time::Instant::now();
        let verdict = match listeners(port) {
            Some(rows) => decide(&rows, targets, port),
            None => Verdict::Unconfirmed,
        };
        use crate::diagnostics::logging::{self, Field, Level};
        let class = match verdict {
            Verdict::Trusted => "trusted",
            Verdict::Foreign => "foreign",
            Verdict::Unconfirmed => "unconfirmed",
        };
        let millis = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        logging::event(
            Level::Debug,
            "local_listener.checked",
            &[
                ("verdict", Field::Text(class)),
                ("took", Field::Millis(millis)),
            ],
        );
        verdict
    }
}

/// The loopback addresses a URL host stands for (`localhost` resolves to
/// both families). `None` when the host is not loopback.
pub(crate) fn loopback_targets(host: &url::Host<&str>) -> Option<Vec<IpAddr>> {
    match host {
        url::Host::Domain(domain) if domain.eq_ignore_ascii_case("localhost") => Some(vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ]),
        url::Host::Domain(_) => None,
        url::Host::Ipv4(ip) => ip.is_loopback().then_some(vec![IpAddr::V4(*ip)]),
        url::Host::Ipv6(ip) => ip.is_loopback().then_some(vec![IpAddr::V6(*ip)]),
    }
}

/// True when a connection to `target` could be accepted by a socket bound
/// to `bound`: the same address, its IPv4-mapped form, or a wildcard (`::`
/// is dual-stack unless the program asked otherwise, so it counts for both
/// families).
fn reaches(bound: IpAddr, target: IpAddr) -> bool {
    if bound == target {
        return true;
    }
    match (bound, target) {
        (IpAddr::V4(bound), IpAddr::V4(_)) => bound.is_unspecified(),
        (IpAddr::V6(bound), IpAddr::V6(_)) => bound.is_unspecified(),
        (IpAddr::V6(bound), IpAddr::V4(target)) => {
            bound.is_unspecified() || bound.to_ipv4_mapped() == Some(target)
        }
        (IpAddr::V4(_), IpAddr::V6(_)) => false,
    }
}

/// The verdict over a socket table: the listeners on `port` that a
/// connection to any of `targets` could reach must all belong to the
/// current user or the superuser, and there must be at least one.
pub(crate) fn decide(rows: &[Listener], targets: &[IpAddr], port: u16) -> Verdict {
    let mut seen = false;
    for row in rows.iter().filter(|row| row.port == port) {
        if !targets.iter().any(|target| reaches(row.addr, *target)) {
            continue;
        }
        seen = true;
        if row.owner == Owner::Other {
            return Verdict::Foreign;
        }
    }
    if seen {
        Verdict::Trusted
    } else {
        Verdict::Unconfirmed
    }
}

#[cfg(target_os = "linux")]
fn listeners(port: u16) -> Option<Vec<Listener>> {
    linux::listeners(port)
}

#[cfg(target_os = "macos")]
fn listeners(port: u16) -> Option<Vec<Listener>> {
    macos::listeners(port)
}

#[cfg(windows)]
fn listeners(port: u16) -> Option<Vec<Listener>> {
    windows::listeners(port)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn listeners(_port: u16) -> Option<Vec<Listener>> {
    None
}

/// `/proc/net/tcp{,6}` (PRD 0036 S-01). The parser is plain text so it is
/// tested on every OS over a captured sample.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod linux {
    use super::{Listener, Owner};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    /// The kernel's `TCP_LISTEN` state as `/proc/net/tcp` prints it.
    const LISTEN: &str = "0A";

    #[cfg(target_os = "linux")]
    pub(super) fn listeners(port: u16) -> Option<Vec<Listener>> {
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        let mut rows = Vec::new();
        let mut read_any = false;
        for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
            // A kernel without IPv6 has no `tcp6`; one readable file is enough.
            if let Ok(text) = std::fs::read_to_string(file) {
                read_any = true;
                rows.extend(parse(&text, me).into_iter().filter(|row| row.port == port));
            }
        }
        read_any.then_some(rows)
    }

    /// The `LISTEN` rows of one `/proc/net/tcp` or `tcp6` text, with each
    /// owner judged against the uid `me`.
    pub(crate) fn parse(text: &str, me: u32) -> Vec<Listener> {
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                // sl local rem st tx:rx tr:when retrnsmt uid ...
                if fields.len() < 8 || fields[3] != LISTEN {
                    return None;
                }
                let (addr, port) = fields[1].split_once(':')?;
                let port = u16::from_str_radix(port, 16).ok()?;
                let addr = address(addr)?;
                let uid: u32 = fields[7].parse().ok()?;
                let owner = if uid == me {
                    Owner::CurrentUser
                } else if uid == 0 {
                    Owner::Superuser
                } else {
                    Owner::Other
                };
                Some(Listener { addr, port, owner })
            })
            .collect()
    }

    /// The kernel prints each 32-bit word of the address as the native
    /// integer of its network-order bytes.
    fn address(hex: &str) -> Option<IpAddr> {
        let word = |chunk: &str| u32::from_str_radix(chunk, 16).ok().map(u32::to_ne_bytes);
        match hex.len() {
            8 => Some(IpAddr::V4(Ipv4Addr::from(word(hex)?))),
            32 => {
                let mut bytes = [0u8; 16];
                for (index, slot) in bytes.chunks_mut(4).enumerate() {
                    slot.copy_from_slice(&word(hex.get(index * 8..index * 8 + 8)?)?);
                }
                Some(IpAddr::V6(Ipv6Addr::from(bytes)))
            }
            _ => None,
        }
    }
}

/// `libproc` walk of the current user's own processes (PRD 0036 S-01).
#[cfg(target_os = "macos")]
mod macos {
    use super::{Listener, Owner};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // From <sys/proc_info.h> (macOS SDK); `libc` does not declare the
    // socket structures. Offsets measured against the SDK header with
    // `offsetof` (struct socket_fdinfo is 792 bytes, 8-aligned):
    // psi at 24, psi.soi_family at 184, psi.soi_kind at 256,
    // psi.soi_proto at 264; in the TCP variant, insi_lport at +4,
    // insi_vflag at +24, insi_laddr at +48 and tcpsi_state at +80.
    const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
    const SOCKET_FDINFO_SIZE: usize = 792;
    const OFF_FAMILY: usize = 184;
    const OFF_KIND: usize = 256;
    const OFF_PROTO: usize = 264;
    const OFF_LPORT: usize = OFF_PROTO + 4;
    const OFF_VFLAG: usize = OFF_PROTO + 24;
    const OFF_LADDR: usize = OFF_PROTO + 48;
    const OFF_STATE: usize = OFF_PROTO + 80;
    const SOCKINFO_TCP: i32 = 2;
    const TSI_S_LISTEN: i32 = 1;
    const INI_IPV4: u8 = 0x1;
    const INI_IPV6: u8 = 0x2;
    const PROC_UID_ONLY: u32 = 4;

    #[repr(C, align(8))]
    struct SocketFdInfo([u8; SOCKET_FDINFO_SIZE]);

    impl SocketFdInfo {
        fn i32_at(&self, offset: usize) -> i32 {
            i32::from_ne_bytes(self.0[offset..offset + 4].try_into().unwrap_or_default())
        }
    }

    pub(super) fn listeners(port: u16) -> Option<Vec<Listener>> {
        // SAFETY: `getuid` has no preconditions and cannot fail.
        let me = unsafe { libc::getuid() };
        let pids = own_pids(me)?;
        let mut rows = Vec::new();
        for pid in pids {
            rows.extend(
                tcp_listeners(pid)
                    .into_iter()
                    .filter(|(_, bound_port)| *bound_port == port)
                    .map(|(addr, port)| Listener {
                        addr,
                        port,
                        owner: Owner::CurrentUser,
                    }),
            );
        }
        Some(rows)
    }

    /// The processes whose user is `uid`.
    fn own_pids(uid: libc::uid_t) -> Option<Vec<libc::pid_t>> {
        // SAFETY: a size query with a null buffer.
        let needed = unsafe { libc::proc_listpids(PROC_UID_ONLY, uid, std::ptr::null_mut(), 0) };
        if needed <= 0 {
            return None;
        }
        // Room for processes started between the two calls.
        let capacity = needed as usize / std::mem::size_of::<libc::pid_t>() + 64;
        let mut pids = vec![0 as libc::pid_t; capacity];
        // SAFETY: `pids` holds `capacity` pids; the size is in bytes.
        let filled = unsafe {
            libc::proc_listpids(
                PROC_UID_ONLY,
                uid,
                pids.as_mut_ptr().cast(),
                (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
            )
        };
        if filled <= 0 {
            return None;
        }
        pids.truncate(filled as usize / std::mem::size_of::<libc::pid_t>());
        pids.retain(|pid| *pid > 0);
        Some(pids)
    }

    /// The listening TCP sockets of `pid` (address, port). A process that
    /// exits or refuses is skipped.
    fn tcp_listeners(pid: libc::pid_t) -> Vec<(IpAddr, u16)> {
        let entry = std::mem::size_of::<libc::proc_fdinfo>();
        // SAFETY: a size query with a null buffer.
        let needed =
            unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if needed <= 0 {
            return Vec::new();
        }
        let capacity = needed as usize / entry + 16;
        let mut fds: Vec<libc::proc_fdinfo> = Vec::with_capacity(capacity);
        // SAFETY: the buffer has room for `capacity` entries; the kernel
        // writes at most that many bytes and returns how many it wrote.
        let filled = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                (capacity * entry) as libc::c_int,
            )
        };
        if filled <= 0 {
            return Vec::new();
        }
        // SAFETY: the kernel initialized `filled` bytes of whole entries.
        unsafe { fds.set_len((filled as usize / entry).min(capacity)) };
        fds.iter()
            .filter(|fd| fd.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32)
            .filter_map(|fd| socket_listener(pid, fd.proc_fd))
            .collect()
    }

    fn socket_listener(pid: libc::pid_t, fd: i32) -> Option<(IpAddr, u16)> {
        let mut info = SocketFdInfo([0; SOCKET_FDINFO_SIZE]);
        // SAFETY: `info` is a buffer of exactly the structure's size.
        let got = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd,
                PROC_PIDFDSOCKETINFO,
                info.0.as_mut_ptr().cast(),
                SOCKET_FDINFO_SIZE as libc::c_int,
            )
        };
        if got as usize != SOCKET_FDINFO_SIZE {
            return None;
        }
        let family = info.i32_at(OFF_FAMILY);
        if info.i32_at(OFF_KIND) != SOCKINFO_TCP
            || !(family == libc::AF_INET || family == libc::AF_INET6)
            || info.i32_at(OFF_STATE) != TSI_S_LISTEN
        {
            return None;
        }
        // The port is kept in network byte order in the low 16 bits.
        let port = u16::from_be((info.i32_at(OFF_LPORT) & 0xffff) as u16);
        let raw: [u8; 16] = info.0[OFF_LADDR..OFF_LADDR + 16].try_into().ok()?;
        let vflag = info.0[OFF_VFLAG];
        let addr = if vflag & INI_IPV6 != 0 {
            IpAddr::V6(Ipv6Addr::from(raw))
        } else if vflag & INI_IPV4 != 0 {
            // in4in6_addr: 12 bytes of padding, then the IPv4 address.
            IpAddr::V4(Ipv4Addr::new(raw[12], raw[13], raw[14], raw[15]))
        } else {
            return None;
        };
        Some((addr, port))
    }
}

/// `GetExtendedTcpTable` and the owning process's token (PRD 0036 S-01).
#[cfg(windows)]
mod windows {
    use super::{Listener, Owner};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The well-known SID of the LocalSystem account.
    const SYSTEM_SID: &str = "S-1-5-18";

    pub(super) fn listeners(port: u16) -> Option<Vec<Listener>> {
        let me = token_user(unsafe { GetCurrentProcess() })?;
        let mut owners: Vec<(u32, Owner)> = Vec::new();
        let mut owner_of = |pid: u32| -> Owner {
            if let Some((_, owner)) = owners.iter().find(|(known, _)| *known == pid) {
                return *owner;
            }
            let owner = process_owner(pid, &me);
            owners.push((pid, owner));
            owner
        };
        let mut rows = Vec::new();
        for (addr, bound_port, pid) in table_v4()?.into_iter().chain(table_v6()?) {
            if bound_port == port {
                rows.push(Listener {
                    addr,
                    port: bound_port,
                    owner: owner_of(pid),
                });
            }
        }
        Some(rows)
    }

    /// The owner class of `pid` against the current user's SID `me`.
    pub(super) fn process_owner(pid: u32, me: &str) -> Owner {
        // The System Idle Process and the System process.
        if pid == 0 || pid == 4 {
            return Owner::Superuser;
        }
        // SAFETY: plain call; a null handle means it could not be opened.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Owner::Other;
        }
        let user = token_user(process);
        // SAFETY: `process` was opened above.
        unsafe { CloseHandle(process) };
        super::windows_owner(user.as_deref(), me)
    }

    /// The SDDL string of the user of `process`'s token.
    fn token_user(process: HANDLE) -> Option<String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: `process` is a valid process handle (or the pseudo-handle).
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        let mut size = 0u32;
        // SAFETY: a size query with a null buffer.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size) };
        let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
        // SAFETY: `buffer` holds at least `size` bytes, 8-aligned.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        };
        // SAFETY: `token` was opened above.
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return None;
        }
        // SAFETY: the buffer holds a TOKEN_USER filled by the call above.
        let sid = unsafe { (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid };
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: `sid` is valid while `buffer` lives; `text` is freed below.
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return None;
        }
        // SAFETY: `text` is a NUL-terminated UTF-16 string.
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        // SAFETY: `len` units precede the terminator.
        let units = unsafe { std::slice::from_raw_parts(text, len) };
        let value = String::from_utf16_lossy(units);
        // SAFETY: `ConvertSidToStringSidW` documents `LocalFree` as release.
        unsafe { LocalFree(text.cast()) };
        Some(value)
    }

    /// One `GetExtendedTcpTable` call grown until the table fits.
    fn raw_table(family: u16) -> Option<Vec<u64>> {
        let mut size = 0u32;
        for _ in 0..4 {
            let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
            let mut have = (buffer.len() * 8) as u32;
            // SAFETY: `buffer` holds `have` bytes, 8-aligned.
            let code = unsafe {
                GetExtendedTcpTable(
                    buffer.as_mut_ptr().cast(),
                    &mut have,
                    0,
                    u32::from(family),
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if code == NO_ERROR {
                return Some(buffer);
            }
            // ERROR_INSUFFICIENT_BUFFER: `have` now holds the size needed.
            size = have.max(size + 4096);
        }
        None
    }

    fn table_v4() -> Option<Vec<(IpAddr, u16, u32)>> {
        let buffer = raw_table(AF_INET)?;
        let base = buffer.as_ptr().cast::<u8>();
        // SAFETY: the table starts with its entry count, then the rows.
        let count = unsafe { *(base as *const u32) } as usize;
        let rows = unsafe { base.add(4) } as *const MIB_TCPROW_OWNER_PID;
        let room =
            (buffer.len() * 8).saturating_sub(4) / std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        Some(
            (0..count.min(room))
                .map(|index| {
                    // SAFETY: `index` is below the count and inside the buffer.
                    let row = unsafe { std::ptr::read_unaligned(rows.add(index)) };
                    (
                        IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
                        u16::from_be((row.dwLocalPort & 0xffff) as u16),
                        row.dwOwningPid,
                    )
                })
                .collect(),
        )
    }

    fn table_v6() -> Option<Vec<(IpAddr, u16, u32)>> {
        let buffer = raw_table(AF_INET6)?;
        let base = buffer.as_ptr().cast::<u8>();
        // SAFETY: the table starts with its entry count, then the rows.
        let count = unsafe { *(base as *const u32) } as usize;
        let rows = unsafe { base.add(4) } as *const MIB_TCP6ROW_OWNER_PID;
        let room =
            (buffer.len() * 8).saturating_sub(4) / std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
        Some(
            (0..count.min(room))
                .map(|index| {
                    // SAFETY: `index` is below the count and inside the buffer.
                    let row = unsafe { std::ptr::read_unaligned(rows.add(index)) };
                    (
                        IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                        u16::from_be((row.dwLocalPort & 0xffff) as u16),
                        row.dwOwningPid,
                    )
                })
                .collect(),
        )
    }

    #[allow(dead_code)]
    pub(super) const SYSTEM: &str = SYSTEM_SID;
}

/// Windows owner rule, platform-free so it is tested on every OS: the
/// current user's SID and LocalSystem are allowed; an unreadable token or
/// any other SID is foreign.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_owner(user: Option<&str>, me: &str) -> Owner {
    match user {
        Some(sid) if sid.eq_ignore_ascii_case(me) => Owner::CurrentUser,
        Some(sid) if sid.eq_ignore_ascii_case("S-1-5-18") => Owner::Superuser,
        _ => Owner::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

    fn row(addr: IpAddr, port: u16, owner: Owner) -> Listener {
        Listener { addr, port, owner }
    }

    #[test]
    fn every_reachable_listener_must_be_ours_or_the_superusers() {
        let any4 = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        let any6 = IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        // Ours on the exact address: trusted.
        assert_eq!(
            decide(&[row(V4, 20128, Owner::CurrentUser)], &[V4], 20128),
            Verdict::Trusted
        );
        // root's wildcard (rootful Docker's proxy): trusted.
        assert_eq!(
            decide(&[row(any4, 20128, Owner::Superuser)], &[V4], 20128),
            Verdict::Trusted
        );
        // Another user's specific bind over our wildcard: foreign.
        assert_eq!(
            decide(
                &[
                    row(any6, 20128, Owner::CurrentUser),
                    row(V4, 20128, Owner::Other)
                ],
                &[V4],
                20128
            ),
            Verdict::Foreign
        );
        // Another user's wildcard reaches loopback too.
        assert_eq!(
            decide(&[row(any4, 20128, Owner::Other)], &[V4], 20128),
            Verdict::Foreign
        );
        // `::` is dual-stack.
        assert_eq!(
            decide(&[row(any6, 20128, Owner::Other)], &[V4], 20128),
            Verdict::Foreign
        );
        // `0.0.0.0` does not take an IPv6 connection.
        assert_eq!(
            decide(&[row(any4, 20128, Owner::Other)], &[V6], 20128),
            Verdict::Unconfirmed
        );
        // Another port, or another loopback address, does not count.
        assert_eq!(
            decide(
                &[
                    row(V4, 20129, Owner::Other),
                    row(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), 20128, Owner::Other)
                ],
                &[V4],
                20128
            ),
            Verdict::Unconfirmed
        );
        // `localhost`: a foreign listener on either family refuses.
        assert_eq!(
            decide(
                &[
                    row(V4, 20128, Owner::CurrentUser),
                    row(V6, 20128, Owner::Other)
                ],
                &[V4, V6],
                20128
            ),
            Verdict::Foreign
        );
        // Nothing listening: could not confirm.
        assert_eq!(decide(&[], &[V4], 20128), Verdict::Unconfirmed);
        // IPv4-mapped bind.
        let mapped = IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped());
        assert_eq!(
            decide(&[row(mapped, 20128, Owner::Other)], &[V4], 20128),
            Verdict::Foreign
        );
    }

    #[test]
    fn loopback_targets_cover_both_families_for_localhost() {
        assert_eq!(
            loopback_targets(&url::Host::Domain("LOCALHOST")),
            Some(vec![V4, V6])
        );
        assert_eq!(
            loopback_targets(&url::Host::Ipv4(Ipv4Addr::new(127, 0, 0, 9))),
            Some(vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 9))])
        );
        assert_eq!(loopback_targets(&url::Host::Domain("example.com")), None);
        assert_eq!(
            loopback_targets(&url::Host::Ipv4(Ipv4Addr::new(10, 0, 0, 1))),
            None
        );
    }

    /// A captured `/proc/net/tcp` and `tcp6` (x86_64, little-endian): our
    /// uid 1000 on 127.0.0.1:20128, root on 0.0.0.0:22, uid 1001 on
    /// 127.0.0.1:8080, an established socket (not `0A`) and `::1:20128`
    /// held by uid 1001.
    #[cfg(target_endian = "little")]
    #[test]
    fn linux_proc_net_tcp_rows_give_each_listeners_uid() {
        let tcp = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:4E A0 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 31337 1 0000000000000000 100 0 0 10 0
   1: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1234 1 0000000000000000 100 0 0 10 0
   2: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1001        0 4321 1 0000000000000000 100 0 0 10 0
   3: 0100007F:4EA0 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1001        0 999 1 0000000000000000 20 4 30 10 -1
"
        .replace("4E A0", "4EA0");
        let rows = linux::parse(&tcp, 1000);
        assert_eq!(
            rows,
            vec![
                row(V4, 20128, Owner::CurrentUser),
                row(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 22, Owner::Superuser),
                row(V4, 8080, Owner::Other),
            ]
        );
        let tcp6 = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:4EA0 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1001        0 5555 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:0050 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 5556 1 0000000000000000 100 0 0 10 0
";
        let rows6 = linux::parse(tcp6, 1000);
        assert_eq!(
            rows6,
            vec![
                row(V6, 20128, Owner::Other),
                row(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 80, Owner::CurrentUser),
            ]
        );
        let all: Vec<Listener> = rows.into_iter().chain(rows6).collect();
        assert_eq!(decide(&all, &[V4], 20128), Verdict::Trusted);
        assert_eq!(decide(&all, &[V4, V6], 20128), Verdict::Foreign);
        assert_eq!(decide(&all, &[V4], 8080), Verdict::Foreign);
        assert_eq!(decide(&all, &[V4], 22), Verdict::Trusted);
        // Garbage lines are skipped, never a panic.
        assert!(linux::parse("header\nnot a row\n 0: zz:zz 0 0A a b c x\n", 1000).is_empty());
    }

    /// The Windows rule over a stubbed table: our SID and SYSTEM pass; any
    /// other SID or an unreadable token is foreign.
    #[test]
    fn windows_owner_rule_allows_only_this_user_and_system() {
        let me = "S-1-5-21-1-2-3-1001";
        assert_eq!(windows_owner(Some(me), me), Owner::CurrentUser);
        assert_eq!(windows_owner(Some("S-1-5-18"), me), Owner::Superuser);
        assert_eq!(windows_owner(Some("S-1-5-21-1-2-3-1002"), me), Owner::Other);
        assert_eq!(windows_owner(Some("S-1-5-19"), me), Owner::Other);
        assert_eq!(windows_owner(None, me), Owner::Other);
        let table = [
            row(V4, 20128, windows_owner(Some(me), me)),
            row(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                20128,
                windows_owner(None, me),
            ),
        ];
        assert_eq!(decide(&table, &[V4], 20128), Verdict::Foreign);
        assert_eq!(decide(&table[..1], &[V4], 20128), Verdict::Trusted);
    }

    /// Read the real table for `port` until it gives `expected`, for up to
    /// 10 s, and return the last verdict. The table is the kernel's view a
    /// moment ago: on a loaded macOS runner a closed port was seen to still
    /// read as ours for a while (another listener of this test process, or a
    /// child forked by a parallel test that has not reached `exec` yet, can
    /// briefly hold the same port or a copy of the socket), so no assertion
    /// about a change of state reads it only once. A state that lasts past
    /// the bound still fails the caller's assertion.
    fn wait_for_verdict(families: &[IpAddr], port: u16, expected: Verdict) -> Verdict {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let started = std::time::Instant::now();
            let verdict = System.verdict(families, port);
            eprintln!(
                "local_listener: check of port {port} took {:?}",
                started.elapsed()
            );
            if verdict == expected || std::time::Instant::now() >= deadline {
                return verdict;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// The real table finds a listener this test process opens (every OS
    /// where a check exists), and stops finding it once it is closed.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn the_system_table_finds_a_listener_of_this_process() {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = socket.local_addr().unwrap().port();
        assert_eq!(
            wait_for_verdict(&[V4], port, Verdict::Trusted),
            Verdict::Trusted,
            "an open listener of this process reads as ours within 10 s"
        );
        drop(socket);
        assert_eq!(
            wait_for_verdict(&[V4], port, Verdict::Unconfirmed),
            Verdict::Unconfirmed,
            "a closed listener stops reading as ours within 10 s"
        );
    }

    /// A listener of a child process this test starts (another process of
    /// the same user, in its own session) is found too, on IPv4 and IPv6.
    #[cfg(unix)]
    #[test]
    fn the_system_table_finds_a_listener_of_a_child_process() {
        use std::io::{BufRead as _, BufReader};
        use std::process::{Command, Stdio};
        for (family, host, target) in [("AF_INET", "127.0.0.1", V4), ("AF_INET6", "::1", V6)] {
            let script = format!(
                "import os,socket,time\nos.setsid()\ns=socket.socket(socket.{family})\ns.bind(('{host}',0))\ns.listen(1)\nprint(s.getsockname()[1],flush=True)\ntime.sleep(30)\n"
            );
            let Ok(mut child) = Command::new("python3")
                .args(["-c", &script])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            else {
                eprintln!("python3 not available; skipped");
                return;
            };
            let mut line = String::new();
            let read = child
                .stdout
                .take()
                .map(|out| BufReader::new(out).read_line(&mut line));
            let port: Option<u16> = read
                .and_then(Result::ok)
                .and_then(|_| line.trim().parse().ok());
            let verdict = port.map(|port| wait_for_verdict(&[target], port, Verdict::Trusted));
            let _ = child.kill();
            let _ = child.wait();
            let Some(verdict) = verdict else {
                eprintln!("the child printed no port ({host}); skipped");
                continue;
            };
            assert_eq!(verdict, Verdict::Trusted, "{host}");
        }
    }
}
