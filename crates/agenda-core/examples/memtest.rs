//! Mémoire occupée après ouverture d'un dossier : `cargo run --release -p agenda-core --example memtest <dossier>`
//! (dossier généré par `GARDER=1 cargo run --release -p agenda-core --example bench`).
use agenda_core::store::Store;
use agenda_core::tz::Tz;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Count;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        System.dealloc(p, l)
    }
}
#[global_allocator]
static A: Count = Count;
fn rss() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("VmRSS"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let r0 = rss();
    let st = Store::open(&dir, false, Tz::utc()).unwrap();
    let r1 = rss();
    let raw: usize = st.items.values().map(|i| i.raw.capacity()).sum();
    let body: usize =
        st.events().map(|e| e.body.capacity()).sum::<usize>() + st.tasks().map(|t| t.body.capacity()).sum::<usize>();
    println!(
        "RSS avant {r0} Ko, après {r1} Ko ; raw {} Ko, body {} Ko, items {}",
        raw / 1024,
        body / 1024,
        st.items.len()
    );
    println!("tas vivant : {} Ko", LIVE.load(Ordering::Relaxed) / 1024);
    println!(
        "taille Event {} o, Item {} o, RRule {} o",
        std::mem::size_of::<agenda_core::model::Event>(),
        std::mem::size_of::<agenda_core::store::Item>(),
        std::mem::size_of::<agenda_core::rrule::RRule>()
    );
}
