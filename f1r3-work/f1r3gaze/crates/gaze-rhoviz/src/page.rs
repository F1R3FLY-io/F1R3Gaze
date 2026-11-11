//! The `rhoviz` page: a document and the f1r3lang program that drives it.
//!
//! The program holds two capabilities, the document and `rhoviz`, and does
//! nothing but introduce them to each other: input values and pointer events
//! go to `rhoviz`, and each picture `rhoviz` produces goes to the document.

use crate::{escape, legend_html};

/// The default term: replication from reflection, which never comes to rest.
pub const DEFAULT_TERM: &str =
    "new x in { for(y <- x){ x!(*y) | *y } | x!( for(y <- x){ x!(*y) | *y } | stdout!(Nil) ) }";
pub const DEFAULT_STEPS: &str = "6";

const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:0;padding:16px 20px;color:#1c1c1c;background:#f7f7f5;line-height:1.4}
h1{font-size:20px;font-weight:600;margin:0 0 10px 0}
.row{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin:6px 0}
input{font:13px ui-monospace,Menlo,Consolas,monospace;padding:7px 8px;border:1px solid #b9bcc4;border-radius:6px;background:#fff}
#src{flex:1 1 420px;min-width:240px}#steps{width:64px}
button{font:inherit;padding:7px 12px;border:1px solid #9aa4b5;border-radius:6px;background:#fff}
#go{background:#0072B2;border-color:#005a8c;color:#fff}
#note{font-weight:600;margin-top:10px}
#term{font:12px ui-monospace,Menlo,Consolas,monospace;white-space:pre-wrap;margin:4px 0 8px 0;max-height:120px;overflow:auto}
#stage{background:#fff;border:1px solid #dcdcd6;border-radius:8px;overflow:hidden;max-width:900px}
#stage svg{display:block}
#legend{display:flex;flex-wrap:wrap;gap:4px 14px;margin-top:8px;font-size:12px;color:#444}
#legend i{display:inline-block;width:10px;height:10px;border-radius:5px;margin-right:4px}
#legend i.ring{background:none;box-sizing:border-box;border:2px solid}
.muted{color:#667085;font-size:12px}";

/// The page's behaviour. `doc` and `rhoviz` are its only imports.
pub const SCRIPT: &str = r##"new out, stR, noR, teR, goR, srcR, nR in {
  rhoviz!("watch", *out) |
  doc!("query1", "#stage", *stR) | doc!("query1", "#note", *noR) | doc!("query1", "#term", *teR) |
  doc!("query1", "#go", *goR) | doc!("query1", "#src", *srcR) | doc!("query1", "#steps", *nR) |

  for (@("ok", *stage) <- stR & @("ok", *note) <- noR & @("ok", *term) <- teR) {
    for (@("ok", svg, n, t) <= out) { stage!("setHTML", svg) | note!("setText", n) | term!("setText", t) } |
    for (@("err", m) <= out) { note!("setText", m) | term!("setText", "") | stage!("setHTML", "") } |
    new down, move, up, s1, s2, s3 in {
      stage!("listen", "pointerdown", *down, {}, *s1) |
      stage!("listen", "pointermove", *move, {}, *s2) |
      stage!("listen", "pointerup", *up, {}, *s3) |
      for (@(_, {"x": x, "y": y ..._}) <= down) { rhoviz!("pointer", "down", x, y) } |
      for (@(_, {"x": x, "y": y ..._}) <= move) { rhoviz!("pointer", "move", x, y) } |
      for (@(_, {"x": x, "y": y ..._}) <= up)   { rhoviz!("pointer", "up", x, y) }
    }
  } |

  for (@("ok", *go) <- goR & @("ok", *src) <- srcR & @("ok", *steps) <- nR) {
    new run, clicks, sub in {
      go!("listen", "click", *clicks, {}, *sub) |
      for (_ <= clicks) { run!(Nil) } |
      run!(Nil) |
      for (_ <= run) {
        new a, b in {
          src!("value", *a) | steps!("value", *b) |
          for (@("ok", t) <- a & @("ok", n) <- b) { rhoviz!("open", t, n) }
        } |
        new box, sz in {
          doc!("query1", "#stage", *box) |
          for (@("ok", *st) <- box) { st!("rect", *sz) | for (@("ok", (_, _, w, _)) <- sz) { rhoviz!("fit", w) } }
        }
      }
    }
  } |

  new r, c, s in { doc!("query1", "#play", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("toggle") } } } |
  new r, c, s in { doc!("query1", "#prev", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("prev") } } } |
  new r, c, s in { doc!("query1", "#next", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("next") } } } |
  new r, c, s in { doc!("query1", "#again", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("restart") } } } |
  new r, c, s in { doc!("query1", "#spin", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("spin") } } } |
  new r, c, s in { doc!("query1", "#zin", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("zoom", "in") } } } |
  new r, c, s in { doc!("query1", "#zout", *r) | for (@("ok", *b) <- r) { b!("listen", "click", *c, {}, *s) | for (_ <= c) { rhoviz!("zoom", "out") } } }
}"##;

fn attr(s: &str) -> String {
    escape(s).replace('"', "&quot;")
}

/// The whole page, with the form filled in with `term` and `steps`. It runs
/// what it is given as soon as it loads.
pub fn page_html(term: &str, steps: &str) -> String {
    format!(
        r#"<html><head><title>rhoviz</title><style>{STYLE}</style></head><body>
<h1>rhoviz</h1>
<div class="row">
  <input id="src" type="text" value="{}" spellcheck="false">
  <input id="steps" type="text" value="{}">
  <button id="go">Reduce</button>
</div>
<div class="muted">A rho term, and how many comm events to animate. <b>*</b> reduces to completion: caveat emptor, a term need not have one. Frames are computed as playback reaches them, so a term that never rests plays for as long as you watch.</div>
<div class="row">
  <button id="prev">◀◀</button><button id="play">play / pause</button><button id="next">▶▶</button>
  <button id="again">restart</button><button id="spin">spin</button><button id="zin">+</button><button id="zout">−</button>
  <span class="muted">drag the picture to turn it</span>
</div>
<div id="note"></div>
<div id="term"></div>
<div id="stage"></div>
<div id="legend">{}</div>
<script type="application/f1r3lang" imports="doc rhoviz">
{SCRIPT}
</script></body></html>"#,
        attr(term),
        attr(steps),
        legend_html()
    )
}
