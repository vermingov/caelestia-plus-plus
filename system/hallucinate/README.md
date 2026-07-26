# hallucinate — one-shot apps dreamed up live by AI

    hallucinate "basic calculator with weird font"

There is no real application behind the window. An LLM (Gemini) invents the
whole thing on the spot: it designs the UI, and then it *is* the backend.
Press `2`, `+`, `2`, `=` and the model — not any local code — decides what the
display now reads. Every click, keystroke, toggle and slider release is sent
back to the model, which returns the next state of the app. Close the window
and it's gone forever; nothing was ever saved, nothing was ever real.

```
hallucinate "basic calculator with weird font"
hallucinate "a mood ring that guesses my feelings"
hallucinate "number guessing game, 1 to 100"
hallucinate "a fake terminal that lies about everything"
hallucinate "unit converter designed by a caffeinated goblin"
```

## How it works

```
hallucinate "concept"
   -> Gemini dreams the initial UI as constrained JSON widgets  -> Tkinter renders it
   -> user clicks / types / toggles
        -> a compact snapshot (ids + current values) + the event are sent to Gemini
        -> Gemini returns a tiny PATCH — only the widgets whose value changed
           (it did the "computation"); the window updates those in place
   -> repeat until closed; hidden state lives in a round-tripped `memory` field
```

The model may only use a small, well-behaved widget palette — `label`,
`button`, `entry`, `text`, `checkbox`, `slider` — laid out on a grid. A narrow
vocabulary means it can only assemble things that actually render, however
weird the concept gets. Bad colours or unknown font names never crash the
window; they fall back to a dark theme.

## Design notes

- **The AI is the backend.** No arithmetic, no game logic, no state machine
  runs locally. A compact snapshot of the widgets (ids + current values) plus a
  round-tripped `memory` string are the entire state the model gets each turn —
  enough for calculators, games with a secret, converters, etc.
- **Patch, not full redraw.** Per interaction the model returns only the
  changed widget values (`updates: [{id, value}]`), not the whole regenerated
  UI — that alone cut the per-press latency from ~2s to ~0.7s. A patch may
  carry a full `widgets` list when the app genuinely needs a new screen.
- **Structured output.** Gemini is pinned to a JSON schema (`responseMimeType`
  + `responseSchema`) so replies are always valid, never prose or markdown.
  Thinking is turned down to the floor — left on, the model reasons *inside* the
  JSON and runs out of output before finishing the widget list.
- **The thinking knob is negotiated, not pinned.** Its spelling changes between
  model generations (`thinkingBudget: 0` on Gemini 2.x, `thinkingLevel` on 3.x,
  which rejects the old key with a bare HTTP 400) and the model alias rolls
  forward on its own. It is sent optimistically and dropped for the rest of the
  run if the API refuses it: a knob the model doesn't recognise costs one retry,
  not the whole app.
- **Stub specs are asked again.** Roughly one first draft in four comes back as
  the display label alone — `finishReason: STOP`, the model thinks it's done —
  which renders as a window with a `0` in it and nothing to press. A spec with
  no interactive widget at all is that failure, so it is re-dreamed (up to 3
  tries; a stub returns in under a second, so retrying is cheap).
- **Kept-alive HTTPS.** One connection is reused across presses, so there is no
  TLS handshake per interaction.
- **Threading.** Model calls run on a worker thread; results cross back to the
  UI through a queue drained by a main-thread poller (Tk is single-threaded and
  crashes if touched off-thread). The title shows `· hallucinating…` while a
  turn is in flight.
- **Pure standard library** apart from the HTTP call: Tkinter for the UI,
  `http.client` for Gemini. No pip, no venv.

## Latency

Every interaction is still a real model round-trip — *everything* is
hallucinated; pressing a digit literally asks the AI what the display should
say next. Two things keep the "backend" fast (~0.6-0.8s/press vs ~2s naively):
the model returns a **patch** (just the changed widget values, a few output
tokens) instead of regenerating the whole UI, and the HTTPS connection is kept
alive so there is no TLS handshake per press. The one slower step is the
initial dream (~2-3s), which builds the whole app.

## API key

The key is never committed to this repo (secret scanning blocks that, rightly).
It is resolved at runtime, in order:

1. `--api-key <key>`
2. `$GEMINI_API_KEY`
3. `~/.config/caelestia/gemini.key` (one line; `chmod 600`)

Drop your Gemini key in the file once and the command just works:

```
install -m600 /dev/stdin ~/.config/caelestia/gemini.key <<<'YOUR_GEMINI_KEY'
```

## Config

- `HALLUCINATE_MODEL` — overrides the model (default `gemini-flash-lite-latest`).
  Lite is used on purpose: it spends no thought tokens, so a turn costs ~2s and
  the whole widget set arrives. The non-lite `gemini-flash-latest` thinks
  regardless — hundreds of thought tokens, ~4s, and a thinner UI for the money.
  Version-pinned ids (`gemini-2.5-flash-lite` and friends) are already 404 for
  new keys, hence the rolling alias.
- `hallucinate --dry-run "concept"` — print the initial UI spec as JSON and
  exit, no window (handy for debugging / headless checks).

## Install

Two steps (the installer and the system scan do both automatically):

```
sudo pacman -S --needed tk
ln -sf "$HOME/.config/quickshell/caelestia/system/hallucinate/hallucinate" ~/.local/bin/hallucinate
```
