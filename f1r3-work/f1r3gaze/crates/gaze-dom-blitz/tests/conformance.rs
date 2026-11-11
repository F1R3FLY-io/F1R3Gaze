//! Protocol conformance across backends (spec §13): the same pages, the same
//! scripted input, over the reference `MemDom` and over Blitz. Both must
//! commit identical documents, frame by frame.

use gaze_dom_blitz::{BNode, BlitzDom};
use gaze_dom_core::mem::MemDom;
use gaze_dom_core::DomBackend;
use gaze_exec::{default_policy, Record, TabExec, TabLog};
use gaze_knf::Knf;
use k1ndl1ng_norm::Norm;
use k1ndl1ng_parse::Level;

const LAMP: &str = r##"new found, sub, clicks, on, off in {
  doc!("query1", "#lamp", *found) |
  for (@("ok", *lamp) <- found) {
    lamp!("listen", "click", *clicks, {}, *sub) |
    off!(Nil) |
    for (_ <= clicks & _ <- off) { lamp!("classAdd", "lit")    | on!(Nil)  } |
    for (_ <= clicks & _ <- on)  { lamp!("classRemove", "lit") | off!(Nil) }
  }
}"##;

/// A list the page builds with structured fragments and `ref` markers; each
/// click on the button appends an item whose own button removes it.
const TODO: &str = r##"new b, l, clicks, made, sub, n in {
  doc!("query1", "#add", *b) | doc!("query1", "#list", *l) |
  for (@("ok", *btn) <- b & @("ok", *list) <- l) {
    btn!("listen", "click", *clicks, {}, *sub) |
    for (_ <= clicks) {
      new r, xs, sub2 in {
        list!("append", ("el", "li", {"class": "item"}, [("text", "task"), ("el", "button", {"class": "x"}, [("ref", "x"), ("text", "x")])]), *r) |
        for (@("ok", {"x": *x}) <- r) {
          x!("listen", "click", *xs, {"once": true}, *sub2) |
          for (@(_, {"target": *t, "type": _, "x": _}) <- xs) {
            t!("closest", "li", *made) |
            for (@("ok", *li) <- made) { li!("remove") }
          }
        }
      }
    }
  }
}"##;

const TODO_HTML: &str = r#"<html><head></head><body><button id="add">add</button><ul id="list"></ul></body></html>"#;
const LAMP_HTML: &str = r#"<html><head></head><body><main><button id="lamp">lamp</button></main></body></html>"#;

fn knf(src: &str) -> Knf {
    Knf::from_source(src, Level::K1G, &[]).expect("compiles")
}

fn frames<B: DomBackend>(tab: &mut TabExec<B>, t: &mut u64, n: usize) {
    for _ in 0..n {
        *t += 16;
        tab.frame(*t);
    }
}

/// Drive a page with a script of clicks given as selectors.
fn drive<B: DomBackend>(
    mut tab: TabExec<B>,
    clicks: &[&str],
    find: &dyn Fn(&TabExec<B>, &str) -> Option<B::Node>,
) -> (Vec<[u8; 32]>, String, TabExec<B>) {
    let mut t = 0;
    frames(&mut tab, &mut t, 6);
    for sel in clicks {
        let target = find(&tab, sel).unwrap_or_else(|| panic!("no {sel}"));
        tab.dispatch(target, "click", vec![("x".into(), Norm::int(1))]);
        frames(&mut tab, &mut t, 8);
    }
    let s = tab.dom.backend.serialize();
    (tab.commit_hashes(), s, tab)
}

fn mem_find(tab: &TabExec<MemDom>, sel: &str) -> Option<usize> {
    let r = tab.dom.backend.root();
    tab.dom.backend.query(r, sel, false).ok()?.first().copied()
}
fn blitz_find(tab: &TabExec<BlitzDom>, sel: &str) -> Option<BNode> {
    let r = tab.dom.backend.root();
    tab.dom.backend.query(r, sel, false).ok()?.first().copied()
}

fn both(src: &str, html: &str, clicks: &[&str]) -> (String, TabExec<BlitzDom>) {
    let k = knf(src);
    let mem = TabExec::load(&k, MemDom::from_html(html), [3; 32], &default_policy).unwrap();
    let blz = TabExec::load(&k, BlitzDom::from_html(html), [3; 32], &default_policy).unwrap();
    let (hm, sm, _) = drive(mem, clicks, &mem_find);
    let (hb, sb, tb) = drive(blz, clicks, &blitz_find);
    assert_eq!(sm, sb, "committed documents differ");
    assert_eq!(hm, hb, "per-frame commit hashes differ");
    (sb, tb)
}

#[test]
fn blitz_parses_like_the_reference() {
    let b = BlitzDom::from_html(LAMP_HTML);
    assert_eq!(b.serialize(), MemDom::from_html(LAMP_HTML).serialize());
}

#[test]
fn lamp_agrees_across_backends() {
    let (s, _) = both(LAMP, LAMP_HTML, &["#lamp", "#lamp", "#lamp"]);
    assert!(s.contains(r#"<button id="lamp" class="lit">"#), "{s}");
}

#[test]
fn todo_agrees_across_backends_and_replays() {
    let (s, tab) = both(TODO, TODO_HTML, &["#add", "#add", "#add", "li .x"]);
    assert_eq!(s.matches(r#"<li class="item">"#).count(), 2, "{s}");

    // Replay the Blitz run from its log bytes, on Blitz. The click on the
    // item's own button targets a node the page never named before the
    // click: replay must mint the same names, which the dispatch record does.
    let log = TabLog::from_bytes(&tab.log.to_bytes()).unwrap();
    assert!(log.records.iter().any(|r| matches!(r, Record::Dispatch { .. })));
    let frames_n = log.records.iter().filter(|r| matches!(r, Record::Frame { .. })).count();
    let mut again = TabExec::replay(&knf(TODO), BlitzDom::from_html(TODO_HTML), &log).unwrap();
    let mut t = 0;
    frames(&mut again, &mut t, frames_n);
    assert_eq!(again.commit_hashes(), tab.commit_hashes());
    // ...and on the reference backend: the log is backend-independent.
    let mut on_mem = TabExec::replay(&knf(TODO), MemDom::from_html(TODO_HTML), &log).unwrap();
    let mut t = 0;
    frames(&mut on_mem, &mut t, frames_n);
    assert_eq!(on_mem.commit_hashes(), tab.commit_hashes());
}

#[test]
fn attenuated_queries_cannot_see_above_the_subtree() {
    let html = r#"<html><head></head><body><div class="outer"><section id="s"><span>t</span></section></div></body></html>"#;
    let b = BlitzDom::from_html(html);
    let s = b.by_id("s").unwrap();
    let root = b.root();
    assert_eq!(b.query(root, ".outer span", true).unwrap().len(), 1);
    // Blitz's own scoped query would answer 1 here; the backend must not.
    assert_eq!(b.query(s, ".outer span", true).unwrap().len(), 0);
    assert_eq!(b.query(s, "span", true).unwrap().len(), 1);
    assert_eq!(b.query(s, "section span", true).unwrap().len(), 0);
    let span = b.query(s, "span", false).unwrap()[0];
    assert_eq!(b.closest(span, "div", s).unwrap(), None);
    assert_eq!(b.closest(span, "section", s).unwrap(), Some(s));
    assert_eq!(b.closest(span, "div section", s).unwrap(), None);
    assert!(b.query(s, "span[", true).is_err());
}

#[test]
fn a_decide_listener_prevents_synchronously() {
    let src = r##"new f, ev, sub in {
      doc!("query1", "#go", *f) |
      for (@("ok", *go) <- f) {
        go!("listen", "click", *ev, {"decide": true}, *sub) |
        for (@(_, {"reply": *r, "target": _, "type": _}) <= ev) { r!("prevent") }
      }
    }"##;
    let html = r#"<html><head></head><body><a id="go" href="https://example.org/">go</a></body></html>"#;
    let mut tab = TabExec::load(&knf(src), BlitzDom::from_html(html), [9; 32], &default_policy).unwrap();
    let mut t = 0;
    frames(&mut tab, &mut t, 6);
    let go = tab.dom.backend.by_id("go").unwrap();
    let d = tab.dispatch_ext(go, "click", vec![], true);
    assert!(d.drained && d.prevent, "{d:?}");
}

#[test]
fn set_html_and_fragments_render() {
    let src = r##"new f, r in {
      doc!("query1", "#box", *f) |
      for (@("ok", *b) <- f) {
        b!("setHTML", "<p>one</p><script>alert(1)</script>") |
        b!("append", ("el", "p", {"id": "two"}, [("text", "two & more")]), *r)
      }
    }"##;
    let html = r#"<html><head></head><body><div id="box">old</div></body></html>"#;
    let (s, _) = both(src, html, &[]);
    assert!(s.contains("<p>one</p>"), "{s}");
    assert!(s.contains(r#"<p id="two">two &amp; more</p>"#), "{s}");
}

// ---------------------------------------------------------------------------
// rhoviz: the picture is an inline SVG that Blitz itself parses and sizes.

const RELAY: &str = "new a, b, c in { a!(Nil) | for(_ <- a){ b!(Nil) } | for(_ <- b){ c!(Nil) } | for(_ <- c){ stdout!(Nil) } }";

#[test]
fn rhoviz_agrees_across_backends_and_blitz_can_draw_the_picture() {
    // The page without its <script> element: the program is loaded from
    // its own text below, and the reference DOM does not treat a script's
    // content as raw text, so `<-` inside one would not round-trip there.
    let page = gaze_rhoviz::page_html(RELAY, "2");
    let html = format!("{}</body></html>", page.split("<script").next().unwrap());
    let k = Knf::from_source(gaze_rhoviz::SCRIPT, Level::K1G, &[]).unwrap();
    let mut mem = TabExec::load(&k, MemDom::from_html(&html), [3; 32], &default_policy).unwrap();
    let mut blz = TabExec::load(&k, BlitzDom::from_html(&html), [3; 32], &default_policy).unwrap();
    let (mut t, mut u) = (0, 0);
    frames(&mut mem, &mut t, 240);
    frames(&mut blz, &mut u, 240);
    let (m, b) = (mem.dom.backend.serialize(), blz.dom.backend.serialize());
    if m != b {
        let at = m.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(0);
        let cut = |s: &str| s.chars().skip(at.saturating_sub(20)).take(260).collect::<String>();
        panic!("documents differ at byte {at}:\n  mem:   {}\n  blitz: {}", cut(&m), cut(&b));
    }
    assert!(mem.commit_hashes() == blz.commit_hashes(), "the same pictures, frame by frame");

    // Lay the document out as the window would. Blitz builds an inline
    // <svg> by parsing its markup; a picture it could not parse would be
    // left without image data and would paint nothing.
    let doc = blz.dom.backend.doc.clone();
    let mut doc = doc.borrow_mut();
    doc.set_viewport(blitz_traits::shell::Viewport::new(1000, 900, 1.0, blitz_traits::shell::ColorScheme::Light));
    doc.resolve(0.0);
    let svg = doc.query_selector("#stage svg").unwrap().expect("a picture is on the stage");
    let node = doc.get_node(svg).unwrap();
    let tree = node.element_data().unwrap().svg_data().expect("Blitz parsed the picture as SVG");
    assert!(tree.root().has_children(), "and it has something in it");
    let size = node.final_layout().size;
    assert!(size.width > 300.0 && size.height > 150.0, "laid out at {size:?}");
}

/// A page that replaces part of its document every frame must not leave the
/// window hit-testing nodes that no longer exist. The window delivers
/// pointer events between a frame's commit and the next layout, and Blitz's
/// hit test walks the paint tree of the *last* layout; a dropped node still
/// listed there used to panic (`Option::unwrap()` in `blitz-dom` `node.rs`).
#[test]
fn pointer_events_between_a_commit_and_the_next_layout_are_safe() {
    let html = gaze_rhoviz::page_html(gaze_rhoviz::DEFAULT_TERM, "*");
    let k = Knf::from_source(gaze_rhoviz::SCRIPT, Level::K1G, &[]).unwrap();
    let mut tab = TabExec::load(&k, BlitzDom::from_html(&html), [3; 32], &default_policy).unwrap();
    let doc = tab.dom.backend.doc.clone();
    doc.borrow_mut().set_viewport(blitz_traits::shell::Viewport::new(1000, 900, 1.0, blitz_traits::shell::ColorScheme::Light));
    let mut t = 0;
    let mut hits = 0;
    for frame in 0..900u32 {
        t += 16;
        tab.frame(t);
        // The pointer moves over the picture after the commit, before layout.
        for (x, y) in [(480.0, 600.0), (300.0, 500.0), (700.0, 700.0), (60.0, 40.0)] {
            hits += doc.borrow().hit(x, y).is_some() as u32;
            // What the window does on a pointer move: hit-test, then update hover.
            doc.borrow_mut().set_hover_to(x, y);
        }
        // The window lays out now and then, not after every commit.
        if frame % 3 == 0 {
            doc.borrow_mut().resolve(0.0);
        }
    }
    assert!(hits > 0);
}

// ---------------------------------------------------------------------------
// The window's own path: a RhoDocument polled in real time while real pointer
// events are fed to it, as `blitz-shell` does. This is the nearest thing to
// the window that runs without a GPU.

struct NoServices;
impl gaze_dom_blitz::Services for NoServices {
    fn request(&mut self, _req: gaze_exec::CapRequest, _out: &mut Vec<gaze_dom_blitz::Delivery>) {}
    fn poll(&mut self, _out: &mut Vec<gaze_dom_blitz::Delivery>) {}
}

fn pointer(x: f32, y: f32, down: bool) -> blitz_traits::events::BlitzPointerEvent {
    use blitz_traits::events::*;
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords { page_x: x, page_y: y, screen_x: x, screen_y: y, client_x: x, client_y: y },
        button: MouseEventButton::Main,
        buttons: if down { MouseEventButtons::Primary } else { MouseEventButtons::None },
        mods: Default::default(),
        details: Default::default(),
        element: Default::default(),
        active_pointers: Default::default(),
    }
}

#[test]
fn rhoviz_survives_the_window_pointer_moves_drags_and_clicks() {
    use blitz_dom::Document;
    use blitz_traits::events::UiEvent;
    let html = gaze_rhoviz::page_html(gaze_rhoviz::DEFAULT_TERM, "*");
    let mut d = gaze_dom_blitz::RhoDocument::from_html(&html, blitz_dom::DocumentConfig::default());
    let k = Knf::from_source(gaze_rhoviz::SCRIPT, Level::K1G, &[]).unwrap();
    d.start(&[k], [5; 32], &default_policy, Box::new(NoServices)).unwrap();
    let base = d.base();
    base.borrow_mut().set_viewport(blitz_traits::shell::Viewport::new(1000, 900, 1.0, blitz_traits::shell::ColorScheme::Light));
    let rect = |id: &str| {
        let doc = base.borrow();
        let n = doc.get_element_by_id(id).unwrap();
        doc.get_client_bounding_rect(n).unwrap()
    };
    let centre = |id: &str| {
        let r = rect(id);
        ((r.x + r.width / 2.0) as f32, (r.y + r.height / 2.0) as f32)
    };
    let note = |d: &gaze_dom_blitz::RhoDocument| {
        let doc = d.base();
        let doc = doc.borrow();
        doc.get_node(doc.get_element_by_id("note").unwrap()).unwrap().text_content()
    };

    let turn = |d: &mut gaze_dom_blitz::RhoDocument, i: u32, events: &[UiEvent]| {
        std::thread::sleep(std::time::Duration::from_millis(17));
        d.poll(None);
        for e in events {
            d.handle_ui_event(e.clone());
        }
        // The window lays out when it paints, which is not after every commit.
        if i % 2 == 0 {
            base.borrow_mut().resolve(0.0);
        }
    };

    // Let it start, then sweep the pointer across the whole page while the
    // picture is being replaced underneath it.
    for i in 0..30 {
        turn(&mut d, i, &[]);
    }
    base.borrow_mut().resolve(0.0);
    assert!(base.borrow().query_selector("#stage svg").unwrap().is_some(), "a picture is up");
    for i in 0..60u32 {
        let (x, y) = (40.0 + 15.0 * i as f32, 120.0 + 12.0 * i as f32);
        turn(&mut d, i, &[UiEvent::PointerMove(pointer(x, y, false))]);
    }

    // Drag across the picture.
    base.borrow_mut().resolve(0.0);
    let (sx, sy) = centre("stage");
    turn(&mut d, 0, &[UiEvent::PointerDown(pointer(sx, sy, true))]);
    for i in 0..20u32 {
        turn(&mut d, i, &[UiEvent::PointerMove(pointer(sx + 6.0 * i as f32, sy + 2.0 * i as f32, true))]);
    }
    turn(&mut d, 0, &[UiEvent::PointerUp(pointer(sx + 120.0, sy + 40.0, false))]);

    // Click pause, then step: the caption must say so.
    base.borrow_mut().resolve(0.0);
    for id in ["play", "next", "spin", "zin", "zout", "prev", "again", "go"] {
        let (x, y) = centre(id);
        turn(&mut d, 0, &[UiEvent::PointerMove(pointer(x, y, false))]);
        turn(&mut d, 0, &[UiEvent::PointerDown(pointer(x, y, true))]);
        turn(&mut d, 0, &[UiEvent::PointerUp(pointer(x, y, false))]);
        for i in 0..4 {
            turn(&mut d, i, &[]);
        }
        if id == "play" {
            assert!(note(&d).contains("(paused)"), "clicking play / pause pauses: {}", note(&d));
        }
        base.borrow_mut().resolve(0.0);
    }
    assert!(!note(&d).is_empty());
}
