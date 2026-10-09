//! Background tile compositing for the canvas (architecture rule 6).
//!
//! When a document changes, the canvas asks for the damaged tiles. Worker
//! threads composite them from a snapshot of the document and send back
//! display-ready 8-bit tiles. A tile requested again before its job runs
//! is skipped, so stale work is cancelled rather than finished.

use eframe::egui::{self, ColorImage};
use iw_engine::document::{AnyTile, Document};
use iw_engine::pixel::ByDepth;
use iw_engine::tile::{TileCoord, TILE_SIZE};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

/// Identifies an open document across the app.
pub type DocKey = u64;

struct Job {
    doc: DocKey,
    coord: TileCoord,
    generation: u64,
    snapshot: Arc<Document>,
}

/// A finished tile. `image` is `None` if the tile is now empty.
pub struct Rendered {
    pub doc: DocKey,
    pub coord: TileCoord,
    pub generation: u64,
    pub image: Option<ColorImage>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    /// The newest generation requested for each tile. A job for an older
    /// generation is out of date and is dropped.
    latest: HashMap<(DocKey, TileCoord), u64>,
    shutdown: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct Renderer {
    shared: Shared,
    results: Receiver<Rendered>,
    workers: Vec<std::thread::JoinHandle<()>>,
    next_generation: u64,
}

impl Renderer {
    /// Starts worker threads. `ctx` is woken whenever a tile is ready.
    pub fn new(ctx: egui::Context) -> Self {
        let shared: Shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (tx, results) = channel();
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get().saturating_sub(1).clamp(1, 8));
        let workers = (0..threads)
            .map(|i| {
                let shared = shared.clone();
                let tx = tx.clone();
                let ctx = ctx.clone();
                std::thread::Builder::new()
                    .name(format!("composite-{i}"))
                    .spawn(move || worker(shared, tx, ctx))
                    .expect("cannot start a compositing thread")
            })
            .collect();
        Self {
            shared,
            results,
            workers,
            next_generation: 1,
        }
    }

    /// Queues `coords` of `snapshot` for compositing, nearest to `focus`
    /// first so what is on screen arrives first. Returns the generation
    /// to expect in the results.
    pub fn request(
        &mut self,
        doc: DocKey,
        snapshot: Arc<Document>,
        mut coords: Vec<TileCoord>,
        focus: Option<TileCoord>,
    ) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        if let Some(f) = focus {
            coords.sort_by_key(|c| (c.tx - f.tx).abs().max((c.ty - f.ty).abs()));
        }
        let (lock, wake) = &*self.shared;
        let mut queue = lock.lock().expect("compositor queue poisoned");
        for coord in coords {
            queue.latest.insert((doc, coord), generation);
            queue.jobs.push_back(Job {
                doc,
                coord,
                generation,
                snapshot: snapshot.clone(),
            });
        }
        wake.notify_all();
        generation
    }

    /// Drops everything queued for a document that is being closed.
    pub fn forget(&mut self, doc: DocKey) {
        let (lock, _) = &*self.shared;
        let mut queue = lock.lock().expect("compositor queue poisoned");
        queue.jobs.retain(|j| j.doc != doc);
        queue.latest.retain(|(d, _), _| *d != doc);
    }

    /// Whether `result` is the newest version of its tile.
    pub fn is_current(&self, result: &Rendered) -> bool {
        let (lock, _) = &*self.shared;
        let queue = lock.lock().expect("compositor queue poisoned");
        queue.latest.get(&(result.doc, result.coord)) == Some(&result.generation)
    }

    /// Finished tiles since the last call.
    pub fn take_results(&self) -> Vec<Rendered> {
        self.results.try_iter().collect()
    }

    /// Number of tiles still waiting to be composited.
    pub fn pending(&self) -> usize {
        let (lock, _) = &*self.shared;
        lock.lock().map(|q| q.jobs.len()).unwrap_or(0)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        {
            let (lock, wake) = &*self.shared;
            if let Ok(mut queue) = lock.lock() {
                queue.shutdown = true;
                queue.jobs.clear();
            }
            wake.notify_all();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker(shared: Shared, tx: Sender<Rendered>, ctx: egui::Context) {
    let (lock, wake) = &*shared;
    loop {
        let job = {
            let mut queue = match lock.lock() {
                Ok(q) => q,
                Err(_) => return,
            };
            loop {
                if queue.shutdown {
                    return;
                }
                match queue.jobs.pop_front() {
                    // Skip work that a newer request has replaced.
                    Some(job)
                        if queue.latest.get(&(job.doc, job.coord)) != Some(&job.generation) =>
                    {
                        continue
                    }
                    Some(job) => break job,
                    None => queue = wake.wait(queue).expect("compositor queue poisoned"),
                }
            }
        };
        let image = job
            .snapshot
            .composite_tile(job.coord)
            .map(|tile| display_image(&tile));
        let done = Rendered {
            doc: job.doc,
            coord: job.coord,
            generation: job.generation,
            image,
        };
        if tx.send(done).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// Converts a composited tile to 8-bit premultiplied RGBA for display.
pub fn display_image(tile: &AnyTile) -> ColorImage {
    let size = TILE_SIZE as usize;
    let mut bytes = Vec::with_capacity(size * size * 4);
    match tile {
        ByDepth::U8(t) => t.pixels().iter().for_each(|p| bytes.extend_from_slice(p)),
        ByDepth::U16(t) => {
            for p in t.pixels() {
                bytes.extend(
                    p.iter()
                        .map(|v| ((u32::from(*v) * 255 + 32767) / 65535) as u8),
                );
            }
        }
        ByDepth::F32(t) => {
            for p in t.pixels() {
                let a = p[3].clamp(0.0, 1.0);
                // Keep colour within alpha so the result stays valid
                // premultiplied data even for values above 1.
                bytes.extend(p[..3].iter().map(|v| (v.clamp(0.0, a) * 255.0 + 0.5) as u8));
                bytes.push((a * 255.0 + 0.5) as u8);
            }
        }
    }
    ColorImage::from_rgba_premultiplied([size, size], &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iw_engine::pixel::BitDepth;
    use std::time::{Duration, Instant};

    fn wait_for(renderer: &Renderer, count: usize) -> Vec<Rendered> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut out = Vec::new();
        while out.len() < count && Instant::now() < deadline {
            out.extend(renderer.take_results());
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }

    #[test]
    fn requested_tiles_come_back_composited() {
        let mut renderer = Renderer::new(egui::Context::default());
        let doc = Arc::new(
            Document::with_background(300, 10, BitDepth::U8, [1.0, 0.0, 0.0, 1.0]).unwrap(),
        );
        let coords = vec![
            TileCoord::new(0, 0),
            TileCoord::new(1, 0),
            TileCoord::new(5, 5),
        ];
        let generation = renderer.request(7, doc, coords, Some(TileCoord::new(0, 0)));
        let results = wait_for(&renderer, 3);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!((r.doc, r.generation), (7, generation));
            assert!(renderer.is_current(r));
            match r.coord {
                c if c == TileCoord::new(5, 5) => assert!(r.image.is_none(), "empty tile"),
                _ => assert_eq!(
                    r.image.as_ref().unwrap().pixels[0],
                    egui::Color32::from_rgb(255, 0, 0)
                ),
            }
        }
    }

    #[test]
    fn newer_requests_replace_older_ones() {
        let mut renderer = Renderer::new(egui::Context::default());
        let old = Arc::new(
            Document::with_background(10, 10, BitDepth::U8, [0.0, 0.0, 1.0, 1.0]).unwrap(),
        );
        let new = Arc::new(
            Document::with_background(10, 10, BitDepth::U8, [0.0, 1.0, 0.0, 1.0]).unwrap(),
        );
        let coord = TileCoord::new(0, 0);
        renderer.request(1, old, vec![coord], None);
        let latest = renderer.request(1, new, vec![coord], None);
        let results = wait_for(&renderer, 1);
        std::thread::sleep(Duration::from_millis(50));
        let results: Vec<_> = results.into_iter().chain(renderer.take_results()).collect();
        let current: Vec<_> = results.iter().filter(|r| renderer.is_current(r)).collect();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].generation, latest);
        assert_eq!(
            current[0].image.as_ref().unwrap().pixels[0],
            egui::Color32::from_rgb(0, 255, 0)
        );
    }

    #[test]
    fn deeper_documents_display_as_8_bit() {
        let doc = Document::with_background(4, 4, BitDepth::U16, [1.0, 0.5, 0.0, 1.0]).unwrap();
        let image = display_image(&doc.composite_tile(TileCoord::new(0, 0)).unwrap());
        assert_eq!(image.pixels[0], egui::Color32::from_rgb(255, 128, 0));
        let doc = Document::with_background(4, 4, BitDepth::F32, [3.0, 0.5, 0.0, 1.0]).unwrap();
        let image = display_image(&doc.composite_tile(TileCoord::new(0, 0)).unwrap());
        assert_eq!(
            image.pixels[0],
            egui::Color32::from_rgb(255, 128, 0),
            "over-range values clamp"
        );
    }
}
