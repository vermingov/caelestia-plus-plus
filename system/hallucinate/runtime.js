/* Injected into every dreamed page before its own scripts run. Gives the app a
 * promise-based line back to the model, and fills <img data-dream="..."> slots
 * without the app having to ask. None of this costs the model any tokens. */
(() => {
  const pending = new Map();
  const queued = [];
  let bridge = null;
  let seq = 0;

  function send(kind, payload) {
    return new Promise((resolve, reject) => {
      const id = String(++seq);
      pending.set(id, { resolve, reject });
      const dispatch = () => bridge.request(id, kind, JSON.stringify(payload));
      if (bridge) dispatch(); else queued.push(dispatch);
    });
  }

  // Defined before the channel is touched. Both this and the engine's transport
  // are installed at document creation, and the order is not guaranteed — if
  // reaching for qt.webChannelTransport threw first, the app would come up with
  // no hallucinate object at all and every call would be a ReferenceError.
  /* Put dreamed markup on screen and let it come alive.
   *
   * innerHTML silently refuses to execute <script> elements, so a dreamed page
   * inserted that way is a photograph of an app rather than an app: its tabs,
   * forms and buttons do nothing. Re-creating each script node runs it, and
   * because it runs in this same world it can dream further content of its own. */
  function mount(target, markup) {
    const host = typeof target === 'string' ? document.querySelector(target) : target;
    if (!host) throw new Error('mount: no such target');
    host.innerHTML = markup;
    host.querySelectorAll('script').forEach(stale => {
      const live = document.createElement('script');
      for (const attr of stale.attributes) live.setAttribute(attr.name, attr.value);
      live.textContent = stale.textContent;
      // Dreamed markup shares one document with the app and with every page
      // mounted before it, so a script that queries by bare id is reaching into
      // its neighbours. hallucinate.root is its own container, set immediately
      // before the script runs (replaceWith executes it synchronously).
      window.hallucinate.root = stale.closest('.page') || host;
      stale.replaceWith(live);
    });
    return host;
  }

  window.hallucinate = {
    dream: prompt => send('content', { prompt }),
    json: (prompt, shape) => send('json', { prompt, shape }),
    mount,
  };

  function connect() {
    if (typeof QWebChannel === 'undefined' || typeof qt === 'undefined' || !qt.webChannelTransport) {
      setTimeout(connect, 0);
      return;
    }
    new QWebChannel(qt.webChannelTransport, channel => {
      bridge = channel.objects.host;
      bridge.resolved.connect((id, ok, payload) => {
        const promise = pending.get(id);
        if (!promise) return;
        pending.delete(id);
        if (ok) promise.resolve(JSON.parse(payload));
        else promise.reject(new Error(payload));
      });
      // Anything the page asked for before the channel opened runs now.
      while (queued.length) queued.shift()();
    });
  }
  connect();

  /* ---- image slots ------------------------------------------------------- */

  const claimed = new WeakSet();

  function fill(img) {
    // Claim only once there is something to draw. Claiming on sight would
    // permanently skip the common case of an app inserting its images first and
    // setting data-dream on them afterwards.
    const brief = img.dataset.dream;
    if (!brief || claimed.has(img)) return;
    claimed.add(img);

    // Attribute width/height, not layout width: the element has no box yet.
    const width = parseInt(img.getAttribute('width'), 10) || 480;
    const height = parseInt(img.getAttribute('height'), 10) || 320;
    img.setAttribute('data-dreaming', '');

    send('art', { description: brief, width, height })
      .then(svg => {
        img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
      })
      .catch(err => {
        img.alt = brief;
        console.warn('hallucinate: art failed —', err.message);
      })
      .finally(() => img.removeAttribute('data-dreaming'));
  }

  function scan(root) {
    if (root.matches && root.matches('img[data-dream]')) fill(root);
    if (root.querySelectorAll) root.querySelectorAll('img[data-dream]').forEach(fill);
  }

  function watch() {
    scan(document);
    // Dreamed pages rewrite themselves — a browser navigating, a feed loading
    // more — so new slots have to be caught as they appear. Attributes are
    // watched too: an app is equally likely to insert a bare <img> and brief it
    // a moment later.
    new MutationObserver(records => {
      for (const record of records) {
        if (record.type === 'attributes') fill(record.target);
        else record.addedNodes.forEach(scan);
      }
    }).observe(document.documentElement, {
      childList: true, subtree: true,
      attributes: true, attributeFilter: ['data-dream'],
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', watch, { once: true });
  } else {
    watch();
  }
})();
