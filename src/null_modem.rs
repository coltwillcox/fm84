//! Alt+N and Alt+Shift+N: the null modem - files sent from one fm84 to
//! another on the same network, panel to panel, as LapLink sent them down a
//! cable.
//!
//! The receiving end listens only while its popup is open: a socket on
//! PORT answers machines looking for it, and a server on a port the system
//! gives takes what they send, one sender at a time, into the directory its
//! panel is in. Unless F11 says otherwise, each sender is asked about first -
//! who, how many files, how big - and nothing is written until the answer is
//! yes.
//!
//! The talk between them is lines of text, fields between tabs, names
//! written so no tab or line end can be in them; a file's bytes follow its
//! line as they are. A file is written beside where it goes and put there
//! once it is whole, so one given up on halfway leaves nothing. What would
//! be written over is said to the sender, who asks there, as a copy asks.
//! Nothing but the standard library's sockets.

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The port a listening fm84 answers on, to be found. '84.
pub const PORT: u16 = 18484;

/// What one asks the network, and how an answer to it starts.
const QUERY: &[u8] = b"FM84 NULL MODEM 1 ?";
const ANSWER: &str = "FM84 NULL MODEM 1 !";
/// The first line a sender says, which is how a listener knows it.
const GREETING: &str = "FM84 NULL MODEM 1";

/// How often a wait looks up to see whether it has been told to stop.
const TICK: Duration = Duration::from_millis(250);
/// How long a line may be waited for: the other side may be asking its user
/// whether to write over a file, and a user may take a while.
const IDLE: Duration = Duration::from_secs(60 * 60);
/// How long a file's bytes may stop coming before it is given up on.
const STALL: Duration = Duration::from_secs(60);
/// How long a sender waits on the question to the receiving user.
pub const ASK_FOR: Duration = Duration::from_secs(120);

/// An fm84 listening somewhere: where to reach it, who it is, and the name
/// of the directory it takes files into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub address: SocketAddr,
    pub name: String,
    pub dir: String,
}

/// Looking for listening fm84s, asking the network again every second.
/// Dropped, it stops.
pub struct Scanner {
    pub found: Receiver<Peer>,
    stop: Arc<AtomicBool>,
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Ask every network the machine is on - and the machine itself, for a
/// second fm84 on it - for whatever listens on `port`.
pub fn scan(port: u16) -> io::Result<Scanner> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(TICK))?;
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, found) = mpsc::channel();
    let stopped = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut asked: Option<Instant> = None;
        let mut buffer = [0u8; 1024];
        while !stopped.load(Ordering::Relaxed) {
            if asked.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) {
                let mut targets = vec![Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST];
                targets.extend(crate::share::broadcast_addresses());
                for target in targets {
                    let _ = socket.send_to(QUERY, (target, port));
                }
                asked = Some(Instant::now());
            }
            if let Ok((read, from)) = socket.recv_from(&mut buffer)
                && let Some(peer) = parse_answer(&buffer[..read], from)
                && sender.send(peer).is_err()
            {
                return;
            }
        }
    });
    Ok(Scanner { found, stop })
}

/// What a listener says when asked: who it is, where its server is, and the
/// directory it takes files into.
fn answer_text(tcp_port: u16, name: &str, dir: &str) -> String {
    format!("{ANSWER}\n{tcp_port}\n{}\n{}", encode(name.as_bytes()), encode(dir.as_bytes()))
}

/// An answer heard from `from`, as the peer it describes. None for anything
/// else that came to the socket.
pub fn parse_answer(bytes: &[u8], from: SocketAddr) -> Option<Peer> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split('\n');
    if lines.next()? != ANSWER {
        return None;
    }
    let port: u16 = lines.next()?.parse().ok().filter(|&port| port != 0)?;
    let name = decoded_text(lines.next()?)?;
    let dir = decoded_text(lines.next()?)?;
    Some(Peer {
        address: SocketAddr::new(from.ip(), port),
        name,
        dir,
    })
}

/// What a listening fm84 has to say, as it says it.
pub enum Update {
    /// A sender asks to send, and waits on the answer: true to take it.
    Asking { from: String, files: u64, bytes: u64, answer: Sender<bool> },
    /// A sender has been let in.
    Connected { from: String },
    /// A file is coming, named so, of so many bytes.
    Started { name: String, size: u64 },
    /// So many bytes of it are here.
    Got(u64),
    /// It is whole, and in its place.
    Saved { size: u64 },
    /// It did not come, or could not be put where it was to go, and why.
    Lost { name: String, reason: String },
    /// Something that was not there before - a file, or a directory with
    /// whatever then came into it - for Ctrl+Z to take back.
    Created(PathBuf),
    /// The sender is gone: said goodbye, or did not.
    Ended { from: String, complete: bool },
    /// The server could not go on.
    Failed(String),
}

/// A directory taking files from other fm84s. Dropped, it stops.
pub struct Listening {
    pub dir: PathBuf,
    /// The port its server is on, and the one it is found on - PORT, but
    /// for the tests, which ask the system for one each.
    pub port: u16,
    #[cfg_attr(not(test), allow(dead_code))]
    pub beacon_port: u16,
    pub updates: Receiver<Update>,
    stop: Arc<AtomicBool>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What every thread of a listener needs.
struct Shared {
    dir: PathBuf,
    ask: bool,
    stop: Arc<AtomicBool>,
    updates: Sender<Update>,
    /// Someone is sending, and the next is told so.
    busy: AtomicBool,
}

/// Take files into `dir` from other fm84s, as `name`, found on
/// `beacon_port` - PORT, or 0 for any. `ask` holds each sender until the
/// user says yes.
pub fn listen(dir: PathBuf, name: String, ask: bool, beacon_port: u16) -> io::Result<Listening> {
    let beacon = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, beacon_port)).map_err(|error| match error.kind() {
        io::ErrorKind::AddrInUse => io::Error::new(io::ErrorKind::AddrInUse, t!("Another null modem is listening on this machine")),
        _ => error,
    })?;
    beacon.set_read_timeout(Some(TICK))?;
    let beacon_port = beacon.local_addr()?.port();
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();

    let stop = Arc::new(AtomicBool::new(false));
    let (sender, updates) = mpsc::channel();
    let shown = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
    let answer = answer_text(port, &name, &shown);
    let stopped = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut buffer = [0u8; 64];
        while !stopped.load(Ordering::Relaxed) {
            if let Ok((read, from)) = beacon.recv_from(&mut buffer)
                && &buffer[..read] == QUERY
            {
                let _ = beacon.send_to(answer.as_bytes(), from);
            }
        }
    });
    let shared = Arc::new(Shared {
        dir: dir.clone(),
        ask,
        stop: Arc::clone(&stop),
        updates: sender,
        busy: AtomicBool::new(false),
    });
    std::thread::spawn(move || accept_all(listener, shared));
    Ok(Listening { dir, port, beacon_port, updates, stop })
}

/// Take connections until told to stop, each on a thread of its own.
fn accept_all(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || {
                    let _ = session(stream, &shared);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = shared.updates.send(Update::Failed(error.to_string()));
                return;
            }
        }
    }
}

/// Lets the next sender in once this one is done, however it ends.
struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// One sender, from its greeting to its goodbye.
fn session(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(TICK))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let stop = &shared.stop;
    if read_line(&mut reader, Duration::from_secs(10), stop)? != GREETING.as_bytes() {
        return Ok(());
    }
    let hello = read_line(&mut reader, Duration::from_secs(10), stop)?;
    let fields = fields(&hello);
    let (Some(b"HELLO"), Some(from), Some(files), Some(bytes)) = (fields.first().copied(), fields.get(1), fields.get(2).and_then(|field| number(field)), fields.get(3).and_then(|field| number(field))) else {
        return say(&mut writer, &["REFUSE", t!("Not understood")]);
    };
    let from = String::from_utf8_lossy(&decode(from).unwrap_or_default()).into_owned();
    if shared.busy.swap(true, Ordering::Relaxed) {
        return say(&mut writer, &["REFUSE", t!("Busy with another sender")]);
    }
    let _busy = Busy(&shared.busy);
    if let Some(free) = crate::fs_ops::free_space(&shared.dir)
        && bytes > free
    {
        return say(&mut writer, &["REFUSE", &t!("Not enough room: {free} free", free = crate::utils::format_size(free))]);
    }
    if shared.ask {
        let (answer, answered) = mpsc::channel();
        let _ = shared.updates.send(Update::Asking { from: from.clone(), files, bytes, answer });
        let asked = Instant::now();
        let yes = loop {
            match answered.recv_timeout(TICK) {
                Ok(yes) => break yes,
                Err(mpsc::RecvTimeoutError::Timeout) if !stop.load(Ordering::Relaxed) && asked.elapsed() < ASK_FOR => {}
                Err(_) => break false,
            }
        };
        if !yes {
            return say(&mut writer, &["REFUSE", t!("Refused")]);
        }
    }
    let shown = shared.dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| shared.dir.display().to_string());
    say(&mut writer, &["ACCEPT", &encode(shown.as_bytes())])?;
    let _ = shared.updates.send(Update::Connected { from: from.clone() });

    let complete = take_all(&mut reader, &mut writer, shared).unwrap_or(false);
    let _ = shared.updates.send(Update::Ended { from, complete });
    Ok(())
}

/// What a sender says once let in, until it says goodbye: true for one that
/// did, false for one that went without a word.
fn take_all(reader: &mut BufReader<TcpStream>, writer: &mut TcpStream, shared: &Shared) -> io::Result<bool> {
    // Directories made here, whose contents were not there before either.
    let mut made: HashSet<PathBuf> = HashSet::new();
    loop {
        let line = read_line(reader, IDLE, &shared.stop)?;
        let fields = fields(&line);
        match fields.first().copied() {
            Some(b"END") => {
                say(writer, &["BYE"])?;
                return Ok(true);
            }
            Some(b"DIR") => {
                let Some(target) = fields.get(1).and_then(|path| relative(path)).map(|path| shared.dir.join(path)) else {
                    say(writer, &["ERR", t!("Not a name to save it under")])?;
                    continue;
                };
                match std::fs::symlink_metadata(&target) {
                    Ok(metadata) if metadata.is_dir() => say(writer, &["OK"])?,
                    Ok(_) => say(writer, &["ERR", &t!("Cannot replace file {path} with a directory", path = target.display())])?,
                    Err(_) => match std::fs::create_dir(&target) {
                        Ok(()) => {
                            if !target.parent().is_some_and(|parent| made.contains(parent)) {
                                let _ = shared.updates.send(Update::Created(target.clone()));
                            }
                            made.insert(target);
                            say(writer, &["OK"])?;
                        }
                        Err(error) => say(writer, &["ERR", &error.to_string()])?,
                    },
                }
            }
            Some(b"FILE") => {
                let target = fields.get(1).and_then(|path| relative(path)).map(|path| shared.dir.join(path));
                let size = fields.get(2).and_then(|size| number(size));
                let (Some(target), Some(size)) = (target, size) else {
                    say(writer, &["ERR", t!("Not a name to save it under")])?;
                    continue;
                };
                let modified = fields.get(3).and_then(|time| time_from(time));
                let mode = fields.get(4).and_then(|mode| std::str::from_utf8(mode).ok()).and_then(|mode| u32::from_str_radix(mode, 8).ok());
                match std::fs::symlink_metadata(&target) {
                    Ok(metadata) if metadata.is_dir() => {
                        say(writer, &["ERR", &t!("Cannot replace directory {path} with a file", path = target.display())])?;
                        continue;
                    }
                    Ok(metadata) => say(writer, &["EXISTS", &metadata.len().to_string(), &time_text(metadata.modified().ok())])?,
                    Err(_) => say(writer, &["NEW"])?,
                }
                let line = read_line(reader, IDLE, &shared.stop)?;
                let fields = self::fields(&line);
                let how = match (fields.first().copied(), fields.get(1).copied()) {
                    (Some(b"SKIP"), _) => continue,
                    (Some(b"SEND"), Some(b"replace")) => How::Replace,
                    (Some(b"SEND"), Some(b"keep-both")) => How::KeepBoth,
                    (Some(b"SEND"), _) => How::New,
                    _ => return Ok(false),
                };
                let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let _ = shared.updates.send(Update::Started { name: name.clone(), size });
                match save(reader, &target, size, modified, mode, how, shared) {
                    Ok(path) => {
                        let saved = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                        // A file written over was there before, and is not
                        // for Ctrl+Z to delete.
                        if path != target || how != How::Replace {
                            let inside_made = path.parent().is_some_and(|parent| made.contains(parent));
                            if !inside_made {
                                let _ = shared.updates.send(Update::Created(path.clone()));
                            }
                        }
                        let _ = shared.updates.send(Update::Saved { size });
                        say(writer, &["SAVED", &encode(saved.as_bytes())])?;
                    }
                    Err(Saving::Here(error)) => {
                        let _ = shared.updates.send(Update::Lost { name, reason: error.to_string() });
                        say(writer, &["ERR", &error.to_string()])?;
                    }
                    Err(Saving::Link(error)) => {
                        let _ = shared.updates.send(Update::Lost { name, reason: error.to_string() });
                        return Err(error);
                    }
                }
            }
            _ => return Ok(false),
        }
    }
}

/// How a file sent is put where it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// Nothing was there; if something is by now, it is kept and this goes
    /// beside it.
    New,
    /// Write over what is there.
    Replace,
    /// Keep what is there, and put this beside it as `name (2).ext`.
    KeepBoth,
}

impl How {
    fn word(self) -> &'static str {
        match self {
            How::New => "new",
            How::Replace => "replace",
            How::KeepBoth => "keep-both",
        }
    }
}

/// Why a file was not saved: something here - the disk, a name taken -
/// after which the sender goes on, or the connection, after which it
/// cannot.
enum Saving {
    Here(io::Error),
    Link(io::Error),
}

/// `size` bytes from the sender, into a file beside `target`, then put there
/// as `how` says. Where it went.
fn save(reader: &mut BufReader<TcpStream>, target: &Path, size: u64, modified: Option<SystemTime>, mode: Option<u32>, how: How, shared: &Shared) -> Result<PathBuf, Saving> {
    // Somewhere to write to, or the bytes read and thrown away, so the talk
    // can go on past them.
    let (temp, mut file) = match crate::fs_ops::temp_beside(target) {
        Ok(made) => made,
        Err(error) => {
            drain(reader, size, &shared.stop).map_err(Saving::Link)?;
            return Err(Saving::Here(error));
        }
    };
    let mut buffer = vec![0; 64 * 1024];
    let mut left = size;
    let mut heard = Instant::now();
    let mut failed: Option<io::Error> = None;
    while left > 0 {
        if shared.stop.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&temp);
            return Err(Saving::Link(io::Error::new(io::ErrorKind::Interrupted, t!("Stopped"))));
        }
        let want = buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        match reader.read(&mut buffer[..want]) {
            Ok(0) => {
                drop(file);
                let _ = std::fs::remove_file(&temp);
                return Err(Saving::Link(io::Error::new(io::ErrorKind::UnexpectedEof, t!("It stopped coming before its end"))));
            }
            Ok(read) => {
                // A write that fails leaves the rest to be read and dropped.
                if failed.is_none()
                    && let Err(error) = file.write_all(&buffer[..read])
                {
                    failed = Some(error);
                }
                left -= read as u64;
                heard = Instant::now();
                let _ = shared.updates.send(Update::Got(size - left));
            }
            Err(error) if waiting(&error) && heard.elapsed() < STALL => {}
            Err(error) => {
                drop(file);
                let _ = std::fs::remove_file(&temp);
                return Err(Saving::Link(error));
            }
        }
    }
    if let Some(error) = failed {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(Saving::Here(error));
    }
    if let Some(modified) = modified {
        let _ = file.set_times(std::fs::FileTimes::new().set_modified(modified));
    }
    let synced = file.sync_all();
    drop(file);
    if let Err(error) = synced {
        let _ = std::fs::remove_file(&temp);
        return Err(Saving::Here(error));
    }
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode & 0o777));
    }
    #[cfg(not(unix))]
    let _ = mode;
    // A name free now, and put there only if it still is.
    let mut tries = 0;
    loop {
        let (to, replacing) = match how {
            How::Replace => (target.to_path_buf(), true),
            How::New | How::KeepBoth if crate::fs_ops::path_exists(target) => (crate::fs_ops::free_name_among(target, crate::fs_ops::path_exists), false),
            How::New | How::KeepBoth => (target.to_path_buf(), false),
        };
        match crate::fs_ops::place(&temp, &to, replacing) {
            Ok(()) => return Ok(to),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && tries < 5 => tries += 1,
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                return Err(Saving::Here(error));
            }
        }
    }
}

/// `size` bytes read and dropped.
fn drain(reader: &mut BufReader<TcpStream>, size: u64, stop: &AtomicBool) -> io::Result<()> {
    let mut buffer = vec![0; 64 * 1024];
    let mut left = size;
    let mut heard = Instant::now();
    while left > 0 {
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, t!("Stopped")));
        }
        let want = buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        match reader.read(&mut buffer[..want]) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, t!("It stopped coming before its end"))),
            Ok(read) => {
                left -= read as u64;
                heard = Instant::now();
            }
            Err(error) if waiting(&error) && heard.elapsed() < STALL => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The sending end of a null modem: connected, and let in.
pub struct Link {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    /// The name of the directory the files go into, over there.
    pub dir: String,
}

/// What the other end says of a file about to be sent: nothing there, or
/// something of this size and age.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum There {
    Nothing,
    Something { size: u64, modified: Option<SystemTime> },
}

/// What a call on a link came to: the connection gone, which ends it, or the
/// other end's answer - which may be that it could not, and why.
pub type Said<T> = io::Result<Result<T, String>>;

impl Link {
    /// Connect to `address`, and ask to send `files` files of `bytes` in all
    /// as `from` - waiting, as long as the other end's user may take to
    /// answer, unless `stop` is set. Refused, the reason is the error.
    pub fn dial(address: SocketAddr, from: &str, files: u64, bytes: u64, stop: &AtomicBool) -> io::Result<Link> {
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
        stream.set_read_timeout(Some(TICK))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        let _ = stream.set_nodelay(true);
        let mut writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        say(&mut writer, &[GREETING])?;
        say(&mut writer, &["HELLO", &encode(from.as_bytes()), &files.to_string(), &bytes.to_string()])?;
        let line = read_line(&mut reader, ASK_FOR + Duration::from_secs(10), stop)?;
        let fields = fields(&line);
        match fields.first().copied() {
            Some(b"ACCEPT") => {
                let dir = fields.get(1).and_then(|dir| decode(dir)).map(|dir| String::from_utf8_lossy(&dir).into_owned()).unwrap_or_default();
                Ok(Link { reader, writer, dir })
            }
            Some(b"REFUSE") => Err(io::Error::new(io::ErrorKind::PermissionDenied, reason(&fields))),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, t!("Not understood"))),
        }
    }

    /// A directory, at `path` under the one the files go into.
    pub fn make_dir(&mut self, path: &Path) -> Said<()> {
        say(&mut self.writer, &["DIR", &encode_path(path)])?;
        let line = self.answer()?;
        let fields = fields(&line);
        Ok(match fields.first().copied() {
            Some(b"OK") => Ok(()),
            _ => Err(reason(&fields)),
        })
    }

    /// A file of `size` about to be sent to `path`, with its time and its
    /// permissions: what is in its way there.
    pub fn offer(&mut self, path: &Path, size: u64, modified: Option<SystemTime>, mode: Option<u32>) -> Said<There> {
        let mode = mode.map_or_else(|| "-".to_string(), |mode| format!("{mode:o}"));
        say(&mut self.writer, &["FILE", &encode_path(path), &size.to_string(), &time_text(modified), &mode])?;
        let line = self.answer()?;
        let fields = fields(&line);
        Ok(match fields.first().copied() {
            Some(b"NEW") => Ok(There::Nothing),
            Some(b"EXISTS") => Ok(There::Something {
                size: fields.get(1).and_then(|size| number(size)).unwrap_or(0),
                modified: fields.get(2).and_then(|time| time_from(time)),
            }),
            _ => Err(reason(&fields)),
        })
    }

    /// The file offered is not sent after all.
    pub fn skip(&mut self) -> io::Result<()> {
        say(&mut self.writer, &["SKIP"])
    }

    /// The file offered, `size` bytes of `source`, put there as `how` says;
    /// `step` hears of each chunk as it goes, and stops it by saying false.
    /// The name it was saved under, or None when stopped - the link is no
    /// use after that, and is dropped.
    pub fn send(&mut self, how: How, source: &mut dyn Read, size: u64, step: &mut dyn FnMut(u64) -> bool) -> Said<Option<String>> {
        say(&mut self.writer, &["SEND", how.word()])?;
        let mut buffer = vec![0; 64 * 1024];
        let mut left = size;
        while left > 0 {
            let want = buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
            let read = match source.read(&mut buffer[..want]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, t!("The file changed while it was being sent"))),
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            self.writer.write_all(&buffer[..read])?;
            left -= read as u64;
            if !step(read as u64) {
                return Ok(Ok(None));
            }
        }
        self.writer.flush()?;
        let line = self.answer()?;
        let fields = fields(&line);
        Ok(match fields.first().copied() {
            Some(b"SAVED") => Ok(Some(fields.get(1).and_then(|name| decode(name)).map(|name| String::from_utf8_lossy(&name).into_owned()).unwrap_or_default())),
            _ => Err(reason(&fields)),
        })
    }

    /// Goodbye, and the other end's to it.
    pub fn end(mut self) -> io::Result<()> {
        say(&mut self.writer, &["END"])?;
        let line = self.answer()?;
        if line == b"BYE" { Ok(()) } else { Err(io::Error::new(io::ErrorKind::InvalidData, t!("Not understood"))) }
    }

    /// The other end's next line. It answers at once - only the bytes of a
    /// large file being written out take it a while.
    fn answer(&mut self) -> io::Result<Vec<u8>> {
        read_line(&mut self.reader, STALL, &AtomicBool::new(false))
    }
}

/// A line written: the fields between tabs, and the end of the line.
fn say(writer: &mut TcpStream, fields: &[&str]) -> io::Result<()> {
    let line = fields.iter().map(|field| field.replace(['\t', '\n', '\r'], " ")).collect::<Vec<_>>().join("\t") + "\n";
    writer.write_all(line.as_bytes())
}

/// The next line, without its end; read across the short timeouts the
/// socket has so a stop is heard, given up on after `limit`.
fn read_line(reader: &mut BufReader<TcpStream>, limit: Duration, stop: &AtomicBool) -> io::Result<Vec<u8>> {
    const LONGEST: usize = 64 * 1024;
    let started = Instant::now();
    let mut line = Vec::new();
    loop {
        match reader.read_until(b'\n', &mut line) {
            Ok(_) if line.ends_with(b"\n") => {
                line.pop();
                return Ok(line);
            }
            Ok(_) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, t!("The other side hung up"))),
            Err(error) if waiting(&error) || error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
        if line.len() > LONGEST {
            return Err(io::Error::new(io::ErrorKind::InvalidData, t!("Not understood")));
        }
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, t!("Stopped")));
        }
        if started.elapsed() > limit {
            return Err(io::Error::new(io::ErrorKind::TimedOut, t!("No answer")));
        }
    }
}

/// A read that found nothing yet, as the timeout set on a socket reports it
/// - one way on Unix, another on Windows.
fn waiting(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

fn fields(line: &[u8]) -> Vec<&[u8]> {
    line.split(|&byte| byte == b'\t').collect()
}

fn number(field: &[u8]) -> Option<u64> {
    std::str::from_utf8(field).ok()?.parse().ok()
}

/// The reason after a refusal or an error, as it was given.
fn reason(fields: &[&[u8]]) -> String {
    match fields.get(1) {
        Some(reason) => String::from_utf8_lossy(reason).into_owned(),
        None => t!("Not understood").to_string(),
    }
}

/// A time as seconds and nanoseconds since 1970, or `-` for none - or one
/// before it.
fn time_text(time: Option<SystemTime>) -> String {
    match time.and_then(|time| time.duration_since(UNIX_EPOCH).ok()) {
        Some(since) => format!("{}.{:09}", since.as_secs(), since.subsec_nanos()),
        None => "-".to_string(),
    }
}

fn time_from(field: &[u8]) -> Option<SystemTime> {
    let text = std::str::from_utf8(field).ok()?;
    let (seconds, nanos) = text.split_once('.').unwrap_or((text, "0"));
    let since = Duration::new(seconds.parse().ok()?, nanos.parse().ok().filter(|&nanos| nanos < 1_000_000_000)?);
    UNIX_EPOCH.checked_add(since)
}

/// Bytes as they go in a line: a `%`, a separator, a tab, a line end or any
/// other control written as `%` and two hex digits.
fn encode(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if byte == b'%' || byte == b'/' || byte < 0x20 || byte == 0x7f {
            out.extend(format!("%{byte:02X}").bytes());
        } else {
            out.push(byte);
        }
    }
    // Bytes that are not UTF-8 - a name on Unix may be anything - are
    // written as %XX too, so the line stays text.
    match String::from_utf8(out) {
        Ok(text) => text,
        Err(error) => {
            let mut text = String::new();
            for chunk in error.as_bytes().utf8_chunks() {
                text.push_str(chunk.valid());
                for &byte in chunk.invalid() {
                    text.push_str(&format!("%{byte:02X}"));
                }
            }
            text
        }
    }
}

fn decode(field: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(field.len());
    let mut at = 0;
    while at < field.len() {
        if field[at] == b'%' {
            let hex = std::str::from_utf8(field.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(field[at]);
            at += 1;
        }
    }
    Some(out)
}

fn decoded_text(field: &str) -> Option<String> {
    decode(field.as_bytes()).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// A path under the directory the files go into, each part encoded, `/`
/// between them.
fn encode_path(path: &Path) -> String {
    path.components().map(|part| encode(&os_bytes(part.as_os_str()))).collect::<Vec<_>>().join("/")
}

#[cfg(unix)]
fn os_bytes(text: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    text.as_bytes().to_vec()
}
#[cfg(not(unix))]
fn os_bytes(text: &std::ffi::OsStr) -> Vec<u8> {
    text.to_string_lossy().into_owned().into_bytes()
}

/// A path sent, made a path under the directory the files go into - and
/// only under it: None for one that is empty, absolute, climbs out with
/// `..`, or has a part no filesystem takes as a name.
pub fn relative(encoded: &[u8]) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in encoded.split(|&byte| byte == b'/') {
        let bytes = decode(part)?;
        if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.iter().any(|&byte| byte == b'/' || byte == 0) {
            return None;
        }
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(&bytes).to_os_string()
        };
        #[cfg(not(unix))]
        let name = {
            let name = String::from_utf8(bytes).ok()?;
            if name.contains(['\\', ':']) {
                return None;
            }
            std::ffi::OsString::from(name)
        };
        path.push(name);
    }
    let plain = path.components().all(|part| matches!(part, Component::Normal(_)));
    (plain && path.components().next().is_some()).then_some(path)
}

/// Whether `address` is the machine's own, reached through its loopback.
pub fn is_loopback(address: &SocketAddr) -> bool {
    match address.ip() {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}

#[cfg(test)]
mod tests;
