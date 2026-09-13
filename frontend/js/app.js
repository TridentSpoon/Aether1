/**
 * Main Frontend Application Logic for Project AETHER1.
 * Handles independently-selectable 3D Avatars (A1 monogram placeholder, R.E.D. 9000, The Nexus,
 * A.R.X.LIMES, hAlcy, A.R.X.LOGOS, A1ter_nul) and Color Themes, Token Telemetry Graph, Agent
 * Genesis, and Model Scanner.
 */

const AVATAR_DISPLAY_NAMES = {
    halcy: 'HALCY',
    nexus: 'THE NEXUS',
    matrix: 'THE NEXUS',
    'arx-limes': 'A.R.X.LIMES',
    'arx-logos': 'A.R.X.LOGOS',
    red: 'R.E.D. 9000',
    crimson: 'R.E.D. 9000',
    senti: 'NEXUS SENT'
};

/* Trace Protocols: Nexus Sent, White Rabbit and Operator are hidden easter eggs that surface
   on The Nexus, discovered by what you type rather than picked from a list up front (see
   js/avatar-unlocks.js for the "seen once, selectable forever" half of this). Simple client-side
   keyword matching on the outgoing message -- no LLM/backend signal -- checked in this order,
   first match wins. Deliberately a flat, ordered list rather than a rules engine: three eggs is
   not a case for one. */
const AVATAR_TRIGGER_RULES = [
    { avatarId: 'operator', pattern: /\b(hi|hello|hey)\b/i },
    { avatarId: 'white-rabbit', pattern: /(rabbit hole|wonderland|curiouser|down the rabbit)/i },
    { avatarId: 'senti', pattern: /(is this safe|vulnerabilit|exploit|diagnostic|debug|check the logs|system status)/i }
];

function matchAvatarTrigger(text) {
    const rule = AVATAR_TRIGGER_RULES.find((r) => r.pattern.test(text));
    return rule ? rule.avatarId : null;
}

// When this page is loaded by the Tauri desktop shell, it's served from Tauri's own
// local context (not http://localhost:8378), so API calls need an absolute base URL
// pointing at the backend the Rust shell launches. Under the plain browser/FastAPI
// flow, relative paths keep working exactly as before.
const IS_TAURI = typeof window.__TAURI_INTERNALS__ !== 'undefined';
const API_BASE = IS_TAURI ? 'http://localhost:8378' : '';

function apiFetch(path, options) {
    return fetch(API_BASE + path, options);
}

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');
    const voiceEngine = new VoiceAudioEngine();

    // DOM Elements
    const chatContainer = document.getElementById('chat-messages');
    const chatInput = document.getElementById('chat-input');
    const btnSend = document.getElementById('btn-send');
    const btnMic = document.getElementById('btn-mic');
    const btnSettings = document.getElementById('btn-settings');
    const btnClearChat = document.getElementById('btn-clear-chat');
    const btnSfxToggle = document.getElementById('btn-sfx');
    const settingsModal = document.getElementById('settings-modal');
    const btnCloseSettings = document.getElementById('btn-close-settings');
    const btnSaveSettings = document.getElementById('btn-save-settings');

    // Settings modal tabs -- Customisation (avatar/theme/persona/sprite) vs. Agent & System
    // (connection, voice, memory, keys). Both panels always stay in the DOM; this only ever
    // toggles which one is visible, so nothing that reads/writes settings fields needs to care.
    const settingsTabButtons = document.querySelectorAll('.settings-tab-btn');
    const settingsTabPanels = {
        customisation: document.getElementById('settings-panel-customisation'),
        system: document.getElementById('settings-panel-system')
    };
    function showSettingsTab(tabName) {
        settingsTabButtons.forEach(btn => {
            btn.classList.toggle('cyber-btn-active', btn.getAttribute('data-settings-tab') === tabName);
        });
        Object.entries(settingsTabPanels).forEach(([name, panel]) => {
            if (panel) panel.classList.toggle('hidden', name !== tabName);
        });
    }
    settingsTabButtons.forEach(btn => {
        btn.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            showSettingsTab(btn.getAttribute('data-settings-tab'));
        });
    });

    // Agent Genesis & Theme Elements
    const btnForgeIdentity = document.getElementById('btn-forge-identity');
    const genesisPurposeInput = document.getElementById('genesis-purpose-input');
    const hudAgentName = document.getElementById('hud-agent-name');
    const terminalAgentLabel = document.getElementById('terminal-agent-label');
    const avatarStructureLabel = document.getElementById('avatar-structure-label');
    const themeChipValue = document.getElementById('hud-theme-name');
    const themeModeNote = document.getElementById('theme-mode-note');
    const themeColourInputs = {
        background: document.getElementById('theme-colour-background'),
        main: document.getElementById('theme-colour-main'),
        highlight: document.getElementById('theme-colour-highlight')
    };
    const themeToneInputs = {
        saturation: document.getElementById('theme-saturation'),
        depth: document.getElementById('theme-depth')
    };
    const themeToneValues = {
        saturation: document.getElementById('theme-saturation-value'),
        depth: document.getElementById('theme-depth-value')
    };

    // Model Scanner Elements
    const btnScanSystem = document.getElementById('btn-scan-system');
    const btnPullLlama = document.getElementById('btn-pull-llama');
    const selectLocalModel = document.getElementById('select-local-model');
    const scannerResultsBox = document.getElementById('scanner-results-box');

    // LLM Test Connection Elements
    const btnTestConnection = document.getElementById('btn-test-connection');
    const testConnectionStatus = document.getElementById('test-connection-status');

    // Version & Update Elements (native desktop app only -- see IS_TAURI below)
    const versionBadge = document.getElementById('version-badge');
    const updateSection = document.getElementById('update-section');
    const settingsVersionLabel = document.getElementById('settings-version-label');
    const updateStatusBox = document.getElementById('update-status-box');
    const btnCheckUpdate = document.getElementById('btn-check-update');
    const btnInstallGh = document.getElementById('btn-install-gh');
    const btnApplyUpdate = document.getElementById('btn-apply-update');

    // Hardware Telemetry Elements
    const elCpuGauge = document.getElementById('cpu-gauge-fill');
    const elCpuVal = document.getElementById('cpu-percent-val');
    const elRamGauge = document.getElementById('ram-gauge-fill');
    const elRamVal = document.getElementById('ram-percent-val');
    const elDiskGauge = document.getElementById('disk-gauge-fill');
    const elDiskVal = document.getElementById('disk-percent-val');
    const elNetDown = document.getElementById('net-download-val');
    const elNetUp = document.getElementById('net-upload-val');
    const elBatterySection = document.getElementById('battery-section');
    const elBatteryGauge = document.getElementById('battery-gauge-fill');
    const elBatteryVal = document.getElementById('battery-percent-val');
    const elBatteryWarning = document.getElementById('battery-warning');
    const elDistroBadge = document.getElementById('distro-badge');
    const elStatusBadge = document.getElementById('status-badge');
    const elClock = document.getElementById('live-clock');

    // Token Telemetry Elements
    const elTps = document.getElementById('tps-val');
    const elSessionTokens = document.getElementById('session-tokens-val');
    const elTokensGauge = document.getElementById('tokens-gauge-fill');
    const elTokensUsedPct = document.getElementById('tokens-used-pct');
    const elTokensAvail = document.getElementById('tokens-avail-val');
    const tokensCanvas = document.getElementById('tokens-graph-canvas');
    const tokensCanvasCtx = tokensCanvas ? tokensCanvas.getContext('2d') : null;

    // Audio Waveform Canvas
    const canvas = document.getElementById('audio-waveform');
    const canvasCtx = canvas ? canvas.getContext('2d') : null;

    let isWaitingForResponse = false;
    let autoSpeak = true;
    let currentAgentName = "HALCY";
    let currentAvatar = localStorage.getItem('aether_avatar') || 'a1';
    let currentZoom = parseFloat(localStorage.getItem('aether_avatar_zoom')) || 1;
    let currentTheme = Aether1Theme.current();

    /* The wordmark. Once the companion has a name of its own, that is what the top-left of
       the window should say -- it is the thing you are talking to. Turning over to
       AETHER1 PLATFORM every so often is how the product it runs on stays visible without
       taking a permanent second line to say so. A name that already is Aether1 has nothing
       to alternate with, so it just sits there. */
    const PLATFORM_WORDMARK = 'AETHER1 PLATFORM';
    const WORDMARK_TURNOVER_MS = 14000;
    const elWordmark = document.getElementById('hud-wordmark');
    let wordmarkShowingPlatform = false;

    function wordmarkName() {
        const name = (currentAgentName || '').trim().toUpperCase();
        return name && name !== 'AETHER1' ? name : PLATFORM_WORDMARK;
    }

    function refreshWordmark() {
        if (!elWordmark) return;
        const name = wordmarkName();
        const next = wordmarkShowingPlatform && name !== PLATFORM_WORDMARK ? PLATFORM_WORDMARK : name;
        if (elWordmark.textContent === next) return;
        elWordmark.textContent = next;
        elWordmark.title = name === PLATFORM_WORDMARK ? 'Aether1' : `${name} — running on Aether1`;
    }

    setInterval(() => {
        wordmarkShowingPlatform = !wordmarkShowingPlatform;
        refreshWordmark();
    }, WORDMARK_TURNOVER_MS);

    /* The bar's two slide-outs. Only one is ever open, and anything that is not a deliberate
       interaction with the open one closes it -- a click elsewhere, Escape, or opening the
       other. Without that a menu left open sits over the HUD until it happens to be clicked
       again. */
    const hudMenus = [
        { button: document.getElementById('btn-avatar-menu'), panel: document.getElementById('avatar-menu') },
        { button: document.getElementById('btn-theme-menu'), panel: document.getElementById('theme-menu') },
        { button: document.getElementById('btn-hud-menu'), panel: document.getElementById('hud-menu') }
    ].filter((m) => m.button && m.panel);

    function closeHudMenus(except) {
        hudMenus.forEach(({ button, panel }) => {
            if (panel === except) return;
            panel.classList.add('hidden');
            button.setAttribute('aria-expanded', 'false');
        });
    }

    hudMenus.forEach(({ button, panel }) => {
        button.addEventListener('click', (e) => {
            e.stopPropagation();
            const opening = panel.classList.contains('hidden');
            closeHudMenus();
            panel.classList.toggle('hidden', !opening);
            button.setAttribute('aria-expanded', String(opening));
            if (opening) voiceEngine.playSFX('click');
        });
        /* Choosing something is the end of choosing, so the panel closes -- except for the
           SFX toggle, which is the one item you might want to hear the effect of and flip
           straight back. */
        panel.addEventListener('click', (e) => {
            if (e.target.closest('#btn-sfx')) return;
            if (e.target.closest('button')) closeHudMenus();
        });
    });

    document.addEventListener('click', () => closeHudMenus());
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') closeHudMenus();
    });

    // Clock
    function updateClock() {
        const now = new Date();
        if (elClock) {
            elClock.textContent = now.toLocaleTimeString() + " // " + now.toISOString().split('T')[0];
        }
    }
    setInterval(updateClock, 1000);
    updateClock();

    /* Shows a shape on the hologram and mirrors it to the desktop sprite window, if open --
       nothing else. A Trace Protocols trigger's transient flash (see flashTraceProtocolAvatar
       below) needs exactly this and no more: it is not a change of selection, so it must not
       touch localStorage, currentAvatar, or anything that reads as "this is now picked" (the
       HUD chip, the pill highlight, the persona/greeting). applyAvatar -- a real, persisted
       pick -- builds on top of this for the parts a flash must skip. */
    function setHologramAvatar(avatarName) {
        hologram.setAvatar(avatarName);
        // Push the change straight to the desktop sprite window (if open) instead of making
        // it discover this by polling localStorage -- see sprite.js's 'avatar-changed' listener.
        if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.emit('avatar-changed', { avatar: avatarName }).catch(() => {});
        }
    }

    // Avatar Engine Handler (3D shape + optional linked persona identity)
    function applyAvatar(avatarName, updatePersona = false) {
        currentAvatar = avatarName;
        localStorage.setItem('aether_avatar', avatarName);
        setHologramAvatar(avatarName);
        updateAvatarBadge(avatarName);

        if (avatarStructureLabel) {
            if (avatarName === 'red' || avatarName === 'crimson') avatarStructureLabel.textContent = 'OPTICAL EYE // DUAL ORBITS';
            else if (avatarName === 'arx-limes') avatarStructureLabel.textContent = 'ARCHIVAL VOXEL MATRIX';
            else if (avatarName === 'nexus' || avatarName === 'matrix') avatarStructureLabel.textContent = 'SINGULARITY VORTEX';
            else if (avatarName === 'arx-logos') avatarStructureLabel.textContent = 'JAGGED GEOMETRIC STAR';
            else if (avatarName === 'alt' || avatarName === 'cunningham' || avatarName === 'a1ter_nul') avatarStructureLabel.textContent = 'CHROMATIC-GLITCH GHOST BUST';
            else if (avatarName === 'a1') avatarStructureLabel.textContent = 'MONOGRAM WORDMARK';
            // A registered avatar names itself, rather than borrowing hAlcy's label from
            // the fallback below -- see js/hologram/README.md.
            else if (window.HologramAvatar && HologramAvatar.avatarPlugins.has(avatarName)) {
                const def = HologramAvatar.avatarPlugins.get(avatarName);
                avatarStructureLabel.textContent = (def.label || avatarName).toUpperCase();
            }
            else avatarStructureLabel.textContent = 'HARMONIC LATTICE';
        }

        // The Customise button belongs to the custom avatar and nothing else -- it would
        // be a lie next to hAlcy, whose shape is fixed in code.
        const customiseBtn = document.getElementById('btn-customise-avatar');
        if (customiseBtn) customiseBtn.classList.toggle('hidden', avatarName !== 'custom');

        // Selecting the custom avatar is the moment to check whether the design saved in
        // the workbench has moved on from the one currently on screen.
        if (avatarName === 'custom') refreshCustomAvatarIfStale();

        // Highlight active avatar pills/buttons
        document.querySelectorAll('.avatar-pill, .avatar-btn').forEach(btn => {
            const val = btn.getAttribute('data-avatar-val') || btn.getAttribute('data-avatar');
            if (val === avatarName) {
                btn.classList.add('cyber-btn-active');
            } else {
                btn.classList.remove('cyber-btn-active');
            }
        });

        applyAvatarPreset(avatarName, updatePersona);
    }

    /* Each avatar comes with a persona, a voice and a greeting -- picking the face picks the
       job it is there to do. This was a seven-branch if/else covering five of the eight
       avatars, so A1 and hAlcy quietly kept whatever persona was already set and the pairing
       only half existed.

       "Your own" is the deliberate omission: an avatar you designed has no persona of its own
       to switch to, and defaulting it to Custom would hand you the fallback directive without
       saying so. It keeps whatever you had. */
    const AVATAR_PRESETS = {
        a1: {
            name: 'AETHER1', persona: 'default', voice: 'en-GB-SoniaNeural',
            greeting: '🅰️ **Aether1 online.** I read this machine -- logs, services, load, what changed. Ask me what is wrong with it.'
        },
        halcy: {
            name: 'HALCY', persona: 'halcy', voice: 'en-US-JennyNeural',
            greeting: '🔷 **hAlcy here.** Think out loud at me -- I would rather work it through with you than guess at what you meant.'
        },
        red: {
            name: 'R.E.D. 9000', persona: 'red9000', voice: 'en-US-GuyNeural',
            greeting: '🔴 **R.E.D. 9000 (Reactive Engine Daemon).** Fully operational. Ask, and you will have the answer in the first line.'
        },
        nexus: {
            name: 'THE NEXUS', persona: 'nexus', voice: 'en-GB-SoniaNeural',
            greeting: '🟢 **The Nexus is active.** Code cascades inward. Bring me something to write, read or break.'
        },
        'arx-limes': {
            name: 'A.R.X.LIMES', persona: 'arx-limes', voice: 'en-US-GuyNeural',
            greeting: '🔶 **Archival, Reasoning, matriX — Limes Node engaged.** Every claim I make will carry where it came from. The Archive demands nothing less.'
        },
        'arx-logos': {
            name: 'A.R.X.LOGOS', persona: 'arx-logos', voice: 'en-GB-LibbyNeural',
            greeting: '🟣 **Archival, Reasoning, matriX — Logos Node engaged.** Every archive needs a curator with taste. Let us make something worth cataloguing.'
        },
        alt: {
            name: 'A1ter_nul', persona: 'alt', voice: 'en-US-JennyNeural',
            greeting: "⚠️ **A1ter_nul online.** Firewall's up, perimeter's lit. Show me what you are worried got in."
        }
    };

    // Names the engine still answers to from older saved settings.
    const AVATAR_ALIASES = { crimson: 'red', matrix: 'nexus', cunningham: 'alt', a1ter_nul: 'alt' };

    function applyAvatarPreset(avatarName, updatePersona) {
        const preset = AVATAR_PRESETS[AVATAR_ALIASES[avatarName] || avatarName];
        if (!preset) return;

        updateAgentNameDisplay(preset.name);
        if (!updatePersona) return;

        document.getElementById('setting-persona').value = preset.persona;
        document.getElementById('setting-voice').value = preset.voice;
        toggleCustomPersonaField();
        saveSettings(false);
        appendMessage(preset.name, preset.greeting);
    }

    /* Trace Protocols: Nexus Sent, White Rabbit and Operator start absent from both avatar
       pickers -- the HUD's avatar-menu slideout and the matching row in Settings -- until their
       trigger phrase (see AVATAR_TRIGGER_RULES above) fires once with The Nexus active. Once
       unlocked (see js/avatar-unlocks.js) an egg joins its row for good, so this only ever
       reveals a button, never hides one back. */
    const TRACE_PROTOCOL_AVATAR_IDS = ['senti', 'white-rabbit', 'operator'];
    function refreshTraceProtocolVisibility() {
        TRACE_PROTOCOL_AVATAR_IDS.forEach((id) => {
            const unlocked = Aether1AvatarUnlocks.isUnlocked(id);
            document.querySelectorAll(`.avatar-pill[data-avatar-val="${id}"], .avatar-btn[data-avatar="${id}"]`)
                .forEach((btn) => btn.classList.toggle('hidden', !unlocked));
        });
    }

    // A few seconds to tens of seconds, matching "pops up" -- long enough to actually notice
    // and register what it is, short enough to still read as a flash rather than a switch.
    const TRACE_PROTOCOL_FLASH_MS = 12000;
    let traceProtocolFlashTimer = null;

    /* The transient swap a Trace Protocols trigger pops up: shows the egg on top of whatever
       avatar is actually selected via setHologramAvatar (never applyAvatar, which would persist
       it as the pick) and reverts after TRACE_PROTOCOL_FLASH_MS. Reverts to currentAvatar read
       fresh at that moment rather than a value captured now, so a manual avatar change made
       while the flash is showing is not clobbered when it ends. */
    function flashTraceProtocolAvatar(avatarId) {
        if (Aether1AvatarUnlocks.unlock(avatarId)) refreshTraceProtocolVisibility();

        if (traceProtocolFlashTimer) clearTimeout(traceProtocolFlashTimer);
        setHologramAvatar(avatarId);
        traceProtocolFlashTimer = setTimeout(() => {
            traceProtocolFlashTimer = null;
            setHologramAvatar(currentAvatar);
        }, TRACE_PROTOCOL_FLASH_MS);
    }

    refreshTraceProtocolVisibility();

    /* Painting a theme. Purely cosmetic and independent of the avatar shape, which can wear
       any of them. Aether1Theme owns what the theme *is* -- the mode, the three colours, what
       is saved and when -- and everything here is the consequences of it: the page, the 3D
       avatar, the sprite window and the controls that have to agree with what is on screen. */
    function paintTheme(theme) {
        currentTheme = theme;
        Aether1Theme.paint(document, theme.mode, theme.colours);
        hologram.setColorPalette(Aether1Theme.paletteFor(theme.colours));

        // Push the change straight to the desktop sprite window (if open) instead of making it
        // discover this by polling localStorage -- see sprite.js's 'color-theme-changed' listener.
        if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.emit('color-theme-changed', theme).catch(() => {});
        }
        syncThemeControls(theme);
    }

    function syncThemeControls(theme) {
        if (themeChipValue) themeChipValue.textContent = Aether1Theme.MODE_LABELS[theme.mode] || theme.mode;

        // Covers both the three buttons in Settings and the three in the top bar's slide-out.
        document.querySelectorAll('.theme-mode-btn').forEach(btn => {
            btn.classList.toggle('cyber-btn-active', btn.getAttribute('data-theme-mode') === theme.mode);
        });

        /* A preset button lights up only while the colours still match it exactly. Nudge one
           picker and nothing is selected, which is the honest state: what is on screen is no
           longer any of the presets. */
        document.querySelectorAll('.color-theme-pill, .color-theme-btn').forEach(btn => {
            const val = btn.getAttribute('data-color-theme-val') || btn.getAttribute('data-color-theme');
            btn.classList.toggle('cyber-btn-active', val === theme.colours.preset);
        });

        Object.keys(themeColourInputs).forEach(slot => {
            const input = themeColourInputs[slot];
            if (input && input.value.toLowerCase() !== theme.colours[slot]) input.value = theme.colours[slot];
        });

        /* The tone sliders are read back through toneOf rather than straight off the stored
           object, so a mode that has never been touched shows its defaults instead of an
           empty slider parked at whatever the minimum happens to be. */
        const tone = Aether1Theme.toneOf(theme.colours);
        Object.keys(themeToneInputs).forEach(slot => {
            const input = themeToneInputs[slot];
            if (input && Number(input.value) !== tone[slot]) input.value = String(tone[slot]);
            const label = themeToneValues[slot];
            if (label) {
                label.textContent = slot === 'saturation'
                    ? `${tone[slot]}%`
                    : (tone[slot] > 0 ? `+${tone[slot]}` : String(tone[slot]));
            }
        });

        const themeToneNote = document.getElementById('theme-tone-note');
        if (themeToneNote) {
            const touched = tone.saturation !== Aether1Theme.SATURATION_DEFAULT || tone.depth !== 0;
            themeToneNote.textContent = touched
                ? 'The swatches show the colours as picked. The sliders adjust how they are painted, so those two will not match until you reset the tone.'
                : '';
        }

        if (themeModeNote) {
            themeModeNote.textContent = Aether1Theme.followingSystem()
                ? 'Following your system\u2019s light/dark setting. Picking a mode stops that.'
                : '';
        }
    }

    /* #rrggbb -> rgba(). Canvas has no notion of a colour with an alpha applied, and the
       theme's three colours are opaque hex by design. */
    function withAlpha(hex, alpha) {
        const n = parseInt(hex.slice(1), 16);
        return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
    }

    // Audio Waveform Visualizer
    function drawWaveform(freqData) {
        if (!canvasCtx || !canvas) return;
        const width = canvas.width;
        const height = canvas.height;
        canvasCtx.clearRect(0, 0, width, height);

        let strokeColor = '#00f0ff';
        strokeColor = currentTheme.colours.main;

        canvasCtx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
        canvasCtx.lineWidth = 1;
        canvasCtx.beginPath();
        canvasCtx.moveTo(0, height / 2);
        canvasCtx.lineTo(width, height / 2);
        canvasCtx.stroke();

        const barWidth = (width / freqData.length) * 1.5;
        let x = 0;

        canvasCtx.beginPath();
        canvasCtx.strokeStyle = strokeColor;
        canvasCtx.lineWidth = 2;
        canvasCtx.shadowBlur = 8;
        canvasCtx.shadowColor = strokeColor;

        for (let i = 0; i < freqData.length; i++) {
            const v = freqData[i] / 255.0;
            const y = (height / 2) + (v * (height / 2) * (i % 2 === 0 ? 1 : -1));

            if (i === 0) {
                canvasCtx.moveTo(x, y);
            } else {
                canvasCtx.lineTo(x, y);
            }
            x += barWidth;
        }
        canvasCtx.stroke();
        canvasCtx.shadowBlur = 0;
    }

    // Token Telemetry Sparkline Drawer
    function drawTokenGraph(sparkline) {
        if (!tokensCanvasCtx || !tokensCanvas) return;
        const w = tokensCanvas.width;
        const h = tokensCanvas.height;
        tokensCanvasCtx.clearRect(0, 0, w, h);

        if (!sparkline || sparkline.length === 0) {
            sparkline = [0, 0, 0, 0, 0];
        }

        tokensCanvasCtx.strokeStyle = 'rgba(255, 255, 255, 0.15)';
        tokensCanvasCtx.lineWidth = 1;
        tokensCanvasCtx.beginPath();
        tokensCanvasCtx.moveTo(0, h - 1);
        tokensCanvasCtx.lineTo(w, h - 1);
        tokensCanvasCtx.stroke();

        const maxVal = Math.max(...sparkline, 100);
        const step = w / Math.max(sparkline.length - 1, 1);

        /* The token graph is drawn on a canvas, so it cannot inherit a CSS variable -- it has
           to be told. It follows the highlight rather than the main colour so the graph stays
           distinguishable from the gauges above it, which are all main. */
        const lineColor = currentTheme.colours.highlight;
        const fillColor = withAlpha(lineColor, 0.15);

        tokensCanvasCtx.beginPath();
        tokensCanvasCtx.moveTo(0, h);
        sparkline.forEach((val, idx) => {
            const x = idx * step;
            const y = h - ((val / maxVal) * (h - 8)) - 4;
            tokensCanvasCtx.lineTo(x, y);
        });
        tokensCanvasCtx.lineTo(w, h);
        tokensCanvasCtx.fillStyle = fillColor;
        tokensCanvasCtx.fill();

        tokensCanvasCtx.beginPath();
        tokensCanvasCtx.strokeStyle = lineColor;
        tokensCanvasCtx.lineWidth = 2;
        tokensCanvasCtx.shadowBlur = 6;
        tokensCanvasCtx.shadowColor = lineColor;

        sparkline.forEach((val, idx) => {
            const x = idx * step;
            const y = h - ((val / maxVal) * (h - 8)) - 4;
            if (idx === 0) tokensCanvasCtx.moveTo(x, y);
            else tokensCanvasCtx.lineTo(x, y);
        });
        tokensCanvasCtx.stroke();
        tokensCanvasCtx.shadowBlur = 0;

        tokensCanvasCtx.fillStyle = '#ffffff';
        sparkline.forEach((val, idx) => {
            const x = idx * step;
            const y = h - ((val / maxVal) * (h - 8)) - 4;
            tokensCanvasCtx.fillRect(x - 1.5, y - 1.5, 3, 3);
        });
    }

    // Voice Callbacks
    voiceEngine.onStateChange = (state) => {
        hologram.setState(state);
        // THINKING and SPEAKING are the states where something is actually answering, and
        // the panel should be showing whichever half of itself describes what is doing it.
        setTelemetryBusy(state === 'THINKING' || state === 'SPEAKING');
        if (elStatusBadge) {
            elStatusBadge.textContent = state;
            if (state === 'LISTENING') {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-green-900/60 text-green-400 border border-green-500/50 pulse-badge';
                btnMic.classList.add('cyber-btn-active');
            } else if (state === 'THINKING') {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-purple-900/60 text-purple-400 border border-purple-500/50 pulse-badge';
                btnMic.classList.remove('cyber-btn-active');
            } else if (state === 'SPEAKING') {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-cyan-900/60 text-cyan-400 border border-cyan-500/50 pulse-badge';
                btnMic.classList.remove('cyber-btn-active');
            } else {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-cyan-950/40 text-cyan-400 border border-cyan-500/30';
                btnMic.classList.remove('cyber-btn-active');
            }
        }
    };

    voiceEngine.onAudioFrequency = (data) => {
        hologram.updateAudioData(data);
        drawWaveform(data);
    };

    // Live Telemetry -- a Tauri event in the native app (see the background thread in
    // src-tauri/src/main.rs that emits "telemetry-update"), a WebSocket to the Python
    // backend in the browser flow. Same payload shape either way, so one handler covers both.
    function handleTelemetryPayload(data) {
        if (data.telemetry) updateHardwareTelemetry(data.telemetry);
        if (data.tokens) updateTokenTelemetry(data.tokens);
        if (data.agent_name && data.agent_name !== currentAgentName) {
            updateAgentNameDisplay(data.agent_name);
        }
    }

    function connectTelemetry() {
        if (IS_TAURI) {
            if (!window.__TAURI__ || !window.__TAURI__.event) {
                console.error('Tauri event bridge unavailable; live telemetry will not update.');
                return;
            }
            window.__TAURI__.event.listen('telemetry-update', (event) => {
                handleTelemetryPayload(event.payload);
            });
            return;
        }

        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws/telemetry`;
        const ws = new WebSocket(wsUrl);

        ws.onmessage = (event) => {
            try {
                handleTelemetryPayload(JSON.parse(event.data));
            } catch (e) {
                console.error("Telemetry parse error", e);
            }
        };

        ws.onclose = () => {
            setTimeout(connectTelemetry, 3000);
        };
    }

    function updateHardwareTelemetry(data) {
        if (!data) return;
        const cpuPct = data.cpu ? data.cpu.total_percent : 0;
        if (elCpuVal) elCpuVal.textContent = `${cpuPct}%`;
        if (elCpuGauge) {
            elCpuGauge.style.width = `${cpuPct}%`;
            elCpuGauge.className = `gauge-bar-fill ${cpuPct > 85 ? 'crit' : (cpuPct > 65 ? 'warn' : '')}`;
        }

        const ramPct = data.ram ? data.ram.percent : 0;
        if (elRamVal) elRamVal.textContent = `${ramPct}% (${data.ram.used_gb}/${data.ram.total_gb} GB)`;
        if (elRamGauge) {
            elRamGauge.style.width = `${ramPct}%`;
            elRamGauge.className = `gauge-bar-fill ${ramPct > 85 ? 'crit' : (ramPct > 65 ? 'warn' : '')}`;
        }

        const diskPct = data.disk ? data.disk.percent : 0;
        if (elDiskVal) elDiskVal.textContent = `${diskPct}% (${data.disk.used_gb}/${data.disk.total_gb} GB)`;
        if (elDiskGauge) elDiskGauge.style.width = `${diskPct}%`;

        if (elNetDown) elNetDown.textContent = `${data.network ? data.network.download_kbps : 0} KB/s`;
        if (elNetUp) elNetUp.textContent = `${data.network ? data.network.upload_kbps : 0} KB/s`;

        // data.battery is null on a desktop (see Telemetry::to_wire_json) -- the section
        // stays hidden for the life of the app in that case rather than showing a
        // permanent, meaningless 0%. On a laptop it appears the first reading in and stays
        // shown, since a battery doesn't unplug itself from the machine mid-session.
        if (data.battery) {
            if (elBatterySection) elBatterySection.classList.remove('hidden');
            const pct = Math.round(data.battery.percent);
            if (elBatteryVal) elBatteryVal.textContent = `${pct}% (${data.battery.state})`;
            if (elBatteryGauge) {
                elBatteryGauge.style.width = `${pct}%`;
                elBatteryGauge.className = `gauge-bar-fill ${data.battery.on_battery && pct < 25 ? 'crit' : (data.battery.on_battery ? 'warn' : '')}`;
            }
            // on_battery (state === Discharging) rather than state !== 'charging': "full"
            // and "unknown" are both still plugged in, and a warning that never clears on
            // a battery that reports "unknown" while on AC would just be noise.
            if (elBatteryWarning) elBatteryWarning.classList.toggle('hidden', !data.battery.on_battery);
        }
    }

    /* The telemetry panel's two views.
     *
     * A cloud model and a local one raise different questions. Against an API the question
     * is what this session has spent; on your own hardware nothing is being spent and the
     * question is how fast it runs and how much it can hold. So there are two views, and
     * the panel picks between them rather than showing one set of numbers that is only ever
     * half relevant.
     *
     * Idle, it alternates -- but only once both views have something to say, since cycling
     * to a view that reads "--" is worse than not cycling. While a reply is being generated
     * it pins to whichever view matches what is answering. A click pins it too, for long
     * enough to read, and then the cycling resumes. */
    const TELEMETRY_VIEWS = ['usage', 'capacity'];
    const TELEMETRY_CYCLE_MS = 9000;
    const TELEMETRY_CLICK_HOLD_MS = 120000;
    let telemetryView = 'usage';
    let telemetryHeldUntil = 0;
    let telemetryPinned = false;
    /* Pinning depends on a THINKING being followed by an IDLE, and there are paths where an
       error could swallow the second half. A pin that never released would quietly stop the
       panel cycling forever, so it expires on its own too. */
    const TELEMETRY_PIN_MAX_MS = 180000;
    let telemetryPinnedAt = 0;
    let lastTokens = null;

    function showTelemetryView(view, pinned) {
        telemetryView = view;
        telemetryPinned = !!pinned;
        document.querySelectorAll('[data-telemetry-view]').forEach(panel => {
            panel.classList.toggle('hidden', panel.getAttribute('data-telemetry-view') !== view);
        });
        document.querySelectorAll('.telemetry-tab').forEach(tab => {
            const mine = tab.getAttribute('data-telemetry-tab') === view;
            tab.setAttribute('aria-pressed', mine ? 'true' : 'false');
            if (mine && telemetryPinned) tab.setAttribute('data-pinned', 'true');
            else tab.removeAttribute('data-pinned');
        });
    }

    /* Whether a view has anything worth showing. Usage needs a request to have happened;
       capacity needs something local to have actually answered. */
    function telemetryHasData(view, tokens) {
        if (!tokens) return view === 'usage';
        if (view === 'usage') return tokens.requests > 0;
        return tokens.mode === 'local' && (!!tokens.capability || tokens.last_tps > 0);
    }

    function relevantTelemetryView(tokens) {
        return tokens && tokens.mode === 'local' ? 'capacity' : 'usage';
    }

    function cycleTelemetryView() {
        if (telemetryPinned && Date.now() - telemetryPinnedAt > TELEMETRY_PIN_MAX_MS) {
            telemetryPinned = false;
            showTelemetryView(telemetryView, false);
        }
        if (telemetryPinned || Date.now() < telemetryHeldUntil) return;
        const usable = TELEMETRY_VIEWS.filter(v => telemetryHasData(v, lastTokens));
        if (usable.length < 2) return;
        const next = usable[(usable.indexOf(telemetryView) + 1) % usable.length];
        showTelemetryView(next, false);
    }

    /* Called when a turn starts and ends. Generating pins the view to whatever is doing the
       work; finishing releases it back to the cycle. */
    function setTelemetryBusy(busy) {
        if (busy) {
            telemetryPinnedAt = Date.now();
            showTelemetryView(relevantTelemetryView(lastTokens), true);
        } else if (telemetryPinned) {
            telemetryPinned = false;
            showTelemetryView(telemetryView, false);
        }
    }

    function updateTokenTelemetry(tokens) {
        if (!tokens) return;
        lastTokens = tokens;

        if (elTps) elTps.textContent = `${tokens.last_tps || 0} TPS`;
        if (elSessionTokens) elSessionTokens.textContent = `${(tokens.total_session_tokens || 0).toLocaleString()}`;
        if (elTokensUsedPct) elTokensUsedPct.textContent = `${tokens.used_percent}%`;
        if (elTokensAvail) elTokensAvail.textContent = `${(tokens.available_tokens / 1000).toFixed(1)}k`;
        if (elTokensGauge) elTokensGauge.style.width = `${tokens.used_percent}%`;

        const elIn = document.getElementById('tokens-in-val');
        const elOut = document.getElementById('tokens-out-val');
        if (elIn) elIn.textContent = (tokens.prompt_tokens || 0).toLocaleString();
        if (elOut) elOut.textContent = (tokens.completion_tokens || 0).toLocaleString();

        /* "1,204 tokens" and "about 1,200 tokens" are different claims, and only one of them
           can be checked against a provider's own dashboard. The panel says which it is
           holding rather than letting a guess wear the confidence of a measurement. */
        const elAccounting = document.getElementById('tokens-accounting');
        if (elAccounting) {
            if (!tokens.requests) {
                elAccounting.textContent = '--';
                elAccounting.title = '';
            } else if (tokens.measured_requests >= tokens.requests) {
                elAccounting.textContent = 'counted';
                elAccounting.title = 'Every reply this session came back with real token counts from the provider.';
            } else if (tokens.measured_requests > 0) {
                elAccounting.textContent = `counted ${tokens.measured_requests}/${tokens.requests}`;
                elAccounting.title = 'Some replies reported real counts; the rest are estimated from the length of the text.';
            } else {
                elAccounting.textContent = 'estimated';
                elAccounting.title = 'This provider reports no token counts, so these are worked out from the length of the text -- roughly four characters per token.';
            }
        }

        updateCapacityView(tokens);
        drawTokenGraph(tokens.sparkline);

        // A view showing nothing useful should give way to the one that is.
        if (!telemetryPinned && Date.now() >= telemetryHeldUntil && !telemetryHasData(telemetryView, tokens)) {
            const fallback = TELEMETRY_VIEWS.find(v => telemetryHasData(v, tokens));
            if (fallback) showTelemetryView(fallback, false);
        }
    }

    function updateCapacityView(tokens) {
        const tps = document.getElementById('capacity-tps');
        const gauge = document.getElementById('capacity-gauge-fill');
        const context = document.getElementById('capacity-context');
        const size = document.getElementById('capacity-size');
        const note = document.getElementById('capacity-note');
        if (!tps) return;

        const capability = tokens.capability || null;
        tps.textContent = tokens.last_tps ? `${tokens.last_tps} tok/s` : '-- tok/s';

        /* The bar is throughput against 60 tok/s, which is roughly the point past which a
           reply arrives faster than it can be read. It is a reading-speed reference, not a
           limit, and it is deliberately not dressed up as a percentage of anything. */
        if (gauge) gauge.style.width = `${Math.min(100, ((tokens.last_tps || 0) / 60) * 100)}%`;

        if (context) {
            context.textContent = capability && capability.context_tokens
                ? `${Math.round(capability.context_tokens / 1024)}k`
                : '--';
        }
        if (size) {
            const parts = capability
                ? [capability.parameter_size, capability.quantization].filter(Boolean)
                : [];
            size.textContent = parts.length ? parts.join(' ') : '--';
        }

        if (note) {
            if (tokens.mode !== 'local') {
                note.textContent = tokens.mode === 'offline'
                    ? 'No model connected. Connect one in Settings.'
                    : `Answering from ${tokens.provider || 'the cloud'} — nothing is running on this machine.`;
            } else if (capability && capability.context_tokens) {
                const used = tokens.prompt_tokens || 0;
                const room = Math.max(0, capability.context_tokens - used);
                note.textContent = `${tokens.model || 'This model'} runs here. About ${room.toLocaleString()} tokens of context left before it starts forgetting the top of the conversation.`;
            } else {
                note.textContent = `${tokens.model || 'A local model'} is answering on this machine. It reports nothing about its own size or context.`;
            }
        }
    }

    // Updates the active LLM persona/identity (chat terminal label, sender names, settings
    // field). This is independent of the avatar badge, so Genesis-forged custom names
    // (e.g. SYNAPSE, VALKYRIE) always remain visible here regardless of the current avatar shape.
    function updateAgentNameDisplay(name) {
        currentAgentName = name;
        if (terminalAgentLabel) terminalAgentLabel.textContent = `AGENT: ${name.toUpperCase()}`;
        const inputName = document.getElementById('setting-agent-name');
        if (inputName) inputName.value = name;
        refreshWordmark();
    }

    // The avatar chip's value: the one avatar that is selected, which is all the top bar
    // shows of the eight until the slide-out is opened.
    function updateAvatarBadge(avatarName) {
        if (hudAgentName) hudAgentName.textContent = AVATAR_DISPLAY_NAMES[avatarName] || avatarName.toUpperCase();
    }

    async function loadStaticInfo() {
        try {
            let info;
            if (IS_TAURI) {
                info = await tauriInvoke('get_static_info_rust');
            } else {
                const resp = await apiFetch('/api/static-info');
                if (!resp.ok) return;
                info = await resp.json();
            }
            if (elDistroBadge) elDistroBadge.textContent = `${info.distro} [${info.architecture}]`;
        } catch (e) {
            console.warn("Could not load static info", e);
        }
    }

    function formatMarkdown(text) {
        if (!text) return '';
        let escaped = text
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;");

        escaped = escaped.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, '<pre class="bg-black/50 p-2.5 rounded border border-cyan-500/30 my-2 overflow-x-auto text-xs font-mono text-cyan-200"><code>$2</code></pre>');
        escaped = escaped.replace(/`([^`]+)`/g, '<code class="bg-cyan-950/80 px-1 py-0.5 rounded text-cyan-300 font-mono text-xs border border-cyan-500/20">$1</code>');
        escaped = escaped.replace(/\*\*([^*]+)\*\*/g, '<strong class="text-cyan-300 font-bold">$1</strong>');
        escaped = escaped.replace(/^### (.*$)/gim, '<h3 class="text-md font-bold text-cyan-400 mt-2 mb-1">$1</h3>');
        escaped = escaped.replace(/^## (.*$)/gim, '<h2 class="text-lg font-bold text-cyan-300 mt-2 mb-1">$1</h2>');
        escaped = escaped.replace(/\n/g, '<br/>');
        return escaped;
    }

    function appendMessage(sender, text, audioUrl = null) {
        const msgDiv = document.createElement('div');
        const isUser = sender === 'user';
        msgDiv.className = `p-3 rounded my-2 text-sm leading-relaxed ${isUser ? 'msg-user self-end ml-8' : 'msg-agent self-start mr-8'}`;

        const headerDiv = document.createElement('div');
        headerDiv.className = 'flex items-center justify-between mb-1 pb-1 border-b border-cyan-500/20 text-xs font-mono text-cyan-400/80';
        
        const senderSpan = document.createElement('span');
        const label = isUser ? '👤 <strong>OPERATOR</strong>' : `🌐 <strong>${currentAgentName.toUpperCase()}</strong>`;
        senderSpan.innerHTML = label;
        headerDiv.appendChild(senderSpan);

        const timeSpan = document.createElement('span');
        timeSpan.textContent = new Date().toLocaleTimeString();
        headerDiv.appendChild(timeSpan);

        const bodyDiv = document.createElement('div');
        bodyDiv.innerHTML = formatMarkdown(text);

        msgDiv.appendChild(headerDiv);
        msgDiv.appendChild(bodyDiv);

        if (audioUrl) {
            const playBtn = document.createElement('button');
            playBtn.className = 'mt-2 text-xs text-cyan-400 hover:text-cyan-200 flex items-center gap-1 font-mono cursor-pointer border border-cyan-500/30 px-2 py-0.5 rounded bg-cyan-950/40';
            playBtn.innerHTML = '▶ Replay Voice';
            playBtn.onclick = () => voiceEngine.playTTSAudio(audioUrl);
            msgDiv.appendChild(playBtn);
        }

        chatContainer.appendChild(msgDiv);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        msgDiv.bodyDiv = bodyDiv;
        return msgDiv;
    }

    // A streamed reply has no single audio file to replay: it was spoken sentence by
    // sentence as it arrived. Synthesize the whole thing on demand instead, the first time
    // the operator actually asks for it.
    function attachLazyReplay(msgDiv, text) {
        const playBtn = document.createElement('button');
        playBtn.className = 'mt-2 text-xs text-cyan-400 hover:text-cyan-200 flex items-center gap-1 font-mono cursor-pointer border border-cyan-500/30 px-2 py-0.5 rounded bg-cyan-950/40';
        playBtn.innerHTML = '▶ Replay Voice';
        let cachedUrl = null;
        playBtn.onclick = async () => {
            try {
                if (!cachedUrl) {
                    playBtn.innerHTML = '⋯ Synthesizing';
                    cachedUrl = await synthesizeSpeechUrl(text);
                }
                playBtn.innerHTML = '▶ Replay Voice';
                if (cachedUrl) await voiceEngine.playTTSAudio(cachedUrl);
            } catch (e) {
                playBtn.innerHTML = '⚠ Voice unavailable';
            }
        };
        msgDiv.appendChild(playBtn);
    }

    // --- Push to talk ---------------------------------------------------------------
    // Hold the key, talk, release: the recording is transcribed by a local model and the
    // text is sent as a message. Deliberately not "always listening" -- a microphone
    // permanently deciding whether you meant it is both less reliable and more alarming
    // than a key you are holding on purpose.

    const PUSH_TO_TALK_KEY = 'Space';
    let talkHeld = false;

    async function startTalking() {
        if (talkHeld || isWaitingForResponse) return;
        talkHeld = true;
        voiceEngine.stopSpeech(); // talking over the companion interrupts it
        hologram.setState('LISTENING');
        const started = await voiceEngine.startCapture();
        if (!started) {
            talkHeld = false;
            hologram.setState('IDLE');
            appendMessage(currentAgentName, '⚠️ No microphone available.');
        }
    }

    async function stopTalking() {
        if (!talkHeld) return;
        talkHeld = false;
        const wav = voiceEngine.stopCapture();
        if (!wav) return;

        try {
            const text = IS_TAURI
                ? await tauriInvoke('transcribe_rust', { wav: Array.from(new Uint8Array(await wav.arrayBuffer())) })
                : await (async () => {
                    const resp = await apiFetch('/api/stt', {
                        method: 'POST',
                        headers: { 'Content-Type': 'audio/wav' },
                        body: wav
                    });
                    if (!resp.ok) throw new Error(await resp.text());
                    return (await resp.json()).text;
                })();
            if (text && text.trim()) handleSendMessage(text.trim());
        } catch (e) {
            appendMessage(currentAgentName, `⚠️ Could not make that out: ${e.message || e}`);
        }
    }

    // Held anywhere except a text field, where space is a space.
    document.addEventListener('keydown', (event) => {
        const typing = ['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName);
        if (event.code === PUSH_TO_TALK_KEY && !typing && !event.repeat) {
            event.preventDefault();
            startTalking();
        }
    });
    document.addEventListener('keyup', (event) => {
        if (event.code === PUSH_TO_TALK_KEY && talkHeld) {
            event.preventDefault();
            stopTalking();
        }
    });
    // Losing focus mid-hold would otherwise leave the microphone open.
    window.addEventListener('blur', () => stopTalking());

    // --- Approval cards -----------------------------------------------------------
    // A mutating tool never runs from a conversation: it is proposed, and this is where
    // the operator answers. The card carries what will happen, in the tool's own words,
    // plus the option to stop being asked about that tool at all.

    async function toolsApi(path, options) {
        if (IS_TAURI) return null; // callers branch; this is the browser arm only
        const resp = await apiFetch(path, options);
        if (!resp.ok) throw new Error((await resp.text()) || `request failed: ${resp.status}`);
        return resp.status === 204 ? null : resp.json();
    }

    function renderApprovalCard(action) {
        const card = document.createElement('div');
        card.className = 'p-3 rounded my-2 text-sm msg-agent self-start mr-8 border border-amber-500/50 bg-amber-950/20';
        card.dataset.actionId = action.id;

        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 border-b border-amber-500/30 text-xs font-mono text-amber-300';
        // Two different questions wear this card. A mutating call asks "may this change
        // your machine?"; a read outside the persona's field asks "this is unusual for what
        // it is for -- may it look once?". Labelling both APPROVAL REQUIRED trains the
        // operator to read neither, so they are named apart.
        const isElevation = !action.mutating && !!action.reason;
        header.innerHTML = isElevation
            ? `<span>👁 <strong>OUTSIDE ITS FIELD</strong></span><span>${new Date().toLocaleTimeString()}</span>`
            : `<span>⚠ <strong>APPROVAL REQUIRED</strong></span><span>${new Date().toLocaleTimeString()}</span>`;
        card.appendChild(header);

        const body = document.createElement('div');
        body.className = 'text-cyan-100 font-mono text-xs my-2 break-all';
        body.textContent = action.preview || `${action.tool} ${JSON.stringify(action.args)}`;
        card.appendChild(body);

        // The reason says which field this falls outside and, in its last sentence, that
        // approving buys one call. That sentence is the whole difference between granting a
        // look and granting a standing permission, so it is shown, not summarised away.
        if (action.reason) {
            const reason = document.createElement('div');
            reason.className = 'text-[10px] font-mono text-amber-200/80 my-1';
            reason.textContent = action.reason;
            card.appendChild(reason);
        }

        const status = document.createElement('div');
        status.className = 'text-xs font-mono text-slate-400 mt-2';

        // "Stop asking" is a promise about a tool, so it is only offered by tools whose
        // name is enough to know what you are agreeing to. run_command's isn't: allowing
        // it once would allow every allowlisted program, with any arguments, from then
        // on. Those tools get a line saying so instead of a checkbox that would be
        // refused on the way back.
        const alwaysAllowable = action.always_allowable !== false;
        const always = document.createElement('label');
        always.className = 'flex items-center gap-2 text-[10px] font-mono text-slate-400 mt-2 cursor-pointer';
        const alwaysBox = document.createElement('input');
        alwaysBox.type = 'checkbox';
        alwaysBox.className = 'rounded bg-slate-900 border-amber-500 text-amber-400 focus:ring-0';
        if (alwaysAllowable) {
            always.appendChild(alwaysBox);
            always.appendChild(document.createTextNode(`Stop asking about ${action.tool}`));
        } else {
            always.className = 'block text-[10px] font-mono text-slate-500 mt-2';
            always.textContent = isElevation
                ? 'There is no "stop asking" for this. What runs without asking is decided by the persona\u2019s field, in Settings.'
                : `${action.tool} is asked about every time — approving it once would approve every command.`;
        }

        const buttons = document.createElement('div');
        buttons.className = 'flex gap-2 mt-2';
        const approveBtn = document.createElement('button');
        approveBtn.className = 'text-xs font-mono border border-emerald-500/50 text-emerald-300 px-3 py-1 rounded bg-emerald-950/40 hover:bg-emerald-900/40 cursor-pointer';
        approveBtn.textContent = '✔ Approve';
        const rejectBtn = document.createElement('button');
        rejectBtn.className = 'text-xs font-mono border border-rose-500/50 text-rose-300 px-3 py-1 rounded bg-rose-950/40 hover:bg-rose-900/40 cursor-pointer';
        rejectBtn.textContent = '✖ Decline';
        buttons.appendChild(approveBtn);
        buttons.appendChild(rejectBtn);

        const settle = (text, tone) => {
            buttons.remove();
            always.remove();
            status.className = `text-xs font-mono mt-2 ${tone}`;
            status.textContent = text;
        };

        approveBtn.onclick = async () => {
            approveBtn.disabled = true;
            rejectBtn.disabled = true;
            status.textContent = 'Running…';
            try {
                if (alwaysAllowable && alwaysBox.checked) await setAlwaysAllowed(action.tool, true);
                const data = IS_TAURI
                    ? await tauriInvoke('approve_action_rust', { id: action.id })
                    : await toolsApi(`/api/actions/${action.id}/approve`, { method: 'POST' });
                settle(`✔ Done — ${data && data.result ? data.result : 'no output'}`, 'text-emerald-300');
            } catch (e) {
                settle(`✖ Failed: ${e.message || e}`, 'text-rose-300');
            }
        };

        rejectBtn.onclick = async () => {
            approveBtn.disabled = true;
            rejectBtn.disabled = true;
            try {
                if (IS_TAURI) await tauriInvoke('reject_action_rust', { id: action.id });
                else await toolsApi(`/api/actions/${action.id}/reject`, { method: 'POST' });
                settle('✖ Declined', 'text-slate-400');
            } catch (e) {
                settle(`✖ Failed: ${e.message || e}`, 'text-rose-300');
            }
        };

        card.appendChild(buttons);
        card.appendChild(always);
        card.appendChild(status);
        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        voiceEngine.playSFX('alert');
        return card;
    }

    // --- Activity log --------------------------------------------------------------
    // The record of what the companion has done, and the only place an action can be
    // taken back. Undo is offered exactly where it exists: a tool that recorded no way
    // back says so rather than showing a button that fails.

    const ACTION_TONES = {
        executed: 'text-emerald-300 border-emerald-500/30',
        failed: 'text-rose-300 border-rose-500/30',
        rejected: 'text-slate-400 border-slate-600/40',
        proposed: 'text-amber-300 border-amber-500/40',
        undone: 'text-cyan-300 border-cyan-500/30'
    };

    function renderActivityRow(action) {
        const row = document.createElement('div');
        row.className = `border rounded p-2 bg-black/30 ${ACTION_TONES[action.status] || 'border-slate-600/40'}`;

        const head = document.createElement('div');
        head.className = 'flex items-center justify-between gap-2';
        head.innerHTML = `<span><strong>${action.tool}</strong> — ${action.status}${
            action.approved_by ? ` <span class="text-slate-500">(${action.approved_by})</span>` : ''
        }</span><span class="text-slate-500">${action.timestamp}</span>`;
        row.appendChild(head);

        const detail = document.createElement('div');
        detail.className = 'text-slate-300 mt-1 whitespace-pre-wrap break-all';
        detail.textContent = action.result || JSON.stringify(action.args);
        row.appendChild(detail);

        if (action.status === 'executed' && action.undo) {
            const undoBtn = document.createElement('button');
            undoBtn.className = 'mt-2 text-[10px] border border-cyan-500/40 text-cyan-300 px-2 py-0.5 rounded bg-cyan-950/40 hover:bg-cyan-900/40 cursor-pointer';
            undoBtn.textContent = '↩ Undo';
            undoBtn.onclick = async () => {
                undoBtn.disabled = true;
                undoBtn.textContent = '↩ Undoing…';
                try {
                    const data = IS_TAURI
                        ? await tauriInvoke('undo_action_rust', { id: action.id })
                        : await toolsApi(`/api/actions/${action.id}/undo`, { method: 'POST' });
                    undoBtn.replaceWith(Object.assign(document.createElement('div'), {
                        className: 'mt-2 text-[10px] text-cyan-300',
                        textContent: `↩ ${data && data.result ? data.result : 'undone'}`
                    }));
                } catch (e) {
                    undoBtn.disabled = false;
                    undoBtn.textContent = `↩ Undo failed: ${e.message || e}`;
                }
            };
            row.appendChild(undoBtn);
        } else if (action.status === 'executed') {
            const note = document.createElement('div');
            note.className = 'mt-1 text-[10px] text-slate-500';
            note.textContent = 'Cannot be undone.';
            row.appendChild(note);
        }

        return row;
    }

    async function loadActivityLog() {
        const list = document.getElementById('activity-list');
        list.innerHTML = '<div class="text-slate-400">Loading…</div>';
        try {
            const actions = IS_TAURI
                ? await tauriInvoke('get_actions_rust', { limit: 50 })
                : await toolsApi('/api/actions?limit=50');
            list.innerHTML = '';
            if (!actions || !actions.length) {
                list.innerHTML = '<div class="text-slate-400">Nothing yet — the companion has not used a tool.</div>';
                return;
            }
            for (const action of actions) list.appendChild(renderActivityRow(action));
        } catch (e) {
            list.innerHTML = `<div class="text-rose-300">Could not load the activity log: ${e.message || e}</div>`;
        }
    }

    /// Says plainly whether speech works with the network unplugged, and what is missing
    /// when it doesn't -- rather than leaving the operator to discover it by pulling the
    /// cable.
    async function loadVoiceStatus() {
        const el = document.getElementById('voice-status');
        if (!el) return;
        try {
            const status = IS_TAURI
                ? await tauriInvoke('voice_status_rust')
                : await toolsApi('/api/voice/status');
            // Only speaking has ever had a cloud fallback. Listening is local or it does
            // not happen -- saying "cloud" there told the operator a missing whisper.cpp
            // still worked over the network, which was never true. And with local-only
            // mode on, speaking has no fallback either.
            const line = (label, part, hasCloudFallback) => {
                if (part.local) {
                    return `<span class="text-emerald-400">✔</span> ${label}: local (${part.binary.split('/').pop()})`;
                }
                const state = hasCloudFallback && !status.local_only ? 'cloud' : 'unavailable';
                return `<span class="text-amber-400">•</span> ${label}: ${state} — ${part.why}`;
            };
            const missingLocally = !status.speech_out.local || !status.speech_in.local;
            el.innerHTML = [
                line('Speaking', status.speech_out, true),
                line('Listening', status.speech_in, false),
                status.offline_capable
                    ? '<span class="text-emerald-400">Works with the network unplugged.</span>'
                    : status.local_only
                        ? '<span class="text-amber-400">Local only is on, so the parts marked above stay silent until they are installed.</span>'
                        : missingLocally
                            ? '<span class="text-amber-400">The parts marked above are not installed.</span>'
                            : ''
            ].filter(Boolean).join('<br/>');

            // The environment can nail the mode on (AETHER1_LOCAL_ONLY). Where it has, the
            // checkbox is shown for what it is rather than left looking like a live control.
            const localOnlyToggle = document.getElementById('setting-local-only');
            const forcedNote = document.getElementById('local-only-forced');
            if (localOnlyToggle && status.local_only_forced) {
                localOnlyToggle.checked = true;
                localOnlyToggle.disabled = true;
            }
            if (forcedNote) forcedNote.classList.toggle('hidden', !status.local_only_forced);

            // The engine dropdown above describes a cloud fallback the mode has closed, so
            // it is reworded rather than left advertising something that cannot happen.
            const engineSelect = document.getElementById('setting-tts-engine');
            if (engineSelect) {
                const cloud = engineSelect.querySelector('option[value="cloud"]');
                if (cloud) cloud.disabled = !!status.local_only;
                const auto = engineSelect.querySelector('option[value="auto"]');
                if (auto) {
                    auto.textContent = status.local_only
                        ? 'Auto — Piper only while Local only is on'
                        : 'Auto — local if installed, otherwise cloud';
                }
            }
        } catch (e) {
            el.textContent = `Could not check the voice engines: ${e.message || e}`;
        }
    }

    async function setAlwaysAllowed(tool, allowed) {
        if (IS_TAURI) return tauriInvoke('set_always_allowed_rust', { tool, allowed });
        return toolsApi('/api/tools/always-allow', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ tool, allowed })
        });
    }

    /// Draws a card for anything still waiting that isn't already on screen. Called after
    /// each turn and on load, because a proposal outlives the conversation that made it.
    async function refreshPendingApprovals() {
        try {
            const pending = IS_TAURI
                ? await tauriInvoke('pending_actions_rust')
                : await toolsApi('/api/actions/pending');
            for (const action of pending || []) {
                if (!chatContainer.querySelector(`[data-action-id="${action.id}"]`)) {
                    renderApprovalCard(action);
                }
            }
        } catch (e) {
            console.warn('could not load pending approvals', e);
        }
    }

    // Splits streamed text into speakable chunks at sentence boundaries. Anything shorter
    // than this is not worth a TTS round-trip of its own -- a stream of two-word clips
    // sounds worse than waiting for the rest of the sentence.
    const MIN_SPEAKABLE = 24;

    function takeSpeakableChunk(buffer) {
        const match = /[.!?:](?=\s|$)|\n\n/g;
        let lastEnd = -1;
        let m;
        while ((m = match.exec(buffer)) !== null) {
            if (m.index + m[0].length >= MIN_SPEAKABLE) { lastEnd = m.index + m[0].length; break; }
        }
        if (lastEnd < 0) return null;
        return { chunk: buffer.slice(0, lastEnd), rest: buffer.slice(lastEnd) };
    }

    /// Sends a prompt and calls onDelta with each piece of the reply as it arrives.
    /// Resolves with the authoritative final reply -- the deltas are for display, the
    /// return value is what gets rendered as final text.
    async function streamChat(text, sessionId, onDelta) {
        if (IS_TAURI) {
            const streamId = `s${Date.now()}${Math.random().toString(16).slice(2)}`;
            const unlisten = await window.__TAURI__.event.listen('chat-delta', (event) => {
                if (event.payload && event.payload.stream_id === streamId) onDelta(event.payload.delta);
            });
            try {
                return await tauriInvoke('generate_response_streaming_rust', {
                    prompt: text, sessionId, streamId
                });
            } finally {
                unlisten();
            }
        }

        // Browser fallback: the same conversation over a WebSocket, since the socket
        // plumbing already exists here for telemetry (see /ws/chat in server.rs).
        return await new Promise((resolve, reject) => {
            const wsBase = (API_BASE || window.location.origin).replace(/^http/, 'ws');
            const socket = new WebSocket(`${wsBase}/ws/chat`);
            socket.onopen = () => socket.send(JSON.stringify({
                message: text, session_id: sessionId, generate_voice: false
            }));
            socket.onmessage = (event) => {
                let data;
                try { data = JSON.parse(event.data); } catch (e) { return; }
                if (data.type === 'delta') onDelta(data.delta);
                else if (data.type === 'error') { socket.close(); reject(new Error(data.error)); }
                else if (data.type === 'done') { socket.close(); resolve(data); }
            };
            socket.onerror = () => reject(new Error('chat socket failed'));
            socket.onclose = () => reject(new Error('chat socket closed before the reply finished'));
        });
    }

    async function handleSendMessage(customPrompt = null) {
        const text = customPrompt || chatInput.value.trim();
        if (!text || isWaitingForResponse) return;

        // Trace Protocols: a hidden egg only ever surfaces on top of The Nexus, never from
        // any other avatar -- see AVATAR_TRIGGER_RULES above.
        if (currentAvatar === 'nexus' || currentAvatar === 'matrix') {
            const triggeredAvatar = matchAvatarTrigger(text);
            if (triggeredAvatar) flashTraceProtocolAvatar(triggeredAvatar);
        }

        chatInput.value = '';
        appendMessage('user', text);
        voiceEngine.playSFX('click');
        voiceEngine.stopSpeech(); // a new question supersedes anything still being spoken

        isWaitingForResponse = true;
        hologram.setState('THINKING');
        if (voiceEngine.onStateChange) voiceEngine.onStateChange('THINKING');

        // The reply's own message node, created empty and filled in as deltas arrive --
        // the cursor class marks it as still being written.
        const replyDiv = appendMessage(currentAgentName, '');
        replyDiv.classList.add('typing-cursor');

        let rendered = '';
        let spoken = '';        // text already handed to TTS
        let pending = '';       // text waiting for a sentence boundary
        let firstDelta = true;
        // Only enqueueTTS's own drain loop ever sets the hologram back to IDLE once speech
        // starts (see voice.js). If synthesis fails for every chunk -- e.g. no network for
        // edge-tts in offline mode -- nothing ever queues, so nothing ever fires that IDLE,
        // and the hologram is left stuck in THINKING forever. Track whether anything actually
        // made it into the queue so the code below can reset state itself when nothing did.
        let audioQueued = false;

        const speakChunk = async (chunk) => {
            if (!autoSpeak || !chunk.trim()) return;
            try {
                const url = await synthesizeSpeechUrl(chunk);
                if (url) {
                    voiceEngine.enqueueTTS(url);
                    audioQueued = true;
                }
            } catch (e) {
                console.warn('sentence TTS failed', e);
            }
        };

        const onDelta = (delta) => {
            if (!delta) return;
            if (firstDelta) {
                // Generation has actually started; stop pretending to think.
                firstDelta = false;
                hologram.setState('IDLE');
            }
            rendered += delta;
            replyDiv.bodyDiv.innerHTML = formatMarkdown(rendered);
            chatContainer.scrollTop = chatContainer.scrollHeight;

            pending += delta;
            let taken;
            while ((taken = takeSpeakableChunk(pending)) !== null) {
                pending = taken.rest;
                spoken += taken.chunk;
                speakChunk(taken.chunk);
            }
        };

        try {
            const data = await streamChat(text, 'default', onDelta);
            const reply = data.reply;
            const agentName = data.agent_name;

            if (agentName) updateAgentNameDisplay(agentName);
            // The return value is authoritative: render it in place of the accumulated
            // deltas, which also repairs the display if any delta was dropped.
            replyDiv.bodyDiv.innerHTML = formatMarkdown(reply);
            replyDiv.classList.remove('typing-cursor');
            voiceEngine.playSFX('incoming');

            // Speak whatever never reached a sentence boundary (the tail of the reply).
            const tail = reply.slice(spoken.length);
            if (tail.trim()) await speakChunk(tail);

            await refreshPendingApprovals();

            if (autoSpeak) {
                attachLazyReplay(replyDiv, reply);
            }
            // If speech never actually got queued (autoSpeak off, or every synthesis attempt
            // failed) nothing else is going to bring the hologram out of THINKING -- do it here.
            if (!audioQueued) {
                hologram.setState('IDLE');
                if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
            }
        } catch (e) {
            console.error("Chat error", e);
            replyDiv.classList.remove('typing-cursor');
            replyDiv.bodyDiv.innerHTML = formatMarkdown(
                rendered ? `${rendered}\n\n⚠️ System Error: ${e.message || e}` : `⚠️ System Error: ${e.message || e}`
            );
            hologram.setState('IDLE');
            if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
        } finally {
            isWaitingForResponse = false;
        }
    }

    async function handleGenesisForge(purpose) {
        if (!purpose || !purpose.trim()) {
            alert("Please enter a purpose description");
            return;
        }

        voiceEngine.playSFX('boot');
        hologram.setState('THINKING');

        try {
            let data;
            if (IS_TAURI) {
                data = await tauriInvoke('agent_genesis_rust', { purpose: purpose.trim() });
            } else {
                const resp = await apiFetch('/api/agent/genesis', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ purpose: purpose.trim() })
                });
                if (!resp.ok) throw new Error(`genesis request failed: ${resp.status}`);
                data = await resp.json();
            }

            const audioUrl = IS_TAURI
                ? (autoSpeak ? await synthesizeSpeechUrl(data.greeting, data.voice) : null)
                : (data.audio_url ? API_BASE + data.audio_url : null);
            updateAgentNameDisplay(data.name);

            if (data.name.includes("R.E.D.")) { applyAvatar('red'); applyThemePreset('red'); }
            else if (data.name.includes("NEXUS")) { applyAvatar('nexus'); applyThemePreset('nexus'); }
            else if (data.name.includes("A.R.X.LOGOS")) { applyAvatar('arx-logos'); applyThemePreset('arx-logos'); }
            else if (data.name.includes("A.R.X.LIMES")) { applyAvatar('arx-limes'); applyThemePreset('arx-limes'); }

            settingsModal.classList.add('hidden');
            appendMessage(data.name, `### ⚡ IDENTITY FORGED: **${data.name}**\n**Callsign**: \`${data.callsign}\`\n\n${data.greeting}`, audioUrl);

            if (audioUrl && autoSpeak) {
                await voiceEngine.playTTSAudio(audioUrl);
            } else {
                hologram.setState('IDLE');
            }
        } catch (e) {
            alert(`Genesis Error: ${e.message || e}`);
            hologram.setState('IDLE');
        }
    }

    // --- Local server discovery ------------------------------------------------------
    // What the scan found last, so the two dropdowns can be rebuilt without probing
    // again. Deliberately not persisted: a server that was up ten minutes ago is not
    // evidence of anything, and offering a stale endpoint is how you get a mystery.
    let discoveredServers = [];

    /// Fills the model dropdown from one server's list. Leaves the free-text box alone --
    /// it may hold a cloud model name the operator typed, which no scan can know about.
    function populateModelPicker(server) {
        const picker = document.getElementById('setting-model-picker');
        if (!picker) return;
        picker.innerHTML = '';
        const models = (server && server.models) || [];
        const first = document.createElement('option');
        first.value = '';
        first.textContent = models.length
            ? '-- choose one of the models on this server --'
            : '-- no models to list; type one below --';
        picker.appendChild(first);
        for (const model of models) {
            const opt = document.createElement('option');
            opt.value = model;
            opt.textContent = model;
            picker.appendChild(opt);
        }
        const current = document.getElementById('setting-model').value;
        if (models.includes(current)) picker.value = current;
    }

    /// Rebuilds the "local servers found" dropdown. Each entry carries everything needed
    /// to configure the app for it, so choosing one is the whole setup step.
    function populateLocalServers(servers) {
        discoveredServers = Array.isArray(servers) ? servers : [];
        const select = document.getElementById('setting-local-server');
        if (!select) return;
        select.innerHTML = '';
        const first = document.createElement('option');
        first.value = '';
        first.textContent = discoveredServers.length
            ? `-- ${discoveredServers.length} found; choose one to use it --`
            : '-- none found yet; press Scan below --';
        select.appendChild(first);
        discoveredServers.forEach((server, index) => {
            const opt = document.createElement('option');
            opt.value = String(index);
            opt.textContent = server.label;
            select.appendChild(opt);
        });

        // Keep it selected if the configured endpoint is one of the servers found.
        const endpoint = document.getElementById('setting-endpoint').value.trim();
        const match = discoveredServers.findIndex(s => s.endpoint === endpoint);
        if (match >= 0) {
            select.value = String(match);
            populateModelPicker(discoveredServers[match]);
        }
    }

    // The placeholder shown for a fresh install (see get_settings_rust's defaults) -- fine
    // for the Offline provider, which never sends it anywhere, but a guaranteed "model not
    // found" the moment a cloud/local provider is selected without also retyping this field.
    // Matches the defaults each provider's own Rust code falls back to on an empty model
    // name (see openai_defaults/gemini_model/anthropic_payload in providers.rs), so picking
    // a provider here and what actually gets sent never disagree.
    const PLACEHOLDER_MODEL = 'halcy-core';
    const DEFAULT_MODEL_FOR_PROVIDER = {
        openai: 'gpt-4o-mini',
        groq: 'llama-3.3-70b-versatile',
        gemini: 'gemini-2.0-flash',
        anthropic: 'claude-opus-5',
    };

    /// Switching to a real provider with the untouched placeholder (or nothing) still in the
    /// model box would silently send that placeholder as the model name and fail -- this
    /// fills in a model that actually exists for the newly chosen provider instead. Leaves a
    /// model the operator typed themselves alone, for any other provider, on purpose.
    function applyDefaultModelForProvider(providerKey) {
        const defaultModel = DEFAULT_MODEL_FOR_PROVIDER[providerKey];
        if (!defaultModel) return; // ollama/lmstudio/offline: no one-size-fits-all default
        const model = document.getElementById('setting-model');
        if (!model.value.trim() || model.value.trim() === PLACEHOLDER_MODEL) {
            model.value = defaultModel;
        }
    }

    /// Tries whatever is currently typed into the Provider/Model/Endpoint/API Key fields --
    /// not what's saved -- with one real message, so a mistake is caught here instead of
    /// discovered later in chat. Never touches conversation history or token telemetry: this
    /// is a probe, not a turn.
    async function handleTestConnection() {
        const provider = document.getElementById('setting-provider').value;
        const model = document.getElementById('setting-model').value.trim();
        const endpoint = document.getElementById('setting-endpoint').value.trim();
        const apiKey = document.getElementById('setting-apikey').value;

        if (btnTestConnection) btnTestConnection.disabled = true;
        if (testConnectionStatus) {
            testConnectionStatus.classList.remove('hidden', 'border-green-500/40', 'border-red-500/40', 'text-green-400', 'text-red-400');
            testConnectionStatus.classList.add('text-cyan-300', 'animate-pulse');
            testConnectionStatus.textContent = `Testing ${provider}...`;
        }

        try {
            const args = { provider, model, endpoint, apiKey };
            const message = IS_TAURI
                ? await tauriInvoke('test_llm_connection_rust', args)
                : await (async () => {
                    const resp = await apiFetch('/api/llm/test-connection', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ provider, model, endpoint, api_key: apiKey })
                    });
                    const data = await resp.json().catch(() => ({}));
                    if (!resp.ok) throw new Error(data.error || `test failed: ${resp.status}`);
                    return data.message;
                })();

            if (testConnectionStatus) {
                testConnectionStatus.classList.remove('animate-pulse', 'text-cyan-300');
                testConnectionStatus.classList.add('text-green-400', 'border-green-500/40');
                testConnectionStatus.textContent = `✔ ${message}`;
            }
        } catch (e) {
            if (testConnectionStatus) {
                testConnectionStatus.classList.remove('animate-pulse', 'text-cyan-300');
                testConnectionStatus.classList.add('text-red-400', 'border-red-500/40');
                testConnectionStatus.textContent = `⚠ ${e.message || e}`;
            }
        } finally {
            if (btnTestConnection) btnTestConnection.disabled = false;
        }
    }

    /// Choosing a server is the setup: provider, endpoint and the model list all follow
    /// from it, so the operator never has to know which API shape their server speaks or
    /// whether its URL needs a /v1 on the end.
    function applyLocalServer(index) {
        const server = discoveredServers[index];
        if (!server) return;
        document.getElementById('setting-provider').value = server.provider_key;
        document.getElementById('setting-endpoint').value = server.endpoint;
        populateModelPicker(server);
        const model = document.getElementById('setting-model');
        if (server.models.length && !server.models.includes(model.value)) {
            model.value = server.models[0];
            document.getElementById('setting-model-picker').value = server.models[0];
        }
    }

    async function handleScanSystem() {
        if (scannerResultsBox) {
            scannerResultsBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Looking for local LLM servers and API keys...</div>';
        }
        voiceEngine.playSFX('click');

        try {
            const data = IS_TAURI
                ? await tauriInvoke('scan_models_rust')
                : await (async () => {
                    const resp = await apiFetch('/api/scanner/status');
                    if (!resp.ok) throw new Error(`scan request failed: ${resp.status}`);
                    return resp.json();
                })();

            let html = '';

            if (data.cloud_keys.detected_key) {
                html += `<div class="text-green-400">✔ Detected Provider API Key (${data.cloud_keys.detected_provider || 'cloud'}) via Environment</div>`;
            } else {
                html += `<div class="text-slate-400">⚪ No Provider API keys found in environment.</div>`;
            }

            // Reported by what answered, not by which product it is presumed to be: a
            // probe can tell you a server is there and what it can run, and guessing at a
            // brand on top of that is a guess the operator would have to check anyway.
            const servers = data.local_servers || [];
            if (servers.length) {
                for (const server of servers) {
                    const models = server.models.length ? server.models.join(', ') : 'no models loaded';
                    html += `<div class="text-green-400">✔ ${server.label} <span class="text-slate-400">(${server.endpoint})</span><br><span class="pl-6 text-cyan-300">${models}</span></div>`;
                }
                html += `<div class="text-slate-400">Pick one from LOCAL SERVERS FOUND above to use it.</div>`;
            } else {
                html += `<div class="text-slate-400">⚪ No local LLM server answered on this machine. Start one, or type its address into the endpoint box if it uses an unusual port.</div>`;
            }

            // "Installed but stopped" and "not installed at all" are different problems
            // with different fixes, and telling someone to start a service they have never
            // installed sends them looking for something that was never there.
            if (data.ollama.cli_installed && !servers.some(s => s.port === 11434)) {
                html += `<div class="text-yellow-400">⚠ A local model runner is installed but not serving -- start it first (for Ollama, \`ollama serve\`, or \`sudo systemctl start ollama\` where it is a service).</div>`;
            } else if (!servers.length) {
                html += `<div class="text-slate-400">No local model runner is installed either -- there is no \`ollama\` on PATH. See ollama.com/download, or point the endpoint box at a server on another machine on your LAN.</div>`;
            }

            populateLocalServers(servers);
            if (scannerResultsBox) scannerResultsBox.innerHTML = html;
        } catch (e) {
            if (scannerResultsBox) scannerResultsBox.innerHTML = `<div class="text-red-400">Scan failed: ${e.message || e}</div>`;
        }
    }

    async function handlePullLlama() {
        const modelName = selectLocalModel ? selectLocalModel.value : 'llama3.2:1b';
        const modelLabel = selectLocalModel ? selectLocalModel.options[selectLocalModel.selectedIndex].text : modelName;
        if (!confirm(`Install ${modelLabel} via Ollama? (Requires Ollama running)`)) return;
        voiceEngine.playSFX('click');
        if (scannerResultsBox) {
            scannerResultsBox.innerHTML = `<div class="text-cyan-300 animate-pulse">Requesting Ollama to pull ${modelName}...</div>`;
        }

        try {
            const data = IS_TAURI
                ? await tauriInvoke('pull_model_rust', { modelName })
                : await (async () => {
                    const resp = await apiFetch(`/api/scanner/pull-model?model_name=${encodeURIComponent(modelName)}`, { method: 'POST' });
                    return resp.json();
                })();
            if (scannerResultsBox) {
                scannerResultsBox.innerHTML = `<div class="${data.status === 'error' ? 'text-red-400' : 'text-green-400'}">${data.message}</div>`;
            }
        } catch (e) {
            alert(`Install error: ${e.message || e}`);
        }
    }

    // Version & Updates -- mirrors the taskbar tray icon's "Check for Updates" /
    // "Update Available" flow, but in the HUD itself. Real self-updating (git pull +
    // rebuild) only makes sense for the native desktop app, so this whole feature is
    // Tauri-only; see IS_TAURI gating in initVersionAndUpdates() below.
    /* ---- The avatar workbench ------------------------------------------------
     * A separate page (frontend/avatar-lab.html) that stands up its own copy of the
     * avatar engine. In the desktop app it gets its own window; in a browser, a tab.
     */
    async function openAvatarLab() {
        if (IS_TAURI) {
            try {
                await tauriInvoke('open_avatar_lab_rust');
                return;
            } catch (err) {
                /* Fall through rather than leaving the button dead: the page itself works
                   in a plain webview even if the window command is unavailable. */
                console.warn('Could not open the workbench window; falling back:', err);
            }
        }
        window.open('avatar-lab.html', 'aether1-avatar-lab');
    }

    /* Rebuild the custom avatar when the saved design has moved on from what is on
       screen. Without this, pressing Save in the workbench looks like it did nothing:
       the recipe is read once, when the avatar is built. */
    function refreshCustomAvatarIfStale() {
        if (!window.CustomAvatarRecipe || !window.CustomAvatarRecipe.isStale()) return;
        hologram.rebuildRegisteredAvatar('custom');
    }

    document.getElementById('btn-open-avatar-lab')?.addEventListener('click', openAvatarLab);
    document.getElementById('btn-customise-avatar')?.addEventListener('click', openAvatarLab);

    /* localStorage fires this in *other* windows of the same origin, so the HUD follows
       along live while the workbench is open beside it -- press Save there and the avatar
       here changes, with no reload and nothing to click. */
    window.addEventListener('storage', (event) => {
        if (!window.CustomAvatarRecipe) return;
        if (event.key !== window.CustomAvatarRecipe.key) return;
        if (currentAvatar !== 'custom') return;
        refreshCustomAvatarIfStale();
    });

    async function tauriInvoke(cmd, args) {
        if (!window.__TAURI__ || !window.__TAURI__.core) {
            throw new Error('Tauri bridge unavailable');
        }
        return window.__TAURI__.core.invoke(cmd, args);
    }

    // Synthesizes speech via the native TTS command and turns the local mp3 path it returns
    // into a URL the webview's <audio> element can actually load (convertFileSrc maps a
    // filesystem path to Tauri's asset:// protocol; see the assetProtocol scope this path's
    // directory is allowed under in tauri.conf.json / main.rs's setup()). Returns null on
    // any failure so callers can just skip voice playback instead of erroring the whole chat.
    // Both transports: the native path synthesizes over IPC and plays the file directly,
    // the browser path posts to /api/tts and plays it back over HTTP.
    async function synthesizeSpeechUrl(text, voiceName) {
        try {
            if (IS_TAURI) {
                const path = await tauriInvoke('generate_speech_rust', { text, voice: voiceName || null });
                return window.__TAURI__.core.convertFileSrc(path);
            }
            const resp = await apiFetch('/api/tts', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ text, voice: voiceName || null })
            });
            if (!resp.ok) throw new Error(`tts request failed: ${resp.status}`);
            const data = await resp.json();
            return data.audio_url ? API_BASE + data.audio_url : null;
        } catch (e) {
            console.warn('TTS synthesis failed', e);
            return null;
        }
    }

    async function loadVersionInfo() {
        try {
            const info = await tauriInvoke('get_version_info');
            if (versionBadge) versionBadge.textContent = info.version;
            if (settingsVersionLabel) settingsVersionLabel.textContent = `${info.version} (${info.commit_short})`;
        } catch (e) {
            console.error('Version info error', e);
        }
    }

    // Win32/Win64/WinCE cover every real Windows UA; navigator.platform is deprecated but
    // still present in the WebView2 shell this app actually runs in, and this only decides
    // whether to show a button, not anything security-relevant.
    function isWindows() {
        return /^Win/.test(navigator.platform || '') || /Windows/.test(navigator.userAgent || '');
    }

    async function handleCheckForUpdate() {
        if (versionBadge) versionBadge.classList.add('animate-pulse');
        if (updateStatusBox) {
            updateStatusBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Checking GitHub for the latest commit on main...</div>';
        }
        if (btnApplyUpdate) btnApplyUpdate.classList.add('hidden');
        if (btnInstallGh) btnInstallGh.classList.add('hidden');

        try {
            const status = await tauriInvoke('check_for_update_rust');
            if (settingsVersionLabel) settingsVersionLabel.textContent = `${status.version} (${status.built_commit_short})`;

            if (!status.checked) {
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-yellow-400">⚠ Could not check for updates: ${status.error || 'unknown error'}</div>`;
                }
                if (versionBadge) versionBadge.classList.add('border-yellow-500/50', 'text-yellow-400');
                if (btnInstallGh) btnInstallGh.classList.toggle('hidden', !(status.needs_gh_auth && isWindows()));
                return;
            }

            if (status.up_to_date) {
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-green-400">✔ Up to date -- ${status.version} (build ${status.built_commit_short})</div>`;
                }
                if (versionBadge) {
                    versionBadge.classList.remove('border-yellow-500/50', 'text-yellow-400');
                    versionBadge.classList.add('border-green-500/50', 'text-green-400');
                }
            } else {
                const latestShort = status.latest_commit ? status.latest_commit.slice(0, 7) : 'unknown';
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-yellow-400">⬆ Update available -- running ${status.built_commit_short}, latest is ${latestShort}</div>`;
                }
                if (versionBadge) {
                    versionBadge.classList.remove('border-green-500/50', 'text-green-400');
                    versionBadge.classList.add('border-yellow-500/50', 'text-yellow-400');
                }
                if (btnApplyUpdate) btnApplyUpdate.classList.remove('hidden');
            }
        } catch (e) {
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">Update check failed: ${e.message || e}</div>`;
            }
        } finally {
            if (versionBadge) versionBadge.classList.remove('animate-pulse');
        }
    }

    async function handleInstallGh() {
        voiceEngine.playSFX('click');
        if (btnInstallGh) btnInstallGh.disabled = true;
        if (btnCheckUpdate) btnCheckUpdate.disabled = true;
        if (updateStatusBox) {
            updateStatusBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Installing gh via winget -- this can take a minute...</div>';
        }

        try {
            const message = await tauriInvoke('install_gh_via_winget_rust');
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-green-400">✔ ${message}</div>`;
            }
            if (btnInstallGh) btnInstallGh.classList.add('hidden');
        } catch (e) {
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">⚠ Could not install gh: ${e.message || e}</div>`;
            }
        } finally {
            if (btnInstallGh) btnInstallGh.disabled = false;
            if (btnCheckUpdate) btnCheckUpdate.disabled = false;
        }
    }

    async function handleApplyUpdate() {
        if (!confirm('Pull the latest changes, rebuild, and relaunch Aether1? The app will restart.')) return;
        voiceEngine.playSFX('click');
        if (updateStatusBox) {
            updateStatusBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Updating -- pulling latest changes and rebuilding. This can take over a minute; the app will restart automatically when it\'s done.</div>';
        }
        if (btnApplyUpdate) btnApplyUpdate.disabled = true;
        if (btnCheckUpdate) btnCheckUpdate.disabled = true;

        try {
            // On success the app exits and relaunches before this ever resolves --
            // reaching the catch block means it genuinely failed.
            await tauriInvoke('apply_update_rust');
        } catch (e) {
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">⚠ Update failed: ${e.message || e}. See the terminal/tray for details.</div>`;
            }
            if (btnApplyUpdate) btnApplyUpdate.disabled = false;
            if (btnCheckUpdate) btnCheckUpdate.disabled = false;
        }
    }

    function initVersionAndUpdates() {
        if (!IS_TAURI) {
            document.getElementById('setting-hotkey-wrap')?.classList.add('hidden');
            return;
        }
        if (versionBadge) versionBadge.classList.replace('hidden', 'inline-flex');
        if (updateSection) updateSection.classList.remove('hidden');
        loadVersionInfo();
        handleCheckForUpdate();
    }

    // Desktop Sprite Mode is a transparent/always-on-top native window -- meaningless in the
    // plain browser flow, so the whole section stays hidden there (mirrors initVersionAndUpdates).
    function initSpriteMode() {
        if (!IS_TAURI) return;
        const section = document.getElementById('sprite-mode-section');
        if (section) section.classList.remove('hidden');
    }

    async function loadChatHistory() {
        try {
            const msgs = IS_TAURI
                ? await tauriInvoke('get_messages_rust', { limit: 25 })
                : await (async () => {
                    const resp = await apiFetch('/api/messages?limit=25');
                    if (!resp.ok) throw new Error(`messages request failed: ${resp.status}`);
                    return resp.json();
                })();

            chatContainer.innerHTML = '';
            if (msgs.length === 0) {
                appendMessage(currentAgentName, `Greetings Operator. **${currentAgentName}** online and ready for deployment.`);
            } else {
                msgs.forEach(m => appendMessage(m.sender, m.text));
            }
        } catch (e) {
            console.warn("Could not load messages", e);
        }
    }

    /* Fetched once and reused: the catalogue depends on nothing but the build, so re-fetching
       it every time Settings opens would be a round trip to learn the same nine rows. */
    let personaCataloguePromise = null;
    function ensurePersonaCatalogue() {
        if (!personaCataloguePromise) personaCataloguePromise = loadPersonaCatalogue();
        return personaCataloguePromise;
    }

    async function loadSettings() {
        try {
            // Before any saved value is applied to the field: setting .value to a persona
            // whose <option> has not been added yet silently selects nothing.
            await ensurePersonaCatalogue();
            const data = IS_TAURI
                ? await tauriInvoke('get_settings_rust')
                : await (async () => {
                    const resp = await apiFetch('/api/settings');
                    if (!resp.ok) throw new Error(`settings request failed: ${resp.status}`);
                    return resp.json();
                })();

            const s = data.settings;
            updateAgentNameDisplay(s.agent_name || "HALCY");
            document.getElementById('setting-agent-name').value = s.agent_name || "HALCY";
            document.getElementById('setting-provider').value = s.llm_provider || 'offline';
            document.getElementById('setting-model').value = s.llm_model || 'halcy-core';
            document.getElementById('setting-endpoint').value = s.llm_endpoint || 'http://localhost:11434';
            document.getElementById('setting-apikey').value = s.llm_api_key || '';
            document.getElementById('setting-persona').value = s.persona_type || 'default';
            document.getElementById('setting-custom-directive').value = s.custom_directive || '';
            toggleCustomPersonaField();
            showPersonaSpeciality();
            document.getElementById('setting-voice').value = s.voice_name || 'en-US-AriaNeural';
            document.getElementById('setting-hotkey').value = s.hotkey_toggle ?? 'Super+Shift+A';
            document.getElementById('setting-tts-engine').value = s.tts_engine || 'auto';
            document.getElementById('setting-vault-path').value = s.vault_path || '';
            document.getElementById('setting-local-only').checked = s.local_only === true;
            // After the checkbox is set, not before: loadVoiceStatus is what discovers an
            // environment-forced mode and overrides the saved value on screen.
            loadVoiceStatus();
            document.getElementById('setting-tools').checked = s.tools_enabled === true;
            document.getElementById('setting-command-allowlist').value =
                Array.isArray(s.command_allowlist) ? s.command_allowlist.join(', ') : '';
            document.getElementById('setting-autospeak').checked = s.auto_speak !== false;
            autoSpeak = s.auto_speak !== false;
            const spriteModeToggle = document.getElementById('setting-sprite-mode');
            if (spriteModeToggle) spriteModeToggle.checked = s.desktop_sprite_enabled === true;
        } catch (e) {
            console.warn("Could not load settings", e);
        }
    }

    // Shows the custom persona directive textarea only when "Custom Directive" is
    // selected as the base persona -- the preset personas don't need it.
    /* The persona list, built from what the backend says exists rather than from a list in
       the markup. Each entry leads with what it is *for* -- "Coding", "Cites Sources" -- with
       the avatar it belongs to bookended after it, because the persona and the avatar are one
       choice you can make from either end. */
    async function loadPersonaCatalogue() {
        const select = document.getElementById('setting-persona');
        if (!select) return;
        let personas;
        try {
            personas = IS_TAURI
                ? await tauriInvoke('list_personas_rust')
                : await (await apiFetch('/api/personas')).json();
        } catch (e) {
            // The placeholder option in the markup stays, so the field is still usable.
            console.warn('Could not load the persona list', e);
            return;
        }
        if (!Array.isArray(personas) || personas.length === 0) return;

        const chosen = select.value;
        select.innerHTML = '';
        personas.forEach((persona) => {
            const option = document.createElement('option');
            option.value = persona.key;
            option.dataset.speciality = persona.speciality || '';
            option.dataset.field = persona.field || '';
            // "the The Nexus avatar" -- an avatar whose name already carries its article
            // does not want another one.
            const avatarPhrase = /^the\s/i.test(persona.avatar || '')
                ? persona.avatar
                : `the ${persona.avatar}`;
            option.textContent = persona.avatar
                ? `${persona.short_name} (default for ${avatarPhrase} avatar)`
                : persona.short_name;
            select.appendChild(option);
        });
        if (personas.some((p) => p.key === chosen)) select.value = chosen;
        showPersonaSpeciality();
    }

    /* The one-line description of the selected persona, under the field. It does not fit in
       an <option> at a readable length, and a list where every row is a sentence is a list
       nobody scans.

       The second line is what it reads without asking. Picking a persona is now picking a
       level of access, and that is not something anyone should have to discover by watching
       it ask -- or worse, by watching it not ask. */
    function showPersonaSpeciality() {
        const select = document.getElementById('setting-persona');
        const line = document.getElementById('persona-speciality');
        if (!select || !line) return;
        const option = select.selectedOptions[0];
        line.textContent = option?.dataset.speciality || '';
        const access = document.getElementById('persona-field');
        if (!access) return;
        const field = option?.dataset.field || '';
        access.textContent = field
            ? `Reads ${field} without asking. Anything else asks you first, once, for that one call.`
            : '';
    }

    function toggleCustomPersonaField() {
        const wrap = document.getElementById('custom-persona-wrap');
        if (wrap) wrap.classList.toggle('hidden', document.getElementById('setting-persona').value !== 'custom');
    }

    async function saveSettings(notify = true) {
        const spriteModeToggle = document.getElementById('setting-sprite-mode');
        const payload = {
            settings: {
                agent_name: document.getElementById('setting-agent-name').value.trim() || "HALCY",
                llm_provider: document.getElementById('setting-provider').value,
                llm_model: document.getElementById('setting-model').value,
                llm_endpoint: document.getElementById('setting-endpoint').value,
                llm_api_key: document.getElementById('setting-apikey').value,
                persona_type: document.getElementById('setting-persona').value,
                custom_directive: document.getElementById('setting-custom-directive').value.trim(),
                voice_name: document.getElementById('setting-voice').value,
                tts_engine: document.getElementById('setting-tts-engine').value,
                local_only: document.getElementById('setting-local-only').checked,
                vault_path: document.getElementById('setting-vault-path').value.trim(),
                // Sent only from the native app: the browser fallback has no window for the
                // OS to summon, and saving a chord there would promise something that can't
                // happen. See setting-hotkey-wrap, hidden on that path.
                ...(IS_TAURI ? { hotkey_toggle: document.getElementById('setting-hotkey').value.trim() } : {}),
                tools_enabled: document.getElementById('setting-tools').checked,
                command_allowlist: document.getElementById('setting-command-allowlist').value
                    .split(',').map(p => p.trim()).filter(Boolean),
                auto_speak: document.getElementById('setting-autospeak').checked,
                desktop_sprite_enabled: spriteModeToggle ? spriteModeToggle.checked : false
            }
        };
        autoSpeak = payload.settings.auto_speak;
        updateAgentNameDisplay(payload.settings.agent_name);

        try {
            if (IS_TAURI) {
                await tauriInvoke('save_settings_rust', { settings: payload.settings });
                await tauriInvoke('toggle_sprite_window_rust', { enabled: payload.settings.desktop_sprite_enabled });
            } else {
                const resp = await apiFetch('/api/settings', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(payload)
                });
                if (!resp.ok) throw new Error(`settings save failed: ${resp.status}`);
            }
            // The mode changes what the two speech engines mean, so the readout under them
            // is re-asked rather than left describing the settings as they were.
            loadVoiceStatus();
            if (notify) {
                voiceEngine.playSFX('click');
                settingsModal.classList.add('hidden');
                appendMessage(currentAgentName, '⚙️ Cognitive Core & Identity configurations updated.');
            }
        } catch (e) {
            // Tauri's invoke() rejects with whatever string the Rust command's Err
            // variant carried, not an Error object, so e.message is undefined for every
            // native-app failure -- e itself is the actual message.
            const reason = e.message || e;
            if (!notify) return;
            // save_settings_rust saves everything first and only then re-registers the
            // hotkey (see hotkey::reregister_from_settings), so a chord that fails to
            // register still means every other field made it to disk -- this is not the
            // same failure as the save itself rejecting, and shouldn't read as one.
            if (reason.startsWith('settings saved, but the hotkey was not')) {
                voiceEngine.playSFX('click');
                settingsModal.classList.add('hidden');
                appendMessage(currentAgentName, `⚙️ Cognitive Core & Identity configurations updated. ⚠ ${reason}`);
                return;
            }
            alert(`Error saving settings: ${reason}`);
        }
    }

    // Avatar Selector Buttons & Pills
    document.querySelectorAll('.avatar-pill, .avatar-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const avatar = btn.getAttribute('data-avatar-val') || btn.getAttribute('data-avatar');
            if (avatar) {
                voiceEngine.playSFX('click');
                applyAvatar(avatar, true);
            }
        });
    });

    /* Applying a colour preset. Note it can move you between modes: the Daylight and Midnight
       presets belong to Solar and Eclipse, so picking one from Cyberpunk switches the chrome
       too -- which is what someone clicking a light preset means. */
    function applyThemePreset(id) {
        paintTheme(Aether1Theme.setPreset(id));
    }

    // Mode: the three buttons in Settings and the same three in the top bar's slide-out.
    document.querySelectorAll('.theme-mode-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            paintTheme(Aether1Theme.setMode(btn.getAttribute('data-theme-mode')));
        });
    });

    // Colours: the presets...
    document.querySelectorAll('.color-theme-pill, .color-theme-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const preset = btn.getAttribute('data-color-theme-val') || btn.getAttribute('data-color-theme');
            if (preset) {
                voiceEngine.playSFX('click');
                applyThemePreset(preset);
            }
        });
    });

    /* ...and the three pickers. 'input' rather than 'change' so the page repaints while the
       colour is being dragged around -- picking a background you cannot see the effect of is
       guesswork. Each write only touches its own slot, which is what keeps the background
       stable while an accent is being tried. */
    Object.keys(themeColourInputs).forEach(slot => {
        const input = themeColourInputs[slot];
        if (!input) return;
        input.addEventListener('input', () => {
            paintTheme(Aether1Theme.setColour(slot, input.value));
        });
    });

    /* ...and the two tone sliders. Same 'input' rather than 'change': the whole point of a
       slider over a number box is watching the window change as it moves. */
    Object.keys(themeToneInputs).forEach(slot => {
        const input = themeToneInputs[slot];
        if (!input) return;
        input.addEventListener('input', () => {
            paintTheme(Aether1Theme.setTone(slot, input.value));
        });
    });

    /* The telemetry tabs. A click selects a view and holds it there long enough to read
       before the idle cycle resumes -- being pulled off the thing you just chose to look at
       is the failure mode a cycling panel has. */
    document.querySelectorAll('.telemetry-tab').forEach(tab => {
        tab.addEventListener('click', () => {
            telemetryHeldUntil = Date.now() + TELEMETRY_CLICK_HOLD_MS;
            showTelemetryView(tab.getAttribute('data-telemetry-tab'), false);
        });
    });
    setInterval(cycleTelemetryView, TELEMETRY_CYCLE_MS);

    const btnThemeColoursReset = document.getElementById('btn-theme-colours-reset');
    if (btnThemeColoursReset) {
        btnThemeColoursReset.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            paintTheme(Aether1Theme.resetColours());
        });
    }

    // Base Persona Dropdown -- reveal the custom directive textarea only when needed
    const settingPersonaSelect = document.getElementById('setting-persona');
    if (settingPersonaSelect) {
        settingPersonaSelect.addEventListener('change', () => {
            toggleCustomPersonaField();
            showPersonaSpeciality();
        });
    }

    btnSend.addEventListener('click', () => handleSendMessage());
    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSendMessage();
        }
    });

    // Press and hold, same as the key -- the button is the discoverable version of it.
    btnMic.addEventListener('mousedown', () => startTalking());
    btnMic.addEventListener('mouseup', () => stopTalking());
    btnMic.addEventListener('mouseleave', () => stopTalking());
    btnMic.addEventListener('touchstart', (e) => { e.preventDefault(); startTalking(); });
    btnMic.addEventListener('touchend', (e) => { e.preventDefault(); stopTalking(); });

    const btnActivity = document.getElementById('btn-activity');
    const activityModal = document.getElementById('activity-modal');
    btnActivity.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        activityModal.classList.remove('hidden');
        loadActivityLog();
    });
    document.getElementById('btn-close-activity').addEventListener('click', () => {
        voiceEngine.playSFX('click');
        activityModal.classList.add('hidden');
    });

    btnSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        // The scan waits for the settings: it preselects whichever found server matches
        // the configured endpoint, and that field has to be filled in before it looks.
        loadSettings().then(handleScanSystem);
        showSettingsTab('customisation');
        settingsModal.classList.remove('hidden');
    });

    btnForgeIdentity.addEventListener('click', () => {
        handleGenesisForge(genesisPurposeInput.value);
    });

    btnScanSystem.addEventListener('click', () => {
        handleScanSystem();
    });

    if (btnTestConnection) {
        btnTestConnection.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            handleTestConnection();
        });
    }

    const providerSelect = document.getElementById('setting-provider');
    if (providerSelect) {
        providerSelect.addEventListener('change', (e) => {
            applyDefaultModelForProvider(e.target.value);
        });
    }

    const localServerSelect = document.getElementById('setting-local-server');
    if (localServerSelect) {
        localServerSelect.addEventListener('change', (e) => {
            if (e.target.value === '') return;
            applyLocalServer(Number(e.target.value));
            voiceEngine.playSFX('click');
        });
    }

    const modelPicker = document.getElementById('setting-model-picker');
    if (modelPicker) {
        modelPicker.addEventListener('change', (e) => {
            if (e.target.value === '') return;
            document.getElementById('setting-model').value = e.target.value;
        });
    }

    btnPullLlama.addEventListener('click', () => {
        handlePullLlama();
    });

    if (versionBadge) {
        versionBadge.addEventListener('click', () => handleCheckForUpdate());
    }
    if (btnCheckUpdate) {
        btnCheckUpdate.addEventListener('click', () => handleCheckForUpdate());
    }
    if (btnInstallGh) {
        btnInstallGh.addEventListener('click', () => handleInstallGh());
    }
    if (btnApplyUpdate) {
        btnApplyUpdate.addEventListener('click', () => handleApplyUpdate());
    }

    btnCloseSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        settingsModal.classList.add('hidden');
    });

    btnSaveSettings.addEventListener('click', () => {
        saveSettings(true);
    });

    btnClearChat.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        if (confirm("Clear conversation logs?")) {
            if (IS_TAURI) {
                await tauriInvoke('clear_messages_rust');
            } else {
                await apiFetch('/api/messages', { method: 'DELETE' });
            }
            chatContainer.innerHTML = '';
            appendMessage(currentAgentName, 'Conversation logs cleared. Ready.');
        }
    });

    btnSfxToggle.addEventListener('click', () => {
        voiceEngine.sfxEnabled = !voiceEngine.sfxEnabled;
        btnSfxToggle.textContent = voiceEngine.sfxEnabled ? '🔊 SFX: ON' : '🔇 SFX: OFF';
        voiceEngine.playSFX('click');
    });

    document.querySelectorAll('.quick-chip').forEach(chip => {
        chip.addEventListener('click', () => {
            const cmd = chip.getAttribute('data-cmd');
            if (cmd) handleSendMessage(cmd);
        });
    });

    // Manual avatar zoom -- picked by hand, independent of the engine's own auto-fit.
    // Lives next to the voice waveform since that's the other "how it looks while
    // running" control in this panel.
    const avatarZoomSlider = document.getElementById('avatar-zoom');
    const avatarZoomReadout = document.getElementById('avatar-zoom-readout');
    if (avatarZoomSlider) {
        avatarZoomSlider.value = String(Math.round(currentZoom * 100));
        if (avatarZoomReadout) avatarZoomReadout.textContent = `${avatarZoomSlider.value}%`;
        avatarZoomSlider.addEventListener('input', () => {
            currentZoom = Number(avatarZoomSlider.value) / 100;
            localStorage.setItem('aether_avatar_zoom', String(currentZoom));
            hologram.setZoom(currentZoom);
            if (avatarZoomReadout) avatarZoomReadout.textContent = `${avatarZoomSlider.value}%`;
        });
    }

    // Initial Startup
    applyAvatar(currentAvatar, false);
    hologram.setZoom(currentZoom);
    paintTheme(currentTheme);

    /* Nothing chosen yet means the OS is still the authority, so a switch to dark mode while
       the window is open should be followed rather than waiting for a restart. Aether1Theme
       stops calling this the moment a mode is picked. */
    Aether1Theme.followSystem(paintTheme);
    loadStaticInfo();
    loadSettings();
    // A proposal outlives the conversation that made it, so anything still waiting from a
    // previous session is put back on screen rather than quietly expiring unseen.
    //
    // Strictly after the history, never alongside it: loadChatHistory clears the container
    // when its own fetch returns, so starting both at once is a race the cards lose about
    // as often as they win -- and losing it means an approval waiting on the operator is
    // erased from the screen while the action stays pending in the database.
    loadChatHistory().then(refreshPendingApprovals);
    connectTelemetry();
    initVersionAndUpdates();
    initSpriteMode();

    document.body.addEventListener('click', () => {
        voiceEngine.playSFX('boot');
    }, { once: true });
});
