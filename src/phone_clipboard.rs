//! Alt+V: a phone's clipboard and fm84's, across the network. A server of its
//! own, on a port the system gives it and a path no one can guess, gives the
//! phone a page with a box to type or paste into: what is in it goes to
//! fm84's clipboard - and the terminal's, as Ctrl+C's paths go - or into a
//! new file in the directory the panel is in; and the system's clipboard -
//! or fm84's own, where the system's cannot be read - comes back into the
//! box, for the phone to copy. Until Esc stops it.

use crate::share::read_request;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The most text the page may send: a clipboard, not a file.
pub const MOST: usize = 1024 * 1024;

/// What a text saved as a file is called, kept beside one of the name.
const NOTE: &str = "note.txt";

/// What the server has to say, as it says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// The phone sent text for the clipboard.
    Text(String),
    /// The phone sent text to keep, and here it is.
    Saved(PathBuf),
    /// The phone took what is on fm84's clipboard.
    Taken,
    /// The server could not go on.
    Failed(String),
}

/// A clipboard shared with a phone. Dropped, it stops.
pub struct Sharing {
    pub dir: PathBuf,
    pub port: u16,
    token: String,
    /// What the phone is given when it asks for the clipboard and the
    /// system's cannot be read - kept up to date by whoever holds this.
    pub clipboard: Arc<Mutex<String>>,
    pub updates: Receiver<Update>,
    stop: Arc<AtomicBool>,
}

impl Sharing {
    /// What a phone opens to reach it at `address`.
    pub fn url(&self, address: IpAddr) -> String {
        format!("http://{}/{}", SocketAddr::new(address, self.port), self.token)
    }

    /// What the phone gets from now on.
    pub fn set_clipboard(&self, text: &str) {
        if let Ok(mut clipboard) = self.clipboard.lock()
            && *clipboard != text
        {
            text.clone_into(&mut clipboard);
        }
    }
}

impl Drop for Sharing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Share `clipboard` with a phone at `address` - every address the machine
/// has, for the unspecified one - saving what it keeps as a file in `dir`.
pub fn share(dir: PathBuf, clipboard: String, address: IpAddr) -> io::Result<Sharing> {
    let listener = TcpListener::bind(SocketAddr::new(address, 0))?;
    listener.set_nonblocking(true)?;
    let token = crate::share::token();
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let clipboard = Arc::new(Mutex::new(clipboard));
    let (sender, updates) = mpsc::channel();
    let shared = Arc::new(Shared {
        dir: dir.clone(),
        token: token.clone(),
        page: page(),
        clipboard: Arc::clone(&clipboard),
        stop: Arc::clone(&stop),
        updates: sender,
    });
    std::thread::spawn(move || listen(listener, shared));
    Ok(Sharing { dir, port, token, clipboard, updates, stop })
}

/// What every connection's thread needs to answer.
struct Shared {
    dir: PathBuf,
    token: String,
    page: String,
    clipboard: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    updates: Sender<Update>,
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

/// A reply of `kind`, with `body`.
fn reply(stream: &mut TcpStream, status: &str, kind: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
}

/// One connection: the page, the clipboard asked for, text sent for it or
/// to keep, or nothing.
fn answer(mut stream: TcpStream, shared: &Shared) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let base = format!("/{}", shared.token);
    match (request.method.as_str(), request.path.strip_prefix(&base)) {
        ("GET", Some("" | "/")) => reply(&mut stream, "200 OK", "text/html", &shared.page),
        ("GET", Some("/clipboard")) => {
            // The system's clipboard, as the desktop's own programs read it;
            // fm84's own where they cannot, or it holds no text.
            let text = crate::utils::read_system_clipboard(MOST)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| shared.clipboard.lock().map(|text| text.clone()).unwrap_or_default());
            let _ = shared.updates.send(Update::Taken);
            reply(&mut stream, "200 OK", "text/plain", &text);
        }
        ("POST", Some(to @ ("/clipboard" | "/file"))) => {
            let text = match body(&mut stream, &request) {
                Ok(text) => text,
                Err((status, said)) => return reply(&mut stream, status, "text/plain", &said),
            };
            if to == "/clipboard" {
                let _ = shared.updates.send(Update::Text(text));
                return reply(&mut stream, "200 OK", "text/plain", "");
            }
            match save(&shared.dir, &text) {
                Ok(path) => {
                    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    let _ = shared.updates.send(Update::Saved(path));
                    reply(&mut stream, "201 Created", "text/plain", &name);
                }
                Err(error) => reply(&mut stream, "500 Internal Server Error", "text/plain", &error.to_string()),
            }
        }
        _ => reply(&mut stream, "404 Not Found", "text/plain", ""),
    }
}

/// The text sent: as long as the head says, no longer than MOST, and UTF-8.
/// The status and the reason, for one that is not.
fn body(stream: &mut TcpStream, request: &crate::share::Request) -> Result<String, (&'static str, String)> {
    let Some(size) = request.header("Content-Length").and_then(|length| length.parse::<usize>().ok()) else {
        return Err(("411 Length Required", t!("No size given").to_string()));
    };
    if size > MOST {
        return Err(("413 Content Too Large", t!("Too long for a clipboard").to_string()));
    }
    let mut bytes = Vec::with_capacity(size);
    request.body_start.as_slice().chain(&mut *stream).take(size as u64).read_to_end(&mut bytes).map_err(|error| ("400 Bad Request", error.to_string()))?;
    if bytes.len() < size {
        return Err(("400 Bad Request", t!("It stopped coming before its end").to_string()));
    }
    String::from_utf8(bytes).map_err(|_| ("400 Bad Request", t!("Not text").to_string()))
}

/// `text` written whole as NOTE in `dir`, or beside one of the name - never
/// over it. Where it went.
fn save(dir: &Path, text: &str) -> io::Result<PathBuf> {
    let wanted = dir.join(NOTE);
    let (temp, mut file) = crate::fs_ops::temp_beside(&wanted)?;
    let written = file.write_all(text.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
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

/// The page a phone opens, in fm84's language and its theme's colours.
fn page() -> String {
    page_in(crate::display::palette())
}

fn page_in(palette: &crate::display::Palette) -> String {
    use crate::receive::{html, script_string};
    let fills = [
        ("%TITLE%", html(t!("fm84 clipboard"))),
        ("%HEADING%", html(t!("Type or paste, then send it across"))),
        ("%PLACEHOLDER%", html(t!("Text"))),
        ("%TO_CLIPBOARD%", html(t!("To fm84's clipboard"))),
        ("%TO_FILE%", html(t!("Save as a file"))),
        ("%FROM_CLIPBOARD%", html(t!("Get fm84's clipboard"))),
        ("%SENT%", script_string(t!("Sent"))),
        ("%SAVED%", script_string(t!("Saved as"))),
        ("%GOT%", script_string(t!("Copied from fm84"))),
        ("%EMPTY%", script_string(t!("fm84's clipboard is empty"))),
        ("%LOST%", script_string(t!("Not sent"))),
        ("%ICON%", crate::receive::pixel_icon()),
    ];
    let mut page = crate::receive::themed(PAGE, palette);
    for (place, fill) in fills {
        page = page.replace(place, &fill);
    }
    page
}

/// The page, its words filled in where it says `%THEM%`.
const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="theme-color" content="%BG%">
<title>%TITLE%</title>
%STYLE%</head><body>
<header>%ICON%<div><h1>%TITLE%</h1><p>%HEADING%</p></div></header>
<textarea id="text" class="pixel" placeholder="%PLACEHOLDER%"></textarea>
<button id="to-clipboard">%TO_CLIPBOARD%</button>
<button id="to-file">%TO_FILE%</button>
<button id="from-clipboard">%FROM_CLIPBOARD%</button>
<ul><li id="said" class="pixel" hidden></li></ul>
<script>
const sent=%SENT%, saved=%SAVED%, got=%GOT%, empty=%EMPTY%, lost=%LOST%;
const text=document.getElementById("text"), said=document.getElementById("said"), base=location.pathname.replace(/\/$/,"");
const say=(ok,words)=>{said.hidden=false;said.className="pixel "+(ok?"ok":"no");said.textContent=words;};
const send=async(to,done)=>{
  try{
    const answer=await fetch(base+to,{method:"POST",headers:{"Content-Type":"text/plain;charset=utf-8"},body:text.value});
    const words=await answer.text();
    answer.ok?say(true,done(words)):say(false,lost+": "+words);
  }catch(e){say(false,lost);}
};
document.getElementById("to-clipboard").onclick=()=>send("/clipboard",()=>sent);
document.getElementById("to-file").onclick=()=>send("/file",name=>saved+" "+name);
document.getElementById("from-clipboard").onclick=async()=>{
  try{
    const answer=await fetch(base+"/clipboard",{cache:"no-store"});
    text.value=await answer.text();
    if(!text.value){say(false,empty);return;}
    // A page not served over https may not write the clipboard itself; the
    // text selected and copied the old way works where that does not.
    try{await navigator.clipboard.writeText(text.value);}catch(e){text.select();document.execCommand("copy");}
    say(true,got);
  }catch(e){say(false,lost);}
};
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fm84-phone-clipboard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A request as the page makes it, and the status line and body of what
    /// comes back.
    fn ask(url: &str, method: &str, path: &str, body: &[u8]) -> (String, String) {
        ask_saying(url, method, path, body.len(), body)
    }

    /// As `ask`, the head saying the body is `length` long, whatever it is.
    fn ask_saying(url: &str, method: &str, path: &str, length: usize, body: &[u8]) -> (String, String) {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, base) = rest.split_at(rest.find('/').unwrap());
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(format!("{method} {base}{path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {length}\r\n\r\n").as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        let mut all = Vec::new();
        stream.read_to_end(&mut all).unwrap();
        let text = String::from_utf8_lossy(&all).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        (head.lines().next().unwrap_or_default().to_string(), body.to_string())
    }

    fn updates(sharing: &Sharing) -> Vec<Update> {
        let mut heard = Vec::new();
        while let Ok(update) = sharing.updates.recv_timeout(Duration::from_millis(300)) {
            heard.push(update);
        }
        heard
    }

    /// The page, text sent to the clipboard, text kept as a file beside one
    /// already there, and fm84's clipboard given back - as it is now.
    #[test]
    fn text_goes_both_ways() {
        let dir = scratch("both");
        std::fs::write(dir.join(NOTE), "mine").unwrap();
        let sharing = share(dir.clone(), "first".to_string(), IpAddr::V4(Ipv4Addr::UNSPECIFIED)).unwrap();
        let url = sharing.url(IpAddr::V4(Ipv4Addr::LOCALHOST));

        let (status, page) = ask(&url, "GET", "", b"");
        assert!(status.contains("200") && page.contains("<textarea") && page.contains("/clipboard"), "{status}");

        let (status, _) = ask(&url, "POST", "/clipboard", "héllo\nwörld".as_bytes());
        assert!(status.contains("200"), "{status}");
        let (status, name) = ask(&url, "POST", "/file", "a note".as_bytes());
        assert!(status.contains("201"), "{status}");
        assert_eq!(name, "note (2).txt");
        assert_eq!(std::fs::read_to_string(dir.join("note (2).txt")).unwrap(), "a note");
        assert_eq!(std::fs::read_to_string(dir.join(NOTE)).unwrap(), "mine");

        assert_eq!(ask(&url, "GET", "/clipboard", b"").1, "first");
        sharing.set_clipboard("second");
        assert_eq!(ask(&url, "GET", "/clipboard", b"").1, "second");

        assert_eq!(updates(&sharing), [Update::Text("héllo\nwörld".to_string()), Update::Saved(dir.join("note (2).txt")), Update::Taken, Update::Taken]);
        drop(sharing);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Too long, not text, or anywhere but its own paths: refused, and
    /// nothing said to fm84.
    #[test]
    fn what_is_not_a_clipboard_is_refused() {
        let dir = scratch("refused");
        let sharing = share(dir.clone(), String::new(), IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        let url = sharing.url(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(ask_saying(&url, "POST", "/clipboard", MOST + 1, b"x").0.contains("413"));
        assert!(ask(&url, "POST", "/clipboard", &[0xff, 0xfe]).0.contains("400"));
        let elsewhere = url.rsplit_once('/').unwrap().0.to_string() + "/guess";
        assert!(ask(&elsewhere, "POST", "/clipboard", b"x").0.contains("404"));
        assert!(ask(&elsewhere, "GET", "/clipboard", b"").0.contains("404"));
        assert_eq!(updates(&sharing), []);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        drop(sharing);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every place in the page filled in, and in the theme's colours.
    #[test]
    fn the_page_is_whole_and_themed() {
        let page = page_in(crate::display::palette_of(crate::options::Theme::Synthwave));
        assert!(page.contains("--bg:#000000") && page.contains("crispEdges"));
        let left = page.split('%').skip(1).step_by(2).find(|between| !between.is_empty() && between.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
        assert_eq!(left, None, "every place filled in");
    }
}
