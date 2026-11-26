//! Alt+Shift+Q: the opposite of Alt+Q - a phone on the same network sends
//! files into the directory the panel is in. A server of its own, on a port
//! the system gives it and a path no one can guess, gives the phone a page
//! to choose files on, and takes each one as it is sent: a request of its
//! own, its bytes as they are, its name in the path. Each is written beside
//! where it goes and put there once it is whole, so one given up on halfway
//! leaves nothing; a name already taken is kept beside, never written over.
//! Until Esc stops it.

use crate::share::{Request, read_request};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// Files coming in at once: the page sends one at a time, and anything past
/// a few is not a phone.
const AT_ONCE: usize = 4;

/// What the server has to say, as it says it. Each file coming in is told
/// apart by a number of its own, for several at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A file is coming, named so, of so many bytes.
    Started { id: u64, name: String, size: u64 },
    /// So many bytes of it are here.
    Got { id: u64, got: u64 },
    /// It is whole, and in its place at `path`.
    Saved { id: u64, path: PathBuf, size: u64 },
    /// It did not come, and why.
    Lost { id: u64, name: String, reason: String },
    /// The server could not go on.
    Failed(String),
}

/// A directory taking files. Dropped, it stops.
pub struct Receiving {
    pub dir: PathBuf,
    pub port: u16,
    token: String,
    pub updates: Receiver<Update>,
    stop: Arc<AtomicBool>,
}

impl Receiving {
    /// What a phone opens to send to it at `address`.
    pub fn url(&self, address: IpAddr) -> String {
        format!("http://{}/{}", SocketAddr::new(address, self.port), self.token)
    }
}

impl Drop for Receiving {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Take files into `dir` at `address` - every address the machine has, for
/// the unspecified one - on a port the system gives, under a path made up
/// now; what comes in is said on its updates.
pub fn receive(dir: PathBuf, address: IpAddr) -> io::Result<Receiving> {
    let listener = TcpListener::bind(SocketAddr::new(address, 0))?;
    listener.set_nonblocking(true)?;
    let token = crate::share::token();
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, updates) = mpsc::channel();
    let shared = Arc::new(Shared {
        dir: dir.clone(),
        token: token.clone(),
        page: page(&dir),
        stop: Arc::clone(&stop),
        updates: sender,
        coming: AtomicUsize::new(0),
        next: AtomicU64::new(0),
    });
    std::thread::spawn(move || listen(listener, shared));
    Ok(Receiving { dir, port, token, updates, stop })
}

/// What every connection's thread needs to answer.
struct Shared {
    dir: PathBuf,
    token: String,
    page: String,
    stop: Arc<AtomicBool>,
    updates: Sender<Update>,
    /// Files coming in now.
    coming: AtomicUsize,
    /// The number the next file is told apart by.
    next: AtomicU64,
}

/// Take connections until told to stop, each on a thread of its own.
fn listen(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || answer(stream, &shared));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = shared.updates.send(Update::Failed(error.to_string()));
                return;
            }
        }
    }
}

/// A reply with nothing to it but what it says.
fn reply(stream: &mut TcpStream, status: &str, text: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{text}",
        text.len()
    );
}

/// One connection: the page, a file sent to the path under it, or nothing.
fn answer(mut stream: TcpStream, shared: &Shared) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let base = format!("/{}", shared.token);
    match (request.method.as_str(), request.path.strip_prefix(&base)) {
        ("GET", Some("" | "/")) => {
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{}",
                shared.page.len(),
                shared.page
            );
        }
        ("PUT", Some(rest)) if rest.starts_with('/') && rest.len() > 1 => take(&mut stream, &request, &rest[1..], shared),
        _ => reply(&mut stream, "404 Not Found", ""),
    }
}

/// A file sent: its name from the path, its bytes from the body, as many as
/// the head says. Written beside its place and put there whole.
fn take(stream: &mut TcpStream, request: &Request, encoded: &str, shared: &Shared) {
    let Some(name) = file_name(&percent_decoded(encoded)) else {
        return reply(stream, "400 Bad Request", t!("No name to save it under"));
    };
    // Its size, said first: a body that only ends when the phone stops
    // sending could be anything.
    let Some(size) = request.header("Content-Length").and_then(|length| length.parse::<u64>().ok()) else {
        return reply(stream, "411 Length Required", t!("No size given"));
    };
    if let Some(free) = crate::fs_ops::free_space(&shared.dir)
        && size > free
    {
        let said = t!("Not enough room: {free} free", free = crate::utils::format_size(free));
        let _ = shared.updates.send(Update::Lost {
            id: shared.next.fetch_add(1, Ordering::Relaxed),
            name,
            reason: said.clone(),
        });
        return reply(stream, "507 Insufficient Storage", &said);
    }
    if shared.coming.fetch_add(1, Ordering::Relaxed) >= AT_ONCE {
        shared.coming.fetch_sub(1, Ordering::Relaxed);
        return reply(stream, "503 Service Unavailable", t!("Too many at once"));
    }
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    let _ = shared.updates.send(Update::Started { id, name: name.clone(), size });
    let outcome = save(stream, request, &name, size, id, shared);
    shared.coming.fetch_sub(1, Ordering::Relaxed);
    match outcome {
        Ok(path) => {
            let saved = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let _ = shared.updates.send(Update::Saved { id, path, size });
            reply(stream, "201 Created", &saved);
        }
        Err(error) => {
            let reason = error.to_string();
            let _ = shared.updates.send(Update::Lost { id, name, reason: reason.clone() });
            reply(stream, "500 Internal Server Error", &reason);
        }
    }
}

/// The body, `size` bytes of it, into a file beside where it goes, then put
/// there - under a name of its own when the one it came with is taken. Less
/// than `size`, and nothing is left.
fn save(stream: &mut TcpStream, request: &Request, name: &str, size: u64, id: u64, shared: &Shared) -> io::Result<PathBuf> {
    let wanted = shared.dir.join(name);
    let (temp, mut file) = crate::fs_ops::temp_beside(&wanted)?;
    let written = (|| -> io::Result<()> {
        let mut body = request.body_start.as_slice().chain(&mut *stream).take(size);
        let mut buffer = vec![0; 64 * 1024];
        let mut got = 0;
        while got < size {
            if shared.stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, t!("Stopped")));
            }
            let read = body.read(&mut buffer)?;
            if read == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, t!("It stopped coming before its end")));
            }
            file.write_all(&buffer[..read])?;
            got += read as u64;
            let _ = shared.updates.send(Update::Got { id, got });
        }
        file.sync_all()
    })();
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    // A name free now, and put there only if it still is.
    let mut tries = 0;
    loop {
        let target = if crate::fs_ops::path_exists(&wanted) {
            crate::fs_ops::free_name_among(&wanted, crate::fs_ops::path_exists)
        } else {
            wanted.clone()
        };
        match crate::fs_ops::place(&temp, &target, false) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && tries < 5 => tries += 1,
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                return Err(error);
            }
        }
    }
}

/// `%XX` in a path made the bytes they stand for; as UTF-8, any that are not
/// put right.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes.get(at + 1..at + 3).and_then(|pair| std::str::from_utf8(pair).ok()).filter(|pair| pair.chars().all(|c| c.is_ascii_hexdigit()));
        match (bytes[at], hex.and_then(|pair| u8::from_str_radix(pair, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The name a file sent with `sent` is saved under, in the directory and no
/// deeper: the last part of whatever path it came with, what no filesystem
/// takes in a name - a separator, a control character, Windows' reserved
/// signs - made a dash, and no longer than a name may be. None for one that
/// is nothing, or only dots.
pub fn file_name(sent: &str) -> Option<String> {
    let last = sent.rsplit(['/', '\\']).next().unwrap_or_default();
    let fit: String = last.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
    let fit = fit.trim().trim_end_matches(['.', ' ']).to_string();
    if fit.is_empty() || fit.chars().all(|c| c == '.') {
        return None;
    }
    Some(shortened(&fit, 255))
}

/// `name` no longer than `most` bytes, cut from the end of its stem so its
/// extension stays.
fn shortened(name: &str, most: usize) -> String {
    if name.len() <= most {
        return name.to_string();
    }
    let (stem, extension) = match name.rfind('.').filter(|&at| at > 0 && name.len() - at <= 16) {
        Some(at) => name.split_at(at),
        None => (name, ""),
    };
    let mut kept = String::new();
    for character in stem.chars() {
        if kept.len() + character.len_utf8() + extension.len() > most {
            break;
        }
        kept.push(character);
    }
    kept + extension
}

/// Text as HTML takes it, and as a JavaScript string between double quotes.
fn html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn script_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The page a phone opens: a button to choose files with, each sent in turn
/// as it is chosen, with a bar of how far it has got and what came of it -
/// in fm84's own language.
fn page(dir: &Path) -> String {
    let into = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
    let title = t!("Send files to fm84");
    let heading = t!("Into {dir}", dir = crate::utils::printable_name(&into));
    let choose = t!("Choose files");
    let sent = t!("Sent");
    let lost = t!("Not sent");
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
body{{font-family:system-ui,sans-serif;margin:0;padding:1.5em;background:#1b1029;color:#f4e9ff}}
h1{{font-size:1.3em;margin:0 0 .3em}}p{{opacity:.8;margin:0 0 1.5em;word-break:break-all}}
label{{display:block;text-align:center;padding:1.2em;border-radius:.8em;background:#ff2a6d;color:#fff;font-size:1.2em;font-weight:bold}}
ul{{list-style:none;padding:0}}li{{margin:1em 0;word-break:break-all}}progress{{width:100%;height:1em}}
.ok{{color:#05d9e8}}.no{{color:#ff8fa3}}
</style></head><body>
<h1>{title_html}</h1><p>{heading}</p>
<label>{choose}<input id="files" type="file" multiple hidden></label>
<ul id="list"></ul>
<script>
const sent={sent}, lost={lost};
const files=document.getElementById("files"), list=document.getElementById("list");
files.onchange=async()=>{{
  for(const file of [...files.files]){{
    const item=document.createElement("li"), bar=document.createElement("progress"), said=document.createElement("div");
    item.textContent=file.name; bar.max=file.size||1; item.append(bar,said); list.prepend(item);
    await new Promise(done=>{{
      const request=new XMLHttpRequest();
      request.open("PUT",location.pathname.replace(/\/$/,"")+"/"+encodeURIComponent(file.name));
      request.upload.onprogress=e=>bar.value=e.loaded;
      request.onload=()=>{{const ok=request.status<300;said.className=ok?"ok":"no";said.textContent=ok?sent:lost+": "+request.responseText;bar.value=ok?bar.max:bar.value;done();}};
      request.onerror=()=>{{said.className="no";said.textContent=lost;done();}};
      request.send(file);
    }});
  }}
  files.value="";
}};
</script></body></html>"#,
        title = html(title),
        title_html = html(title),
        heading = html(&heading),
        choose = html(choose),
        sent = script_string(sent),
        lost = script_string(lost),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fm84-receive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A request as the page makes it, and the status line and body of what
    /// comes back.
    fn send(url: &str, method: &str, path: &str, head: &str, body: &[u8]) -> (String, String) {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, base) = rest.split_at(rest.find('/').unwrap());
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(format!("{method} {base}{path} HTTP/1.1\r\nHost: {host}\r\n{head}\r\n").as_bytes()).unwrap();
        let _ = stream.write_all(body);
        let mut all = Vec::new();
        stream.read_to_end(&mut all).unwrap();
        let text = String::from_utf8_lossy(&all).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        (head.lines().next().unwrap_or_default().to_string(), body.to_string())
    }

    fn put(url: &str, name: &str, body: &[u8]) -> (String, String) {
        send(url, "PUT", &format!("/{name}"), &format!("Content-Length: {}\r\n", body.len()), body)
    }

    #[test]
    fn the_page_is_given_and_files_sent_are_saved() {
        let dir = scratch("saved");
        let receiving = receive(dir.clone(), IpAddr::V4(Ipv4Addr::UNSPECIFIED)).unwrap();
        let url = receiving.url(IpAddr::V4(Ipv4Addr::LOCALHOST));

        let (status, page) = send(&url, "GET", "", "", b"");
        assert!(status.contains("200"), "{status}");
        assert!(page.contains("type=\"file\" multiple") && page.contains("XMLHttpRequest"), "{page}");

        let photo: Vec<u8> = (0..300_000u32).map(|n| n as u8).collect();
        let (status, saved) = put(&url, "Dovolená%20%C4%8Daj.jpg", &photo);
        assert!(status.contains("201"), "{status}");
        assert_eq!(saved, "Dovolená čaj.jpg");
        assert_eq!(std::fs::read(dir.join("Dovolená čaj.jpg")).unwrap(), photo);
        // An empty file is a file.
        assert!(put(&url, "empty.txt", b"").0.contains("201"));
        assert_eq!(std::fs::read(dir.join("empty.txt")).unwrap(), b"");

        // The same name again: kept beside, the first left as it was.
        assert_eq!(put(&url, "empty.txt", b"second").1, "empty (2).txt");
        assert_eq!(std::fs::read(dir.join("empty.txt")).unwrap(), b"");

        let mut updates = Vec::new();
        while let Ok(update) = receiving.updates.recv_timeout(Duration::from_millis(500)) {
            updates.push(update);
        }
        let saved: Vec<PathBuf> = updates.iter().filter_map(|update| if let Update::Saved { path, .. } = update { Some(path.clone()) } else { None }).collect();
        assert_eq!(saved, [dir.join("Dovolená čaj.jpg"), dir.join("empty.txt"), dir.join("empty (2).txt")]);
        assert!(updates.contains(&Update::Started {
            id: 0,
            name: "Dovolená čaj.jpg".to_string(),
            size: 300_000
        }));

        // Anything but its own paths: nothing there.
        let elsewhere = url.rsplit_once('/').unwrap().0.to_string() + "/guess";
        assert!(put(&elsewhere, "x", b"x").0.contains("404"));
        drop(receiving);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A name with a path in it, or one that would climb out, is saved in
    /// the directory and nowhere else - by its own last part, made fit for a
    /// name; one that is nothing is refused.
    #[test]
    fn a_name_cannot_reach_outside_the_directory() {
        assert_eq!(file_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(file_name("..\\..\\evil.exe").as_deref(), Some("evil.exe"));
        assert_eq!(file_name("a:b*c?.txt").as_deref(), Some("a-b-c-.txt"));
        assert_eq!(file_name("line\nbreak.txt").as_deref(), Some("line-break.txt"));
        assert_eq!(file_name("..").as_deref(), None);
        assert_eq!(file_name("dir/").as_deref(), None);
        assert_eq!(file_name("   ").as_deref(), None);
        let long = format!("{}.jpg", "é".repeat(200));
        let short = file_name(&long).unwrap();
        assert!(short.len() <= 255 && short.ends_with(".jpg"), "{short}");

        let dir = scratch("outside");
        let inside = dir.join("in");
        std::fs::create_dir_all(&inside).unwrap();
        let receiving = receive(inside.clone(), IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        let url = receiving.url(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(put(&url, "..%2Fescaped.txt", b"x").0.contains("201"));
        assert!(!dir.join("escaped.txt").exists() && inside.join("escaped.txt").exists());
        assert!(put(&url, "..", b"x").0.contains("400"));
        drop(receiving);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file that stops coming before its end - the phone's screen locked,
    /// its Wi-Fi gone - leaves nothing behind, not even its beginning; one
    /// sent without saying its size is refused.
    #[test]
    fn a_file_given_up_on_leaves_nothing() {
        let dir = scratch("given-up");
        let receiving = receive(dir.clone(), IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        let url = receiving.url(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let rest = url.strip_prefix("http://").unwrap();
        let (host, base) = rest.split_at(rest.find('/').unwrap());
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(format!("PUT {base}/half.bin HTTP/1.1\r\nContent-Length: 100000\r\n\r\n").as_bytes()).unwrap();
        stream.write_all(&[7; 40_000]).unwrap();
        drop(stream);
        let mut lost = None;
        while let Ok(update) = receiving.updates.recv_timeout(Duration::from_secs(5)) {
            if let Update::Lost { name, .. } = update {
                lost = Some(name);
                break;
            }
        }
        assert_eq!(lost.as_deref(), Some("half.bin"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing left, not even a temporary");

        let (status, _) = send(&url, "PUT", "/no-size.bin", "Transfer-Encoding: chunked\r\n", b"3\r\nabc\r\n0\r\n\r\n");
        assert!(status.contains("411"), "{status}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        drop(receiving);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The page's text is the language fm84 is in, and a directory's name
    /// can put nothing into the page but its letters.
    #[test]
    fn the_page_says_where_and_escapes_it() {
        let page = page(Path::new("/home/me/<img src=x onerror=alert(1)>"));
        assert!(!page.contains("<img src=x"), "{page}");
        assert!(page.contains("&lt;img src=x onerror=alert(1)&gt;"));
        // The quote escaped, and `<` too, so `</script>` cannot end the script.
        assert_eq!(script_string("a\"b</c"), "\"a\\\"b\\u003c/c\"");
    }
}
