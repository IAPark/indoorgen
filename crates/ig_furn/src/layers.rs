//! The layers of the layout (L0 plan: rooms, walls, doors, windows, built-ins; L1 room: where each functional group goes; L2 furniture:
//! each piece; L3 on top: small items) and what one layer tells the layer above when it cannot do its job: a complaint, with the room,
//! a kind, where it is (when known), what is needed and whether it is hard (the layout is wrong) or soft (it is worse than it could be).
//! The passes report complaints into a per-thread sink while a room is solved (each room is solved on one thread); the solve drains it.
//! The driver (scenegen, HIER=1) answers hard complaints by moving the plan's walls and solving again.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Plan,
    Room,
    Furniture,
    OnTop,
}

#[derive(Clone, Debug)]
pub struct Complaint {
    pub layer: Layer,
    pub room: String,
    pub kind: &'static str,
    pub at: Option<[f64; 2]>,
    pub need: String,
    pub hard: bool,
}

/// a layer's verdict: its score (higher is better) and what it could not do
#[derive(Clone, Debug, Default)]
pub struct LayerResult {
    pub score: f64,
    pub complaints: Vec<Complaint>,
}

impl LayerResult {
    pub fn hard(&self) -> usize { self.complaints.iter().filter(|c| c.hard).count() }
    pub fn soft(&self) -> usize { self.complaints.iter().filter(|c| !c.hard).count() }
}

thread_local! {
    static SINK: std::cell::RefCell<Vec<Complaint>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// report a complaint from inside a pass
pub fn complain(layer: Layer, room: &str, kind: &'static str, at: Option<[f64; 2]>, need: impl Into<String>, hard: bool) {
    let c = Complaint { layer, room: room.to_string(), kind, at, need: need.into(), hard };
    if ig_core::env_on!("LAYERDBG") { eprintln!("COMPLAINT {:?} {} {} {} at {:?}: {}", c.layer, if c.hard { "hard" } else { "soft" }, c.room, c.kind, c.at, c.need); }
    SINK.with(|s| s.borrow_mut().push(c));
}

/// the complaints reported on this thread since the last drain
pub fn drain() -> Vec<Complaint> {
    SINK.with(|s| std::mem::take(&mut *s.borrow_mut()))
}
