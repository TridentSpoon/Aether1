/**
 * Desktop Sprite window logic for Project AETHER1.
 * A small transparent, always-on-top, undecorated window (see src-tauri/src/main.rs's
 * build_sprite_window) showing just the hologram avatar, PNGTuber-style -- click it to chat,
 * drag it around the desktop, or reopen the full HUD.
 *
 * This window only ever exists inside the native Tauri app (a browser tab can't be
 * transparent/always-on-top/frameless), so unlike app.js there's no IS_TAURI branching here --
 * every call below goes straight through window.__TAURI__.core.invoke.
 */

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');
    const voiceEngine = new VoiceAudioEngine();
    voiceEngine.sfxEnabled = false; // the sprite stays quiet -- SFX is a main-HUD touch

    const viewportEl = document.getElementById('hologram-viewport');
    const bubbleEl = document.getElementById('sprite-bubble');
    const chatBar = document.getElementById('sprite-chat-bar');
    const chatInput = document.getElementById('sprite-chat-input');
    const btnHud = document.getElementById('sprite-btn-hud');
    const btnClose = document.getElementById('sprite-btn-close');

    let autoSpeak = true;
    let isWaitingForResponse = false;
    let bubbleTimer = null;

    async function tauriInvoke(cmd, args) {
        if (!window.__TAURI__ || !window.__TAURI__.core) {
            throw new Error('Tauri bridge unavailable');
        }
        return window.__TAURI__.core.invoke(cmd, args);
    }

    voiceEngine.onStateChange = (state) => hologram.setState(state);
    voiceEngine.onAudioFrequency = (data) => hologram.updateAudioData(data);

    function showBubble(text) {
        clearTimeout(bubbleTimer);
        bubbleEl.textContent = text;
        bubbleEl.classList.remove('hidden');
        bubbleTimer = setTimeout(() => bubbleEl.classList.add('hidden'), 9000);
    }

    function toggleChatBar() {
        const willShow = chatBar.classList.contains('hidden');
        chatBar.classList.toggle('hidden');
        if (willShow) chatInput.focus();
    }

    // Dragging: the window has no native title bar, so start an OS-level drag on mousedown
    // over the avatar (Tauri's usual pattern for custom-titlebar dragging -- see
    // start_window_drag_rust). If the user doesn't actually move the pointer, the browser
    // still fires a normal click afterward, which opens the chat bar instead.
    viewportEl.addEventListener('mousedown', (e) => {
        if (e.button !== 0) return;
        tauriInvoke('start_window_drag_rust').catch(() => {});
    });
    viewportEl.addEventListener('click', toggleChatBar);

    async function synthesizeSpeechUrl(text) {
        try {
            const path = await tauriInvoke('generate_speech_rust', { text, voice: null });
            return window.__TAURI__.core.convertFileSrc(path);
        } catch (e) {
            console.warn('Sprite TTS synthesis failed', e);
            return null;
        }
    }

    async function sendMessage() {
        const text = chatInput.value.trim();
        if (!text || isWaitingForResponse) return;
        chatInput.value = '';

        isWaitingForResponse = true;
        hologram.setState('THINKING');

        try {
            const data = await tauriInvoke('generate_response_rust', { prompt: text, sessionId: 'default' });
            const audioUrl = autoSpeak ? await synthesizeSpeechUrl(data.reply) : null;
            showBubble(data.reply);

            if (audioUrl) {
                await voiceEngine.playTTSAudio(audioUrl);
            } else {
                hologram.setState('IDLE');
            }
        } catch (e) {
            console.error('Sprite chat error', e);
            showBubble(`⚠️ ${e.message || e}`);
            hologram.setState('IDLE');
        } finally {
            isWaitingForResponse = false;
        }
    }

    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            sendMessage();
        } else if (e.key === 'Escape') {
            chatBar.classList.add('hidden');
        }
    });

    btnHud.addEventListener('click', (e) => {
        e.stopPropagation();
        tauriInvoke('show_main_window_rust').catch((err) => console.warn('Could not open main HUD', err));
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
    // aether_avatar/aether_color_theme are plain localStorage keys the main window already
    // writes (see app.js), shared here because both windows load from the same Tauri origin.
    const savedAvatar = localStorage.getItem('aether_avatar') || 'halcy';
    const savedTheme = localStorage.getItem('aether_color_theme') || 'halcy';
    hologram.setAvatar(savedAvatar);
    hologram.setColorTheme(savedTheme);
    document.documentElement.setAttribute('data-theme', savedTheme);

    tauriInvoke('get_settings_rust').then((data) => {
        autoSpeak = data.settings.auto_speak !== false;
    }).catch((e) => console.warn('Could not load settings for sprite', e));
});
