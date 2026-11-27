//! Alt+W: a phone as fm84's remote - for a terminal on a TV across the room.
//! A server of its own, on a port the system gives it and a path no one can
//! guess, gives the phone a page with the active panel's listing on it and
//! a pad of keys. A tap on an entry moves the cursor to it, and a tap on the
//! one it is on opens it; the keys go to fm84 as if typed - only these keys,
//! so the phone can look and open, page the viewer and close it, and do
//! nothing else. The page asks for what fm84 shows every half second.
//! Until stopped.

use crate::share::read_request;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The keys the phone has, by the names its page sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Backspace,
    Tab,
    Esc,
    /// F3: the file under the cursor, in the viewer.
    View,
}

impl Key {
    const ALL: [(&'static str, Key); 13] = [
        ("up", Key::Up),
        ("down", Key::Down),
        ("left", Key::Left),
        ("right", Key::Right),
        ("page-up", Key::PageUp),
        ("page-down", Key::PageDown),
        ("home", Key::Home),
        ("end", Key::End),
        ("enter", Key::Enter),
        ("backspace", Key::Backspace),
        ("tab", Key::Tab),
        ("esc", Key::Esc),
        ("view", Key::View),
    ];

    /// The key the page names; None for any other name.
    pub fn named(name: &str) -> Option<Key> {
        Key::ALL.iter().find(|(known, _)| *known == name).map(|&(_, key)| key)
    }
}

/// What the phone has done, as it does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A phone at this address has opened the page, or asked since.
    Heard(IpAddr),
    /// A key pressed.
    Key(Key),
    /// An entry tapped: the cursor to its row.
    Pick(usize),
    /// The server could not go on.
    Failed(String),
}

/// A remote being served. Dropped, it stops.
pub struct Remote {
    pub port: u16,
    token: String,
    /// What fm84 shows, as the page reads it - kept up to date by whoever
    /// holds this.
    shown: Arc<Mutex<String>>,
    pub updates: Receiver<Update>,
    stop: Arc<AtomicBool>,
}

impl Remote {
    /// What a phone opens to reach it at `address`.
    pub fn url(&self, address: IpAddr) -> String {
        format!("http://{}/{}", SocketAddr::new(address, self.port), self.token)
    }

    /// What the page shows from now on, as JSON.
    pub fn show(&self, json: String) {
        if let Ok(mut shown) = self.shown.lock()
            && *shown != json
        {
            *shown = json;
        }
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A remote at `address` - every address the machine has, for the
/// unspecified one - on a port the system gives, under a path made up now.
pub fn serve(address: IpAddr) -> io::Result<Remote> {
    let listener = TcpListener::bind(SocketAddr::new(address, 0))?;
    listener.set_nonblocking(true)?;
    let token = crate::share::token();
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let shown = Arc::new(Mutex::new("{}".to_string()));
    let (sender, updates) = mpsc::channel();
    let shared = Arc::new(Shared {
        token: token.clone(),
        page: page(),
        shown: Arc::clone(&shown),
        stop: Arc::clone(&stop),
        updates: sender,
    });
    std::thread::spawn(move || listen(listener, shared));
    Ok(Remote { port, token, shown, updates, stop })
}

/// What every connection's thread needs to answer.
struct Shared {
    token: String,
    page: String,
    shown: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    updates: Sender<Update>,
}

/// Take connections until told to stop, each on a thread of its own.
fn listen(listener: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || answer(stream, peer.ip(), &shared));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = shared.updates.send(Update::Failed(error.to_string()));
                return;
            }
        }
    }
}

fn reply(stream: &mut TcpStream, status: &str, kind: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
}

/// One connection: the page, what fm84 shows, a key or a tap - or nothing.
fn answer(mut stream: TcpStream, peer: IpAddr, shared: &Shared) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_nodelay(true);
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let base = format!("/{}", shared.token);
    let Some(path) = request.path.strip_prefix(&base) else {
        return reply(&mut stream, "404 Not Found", "text/plain", "");
    };
    match (request.method.as_str(), path) {
        ("GET", "" | "/") => {
            let _ = shared.updates.send(Update::Heard(peer));
            reply(&mut stream, "200 OK", "text/html", &shared.page);
        }
        ("GET", "/state") => {
            let _ = shared.updates.send(Update::Heard(peer));
            let shown = shared.shown.lock().map(|shown| shown.clone()).unwrap_or_default();
            reply(&mut stream, "200 OK", "application/json", &shown);
        }
        ("POST", "/key" | "/pick") => {
            // A word or a number: nothing a phone would send runs longer.
            let size = request.header("Content-Length").and_then(|length| length.parse::<u64>().ok()).unwrap_or(0).min(32);
            let mut body = String::new();
            let _ = request.body_start.as_slice().chain(&mut stream).take(size).read_to_string(&mut body);
            let update = if path == "/key" { Key::named(body.trim()).map(Update::Key) } else { body.trim().parse().ok().map(Update::Pick) };
            match update {
                Some(update) => {
                    let _ = shared.updates.send(update);
                    reply(&mut stream, "204 No Content", "text/plain", "");
                }
                None => reply(&mut stream, "400 Bad Request", "text/plain", ""),
            }
        }
        _ => reply(&mut stream, "404 Not Found", "text/plain", ""),
    }
}

/// Text as a JSON string, quotes and all - with `<` written so it cannot
/// close a script it is put in.
pub fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The page a phone opens, in fm84's language and its theme's colours.
fn page() -> String {
    page_in(crate::display::palette())
}

fn page_in(palette: &crate::display::Palette) -> String {
    use crate::receive::html;
    let fills = [
        ("%TITLE%", html(t!("fm84 remote"))),
        ("%BACK%", html(t!("Back"))),
        ("%VIEW%", html(t!("View"))),
        ("%PANEL%", html(t!("Panel"))),
        ("%ICON%", crate::receive::pixel_icon()),
    ];
    let mut page = crate::receive::themed(PAGE, palette);
    for (place, fill) in fills {
        page = page.replace(place, &fill);
    }
    page
}

/// The page, its words filled in where it says `%THEM%`. The listing above,
/// the pad below it, held at the foot of the screen for a thumb.
const PAGE: &str = r##"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,user-scalable=no">
<meta name="theme-color" content="%BG%">
<title>%TITLE%</title>
%STYLE%
<style>
body{padding-bottom:0;display:flex;flex-direction:column;height:100vh;height:100dvh;overflow:hidden}
header{margin-bottom:calc(var(--px)*3)}
#where{margin:0}
#list{flex:1;overflow-y:auto;margin:0 var(--px) calc(var(--px)*4);padding:var(--px) 0}
#list li{margin:0;padding:calc(var(--px)*2) calc(var(--px)*3);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;cursor:pointer}
#list li.dir{color:var(--head)}
#list li.at{background:var(--button);color:var(--on-button)}
#list li .size{float:right;color:var(--muted);margin-left:calc(var(--px)*3)}
#list li.at .size{color:var(--on-button)}
#note{color:var(--no)}
#pad{display:grid;grid-template-columns:repeat(4,1fr);gap:calc(var(--px)*4);padding:calc(var(--px)*2) var(--px) calc(var(--px)*6)}
#pad button{width:auto;margin:0;padding:calc(var(--px)*4) 0;font-size:1em}
#pad button::before,#pad button::after{content:""}
#pad button+button{margin-top:0}
</style></head><body>
<header>%ICON%<div><h1>%TITLE%</h1><p id="where"></p><p id="note" hidden></p></div></header>
<ul id="list" class="pixel"></ul>
<div id="pad">
<button data-key="home">⇤</button><button data-key="up">▲</button><button data-key="page-up">⇞</button><button data-key="backspace">%BACK%</button>
<button data-key="left">◀</button><button data-key="enter">OK</button><button data-key="right">▶</button><button data-key="view">%VIEW%</button>
<button data-key="end">⇥</button><button data-key="down">▼</button><button data-key="page-down">⇟</button><button data-key="tab">%PANEL%</button>
<button data-key="esc" style="grid-column:span 4">Esc</button>
</div>
<script>
const base=location.pathname.replace(/\/$/,""), list=document.getElementById("list"), where=document.getElementById("where"), note=document.getElementById("note");
let shown=null, at=-1;
const post=(to,body)=>fetch(base+to,{method:"POST",headers:{"Content-Type":"text/plain"},body}).then(()=>setTimeout(look,120)).catch(()=>{});
for(const button of document.querySelectorAll("#pad button"))button.onclick=()=>post("/key",button.dataset.key);
const draw=state=>{
  where.textContent=state.title||"";
  note.hidden=!state.note; note.textContent=state.note||"";
  const items=state.items||[];
  const key=JSON.stringify([state.title,state.start,items]);
  if(key!==shown){
    shown=key; list.textContent="";
    items.forEach((item,index)=>{
      const row=document.createElement("li"), size=document.createElement("span");
      size.className="size"; size.textContent=item.s||"";
      row.append(size,document.createTextNode(item.d?"["+item.n+"]":item.n));
      if(item.d)row.classList.add("dir");
      const number=state.start+index;
      row.onclick=()=>number===at?post("/key","enter"):post("/pick",String(number));
      list.append(row);
    });
    at=-1;
  }
  if(state.cursor!==at){
    for(const row of list.querySelectorAll(".at"))row.classList.remove("at");
    const row=list.children[state.cursor-state.start];
    if(row){row.classList.add("at");row.scrollIntoView({block:"nearest"});}
    at=state.cursor;
  }
};
const look=()=>fetch(base+"/state",{cache:"no-store"}).then(answer=>answer.json()).then(draw).catch(()=>{});
look(); setInterval(look,500);
</script></body></html>"##;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// A request as the page makes it; the status line and what comes back.
    fn ask(url: &str, method: &str, path: &str, body: &str) -> (String, String) {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, base) = rest.split_at(rest.find('/').unwrap());
        let mut stream = TcpStream::connect(host).unwrap();
        stream.write_all(format!("{method} {base}{path} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        let mut all = String::new();
        stream.read_to_string(&mut all).unwrap();
        let (head, body) = all.split_once("\r\n\r\n").unwrap_or((&all, ""));
        (head.lines().next().unwrap_or_default().to_string(), body.to_string())
    }

    fn updates(remote: &Remote) -> Vec<Update> {
        let mut heard = Vec::new();
        while let Ok(update) = remote.updates.recv_timeout(Duration::from_millis(300)) {
            heard.push(update);
        }
        heard
    }

    /// The page, what fm84 shows, and the keys and taps it sends - only the
    /// keys it has, and only at its own path.
    #[test]
    fn the_phone_sees_and_presses() {
        let remote = serve(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        let url = remote.url(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let (status, page) = ask(&url, "GET", "", "");
        assert!(status.contains("200") && page.contains("data-key=\"page-down\"") && page.contains("/state"), "{status}");

        remote.show(r#"{"title":"/home","cursor":2}"#.to_string());
        assert_eq!(ask(&url, "GET", "/state", "").1, r#"{"title":"/home","cursor":2}"#);

        assert!(ask(&url, "POST", "/key", "page-down").0.contains("204"));
        assert!(ask(&url, "POST", "/pick", "7").0.contains("204"));
        // A key the phone has not got, as Delete or F10 would be, does nothing.
        assert!(ask(&url, "POST", "/key", "delete").0.contains("400"));
        assert!(ask(&url, "POST", "/key", "f10").0.contains("400"));
        assert!(ask(&url, "POST", "/pick", "minus one").0.contains("400"));
        let elsewhere = url.rsplit_once('/').unwrap().0.to_string() + "/guess";
        assert!(ask(&elsewhere, "POST", "/key", "enter").0.contains("404"));

        let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert_eq!(updates(&remote), [Update::Heard(localhost), Update::Heard(localhost), Update::Key(Key::PageDown), Update::Pick(7)]);
    }

    #[test]
    fn every_key_the_page_has_is_known() {
        let page = page_in(crate::display::palette_of(crate::options::Theme::Synthwave));
        let named: Vec<&str> = page.split("data-key=\"").skip(1).map(|rest| &rest[..rest.find('"').unwrap()]).collect();
        assert_eq!(named.len(), Key::ALL.len());
        assert!(named.iter().all(|name| Key::named(name).is_some()), "{named:?}");
        let left = page.split('%').skip(1).step_by(2).find(|between| !between.is_empty() && between.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
        assert_eq!(left, None, "every place filled in");
    }

    #[test]
    fn a_name_cannot_break_out_of_its_string() {
        assert_eq!(json_string("a\"b\\c</script>\n\u{2028}"), "\"a\\\"b\\\\c\\u003c/script>\\u000a\\u2028\"");
    }
}
