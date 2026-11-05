//! Decode large artwork away from the event loop and retain a bounded set of display images.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

use crate::{AppWindow, LibraryState};

type Source = (u64, Arc<[u8]>);
type Decoded = (Source, Option<SharedPixelBuffer<Rgba8Pixel>>);

#[derive(Default)]
struct Cache {
    focus: Option<Source>,
    pending: HashMap<u64, Arc<[u8]>>,
    images: VecDeque<(Source, Option<Image>)>,
}

#[derive(Default)]
pub(super) struct Artworks {
    cache: Rc<RefCell<Cache>>,
    jobs: Option<mpsc::Sender<Source>>,
    results: Option<Rc<mpsc::Receiver<Decoded>>>,
    timer: Rc<slint::Timer>,
}

fn same(a: &Source, b: &Source) -> bool {
    a.0 == b.0 && Arc::ptr_eq(&a.1, &b.1)
}

fn decode(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let image = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    // Keep CPU resampling and texture sizes bounded, including for old disk-cache entries.
    let image = image.thumbnail(1280, 720).into_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        image.as_raw(),
        image.width(),
        image.height(),
    ))
}

impl Artworks {
    pub(super) fn clear_focus(&self) {
        self.cache.borrow_mut().focus = None;
    }

    pub(super) fn get(&mut self, ui: &AppWindow, id: u64, bytes: &Arc<[u8]>) -> Option<Image> {
        let source = (id, bytes.clone());
        let mut cache = self.cache.borrow_mut();
        cache.focus = Some(source.clone());
        if let Some(index) = cache.images.iter().position(|(key, _)| same(key, &source)) {
            let entry = cache.images.remove(index).unwrap();
            let image = entry.1.clone();
            cache.images.push_back(entry);
            return image;
        }
        if cache
            .pending
            .get(&id)
            .is_some_and(|pending| Arc::ptr_eq(pending, bytes))
        {
            return None;
        }
        cache.pending.insert(id, bytes.clone());
        drop(cache);
        if self.jobs.is_none() {
            let (send, jobs) = mpsc::channel::<Source>();
            let (results, receive) = mpsc::channel();
            // One worker serializes decodes even while the user rapidly changes selection.
            std::thread::spawn(move || {
                for source in jobs {
                    let pixels = decode(&source.1);
                    if results.send((source, pixels)).is_err() {
                        break;
                    }
                }
            });
            self.jobs = Some(send);
            self.results = Some(Rc::new(receive));
        }
        let _ = self.jobs.as_ref().unwrap().send(source);
        if !self.timer.running() {
            let cache = self.cache.clone();
            let receive = self.results.as_ref().unwrap().clone();
            let weak = ui.as_weak();
            let timer = Rc::downgrade(&self.timer);
            self.timer.start(
                slint::TimerMode::Repeated,
                Duration::from_millis(16),
                move || {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    for (source, pixels) in receive.try_iter() {
                        complete(&mut cache.borrow_mut(), &ui, source, pixels);
                    }
                    if cache.borrow().pending.is_empty()
                        && let Some(timer) = timer.upgrade()
                    {
                        timer.stop();
                    }
                },
            );
        }
        None
    }

    #[cfg(test)]
    pub(super) fn wait(&self, ui: &AppWindow) {
        while !self.cache.borrow().pending.is_empty() {
            let (source, pixels) = self
                .results
                .as_ref()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            complete(&mut self.cache.borrow_mut(), ui, source, pixels);
        }
    }
}

fn complete(
    cache: &mut Cache,
    ui: &AppWindow,
    source: Source,
    pixels: Option<SharedPixelBuffer<Rgba8Pixel>>,
) {
    if !cache
        .pending
        .get(&source.0)
        .is_some_and(|pending| Arc::ptr_eq(pending, &source.1))
    {
        return;
    }
    cache.pending.remove(&source.0);
    let image = pixels.map(Image::from_rgba8);
    if cache
        .focus
        .as_ref()
        .is_some_and(|focus| same(focus, &source))
    {
        let state = ui.global::<LibraryState>();
        state.set_detail_has_cover(image.is_some());
        state.set_detail_cover(image.clone().unwrap_or_default());
    }
    cache.images.push_back((source, image));
    if cache.images.len() > 8 {
        cache.images.pop_front();
    }
}
