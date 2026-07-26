# hallucinate — one-shot apps dreamed up live by AI

    hallucinate "a web browser but every image is Garry Newman"

There is no real application behind the window. An LLM (Gemini) writes the whole
thing on the spot — real HTML, real CSS, real JavaScript — and a real browser
engine runs it. What the app can compute, it computes: a calculator does
arithmetic instantly, for free. What it cannot know, it asks the model for while
you use it: the page behind a URL you typed, an NPC's next line, the picture in
an image slot. Close the window and it's gone; nothing was ever saved, nothing
was ever real.

```
hallucinate "a web browser but every image is Garry Newman"
hallucinate "a calculator that gets increasingly passive-aggressive"
hallucinate "a mood ring that guesses my feelings"
hallucinate "a fake terminal that lies about everything"
hallucinate "a file manager for files that don't exist"
```

## How it works

```
hallucinate "concept"
   -> Gemini writes the app as HTML/CSS/JS against a design system it is handed
      (streamed into the splash, so you watch it being written)
   -> Chromium runs it: real layout, real CSS, real events
   -> you use it
        clicking, typing, dragging      -> the app's own JS. no model, no cost
        <img data-dream="...">          -> host draws it as SVG, cached on disk
        await hallucinate.dream(...)    -> model invents what the app can't know
   -> Ctrl+R dreams the same concept again from scratch
```

## The three pieces

**`kit.css` — the design system.** Injected into every dreamed page, never
generated. Tokens (`--bg`, `--accent`, `--s1..--s6`, `--r`, …) plus about forty
component classes: `.app .titlebar .toolbar .sidebar .main .card .list .item
.tabs .chip .badge .sheet .toast .keypad .display .spinner .overlay`, and bare
`button`/`input`/`table` are styled already. Separately, `.page` carries
*document* typography — margins, lists, links, blockquote, code — because app
chrome wants those margins gone and a dreamed article or web page needs them
back; dreamed HTML arrives wrapped in it and can override it with its own style. This is the whole quality argument
*and* the whole token argument: naming the classes costs ~350 input tokens once
and saves the model writing a stylesheet — thousands of output tokens — on every
single dream. A concept that wants to be loud overrides `:root` and inherits the
rest of the system.

**`runtime.js` — the line back to the model.** Also injected, also free:

| | |
|---|---|
| `await hallucinate.dream(prompt)` | invented text or markup, as a string |
| `await hallucinate.json(prompt, shape)` | invented data, shaped like the example you pass |
| `hallucinate.mount(el, html)` | put dreamed markup on screen **and run it** |
| `hallucinate.root` | inside a dreamed page's script, its own container |
| `<img data-dream="...">` | filled in automatically, no call needed |

Images are caught however they arrive — in the first markup, added later by the
app's own code, or inserted bare and briefed a moment afterwards.

`mount()` is what makes dreamed content *work*. `innerHTML` silently refuses to
execute `<script>` elements, so a page assigned that way is a photograph of an
app: its tabs, filters and forms do nothing. `mount()` re-creates each script so
it runs — and since it runs in the same world, **a dreamed page can dream its own
next page.** Type a URL, get a working site; click a category on that site and it
filters; follow a link and the next page is dreamed in turn.

**`dreamer.py` — everything that talks to Gemini.** The opening dream (streamed),
content a running app asks for (cached by prompt), and art (cached on disk by
content hash, deduplicated in flight, drawn at a detail level chosen from the
slot size).

## Images

Every image is a dreamed SVG illustration — the text model draws it, styled to
sit in the app's palette. Vector, not photoreal, and free.

Real photographic generation was the other option and is deliberately not wired
up: `gemini-3.1-flash-image`, `nano-banana-pro` and the imagen models all return
HTTP 429 on a free-tier key, and would cost money and ~5-10s per picture. SVG is
unlimited, ~1s for an icon, and stylistically consistent with the app around it.

Slot size buys detail, because a 32px favicon that costs as much as a hero image
is the budget spent on something nobody can see:

| slot | detail | ceiling | measured |
|---|---|---|---|
| ≤128px | icon | 1200 tok | ~1.0s, 0.6 KB |
| ≤360px | thumbnail | 5000 tok | ~6s, 5.8 KB |
| larger | full scene | 9000 tok | ~11s, 8.3 KB |

An `<img>` parses SVG as strict XML, so a drawing truncated mid-attribute is a
broken image rather than a rougher one. Anything that doesn't end in `</svg>` is
redrawn once with double the ceiling, and a broken drawing is never cached.

## Latency and cost

Measured on `gemini-flash-lite-latest`:

| | time | output tokens |
|---|---|---|
| opening dream | 7-10s | ~3k |
| a dreamed page / reply | 1-3s | a few hundred |
| an image | 1-11s by size | 600-9000 |
| pressing a button, typing, dragging | 0 ms | **0** |

That last row is the point of the hybrid design. Under the old
every-event-round-trips model, a calculator keypress cost a request and ~0.6s;
now the app's own JavaScript handles it, and the model is spent only on things
that have to be invented.

Caches: art on disk at `~/.cache/hallucinate/art/<sha1>.svg` (so a repeat image
is free forever), dreamed content in memory for the session (so navigating back
to a page is instant).

## Config

- `HALLUCINATE_MODEL` — default `gemini-flash-lite-latest`. Lite spends no
  thought tokens and returns a complete app in 7-10s. `gemini-flash-latest` is a
  thinking model: 40-60s and 8-13k output tokens, for a markedly richer app (one
  browser came back with 31 image slots instead of 4). Worth it when you want to
  stare at the result, not when you want to use it. Version-pinned ids are 404
  for new keys, hence the rolling alias.
- Thinking is asked for at `thinkingLevel: minimal` and *negotiated*, not pinned:
  the field's spelling changes between model generations, so it is sent
  optimistically and dropped for the rest of the run if a model rejects it.
- `hallucinate --print "concept"` — write the dreamed HTML to stdout, no window.
- `Ctrl+R` / `F5` — dream the concept again.
- The dreamed page's console is forwarded to the terminal, prefixed
  `hallucinate[page:N]` — the only way to see why an app nobody reviewed
  misbehaves.

## API key

Never committed (secret scanning blocks that, rightly). Resolved at runtime:

1. `--api-key <key>`
2. `$GEMINI_API_KEY`
3. `~/.config/caelestia/gemini.key` (one line, `chmod 600`)

```
install -m600 /dev/stdin ~/.config/caelestia/gemini.key <<<'YOUR_GEMINI_KEY'
```

## Install

```
sudo pacman -S --needed python-pyqt6-webengine
ln -sf "$HOME/.config/quickshell/caelestia/system/hallucinate/hallucinate" ~/.local/bin/hallucinate
```

`qt6-webengine` — the ~100 MB Chromium half — is already a caelestia dependency;
only the Python bindings are added. No pip, no venv. The `tk` dependency the
first version needed is gone.
