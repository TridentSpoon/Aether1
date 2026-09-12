/**
 * Trace Protocols unlock tracking.
 *
 * Nexus Sent, White Rabbit and Operator start hidden from both avatar pickers (the HUD's
 * avatar-menu slideout and the matching row in Settings) -- they are easter eggs surfaced by
 * a trigger phrase typed while The Nexus is the active avatar (see app.js's
 * AVATAR_TRIGGER_RULES / matchAvatarTrigger), not picked from a list up front. The first time
 * a given egg's trigger fires it is recorded here, and from then on it stays selectable like
 * any other avatar, surviving reloads.
 *
 * The avatar workbench (avatar-lab.html) deliberately ignores this and always shows all
 * three -- there is no chat/greeting flow there to ever trigger anything, so respecting these
 * flags would just make three avatars permanently unreachable in the tool meant for
 * previewing and tuning them.
 */
(function (global) {
    'use strict';

    const STORAGE_KEY = 'aether_avatar_unlocks';

    function load() {
        try {
            const raw = localStorage.getItem(STORAGE_KEY);
            const parsed = raw ? JSON.parse(raw) : {};
            return (parsed && typeof parsed === 'object') ? parsed : {};
        } catch (err) {
            return {};
        }
    }

    function isUnlocked(id) {
        return load()[id] === true;
    }

    /* Returns true only when this call is what unlocked it -- false if it was already
       unlocked, or the write failed -- so a caller can tell a real transition (reveal the
       picker entry) apart from a trigger firing again on an avatar already unlocked. */
    function unlock(id) {
        const flags = load();
        if (flags[id] === true) return false;
        flags[id] = true;
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(flags));
        } catch (err) {
            return false;
        }
        return true;
    }

    global.Aether1AvatarUnlocks = { STORAGE_KEY, isUnlocked, unlock };
})(window);
