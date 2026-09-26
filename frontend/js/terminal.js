/* The operator's terminal -- the drawing half.
 *
 * This file owns an xterm.js instance and the four invokes that drive a real shell on a
 * real pty in src-tauri/src/terminal.rs. What it deliberately does not own is any path
 * back into the rest of the app: nothing here writes to the conversation, the vault or the
 * database, and nothing here is reachable from the companion. The terminal is a window onto
 * the operator's own shell and that is all it is.
 *
 * It exists only in the native desktop app. In a browser -- `aether1 --serve`, or the HUD
 * reached over the LAN -- mount() is never called and the panel is removed from the DOM
 * outright, because there is no HTTP route behind it and a dead terminal on screen is worse
 * than no terminal: it looks like a thing that works.
 */
(function () {
    'use strict';

    /* Bytes, not text, in both directions. A pty carries escape sequences and cursor
       moves, and a multi-byte character lands across a chunk boundary often enough to
       matter, so the encoding stays base64 until xterm -- which understands both -- gets
       hold of it. */
    function bytesToBase64(bytes) {
        let binary = '';
        for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i]);
        return btoa(binary);
    }

    function base64ToBytes(b64) {
        const binary = atob(b64);
        const out = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
        return out;
    }

    /* The terminal's colours come from the HUD's theme rather than xterm's defaults, for
       the same reason the note graph's do: a canvas inherits nothing, and HUD cyan on
       Solar's near-white page is invisible. Read once per shell, which is the right
       granularity -- an open shell keeps the palette it started with rather than
       re-theming mid-command. */
    function readTheme(el) {
        const s = getComputedStyle(el);
        const pick = (name, fallback) => (s.getPropertyValue(name) || '').trim() || fallback;
        return {
            background: pick('--bg-core', '#050810'),
            foreground: pick('--text-main', '#d8e6f2'),
            cursor: pick('--neon-cyan', '#22d3ee'),
            selectionBackground: pick('--neon-blue', '#3b82f6') + '55',
        };
    }

    /* Mounts a terminal into `surface`.
     *
     * `invoke` and `listen` are handed in rather than reached for, so this file has no
     * opinion about how it is talking to Rust and app.js keeps the one place that knows --
     * the same split notes-graph.js uses.
     *
     * Returns { start, stop, fit, running, destroy }. */
    function mount(surface, { invoke, listen, onState }) {
        const term = new window.Terminal({
            fontFamily: '"Share Tech Mono", ui-monospace, monospace',
            fontSize: 13,
            cursorBlink: true,
            /* Enough to scroll back through a build, not so much that a runaway process
               eats the window's memory. */
            scrollback: 5000,
            theme: readTheme(surface),
        });
        /* Copy and paste, in the terminal's own spelling. xterm draws its own selection
           rather than the page's, so the window-wide rule that everything is selectable
           stops at this surface and the keys have to be wired by hand. Ctrl-C cannot be
           copy in here -- it is SIGINT, and a terminal that swallows it is broken -- so
           copy is Ctrl-Shift-C, as it is in every other terminal, and Cmd-C on macOS
           where there is no such clash. Ctrl-V already pastes through the helper
           textarea; Ctrl-Shift-V is the terminal spelling of the same thing. */
        const IS_MAC = /mac/i.test(navigator.platform || '');
        term.attachCustomKeyEventHandler((e) => {
            if (e.type !== 'keydown') return true;
            const combo = IS_MAC ? (e.metaKey && !e.ctrlKey) : (e.ctrlKey && e.shiftKey);
            if (!combo) return true;
            const key = (e.key || '').toLowerCase();
            if (key === 'c' && term.hasSelection()) {
                navigator.clipboard?.writeText(term.getSelection()).catch(() => {});
                return false;
            }
            if (key === 'v') {
                /* readText is the half of the clipboard a webview is most likely to
                   refuse. When it does, say nothing and let the keypress fall through:
                   the plain Ctrl-V path still works. */
                if (!navigator.clipboard?.readText) return true;
                navigator.clipboard.readText()
                    .then((text) => { if (text) term.paste(text); })
                    .catch(() => {});
                return false;
            }
            return true;
        });

        const fitAddon = new window.FitAddon.FitAddon();
        term.loadAddon(fitAddon);
        term.open(surface);

        let id = null;
        let unlistenOutput = null;
        let unlistenExit = null;
        let disposed = false;

        function state(text) {
            if (typeof onState === 'function') onState(text);
        }

        /* The panel is resizable and the window is resizable, and the shell has to be told
           both times or `less` pages at the wrong height for the rest of its life. */
        function fit() {
            if (disposed) return;
            try {
                fitAddon.fit();
            } catch (e) {
                /* Fitting a surface with no layout box (the panel switched off, the window
                   minimised) throws rather than returning nothing useful. */
                return;
            }
            if (id) {
                invoke('terminal_resize_rust', { id, rows: term.rows, cols: term.cols })
                    .catch(() => { /* the shell went away between the resize and here */ });
            }
        }

        async function start() {
            if (id || disposed) return;
            try {
                fitAddon.fit();
            } catch (e) { /* first fit before layout has settled; open with the default size */ }

            id = await invoke('terminal_open_rust', {
                rows: term.rows || 24,
                cols: term.cols || 80,
            });

            unlistenOutput = await listen('terminal-output', (event) => {
                const p = event.payload;
                if (!p || p.id !== id) return;
                term.write(base64ToBytes(p.bytes));
            });

            unlistenExit = await listen('terminal-exit', (event) => {
                if (!event.payload || event.payload.id !== id) return;
                /* Said in the terminal itself rather than only in the status line, because
                   that is where the operator is looking when they type `exit`. */
                term.write('\r\n\x1b[2m[the shell exited]\x1b[0m\r\n');
                id = null;
                state('exited');
            });

            /* Keystrokes straight through, unexamined. A terminal that second-guesses your
               keys is broken -- Ctrl-C, Ctrl-D and an arrow key are all just bytes. */
            term.onData((data) => {
                if (!id) return;
                const bytes = new TextEncoder().encode(data);
                invoke('terminal_write_rust', { id, bytes: bytesToBase64(bytes) })
                    .catch(() => { /* raced with the shell exiting */ });
            });

            state('running');
            term.focus();
            fit();
        }

        /* Types text into the shell, exactly as the keyboard would, and stops.
         *
         * This is the one way anything outside this file puts characters in here, and the
         * one rule it enforces is the reason it can exist at all: **no newline, ever.** A
         * shell runs a line the instant the newline arrives, so text carrying its own
         * newlines would execute itself on the way in, and the operator's Return key --
         * which is the whole consent model for a command the model suggested -- would
         * never get a say. The Rust side already yields one command per line with no
         * newline in it; this refuses them again because this is where the bytes actually
         * reach a pty, and a guarantee is worth having at the boundary that can break it.
         *
         * Returns false when there is no shell running, so the caller can start one. */
        function type(text) {
            if (!id) return false;
            const clean = String(text).replace(/[\r\n]+/g, ' ').trim();
            if (!clean) return false;
            const bytes = new TextEncoder().encode(clean);
            invoke('terminal_write_rust', { id, bytes: bytesToBase64(bytes) })
                .catch(() => { /* raced with the shell exiting */ });
            term.focus();
            return true;
        }

        async function stop() {
            if (!id) return;
            const closing = id;
            id = null;
            state('closed');
            await invoke('terminal_close_rust', { id: closing }).catch(() => {});
        }

        function destroy() {
            disposed = true;
            if (unlistenOutput) { unlistenOutput(); unlistenOutput = null; }
            if (unlistenExit) { unlistenExit(); unlistenExit = null; }
            stop();
            term.dispose();
        }

        /* The panel is a grid cell the operator can drag to any size, so this watches the
           box rather than the window. */
        const observer = new ResizeObserver(() => fit());
        observer.observe(surface);

        return {
            start,
            stop,
            type,
            fit,
            destroy() {
                observer.disconnect();
                destroy();
            },
            running() {
                return id !== null;
            },
        };
    }

    window.Aether1Terminal = { mount };
})();
