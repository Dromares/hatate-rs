//! Real Queue thumbnails (flip condition (c), DAN-1983): decoded from image files, through a
//! texture cache, off the main thread.
//!
//! * [`generate`] writes one synthetic JPEG per row into a directory at run time (the Python
//!   repo has no binary fixtures and neither does this one). Each row has its own file, so the
//!   cache cannot get its hit rate from a handful of repeated images.
//! * Rows ask the [`ThumbCache`] for their texture when they are bound. A miss is queued for a
//!   small pool of decode threads; the queue is LIFO and a job whose row has been unbound
//!   before a thread reaches it is dropped, so a fling decodes the rows it ends on, not every
//!   row it passes.
//! * Decoded thumbnails come back over a channel and become `gdk::MemoryTexture`s on the main
//!   thread, held in a bounded LRU.

use crate::fixtures;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use image::imageops::FilterType;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// Source image size on disk and thumbnail size in memory (2x the 32 px cell, for HiDPI).
pub const SOURCE: (u32, u32) = (640, 480);
pub const THUMB_PX: u32 = 64;

pub fn path_for(dir: &Path, id: u32) -> PathBuf {
    dir.join(format!("t_{id:05}.jpg"))
}

/// A distinct synthetic picture per id: the Python tests' `artwork`, with a per-id tint on the
/// lower block and a per-id mark, so every file differs.
pub fn picture(id: u32) -> image::RgbaImage {
    let mut im = fixtures::artwork(SOURCE.0, SOURCE.1);
    let tint = [
        (id.wrapping_mul(53) % 160) as u8,
        (id.wrapping_mul(97) % 160) as u8,
        (id.wrapping_mul(193) % 160) as u8,
    ];
    for y in (SOURCE.1 * 55 / 100)..(SOURCE.1 * 95 / 100) {
        for x in (SOURCE.0 * 15 / 100)..(SOURCE.0 * 85 / 100) {
            let p = im.get_pixel_mut(x, y);
            for c in 0..3 {
                p[c] = p[c].saturating_add(tint[c]);
            }
        }
    }
    let (mx, my) = (
        id.wrapping_mul(31) % (SOURCE.0 - 60),
        id.wrapping_mul(17) % 80,
    );
    for y in my..my + 40 {
        for x in mx..mx + 60 {
            im.put_pixel(
                x,
                y,
                image::Rgba([tint[0] / 2 + 90, tint[1] / 2 + 90, 40, 255]),
            );
        }
    }
    im
}

/// Write `n` JPEGs under `dir`; files that already exist are kept. Returns how many were
/// written. Parallel, because 5,000 encodes in one thread is half a minute.
pub fn generate(dir: &Path, n: u32) -> Result<u32, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let next = std::sync::atomic::AtomicU32::new(0);
    let written = std::sync::atomic::AtomicU32::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                use std::sync::atomic::Ordering::Relaxed;
                loop {
                    let id = next.fetch_add(1, Relaxed);
                    if id >= n {
                        break;
                    }
                    let path = path_for(dir, id);
                    if path.exists() {
                        continue;
                    }
                    let rgb = image::DynamicImage::ImageRgba8(picture(id)).to_rgb8();
                    // Write to a name that is not a final path, so a killed run leaves no
                    // truncated JPEG that a later run would trust.
                    let tmp = path.with_extension("jpg.tmp");
                    let ok = std::fs::File::create(&tmp)
                        .map_err(|e| e.to_string())
                        .and_then(|f| {
                            image::codecs::jpeg::JpegEncoder::new_with_quality(
                                std::io::BufWriter::new(f),
                                80,
                            )
                            .encode_image(&rgb)
                            .map_err(|e| e.to_string())
                        })
                        .and_then(|()| std::fs::rename(&tmp, &path).map_err(|e| e.to_string()));
                    if ok.is_ok() {
                        written.fetch_add(1, Relaxed);
                    }
                }
            });
        }
    });
    Ok(written.load(std::sync::atomic::Ordering::Relaxed))
}

/// Decode `path` to a `THUMB_PX` square (cover fit), RGBA8, opaque.
pub fn decode(path: &Path) -> Result<Vec<u8>, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    Ok(img
        .resize_to_fill(THUMB_PX, THUMB_PX, FilterType::Triangle)
        .to_rgba8()
        .into_raw())
}

#[derive(Default)]
struct Jobs {
    stack: Vec<u32>,
    wanted: HashSet<u32>,
    shutdown: bool,
}

type Decoded = (u32, Result<Vec<u8>, String>, f64);

#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub requested: u64,
    pub hits: u64,
    pub decoded: u64,
    pub failed: u64,
    pub evicted: u64,
    pub decode_ms: Vec<f64>,
}

struct Inner {
    cap: usize,
    textures: RefCell<HashMap<u32, (gdk::Texture, u64)>>,
    stamp: Cell<u64>,
    bound: RefCell<HashMap<u32, glib::WeakRef<gtk4::Picture>>>,
    jobs: Arc<(Mutex<Jobs>, Condvar)>,
    in_flight: Cell<u64>,
    stats: RefCell<Stats>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let (m, cv) = &*self.jobs;
        if let Ok(mut j) = m.lock() {
            j.shutdown = true;
        }
        cv.notify_all();
    }
}

/// Handle shared by the Queue's cell factory and the harness.
#[derive(Clone)]
pub struct ThumbCache(Rc<Inner>);

impl ThumbCache {
    /// Start `workers` decode threads over `dir`, holding at most `cap` textures.
    pub fn new(dir: PathBuf, cap: usize, workers: usize) -> ThumbCache {
        let jobs = Arc::new((Mutex::new(Jobs::default()), Condvar::new()));
        let (tx, rx) = async_channel::unbounded::<Decoded>();
        for _ in 0..workers {
            let (jobs, tx, dir) = (jobs.clone(), tx.clone(), dir.clone());
            std::thread::spawn(move || {
                loop {
                    let id = {
                        let (m, cv) = &*jobs;
                        let mut j = m.lock().expect("jobs lock");
                        loop {
                            if j.shutdown {
                                return;
                            }
                            match j.stack.pop() {
                                // Unbound since it was queued: nobody is waiting for it.
                                Some(id) if !j.wanted.remove(&id) => continue,
                                Some(id) => break id,
                                None => j = cv.wait(j).expect("jobs wait"),
                            }
                        }
                    };
                    let t = Instant::now();
                    let r = decode(&path_for(&dir, id));
                    let ms = t.elapsed().as_secs_f64() * 1e3;
                    if tx.send_blocking((id, r, ms)).is_err() {
                        return;
                    }
                }
            });
        }
        let inner = Rc::new(Inner {
            cap,
            textures: RefCell::default(),
            stamp: Cell::new(0),
            bound: RefCell::default(),
            jobs,
            in_flight: Cell::new(0),
            stats: RefCell::default(),
        });
        let weak = Rc::downgrade(&inner);
        glib::spawn_future_local(async move {
            while let Ok((id, result, ms)) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                inner.in_flight.set(inner.in_flight.get().saturating_sub(1));
                ThumbCache(inner).accept(id, result, ms);
            }
        });
        ThumbCache(inner)
    }

    fn accept(&self, id: u32, result: Result<Vec<u8>, String>, ms: f64) {
        let inner = &self.0;
        let mut st = inner.stats.borrow_mut();
        st.decode_ms.push(ms);
        match result {
            Ok(rgba) => {
                st.decoded += 1;
                drop(st);
                let tex: gdk::Texture = gdk::MemoryTexture::new(
                    THUMB_PX as i32,
                    THUMB_PX as i32,
                    gdk::MemoryFormat::R8g8b8a8Premultiplied,
                    &glib::Bytes::from_owned(rgba),
                    THUMB_PX as usize * 4,
                )
                .upcast();
                self.insert(id, tex.clone());
                if let Some(p) = inner.bound.borrow().get(&id).and_then(|w| w.upgrade()) {
                    p.set_paintable(Some(&tex));
                }
            }
            Err(_) => st.failed += 1,
        }
    }

    fn insert(&self, id: u32, tex: gdk::Texture) {
        let inner = &self.0;
        let mut map = inner.textures.borrow_mut();
        let s = inner.stamp.get() + 1;
        inner.stamp.set(s);
        map.insert(id, (tex, s));
        while map.len() > inner.cap {
            if let Some(oldest) = map.iter().min_by_key(|(_, (_, s))| *s).map(|(k, _)| *k) {
                map.remove(&oldest);
                inner.stats.borrow_mut().evicted += 1;
            }
        }
    }

    /// A row showing `pic` is now bound to `id`: show the cached texture or queue a decode.
    pub fn bind(&self, id: u32, pic: &gtk4::Picture) {
        let inner = &self.0;
        inner.bound.borrow_mut().insert(id, pic.downgrade());
        let hit = {
            let mut map = inner.textures.borrow_mut();
            map.get_mut(&id).map(|(t, s)| {
                inner.stamp.set(inner.stamp.get() + 1);
                *s = inner.stamp.get();
                t.clone()
            })
        };
        let mut st = inner.stats.borrow_mut();
        st.requested += 1;
        if let Some(t) = hit {
            st.hits += 1;
            pic.set_paintable(Some(&t));
            return;
        }
        drop(st);
        pic.set_paintable(None::<&gdk::Paintable>);
        let (m, cv) = &*inner.jobs;
        let mut j = m.lock().expect("jobs lock");
        if j.wanted.insert(id) {
            j.stack.push(id);
            inner.in_flight.set(inner.in_flight.get() + 1);
            cv.notify_one();
        }
    }

    /// The row bound to `id` was recycled or scrolled away.
    pub fn unbind(&self, id: u32) {
        let inner = &self.0;
        inner.bound.borrow_mut().remove(&id);
        let (m, _) = &*inner.jobs;
        if m.lock().expect("jobs lock").wanted.remove(&id) {
            inner.in_flight.set(inner.in_flight.get().saturating_sub(1));
        }
    }

    /// Decodes queued or running whose result has not been delivered or cancelled.
    pub fn pending(&self) -> u64 {
        self.0.in_flight.get()
    }

    pub fn cached(&self) -> usize {
        self.0.textures.borrow().len()
    }

    pub fn stats(&self) -> Stats {
        self.0.stats.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hatate-thumbs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn every_id_gets_its_own_file_and_they_all_decode() {
        let d = scratch("gen");
        assert_eq!(generate(&d, 12).unwrap(), 12);
        // Second run keeps the files.
        assert_eq!(generate(&d, 12).unwrap(), 0);
        let mut seen = HashSet::new();
        for id in 0..12 {
            let raw = decode(&path_for(&d, id)).unwrap();
            assert_eq!(raw.len(), (THUMB_PX * THUMB_PX * 4) as usize);
            assert!(
                seen.insert(raw),
                "id {id} decoded to the same pixels as an earlier id"
            );
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        assert!(decode(Path::new("/nonexistent/t_00000.jpg")).is_err());
    }

    #[test]
    fn generated_files_are_real_jpegs() {
        let d = scratch("magic");
        generate(&d, 1).unwrap();
        let b = std::fs::read(path_for(&d, 0)).unwrap();
        assert_eq!(&b[..2], &[0xFF, 0xD8]);
        assert!(b.len() > 2_000, "{} bytes", b.len());
        std::fs::remove_dir_all(&d).ok();
    }
}
