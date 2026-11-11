# rhoviz

Watch a rho term reduce, drawn in three dimensions.

```
f1r3gaze gaze://rhoviz
f1r3gaze 'gaze://rhoviz?term=new%20a%20in%20%7B%20a!(Nil)%20%7C%20for(_%20%3C-%20a)%7B%20Nil%20%7D%20%7D&steps=*'
```

The page has two inputs: a rho term, and the number of comm events to
animate. `*` reduces to completion. Caveat emptor: a term need not have one.
That costs nothing but the frames watched, because the reduction is lazy (see
below). **Reduce** runs what is in the form; the page also runs it once when it
loads. The buttons step, pause, restart, stop the spin and zoom; dragging the
picture turns it.

## The figure

The constructor of a node is its **colour**; the position of an argument is one
of three orthogonal **axes**. A node whose largest child has radius `r` puts its
children at distance `sqrt(2) r + 1` along the axes, so sibling figures lie in
disjoint balls and no two spokes meet except at a shared hub. (The construction
and its proof are in `publications`, "A Geometric Rendering of GSLT Syntax",
Realisation D.)

| node | children along axes 1, 2, 3 |
| --- | --- |
| `P \| Q` | left half, right half (balanced: `\|` is associative) |
| `x!(P)` | channel, payload |
| `for(y <- x){P}` | pattern, channel, body |
| `for(.. & ..){P}` | pattern, channel, then the next bind or the body |
| `@P`, `*x`, `new n in {P}` | the one argument |
| lists, tuples, maps, polyadic payloads | head, rest |

Bound names are de Bruijn indices, shown as labelled leaves. Binding itself is
not drawn.

## The animation

A component is identified by its content hash, so one that takes no part in a
step is the same object in the next frame and glides to its new place. A red
ring marks a component about to react; a green ring marks one just produced.
Minting by `new` is a frame of its own and does not count as a step.

## How it is built

f1r3lang has no arithmetic, so the page's program cannot lay anything out. The
work is split along that line:

- **`rho:gaze:rhoviz`** (crate `gaze-rhoviz`, served inside `gaze-exec` like
  `clock` and `rand`) holds a private CampF1R3 machine for the term, reads the
  world back after every tick, lays each state out, and draws the current
  picture as SVG. It grants no authority: it is a pure function of the term
  and of the events it is sent, so the default policy grants it to every page.
- **The page's program** (`gaze_rhoviz::SCRIPT`, about fifty lines of f1r3lang)
  imports `doc` and `rhoviz` and introduces them: input values and pointer
  events go to `rhoviz`; each picture goes to the document with `setHTML`.

The picture is an inline `<svg>`, which Blitz parses and paints itself.

### The capability

| send | meaning |
| --- | --- |
| `rhoviz!("watch", *out)` | pictures arrive on `out` as `("ok", svg, note, term)`, one per frame in which the picture changed; a term that does not parse arrives as `("err", message)` |
| `rhoviz!("open", term, steps)` | start on `term` (a string); `steps` is a whole number or `"*"` |
| `rhoviz!("play")`, `("pause")`, `("toggle")`, `("next")`, `("prev")`, `("restart")` | playback |
| `rhoviz!("spin")`, `("zoom", "in" \| "out")`, `("pointer", "down" \| "move" \| "up", x, y)` | the camera |
| `rhoviz!("fit", w)`, `("size", w, h)` | the size to draw at, in pixels |
| `rhoviz!("close")` | forget the term |

### Laziness

Frames are computed as playback reaches them: one more state of the world per
step of the animation. A term that never comes to rest therefore plays for as
long as it is watched and never stalls a frame. Three limits apply: 5,000
frames are kept (playback can seek backwards); a single frame may consume at
most 20,000 ticks without the world visibly changing; and the term itself may
grow without bound, which is the emptor's caveat.

### Replay

Pictures are injections like any other, so they are recorded in the `.gzlog`,
and a replay shows the same pictures without reducing anything.

## Limits

- Sizes grow by about 2.4 per level of nesting, so deep subterms are small at
  the default zoom, and there is no pan.
- A component that reacts and is re-formed identically (the listener in the
  default term) keeps its identity and is not ringed.
- The store does not say whether a datum at rest is persistent, so a
  persistent send at rest is drawn as an ordinary send.
- The term is one line of text; the page reads an `<input>`.
