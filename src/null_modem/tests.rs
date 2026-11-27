use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fm84-null-modem-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every update the listener has said within `wait`, until `until` says
/// enough.
fn collect(listening: &Listening, wait: Duration, until: impl Fn(&[Update]) -> bool) -> Vec<Update> {
    let started = Instant::now();
    let mut heard = Vec::new();
    while started.elapsed() < wait && !until(&heard) {
        if let Ok(update) = listening.updates.recv_timeout(Duration::from_millis(20)) {
            heard.push(update);
        }
    }
    heard
}

fn here(listening: &Listening) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), listening.port)
}

/// A path sent can only land under the directory taking it.
#[test]
fn a_path_sent_stays_under_the_directory() {
    assert_eq!(relative(b"a/b.txt"), Some(PathBuf::from("a").join("b.txt")));
    assert_eq!(relative(b"with%2Fslash"), None);
    assert_eq!(relative(b"%25done"), Some(PathBuf::from("%done")));
    for bad in [&b""[..], b"/etc/passwd", b"../up", b"a/../../up", b"a//b", b".", b"a/.", b"nul%00"] {
        assert_eq!(relative(bad), None, "{:?}", String::from_utf8_lossy(bad));
    }
    // Round trip, with what a line cannot carry as it is.
    let path = PathBuf::from("dir with\ttab").join("50% off\n.txt");
    assert_eq!(relative(encode_path(&path).as_bytes()), Some(path));
}

#[test]
fn times_go_across_to_the_nanosecond() {
    let time = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
    assert_eq!(time_from(time_text(Some(time)).as_bytes()), Some(time));
    assert_eq!(time_text(None), "-");
    assert_eq!(time_from(b"-"), None);
}

/// Asked on the network, a listener answers with where it is and who.
#[test]
fn a_listener_is_found() {
    let dir = scratch("found");
    let listening = listen(dir.join(""), "colt@ramrod".to_string(), false, 0).unwrap();
    let scanner = scan(listening.beacon_port).unwrap();
    let peer = scanner.found.recv_timeout(Duration::from_secs(5)).expect("nobody answered");
    assert_eq!(peer.address.port(), listening.port);
    assert_eq!(peer.name, "colt@ramrod");
    assert_eq!(peer.dir, dir.file_name().unwrap().to_string_lossy());
    assert_eq!(parse_answer(b"something else", peer.address), None);
    drop(listening);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Let in, a sender's directories and files land whole, with their times,
/// and each new thing is said so Ctrl+Z can take it back.
#[test]
fn files_and_directories_go_across() {
    let dir = scratch("across");
    let listening = listen(dir.clone(), "here".to_string(), false, 0).unwrap();
    let stop = AtomicBool::new(false);
    let mut link = Link::dial(here(&listening), "there", 2, 8, &stop).unwrap();
    assert_eq!(link.dir, dir.file_name().unwrap().to_string_lossy());

    link.make_dir(Path::new("tree")).unwrap().unwrap();
    link.make_dir(Path::new("tree/deeper")).unwrap().unwrap();
    let modified = UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    assert_eq!(link.offer(Path::new("tree/deeper/a"), 5, Some(modified), Some(0o755)).unwrap().unwrap(), There::Nothing);
    let mut steps = 0;
    let saved = link.send(How::New, &mut &b"hello"[..], 5, &mut |_| {
        steps += 1;
        true
    });
    assert_eq!(saved.unwrap().unwrap(), Some("a".to_string()));
    assert!(steps > 0);
    assert_eq!(link.offer(Path::new("b"), 3, None, None).unwrap().unwrap(), There::Nothing);
    link.send(How::New, &mut &b"abc"[..], 3, &mut |_| true).unwrap().unwrap();
    link.end().unwrap();

    let file = dir.join("tree").join("deeper").join("a");
    assert_eq!(std::fs::read(&file).unwrap(), b"hello");
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), modified);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o755);
    }
    assert_eq!(std::fs::read(dir.join("b")).unwrap(), b"abc");

    let heard = collect(&listening, Duration::from_secs(5), |heard| heard.iter().any(|update| matches!(update, Update::Ended { .. })));
    let created: Vec<&PathBuf> = heard.iter().filter_map(|update| if let Update::Created(path) = update { Some(path) } else { None }).collect();
    // The tree once, not what is in it, and the file beside it.
    assert_eq!(created, [&dir.join("tree"), &dir.join("b")]);
    assert!(heard.iter().any(|update| matches!(update, Update::Ended { from, complete: true } if from == "there")));
    drop(listening);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file in the way is told to the sender, who chooses: written over,
/// kept beside, or not sent.
#[test]
fn what_is_in_the_way_is_the_senders_to_decide() {
    let dir = scratch("clash");
    std::fs::write(dir.join("a"), "old").unwrap();
    let listening = listen(dir.clone(), "here".to_string(), false, 0).unwrap();
    let stop = AtomicBool::new(false);
    let mut link = Link::dial(here(&listening), "there", 3, 9, &stop).unwrap();

    let There::Something { size, modified } = link.offer(Path::new("a"), 3, None, None).unwrap().unwrap() else {
        panic!("nothing said to be in the way");
    };
    assert_eq!(size, 3);
    assert_eq!(modified, std::fs::metadata(dir.join("a")).unwrap().modified().ok());
    link.skip().unwrap();
    assert_eq!(std::fs::read(dir.join("a")).unwrap(), b"old");

    link.offer(Path::new("a"), 3, None, None).unwrap().unwrap();
    assert_eq!(link.send(How::KeepBoth, &mut &b"two"[..], 3, &mut |_| true).unwrap().unwrap(), Some("a (2)".to_string()));
    assert_eq!(std::fs::read(dir.join("a (2)")).unwrap(), b"two");

    link.offer(Path::new("a"), 3, None, None).unwrap().unwrap();
    link.send(How::Replace, &mut &b"new"[..], 3, &mut |_| true).unwrap().unwrap();
    assert_eq!(std::fs::read(dir.join("a")).unwrap(), b"new");

    // A directory in the way of a file cannot be written over.
    std::fs::create_dir(dir.join("d")).unwrap();
    assert!(link.offer(Path::new("d"), 1, None, None).unwrap().is_err());
    // Nor a path that climbs out.
    assert!(link.offer(Path::new("../out"), 1, None, None).unwrap().is_err());
    link.end().unwrap();

    let heard = collect(&listening, Duration::from_secs(5), |heard| heard.iter().any(|update| matches!(update, Update::Ended { .. })));
    // Written over, it was there before: only the one kept beside is new.
    let created: Vec<&PathBuf> = heard.iter().filter_map(|update| if let Update::Created(path) = update { Some(path) } else { None }).collect();
    assert_eq!(created, [&dir.join("a (2)")]);
    assert!(!dir.parent().unwrap().join("out").exists());
    drop(listening);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Asking first, the sender waits on the answer; no is said to it.
#[test]
fn a_sender_waits_to_be_let_in() {
    let dir = scratch("ask");
    let listening = listen(dir.clone(), "here".to_string(), true, 0).unwrap();
    let address = here(&listening);
    let refused = std::thread::spawn(move || {
        let stop = AtomicBool::new(false);
        Link::dial(address, "stranger", 7, 700, &stop).map(|_| ())
    });
    let heard = collect(&listening, Duration::from_secs(5), |heard| !heard.is_empty());
    let Some(Update::Asking { from, files, bytes, answer }) = heard.into_iter().next() else {
        panic!("not asked");
    };
    assert_eq!((from.as_str(), files, bytes), ("stranger", 7, 700));
    answer.send(false).unwrap();
    let error = refused.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "Refused");

    let accepted = std::thread::spawn(move || {
        let stop = AtomicBool::new(false);
        Link::dial(address, "friend", 0, 0, &stop).and_then(Link::end)
    });
    let heard = collect(&listening, Duration::from_secs(5), |heard| !heard.is_empty());
    let Some(Update::Asking { answer, .. }) = heard.into_iter().next() else {
        panic!("not asked");
    };
    answer.send(true).unwrap();
    accepted.join().unwrap().unwrap();
    drop(listening);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A sender that stops halfway through a file leaves nothing of it.
#[test]
fn a_file_cut_short_leaves_nothing() {
    let dir = scratch("short");
    let listening = listen(dir.clone(), "here".to_string(), false, 0).unwrap();
    let stop = AtomicBool::new(false);
    let mut link = Link::dial(here(&listening), "there", 1, 1 << 20, &stop).unwrap();
    link.offer(Path::new("big"), 1 << 20, None, None).unwrap().unwrap();
    let mut source = std::io::repeat(7).take(1 << 20);
    let mut sent = 0;
    assert_eq!(
        link.send(How::New, &mut source, 1 << 20, &mut |amount| {
            sent += amount;
            sent < 100_000
        })
        .unwrap()
        .unwrap(),
        None
    );
    drop(link);
    let heard = collect(&listening, Duration::from_secs(5), |heard| heard.iter().any(|update| matches!(update, Update::Ended { .. })));
    assert!(heard.iter().any(|update| matches!(update, Update::Ended { complete: false, .. })));
    assert!(heard.iter().any(|update| matches!(update, Update::Lost { .. })));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "something was left");
    drop(listening);
    std::fs::remove_dir_all(&dir).unwrap();
}
