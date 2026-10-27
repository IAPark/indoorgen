//! Architectural room features generated after the floor plan and before the furniture solve: masonry fireplaces with chimney breasts,
//! bookshelf walls, window seats. Each feature is built flush against one wall span of a room and then becomes a fixed obstacle
//! (and, where it has shelves / a mantel / a seat, a support surface) for the furniture solver, mess clutter and people.
//!
//! Coordinates of a built feature are in the span's local frame:
//!   x: along the wall, 0 .. span.length (left to right when facing the wall from inside the room)
//!   y: into the room, 0 = the wall's inner face, positive = towards the room interior
//!   z: up, 0 = finished floor, span.height = ceiling
//! The placement code maps this frame to world space (rotation about z + translation).
use ig_kit::api::Asset;

/// An opening (door or window) on the wall span, in span coordinates.
#[derive(Clone, Debug)]
pub struct Opening {
    pub is_window: bool,
    /// [x0, x1] along the wall
    pub x: [f64; 2],
    /// [z0, z1] (sill / head height)
    pub z: [f64; 2],
}

/// A straight wall span available to a feature.
#[derive(Clone, Debug)]
pub struct WallSpan {
    /// usable length along the wall (m)
    pub length: f64,
    /// floor-to-ceiling height (m)
    pub height: f64,
    /// wall thickness (m), e.g. for a window seat set into a deep window reveal
    pub wall_thickness: f64,
    /// openings that intersect this span (features must keep doors clear; window seats sit under a window)
    pub openings: Vec<Opening>,
    /// room type ("LivingRoom", "Bedroom", ...)
    pub room_type: String,
}

/// A built feature.
pub struct FeatureAsset {
    /// geometry + materials (parts carry Infinigen-style material specs so Cycles can shade them); supports = tops of shelves,
    /// mantel, hearth, seat cushion (as boxes, like ig_kit assets)
    pub asset: Asset,
    /// footprint on the floor in span coordinates (polygon, counter-clockwise): the area the solver must keep free of furniture
    pub footprint: Vec<[f64; 2]>,
    /// extra keep-out boxes in span coordinates (lo, hi), e.g. the hearth's clearance zone in front of a fireplace
    pub keepout: Vec<([f64; 3], [f64; 3])>,
    /// class name used for node names / sidecar objects / rules ("FireplaceFeature", "BookshelfWallFeature", "WindowSeatFeature")
    pub class: &'static str,
}

/// A feature generator. `fits` says whether (and where along x) the feature can go on a span; `build` makes it.
pub trait Feature: Send + Sync {
    fn class(&self) -> &'static str;
    /// candidate x-intervals [x0, x1] on this span where the feature fits (empty = doesn't fit)
    fn fits(&self, span: &WallSpan) -> Vec<[f64; 2]>;
    /// build the feature occupying `x` (one of the intervals returned by `fits`, possibly shrunk) on `span`, deterministically from `seed`
    fn build(&self, span: &WallSpan, x: [f64; 2], seed: u64) -> FeatureAsset;
}

pub mod bookshelf;
pub mod util;
pub mod windowseat;

/// the built-in feature generators implemented in this crate (the fireplace worker adds FireplaceFeature to its own list)
pub fn builtin_features() -> Vec<Box<dyn Feature>> {
    vec![Box::new(bookshelf::BookshelfWallFeature), Box::new(windowseat::WindowSeatFeature)]
}
