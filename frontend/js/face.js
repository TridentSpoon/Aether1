/**
 * The fullscreen face for Project AETHER1: the avatar filling a whole screen, with nothing
 * on it but the word for what the avatar is doing. Meant for a spare monitor, a television,
 * or the second screen of a laptop dock -- see build_face_window in src-tauri/src/main.rs,
 * the tray's "Fullscreen Face" item, and `aether1 face`.
 *
 * Like frontend/js/sprite.js this is a *mirror* of the main HUD's hologram, not a second
 * avatar: which shape, which colours, IDLE/LISTENING/THINKING/SPEAKING and the audio it is
 * reacting to all arrive over Tauri events the main window pushes (see setAvatarState and
 * pushAudioToSprite in js/app.js). Nothing is decided here.
 *
 * Unlike the sprite, it is a mirror in one direction only. The sprite can be clicked to
 * start and stop listening, because it replaces the HUD on your desktop and you are sitting
 * in front of it. This window is on a screen you are not at, and a fullscreen surface that
 * starts recording when something brushes the mouse is not a feature. So: no drag, no
 * click, no microphone. Esc closes it, and that is the whole of its input.
 *
 * Only ever exists inside the native Tauri app -- like sprite.js, there is no browser
 * fallback branch here, because a browser tab cannot be an undecorated fullscreen window on
 * a chosen monitor.
 */

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');
    const stateEl = document.getElementById('face-state');
    const hintEl = document.getElementById('face-hint');

    const tauri = window.__TAURI__;
    const events = tauri && tauri.event;

    function setState(state) {
        // Only the four the state machine actually produces (see core.js). Anything else is
        // a bug somewhere upstream, and painting it here would make this window the place it
        // gets noticed, which is the wrong place -- so it is passed to the avatar (which has
        // its own handling) and left off the screen.
        const known = ['IDLE', 'LISTENING', 'THINKING', 'SPEAKING'];
        hologram.setState(state);
        if (!stateEl || !known.includes(state)) return;
        stateEl.textContent = state;
        stateEl.setAttribute('data-state', state);
    }

    /* Closing. This window has no title bar, so Esc is the way out, and the hint in the
       corner says so. The close itself goes through Rust rather than window.close() so that
       the tray item, `aether1 face` and this key are all asking the same command whether the
       face is open (see toggle_face_window_rust). */
    function close() {
        if (!tauri || !tauri.core) return;
        tauri.core.invoke('toggle_face_window_rust', { enabled: false }).catch((e) => {
            console.warn('Could not close the fullscreen face', e);
        });
    }

    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') close();
    });

    /* The hint, and the pointer, on the same timer: both are shown for a few seconds at a
       time and then get out of the way, because everything on this screen that is not the
       avatar is clutter the moment it has been read. Any mouse movement brings both back --
       on a monitor that never takes keyboard focus, Esc never arrives, and the pointer is
       then the only way to reach the window at all. */
    const HINT_MS = 4000;
    let hintTimer = null;

    function showHint() {
        if (hintEl) hintEl.classList.add('showing');
        document.body.classList.add('pointer-visible');
        clearTimeout(hintTimer);
        hintTimer = setTimeout(() => {
            if (hintEl) hintEl.classList.remove('showing');
            document.body.classList.remove('pointer-visible');
        }, HINT_MS);
    }

    showHint();
    document.addEventListener('mousemove', showHint);

    /* Startup: the same localStorage keys the HUD writes, read the same way sprite.js reads
       them, so the face opens already showing the right avatar in the right colours rather
       than a default one that corrects itself a moment later. */
    const savedAvatar = localStorage.getItem('aether_avatar') || 'a1';
    const savedTheme = Aether1Theme.apply(document);
    /* The one thing this window asks the engine to do differently from every other: use the
       screen. In the HUD an avatar reads at the resting size it was designed at, because it
       shares a window with eleven other panels and a chat log. Here it is the only thing
       there is, and an avatar drawn at panel size in the middle of a monitor is a postage
       stamp on a wall. Set before setAvatar so the first frame is already the right size --
       the engine reapplies it on every avatar change and every resize (see applyContentFit
       in js/hologram/core.js), so nothing here has to remember to. */
    hologram.setFillFraction(1);
    hologram.setAvatar(savedAvatar);
    hologram.setColorPalette(Aether1Theme.paletteFor(savedTheme.colours));
    setState('IDLE');

    // Same staleness check app.js and sprite.js run: build() reads the custom avatar's
    // recipe once, when the plugin is first constructed, so without this the face keeps
    // showing whatever "Your own" looked like when this window opened.
    function refreshCustomAvatarIfStale() {
        if (!window.CustomAvatarRecipe || !window.CustomAvatarRecipe.isStale()) return;
        hologram.rebuildRegisteredAvatar('custom');
    }

    if (events) {
        events.listen('avatar-changed', (event) => {
            hologram.setAvatar(event.payload.avatar);
            if (event.payload.avatar === 'custom') refreshCustomAvatarIfStale();
        }).catch((e) => console.warn('Could not listen for avatar changes', e));

        events.listen('color-theme-changed', (event) => {
            const theme = event.payload;
            if (!theme || !theme.colours) return;
            Aether1Theme.paint(document, theme.mode, theme.colours);
            hologram.setColorPalette(Aether1Theme.paletteFor(theme.colours));
        }).catch((e) => console.warn('Could not listen for color theme changes', e));

        events.listen('hologram-state-changed', (event) => {
            setState(event.payload.state);
        }).catch((e) => console.warn('Could not listen for hologram state changes', e));

        events.listen('hologram-audio-changed', (event) => {
            hologram.updateAudioData(event.payload.data);
        }).catch((e) => console.warn('Could not listen for hologram audio changes', e));

        /* State and audio are the two the HUD does *not* send unconditionally: they are
           per-frame-ish traffic across the IPC boundary, so app.js only emits them while
           something is actually mirroring the avatar (see the mirrors set in app.js). Saying
           so is this window's one and only outbound message. The reply is a snapshot of
           where the avatar is right now, which is what stops a face opened mid-sentence from
           sitting on IDLE until the next thing happens. */
        events.emit('avatar-mirror-attached', { window: 'face' })
            .catch((e) => console.warn('Could not attach to the HUD avatar', e));

        window.addEventListener('beforeunload', () => {
            events.emit('avatar-mirror-detached', { window: 'face' }).catch(() => {});
        });
    }

    // localStorage fires this in *other* windows of the same origin -- so a design saved in
    // the workbench while the face is already showing "Your own" updates here immediately,
    // with no reload and nothing to click.
    window.addEventListener('storage', (event) => {
        if (!window.CustomAvatarRecipe) return;
        if (event.key !== window.CustomAvatarRecipe.key) return;
        if (hologram.currentAvatar !== 'custom') return;
        refreshCustomAvatarIfStale();
    });
});
