/* Which parts of the app are switched on.
 *
 * Trident, 2026-11-03: "I think it should be modular and allow the various
 * different options be it chat window, avatar holo or sub tabs in chat are
 * modules to enable I.E chat only or code only or term if a user wanted to use
 * the terminal only."
 *
 * Half of that already existed: layout.js has switched panels on and off since
 * the HUD had panels, so chat-only -- the hologram off -- was already a
 * checkbox. What had no switch was the eight tabs inside the chat panel, which
 * is where terminal-only and code-only live. So this is that half, built the
 * same way layout.js builds its own: the list comes from the markup rather than
 * from a list kept here, so a tab added to index.html turns up in Settings
 * without anyone having to write it down twice.
 *
 * Screen-local, in localStorage, for the same reason the panel arrangement is:
 * which tabs you want in front of you is a property of the machine you are
 * sitting at, not of the companion. A second screen can be set up differently
 * and neither has to agree with the other.
 */
(function (global) {
    'use strict';

    const KEY = 'aether_modules';

    /* Ids that are off. Stored as the exceptions rather than as the whole set,
       so a tab added later is on by default and an install that has never
       opened this screen stores nothing at all. */
    let off = new Set();
    let ids = [];

    function read() {
        try {
            const raw = JSON.parse(global.localStorage.getItem(KEY) || '[]');
            return new Set(Array.isArray(raw) ? raw.filter((v) => typeof v === 'string') : []);
        } catch (e) {
            return new Set();
        }
    }

    function write() {
        try {
            if (off.size) global.localStorage.setItem(KEY, JSON.stringify([...off]));
            else global.localStorage.removeItem(KEY);
        } catch (e) {
            /* The switches still work for this session; they will not survive it. */
        }
    }

    /* Read the tabs that are actually in the document. The terminal's tab and
       view are removed in a browser, where there is no route behind a shell, so
       the list is shorter there -- and a tab that is not there is not a module
       anyone can switch. */
    function scan() {
        ids = [...global.document.querySelectorAll('.chat-tab[data-chat-tab]')]
            .map((tab) => tab.getAttribute('data-chat-tab'));
        // Nothing may be off that is not here, or a stale entry quietly counts
        // against the "never switch the last one off" rule below.
        off = new Set([...off].filter((id) => ids.includes(id)));
        /* And nothing may leave every tab off. `set` refuses to switch off the last
           one, but the stored set can still cover every tab that exists -- switch
           everything but the terminal off in the app, then open the same machine in a
           browser, where there is no terminal tab, and the seven stored ids are all of
           them. That left a chat panel with no tabs and no way to get one back. The
           store is a preference, not an instruction: when honouring it would leave
           nothing, it is wrong and everything comes back on. */
        if (ids.length && off.size >= ids.length) off.clear();
    }

    function on() {
        return ids.filter((id) => !off.has(id));
    }

    function label(id) {
        const tab = global.document.querySelector(`.chat-tab[data-chat-tab="${id}"]`);
        return (tab && tab.textContent.trim()) || id;
    }

    function paint() {
        ids.forEach((id) => {
            const hidden = off.has(id);
            global.document.querySelector(`.chat-tab[data-chat-tab="${id}"]`)
                ?.classList.toggle('module-off', hidden);
            // The studio's sidebar is the same eight views by another route, so a
            // tab switched off has to go from both or the sidebar is a way back
            // into something the operator said they did not want.
            global.document.querySelector(`[data-studio-nav="${id}"]`)
                ?.classList.toggle('module-off', hidden);
        });
        global.dispatchEvent(new CustomEvent('a1-modules-change', { detail: { on: on() } }));
    }

    global.A1Modules = {
        /* Called once the document is parsed, and again if the terminal's tab is
           removed -- the list is only as good as the markup it was read from. */
        refresh() {
            scan();
            paint();
            return on();
        },
        ids: () => [...ids],
        on,
        label,
        isOn: (id) => ids.includes(id) && !off.has(id),
        set(id, wanted) {
            if (!ids.includes(id)) return on();
            /* The last one stays. A chat panel with no tabs is a panel with
               nothing in it and no way back, since the way back is a tab. */
            if (!wanted && on().length <= 1) return on();
            if (wanted) off.delete(id); else off.add(id);
            write();
            paint();
            return on();
        },
        reset() {
            off = new Set();
            write();
            paint();
            return on();
        },
    };

    off = read();
}(window));
