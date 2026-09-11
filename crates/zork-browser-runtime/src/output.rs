//! Reliable control messages and a replaceable latest paint frame. A slow
//! consumer never queues an unbounded sequence of full page images.
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    io::Write,
    sync::{Arc, Condvar, Mutex, OnceLock},
};
struct Packet {
    header: Value,
    pixels: Arc<Vec<u8>>,
}
#[derive(Default)]
struct Queue {
    control: VecDeque<Packet>,
    frame: Option<Packet>,
}
static OUTPUT: OnceLock<Arc<(Mutex<Queue>, Condvar)>> = OnceLock::new();
pub fn start(mut out: Box<dyn Write + Send>) {
    let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
    let _ = OUTPUT.set(queue.clone());
    std::thread::spawn(move || loop {
        let packet = {
            let (lock, ready) = &*queue;
            let mut q = lock.lock().unwrap();
            while q.control.is_empty() && q.frame.is_none() {
                q = ready.wait(q).unwrap();
            }
            q.control.pop_front().or_else(|| q.frame.take()).unwrap()
        };
        let header = serde_json::to_vec(&packet.header).unwrap();
        let mut write = || -> std::io::Result<()> {
            out.write_all(&(header.len() as u32).to_le_bytes())?;
            out.write_all(&(packet.pixels.len() as u32).to_le_bytes())?;
            out.write_all(&header)?;
            out.write_all(&packet.pixels)?;
            out.flush()
        };
        if write().is_err() {
            crate::engine::post(json!({"method":"Browser.close"}));
            break;
        }
    });
}
pub fn control(header: Value) {
    if let Some(queue) = OUTPUT.get() {
        queue.0.lock().unwrap().control.push_back(Packet {
            header,
            pixels: Arc::new(Vec::new()),
        });
        queue.1.notify_one();
    }
}
pub fn paint(tab: String, width: i32, height: i32, scale: f32, pixels: Arc<Vec<u8>>) {
    if let Some(queue) = OUTPUT.get() {
        queue.0.lock().unwrap().frame = Some(Packet {
            header: json!({"method":"Zork.paint","sessionId":tab.to_string(),"params":{"width":width,"height":height,"scale":scale}}),
            pixels,
        });
        queue.1.notify_one();
    }
}
