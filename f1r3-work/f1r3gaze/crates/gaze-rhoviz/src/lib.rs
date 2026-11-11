//! `gaze-rhoviz` — a rho term drawn in three dimensions, and its reduction
//! animated.
//!
//! This is the service behind the `rho:gaze:rhoviz` capability. A page hands
//! it a term and a step count; it answers with pictures. f1r3lang has no
//! arithmetic, so everything numerical lives here and the page's program is
//! pure coordination: it carries input values and pointer events in, and
//! carries each picture to the document.
//!
//! **The figure** is the chromatic realisation of the term's syntax tree: the
//! constructor of a node is its colour, the position of an argument is one of
//! three orthogonal axes, and child `i` sits along axis `i` just past the
//! reach of its siblings along that axis. Every figure lies in the positive
//! orthant of its own hub, so sibling figures are disjoint and no two spokes
//! meet except at a shared hub; and because a child is moved only as far as
//! its siblings actually reach, a figure's size is a sum over its parts, not
//! a power of its depth.
//!
//! **The reduction** is run by a private CampF1R3 machine, one component per
//! tick. After each tick the whole world is read back as one parallel
//! composition (queued components, data at rest as sends, continuations at
//! rest as receipts) and a frame is recorded when that composition changes.
//! Frames are computed *lazily*, as playback reaches them, so a term that
//! never comes to rest can be watched for as long as one likes without the
//! browser ever waiting on it.
//!
//! Portable: `std` and CampF1R3 only, so it serves the native browser and the
//! reach tier alike.

#![forbid(unsafe_code)]

use b3ll0ws::{Direct, Machine, Sequential, Step};
use campf1r3_core::Config;
use k1ndl1ng_campf1r3::Chan;
use k1ndl1ng_norm::{normalise, show_pretty, CollKind, NBind, Name, Node, Norm, Options as NOpts};
use k1ndl1ng_parse::{parse, Level, Options as POpts};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

const GAP: f64 = 1.0;
/// Milliseconds of playback per step: a hold, then the move to the next frame.
const STEP_MS: f64 = 2800.0;
/// The fraction of a step spent holding still before the figure moves.
const HOLD: f64 = 0.4;
/// Ticks one frame may consume without the world visibly changing.
const MAX_TICKS_PER_FRAME: u64 = 20_000;
/// Frames kept. Playback can seek backwards, so they are all retained.
const MAX_FRAMES: usize = 5_000;

/// One colour per constructor (the Okabe–Ito palette, which survives the
/// common colour-vision deficiencies), with the legend text for each.
pub const KINDS: &[(&str, &str, &str)] = &[
    ("par", "#0072B2", "| parallel"),
    ("recv", "#D55E00", "for: pattern, channel, body"),
    ("send", "#009E73", "! send: channel, payload"),
    ("quote", "#CC79A7", "@ quote"),
    ("drop", "#56B4E9", "* dereference"),
    ("new", "#B8A400", "new"),
    ("list", "#8C8C8C", "list, tuple, map"),
    ("nil", "#000000", "Nil"),
    ("var", "#E69F00", "variable"),
    ("unf", "#6A3D9A", "unforgeable name"),
    ("lit", "#117733", "literal"),
];
const LEAVING: &str = "#C1121F";
const ARRIVED: &str = "#1B7F3B";

fn colour(kind: &str) -> &'static str {
    KINDS.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or("#000000")
}

// ---- the drawable tree ------------------------------------------------------

/// One node of the figure. At most three children, so three axes suffice.
struct V {
    kind: &'static str,
    label: String,
    kids: Vec<V>,
    /// How far the figure reaches along each axis from its hub.
    ext: [f64; 3],
}

fn leaf(kind: &'static str, label: impl Into<String>) -> V {
    V { kind, label: label.into(), kids: Vec::new(), ext: [0.0; 3] }
}

/// A node over `kids`, measured.
fn node(kind: &'static str, label: impl Into<String>, kids: Vec<V>) -> V {
    let exts: Vec<[f64; 3]> = kids.iter().map(|k| k.ext).collect();
    let (_, ext) = arrange(&exts);
    V { kind, label: label.into(), kids, ext }
}

/// Where the children of one node go, and how far the whole then reaches.
///
/// Child `i` goes along axis `i` to just past the furthest any *sibling*
/// reaches along that axis. Then no sibling has a point with coordinate `i`
/// as large as child `i`'s smallest, so the figures are disjoint; a spoke
/// lies on an axis where every other child has a positive coordinate, so it
/// meets none of them; and it reaches its own child from below, where that
/// child, lying in the positive orthant of its hub, has nothing.
fn arrange(kids: &[[f64; 3]]) -> (Vec<f64>, [f64; 3]) {
    let mut ext = [0.0f64; 3];
    let mut at = Vec::with_capacity(kids.len());
    for i in 0..kids.len() {
        let others = kids.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, e)| e[i]).fold(0.0, f64::max);
        at.push(others + GAP);
    }
    for (j, e) in kids.iter().enumerate() {
        for k in 0..3 {
            ext[k] = ext[k].max(e[k] + if j == k { at[j] } else { 0.0 });
        }
    }
    (at, ext)
}

/// A sequence as a right-nested chain of `list` nodes: (head, rest).
fn chain(mut xs: Vec<V>, label: &str) -> V {
    let Some(mut acc) = xs.pop() else { return leaf("list", label) };
    if xs.is_empty() && label != "," {
        return node("list", label, vec![acc]);
    }
    while let Some(x) = xs.pop() {
        acc = node("list", label, vec![x, acc]);
    }
    acc
}

/// Parallel composition is associative, so the bracketing is ours to choose.
/// A balanced tree keeps the spine short.
fn balanced(mut xs: Vec<V>) -> V {
    match xs.len() {
        0 => leaf("nil", "Nil"),
        1 => xs.pop().unwrap(),
        n => {
            let right = xs.split_off(n.div_ceil(2));
            node("par", "|", vec![balanced(xs), balanced(right)])
        }
    }
}

fn name_tree(n: &Name) -> V {
    match n {
        Name::Quote(p) => node("quote", "@", vec![proc_tree(p)]),
        Name::Bound(i) => leaf("var", format!("n{i}")),
        Name::Free(l) => leaf("var", format!("f{l}")),
        Name::Unforgeable(b) => leaf("unf", format!("u_{:02x}{:02x}{:02x}{:02x}", b[0], b[1], b[2], b[3])),
    }
}

fn short(t: &Norm) -> String {
    let s = show_pretty(t);
    if s.chars().count() > 14 {
        let mut cut: String = s.chars().take(13).collect();
        cut.push('…');
        cut
    } else {
        s
    }
}

fn proc_tree(t: &Norm) -> V {
    match t.node() {
        Node::Nil => leaf("nil", "Nil"),
        Node::Par(v) => balanced(v.iter().map(proc_tree).collect()),
        Node::Send { chan, persistent, args } => {
            let mut kids = vec![name_tree(chan)];
            if !args.is_empty() {
                kids.push(chain(args.iter().map(proc_tree).collect(), ","));
            }
            node("send", if *persistent { "!!" } else { "!" }, kids)
        }
        Node::Receive { binds, body } => {
            // A join is a chain of receipt nodes, one per bind, ending in the
            // body: (patterns, channel, rest). A single bind is (y, x, P).
            let mut acc = proc_tree(body);
            for (i, b) in binds.iter().enumerate().rev() {
                let pats = if b.pats.is_empty() {
                    leaf("var", "()")
                } else {
                    chain(b.pats.iter().map(name_tree).collect(), ",")
                };
                let head = if i == 0 { "for" } else { "&" };
                acc = node("recv", format!("{head} {}", b.kind.arrow()), vec![pats, name_tree(&b.chan), acc]);
            }
            acc
        }
        Node::New { count, body } => node("new", format!("new {count}"), vec![proc_tree(body)]),
        Node::Eval(n) => node("drop", "*", vec![name_tree(n)]),
        Node::BoundVar(i) => leaf("var", format!("p{i}")),
        Node::FreeVar(l) => leaf("var", format!("P{l}")),
        Node::Wild => leaf("var", "_"),
        Node::Lit(_) => leaf("lit", short(t)),
        Node::Coll { kind, items } => chain(items.iter().map(proc_tree).collect(), coll_label(*kind)),
        Node::CollRest { kind, items, rest } => {
            let mut xs: Vec<V> = items.iter().map(proc_tree).collect();
            xs.push(proc_tree(rest));
            chain(xs, coll_label(*kind))
        }
    }
}

fn coll_label(k: CollKind) -> &'static str {
    match k {
        CollKind::List => "[ ]",
        CollKind::Tuple => "( )",
        CollKind::Map => "{ }",
    }
}

// ---- the chromatic realisation ----------------------------------------------

/// One placed hub.
#[derive(Clone, Debug)]
pub struct Hub {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub pos: [f64; 3],
    /// Index of the parent hub in the same frame, or -1 for the root.
    pub parent: i64,
    /// Bit 0: its component is about to react. Bit 1: it was just produced.
    pub flag: u8,
}

/// Place `v` with its hub at `at`; child `i` goes along axis `i`.
fn place(v: &V, id: String, at: [f64; 3], parent: i64, flag: u8, out: &mut Vec<Hub>) {
    let me = out.len() as i64;
    out.push(Hub { id: id.clone(), kind: v.kind, label: v.label.clone(), pos: at, parent, flag });
    let exts: Vec<[f64; 3]> = v.kids.iter().map(|k| k.ext).collect();
    let (lambda, _) = arrange(&exts);
    for (i, k) in v.kids.iter().enumerate() {
        let mut p = at;
        p[i] += lambda[i];
        place(k, format!("{id}.{i}"), p, me, flag, out);
    }
}

struct Slot<'a> {
    id: &'a str,
    tree: V,
    flag: u8,
}

/// How far a balanced par over `slots` would reach.
fn spine_extent(slots: &[Slot]) -> [f64; 3] {
    match slots.len() {
        0 => [0.0; 3],
        1 => slots[0].tree.ext,
        n => {
            let (l, r) = slots.split_at(n.div_ceil(2));
            arrange(&[spine_extent(l), spine_extent(r)]).1
        }
    }
}

/// The top-level composition as one balanced par over the components. Each
/// component keeps its own id prefix, so its hubs are the same objects in
/// every frame it survives; the par spine is identified by position.
fn spine(slots: &[Slot], path: String, at: [f64; 3], parent: i64, out: &mut Vec<Hub>) {
    match slots.len() {
        0 => out.push(Hub { id: "top".into(), kind: "nil", label: "Nil".into(), pos: at, parent, flag: 0 }),
        1 => place(&slots[0].tree, slots[0].id.to_string(), at, parent, slots[0].flag, out),
        n => {
            let (l, r) = slots.split_at(n.div_ceil(2));
            let (lambda, _) = arrange(&[spine_extent(l), spine_extent(r)]);
            let me = out.len() as i64;
            out.push(Hub { id: format!("top{path}"), kind: "par", label: "|".into(), pos: at, parent, flag: 0 });
            for (i, half) in [l, r].into_iter().enumerate() {
                let mut p = at;
                p[i] += lambda[i];
                spine(half, format!("{path}.{i}"), p, me, out);
            }
        }
    }
}

// ---- reading the world back as one term -------------------------------------

type M = Machine<Direct, Sequential>;
type Comps = Vec<(String, Norm)>;

/// Everything alive, as parallel components: queued jobs, data at rest shown
/// as the sends they came from, continuations at rest shown as receipts.
fn world(m: &M) -> Vec<Norm> {
    let mut out: Vec<Norm> = Vec::new();
    for j in m.jobs() {
        out.extend(j.term.top_level());
    }
    let chans = m.channels();
    for c in &chans {
        for d in m.space().get_data(c) {
            out.push(Norm::send(c.name().clone(), false, d.args.clone()));
        }
    }
    for c in &chans {
        let mut groups: Vec<Vec<Chan>> = vec![vec![c.clone()]];
        for g in m.space().get_joins(c) {
            if g.first() == Some(c) && g.len() > 1 {
                groups.push(g);
            }
        }
        for g in groups {
            for (pats, k, _persist) in m.space().get_waiting(&g) {
                let binds: Vec<NBind> = g
                    .iter()
                    .zip(pats.iter())
                    .map(|(ch, p)| NBind::new(p.kind, ch.name().clone(), p.pats.clone(), p.binders))
                    .collect();
                out.push(Norm::receive(binds, k.body.clone()));
            }
        }
    }
    out
}

/// Give each component an id that survives across frames: its content hash,
/// plus an occurrence number so that two equal components are two objects.
fn identify(terms: Vec<Norm>) -> Comps {
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut out: Comps = terms.into_iter().map(|t| (t.hash().hex(), t)).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    for (h, _) in out.iter_mut() {
        let n = seen.entry(h.clone()).or_insert(0);
        let id = format!("{}#{}", &h[..16], *n);
        *n += 1;
        *h = id;
    }
    out
}

/// Keep survivors where they were and put arrivals where the first departure
/// was, so that a component which took no part in a step does not jump.
fn reorder(prev: &Comps, next: Comps) -> Comps {
    let mut by_id: HashMap<String, Norm> = next.into_iter().collect();
    let mut out: Comps = Vec::new();
    let mut hole: Option<usize> = None;
    for (id, _) in prev {
        match by_id.remove(id) {
            Some(t) => out.push((id.clone(), t)),
            None => {
                hole.get_or_insert(out.len());
            }
        }
    }
    let mut fresh: Comps = by_id.into_iter().collect();
    fresh.sort_by(|a, b| a.0.cmp(&b.0));
    let at = hole.unwrap_or(out.len());
    out.splice(at..at, fresh);
    out
}

fn id_set(c: &Comps) -> HashSet<String> {
    c.iter().map(|x| x.0.clone()).collect()
}

/// One recorded state of the world, laid out.
pub struct Frame {
    comps: Comps,
    pub note: String,
    pub text: String,
    pub hubs: Vec<Hub>,
    index: HashMap<String, usize>,
    centre: [f64; 3],
    extent: f64,
}

impl Frame {
    fn new(comps: Comps, note: String, before: Option<&HashSet<String>>) -> Frame {
        let text = if comps.is_empty() {
            "Nil".to_string()
        } else {
            comps.iter().map(|c| show_pretty(&c.1)).collect::<Vec<_>>().join("\n| ")
        };
        let mut f = Frame { comps, note, text, hubs: Vec::new(), index: HashMap::new(), centre: [0.0; 3], extent: 1.0 };
        f.lay_out(before, None);
        f
    }

    fn lay_out(&mut self, before: Option<&HashSet<String>>, after: Option<&HashSet<String>>) {
        let slots: Vec<Slot> = self
            .comps
            .iter()
            .map(|(id, t)| {
                let leaving = after.is_some_and(|s| !s.contains(id));
                let arrived = before.is_some_and(|s| !s.contains(id));
                Slot { id, tree: proc_tree(t), flag: leaving as u8 | (arrived as u8) << 1 }
            })
            .collect();
        let keep: HashMap<String, u8> = self.hubs.iter().map(|h| (h.id.clone(), h.flag)).collect();
        let mut hubs = Vec::new();
        spine(&slots, String::new(), [0.0; 3], -1, &mut hubs);
        // A second layout only adds the `leaving` bit; keep `arrived` as it was.
        for h in &mut hubs {
            if let Some(f) = keep.get(&h.id) {
                h.flag |= f & 2;
            }
        }
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for h in &hubs {
            for i in 0..3 {
                lo[i] = lo[i].min(h.pos[i]);
                hi[i] = hi[i].max(h.pos[i]);
            }
        }
        self.centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        self.extent = hubs
            .iter()
            .map(|h| (0..3).map(|i| (h.pos[i] - self.centre[i]).powi(2)).sum::<f64>().sqrt())
            .fold(1.0, f64::max);
        self.index = hubs.iter().enumerate().map(|(i, h)| (h.id.clone(), i)).collect();
        self.hubs = hubs;
    }
}

// ---- a session ---------------------------------------------------------------

/// How many comm events to animate.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Steps {
    /// Stop after this many.
    Exactly(u64),
    /// Reduce to completion. Caveat emptor: a term need not have one.
    All,
}

impl Steps {
    /// `"*"` is [`Steps::All`]; otherwise a whole number.
    pub fn parse(s: &str) -> Result<Steps, String> {
        match s.trim() {
            "*" => Ok(Steps::All),
            n => n
                .parse::<u64>()
                .map(Steps::Exactly)
                .map_err(|_| format!("steps must be a whole number or *, not `{n}`")),
        }
    }
}

/// What a page receives when the picture has changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub svg: String,
    pub note: String,
    pub term: String,
    /// Whether playback is running, for a page to label its button by.
    pub playing: bool,
}

/// One term being watched: its machine, the frames reached so far, and the
/// state of playback and of the camera.
pub struct Session {
    machine: M,
    frames: Vec<Frame>,
    steps: Steps,
    comms: u64,
    /// `Some(why)` once no further frame will be computed.
    done: Option<String>,
    t: f64,
    playing: bool,
    spin: bool,
    yaw: f64,
    pitch: f64,
    zoom: f64,
    /// Where the figure's centre is drawn, in pixels from the picture's centre.
    pan: (f64, f64),
    size: (f64, f64),
    /// The picture's top-left corner in the page, to place pointer events.
    origin: (f64, f64),
    /// The pointer, in the picture's own pixels, when it was last seen.
    cursor: Option<(f64, f64)>,
    drag: Option<(i64, i64)>,
    /// Whether a drag moves the figure (true) or turns it (false).
    drag_moves: bool,
    last_now: Option<u64>,
    /// When a picture was last sent, and how large it was.
    last_emit: Option<u64>,
    last_bytes: usize,
    dirty: bool,
}

impl Session {
    /// Parse, normalise and deploy `source` on a fresh machine.
    pub fn open(source: &str, steps: Steps) -> Result<Session, String> {
        let p = parse(source, &POpts { level: Level::K1G, ..POpts::default() });
        if !p.ok() {
            let d = p.diags.first();
            return Err(d.map(|d| format!("at {}: {}", d.span.lo, d.message)).unwrap_or_else(|| "does not parse".into()));
        }
        let n = normalise(&p.tree, &NOpts::default())
            .map_err(|ds| ds.first().map(|d| format!("at {}: {}", d.span.lo, d.message)).unwrap_or_default())?;
        let free: Vec<String> = n.free.iter().map(|f| f.ident.clone()).collect();
        let mut machine = Machine::new(Config::default());
        machine.spawn(n.term.ground_free_by(&free));
        let first = Frame::new(identify(world(&machine)), "the term as given".into(), None);
        Ok(Session {
            machine,
            frames: vec![first],
            steps,
            comms: 0,
            done: None,
            t: 0.0,
            playing: true,
            spin: true,
            yaw: -0.7,
            pitch: 0.5,
            zoom: 1.0,
            pan: (0.0, 0.0),
            size: (900.0, 520.0),
            origin: (0.0, 0.0),
            cursor: None,
            drag: None,
            drag_moves: false,
            last_now: None,
            last_emit: None,
            last_bytes: 0,
            dirty: true,
        })
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
    pub fn comms(&self) -> u64 {
        self.comms
    }
    /// Why no further frame will come, once that is known.
    pub fn finished(&self) -> Option<&str> {
        self.done.as_deref()
    }

    fn plural(n: u64) -> &'static str {
        if n == 1 { "" } else { "s" }
    }

    /// Compute one more frame, or learn that there is none.
    fn step_frame(&mut self) {
        if let Steps::Exactly(n) = self.steps {
            if self.comms >= n {
                let more = if self.machine.is_quiescent() { "" } else { ", work still queued" };
                self.done = Some(format!("stopped after {n} comm{}{more}", Self::plural(n)));
                return;
            }
        }
        if self.frames.len() >= MAX_FRAMES {
            self.done = Some(format!("stopped at {MAX_FRAMES} frames"));
            return;
        }
        for _ in 0..MAX_TICKS_PER_FRAME {
            if self.machine.is_quiescent() {
                self.done = Some(format!("at rest after {} comm{}", self.comms, Self::plural(self.comms)));
                return;
            }
            let steps = match self.machine.run(1) {
                Ok(s) => s,
                Err(e) => {
                    self.done = Some(format!("the store refused a step: {e:?}"));
                    return;
                }
            };
            let fired = steps.iter().filter(|s| matches!(s, Step::Comm { .. })).count() as u64;
            let minted: u32 = steps.iter().map(|s| if let Step::Minted { count, .. } = s { *count } else { 0 }).sum();
            self.comms += fired;
            let now = identify(world(&self.machine));
            let last = self.frames.last().expect("a session has a first frame");
            let (was, is) = (id_set(&last.comps), id_set(&now));
            if was == is {
                if let (Steps::Exactly(n), true) = (self.steps, fired > 0) {
                    if self.comms >= n {
                        break;
                    }
                }
                continue;
            }
            let note = if fired > 0 {
                format!("comm {}: a send met a receipt", self.comms)
            } else if minted > 0 {
                format!("new: {minted} fresh name{} minted", Self::plural(minted as u64))
            } else {
                "structural step".to_string()
            };
            let comps = reorder(&last.comps, now);
            let k = self.frames.len() - 1;
            self.frames[k].lay_out(None, Some(&is));
            self.frames.push(Frame::new(comps, note, Some(&was)));
            return;
        }
        if self.done.is_none() && !matches!(self.steps, Steps::Exactly(n) if self.comms >= n) {
            self.done = Some(format!("no visible change in {MAX_TICKS_PER_FRAME} ticks; stopped"));
        }
    }

    /// Make frame `k` exist if the reduction reaches it.
    pub fn ensure(&mut self, k: usize) {
        while self.frames.len() <= k && self.done.is_none() {
            self.step_frame();
        }
    }

    // ---- controls ----

    pub fn play(&mut self, on: bool) {
        if on && self.at_end() {
            self.t = 0.0;
        }
        self.playing = on;
        self.dirty = true;
    }
    pub fn toggle(&mut self) {
        self.play(!self.playing);
    }
    pub fn next(&mut self) {
        self.playing = false;
        let k = self.t.floor() as usize + 1;
        self.ensure(k);
        self.t = k.min(self.frames.len() - 1) as f64;
        self.dirty = true;
    }
    pub fn prev(&mut self) {
        self.playing = false;
        self.t = (self.t.ceil() - 1.0).max(0.0);
        self.dirty = true;
    }
    pub fn restart(&mut self) {
        self.t = 0.0;
        self.playing = true;
        self.dirty = true;
    }
    pub fn toggle_spin(&mut self) {
        self.spin = !self.spin;
        self.dirty = true;
    }
    /// Zoom about the middle of the picture.
    pub fn zoom_by(&mut self, factor: f64) {
        self.zoom_about(factor, None);
    }

    /// Zoom, keeping the point `at` (in the picture's pixels) where it is.
    fn zoom_about(&mut self, factor: f64, at: Option<(f64, f64)>) {
        let zoom = (self.zoom * factor).clamp(0.2, 20_000.0);
        let f = zoom / self.zoom;
        let (ux, uy) = match at {
            Some((x, y)) => (x - self.size.0 / 2.0, y - self.size.1 / 2.0),
            None => (0.0, 0.0),
        };
        self.pan = (ux - (ux - self.pan.0) * f, uy - (uy - self.pan.1) * f);
        self.zoom = zoom;
        self.dirty = true;
    }

    /// The zoom gesture: a wheel or two-finger scroll of `dy` pixels zooms
    /// about the pointer, so scrolling over a detail brings it closer.
    pub fn wheel(&mut self, dy: i64) {
        let factor = ((dy as f64) * 0.004).clamp(-0.6, 0.6).exp();
        let inside = self.cursor.filter(|(x, y)| *x >= 0.0 && *y >= 0.0 && *x <= self.size.0 && *y <= self.size.1);
        self.zoom_about(factor, inside);
    }

    /// Back to the whole figure, seen from where it started.
    pub fn home(&mut self) {
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        self.yaw = -0.7;
        self.pitch = 0.5;
        self.dirty = true;
    }

    /// Switch what a drag does: turn the figure, or move it.
    pub fn toggle_drag(&mut self) {
        self.drag_moves = !self.drag_moves;
        self.dirty = true;
    }

    /// Where the picture sits in the page and how wide it is drawn.
    pub fn place(&mut self, x: i64, y: i64, w: i64) {
        self.origin = (x as f64, y as f64);
        self.fit(w);
    }
    /// Draw at this width, keeping the default proportions.
    pub fn fit(&mut self, w: i64) {
        self.resize(w, (w as f64 * 0.58) as i64);
    }
    pub fn resize(&mut self, w: i64, h: i64) {
        self.size = ((w.clamp(120, 4096)) as f64, (h.clamp(120, 4096)) as f64);
        self.dirty = true;
    }
    /// A pointer event over the picture: `down`, `move`, `up`. Dragging turns
    /// the figure. Coordinates are the event's integers, so replay is exact.
    pub fn pointer(&mut self, phase: &str, x: i64, y: i64) {
        self.cursor = Some((x as f64 - self.origin.0, y as f64 - self.origin.1));
        match phase {
            "down" => self.drag = Some((x, y)),
            "move" => {
                if let Some((px, py)) = self.drag {
                    let (dx, dy) = ((x - px) as f64, (y - py) as f64);
                    if self.drag_moves {
                        self.pan = (self.pan.0 + dx, self.pan.1 + dy);
                    } else {
                        self.yaw += dx * 0.008;
                        self.pitch = (self.pitch + dy * 0.008).clamp(-1.5, 1.5);
                    }
                    self.drag = Some((x, y));
                    self.dirty = true;
                }
            }
            _ => self.drag = None,
        }
    }

    fn at_end(&self) -> bool {
        self.done.is_some() && self.t >= (self.frames.len() - 1) as f64
    }

    /// Whether the picture will change without further input.
    pub fn is_active(&self) -> bool {
        self.dirty || self.playing || self.spin
    }

    /// Advance playback to `now_ms` and, if the picture changed, draw it.
    pub fn advance(&mut self, now_ms: u64) -> Option<Update> {
        let dt = match self.last_now {
            Some(l) => now_ms.saturating_sub(l).min(100) as f64,
            None => 0.0,
        };
        self.last_now = Some(now_ms);
        if self.playing {
            self.t += dt / STEP_MS;
            self.ensure(self.t.floor() as usize + 1);
            let last = (self.frames.len() - 1) as f64;
            if self.t >= last && self.done.is_some() {
                self.t = last;
                self.playing = false;
            } else if self.t > last {
                self.t = last;
            }
            self.dirty = true;
        }
        if self.spin && self.drag.is_none() && dt > 0.0 {
            self.yaw += dt * 0.00018;
            self.dirty = true;
        }
        if !self.dirty {
            return None;
        }
        // A picture is parsed, laid out and painted by the browser on the
        // thread that also takes its input, so pictures are rationed: about
        // thirty a second while they are small, fewer as they grow. Whatever
        // changed stays pending and is drawn when its turn comes.
        let interval = (self.last_bytes as u64 / 1500).clamp(33, 120);
        if self.last_emit.is_some_and(|l| now_ms < l + interval) {
            return None;
        }
        self.dirty = false;
        self.last_emit = Some(now_ms);
        let u = self.update();
        self.last_bytes = u.svg.len();
        Some(u)
    }

    /// The picture as it stands.
    pub fn update(&self) -> Update {
        let (k, k1, u) = self.where_();
        let shown = if k1 != k && u >= 0.5 + HOLD / 2.0 { k1 } else { k };
        let last = self.frames.len() - 1;
        let total = if self.done.is_some() { format!("{last}") } else { format!("{last}+") };
        let mut note = format!("{shown} / {total} — {}", self.frames[shown].note);
        if let (Some(d), true) = (&self.done, shown == last) {
            let _ = write!(note, " — {d}");
        }
        if !self.playing && !self.at_end() {
            note.push_str(" (paused)");
        }
        if self.drag_moves {
            note.push_str(" · drag moves");
        }
        Update { svg: self.svg(), note, term: self.frames[shown].text.clone(), playing: self.playing }
    }

    fn where_(&self) -> (usize, usize, f64) {
        let last = self.frames.len() - 1;
        let k = (self.t.floor().max(0.0) as usize).min(last);
        (k, (k + 1).min(last), (self.t - k as f64).max(0.0))
    }

    fn svg(&self) -> String {
        let (k, k1, u) = self.where_();
        let s = if k1 == k { 0.0 } else { smooth((u - HOLD) / (1.0 - HOLD)) };
        let (a, b) = (&self.frames[k], &self.frames[k1]);
        let (w, h) = self.size;

        // Where each hub is now, how visible, and which frame's hub describes it.
        struct Item<'a> {
            p: [f64; 3],
            q: [f64; 3],
            alpha: f64,
            hub: &'a Hub,
            in_a: bool,
        }
        let lerp = |x: f64, y: f64| x + (y - x) * s;
        let mut items: Vec<Item> = Vec::with_capacity(a.hubs.len() + 8);
        let mut at: HashMap<&str, usize> = HashMap::new();
        for ha in &a.hubs {
            let item = match b.index.get(&ha.id).map(|i| &b.hubs[*i]) {
                Some(hb) => Item {
                    p: [lerp(ha.pos[0], hb.pos[0]), lerp(ha.pos[1], hb.pos[1]), lerp(ha.pos[2], hb.pos[2])],
                    q: [0.0; 3],
                    alpha: 1.0,
                    hub: if s < 0.5 { ha } else { hb },
                    in_a: true,
                },
                None => Item { p: ha.pos, q: [0.0; 3], alpha: 1.0 - s, hub: ha, in_a: true },
            };
            at.insert(&ha.id, items.len());
            items.push(item);
        }
        if k1 != k {
            for hb in &b.hubs {
                if !a.index.contains_key(&hb.id) {
                    at.insert(&hb.id, items.len());
                    items.push(Item { p: hb.pos, q: [0.0; 3], alpha: s, hub: hb, in_a: false });
                }
            }
        }

        // Camera: fit the interpolated bounding ball; orthographic; z is up.
        let c = [lerp(a.centre[0], b.centre[0]), lerp(a.centre[1], b.centre[1]), lerp(a.centre[2], b.centre[2])];
        let r = lerp(a.extent.ln(), b.extent.ln()).exp();
        let sc = self.zoom * 0.44 * w.min(h) / r;
        let (cyw, syw, cp, sp) = (self.yaw.cos(), self.yaw.sin(), self.pitch.cos(), self.pitch.sin());
        for it in &mut items {
            let (x, y, z) = (it.p[0] - c[0], it.p[1] - c[1], it.p[2] - c[2]);
            let (xx, d) = (x * cyw - y * syw, x * syw + y * cyw);
            it.q = [w / 2.0 + self.pan.0 + sc * xx, h / 2.0 + self.pan.1 - sc * (z * cp - d * sp), d * cp + z * sp];
        }

        // Spokes. A spoke is drawn in the colour of the hub it leaves.
        struct Spoke {
            a: [f64; 3],
            b: [f64; 3],
            col: &'static str,
            alpha: f64,
        }
        let mut spokes: Vec<Spoke> = Vec::new();
        let mut edge = |f: &Frame, hub: &Hub, alpha: f64| {
            if hub.parent < 0 || alpha <= 0.004 {
                return;
            }
            let parent = &f.hubs[hub.parent as usize];
            let (Some(ci), Some(pi)) = (at.get(hub.id.as_str()), at.get(parent.id.as_str())) else { return };
            let (p, q) = (items[*pi].q, items[*ci].q);
            // Leave out what cannot be seen: a spoke wholly to one side of
            // the picture, or shorter than a third of a pixel.
            let off = (p[0] < 0.0 && q[0] < 0.0) || (p[0] > w && q[0] > w) || (p[1] < 0.0 && q[1] < 0.0) || (p[1] > h && q[1] > h);
            if off || (p[0] - q[0]).hypot(p[1] - q[1]) < 0.33 {
                return;
            }
            spokes.push(Spoke { a: p, b: q, col: colour(parent.kind), alpha });
        };
        for ha in &a.hubs {
            let hb = b.index.get(&ha.id).map(|i| &b.hubs[*i]);
            let same = match hb {
                Some(hb) => ha.parent >= 0 && hb.parent >= 0 && a.hubs[ha.parent as usize].id == b.hubs[hb.parent as usize].id,
                None => false,
            };
            if same || k == k1 {
                edge(a, ha, 1.0);
            } else {
                edge(a, ha, 1.0 - s);
                if let Some(hb) = hb {
                    edge(b, hb, s);
                }
            }
        }
        if k1 != k {
            for hb in &b.hubs {
                if !a.index.contains_key(&hb.id) {
                    edge(b, hb, s);
                }
            }
        }
        spokes.sort_by(|x, y| ((x.a[2] + x.b[2]) / 2.0).total_cmp(&((y.a[2] + y.b[2]) / 2.0)));

        // No `viewBox`: the picture is drawn at the size it is shown, and every
        // attribute name is lower-case, so an HTML parser and the reference
        // DOM serialise it identically.
        let mut out = String::with_capacity(96 * (spokes.len() + items.len()) + 256);
        let _ = write!(
            out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}">"#
        );
        let lw = (sc * 0.11).clamp(1.2, 3.2);
        for e in &spokes {
            let _ = write!(
                out,
                r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="{lw:.1}" stroke-linecap="round" stroke-opacity="{:.2}"/>"#,
                e.a[0], e.a[1], e.b[0], e.b[1], e.col, e.alpha
            );
        }
        let dot = (sc * 0.2).clamp(2.4, 6.5);
        let resting = k == k1 || u < HOLD;
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by(|x, y| items[*x].q[2].total_cmp(&items[*y].q[2]));
        for i in order {
            let it = &items[i];
            if it.alpha <= 0.004 || it.q[0] < -12.0 || it.q[0] > w + 12.0 || it.q[1] < -12.0 || it.q[1] > h + 12.0 {
                continue;
            }
            let _ = write!(
                out,
                r##"<circle cx="{:.1}" cy="{:.1}" r="{dot:.1}" fill="{}" fill-opacity="{:.2}" stroke="#f7f7f5" stroke-width="1"/>"##,
                it.q[0], it.q[1], colour(it.hub.kind), it.alpha
            );
            if resting && it.in_a && it.hub.flag != 0 && !it.hub.id.contains('.') {
                let col = if it.hub.flag & 1 != 0 { LEAVING } else { ARRIVED };
                let _ = write!(
                    out,
                    r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="none" stroke="{col}" stroke-width="2"/>"#,
                    it.q[0], it.q[1], dot + 4.5
                );
            }
            if sc > 9.0 && matches!(it.hub.kind, "var" | "unf" | "lit") {
                let _ = write!(
                    out,
                    r##"<text x="{:.1}" y="{:.1}" font-family="sans-serif" font-size="11" fill="#1c1c1c" fill-opacity="{:.2}">{}</text>"##,
                    it.q[0] + dot + 3.0,
                    it.q[1] - dot - 1.0,
                    it.alpha,
                    escape(&it.hub.label)
                );
            }
        }
        out.push_str("</svg>");
        out
    }
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The legend, as HTML, for a page to show beside the picture.
pub fn legend_html() -> String {
    let mut out = String::new();
    for (_, col, text) in KINDS {
        let _ = write!(out, r#"<span class="k"><i style="background:{col}"></i>{}</span>"#, escape(text));
    }
    let _ = write!(
        out,
        r#"<span class="k"><i class="ring" style="border-color:{LEAVING}"></i>component about to react</span><span class="k"><i class="ring" style="border-color:{ARRIVED}"></i>just produced</span><span class="k">axes: 1st, 2nd, 3rd argument</span>"#
    );
    out
}

pub mod page;
pub use page::{page_html, DEFAULT_STEPS, DEFAULT_TERM, SCRIPT};

#[cfg(test)]
mod tests;
