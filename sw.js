/* fastPlay service worker — app-shell caching.
 *
 * Strategy:
 *   • navigations: network-first, falling back to the cached index.html so
 *     an installed PWA opens offline and the WS layer shows "reconnecting".
 *   • hashed static assets (wasm/js/css): cache-first — the filename
 *     changes with every build, so a cached copy is never stale.
 *   • anything else (including /ws upgrades, which never reach a SW):
 *     passthrough.
 */

const CACHE = 'fastplay-v1';

self.addEventListener('install', (event) => {
    event.waitUntil(caches.open(CACHE).then((c) => c.addAll(['/'])));
    self.skipWaiting();
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches.keys()
            .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
            .then(() => self.clients.claim())
    );
});

self.addEventListener('fetch', (event) => {
    const req = event.request;
    if (req.method !== 'GET') return;

    const url = new URL(req.url);
    if (url.origin !== self.location.origin) return;

    if (req.mode === 'navigate') {
        event.respondWith(
            fetch(req).catch(async () => {
                const cached = await caches.match('/');
                return cached || Response.error();
            })
        );
        return;
    }

    if (url.pathname.match(/\.(wasm|js|css|png|webmanifest)$/)) {
        event.respondWith(
            caches.match(req).then((hit) =>
                hit ||
                fetch(req).then((res) => {
                    const copy = res.clone();
                    caches.open(CACHE).then((c) => c.put(req, copy));
                    return res;
                })
            )
        );
    }
});
