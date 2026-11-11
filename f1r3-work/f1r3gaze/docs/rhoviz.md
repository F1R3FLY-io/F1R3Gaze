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
loads.

| control | does |
| --- | --- |
| scroll over the picture | zooms about the pointer: scroll over a detail to bring it closer |
| drag | turns the figure; **drag: turn / move** switches a drag to moving it |
| **+**, **−** | zoom about the middle |
| **whole figure** | back to the starting view |
| **pause** / **play**, ◀◀, ▶▶, **restart** | playback; the button says what it will do next |
| **spin** | stops and starts the slow rotation |

## The figure

The constructor of a node is its **colour**; the position of an argument is one
of three orthogonal **axes**. Child `i` of a node sits along axis `i`, one unit
past the furthest any of its *siblings* reaches along that axis.

Every figure lies in the positive orthant of its own hub, and that is the whole
argument that nothing overlaps. No sibling has a point as far along axis `i` as
child `i`'s nearest, so sibling figures are disjoint. A spoke lies on an axis
along which every other child is displaced by a positive amount, so it meets
none of them. And it reaches its own child from below, where that child has
nothing.

A figure's size is then a *sum* over its parts. The first version of this page
used the rule of "A Geometric Rendering of GSLT Syntax", Realisation D, which
moves every child past a ball around its largest sibling; that multiplies the
size by about 2.4 at every level of nesting, and a term fourteen levels deep
came out a hundred thousand units across with leaves one unit long. The same
term is now a few hundred units across. The note has not yet been revised to
match.

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
| `rhoviz!("watch", *out)` | pictures arrive on `out` as `("ok", svg, note, term, button)`, at most one a frame and only when the picture changed; `button` is `"pause"` while playback runs, else `"play"`; a term that does not parse arrives as `("err", message)` |
| `rhoviz!("open", term, steps)` | start on `term` (a string); `steps` is a whole number or `"*"` |
| `rhoviz!("play")`, `("pause")`, `("toggle")`, `("next")`, `("prev")`, `("restart")` | playback |
| `rhoviz!("spin")`, `("zoom", "in" \| "out")`, `("wheel", dy)`, `("home")`, `("drag")`, `("pointer", "down" \| "move" \| "up", x, y)` | the camera: `wheel` zooms about the last pointer position, `drag` switches a drag between turning and moving |
| `rhoviz!("place", x, y, w)`, `("fit", w)`, `("size", w, h)` | where the picture is in the page and the size to draw it at, in pixels |
| `rhoviz!("close")` | forget the term |

### Laziness

Frames are computed as playback reaches them: one more state of the world per
step of the animation. A term that never comes to rest therefore plays for as
long as it is watched and never stalls a frame. Three limits apply: 5,000
frames are kept (playback can seek backwards); a single frame may consume at
most 20,000 ticks without the world visibly changing; and the term itself may
grow without bound, which is the emptor's caveat.

### Keeping the window responsive

The browser parses, lays out and paints a picture on the thread that also takes
its input, and a large picture costs tens of milliseconds. Pictures are
therefore rationed: about thirty a second while they are small, down to eight a
second for a picture of a few hundred nodes. What is off the picture, or
smaller than a third of a pixel, is not drawn at all.

### Replay

Pictures are large and frequent, so they are *not* written to the `.gzlog`. A
picture is a function of the term, of the events the page forwarded and of the
frame times, all of which the log holds, so a replay draws the pictures again.
This is exact on the machine that recorded the log. The pictures are computed in
floating point, so a replay on a different platform could differ in a last
digit and report a different commit hash; nothing else in F1R3Gaze has that
property, and it is the price of a log that does not grow by megabytes a
minute.

## Limits

- A large term is still a large figure: zoom in to read it.
- Which way a scroll zooms follows the sign the device reports, and has only
  been tried through synthetic events.
- A component that reacts and is re-formed identically (the listener in the
  default term) keeps its identity and is not ringed.
- The store does not say whether a datum at rest is persistent, so a
  persistent send at rest is drawn as an ordinary send.
- The term is one line of text; the page reads an `<input>`.
