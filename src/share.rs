//! Alt+Q: what is chosen, shared with a phone on the same network. A server
//! of its own, on a port the system gives it and a path no one can guess,
//! answers downloads - a file as it is, or several and directories as one
//! zip, written as it goes out - until Esc stops it. Not after the first: a
//! phone's browser asks once to see what the file is, and its download
//! manager asks again to save it, which a server already stopped would
//! leave cancelled. The address is drawn as a QR code to scan.
//! Nothing but the standard library's sockets; the code from the qrcode crate.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// What is shared.
#[derive(Clone, Debug)]
pub enum What {
    /// A file, sent as it is.
    File(PathBuf),
    /// Files and directories, sent as one zip of them, each under its own
    /// name at its top.
    Zip(Vec<PathBuf>),
}

/// What the server has to say, as it says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A download has begun, to the device at this address.
    Started(IpAddr),
    /// So many bytes of it have gone.
    Sent(u64),
    /// The download stopped short - cancelled on the phone, or its
    /// connection gone. The server goes on waiting for another.
    Broken,
    /// It came to its end, the phone saying it had all of it. The server
    /// goes on, for the same phone asking again.
    Done(IpAddr),
    /// The server could not go on.
    Failed(String),
}

/// A share being served. Dropped, it stops.
pub struct Serving {
    /// The port it listens on, at every address the machine has.
    pub port: u16,
    /// The path a phone opens, made up for this share.
    token: String,
    /// The name the download is saved under.
    pub name: String,
    /// How many bytes it is, when known before it is sent: a file's size. A
    /// zip is made as it goes, and is as big as it comes out.
    pub total: Option<u64>,
    pub updates: Receiver<Update>,
    stop: Arc<AtomicBool>,
}

impl Serving {
    /// What a phone opens to reach it at `address`.
    pub fn url(&self, address: IpAddr) -> String {
        format!("http://{}/{}", SocketAddr::new(address, self.port), self.token)
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The addresses a phone on the same network might reach this machine by,
/// likeliest first - the machine's own network before a VPN's tunnel, a
/// container's bridge or a link-local address - for the popup to start on
/// the first and Tab through the rest. Where the interfaces cannot be read,
/// the one address a packet out would leave from.
pub fn lan_addresses() -> Vec<IpAddr> {
    let ranked = rank(&interfaces());
    if ranked.is_empty() { route_address().into_iter().collect() } else { ranked }
}

/// An IPv4 address of one of the machine's interfaces, and what it says of
/// itself.
#[derive(Clone, Debug)]
pub struct Interface {
    pub name: String,
    pub address: Ipv4Addr,
    /// Bits of the address that name its network: 24 for a home network,
    /// 32 for a tunnel's address alone.
    pub prefix: u32,
    /// Up and running: carrying traffic.
    pub running: bool,
    pub loopback: bool,
    /// A link to one other end only, as a VPN's tunnel is.
    pub point_to_point: bool,
}

/// The interfaces' addresses, likeliest for a phone to reach first: the
/// machine's own network, then a bridge or a container's, then what looks
/// like a VPN's tunnel, then a link-local address. One not running, or the
/// loopback, is left out. Kept in the system's order within each.
pub fn rank(interfaces: &[Interface]) -> Vec<IpAddr> {
    let named = |name: &str, prefixes: &[&str]| prefixes.iter().any(|prefix| name.starts_with(prefix));
    let mut usable: Vec<(u8, &Interface)> = interfaces
        .iter()
        .filter(|interface| interface.running && !interface.loopback && !interface.address.is_loopback() && !interface.address.is_unspecified())
        .map(|interface| {
            let tunnel = interface.point_to_point || interface.prefix >= 32 || named(&interface.name, &["tun", "tap", "wg", "proton", "tailscale", "utun", "ppp", "ipsec", "zt", "nordlynx", "mullvad"]);
            let bridge = named(&interface.name, &["docker", "br-", "veth", "virbr", "vboxnet", "vmnet", "lxc", "lxd", "podman", "cni", "flannel"]);
            let rank = if interface.address.is_link_local() {
                4
            } else if tunnel {
                3
            } else if bridge {
                2
            } else if interface.address.is_private() {
                0
            } else {
                1
            };
            (rank, interface)
        })
        .collect();
    usable.sort_by_key(|&(rank, _)| rank);
    let mut addresses: Vec<IpAddr> = Vec::new();
    for (_, interface) in usable {
        let address = IpAddr::V4(interface.address);
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    addresses
}

/// Each interface's IPv4 addresses, as the system lists them.
#[cfg(unix)]
fn interfaces() -> Vec<Interface> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills in a list for freeifaddrs to free, which is
    // read only while it is held, entry by entry, each pointer checked.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut at = list;
    while !at.is_null() {
        // SAFETY: a non-null entry of the list getifaddrs gave.
        let entry = unsafe { &*at };
        at = entry.ifa_next;
        // SAFETY: checked for null; an address of AF_INET is a sockaddr_in,
        // and so is its netmask.
        if entry.ifa_addr.is_null() || i32::from(unsafe { (*entry.ifa_addr).sa_family }) != libc::AF_INET {
            continue;
        }
        let address = Ipv4Addr::from(u32::from_be(unsafe { (*entry.ifa_addr.cast::<libc::sockaddr_in>()).sin_addr.s_addr }));
        let prefix = if entry.ifa_netmask.is_null() {
            32
        } else {
            u32::from_be(unsafe { (*entry.ifa_netmask.cast::<libc::sockaddr_in>()).sin_addr.s_addr }).count_ones()
        };
        // SAFETY: the name of a listed interface, ended by its nul.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }.to_string_lossy().into_owned();
        let flags = entry.ifa_flags as u32;
        let has = |flag: libc::c_int| flags & flag as u32 != 0;
        found.push(Interface {
            name,
            address,
            prefix,
            running: has(libc::IFF_UP) && has(libc::IFF_RUNNING),
            loopback: has(libc::IFF_LOOPBACK),
            point_to_point: has(libc::IFF_POINTOPOINT),
        });
    }
    // SAFETY: the list getifaddrs gave, freed once, nothing of it kept.
    unsafe { libc::freeifaddrs(list) };
    found
}

/// Elsewhere the interfaces are not read: the one address a packet out
/// leaves from stands for them.
#[cfg(not(unix))]
fn interfaces() -> Vec<Interface> {
    Vec::new()
}

/// The address a packet out would be sent from. Nothing is sent - a
/// datagram socket only picks its route on connecting. None with no
/// network, or only the machine's own loopback. With a VPN up, the tunnel's.
fn route_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// The name `what` is saved under on the phone: a file's own, or for a zip
/// the one thing in it, or `name` - the directory the panel is in - when
/// there are several.
pub fn download_name(what: &What, name: &str) -> String {
    let own = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    match what {
        What::File(path) => own(path),
        What::Zip(items) if items.len() == 1 => format!("{}.zip", own(&items[0])),
        What::Zip(_) => format!("{}.zip", if name.is_empty() { "fm84" } else { name }),
    }
}

/// Serve `what` at `address` - every address the machine has, for the
/// unspecified one - on a port the system gives, under a path made up now;
/// what happens comes on its updates.
pub fn serve(what: What, name: String, address: IpAddr) -> io::Result<Serving> {
    let listener = TcpListener::bind(SocketAddr::new(address, 0))?;
    listener.set_nonblocking(true)?;
    let token = token();
    let port = listener.local_addr()?.port();
    let total = match &what {
        What::File(path) => Some(std::fs::metadata(path)?.len()),
        What::Zip(_) => None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, updates) = mpsc::channel();
    let shared = Arc::new(Shared {
        what,
        name: name.clone(),
        token: token.clone(),
        stop: Arc::clone(&stop),
        updates: sender,
    });
    std::thread::spawn(move || listen(listener, shared));
    Ok(Serving { port, token, name, total, updates, stop })
}

/// What every connection's thread needs to answer.
struct Shared {
    what: What,
    name: String,
    token: String,
    stop: Arc<AtomicBool>,
    updates: Sender<Update>,
}

/// A path no one on the network could guess: ten hex digits from the hash
/// the standard library seeds afresh for each process and each hasher.
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos()));
    format!("{:010x}", hasher.finish() & 0xFF_FFFF_FFFF)
}

/// Take connections until told to stop, or until one has had the whole
/// download - each on a thread of its own, so a phone that opens one and
/// says nothing holds no one else up.
fn listen(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || answer(stream, peer.ip(), &shared));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = shared.updates.send(Update::Failed(error.to_string()));
                return;
            }
        }
    }
}

/// One connection: its request read, and the download sent - or, for any
/// other path, nothing but that there is nothing there.
fn answer(mut stream: TcpStream, peer: IpAddr, shared: &Shared) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let Some((method, path)) = read_request(&mut stream) else {
        return;
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    if path != format!("/{}", shared.token) || !matches!(method.as_str(), "GET" | "HEAD") {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    let mut head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/{}\r\n{}\r\nConnection: close\r\nCache-Control: no-store\r\nAccept-Ranges: none\r\n",
        if matches!(shared.what, What::Zip(_)) { "zip" } else { "octet-stream" },
        disposition(&shared.name)
    );
    if let What::File(path) = &shared.what {
        match std::fs::metadata(path) {
            Ok(metadata) => head.push_str(&format!("Content-Length: {}\r\n", metadata.len())),
            Err(error) => {
                let _ = stream.write_all(b"HTTP/1.1 410 Gone\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = shared.updates.send(Update::Failed(error.to_string()));
                return;
            }
        }
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).is_err() || method == "HEAD" {
        return;
    }
    let _ = shared.updates.send(Update::Started(peer));
    let mut out = Counting {
        inner: &mut stream,
        sent: 0,
        updates: &shared.updates,
        stop: &shared.stop,
    };
    let sent = match &shared.what {
        What::File(path) => std::fs::File::open(path).and_then(|mut file| io::copy(&mut file, &mut out)).map(drop),
        What::Zip(items) => send_zip(items, &mut out),
    };
    match sent.and_then(|()| stream.flush()).and_then(|()| received(&mut stream)) {
        Ok(()) => {
            let _ = shared.updates.send(Update::Done(peer));
        }
        Err(_) if shared.stop.load(Ordering::Relaxed) => {}
        Err(_) => {
            let _ = shared.updates.send(Update::Broken);
        }
    }
}

/// Whether the phone had all of what was written: written is only handed to
/// the system, and a small file is all handed over before the phone has read
/// a byte of it. Told the end has come, a phone that has it all closes its
/// side; one that gave up resets the connection, the rest unread. Waited for
/// a while, and taken as had if it says nothing either way.
fn received(stream: &mut TcpStream) -> io::Result<()> {
    stream.shutdown(std::net::Shutdown::Write)?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut rest = [0; 512];
    loop {
        match stream.read(&mut rest) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

/// The request's method and path, from a head no longer than a few
/// kilobytes - what a browser asks with - or None for anything else.
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut head = Vec::new();
    let mut buffer = [0; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return None;
        }
        let read = stream.read(&mut buffer).ok().filter(|&read| read > 0)?;
        head.extend_from_slice(&buffer[..read]);
    }
    let line = String::from_utf8_lossy(&head[..head.iter().position(|&byte| byte == b'\r')?]).into_owned();
    let mut parts = line.split(' ');
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

/// The header that saves the download under `name`: plain, with what is
/// not plain ASCII made an underscore, for an old browser, and whole in
/// UTF-8 for the rest.
fn disposition(name: &str) -> String {
    let plain: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
    let encoded: String = name.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect();
    format!("Content-Disposition: attachment; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

/// The socket a download goes out on, counting what it takes and saying so,
/// and refusing to go on once the share is stopped.
struct Counting<'a> {
    inner: &'a mut TcpStream,
    sent: u64,
    updates: &'a Sender<Update>,
    stop: &'a AtomicBool,
}

impl Write for Counting<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
        }
        let written = self.inner.write(buffer)?;
        self.sent += written as u64;
        let _ = self.updates.send(Update::Sent(self.sent));
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// `items` as one zip, written out as it is made: each directory with what
/// is in it, under its own name. Links and what is neither file nor
/// directory are left out - a link could lead anywhere on the disk.
fn send_zip(items: &[PathBuf], out: &mut dyn Write) -> io::Result<()> {
    let mut zip = zip::ZipWriter::new_stream(out);
    let mut stack: Vec<(PathBuf, String)> = items.iter().rev().filter_map(|path| Some((path.clone(), path.file_name()?.to_string_lossy().into_owned()))).collect();
    while let Some((path, name)) = stack.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let mut options = zip::write::SimpleFileOptions::default();
        if let Some(time) = metadata.modified().ok().and_then(crate::fs_ops::zip_time) {
            options = options.last_modified_time(time);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            options = options.unix_permissions(metadata.permissions().mode() & 0o777);
        }
        if metadata.is_dir() {
            zip.add_directory(format!("{name}/"), options).map_err(io::Error::other)?;
            let mut inside: Vec<_> = std::fs::read_dir(&path).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
            inside.sort();
            for child in inside.into_iter().rev() {
                let child_name = format!("{name}/{}", child.file_name().unwrap_or_default().to_string_lossy());
                stack.push((child, child_name));
            }
        } else if metadata.is_file() {
            let options = options.compression_method(zip::CompressionMethod::Deflated).compression_level(Some(1)).large_file(metadata.len() >= 0xFFFF_FFFF);
            zip.start_file(name, options).map_err(io::Error::other)?;
            io::copy(&mut std::fs::File::open(&path)?, &mut zip)?;
        }
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

/// The code for `text`, at the lowest error correction - the fewest modules,
/// the smallest to fit in a terminal, and still read at once off a screen
/// by any phone - as its width and whether each module, row by row, is dark.
pub fn qr(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::L).ok()?;
    Some((code.width(), code.to_colors().into_iter().map(|colour| colour == qrcode::Color::Dark).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a download brings: the head and the body, as a phone would ask.
    fn fetch(url: &str) -> (String, Vec<u8>) {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_at(rest.find('/').unwrap());
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes()).unwrap();
        let mut all = Vec::new();
        stream.read_to_end(&mut all).unwrap();
        let split = all.windows(4).position(|window| window == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&all[..split]).into_owned(), all[split + 4..].to_vec())
    }

    /// Updates until the server says it is done, or a few seconds pass.
    fn until_done(serving: &Serving) -> Vec<Update> {
        let mut seen = Vec::new();
        while let Ok(update) = serving.updates.recv_timeout(Duration::from_secs(5)) {
            let done = matches!(update, Update::Done(_));
            seen.push(update);
            if done {
                break;
            }
        }
        seen
    }

    #[test]
    fn a_file_is_downloaded_and_downloaded_again_until_the_share_stops() {
        let dir = std::env::temp_dir().join(format!("fm84-share-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Fotka čaj.jpg");
        let content: Vec<u8> = (0..200_000u32).map(|n| n as u8).collect();
        std::fs::write(&file, &content).unwrap();

        // Listening at every address, as a share does, and reached at one.
        let serving = serve(What::File(file.clone()), download_name(&What::File(file.clone()), ""), IpAddr::V4(Ipv4Addr::UNSPECIFIED)).unwrap();
        let url = serving.url(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(serving.total, Some(200_000));
        // Anything but its own path: nothing there, and the share goes on.
        let wrong = url.rsplit_once('/').unwrap().0.to_string() + "/guess";
        let (head, body) = fetch(&wrong);
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        assert!(body.is_empty());

        let (head, body) = fetch(&url);
        let updates = until_done(&serving);
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(head.contains("Content-Length: 200000"));
        assert!(head.contains("filename*=UTF-8''Fotka%20%C4%8Daj.jpg"), "{head}");
        assert!(head.contains("filename=\"Fotka _aj.jpg\""), "{head}");
        assert_eq!(body, content);
        assert_eq!(updates.first(), Some(&Update::Started(IpAddr::V4(Ipv4Addr::LOCALHOST))));
        assert_eq!(updates.last(), Some(&Update::Done(IpAddr::V4(Ipv4Addr::LOCALHOST))));
        assert!(updates.contains(&Update::Sent(200_000)));

        // Asked again, as a phone's download manager asks after its browser
        // has looked: answered whole again.
        let (_, again) = fetch(&url);
        assert_eq!(again, content);
        assert!(matches!(until_done(&serving).last(), Some(Update::Done(_))));

        // Put away: no one is answered.
        let host = url.strip_prefix("http://").unwrap().split('/').next().unwrap().to_string();
        drop(serving);
        std::thread::sleep(Duration::from_millis(200));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            TcpStream::connect(&host)
                .map(|mut stream| {
                    let _ = stream.write_all(b"GET / HTTP/1.1\r\n\r\n");
                    let mut all = Vec::new();
                    let _ = stream.read_to_end(&mut all);
                    all.is_empty()
                })
                .unwrap_or(true),
            "stopped once put away"
        );
    }

    /// What a phone's browser does before its download manager takes over:
    /// asks, reads the head, and lets go without the rest. A small file is
    /// handed over whole before then, yet it was not had - said so, not that
    /// it was sent; and the download manager, asking next, gets it whole.
    #[cfg(unix)]
    #[test]
    fn a_download_given_up_on_is_not_taken_for_one_had() {
        let dir = std::env::temp_dir().join(format!("fm84-share-given-up-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("small.txt");
        std::fs::write(&file, vec![b'x'; 50_000]).unwrap();
        let serving = serve(What::File(file.clone()), "small.txt".to_string(), IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        let url = serving.url(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_at(rest.find('/').unwrap());

        let mut browser = TcpStream::connect(host).unwrap();
        browser.write_all(format!("GET {path} HTTP/1.1\r\n\r\n").as_bytes()).unwrap();
        let mut head = [0; 64];
        browser.read_exact(&mut head).unwrap();
        // Long enough for all of it to be handed over, unread.
        std::thread::sleep(Duration::from_millis(300));
        drop(browser);
        let mut first = Vec::new();
        while let Ok(update) = serving.updates.recv_timeout(Duration::from_secs(5)) {
            let over = matches!(update, Update::Broken | Update::Done(_));
            first.push(update);
            if over {
                break;
            }
        }

        let (_, body) = fetch(&url);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(first.last(), Some(&Update::Broken), "given up on, not sent: {first:?}");
        assert_eq!(body.len(), 50_000, "the next one answered whole");
    }

    #[test]
    fn several_go_as_one_zip_made_as_it_is_sent() {
        let dir = std::env::temp_dir().join(format!("fm84-share-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("photos/trip")).unwrap();
        std::fs::write(dir.join("photos/a.txt"), "first").unwrap();
        std::fs::write(dir.join("photos/trip/b.txt"), "second").unwrap();
        std::fs::write(dir.join("notes.txt"), "third").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", dir.join("photos/link")).unwrap();

        let what = What::Zip(vec![dir.join("photos"), dir.join("notes.txt")]);
        assert_eq!(download_name(&what, "home"), "home.zip");
        assert_eq!(download_name(&What::Zip(vec![dir.join("photos")]), "home"), "photos.zip");
        let serving = serve(what, "home.zip".to_string(), IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        assert_eq!(serving.total, None);
        let (head, body) = fetch(&serving.url(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        let updates = until_done(&serving);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(head.contains("Content-Type: application/zip"), "{head}");
        assert!(matches!(updates.last(), Some(Update::Done(_))));

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
        let mut names: Vec<String> = zip.file_names().map(|name| name.unwrap().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["notes.txt", "photos/", "photos/a.txt", "photos/trip/", "photos/trip/b.txt"], "no link in it");
        let mut text = String::new();
        zip.by_name("photos/trip/b.txt").unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "second");
    }

    /// The machine's own network first - a VPN up, Docker's bridge down,
    /// libvirt's up - then the bridge, then the tunnel; nothing down, and
    /// never the loopback.
    #[test]
    fn the_home_network_comes_before_a_vpn_or_a_bridge() {
        let interface = |name: &str, address: [u8; 4], prefix: u32, running: bool, point_to_point: bool| Interface {
            name: name.to_string(),
            address: Ipv4Addr::from(address),
            prefix,
            running,
            loopback: name == "lo",
            point_to_point,
        };
        let ip = |address: [u8; 4]| IpAddr::V4(Ipv4Addr::from(address));
        let found = rank(&[
            interface("lo", [127, 0, 0, 1], 8, true, false),
            interface("proton0", [10, 2, 0, 2], 32, true, true),
            interface("docker0", [172, 17, 0, 1], 16, false, false),
            interface("virbr0", [192, 168, 122, 1], 24, true, false),
            interface("eno1", [192, 168, 0, 35], 24, true, false),
            interface("wlan0", [169, 254, 3, 4], 16, true, false),
        ]);
        assert_eq!(found, [ip([192, 168, 0, 35]), ip([192, 168, 122, 1]), ip([10, 2, 0, 2]), ip([169, 254, 3, 4])]);

        // Wi-Fi and Ethernet both: each, in the system's order.
        let both = rank(&[interface("eth0", [192, 168, 1, 10], 24, true, false), interface("wlan0", [10, 0, 0, 7], 24, true, false)]);
        assert_eq!(both, [ip([192, 168, 1, 10]), ip([10, 0, 0, 7])]);
        // A tunnel known only by its name.
        assert_eq!(rank(&[interface("wg0", [10, 8, 0, 2], 24, true, false), interface("eth0", [192, 168, 1, 10], 24, true, false)])[0], ip([192, 168, 1, 10]));
    }

    /// This machine's own interfaces are read: the loopback among them, which
    /// the ranking then leaves out.
    #[cfg(unix)]
    #[test]
    fn the_interfaces_are_read() {
        let found = interfaces();
        assert!(found.iter().any(|interface| interface.loopback && interface.address.is_loopback()), "{found:?}");
        assert!(!lan_addresses().iter().any(IpAddr::is_loopback));
    }

    #[test]
    fn the_address_makes_a_code_small_enough_for_a_terminal() {
        let (width, modules) = qr("http://192.168.100.200:65535/0123456789").unwrap();
        assert_eq!(modules.len(), width * width);
        assert!(width <= 29, "version 3 at most: {width}");
        // The finder pattern at the top left: its corner dark.
        assert!(modules[0] && modules[6] && !modules[7]);
    }
}
