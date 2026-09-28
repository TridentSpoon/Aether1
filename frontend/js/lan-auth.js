/* The browser half of `--serve --lan` pairing.
 *
 * Step 45 built every piece of LAN access except the one a person actually touches. The
 * server hands out a per-device token at `POST /api/pair` in exchange for the pairing
 * phrase, and refuses every other route without `Authorization: Bearer <token>` -- but
 * nothing in this page had ever asked for a phrase, stored a token, or sent one. So a
 * browser on another machine loaded the interface, every request behind it came back 401,
 * and the screen sat there empty no matter which phrase the operator had. That is what
 * "connecting via LAN doesn't work, regardless of the passphrase" was: not a wrong phrase,
 * no way to type one.
 *
 * Three decisions worth keeping:
 *
 * 1. **The static files stay reachable without a token, on purpose.** They are how this
 *    prompt gets on screen; gating them would mean pairing in a browser was impossible by
 *    construction. Nothing behind them is readable until a token is held, which is where the
 *    boundary belongs.
 * 2. **The token lives in localStorage, per origin.** It is a credential, so this is not
 *    free -- but the alternative is re-typing twelve words on every reload, which is how
 *    people end up leaving the phrase in a note next to the machine. It is scoped to the
 *    server's own origin, cleared the moment the server stops honouring it, and revocable
 *    from the other end at any time (`aether1 revoke <id>`, or Revoke in the pane).
 * 3. **The desktop shell never sees any of this.** It talks to loopback, which needs no
 *    token at all, so `IS_TAURI` short-circuits every path here rather than the app
 *    carrying a credential it has no use for.
 */

(function () {
    'use strict';

    // Kept in step with app.js's own test, which runs later: this file has to answer the
    // question before app.js exists.
    const IS_TAURI = typeof window.__TAURI_INTERNALS__ !== 'undefined';

    // Namespaced by origin already (localStorage is), so the key only has to distinguish
    // this from AETHER1's other stored settings.
    const TOKEN_KEY = 'aether1-lan-token';

    /* The two subprotocol names, matching WS_PROTOCOL and WS_TOKEN_PREFIX in
     * src-tauri/src/server.rs. WS_PROTOCOL is the one the server selects; the prefixed one is
     * how the token gets there. */
    const WS_PROTOCOL = 'aether1';
    const WS_TOKEN_PREFIX = 'aether1.token.';

    /* A browser with storage blocked (private windows, some embedded webviews) must still be
     * able to pair -- it just pairs again next reload. Every access is guarded rather than
     * assuming localStorage is there and writable. */
    let memoryToken = null;

    function readToken() {
        try {
            return window.localStorage.getItem(TOKEN_KEY) || null;
        } catch (e) {
            return memoryToken;
        }
    }

    function writeToken(token) {
        memoryToken = token;
        try {
            window.localStorage.setItem(TOKEN_KEY, token);
        } catch (e) {
            /* in-memory only for this page's life */
        }
    }

    function clearToken() {
        memoryToken = null;
        try {
            window.localStorage.removeItem(TOKEN_KEY);
        } catch (e) {
            /* nothing stored to clear */
        }
    }

    /* Adds the credential to a fetch's options without disturbing whatever headers the
     * caller set. Returns the options unchanged when there is nothing to add, so the desktop
     * app and a loopback browser tab send exactly the bytes they always did. */
    function authorize(options) {
        if (IS_TAURI) return options;
        const token = readToken();
        if (!token) return options;
        const next = Object.assign({}, options || {});
        const headers = new Headers((options && options.headers) || {});
        headers.set('Authorization', `Bearer ${token}`);
        next.headers = headers;
        return next;
    }

    /* A WebSocket handshake from a browser cannot carry a custom header -- except this one.
     * The constructor's second argument becomes `Sec-WebSocket-Protocol`, so the token rides
     * there rather than in the URL, where access logs and proxy logs would write it down.
     *
     * Two are offered: the one carrying the token, and the plain one the server selects and
     * echoes. The plain one has to be in the list, because a browser closes a socket whose
     * requested subprotocol was not selected, and the server never selects the one with the
     * credential in it. */
    function wsProtocols() {
        if (IS_TAURI) return [WS_PROTOCOL];
        const token = readToken();
        return token ? [`${WS_TOKEN_PREFIX}${token}`, WS_PROTOCOL] : [WS_PROTOCOL];
    }

    /* Whether a response is the server saying "pair first" rather than any other failure.
     * 401 is only ever that here: no other route in server.rs returns it. */
    function isUnauthorized(response) {
        return !!response && response.status === 401;
    }

    /* The attempt limiter's answer. It is deliberately *not* treated as needing a phrase
     * when a token is already held: a paired device that has been made to wait is still
     * paired, and throwing its token away over a lockout would turn a minute's wait into
     * re-typing twelve words. With no token it is worth saying, because otherwise the prompt
     * would sit there looking like the phrase was the problem. */
    function isLockedOut(response) {
        return !!response && response.status === 429;
    }

    function retryAfter(response) {
        const header = response && response.headers && response.headers.get('Retry-After');
        const seconds = parseInt(header, 10);
        return Number.isFinite(seconds) && seconds > 0 ? seconds : 60;
    }

    let gate = null;

    /* Shown when the server refuses us and hidden by a reload, never by a dismiss button:
     * there is nothing to look at behind it, and an interface that half-works while every
     * request 401s is worse than a prompt that says why. */
    function showGate(message) {
        if (IS_TAURI) return;
        if (gate) {
            if (message) setNote(message, 'bad');
            return;
        }

        gate = document.createElement('div');
        gate.id = 'lan-pair-gate';
        /* Styled inline rather than from A1theme.css because this has to render before
         * anything else is known to have loaded, including the stylesheet's own variables. */
        gate.setAttribute(
            'style',
            'position:fixed;inset:0;z-index:99999;display:flex;align-items:center;'
            + 'justify-content:center;padding:24px;background:rgba(6,10,16,0.94);'
            + 'backdrop-filter:blur(6px);font-family:system-ui,-apple-system,sans-serif;'
            + 'color:#dbeafe;overflow:auto;'
        );
        gate.innerHTML = `
            <div style="width:100%;max-width:30rem;border:1px solid rgba(56,189,248,0.35);
                        border-radius:12px;background:rgba(8,15,26,0.96);padding:22px;
                        box-shadow:0 18px 50px rgba(0,0,0,0.5)">
                <h1 style="margin:0 0 6px;font-size:1.05rem;letter-spacing:0.04em;
                           text-transform:uppercase;color:#7dd3fc">Pair this device</h1>
                <p style="margin:0 0 16px;font-size:0.85rem;line-height:1.5;color:#94a3b8">
                    Type the code the other machine is showing you &mdash; Settings, Network
                    &amp; Remote, Pair a device &mdash; and this one is in. The twelve-word
                    pairing phrase works here too, if that is what you have.
                </p>
                <label for="lan-pair-phrase" style="display:block;margin-bottom:6px;
                       font-size:0.72rem;letter-spacing:0.08em;text-transform:uppercase;
                       color:#64748b">Pairing code or phrase</label>
                <textarea id="lan-pair-phrase" rows="2" autocomplete="off"
                    spellcheck="false" autocapitalize="characters"
                    placeholder="the code, or twelve words"
                    style="width:100%;box-sizing:border-box;padding:10px;border-radius:8px;
                           border:1px solid rgba(148,163,184,0.35);background:rgba(2,6,12,0.8);
                           color:#e2e8f0;font-size:0.9rem;resize:vertical"></textarea>
                <p id="lan-pair-note" style="margin:10px 0 0;font-size:0.8rem;line-height:1.45;
                   min-height:1.2em;color:#94a3b8"></p>
                <button id="lan-pair-submit" type="button" disabled
                    style="margin-top:14px;width:100%;padding:10px;border-radius:8px;border:0;
                           background:#0284c7;color:#f0f9ff;font-size:0.9rem;font-weight:600;
                           cursor:pointer">Pair this device</button>
            </div>`;
        document.body.appendChild(gate);

        const field = gate.querySelector('#lan-pair-phrase');
        const submit = gate.querySelector('#lan-pair-submit');

        /* Only emptiness is checked here. Whether twelve words are *the* twelve words is a
         * question only the server can answer, and guessing on this side would either refuse
         * a valid phrase or promise a bad one -- the same rule the pane's own field keeps. */
        function sync() {
            submit.disabled = !field.value.trim();
            submit.style.opacity = submit.disabled ? '0.5' : '1';
        }
        field.addEventListener('input', sync);
        field.addEventListener('keydown', (event) => {
            if (event.key === 'Enter' && !event.shiftKey && !submit.disabled) {
                event.preventDefault();
                submit.click();
            }
        });
        submit.addEventListener('click', () => attempt(field.value.trim(), submit));
        sync();
        field.focus();
        if (message) setNote(message, 'bad');
    }

    function setNote(text, tone) {
        const note = gate && gate.querySelector('#lan-pair-note');
        if (!note) return;
        note.textContent = text || '';
        note.style.color = tone === 'bad' ? '#fca5a5' : '#94a3b8';
    }

    async function attempt(phrase, submit) {
        if (!phrase) return;
        submit.disabled = true;
        submit.style.opacity = '0.5';
        setNote('Pairing…');
        try {
            const response = await fetch('/api/pair', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                /* The label the operator will see in the paired devices list. The server
                 * falls back to the User-Agent when this is absent; sending the hostname it
                 * was reached on is more use than "Chrome on Linux" when several devices
                 * pair from the same kind of browser. */
                body: JSON.stringify({ phrase, device: deviceLabel() }),
            });
            if (response.status === 429) {
                const wait = response.headers.get('Retry-After') || '60';
                setNote(
                    `Too many failed attempts from this device. Wait ${wait} seconds and try `
                    + 'again.',
                    'bad'
                );
                return;
            }
            if (!response.ok) {
                setNote(
                    (await response.text())
                        || `the server refused that phrase (${response.status})`,
                    'bad'
                );
                return;
            }
            const data = await response.json();
            if (!data || !data.token) {
                setNote('The server paired but sent no token back.', 'bad');
                return;
            }
            writeToken(data.token);
            setNote('Paired. Loading…');
            /* Reloaded rather than carried on from here: everything this page reads was
             * already asked for and refused while there was no token, and re-running startup
             * is both simpler and more honest than replaying it call by call. */
            window.location.reload();
        } catch (e) {
            setNote(String((e && e.message) || e), 'bad');
        } finally {
            if (submit.isConnected) {
                submit.disabled = false;
                submit.style.opacity = '1';
            }
        }
    }

    /* Something the operator can match against a machine they own. Deliberately not a
     * fingerprint: it goes in a list a person reads, not into any decision. */
    function deviceLabel() {
        const platform = (navigator.platform || '').trim();
        return platform ? `a browser on ${platform}` : 'a browser';
    }

    /* Called by apiFetch when the server refuses a request. The token is dropped first: it
     * has either been revoked or was minted against a phrase that has since been replaced,
     * and either way sending it again only spends attempts against the limiter. */
    function onUnauthorized() {
        if (IS_TAURI) return;
        const had = !!readToken();
        clearToken();
        showGate(had ? 'That device is no longer paired. Pair it again to carry on.' : '');
    }

    /* Asked once at load, so the prompt is up before the interface starts failing behind it
     * rather than after. `/api/static-info` is the cheapest route behind the token wall and
     * says nothing itself; loopback answers it without a token, which is exactly the case
     * that must see no prompt. */
    async function probe() {
        if (IS_TAURI) return;
        try {
            const response = await fetch('/api/static-info', authorize({}));
            if (isUnauthorized(response)) {
                onUnauthorized();
            } else if (isLockedOut(response) && !readToken()) {
                showGate(
                    `This device has spent its pairing attempts. Wait ${retryAfter(response)} `
                    + 'seconds, then type the phrase.'
                );
            }
        } catch (e) {
            /* A server that cannot be reached at all is not a pairing problem, and a prompt
             * asking for a phrase would be the wrong thing to say about it. */
        }
    }

    window.LanAuth = {
        authorize,
        wsProtocols,
        isUnauthorized,
        isLockedOut,
        onUnauthorized,
        token: readToken,
        clear: clearToken,
    };

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', probe);
    } else {
        probe();
    }
})();
