/* Which shell the window wears, stamped onto <html> before anything is painted.
 *
 * Two of them. HUD is the free-form panel grid AETHER1 has always been: everything
 * on screen at once, draggable, resizable. STUDIO is the chat-first one -- a left
 * sidebar, one centred column, the avatar behind it -- for the evenings that are a
 * conversation rather than a console. STUDIO is the one a window opens in, because
 * the first thing anyone does with this app is talk to it; the HUD is a keypress away
 * and is still where you go to have the whole console at once.
 *
 * It lives here, in the head and in localStorage, for the same reason the theme does:
 * read after first paint, the window draws the panel grid and then jumps to the studio,
 * which reads as a bug in an app whose whole first impression is this screen.
 *
 * The avatar's place in the studio rides along, because it is the same question asked
 * about the same screen: behind the conversation, or small in the corner.
 */
(function (global) {
    'use strict';

    const KEY = 'aether_shell';
    const SHELLS = ['hud', 'studio'];
    const PLACES = ['background', 'minitar', 'off'];
    const DEFAULTS = { shell: 'studio', avatar: 'background', sidebar: 'open' };

    /* localStorage throws rather than returning null when site data is blocked, so every
       read and write here is guarded. A window that cannot remember which shell it was in
       is a smaller problem than one that will not start. */
    function clean(raw) {
        const stored = raw || {};
        return {
            shell: SHELLS.includes(stored.shell) ? stored.shell : DEFAULTS.shell,
            avatar: PLACES.includes(stored.avatar) ? stored.avatar : DEFAULTS.avatar,
            sidebar: stored.sidebar === 'closed' ? 'closed' : DEFAULTS.sidebar,
        };
    }

    /* Read back as the raw object rather than a cleaned one, because the difference
       between "stored nothing" and "stored the default" is the whole of how a default
       can ever change. Collapsing the sidebar writes the sidebar; it must not quietly
       write down the shell the window happened to be in at the time, or every install
       that ever touched the sidebar would be pinned to whatever the default was then. */
    function readRaw() {
        try {
            const raw = JSON.parse(global.localStorage.getItem(KEY) || '{}');
            return raw && typeof raw === 'object' ? raw : {};
        } catch (e) {
            return {};
        }
    }

    /* Only the keys that were actually asked for, each validated. An unknown value is
       dropped rather than stored, so it falls back to the default on the next read
       instead of sitting in storage forever. */
    function keep(raw) {
        const out = {};
        if (SHELLS.includes(raw.shell)) out.shell = raw.shell;
        if (PLACES.includes(raw.avatar)) out.avatar = raw.avatar;
        if (raw.sidebar === 'closed' || raw.sidebar === 'open') out.sidebar = raw.sidebar;
        return out;
    }

    function write(chosen) {
        try {
            global.localStorage.setItem(KEY, JSON.stringify(chosen));
        } catch (e) {
            /* Nothing to do about it and nothing worth saying: the shell still works,
               it just will not be the one on screen next launch. */
        }
    }

    let chosen = keep(readRaw());
    let state = clean(chosen);

    function paint() {
        const root = global.document.documentElement;
        root.dataset.shell = state.shell;
        root.dataset.studioAvatar = state.avatar;
        root.dataset.studioSidebar = state.sidebar;
    }

    paint();

    global.A1Shell = {
        get: () => ({ ...state }),
        set(patch) {
            // Validated on the way in as well as on the way out of storage, so a bad value
            // from a caller cannot reach <html> any more than a bad one in storage can.
            chosen = keep({ ...chosen, ...(patch || {}) });
            state = clean(chosen);
            write(chosen);
            paint();
            global.dispatchEvent(new CustomEvent('a1-shell-change', { detail: { ...state } }));
            return { ...state };
        },
    };
}(window));
