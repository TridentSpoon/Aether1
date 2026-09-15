/**
 * Desktop Sprite window logic for Project AETHER1.
 * A small transparent, always-on-top, undecorated window (see src-tauri/src/main.rs's
 * build_sprite_window) showing just the hologram avatar -- click it to bring the main HUD
 * forward, drag it around the desktop, or reopen the full HUD from its topbar button.
 *
 * This is a pure mirror of the main window's hologram, not a second, separate avatar: it has
 * no chat of its own. Whatever the main HUD's avatar is doing -- which shape, which colours,
 * IDLE/LISTENING/THINKING/SPEAKING, the audio it's reacting to -- arrives here over Tauri
 * events the main window pushes the moment it changes (see setHologramAvatar, paintTheme,
 * setAvatarState and pushAudioToSprite in js/app.js). This window only ever reads them.
 *
 * This window only ever exists inside the native Tauri app (a browser tab can't be
 * transparent/always-on-top/frameless), so unlike app.js there's no IS_TAURI branching here --
 * every call below goes straight through window.__TAURI__.core.invoke.
 */

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');

    const viewportEl = document.getElementById('hologram-viewport');
    const btnHud = document.getElementById('sprite-btn-hud');
    const btnClose = document.getElementById('sprite-btn-close');

    async function tauriInvoke(cmd, args) {
        if (!window.__TAURI__ || !window.__TAURI__.core) {
            throw new Error('Tauri bridge unavailable');
        }
        return window.__TAURI__.core.invoke(cmd, args);
    }

    function showMainHud() {
        tauriInvoke('show_main_window_rust').catch((err) => console.warn('Could not open main HUD', err));
    }

    // Dragging: the window has no native title bar, so start an OS-level drag on mousedown
    // over the avatar (Tauri's usual pattern for custom-titlebar dragging -- see
    // start_window_drag_rust). If the user doesn't actually move the pointer, the browser
    // still fires a normal click afterward -- this is a mirror, not a control surface, so a
    // plain click's job is to raise the real conversation instead of opening one here.
    viewportEl.addEventListener('mousedown', (e) => {
        if (e.button !== 0) return;
        tauriInvoke('start_window_drag_rust').catch(() => {});
    });
    viewportEl.addEventListener('click', showMainHud);

    btnHud.addEventListener('click', (e) => {
        e.stopPropagation();
        showMainHud();
    });

    btnClose.addEventListener('click', async (e) => {
        e.stopPropagation();
        try {
            await tauriInvoke('save_settings_rust', { settings: { desktop_sprite_enabled: false } });
        } catch (err) {
            console.warn('Could not persist sprite-mode-off setting', err);
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
