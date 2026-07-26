"""The window: a Chromium view, a bridge the dreamed page can call, and the
splash you watch the app being written into.
"""
from __future__ import annotations

import html
import json
import os
import sys
from concurrent.futures import ThreadPoolExecutor

from PyQt6.QtCore import QFile, QIODevice, QObject, pyqtSignal, pyqtSlot
from PyQt6.QtGui import QColor, QKeySequence, QShortcut
from PyQt6.QtWebChannel import QWebChannel
from PyQt6.QtWebEngineCore import QWebEnginePage, QWebEngineSettings
from PyQt6.QtWebEngineWidgets import QWebEngineView

from dreamer import DreamError

HERE = os.path.dirname(os.path.abspath(__file__))


def _read(name: str) -> str:
    with open(os.path.join(HERE, name), encoding="utf-8") as f:
        return f.read()


def _qt_resource(path: str) -> str:
    """Qt ships qwebchannel.js inside the library rather than on disk."""
    f = QFile(path)
    if not f.open(QIODevice.OpenModeFlag.ReadOnly):
        raise RuntimeError(f"missing Qt resource {path}")
    try:
        return bytes(f.readAll()).decode()
    finally:
        f.close()


def document(body: str, kit: str, runtime: str) -> str:
    """Kit and runtime go in the head of the document we build.

    QWebEngineScript's DocumentCreation injection is not reliably earlier than
    the page's own first inline <script>, and a dreamed app that calls
    hallucinate.dream() at parse time would find nothing there. Head scripts
    have no such ambiguity: they finish before the body is parsed.
    """
    return f"""<!doctype html>
<html><head><meta charset="utf-8"><style>{kit}</style>
<script>{runtime}</script></head>
<body>{body}</body></html>"""


class Host(QObject):
    """What the page can call. Work runs off the UI thread and resolves by signal."""

    resolved = pyqtSignal(str, bool, str)

    def __init__(self, dreamer, parent=None):
        super().__init__(parent)
        self.dreamer = dreamer
        # Enough workers that a page full of illustrations draws them at once,
        # few enough to stay under the free tier's per-minute request ceiling.
        self.pool = ThreadPoolExecutor(max_workers=5, thread_name_prefix="dream")

    @pyqtSlot(str, str, str)
    def request(self, request_id: str, kind: str, payload: str) -> None:
        self.pool.submit(self._work, request_id, kind, json.loads(payload))

    def _work(self, request_id: str, kind: str, args: dict) -> None:
        try:
            if kind == "content":
                result = self.dreamer.content(args["prompt"])
            elif kind == "json":
                result = self.dreamer.structured(args["prompt"], args["shape"])
            elif kind == "art":
                result = self.dreamer.art(args["description"], args["width"], args["height"])
            else:
                raise DreamError(f"unknown request {kind!r}")
        except DreamError as e:
            self.resolved.emit(request_id, False, str(e))
        else:
            self.resolved.emit(request_id, True, json.dumps(result))

    def shutdown(self) -> None:
        self.pool.shutdown(wait=False, cancel_futures=True)


class Page(QWebEnginePage):
    """A dreamed app is code nobody reviewed, so its console is the only window
    into why it misbehaves. Forward it to the terminal that launched us."""

    def javaScriptConsoleMessage(self, level, message, line, source):
        del level, source
        print(f"hallucinate[page:{line}] {message}", file=sys.stderr)


class Window(QWebEngineView):
    """Splash first, then the dreamed app in its place."""

    chunk_arrived = pyqtSignal(str)
    dream_done = pyqtSignal(str)
    dream_failed = pyqtSignal(str)
    redream = pyqtSignal()

    def __init__(self, dreamer):
        super().__init__()
        self.dreamer = dreamer
        self.kit = _read("kit.css")

        self.setWindowTitle(f"hallucinate · {dreamer.concept}")
        self.resize(1100, 760)
        self.setPage(Page(self))
        self.page().setBackgroundColor(QColor("#0d0d0d"))
        settings = self.settings()
        settings.setAttribute(QWebEngineSettings.WebAttribute.ShowScrollBars, False)
        settings.setAttribute(QWebEngineSettings.WebAttribute.FocusOnNavigationEnabled, True)

        self.host = Host(dreamer, self)
        channel = QWebChannel(self)
        channel.registerObject("host", self.host)
        self.page().setWebChannel(channel)
        # Qt ships qwebchannel.js inside the library; it has to run before ours.
        self.runtime = _qt_resource(":/qtwebchannel/qwebchannel.js") + "\n" + _read("runtime.js")

        self.chunk_arrived.connect(self._append_chunk)
        self.dream_done.connect(self._show_app)
        self.dream_failed.connect(self._show_error)
        self.titleChanged.connect(self._retitle)

        for keys in ("Ctrl+R", "F5"):
            QShortcut(QKeySequence(keys), self, activated=self.redream.emit)

    def _render(self, body: str) -> None:
        self.setHtml(document(body, self.kit, self.runtime))

    # -- splash -------------------------------------------------------------- #
    def show_splash(self) -> None:
        concept = html.escape(self.dreamer.concept)
        body = f"""
<div class="app center col" style="gap:var(--s4);padding:var(--s6)">
  <div class="col center" style="gap:var(--s2)">
    <div class="caps">hallucinating</div>
    <h1 class="dreaming" style="text-align:center;max-width:34ch">{concept}</h1>
  </div>
  <pre id="stream" class="mono faint" style="flex:1;width:min(880px,92vw);margin:0;
       overflow:hidden;font-size:11px;line-height:1.35;white-space:pre-wrap;
       word-break:break-all;mask-image:linear-gradient(#0000,#000 22%,#000 60%,#0000)"></pre>
</div>"""
        self._render(body)

    def _append_chunk(self, text: str) -> None:
        # Keep only the tail: this is a texture of the app being written, and a
        # 60 KB <pre> would just be slow.
        self.page().runJavaScript(
            "(t=>{const p=document.getElementById('stream');if(!p)return;"
            "p.textContent=(p.textContent+t).slice(-3000);"
            f"p.scrollTop=p.scrollHeight}})({json.dumps(text)})")

    def _show_error(self, message: str) -> None:
        body = f"""
<div class="app center col" style="gap:var(--s3);padding:var(--s6);text-align:center">
  <h2>the dream collapsed</h2>
  <p class="muted mono" style="max-width:60ch">{html.escape(message)}</p>
  <p class="faint">Ctrl+R to dream again</p>
</div>"""
        self._render(body)

    def _show_app(self, body: str) -> None:
        self._render(body)

    def _retitle(self, title: str) -> None:
        if title and not title.startswith("about:"):
            self.setWindowTitle(f"{title} · hallucinated")

    def closeEvent(self, event) -> None:
        self.host.shutdown()
        super().closeEvent(event)
