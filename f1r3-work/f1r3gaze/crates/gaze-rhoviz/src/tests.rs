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

/// The contract form of a recursive definition: deep, and it grows.
const DEEP: &str = "new P, a, b in { for(s1, s2 <= P){ for(y <- s1){ P!(@Nil!(*s1), @Nil!(*s2)) | *y } | s1!( P!(@Nil!(*s1), @Nil!(*s2)) ) | for(y <- s2){ P!(@Nil!(*s1), @Nil!(*s2)) | *y } | s2!( P!(@Nil!(*s1), @Nil!(*s2)) ) } | P!(*a, *b) }";

/// No two spokes cross or overlap: with axis-parallel spokes, two of them
/// can only meet at a common endpoint.
fn assert_spokes_disjoint(f: &Frame) {
    let segs: Vec<([f64; 3], [f64; 3])> =
        f.hubs.iter().filter(|h| h.parent >= 0).map(|h| (f.hubs[h.parent as usize].pos, h.pos)).collect();
    let within = |x: f64, a: f64, b: f64| x > a.min(b) - 1e-9 && x < a.max(b) + 1e-9;
    for (i, (a0, a1)) in segs.iter().enumerate() {
        let ka = (0..3).find(|&k| (a0[k] - a1[k]).abs() > 1e-9).unwrap();
        for (b0, b1) in &segs[i + 1..] {
            let kb = (0..3).find(|&k| (b0[k] - b1[k]).abs() > 1e-9).unwrap();
            // A common point p: on a, so p = a0 off axis ka; on b likewise.
            let mut p = *a0;
            let meet = if ka == kb {
                (0..3).all(|k| k == ka || (a0[k] - b0[k]).abs() < 1e-9) && {
                    let (lo, hi) = (a0[ka].min(a1[ka]).max(b0[ka].min(b1[ka])), a0[ka].max(a1[ka]).min(b0[ka].max(b1[ka])));
                    p[ka] = lo;
                    lo <= hi + 1e-9
                }
            } else {
                p[ka] = b0[ka];
                (0..3).all(|k| k == ka || k == kb || (a0[k] - b0[k]).abs() < 1e-9)
                    && within(b0[ka], a0[ka], a1[ka])
                    && within(a0[kb], b0[kb], b1[kb])
            };
            if meet {
                let end = |q: &[f64; 3]| (0..3).all(|k| (q[k] - p[k]).abs() < 1e-9);
                let shared = (end(a0) || end(a1)) && (end(b0) || end(b1));
                let overlap = ka == kb && (a0[ka].max(a1[ka]).min(b0[ka].max(b1[ka])) - a0[ka].min(a1[ka]).max(b0[ka].min(b1[ka]))) > 1e-9;
                assert!(shared && !overlap, "two spokes meet away from a shared hub");
            }
        }
    }
}

#[test]
fn a_deep_term_stays_a_reasonable_size_and_an_embedding() {
    let mut s = Session::open(DEEP, Steps::Exactly(12)).unwrap();
    run_out(&mut s);
    for f in s.frames() {
        assert_spokes_disjoint(f);
        let n = f.hubs.len() as f64;
        assert!(f.extent < 1.5 * n, "extent {} for {} hubs: size is a sum over parts, not a power of depth", f.extent, n);
        for (i, a) in f.hubs.iter().enumerate() {
            for b in &f.hubs[i + 1..] {
                assert!((0..3).any(|k| (a.pos[k] - b.pos[k]).abs() > 1e-9), "two hubs coincide");
            }
        }
    }
    assert!(s.frames().last().unwrap().hubs.len() > 300);
}

#[test]
fn pictures_are_rationed_and_the_wheel_zooms_about_the_pointer() {
    let mut s = Session::open(DEEP, Steps::All).unwrap();
    let mut sent = 0;
    for i in 0..=125u64 {
        sent += s.advance(i * 16).is_some() as u32;
    }
    assert!((15..=62).contains(&sent), "{sent} pictures in two seconds of sixty frames a second");

    // Zooming about a point leaves that point of the figure where it was.
    let mut z = Session::open(RELAY, Steps::Exactly(0)).unwrap();
    z.toggle_spin();
    z.place(100, 200, 900);
    z.pointer("move", 100 + 700, 200 + 150);
    let before = (z.zoom, z.pan);
    z.wheel(120);
    assert!(z.zoom > before.0, "scrolling one way zooms in");
    let (ux, uy) = (700.0 - 450.0, 150.0 - z.size.1 / 2.0);
    let f = z.zoom / before.0;
    assert!((z.pan.0 - (ux - (ux - before.1 .0) * f)).abs() < 1e-9 && (z.pan.1 - (uy - (uy - before.1 .1) * f)).abs() < 1e-9);
    z.wheel(-120);
    assert!((z.zoom - before.0).abs() < 1e-9 && z.pan.0.abs() < 1e-9 && z.pan.1.abs() < 1e-9, "and back again");
    z.zoom_by(40.0);
    assert!(z.update().svg.len() < 4_000, "zoomed far in, what is off the picture is not drawn");
    z.home();
    assert_eq!((z.zoom, z.pan), (1.0, (0.0, 0.0)));
}
