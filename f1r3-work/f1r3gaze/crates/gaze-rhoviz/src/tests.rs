use super::*;

const RELAY: &str = "new a, b, c in { a!(Nil) | for(_ <- a){ b!(Nil) } | for(_ <- b){ c!(Nil) } | for(_ <- c){ stdout!(Nil) } }";
const REPLICATION: &str =
    "new x in { for(y <- x){ x!(*y) | *y } | x!( for(y <- x){ x!(*y) | *y } | stdout!(Nil) ) }";

fn run_out(s: &mut Session) {
    let mut k = 0;
    while s.finished().is_none() {
        k += 1;
        s.ensure(k);
    }
}

#[test]
fn relay_is_three_comms_and_five_frames() {
    let mut s = Session::open(RELAY, Steps::All).unwrap();
    run_out(&mut s);
    assert_eq!(s.comms(), 3);
    assert_eq!(s.frames().len(), 5, "the term, the minting of names, three comms");
    assert_eq!(s.finished(), Some("at rest after 3 comms"));
    assert_eq!(s.frames().last().unwrap().hubs.len(), 3, "one send: hub, channel, payload");
}

#[test]
fn a_step_count_stops_the_run() {
    let mut s = Session::open(RELAY, Steps::Exactly(2)).unwrap();
    run_out(&mut s);
    assert_eq!(s.comms(), 2);
    assert_eq!(s.frames().len(), 4);
    assert!(s.finished().unwrap().starts_with("stopped after 2 comms"));
    let mut z = Session::open(RELAY, Steps::Exactly(0)).unwrap();
    run_out(&mut z);
    assert_eq!(z.frames().len(), 1, "zero steps draws the term only");
}

/// Reduction to completion of a term that has none: frames come only as
/// they are asked for, so asking for a few returns at once.
#[test]
fn a_divergent_term_is_reduced_lazily() {
    let mut s = Session::open(REPLICATION, Steps::All).unwrap();
    s.ensure(6);
    assert_eq!(s.frames().len(), 7);
    assert!(s.finished().is_none(), "there is always more");
    s.ensure(3);
    assert_eq!(s.frames().len(), 7, "nothing is computed twice");
}

/// The figure is an embedding: hubs are pairwise distinct, every spoke is
/// parallel to one axis, and no hub sits in the interior of a spoke.
#[test]
fn every_frame_is_an_embedding() {
    let mut s = Session::open(REPLICATION, Steps::Exactly(5)).unwrap();
    run_out(&mut s);
    for f in s.frames() {
        let hubs = &f.hubs;
        for (i, a) in hubs.iter().enumerate() {
            for b in &hubs[i + 1..] {
                let d: f64 = (0..3).map(|k| (a.pos[k] - b.pos[k]).powi(2)).sum();
                assert!(d > 1e-9, "two hubs coincide");
            }
            if a.parent < 0 {
                continue;
            }
            let p = &hubs[a.parent as usize];
            let moved: Vec<usize> = (0..3).filter(|&k| (a.pos[k] - p.pos[k]).abs() > 1e-9).collect();
            assert_eq!(moved.len(), 1, "a spoke leaves its axis");
            let k = moved[0];
            for c in hubs {
                let on_line = (0..3).all(|j| j == k || (c.pos[j] - p.pos[j]).abs() < 1e-9);
                let inside = c.pos[k] > p.pos[k] + 1e-9 && c.pos[k] < a.pos[k] - 1e-9;
                assert!(!(on_line && inside), "a hub lies inside a spoke");
            }
        }
    }
}

#[test]
fn components_keep_their_identity_and_reactants_are_marked() {
    let mut s = Session::open(RELAY, Steps::All).unwrap();
    run_out(&mut s);
    let f = s.frames();
    // Frame 1 is the four components; the comm to frame 2 consumes two of them.
    let roots = |fr: &Frame, bit: u8| fr.hubs.iter().filter(|h| !h.id.contains('.') && h.flag & bit != 0).count();
    assert_eq!(roots(&f[1], 1), 2, "a send and a receipt are about to react");
    assert_eq!(roots(&f[2], 2), 1, "one send was produced");
    let ids = |fr: &Frame| -> HashSet<String> { fr.hubs.iter().filter(|h| !h.id.contains('.') && h.id != "top").map(|h| h.id.clone()).collect() };
    assert_eq!(ids(&f[1]).intersection(&ids(&f[2])).count(), 2, "two bystanders are the same objects in the next frame");
}

#[test]
fn playback_draws_only_when_something_changes() {
    let mut s = Session::open(RELAY, Steps::Exactly(1)).unwrap();
    s.toggle_spin();
    let first = s.advance(0).expect("the first picture");
    assert!(first.svg.starts_with("<svg") && first.svg.contains("<line") && first.svg.ends_with("</svg>"));
    assert!(first.note.starts_with("0 / "));
    let mut now = 0;
    let mut last = first;
    for _ in 0..600 {
        now += 16;
        if let Some(u) = s.advance(now) {
            last = u;
        }
    }
    assert!(last.note.contains("stopped after 1 comm"), "{}", last.note);
    assert!(!s.is_active());
    assert_eq!(s.advance(now + 16), None, "at rest, nothing is redrawn");
    s.pointer("down", 10, 10);
    s.pointer("move", 40, 10);
    assert!(s.advance(now + 32).is_some(), "a drag turns the figure");
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(Session::open("for(", Steps::All).is_err());
    assert!(Steps::parse("many").is_err());
    assert_eq!(Steps::parse(" * "), Ok(Steps::All));
    assert_eq!(Steps::parse("12"), Ok(Steps::Exactly(12)));
}
