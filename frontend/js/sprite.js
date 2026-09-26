/**
 * Desktop Sprite window logic for Project AETHER1.
 * A small transparent, always-on-top, undecorated window (see src-tauri/src/main.rs's
 * build_sprite_window) showing just the hologram avatar -- click it to toggle push-to-talk
 * listening in the main HUD, drag it around the desktop, reopen the full HUD from its topbar
 * button, or use the always-visible power button to send the avatar back into the main window.
 *
 * This is a pure mirror of the main window's hologram, not a second, separate avatar: it has
 * no chat of its own. Whatever the main HUD's avatar is doing -- which shape, which colours,
 * IDLE/LISTENING/THINKING/SPEAKING, the audio it's reacting to -- arrives here over Tauri
 * events the main window pushes the moment it changes (see setHologramAvatar, paintTheme,
 * setAvatarState and pushAudioToSprite in js/app.js). This window only ever reads them, with
 * one exception: a click on the avatar asks the main window to start or stop listening (see
 * initSpriteListenBridge in js/app.js) -- the mirror can request, but never decide, what the
 * real conversation does.
 *
 * This window only ever exists inside the native Tauri app (a browser tab can't be
 * transparent/always-on-top/frameless), so unlike app.js there's no IS_TAURI branching here --
 * every call below goes straight through window.__TAURI__.core.invoke.
 */

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');

    const viewportEl = document.getElementById('hologram-viewport');
    const btnHud = document.getElementById('sprite-btn-hud');
    const btnPower = document.getElementById('sprite-btn-power');

    async function tauriInvoke(cmd, args) {
        if (!window.__TAURI__ || !window.__TAURI__.core) {
            throw new Error('Tauri bridge unavailable');
        }
        return window.__TAURI__.core.invoke(cmd, args);
    }

    function showMainHud() {
        tauriInvoke('show_main_window_rust').catch((err) => console.warn('Could not open main HUD', err));
    }

    // Asks the main HUD to start or stop push-to-talk listening -- it decides which, since
    // this window has no idea whether the HUD is already mid-capture (see
    // initSpriteListenBridge in js/app.js).
    function toggleListening() {
        if (!window.__TAURI__ || !window.__TAURI__.event) return;
        window.__TAURI__.event.emit('sprite-toggle-listen').catch((err) => console.warn('Could not request listening toggle', err));
    }

    // Dragging: the window has no native title bar, so start an OS-level drag on mousedown
    // over the avatar (Tauri's usual pattern for custom-titlebar dragging -- see
    // start_window_drag_rust). If the user doesn't actually move the pointer, the browser
    // still fires a normal click afterward -- that click's job is to toggle listening in the
    // real conversation rather than starting one here.
    viewportEl.addEventListener('mousedown', (e) => {
        if (e.button !== 0) return;
        tauriInvoke('start_window_drag_rust').catch(() => {});
    });
    viewportEl.addEventListener('click', toggleListening);

    btnHud.addEventListener('click', (e) => {
        e.stopPropagation();
        showMainHud();
    });

    // Always visible, unlike the topbar (which only fades in on hover): this is the one
    // control that turns the sprite off, so it needs to be findable without first discovering
    // that hovering reveals a topbar at all.
    btnPower.addEventListener('click', async (e) => {
        e.stopPropagation();
        try {
            await tauriInvoke('save_settings_rust', { settings: { desktop_sprite_enabled: false } });
        } catch (err) {
            console.warn('Could not persist sprite-mode-off setting', err);
        }

        // Tell the HUD to take the avatar back *before* this window goes. Closing the sprite
        // is only half of what this button says it does: until the main window hears about
        // it, its hologram panel stays quiet behind the "the avatar is floating, click to
        // bring it back" notice, so the avatar isn't sent anywhere -- it just stops being
        // drawn. Awaited, because an event emitted from a window that has already closed
        // never lands.
        if (window.__TAURI__ && window.__TAURI__.event) {
            try {
                await window.__TAURI__.event.emit('sprite-mode-changed', { enabled: false });
            } catch (err) {
                console.warn('Could not tell the main window the sprite is going', err);
            }
        }

        // And the main window has to be on screen to send anything back to: closing the HUD
        // puts it in the tray rather than quitting it (see the CloseRequested handler in
        // main.rs), so without this the avatar can leave the desktop for a window nobody can
        // see, which is indistinguishable from the button doing nothing.
        try {
            await tauriInvoke('show_main_window_rust');
        } catch (err) {
            console.warn('Could not open the main HUD', err);
        }

        tauriInvoke('toggle_sprite_window_rust', { enabled: false }).catch(() => {});
    });

    // Matches whatever avatar/theme/voice-preference the main HUD is currently using --
    // aether_avatar is a plain localStorage key the main window already writes (see app.js),
    // shared here because both windows load from the same Tauri origin; the theme goes through
    // Aether1Theme so this window resolves it exactly as the HUD does, including falling back
    // to the OS light/dark setting when nothing has been chosen.
    // That only covers the sprite's own startup, though: if the HUD switches avatar/theme
    // while the sprite is already open, localStorage alone won't tell this window that
    // happened. Rather than have the sprite sit there polling localStorage for a change, the
    // HUD pushes it directly the moment it happens, over a Tauri event both windows share.
    const savedAvatar = localStorage.getItem('aether_avatar') || 'a1';
    const savedTheme = Aether1Theme.apply(document);
    hologram.setAvatar(savedAvatar);
    hologram.setColorPalette(Aether1Theme.paletteFor(savedTheme.colours));

    // Rebuild the custom avatar when the saved design has moved on from what is on screen
    // here -- same staleness check app.js runs. build() only ever reads the recipe once
    // (when the plugin is first constructed), so without this the sprite keeps showing
    // whatever "Your own" looked like when this window was opened, even after the HUD
    // (or the workbench) saves a new design.
    function refreshCustomAvatarIfStale() {
        if (!window.CustomAvatarRecipe || !window.CustomAvatarRecipe.isStale()) return;
        hologram.rebuildRegisteredAvatar('custom');
    }

    if (window.__TAURI__ && window.__TAURI__.event) {
        window.__TAURI__.event.listen('avatar-changed', (event) => {
            hologram.setAvatar(event.payload.avatar);
            if (event.payload.avatar === 'custom') refreshCustomAvatarIfStale();
        }).catch((e) => console.warn('Could not listen for avatar changes', e));

        /* The HUD sends the whole theme -- mode plus the three colours -- rather than a name,
           because a hand-mixed set has no name to send. */
        window.__TAURI__.event.listen('color-theme-changed', (event) => {
            const theme = event.payload;
            if (!theme || !theme.colours) return;
            Aether1Theme.paint(document, theme.mode, theme.colours);
            hologram.setColorPalette(Aether1Theme.paletteFor(theme.colours));
        }).catch((e) => console.warn('Could not listen for color theme changes', e));

        // The live mirror: whatever the main HUD's avatar is doing right now, not this
        // window's own idea of it -- see setAvatarState/pushAudioToSprite in app.js.
        window.__TAURI__.event.listen('hologram-state-changed', (event) => {
            hologram.setState(event.payload.state);
        }).catch((e) => console.warn('Could not listen for hologram state changes', e));

        window.__TAURI__.event.listen('hologram-audio-changed', (event) => {
            hologram.updateAudioData(event.payload.data);
        }).catch((e) => console.warn('Could not listen for hologram audio changes', e));
    }

    // localStorage fires this in *other* windows of the same origin -- same live-update
    // path app.js uses, so a design saved in the workbench while the sprite is already
    // showing "Your own" updates immediately, with no reload and nothing to click.
    window.addEventListener('storage', (event) => {
        if (!window.CustomAvatarRecipe) return;
        if (event.key !== window.CustomAvatarRecipe.key) return;
        if (hologram.currentAvatar !== 'custom') return;
        refreshCustomAvatarIfStale();
    });
});
