"""Everything that talks to Gemini.

One object owns the app's whole relationship with the model: the opening dream,
the content a running app asks for, and the art that fills its image slots. It is
thread-safe because a page full of images dreams them all at once.
"""
from __future__ import annotations

import hashlib
import http.client
import json
import os
import re
import threading
import time
from contextlib import contextmanager

import prompts

HOST = "generativelanguage.googleapis.com"
CACHE_DIR = os.path.expanduser("~/.cache/hallucinate/art")

# flash-lite spends no thought tokens and answers in ~2s. Thinking must stay off
# or the model reasons inside the output; Gemini 2.x spelled that
# `thinkingBudget: 0` and Gemini 3 renamed it, so the knob is sent optimistically
# and dropped if a model does not recognise it (see _Client.call).
DEFAULT_MODEL = "gemini-flash-lite-latest"
THINKING_CONFIG = {"thinkingLevel": "minimal"}

FENCE = re.compile(r"^```[a-z]*\s*|\s*```$", re.I)

# Output ceilings per slot size. Generous enough that a drawing finishes — a
# truncated SVG is a broken image, not a rougher one — and small enough that a
# favicon never costs what a hero illustration does.
ART_BUDGET = {"icon": 1200, "thumb": 5000, "hero": 9000}


class DreamError(RuntimeError):
    pass


def _strip_fence(text: str) -> str:
    return FENCE.sub("", text.strip())


def _api_error(status: int, raw: bytes) -> str:
    """Google's failures are a JSON envelope; surface the message, not the box."""
    try:
        err = json.loads(raw)["error"]
        return f"HTTP {status}: {err.get('message')} [{err.get('status')}]"
    except (ValueError, KeyError, TypeError):
        return f"HTTP {status}: {raw[:300].decode(errors='replace')}"


def _retry_after(raw: bytes, fallback: float) -> float:
    """Seconds the API asked us to wait, from the RetryInfo detail on a 429."""
    try:
        for detail in json.loads(raw)["error"].get("details", []):
            delay = detail.get("retryDelay")
            if delay:
                return min(float(delay.rstrip("s")), 30.0)
    except (ValueError, KeyError, TypeError):
        pass
    return fallback


class _Pool:
    """A few kept-alive HTTPS connections.

    Reuse matters — a TLS handshake per call is a visible slice of the latency —
    but a single locked connection would serialise a page that wants eight
    illustrations at once, so idle connections are pooled instead.
    """

    def __init__(self, host: str, size: int = 6):
        self.host = host
        self.size = size
        self._idle: list[http.client.HTTPSConnection] = []
        self._lock = threading.Lock()

    @contextmanager
    def take(self, timeout: int):
        with self._lock:
            conn = self._idle.pop() if self._idle else None
        conn = conn or http.client.HTTPSConnection(self.host, timeout=timeout)
        try:
            yield conn
        except Exception:
            conn.close()
            raise
        else:
            with self._lock:
                if len(self._idle) < self.size:
                    self._idle.append(conn)
                    return
            conn.close()


class _Client:
    """HTTP layer: retries, the thinking-knob negotiation, error shaping."""

    def __init__(self, api_key: str, model: str):
        self.api_key = api_key
        self.model = model
        self.pool = _Pool(HOST)
        self.thinking = True
        self._thinking_lock = threading.Lock()

    def _body(self, system: str, user: str, config: dict, thinking: bool) -> bytes:
        generation = dict(config)
        if thinking:
            generation["thinkingConfig"] = THINKING_CONFIG
        return json.dumps({
            "system_instruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": user}]}],
            "generationConfig": generation,
        }).encode()

    def _post(self, path: str, body: bytes, timeout: int) -> tuple[int, bytes]:
        headers = {"Content-Type": "application/json", "x-goog-api-key": self.api_key}
        for attempt in (1, 2):  # a pooled socket can go stale; that fails once
            try:
                with self.pool.take(timeout) as conn:
                    conn.request("POST", path, body=body, headers=headers)
                    resp = conn.getresponse()
                    return resp.status, resp.read()
            except (http.client.HTTPException, OSError) as e:
                if attempt == 2:
                    raise DreamError(f"network: {e}") from e
        raise AssertionError("unreachable")

    def call(self, system: str, user: str, config: dict | None = None,
             timeout: int = 120, tries: int = 3) -> str:
        """One completion, as text. Retries rate limits, negotiates the thinking knob."""
        path = f"/v1beta/models/{self.model}:generateContent"
        config = config or {"temperature": 1.0}
        for attempt in range(tries):
            status, raw = self._post(path, self._body(system, user, config, self.thinking), timeout)
            if status == 400 and self.thinking:
                # The thinking config is the one field whose spelling drifts
                # between model generations. Running with default thinking beats
                # not running, so drop it — but only latch that off if dropping
                # it actually helped, or an unrelated 400 disables it forever.
                retry_status, retry_raw = self._post(path, self._body(system, user, config, False), timeout)
                if retry_status == 200:
                    with self._thinking_lock:
                        self.thinking = False
                    status, raw = retry_status, retry_raw
            if status == 429 and attempt < tries - 1:
                time.sleep(_retry_after(raw, fallback=2 ** attempt))
                continue
            if status != 200:
                raise DreamError(_api_error(status, raw))
            return self._text(json.loads(raw))
        raise DreamError("rate limited")

    def stream(self, system: str, user: str, on_chunk, timeout: int = 300) -> str:
        """Same, but hand back text as it arrives so the UI can show it landing.

        The opening dream is always a stream, so this needs the same thinking-knob
        fallback as call() — otherwise the next time the field is renamed the app
        cannot start at all.
        """
        path = f"/v1beta/models/{self.model}:streamGenerateContent?alt=sse"
        headers = {"Content-Type": "application/json", "x-goog-api-key": self.api_key}
        for thinking in (self.thinking, False):
            with self.pool.take(timeout) as conn:
                conn.request("POST", path, headers=headers,
                             body=self._body(system, user, {"temperature": 1.0}, thinking))
                resp = conn.getresponse()
                if resp.status == 400 and thinking:
                    resp.read()  # drain, so the pooled socket stays reusable
                    continue
                if resp.status != 200:
                    raise DreamError(_api_error(resp.status, resp.read()))
                parts: list[str] = []
                # Iterate the response, not resp.fp: the raw socket knows nothing
                # about chunked framing, so it never sees the end of the body and
                # blocks until the timeout.
                for line in resp:
                    if not line.startswith(b"data:"):
                        continue
                    try:
                        chunk = self._text(json.loads(line[5:]))
                    except (ValueError, DreamError):
                        continue
                    parts.append(chunk)
                    on_chunk(chunk)
            with self._thinking_lock:
                self.thinking = thinking
            return "".join(parts)
        raise AssertionError("unreachable")

    @staticmethod
    def _text(payload: dict) -> str:
        try:
            parts = payload["candidates"][0]["content"]["parts"]
        except (KeyError, IndexError) as e:
            feedback = payload.get("promptFeedback") or payload.get("error") or payload
            raise DreamError(f"no candidate ({json.dumps(feedback)[:200]})") from e
        return "".join(p.get("text", "") for p in parts)


class Dreamer:
    """The app's imagination. Every method is safe to call from any thread."""

    def __init__(self, api_key: str, model: str, concept: str):
        self.client = _Client(api_key, model)
        self.concept = concept
        self._content_cache: dict[str, str] = {}
        self._art_locks: dict[str, threading.Lock] = {}
        self._registry_lock = threading.Lock()
        os.makedirs(CACHE_DIR, exist_ok=True)

    # -- the opening dream --------------------------------------------------- #
    def app(self, on_chunk) -> str:
        html = self.client.stream(prompts.APP_SYSTEM,
                                  f"Dream this app: {self.concept!r}", on_chunk)
        return _strip_fence(html)

    # -- what a running app asks for ----------------------------------------- #
    def content(self, prompt: str) -> str:
        """Invented text or markup. Cached: apps re-ask for the same page."""
        cached = self._content_cache.get(prompt)
        if cached is not None:
            return cached
        text = _strip_fence(self.client.call(
            prompts.CONTENT_SYSTEM.format(concept=self.concept), prompt))
        self._content_cache[prompt] = text
        return text

    def structured(self, prompt: str, shape) -> object:
        """Invented data, shaped like the example the page passed in."""
        system = prompts.CONTENT_SYSTEM.format(concept=self.concept)
        user = f"{prompt}\n\nAnswer as JSON with exactly this shape:\n{json.dumps(shape)}"
        text = self.client.call(system, user, {"temperature": 1.0,
                                               "responseMimeType": "application/json"})
        try:
            return json.loads(text)
        except json.JSONDecodeError as e:
            raise DreamError(f"model returned non-JSON: {text[:160]}") from e

    # -- art ----------------------------------------------------------------- #
    def art(self, description: str, width: int, height: int) -> str:
        """An SVG for one image slot, cached on disk and deduplicated in flight.

        Detail is bought by slot size: a 24px favicon that costs as much as a hero
        illustration is the token budget spent on something nobody can see.
        """
        biggest = max(width, height)
        detail = "icon" if biggest <= 128 else "thumb" if biggest <= 360 else "hero"
        key = hashlib.sha1(f"{self.concept}|{detail}|{description}".encode()).hexdigest()
        path = os.path.join(CACHE_DIR, f"{key}.svg")

        with self._registry_lock:
            lock = self._art_locks.setdefault(key, threading.Lock())
        with lock:  # two slots with the same brief draw once, not twice
            try:
                with open(path, encoding="utf-8") as f:
                    return f.read()
            except OSError:
                pass
            svg = self._draw(description, width, height, detail)
            with open(path, "w", encoding="utf-8") as f:
                f.write(svg)
            return svg

    def _draw(self, description: str, width: int, height: int, detail: str) -> str:
        """Ask for the SVG, and insist on getting a whole one.

        An <img> renders SVG as strict XML, so a drawing cut off mid-attribute is
        not a slightly worse picture — it is a broken image icon. The ceiling is
        there to stop a favicon costing hero money, so when a drawing genuinely
        needs the room, buy it once rather than caching the wreckage.
        """
        system = prompts.ART_SYSTEM.format(concept=self.concept,
                                           detail=prompts.ART_DETAIL[detail])
        user = f"The slot is {width}x{height}px. Draw: {description}"
        for budget in (ART_BUDGET[detail], ART_BUDGET[detail] * 2):
            svg = _strip_fence(self.client.call(
                system, user, {"temperature": 1.0, "maxOutputTokens": budget}))
            start, end = svg.find("<svg"), svg.rfind("</svg>")
            if start >= 0 and end > start:
                return svg[start:end + len("</svg>")]
        raise DreamError(f"drawing never completed: {description[:60]}")
