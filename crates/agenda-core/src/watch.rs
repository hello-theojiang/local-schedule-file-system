//! Surveillance du dossier : inotify sur Linux/Android (appels système directs),
//! scrutation périodique partout (filet de sécurité et seul mécanisme sur macOS).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Pending {
    paths: Vec<String>,
    full: bool,
}

pub struct Watcher {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    stop: Arc<AtomicBool>,
    pub native: bool,
}

impl Watcher {
    /// Démarre la surveillance. `poll` : période de scrutation complète.
    pub fn start(root: &Path, poll: Duration) -> Watcher {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let native = native::start(root.to_path_buf(), shared.clone(), stop.clone());
        // scrutation : plus fréquente si inotify n'est pas disponible
        let period = if native { poll.max(Duration::from_secs(30)) } else { poll };
        {
            let shared = shared.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("agenda-poll".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(period);
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let (m, c) = &*shared;
                        if let Ok(mut p) = m.lock() {
                            p.full = true;
                        }
                        c.notify_all();
                    }
                })
                .ok();
        }
        Watcher { shared, stop, native }
    }

    /// Attend un changement (au plus `timeout`). Vrai si quelque chose est en attente.
    pub fn wait(&self, timeout: Duration) -> bool {
        let (m, c) = &*self.shared;
        let Ok(g) = m.lock() else { return false };
        if !g.paths.is_empty() || g.full {
            return true;
        }
        match c.wait_timeout(g, timeout) {
            Ok((g, _)) => !g.paths.is_empty() || g.full,
            Err(_) => false,
        }
    }

    /// Récupère les chemins modifiés et l'indicateur « tout rescanner ».
    pub fn take(&self) -> (Vec<String>, bool) {
        let (m, _) = &*self.shared;
        match m.lock() {
            Ok(mut p) => {
                let mut paths = std::mem::take(&mut p.paths);
                paths.sort();
                paths.dedup();
                (paths, std::mem::take(&mut p.full))
            }
            Err(_) => (vec![], true),
        }
    }

    /// Réveille les attentes en cours (après une écriture de l'application, par exemple).
    pub fn poke(&self) {
        self.shared.1.notify_all();
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.shared.1.notify_all();
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod native {
    use super::Pending;
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_ulong, c_void};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: i16,
        revents: i16,
    }

    extern "C" {
        fn inotify_init1(flags: c_int) -> c_int;
        fn inotify_add_watch(fd: c_int, path: *const c_char, mask: u32) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        fn close(fd: c_int) -> c_int;
        fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    }

    const IN_NONBLOCK: c_int = 0o4000;
    const IN_CLOEXEC: c_int = 0o2000000;
    const IN_CLOSE_WRITE: u32 = 0x8;
    const IN_MOVED_FROM: u32 = 0x40;
    const IN_MOVED_TO: u32 = 0x80;
    const IN_CREATE: u32 = 0x100;
    const IN_DELETE: u32 = 0x200;
    const IN_DELETE_SELF: u32 = 0x400;
    const IN_MOVE_SELF: u32 = 0x800;
    const IN_Q_OVERFLOW: u32 = 0x4000;
    const IN_ISDIR: u32 = 0x4000_0000;
    const MASK: u32 =
        IN_CLOSE_WRITE | IN_MOVED_FROM | IN_MOVED_TO | IN_CREATE | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF;
    const POLLIN: i16 = 1;

    struct Ino {
        fd: c_int,
        dirs: HashMap<c_int, String>,
        root: PathBuf,
    }

    impl Ino {
        fn add(&mut self, rel: &str) -> bool {
            let p = if rel.is_empty() { self.root.clone() } else { self.root.join(rel) };
            let Ok(c) = CString::new(p.to_string_lossy().as_bytes()) else { return false };
            // SAFETY: fd valide, chemin terminé par NUL.
            let wd = unsafe { inotify_add_watch(self.fd, c.as_ptr(), MASK) };
            if wd < 0 {
                return false;
            }
            self.dirs.insert(wd, rel.to_string());
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.starts_with('.')
                        && !(rel.is_empty() && n == ".cache")
                        && !(rel == ".cache" && n == "subscriptions")
                    {
                        continue;
                    }
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let child = if rel.is_empty() { n } else { format!("{rel}/{n}") };
                        if rel.is_empty() && !matches!(child.as_str(), "events" | "tasks" | "calendars" | ".cache") {
                            continue;
                        }
                        self.add(&child);
                    }
                }
            }
            true
        }
    }

    pub fn start(root: PathBuf, shared: Arc<(Mutex<Pending>, Condvar)>, stop: Arc<AtomicBool>) -> bool {
        // SAFETY: appel système sans pointeur.
        let fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
        if fd < 0 {
            return false;
        }
        let mut ino = Ino { fd, dirs: HashMap::new(), root };
        if !ino.add("") {
            // SAFETY: fd ouvert ci-dessus.
            unsafe { close(fd) };
            return false;
        }
        std::thread::Builder::new()
            .name("agenda-inotify".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                while !stop.load(Ordering::Relaxed) {
                    let mut pfd = PollFd { fd: ino.fd, events: POLLIN, revents: 0 };
                    // SAFETY: un seul pollfd valide.
                    let r = unsafe { poll(&mut pfd, 1, 1000) };
                    if r <= 0 {
                        continue;
                    }
                    // SAFETY: tampon de taille connue.
                    let n = unsafe { read(ino.fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
                    if n <= 0 {
                        continue;
                    }
                    let mut paths = Vec::new();
                    let mut full = false;
                    let mut new_dirs = Vec::new();
                    let mut off = 0usize;
                    let n = n as usize;
                    while off + 16 <= n {
                        let wd = i32::from_ne_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4]));
                        let mask = u32::from_ne_bytes(buf[off + 4..off + 8].try_into().unwrap_or([0; 4]));
                        let len = u32::from_ne_bytes(buf[off + 12..off + 16].try_into().unwrap_or([0; 4])) as usize;
                        let name_bytes = &buf[off + 16..(off + 16 + len).min(n)];
                        let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
                        let name = String::from_utf8_lossy(&name_bytes[..end]).to_string();
                        off += 16 + len;
                        if mask & IN_Q_OVERFLOW != 0 {
                            full = true;
                            continue;
                        }
                        let Some(dir) = ino.dirs.get(&wd).cloned() else { continue };
                        if name.is_empty() {
                            if mask & (IN_DELETE_SELF | IN_MOVE_SELF) != 0 {
                                ino.dirs.remove(&wd);
                                full = true;
                            }
                            continue;
                        }
                        let rel = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
                        if mask & IN_ISDIR != 0 {
                            if mask & (IN_CREATE | IN_MOVED_TO) != 0 {
                                new_dirs.push(rel);
                            }
                            full = true;
                            continue;
                        }
                        if name.starts_with(".syncthing.") || name.starts_with('~') || name.ends_with(".tmp") {
                            continue;
                        }
                        // IN_CREATE seul : le contenu arrivera avec IN_CLOSE_WRITE
                        if mask & (IN_CLOSE_WRITE | IN_MOVED_TO | IN_MOVED_FROM | IN_DELETE) != 0 {
                            paths.push(rel);
                        }
                    }
                    for d in new_dirs {
                        ino.add(&d);
                    }
                    if !paths.is_empty() || full {
                        let (m, c) = &*shared;
                        if let Ok(mut p) = m.lock() {
                            p.paths.extend(paths);
                            p.full |= full;
                        }
                        c.notify_all();
                    }
                }
                // SAFETY: fd ouvert par ce thread.
                unsafe { close(ino.fd) };
            })
            .is_ok()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod native {
    use super::Pending;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex};

    pub fn start(_root: PathBuf, _shared: Arc<(Mutex<Pending>, Condvar)>, _stop: Arc<AtomicBool>) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecte_une_ecriture() {
        let dir = crate::store::tests::tmpdir("watch");
        std::fs::create_dir_all(dir.join("tasks")).unwrap();
        let w = Watcher::start(&dir, Duration::from_secs(3600));
        #[cfg(target_os = "linux")]
        assert!(w.native, "inotify indisponible");
        if !w.native {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(dir.join("tasks/a.md"), "---\ntitle: a\n---\n").unwrap();
        assert!(w.wait(Duration::from_secs(3)));
        let (paths, _) = w.take();
        assert_eq!(paths, vec!["tasks/a.md"]);
        // un sous-dossier créé après coup est aussi surveillé
        std::fs::create_dir_all(dir.join("events/2026-10")).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        w.take();
        std::fs::write(dir.join("events/2026-10/b.md"), "x").unwrap();
        assert!(w.wait(Duration::from_secs(3)));
        let (paths, _) = w.take();
        assert!(paths.contains(&"events/2026-10/b.md".to_string()), "{paths:?}");
        drop(w);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
