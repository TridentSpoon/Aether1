/* Which shell the window wears, stamped onto <html> before anything is painted.
 *
 * Two of them. HUD is the free-form panel grid AETHER1 has always been: everything
 * on screen at once, draggable, resizable. STUDIO is the chat-first one -- a left
 * sidebar, one centred column, the avatar behind it -- for the evenings that are a
 * conversation rather than a console.
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
    const DEFAULTS = { shell: 'hud', avatar: 'background', sidebar: 'open' };

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

    function read() {
        try {
            return clean(JSON.parse(global.localStorage.getItem(KEY) || '{}'));
        } catch (e) {
            return { ...DEFAULTS };
        }
    }

    function write(state) {
        try {
            global.localStorage.setItem(KEY, JSON.stringify(state));
        } catch (e) {
            /* Nothing to do about it and nothing worth saying: the shell still works,
               it just will not be the one on screen next launch. */
        }
    }

    let state = read();

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
            state = clean({ ...state, ...patch });
            write(state);
            paint();
            global.dispatchEvent(new CustomEvent('a1-shell-change', { detail: { ...state } }));
            return { ...state };
        },
    };
}(window));
