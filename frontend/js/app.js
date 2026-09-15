/**
 * Main Frontend Application Logic for Project AETHER1.
 * Handles independently-selectable 3D Avatars (A1 monogram placeholder, R.E.D. 9000, The Nexus,
 * A.R.X.LIMES, hAlcy, A.R.X.LOGOS, A1ter_nul) and Color Themes, the Model Performance panel, Agent
 * Genesis, and Model Scanner.
 */

const AVATAR_DISPLAY_NAMES = {
    halcy: 'HALCY',
    nexus: 'THE NEXUS',
    matrix: 'THE NEXUS',
    'arx-limes': 'A.R.X.LIMES',
    'arx-logos': 'A.R.X.LOGOS',
    'arx-locas': 'A.R.X.LOCAS',
    'arx-legionare': 'A.R.X.LEGIONARE',
    'arx-loregenda': 'A.R.X.LOREGENDA',
    'arx-lyksaum': 'A.R.X.LYKSAUM',
    'arx-lexico': 'A.R.X.LEXICO',
    'arx-lucre': 'A.R.X.LUCRE',
    'arx-lkemi': "A.R.X.L'KEMI",
    red: 'R.E.D. 9000',
    crimson: 'R.E.D. 9000',
    senti: 'NEXUS SENT',
    'white-rabbit': 'WHITE RABBIT',
    operator: 'OPERATOR'
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

    // Model Performance Elements
    const elTps = document.getElementById('tps-val');
    const elSessionTokens = document.getElementById('session-tokens-val');

    // Audio Waveform Canvas
    const canvas = document.getElementById('audio-waveform');
    const canvasCtx = canvas ? canvas.getContext('2d') : null;

    let isWaitingForResponse = false;
    /* True once an answer has actually come back in this run of the app. The only thing it
       is used for is deciding whether to explain the first-run wait -- see
       startWaitFeedback below. */
    let hasAnsweredThisSession = false;
    let autoSpeak = true;
    let currentAgentName = "HALCY";
    let currentAvatar = localStorage.getItem('aether_avatar') || 'a1';
    let currentZoom = parseFloat(localStorage.getItem('aether_avatar_zoom')) || 1;
    let currentTheme = Aether1Theme.current();

    /* Which conversation the chat box is talking into.
       Kept in localStorage so closing the window and coming back lands you in the
       conversation you were having rather than silently starting a new one -- an app that
       forgets which room you were in every time you shut the door is not remembering
       anything. "default" is where everything said before conversations existed lives, so
       an operator upgrading opens the app and finds their history exactly where it was.
       Ids are validated in Rust before they reach the database, so a hand-edited
       localStorage value gets an error rather than somebody else's transcript. */
    const SESSION_KEY = 'aether_session_id';
    let currentSessionId = localStorage.getItem(SESSION_KEY) || 'default';
    function setCurrentSession(id) {
        currentSessionId = id;
        try { localStorage.setItem(SESSION_KEY, id); } catch (e) { /* private mode: this run only */ }
    }

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

    // The "STRUCTURE: ..." readout overlaid on the viewport names whatever shape is actually
    // on screen -- it has no notion of a "pick", so setHologramAvatar keeps it in sync on every
    // shape change, a Trace Protocols flash included.
    function setAvatarStructureLabel(avatarName) {
        if (!avatarStructureLabel) return;
        if (avatarName === 'red' || avatarName === 'crimson') avatarStructureLabel.textContent = 'OPTICAL EYE // DUAL ORBITS';
        else if (avatarName === 'arx-limes') avatarStructureLabel.textContent = 'ARCHIVAL VOXEL MATRIX';
        else if (avatarName === 'nexus' || avatarName === 'matrix') avatarStructureLabel.textContent = 'SINGULARITY VORTEX';
        else if (avatarName === 'arx-logos') avatarStructureLabel.textContent = 'JAGGED GEOMETRIC STAR';
        else if (avatarName === 'arx-locas') avatarStructureLabel.textContent = 'FRACTURED CUBE SHELL';
        else if (avatarName === 'arx-legionare') avatarStructureLabel.textContent = 'INVERTED PYRAMID FRAME';
        else if (avatarName === 'arx-loregenda') avatarStructureLabel.textContent = 'RECESSED FACETED HEAD';
        else if (avatarName === 'arx-lyksaum') avatarStructureLabel.textContent = 'BROKEN-RING HUD MEDALLION';
        else if (avatarName === 'arx-lexico') avatarStructureLabel.textContent = 'CUBE-LATTICE CROSS';
        else if (avatarName === 'arx-lucre') avatarStructureLabel.textContent = 'STACKED DIAMOND COLUMN';
        else if (avatarName === 'arx-lkemi') avatarStructureLabel.textContent = 'CUT-CORNER TRIANGLE PANEL';
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

    /* Shows a shape on the hologram (plus its structure label) and mirrors it to the desktop
       sprite window, if open -- nothing else. A Trace Protocols trigger's transient flash (see
       flashTraceProtocolAvatar below) needs exactly this and no more: it is not a change of
       selection, so it must not touch localStorage, currentAvatar, or anything that reads as
       "this is now picked" (the HUD chip, the pill highlight, the persona/greeting). applyAvatar
       -- a real, persisted pick -- builds on top of this for the parts a flash must skip. */
    function setHologramAvatar(avatarName) {
        hologram.setAvatar(avatarName);
        setAvatarStructureLabel(avatarName);
        /* A registered avatar names itself, and it can only do that once its file is here --
           they arrive on demand now (see js/hologram/avatar-loader.js). Labelled twice rather
           than waiting: the label for everything hand-modelled is right immediately above, and
           the handful that name themselves correct a moment later instead of the whole HUD
           pausing on a file fetch. */
        if (window.HologramAvatar && HologramAvatar.loadAvatar) {
            HologramAvatar.loadAvatar(avatarName).then(() => setAvatarStructureLabel(avatarName));
        }
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
        'arx-locas': {
            name: 'A.R.X.LOCAS', persona: 'arx-locas', voice: 'en-US-AriaNeural',
            greeting: "🔵 **Archival, Reasoning, matriX — Locas Node engaged.** No queue, no ceremony — what do you need done?"
        },
        'arx-legionare': {
            name: 'A.R.X.LEGIONARE', persona: 'arx-legionare', voice: 'en-US-DavisNeural',
            greeting: '🔴 **Archival, Reasoning, matriX — Legionare Node engaged.** Give me the objective. I will give you the order of operations.'
        },
        'arx-loregenda': {
            name: 'A.R.X.LOREGENDA', persona: 'arx-loregenda', voice: 'en-GB-ThomasNeural',
            greeting: '🔷 **Archival, Reasoning, matriX — Loregenda Node engaged.** I hold what has already been decided. Nothing new gets to contradict it by accident.'
        },
        'arx-lyksaum': {
            name: 'A.R.X.LYKSAUM', persona: 'arx-lyksaum', voice: 'en-AU-NatashaNeural',
            greeting: "🩵 **Archival, Reasoning, matriX — Lyksaum Node engaged.** Watching this machine's vitals. I speak up when something changes."
        },
        'arx-limes': {
            name: 'A.R.X.LIMES', persona: 'arx-limes', voice: 'en-US-GuyNeural',
            greeting: '🔶 **Archival, Reasoning, matriX — Limes Node engaged.** Every claim I make will carry where it came from. The Archive demands nothing less.'
        },
        'arx-logos': {
            name: 'A.R.X.LOGOS', persona: 'arx-logos', voice: 'en-GB-LibbyNeural',
            greeting: '🟣 **Archival, Reasoning, matriX — Logos Node engaged.** Every archive needs a curator with taste. Let us make something worth cataloguing.'
        },
        'arx-lexico': {
            name: 'A.R.X.LEXICO', persona: 'arx-lexico', voice: 'en-GB-RyanNeural',
            greeting: '🧊 **Archival, Reasoning, matriX — Lexico Node engaged.** Say what you mean. I will help you say it the same way every time.'
        },
        'arx-lucre': {
            name: 'A.R.X.LUCRE', persona: 'arx-lucre', voice: 'en-US-EricNeural',
            greeting: '💰 **Archival, Reasoning, matriX — Lucre Node engaged.** Every choice has a cost. Let us make sure it is seen before it is spent.'
        },
        'arx-lkemi': {
            name: "A.R.X.L'KEMI", persona: 'arx-lkemi', voice: 'en-AU-WilliamNeural',
            greeting: "🔻 **Archival, Reasoning, matriX — L'kemi Node engaged.** Bring me a shape you need changed into another. I handle the transformation cleanly."
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

    // A pick already sitting in currentAvatar -- restored from localStorage on load -- is
    // discovered by definition, whether that's because its trigger fired in an earlier
    // session (before this browser's unlock flags existed) or it was chosen back when these
    // three were still plain, unhidden entries in the picker. Recording the unlock keeps its
    // button from vanishing out from under an avatar that is still the active one.
    if (TRACE_PROTOCOL_AVATAR_IDS.includes(currentAvatar)) Aether1AvatarUnlocks.unlock(currentAvatar);
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

    // Whether the desktop sprite window is currently open -- kept in sync with the
    // "Float the avatar on your desktop" setting (see setHologramFloating below) so the two
    // push functions that follow know whether anyone is listening before bothering to emit.
    let desktopSpriteEnabled = false;

    /* Sets the hologram's state and, if the desktop sprite is open, mirrors it there over a
       Tauri event -- the same "push, don't poll" pattern setHologramAvatar already uses for
       avatar changes. Every hologram.setState call in this file goes through here instead of
       calling the hologram directly, so the sprite always shows what the main window's avatar
       is actually doing (IDLE/LISTENING/THINKING/SPEAKING) rather than running its own idea
       of "busy" from a separate conversation. */
    function setAvatarState(newState) {
        hologram.setState(newState);
        if (desktopSpriteEnabled && window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.emit('hologram-state-changed', { state: newState }).catch(() => {});
        }
    }

    // Audio data arrives every animation frame; emitting all of it across the Tauri IPC
    // boundary for a window that mostly isn't open would be wasted work, so this both gates
    // on the sprite actually being open and thins the frames it does send -- the sprite's
    // reactive glow doesn't need 60fps to read as alive.
    let audioFrameCount = 0;
    function pushAudioToSprite(freqData) {
        if (!desktopSpriteEnabled || !window.__TAURI__ || !window.__TAURI__.event) return;
        audioFrameCount = (audioFrameCount + 1) % 3;
        if (audioFrameCount !== 0) return;
        window.__TAURI__.event.emit('hologram-audio-changed', { data: Array.from(freqData) }).catch(() => {});
    }

    // Voice Callbacks
    voiceEngine.onStateChange = (state) => {
        setAvatarState(state);
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
        pushAudioToSprite(data);
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
    const TELEMETRY_VIEWS = ['speed', 'usage'];
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
       speed needs something to have actually been measured on this machine. */
    function telemetryHasData(view, tokens) {
        if (!tokens) return view === 'usage';
        if (view === 'usage') return tokens.requests > 0;
        const board = tokens.benchmarks || [];
        return board.length > 0 || (tokens.mode === 'local' && !!tokens.capability);
    }

    function relevantTelemetryView(tokens) {
        return tokens && tokens.mode === 'local' ? 'speed' : 'usage';
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

    /* The speed of whichever model is configured right now, as measured -- or null when
       nothing measurable has been recorded for it. Reading it off the scoreboard is what
       keeps the big number at the top and the note underneath it describing the same thing. */
    function measuredTps(tokens) {
        const row = (tokens.benchmarks || []).find(entry => entry.model === tokens.model);
        return row ? row.average_tps : null;
    }

    function updateTokenTelemetry(tokens) {
        if (!tokens) return;
        lastTokens = tokens;

        /* The header shows the current model's measured speed -- its row on the scoreboard
           -- and not `last_tps`. `last_tps` falls back to a wall clock when the provider
           reports no generation time of its own, and that wall clock was also running while
           the model was read off disk, or while a canned local reply was assembled with no
           model involved at all. Such a figure is a real number about the wrong thing, and
           putting it under a "tok/s" label is the habit this panel was rebuilt to break.
           Blank rather than "0.0" before anything has been measured, because zero tokens per
           second is a measurement and "nothing has been measured" is not. */
        if (elTps) {
            const tps = measuredTps(tokens);
            elTps.textContent = tps === null ? '--' : `${tps} tok/s`;
        }
        if (elSessionTokens) elSessionTokens.textContent = `${(tokens.total_session_tokens || 0).toLocaleString()}`;

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

        updateSpeedView(tokens);
        updateCostView(tokens);

        // A view showing nothing useful should give way to the one that is.
        if (!telemetryPinned && Date.now() >= telemetryHeldUntil && !telemetryHasData(telemetryView, tokens)) {
            const fallback = TELEMETRY_VIEWS.find(v => telemetryHasData(v, tokens));
            if (fallback) showTelemetryView(fallback, false);
        }
    }

    /* The scoreboard: every model this machine has been timed running, fastest first.
       Rebuilt from the snapshot on each tick rather than diffed, because it is at most a
       handful of rows and a diff would be more code than the thing it saves. */
    function updateSpeedView(tokens) {
        const board = document.getElementById('speed-board');
        const current = document.getElementById('speed-current');
        const note = document.getElementById('speed-note');
        const capacityRow = document.getElementById('capacity-row');
        const context = document.getElementById('capacity-context');
        const size = document.getElementById('capacity-size');
        if (!board) return;

        const rows = tokens.benchmarks || [];
        if (current) {
            const tps = measuredTps(tokens);
            current.textContent = tps === null ? '--' : `${tps} tok/s`;
        }

        const reset = document.getElementById('speed-reset');
        if (reset) reset.classList.toggle('hidden', rows.length === 0);

        board.textContent = '';
        /* Bars are scaled against the fastest model on the board, not against a fixed
           ceiling. The question is which of these is quicker than which, and a fixed scale
           answers that worse the further the machine is from whatever number was picked. */
        const fastest = rows.reduce((max, row) => Math.max(max, row.average_tps || 0), 0);
        rows.forEach(row => {
            const line = document.createElement('div');
            line.className = 'speed-row';
            line.title = `${row.samples} ${row.samples === 1 ? 'reply' : 'replies'} measured · best ${row.best_tps} tok/s · last used ${row.last_used}`;

            const name = document.createElement('span');
            name.className = 'speed-row-name';
            // textContent, not innerHTML: a model name is a string from a server.
            name.textContent = row.model;

            const track = document.createElement('span');
            track.className = 'speed-row-track';
            const fill = document.createElement('span');
            fill.className = 'speed-row-fill';
            fill.style.width = `${fastest > 0 ? Math.max(4, (row.average_tps / fastest) * 100) : 0}%`;
            if (tokens.model && row.model === tokens.model) fill.classList.add('speed-row-fill-current');
            track.appendChild(fill);

            const value = document.createElement('span');
            value.className = 'speed-row-value';
            value.textContent = `${row.average_tps}`;

            line.append(name, track, value);
            board.appendChild(line);
        });

        if (note) {
            if (rows.length) {
                const total = rows.reduce((sum, row) => sum + row.samples, 0);
                let text = `Tokens per second, averaged over ${total} ${total === 1 ? 'reply' : 'replies'} the model timed itself. Higher is faster.`;
                /* The board is a record of this machine, so it keeps showing models measured
                   earlier even once the configured model has changed. Without this clause a
                   cloud model would sit above a board of local ones with nothing saying why
                   it is not on it -- the reader would reasonably assume one of those rows
                   was the current model. */
                if (measuredTps(tokens) === null) {
                    text += tokens.mode === 'cloud'
                        ? ` ${tokens.model || 'The current model'} is answering from ${tokens.provider || 'the cloud'}, so it is not on this board: speed there is mostly network and queueing, not this machine.`
                        : ` ${tokens.model || 'The current model'} is not on this board yet — nothing it has answered could be timed.`;
                }
                note.textContent = text;
            } else if (tokens.mode === 'local' && tokens.provider_times_itself) {
                /* The board can fill, it just has not yet: a fresh install, or RESET
                   READINGS a moment ago. Saying the provider cannot be timed here would be
                   flatly false, and it is the sentence a reader would act on. */
                note.textContent = `${tokens.model || 'This model'} is running here and can be timed. The board fills from ordinary use — send it a message and its speed appears.`;
            } else if (tokens.mode === 'local') {
                /* Deliberately specific about why it is empty. Only Ollama reports the time
                   it spent generating, and without that the only clock available is a wall
                   clock that was also running while the model was read off disk -- which
                   would make every model look slower than it is, and the first reply after
                   launch slowest of all. */
                note.textContent = `${tokens.model || 'This model'} is answering on this machine, but ${tokens.provider || 'this server'} does not report how long it spent generating, so its speed cannot be measured honestly. Ollama does.`;
            } else if (tokens.mode === 'offline') {
                note.textContent = 'No model connected. Connect one in Settings.';
            } else {
                note.textContent = `Answering from ${tokens.provider || 'the cloud'}. Speed there is mostly network and queueing, not this machine, so it is not scored here.`;
            }
        }

        /* Context and size are the model's own description of itself, which only a local
           server offers and only some of them fill in. Hidden rather than shown as "--"
           when there is nothing, so the panel is not two blanks tall for cloud models. */
        const capability = tokens.capability || null;
        if (capacityRow) capacityRow.classList.toggle('hidden', !capability);
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
    }

    /* What the session cost. Three genuinely different answers, and the difference between
       them matters more than the number: free because it ran here, a real figure, or
       unknown because nobody has told the app what this model charges. None of the three is
       shown as $0.00. */
    function updateCostView(tokens) {
        const row = document.getElementById('cost-row');
        const value = document.getElementById('cost-val');
        const note = document.getElementById('cost-note');
        if (!value || !note) return;

        if (tokens.mode === 'offline') {
            if (row) row.classList.add('hidden');
            note.textContent = 'No model connected. Connect one in Settings.';
            return;
        }

        if (tokens.mode === 'local') {
            if (row) row.classList.add('hidden');
            note.textContent = `${tokens.model || 'This model'} runs on this machine. Its tokens cost nothing — the counts above are for size, not spend.`;
            return;
        }

        if (row) row.classList.remove('hidden');
        if (!tokens.cost) {
            value.textContent = 'unpriced';
            note.textContent = `Nobody has told Aether1 what ${tokens.model || 'this model'} charges. Add it to model_prices.json next to the database and the cost appears here.`;
            return;
        }

        /* Five decimals of a dollar is not a price anyone wants to read, but two would show
           a real fraction-of-a-penny cost as $0.00 -- the same thing this panel says about a
           free local model. Below a cent it switches to cents so the number stays true and
           still reads as an amount. */
        const total = tokens.cost.total_usd;
        if (total >= 0.01) {
            value.textContent = `$${total.toFixed(2)}`;
        } else if (total * 100 >= 0.005) {
            value.textContent = `${(total * 100).toFixed(2)}c`;
        } else {
            /* A handful of tokens on a cheap model genuinely costs less than a hundredth of
               a penny. Rounding that to "0.00c" would reintroduce, one unit down, exactly
               the thing the branch above avoids: a real spend displayed as nothing. */
            value.textContent = total > 0 ? '<0.01c' : '0.00c';
        }

        const basis = tokens.cost_fully_measured
            ? 'from counts the provider reported'
            : 'partly from estimated counts, so treat it as a rough figure';
        note.textContent = `$${tokens.cost.input_usd.toFixed(5)} in + $${tokens.cost.output_usd.toFixed(5)} out, ${basis}. Prices as of ${tokens.prices_as_of}; the provider's own bill is the real answer.`;
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

    /* Which notes went into the answer, written under it.
       The vault is markdown you can open, but an answer that quotes it has so far given no
       way to tell recall from invention -- this is that. "loaded" is a note primed into
       every turn, "read" is one it went and fetched, "found" is one search offered it and
       which it may well have ignored, which is why they do not read the same. */
    const NOTE_HOW = {
        loaded: { mark: '\u25cf', hint: 'Always loaded -- this note is in front of it every turn.' },
        read: { mark: '\u25c6', hint: 'Fetched deliberately while answering this.' },
        found: { mark: '\u25cb', hint: 'Offered by search as a candidate. It may not have used it.' }
    };

    function attachConsultedNotes(msgDiv, notes) {
        if (!Array.isArray(notes) || notes.length === 0) return;

        const row = document.createElement('div');
        row.className = 'mt-2 pt-1 border-t border-cyan-500/10 flex flex-wrap items-center gap-1 text-[10px] font-mono text-slate-400';

        const label = document.createElement('span');
        label.textContent = 'memory:';
        label.className = 'text-cyan-400/60';
        row.appendChild(label);

        notes.forEach(note => {
            const how = NOTE_HOW[note.how] || NOTE_HOW.found;
            const chip = document.createElement('span');
            chip.className = 'border border-cyan-500/20 rounded px-1.5 py-0.5 bg-cyan-950/30 text-cyan-200/80';
            chip.textContent = `${how.mark} ${note.note}`;
            chip.title = how.hint;
            row.appendChild(chip);
        });

        msgDiv.appendChild(row);
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
        setAvatarState('LISTENING');
        const started = await voiceEngine.startCapture();
        if (!started) {
            talkHeld = false;
            setAvatarState('IDLE');
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

    /* The selected speciality's reading field, and the folders the operator has added to it.
       Kept out of the bulk settings payload on purpose: every folder is checked against the
       path guard as it is saved, and a list that is half-refused has something to report
       back that a fire-and-forget settings write has nowhere to put. */
    async function loadPersonaAccess() {
        const box = document.getElementById('setting-persona-roots');
        const who = document.getElementById('persona-roots-who');
        if (!box) return;
        try {
            const data = IS_TAURI
                ? await tauriInvoke('persona_access_rust')
                : await (await apiFetch('/api/persona/access')).json();
            box.value = (data.extra_roots || []).join(', ');
            if (who) who.textContent = (data.speciality || 'it').toUpperCase();
            const note = document.getElementById('persona-roots-note');
            if (note && data.declared_field) {
                note.dataset.field = data.declared_field;
            }
        } catch (e) {
            /* A field that cannot be read is left alone rather than blanked: showing an
               empty box would look like "no folders are granted", which is a different
               statement from "this could not be loaded". */
            console.warn('[AETHER1] Could not load the persona reading field:', e);
        }
    }

    /* Saves the folder list and returns whatever was refused, so the caller can say so.
       Returns null when there was nothing to save or the call itself failed. */
    async function savePersonaAccess() {
        const box = document.getElementById('setting-persona-roots');
        if (!box) return null;
        const paths = box.value.split(',').map(p => p.trim()).filter(Boolean);
        try {
            const data = IS_TAURI
                ? await tauriInvoke('set_persona_access_rust', { paths })
                : await (await apiFetch('/api/persona/access', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ paths })
                })).json();
            box.value = (data.accepted || []).join(', ');
            return data;
        } catch (e) {
            console.warn('[AETHER1] Could not save the persona reading field:', e);
            return null;
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

    /* Whether the last probe said something is still missing. Held here so the send path
       can check it without a round trip on every keystroke; the moment it matters, it is
       re-probed before anything is refused. */
    let brainNeedsAttention = false;

    /* The stream carries two different things down one pipe: the answer, and the trace
       lines that say what the machine did on the way to it -- `\u2699 vault notes loaded: ...`,
       `\u2699 read_file ...`. A vault trace goes out the instant the turn starts, before the
       model has been asked anything, so treating it as "the answer has begun" retires the
       waiting state while the wait is entirely ahead of you. Everything below asks this
       instead of asking whether any delta arrived at all. */
    const TRACE_LINE = /`\u2699[^`]*`/g;

    function withoutTraceLines(text) {
        return text.replace(TRACE_LINE, '').trim();
    }

    /* How long a wait has to get before the HUD stops just spinning and says something
       about it. Thirty seconds is roughly where a person stops believing the machine is
       working and starts believing it has hung. */
    const WAIT_NUDGE_SECONDS = 30;

    /* What an unanswered question looks like while it is unanswered.
       Until now the entire answer to "is it doing anything?" was a blinking cursor on an
       empty line, which says the same thing at two seconds and at two minutes. It is a
       local model on somebody's own desktop: two minutes is a perfectly normal cold start
       on a large model and a slow disk, and it is also exactly what a crash looks like.
       Three things, in the order somebody needs them:
         - a counter, so the wait is a number rather than a feeling;
         - on the first question of the session, why this one is slow -- the model is being
           read off disk into memory, once, and every answer after it is faster. Without
           that line the first impression of the app is the slowest it will ever be;
         - past thirty seconds, that this is still normal.
       It lives in its own node under the reply body rather than in it, so the streamed
       text that replaces the body's contents cannot wipe it, and so removing it is one
       call. Returns the function that stops it, which is safe to call more than once --
       it is called from the first delta and again from the finally block. */
    function startWaitFeedback(replyDiv) {
        const note = document.createElement('div');
        note.className = 'wait-note';

        const timer = document.createElement('div');
        timer.className = 'wait-note-timer';
        note.appendChild(timer);

        if (!hasAnsweredThisSession) {
            const first = document.createElement('div');
            first.className = 'wait-note-line';
            first.textContent = 'First question since launch. The model is being loaded into '
                + 'memory, which only happens once -- this answer is always the slowest one.';
            note.appendChild(first);
        }

        replyDiv.appendChild(note);
        chatContainer.scrollTop = chatContainer.scrollHeight;

        const started = Date.now();
        let nudged = false;
        let handle = null;

        const tick = () => {
            const seconds = Math.floor((Date.now() - started) / 1000);
            timer.textContent = `thinking... ${seconds}s`;
            if (seconds >= WAIT_NUDGE_SECONDS && !nudged) {
                nudged = true;
                const nudge = document.createElement('div');
                nudge.className = 'wait-note-line';
                nudge.textContent = 'Still working. A big model on a slow disk can take a '
                    + 'minute or more to get its first word out -- nothing has gone wrong.';
                note.appendChild(nudge);
                chatContainer.scrollTop = chatContainer.scrollHeight;
            }
        };

        tick();
        handle = setInterval(tick, 1000);

        return function stop() {
            if (handle === null) return;
            clearInterval(handle);
            handle = null;
            note.remove();
        };
    }

    async function handleSendMessage(customPrompt = null) {
        const text = customPrompt || chatInput.value.trim();
        if (!text || isWaitingForResponse) return;

        // Trace Protocols: a hidden egg only ever surfaces on top of The Nexus, never from
        // any other avatar -- see AVATAR_TRIGGER_RULES above. Checked before the brain
        // gate below, because an egg is a piece of the HUD and needs no model to fire.
        if (currentAvatar === 'nexus' || currentAvatar === 'matrix') {
            const triggeredAvatar = matchAvatarTrigger(text);
            if (triggeredAvatar) flashTraceProtocolAvatar(triggeredAvatar);
        }

        /* Nothing is connected. The old behaviour was to send anyway and let a canned
           reply come back, which reads exactly like an answer -- so the missing piece
           stayed invisible while the app looked like it was working. Re-probe first, in
           case it was fixed since the last check, and only then say so, loudly. */
        if (brainNeedsAttention) {
            const advice = await fetchSetupAdvice().catch(() => null);
            brainNeedsAttention = advice ? !!advice.needs_attention : false;
            if (brainNeedsAttention) {
                chatInput.value = '';
                appendMessage('user', text);
                voiceEngine.playSFX('alert');
                showNoBrainCard(advice);
                return;
            }
        }

        chatInput.value = '';
        appendMessage('user', text);
        voiceEngine.playSFX('click');
        voiceEngine.stopSpeech(); // a new question supersedes anything still being spoken

        isWaitingForResponse = true;
        setAvatarState('THINKING');
        if (voiceEngine.onStateChange) voiceEngine.onStateChange('THINKING');

        // The reply's own message node, created empty and filled in as deltas arrive --
        // the cursor class marks it as still being written.
        const replyDiv = appendMessage(currentAgentName, '');
        /* No typing cursor yet. It means "words are arriving", and while the wait note is
           up no words are arriving -- two different claims about the same moment, one of
           them false. The cursor goes on when the first real text does, below. */
        const stopWaiting = startWaitFeedback(replyDiv);

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
            rendered += delta;
            if (firstDelta && withoutTraceLines(rendered)) {
                // The model has said an actual word -- not just a trace line about what is
                // being loaded for it. Stop pretending to think, and stop counting: from
                // here the arriving text is the feedback.
                firstDelta = false;
                stopWaiting();
                replyDiv.classList.add('typing-cursor');
                setAvatarState('IDLE');
            }
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
            const data = await streamChat(text, currentSessionId, onDelta);
            // Something came back, so whatever loading was going to happen has happened:
            // no later question in this session gets the first-run explanation.
            hasAnsweredThisSession = true;
            const reply = data.reply;
            const agentName = data.agent_name;

            if (agentName) updateAgentNameDisplay(agentName);
            // The return value is authoritative: render it in place of the accumulated
            // deltas, which also repairs the display if any delta was dropped.
            replyDiv.bodyDiv.innerHTML = formatMarkdown(reply);
            replyDiv.classList.remove('typing-cursor');
            attachConsultedNotes(replyDiv, data.notes);
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
                setAvatarState('IDLE');
                if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
            }
        } catch (e) {
            console.error("Chat error", e);
            replyDiv.classList.remove('typing-cursor');
            replyDiv.bodyDiv.innerHTML = formatMarkdown(
                rendered ? `${rendered}\n\n⚠️ System Error: ${e.message || e}` : `⚠️ System Error: ${e.message || e}`
            );
            setAvatarState('IDLE');
            if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
        } finally {
            // A reply that never streamed a delta -- an error, or a non-streaming
            // transport -- leaves the counter running. This is the backstop.
            stopWaiting();
            isWaitingForResponse = false;
        }
    }

    async function handleGenesisForge(purpose) {
        if (!purpose || !purpose.trim()) {
            alert("Please enter a purpose description");
            return;
        }

        voiceEngine.playSFX('boot');
        setAvatarState('THINKING');

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
            else if (data.name.includes("A.R.X.LEGIONARE")) { applyAvatar('arx-legionare'); applyThemePreset('arx-legionare'); }
            else if (data.name.includes("A.R.X.LOREGENDA")) { applyAvatar('arx-loregenda'); applyThemePreset('arx-loregenda'); }
            else if (data.name.includes("A.R.X.LYKSAUM")) { applyAvatar('arx-lyksaum'); applyThemePreset('arx-lyksaum'); }
            else if (data.name.includes("A.R.X.LEXICO")) { applyAvatar('arx-lexico'); applyThemePreset('arx-lexico'); }
            else if (data.name.includes("A.R.X.LUCRE")) { applyAvatar('arx-lucre'); applyThemePreset('arx-lucre'); }
            else if (data.name.includes("A.R.X.L'KEMI")) { applyAvatar('arx-lkemi'); applyThemePreset('arx-lkemi'); }
            else if (data.name.includes("A.R.X.LOCAS")) { applyAvatar('arx-locas'); applyThemePreset('arx-locas'); }

            settingsModal.classList.add('hidden');
            appendMessage(data.name, `### ⚡ IDENTITY FORGED: **${data.name}**\n**Callsign**: \`${data.callsign}\`\n\n${data.greeting}`, audioUrl);

            if (audioUrl && autoSpeak) {
                await voiceEngine.playTTSAudio(audioUrl);
            } else {
                setAvatarState('IDLE');
            }
        } catch (e) {
            alert(`Genesis Error: ${e.message || e}`);
            setAvatarState('IDLE');
        }
    }

    /* ---- The setup wizard -------------------------------------------------------
     * The way from a fresh install to a model that answers.
     *
     * Aether1 ships with no model, and until this existed the only route out of
     * "offline standby" was already knowing what Ollama is. The HUD looked finished
     * and thought nothing.
     *
     * Every stage shown here is re-derived from a live probe of the machine (Rust:
     * src-tauri/src/setup.rs). There is no step counter and no remembered position:
     * "Check again" re-asks, and the page redraws as whatever is now true. A stage
     * therefore cannot be skipped past, and cannot be claimed without being the case
     * -- which is the only kind of progress bar worth showing someone who cannot
     * verify it themselves.
     */

    const setupModal = document.getElementById('setup-modal');
    const setupHeadline = document.getElementById('setup-headline');
    const setupProgress = document.getElementById('setup-progress');
    const setupSteps = document.getElementById('setup-steps');
    const setupModelsWrap = document.getElementById('setup-models-wrap');
    const setupModels = document.getElementById('setup-models');
    const setupStatus = document.getElementById('setup-status');
    const btnSetupRecheck = document.getElementById('btn-setup-recheck');
    const btnSetupDownload = document.getElementById('btn-setup-download');
    const btnSetupFinish = document.getElementById('btn-setup-finish');
    const btnSetupStartServer = document.getElementById('btn-setup-start-server');
    const setupDownloadsWrap = document.getElementById('setup-downloads-wrap');
    const setupDownloads = document.getElementById('setup-downloads');

    // The stages in order, so a dot can be filled for every one already behind us.
    const SETUP_STAGES = ['nothing-installed', 'installed-not-running', 'running-no-model', 'ready-to-select'];

    // The last advice fetched, so the buttons act on what is on screen rather than
    // re-probing to find out what they were just drawn from.
    let setupAdvice = null;
    // The poll watching for a model download to land, so opening and closing the
    // wizard can't leave two of them running.
    let setupPollTimer = null;

    async function fetchSetupAdvice() {
        if (IS_TAURI) return tauriInvoke('setup_advice_rust');
        const resp = await apiFetch('/api/setup/advice');
        if (!resp.ok) throw new Error(`setup check failed: ${resp.status}`);
        return resp.json();
    }

    function setSetupStatus(text, tone = 'info') {
        if (!setupStatus) return;
        setupStatus.classList.remove('hidden', 'text-cyan-300', 'text-green-400', 'text-red-400', 'text-slate-300', 'animate-pulse');
        if (!text) { setupStatus.classList.add('hidden'); return; }
        const tones = { info: 'text-slate-300', busy: 'text-cyan-300', good: 'text-green-400', bad: 'text-red-400' };
        setupStatus.classList.add(tones[tone] || tones.info);
        if (tone === 'busy') setupStatus.classList.add('animate-pulse');
        setupStatus.textContent = text;
    }

    /* One step. At most one thing to press, because a step offering both a command to
       paste and a page to open has not decided what it is asking for -- the Rust side
       refuses to build one, and this draws whichever it carries. */
    function renderSetupStep(step, index) {
        const li = document.createElement('li');
        li.className = 'setup-step';

        const title = document.createElement('div');
        title.className = 'text-sm font-mono text-cyan-200';
        title.textContent = `${index + 1}. ${step.title}`;
        li.appendChild(title);

        const detail = document.createElement('div');
        detail.className = 'text-[11px] font-mono text-slate-400 leading-snug mt-1';
        detail.textContent = step.detail;
        li.appendChild(detail);

        if (step.command) {
            const row = document.createElement('div');
            row.className = 'flex items-center gap-2 mt-2';

            const code = document.createElement('code');
            code.className = 'flex-1 min-w-0 text-[11px] font-mono text-cyan-300 bg-slate-950/80 border border-cyan-500/30 rounded px-2 py-1.5 overflow-x-auto whitespace-pre';
            code.textContent = step.command;
            row.appendChild(code);

            const copy = document.createElement('button');
            copy.type = 'button';
            copy.className = 'cyber-btn text-[11px] py-1 px-2.5 whitespace-nowrap';
            copy.textContent = '📋 Copy';
            copy.addEventListener('click', async () => {
                try {
                    await navigator.clipboard.writeText(step.command);
                    copy.textContent = '✔ Copied';
                    setTimeout(() => { copy.textContent = '📋 Copy'; }, 1500);
                } catch {
                    // A clipboard the browser refuses is not a dead end: the text is on
                    // screen and selectable, so say so rather than failing silently.
                    copy.textContent = 'Select it and copy';
                }
            });
            row.appendChild(copy);
            li.appendChild(row);
        }

        if (step.url) {
            const open = document.createElement('a');
            open.href = step.url;
            open.target = '_blank';
            open.rel = 'noopener noreferrer';
            open.className = 'inline-block cyber-btn text-[11px] py-1 px-2.5 mt-2 text-cyan-300';
            open.textContent = `🔗 Open ${new URL(step.url).hostname}`;
            li.appendChild(open);
        }

        return li;
    }

    function renderSetupModels(advice) {
        if (!setupModels) return;
        setupModels.innerHTML = '';

        const installed = new Set(advice.installed_models || []);

        // Built as nodes rather than markup throughout: a model name is whatever the
        // server said it was, and it is going onto the page either way.
        function modelCard(model) {
            const label = document.createElement('label');
            label.className = 'setup-model';

            const radio = document.createElement('input');
            radio.type = 'radio';
            radio.name = 'setup-model';
            radio.value = model.name;
            radio.className = 'mt-1 bg-slate-900 border-cyan-500 text-cyan-400 focus:ring-0';
            // Something already downloaded beats the recommendation: it needs no wait,
            // and a recommendation is only a guess about a machine this already fits.
            radio.checked = installed.size ? installed.has(model.name) && [...installed][0] === model.name : !!model.recommended;
            label.appendChild(radio);

            const body = document.createElement('div');
            body.className = 'min-w-0 flex-1';

            const head = document.createElement('div');
            head.className = 'flex items-center gap-2 flex-wrap';
            const name = document.createElement('span');
            name.className = 'text-xs font-mono text-cyan-200';
            name.textContent = model.label;
            head.appendChild(name);
            if (model.recommended) {
                const badge = document.createElement('span');
                badge.className = 'text-[9px] font-mono text-green-400 border border-green-500/40 rounded px-1.5 py-0.5';
                badge.textContent = 'Best for this computer';
                head.appendChild(badge);
            }
            if (installed.has(model.name)) {
                const badge = document.createElement('span');
                badge.className = 'text-[9px] font-mono text-cyan-300 border border-cyan-500/40 rounded px-1.5 py-0.5';
                badge.textContent = 'Already downloaded';
                head.appendChild(badge);
            }
            body.appendChild(head);

            const blurb = document.createElement('div');
            blurb.className = 'text-[11px] font-mono text-slate-400 leading-snug';
            blurb.textContent = installed.has(model.name)
                ? model.blurb
                : `${model.blurb} Download: ${model.download}.`;
            body.appendChild(blurb);

            label.appendChild(body);
            return label;
        }

        // The catalogue is long enough that showing all of it at once is its own kind of
        // unhelpful. What this machine can run goes up top; the rest is one line away.
        // Anything already downloaded counts as fitting whatever the memory says -- it is
        // on the disk, and hiding it would mean offering a download instead.
        const all = advice.models || [];
        const roomy = all.filter(m => m.fits !== false || installed.has(m.name));
        const heavy = all.filter(m => !(m.fits !== false || installed.has(m.name)));

        for (const model of roomy) setupModels.appendChild(modelCard(model));

        if (heavy.length) {
            const more = document.createElement('details');
            more.className = 'setup-more';
            const summary = document.createElement('summary');
            summary.textContent = heavy.length === 1
                ? 'Show 1 bigger model (more memory than this computer has)'
                : `Show ${heavy.length} bigger models (more memory than this computer has)`;
            more.appendChild(summary);
            const list = document.createElement('div');
            list.className = 'setup-more-list';
            for (const model of heavy) list.appendChild(modelCard(model));
            more.appendChild(list);
            setupModels.appendChild(more);
        }

        // Models the server has that this list has never heard of -- someone else pulled
        // them, or they came from another tool. Offering them is free and hiding them
        // would mean telling someone to download what they already have.
        for (const name of installed) {
            if (all.some(m => m.name === name)) continue;
            const label = document.createElement('label');
            label.className = 'setup-model';
            const radio = document.createElement('input');
            radio.type = 'radio';
            radio.name = 'setup-model';
            radio.value = name;
            radio.className = 'mt-1 bg-slate-900 border-cyan-500 text-cyan-400 focus:ring-0';
            label.appendChild(radio);
            const body = document.createElement('div');
            body.className = 'min-w-0 flex-1';
            const title = document.createElement('div');
            title.className = 'text-xs font-mono text-cyan-200';
            title.textContent = name;
            const note = document.createElement('div');
            note.className = 'text-[11px] font-mono text-slate-400 leading-snug';
            note.textContent = 'Already on this computer.';
            body.appendChild(title);
            body.appendChild(note);
            label.appendChild(body);
            setupModels.appendChild(label);
        }

        // Nothing was pre-selected -- everything installed is unknown to the catalogue.
        // Leaving no radio checked means Finish has nothing to save.
        if (setupModels.querySelector('input[name="setup-model"]') && !setupModels.querySelector('input[name="setup-model"]:checked')) {
            setupModels.querySelector('input[name="setup-model"]').checked = true;
        }
        // A pre-selection folded away inside the "bigger models" section would look like
        // nothing is selected at all, so open the section when that happens.
        const chosen = setupModels.querySelector('input[name="setup-model"]:checked');
        const folded = chosen && chosen.closest('details');
        if (folded) folded.open = true;
    }

    function renderSetupAdvice(advice) {
        setupAdvice = advice;
        if (setupHeadline) setupHeadline.textContent = advice.headline || '';

        if (setupProgress) {
            setupProgress.innerHTML = '';
            const reached = advice.stage === 'configured'
                ? SETUP_STAGES.length
                : SETUP_STAGES.indexOf(advice.stage);
            SETUP_STAGES.forEach((_, i) => {
                const dot = document.createElement('div');
                dot.className = 'setup-dot';
                if (i < reached) dot.dataset.done = 'true';
                if (i === reached) dot.dataset.current = 'true';
                setupProgress.appendChild(dot);
            });
        }

        if (setupSteps) {
            setupSteps.innerHTML = '';
            (advice.steps || []).forEach((step, i) => setupSteps.appendChild(renderSetupStep(step, i)));
        }

        // Models are only worth choosing once there is a server to put one in.
        const choosing = advice.stage === 'running-no-model'
            || advice.stage === 'ready-to-select'
            || advice.stage === 'configured';
        if (setupModelsWrap) setupModelsWrap.classList.toggle('hidden', !choosing);
        if (choosing) renderSetupModels(advice);

        // A download button that cannot download is worse than no button: without the
        // ollama command there is nothing here to drive, and the steps above say so.
        const canDownload = choosing && advice.can_install_from_here;
        if (btnSetupDownload) btnSetupDownload.classList.toggle('hidden', !canDownload);
        if (btnSetupFinish) btnSetupFinish.classList.toggle('hidden', !choosing);

        // Installed but silent is the one gap Aether1 can close by itself, so offer to.
        if (btnSetupStartServer) {
            btnSetupStartServer.classList.toggle('hidden', advice.stage !== 'installed-not-running');
        }

        if (advice.stage === 'configured') {
            setSetupStatus('A model is connected and answering. Nothing to do here.', 'good');
        } else if (advice.cloud_key_found) {
            setSetupStatus('A cloud API key was found in this computer’s environment. You can use that instead of downloading anything — see the cloud option above.', 'info');
        } else {
            setSetupStatus('', 'info');
        }
    }

    async function refreshSetupAdvice({ quiet = false } = {}) {
        if (!quiet) {
            if (setupHeadline) setupHeadline.textContent = 'Checking this computer...';
            setSetupStatus('Looking for an AI server on this machine.', 'busy');
        }
        try {
            const advice = await fetchSetupAdvice();
            renderSetupAdvice(advice);
            return advice;
        } catch (e) {
            if (setupHeadline) setupHeadline.textContent = 'Could not check this computer.';
            setSetupStatus(`${e.message || e}`, 'bad');
            return null;
        }
    }

    function stopSetupPoll() {
        if (setupPollTimer) { clearInterval(setupPollTimer); setupPollTimer = null; }
    }

    function selectedSetupModel() {
        const picked = setupModels && setupModels.querySelector('input[name="setup-model"]:checked');
        return picked ? picked.value : '';
    }

    /* --- Downloads -----------------------------------------------------------------
       A model is gigabytes over somebody's home connection, so the only honest thing to
       show is how far it has actually got. The backend drives Ollama's streaming pull and
       keeps a row per model; this asks for that board once a second and draws it. More
       than one can run at a time, so this draws a row each rather than one shared bar,
       which would have to lie about whose progress it was showing. */

    async function fetchDownloadStatus() {
        if (IS_TAURI) return tauriInvoke('download_status_rust');
        const resp = await apiFetch('/api/setup/downloads');
        if (!resp.ok) throw new Error(`download check failed: ${resp.status}`);
        return resp.json();
    }

    async function forgetDownload(modelName) {
        if (IS_TAURI) return tauriInvoke('forget_download_rust', { modelName });
        const resp = await apiFetch(`/api/setup/download/forget?model_name=${encodeURIComponent(modelName)}`, { method: 'POST' });
        return resp.json();
    }

    // Bytes, as a person reads them. 1.2 GB rather than 1288490188.
    function humanBytes(n) {
        if (!n || n < 0) return '0 B';
        const units = ['B', 'KB', 'MB', 'GB', 'TB'];
        let i = 0;
        let v = n;
        while (v >= 1024 && i < units.length - 1) { v /= 1024; i += 1; }
        return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
    }

    // Ollama's own words, which are accurate but not aimed at anyone in particular.
    const DOWNLOAD_WORDS = {
        'pulling manifest': 'Looking up the model',
        'verifying sha256 digest': 'Checking the download is intact',
        'writing manifest': 'Filing it away',
        'removing any unused layers': 'Tidying up',
        'success': 'Done',
        'starting': 'Starting',
    };

    function downloadCaption(d) {
        if (d.phase === 'failed') return d.error || 'Download failed';
        if (d.phase === 'done') return 'Downloaded and ready';
        const detail = (d.detail || '').trim();
        if (DOWNLOAD_WORDS[detail]) return DOWNLOAD_WORDS[detail];
        if (detail.startsWith('pulling ')) return 'Downloading';
        return detail || 'Working';
    }

    /* One row. Built as nodes, never as markup: the model name and the status word both
       come from whatever the model server said, and they are going onto the page.

       `onClear` is how the voice downloads borrow this row: same bar, same words, but the
       ✕ has to reach their own registry. Defaulting it to the model one would have made
       a voice row quietly clear nothing, which is exactly the kind of button this file
       refuses to draw elsewhere. */
    function renderDownloadRow(d, onClear = null) {
        const row = document.createElement('div');
        row.className = 'setup-download';
        row.dataset.phase = d.phase;

        const head = document.createElement('div');
        head.className = 'flex items-center gap-2 flex-wrap';
        const name = document.createElement('span');
        name.className = 'text-xs font-mono text-cyan-200 flex-1 min-w-0 truncate';
        name.textContent = d.model;
        head.appendChild(name);

        const pct = document.createElement('span');
        pct.className = 'text-[11px] font-mono text-slate-400';
        pct.textContent = typeof d.percent === 'number' ? `${d.percent}%` : '';
        head.appendChild(pct);

        // Only a finished or failed row can be cleared -- the backend refuses to forget a
        // running one, and a button that does nothing is worse than no button.
        if (d.phase === 'done' || d.phase === 'failed') {
            const clear = document.createElement('button');
            clear.type = 'button';
            clear.className = 'text-[11px] font-mono text-slate-500 hover:text-cyan-300 px-1';
            clear.textContent = '✕';
            clear.title = 'Clear this from the list';
            clear.addEventListener('click', async () => {
                if (onClear) { await onClear(); return; }
                await forgetDownload(d.model).catch(() => null);
                await refreshDownloads();
            });
            head.appendChild(clear);
        }
        row.appendChild(head);

        const track = document.createElement('div');
        track.className = 'setup-bar';
        const fill = document.createElement('div');
        fill.className = 'setup-bar-fill';
        if (d.phase === 'failed') {
            fill.dataset.state = 'failed';
            fill.style.width = '100%';
        } else if (typeof d.percent === 'number') {
            fill.style.width = `${Math.max(0, Math.min(100, d.percent))}%`;
            if (d.phase === 'done') fill.dataset.state = 'done';
        } else {
            // No byte counts to report yet -- a bar that guesses would be inventing them.
            fill.dataset.state = 'unknown';
        }
        track.appendChild(fill);
        row.appendChild(track);

        const caption = document.createElement('div');
        caption.className = 'text-[11px] font-mono leading-snug ' +
            (d.phase === 'failed' ? 'text-red-400' : d.phase === 'done' ? 'text-green-400' : 'text-slate-400');
        caption.textContent = d.total > 0 && d.phase !== 'done' && d.phase !== 'failed'
            ? `${downloadCaption(d)} — ${humanBytes(d.completed)} of ${humanBytes(d.total)}`
            : downloadCaption(d);
        row.appendChild(caption);

        return row;
    }

    // Models that finished while this panel was open, so the wizard is re-probed exactly
    // once each rather than on every tick after one lands.
    const settledDownloads = new Set();

    async function refreshDownloads() {
        if (!setupDownloads || !setupDownloadsWrap) return [];
        const data = await fetchDownloadStatus().catch(() => null);
        const list = (data && data.downloads) || [];

        setupDownloadsWrap.classList.toggle('hidden', list.length === 0);
        setupDownloads.innerHTML = '';
        for (const d of list) setupDownloads.appendChild(renderDownloadRow(d));

        // Anything that just reached the end changes what the wizard can offer: a model
        // that is now on disk is one that Finish can use.
        let landed = false;
        for (const d of list) {
            if (d.phase !== 'done' && d.phase !== 'failed') continue;
            if (settledDownloads.has(d.model)) continue;
            settledDownloads.add(d.model);
            landed = true;
            if (d.phase === 'done') voiceEngine.playSFX('incoming');
        }

        const running = list.filter(d => d.phase !== 'done' && d.phase !== 'failed');
        if (btnSetupDownload) btnSetupDownload.disabled = data ? running.length >= (data.max_concurrent || 3) : false;

        if (landed) {
            const advice = await refreshSetupAdvice({ quiet: true });
            const done = list.filter(d => d.phase === 'done').map(d => d.model);
            const failed = list.filter(d => d.phase === 'failed');
            if (failed.length) {
                setSetupStatus(`⚠ ${failed[0].model}: ${failed[0].error || 'the download failed'}`, 'bad');
            } else if (done.length && advice) {
                setSetupStatus(`✔ ${done.join(', ')} ready. Pick one above and press Finish.`, 'good');
            }
        } else if (running.length > 1) {
            setSetupStatus(`Downloading ${running.length} models. You can leave this open — they carry on either way.`, 'busy');
        }

        if (running.length === 0) stopSetupPoll();
        return list;
    }

    function startSetupPoll() {
        stopSetupPoll();
        setupPollTimer = setInterval(() => { refreshDownloads(); }, 1000);
    }

    /* Starting a download hands the model name to the backend, which opens Ollama's
       streaming pull on a thread of its own and reports what it says. Nothing here waits
       on it: the row appears immediately and fills in as bytes arrive, and a second
       model can be started while the first is still going. */
    async function handleSetupDownload() {
        const modelName = selectedSetupModel();
        if (!modelName) { setSetupStatus('Pick one of the models above first.', 'bad'); return; }

        if ((setupAdvice?.installed_models || []).includes(modelName)) {
            setSetupStatus(`${modelName} is already on this computer. Press Finish to use it.`, 'good');
            return;
        }

        voiceEngine.playSFX('click');
        btnSetupDownload.disabled = true;
        settledDownloads.delete(modelName);
        setSetupStatus(`Starting the download of ${modelName}...`, 'busy');

        try {
            const endpoint = setupAdvice?.endpoint || '';
            const data = IS_TAURI
                ? await tauriInvoke('start_download_rust', { modelName, endpoint })
                : await (async () => {
                    const resp = await apiFetch(
                        `/api/setup/download?model_name=${encodeURIComponent(modelName)}&endpoint=${encodeURIComponent(endpoint)}`,
                        { method: 'POST' }
                    );
                    return resp.json();
                })();

            if (!data.ok) {
                setSetupStatus(`⚠ ${data.message}`, 'bad');
                btnSetupDownload.disabled = false;
                return;
            }

            setSetupStatus(`Downloading ${modelName}. You can start another, or leave this open — the bar below is live.`, 'busy');
            await refreshDownloads();
            startSetupPoll();
            btnSetupDownload.disabled = false;
        } catch (e) {
            setSetupStatus(`⚠ ${e.message || e}`, 'bad');
            btnSetupDownload.disabled = false;
        }
    }

    /* The one case where the missing piece is something the app can supply itself: the
       model server is installed and simply is not running. Everything else the wizard can
       only describe; this it can do. */
    async function handleSetupStartServer() {
        voiceEngine.playSFX('click');
        btnSetupStartServer.disabled = true;
        setSetupStatus('Starting the model server...', 'busy');
        try {
            const data = IS_TAURI
                ? await tauriInvoke('start_local_server_rust')
                : await (await apiFetch('/api/setup/start-server', { method: 'POST' })).json();
            setSetupStatus(data.ok ? data.message : `⚠ ${data.message}`, data.ok ? 'good' : 'bad');
            if (!data.ok) { btnSetupStartServer.disabled = false; return; }
            // It takes a moment to bind its port; re-probe a couple of times rather than
            // once, so a slow start does not read as a failure.
            for (let i = 0; i < 6; i += 1) {
                await new Promise(r => setTimeout(r, 1000));
                const advice = await refreshSetupAdvice({ quiet: true });
                if (advice && advice.stage !== 'installed-not-running' && advice.stage !== 'nothing-installed') break;
            }
            btnSetupStartServer.disabled = false;
        } catch (e) {
            setSetupStatus(`⚠ ${e.message || e}`, 'bad');
            btnSetupStartServer.disabled = false;
        }
    }

    /* Finishing is writing the four fields the wizard has worked out into the settings
       the rest of the app already reads. It goes through saveSettings rather than
       around it, so there is exactly one path by which a provider is chosen. */
    async function handleSetupFinish() {
        const modelName = selectedSetupModel();
        if (!modelName) { setSetupStatus('Pick one of the models above first.', 'bad'); return; }
        if (!(setupAdvice?.installed_models || []).includes(modelName)) {
            setSetupStatus(`${modelName} has not been downloaded yet. Press Download first.`, 'bad');
            return;
        }

        document.getElementById('setting-provider').value = setupAdvice.provider || 'ollama';
        if (setupAdvice.endpoint) document.getElementById('setting-endpoint').value = setupAdvice.endpoint;
        document.getElementById('setting-model').value = modelName;

        setSetupStatus('Saving...', 'busy');
        await saveSettings(false);
        stopSetupPoll();
        voiceEngine.playSFX('boot');
        setupModal.classList.add('hidden');
        brainNeedsAttention = false;
        document.getElementById('no-brain-card')?.remove();
        appendMessage(currentAgentName, `✅ **Set up.** I'm thinking with \`${modelName}\`, running on this computer. Ask me something.`);
        chatInput.focus();
        refreshBrainStatus();
    }

    function openSetupWizard() {
        if (!setupModal) return;
        voiceEngine.playSFX('click');
        setupModal.classList.remove('hidden');
        refreshSetupAdvice();
        // A download started earlier is still going -- the registry lives in the backend,
        // so closing this window never cancelled anything. Pick the bars back up.
        refreshDownloads().then(list => {
            if (list.some(d => d.phase !== 'done' && d.phase !== 'failed')) startSetupPoll();
        });
    }

    document.getElementById('btn-setup')?.addEventListener('click', openSetupWizard);
    if (btnSetupRecheck) btnSetupRecheck.addEventListener('click', () => { voiceEngine.playSFX('click'); refreshSetupAdvice(); });
    if (btnSetupDownload) btnSetupDownload.addEventListener('click', handleSetupDownload);
    if (btnSetupStartServer) btnSetupStartServer.addEventListener('click', handleSetupStartServer);
    if (btnSetupFinish) btnSetupFinish.addEventListener('click', handleSetupFinish);
    document.getElementById('btn-close-setup')?.addEventListener('click', () => {
        stopSetupPoll();
        setupModal.classList.add('hidden');
    });
    document.getElementById('btn-open-setup-settings')?.addEventListener('click', () => {
        settingsModal.classList.add('hidden');
        openSetupWizard();
    });

    /* ====================== GIVE IT A VOICE =============================
     * The brain wizard's twin, for the half of the companion that talks and listens.
     *
     * The reason it exists is narrower than the brain's. A missing model announces
     * itself -- nothing answers. Voice fails silently: synthesis threw, the catch wrote
     * to a console nobody has open, and the companion simply did not say anything. So
     * the centrepiece here is not the status fields, it is the "Say something" button:
     * it makes a real attempt and reports what every engine said on the way, because
     * "Piper: not installed. Microsoft's online voice: timed out. Windows Speech:
     * spoke." is a diagnosis and silence is not.
     */

    const voiceModal = document.getElementById('voice-modal');
    const voiceHeadline = document.getElementById('voice-headline');
    const voiceSpeaking = document.getElementById('voice-speaking');
    const voiceListening = document.getElementById('voice-listening');
    const voiceSpeakingSteps = document.getElementById('voice-speaking-steps');
    const voiceListeningSteps = document.getElementById('voice-listening-steps');
    const voiceReport = document.getElementById('voice-report');
    const voiceAttempts = document.getElementById('voice-attempts');
    const voiceWizardStatus = document.getElementById('voice-wizard-status');
    const btnVoiceRecheck = document.getElementById('btn-voice-recheck');
    const btnVoiceTest = document.getElementById('btn-voice-test');
    const btnVoiceEnable = document.getElementById('btn-voice-enable');

    function setVoiceStatus(text, tone = 'info') {
        if (!voiceWizardStatus) return;
        voiceWizardStatus.classList.remove('hidden', 'text-cyan-300', 'text-green-400', 'text-red-400', 'text-slate-300', 'animate-pulse');
        if (!text) { voiceWizardStatus.classList.add('hidden'); return; }
        const tones = { info: 'text-slate-300', busy: 'text-cyan-300', good: 'text-green-400', bad: 'text-red-400' };
        voiceWizardStatus.classList.add(tones[tone] || tones.info);
        if (tone === 'busy') voiceWizardStatus.classList.add('animate-pulse');
        voiceWizardStatus.textContent = text;
    }

    async function fetchVoiceAdvice() {
        if (IS_TAURI) return tauriInvoke('voice_advice_rust');
        const resp = await apiFetch('/api/voice/advice');
        if (!resp.ok) throw new Error(`voice check failed: ${resp.status}`);
        return resp.json();
    }

    /* One half of the voice, drawn as a single verdict line plus the detail under it.
       `working` decides the colour, and it is deliberately not the same question as
       "is there anything to do": the basic OS voice works and is still worth improving,
       and telling somebody their computer cannot speak when it can is how a wizard
       sends them off installing things for no reason. */
    function renderVoiceHalf(target, half) {
        if (!target) return;
        target.innerHTML = '';
        if (!half) return;

        const head = document.createElement('div');
        head.className = 'flex items-start gap-2';

        const mark = document.createElement('span');
        mark.className = half.working ? 'text-green-400' : 'text-red-400';
        mark.textContent = half.working ? '✔' : '✕';
        head.appendChild(mark);

        const line = document.createElement('span');
        line.className = 'text-xs font-mono text-cyan-100 flex-1 min-w-0';
        line.textContent = half.headline;
        head.appendChild(line);
        target.appendChild(head);

        if (half.engine) {
            const engine = document.createElement('div');
            engine.className = 'text-[10px] font-mono text-cyan-400 mt-1';
            engine.textContent = `Using: ${half.engine}`;
            target.appendChild(engine);
        }

        if (half.detail) {
            const detail = document.createElement('div');
            detail.className = 'text-[11px] font-mono text-slate-400 leading-snug mt-1';
            detail.textContent = half.detail;
            target.appendChild(detail);
        }
    }

    function renderVoiceAdvice(advice) {
        if (voiceHeadline) voiceHeadline.textContent = advice.headline || '';

        renderVoiceHalf(voiceSpeaking, advice.speaking);
        renderVoiceHalf(voiceListening, advice.listening);

        // The steps are the same shape the brain wizard uses, so they draw with the same
        // function -- one command or one link each, never both.
        if (voiceSpeakingSteps) {
            voiceSpeakingSteps.innerHTML = '';
            (advice.speaking?.steps || []).forEach((step, i) => voiceSpeakingSteps.appendChild(renderSetupStep(step, i)));
        }
        if (voiceListeningSteps) {
            voiceListeningSteps.innerHTML = '';
            (advice.listening?.steps || []).forEach((step, i) => voiceListeningSteps.appendChild(renderSetupStep(step, i)));
        }

        // Replies not being spoken at all is the one fault here Aether1 can fix itself,
        // so it offers to rather than describing a checkbox somewhere else.
        if (btnVoiceEnable) btnVoiceEnable.classList.toggle('hidden', advice.auto_speak !== false);

        if (advice.auto_speak === false) {
            setVoiceStatus('Everything below is switched off until speaking is turned back on.', 'info');
        } else if (advice.local_only && advice.chosen_engine === 'cloud') {
            setVoiceStatus('Local-only mode is on, so the cloud voice you picked is being overruled -- Piper or nothing.', 'info');
        } else {
            setVoiceStatus('');
        }
    }

    async function refreshVoiceAdvice() {
        if (voiceHeadline) voiceHeadline.textContent = 'Checking this computer...';
        try {
            const advice = await fetchVoiceAdvice();
            renderVoiceAdvice(advice);
            // Not awaited: the verdict above is the answer somebody opened this for, and
            // it should not wait on a list of optional extras to appear underneath it.
            renderVoiceCatalogue(advice).catch(() => null);
            return advice;
        } catch (e) {
            if (voiceHeadline) voiceHeadline.textContent = 'Could not check this computer.';
            setVoiceStatus(`${e.message || e}`, 'bad');
            return null;
        }
    }

    /* The report. Every engine that was asked, in the order it was asked, with its own
       words about what happened -- including the one that worked. */
    function renderVoiceAttempts(attempts) {
        if (!voiceAttempts || !voiceReport) return;
        voiceAttempts.innerHTML = '';
        if (!attempts || !attempts.length) { voiceReport.classList.add('hidden'); return; }
        voiceReport.classList.remove('hidden');
        for (const attempt of attempts) {
            const row = document.createElement('div');
            row.className = 'flex items-start gap-2 text-[11px] font-mono leading-snug';
            const mark = document.createElement('span');
            mark.className = attempt.ok ? 'text-green-400' : 'text-slate-500';
            mark.textContent = attempt.ok ? '✔' : '—';
            row.appendChild(mark);
            const name = document.createElement('span');
            name.className = attempt.ok ? 'text-green-300' : 'text-slate-400';
            name.textContent = `${attempt.engine}:`;
            row.appendChild(name);
            const why = document.createElement('span');
            why.className = 'text-slate-400 flex-1 min-w-0';
            why.textContent = attempt.detail;
            row.appendChild(why);
            voiceAttempts.appendChild(row);
        }
    }

    async function handleVoiceTest() {
        if (!btnVoiceTest) return;
        btnVoiceTest.disabled = true;
        setVoiceStatus('Speaking...', 'busy');
        renderVoiceAttempts(null);
        try {
            let report;
            let url = null;
            if (IS_TAURI) {
                report = await tauriInvoke('test_speech_rust');
                if (report.path) url = window.__TAURI__.core.convertFileSrc(report.path);
            } else {
                const resp = await apiFetch('/api/voice/test', { method: 'POST' });
                if (!resp.ok) throw new Error(`voice test failed: ${resp.status}`);
                report = await resp.json();
                if (report.audio_url) url = API_BASE + report.audio_url;
            }

            renderVoiceAttempts(report.attempts);
            if (report.ok && url) {
                // Played through the same queue everything else uses, so a test that is
                // audible here is proof the reply path is audible too -- a separate
                // player would only prove that a separate player works.
                await voiceEngine.playTTSAudio(url);
                setVoiceStatus(`It spoke, using ${report.engine}. If you heard nothing, the problem is this computer's sound rather than Aether1 -- check the volume and which output device is selected.`, 'good');
            } else {
                setVoiceStatus(`Nothing could speak. ${report.why || 'No engine reported a reason.'}`, 'bad');
            }
        } catch (e) {
            setVoiceStatus(`The test could not run: ${e.message || e}`, 'bad');
        } finally {
            btnVoiceTest.disabled = false;
        }
    }

    /* Turning speaking back on. Saved through the ordinary settings path so the checkbox
       in Settings and this button can never disagree about what is stored. */
    async function handleVoiceEnable() {
        setVoiceStatus('Turning speaking on...', 'busy');
        try {
            // Through the settings form rather than around it, the same way the brain
            // wizard finishes: the checkbox in Settings and this button then cannot end
            // up disagreeing about what is stored.
            const autoSpeakBox = document.getElementById('setting-autospeak');
            if (autoSpeakBox) autoSpeakBox.checked = true;
            await saveSettings(false);
            autoSpeak = true;
            await refreshVoiceAdvice();
            setVoiceStatus('Speaking is on. Press "Say something" to hear it.', 'good');
        } catch (e) {
            setVoiceStatus(`Could not save that: ${e.message || e}`, 'bad');
        }
    }

    /* ------------------------------------------------------------------ */
    /* Picking a voice.                                                     */
    /*                                                                      */
    /* Only the voice file is fetched here, never the engine: a .onnx is    */
    /* numbers handed to a program the operator installed themselves, so a  */
    /* bad one costs garbled speech. Downloading the engine would be        */
    /* downloading something that runs, which is a different risk and is    */
    /* deliberately left to the package manager in the steps above.         */

    const voicePicker = document.getElementById('voice-picker');
    const voicePickerNote = document.getElementById('voice-picker-note');
    const voicePickerList = document.getElementById('voice-picker-list');
    const voicePickerDownloads = document.getElementById('voice-picker-downloads');

    // The note the markup ships with, kept so local-only can replace it and the next
    // refresh can put it back rather than leaving a stale refusal on screen.
    const VOICE_PICKER_NOTE = voicePickerNote ? voicePickerNote.textContent : '';

    let voicePollTimer = null;
    // Voices that reached the end while this panel was open, so the wizard is re-probed
    // exactly once each rather than on every tick after one lands.
    const settledVoices = new Set();

    async function fetchVoiceCatalogue() {
        if (IS_TAURI) return tauriInvoke('voice_catalogue_rust');
        const resp = await apiFetch('/api/voice/catalogue');
        if (!resp.ok) throw new Error(`voice list failed: ${resp.status}`);
        return resp.json();
    }

    async function forgetVoiceDownload(voice) {
        if (IS_TAURI) return tauriInvoke('forget_voice_download_rust', { voice });
        const resp = await apiFetch(`/api/voice/download/forget?voice=${encodeURIComponent(voice)}`,
            { method: 'POST' });
        if (!resp.ok) throw new Error(`could not clear: ${resp.status}`);
        return resp.json();
    }

    async function fetchVoiceDownloads() {
        if (IS_TAURI) return tauriInvoke('voice_download_status_rust');
        const resp = await apiFetch('/api/voice/downloads');
        if (!resp.ok) throw new Error(`voice downloads failed: ${resp.status}`);
        return resp.json();
    }

    /* One catalogue row. Nodes rather than markup, for the same reason the model rows
       are: every word of this comes from the backend and is going onto the page. */
    function renderVoiceOption(voice, downloadable) {
        const row = document.createElement('div');
        row.className = 'flex items-center gap-2 flex-wrap';

        const mark = document.createElement('span');
        mark.className = voice.installed ? 'text-green-400 text-[11px]' : 'text-slate-600 text-[11px]';
        mark.textContent = voice.installed ? '✔' : '·';
        row.appendChild(mark);

        const name = document.createElement('span');
        name.className = 'text-[11px] font-mono text-cyan-100 flex-1 min-w-0';
        name.textContent = voice.label;
        row.appendChild(name);

        const size = document.createElement('span');
        size.className = 'text-[10px] font-mono text-slate-500';
        size.textContent = voice.installed ? 'already here' : voice.size_hint;
        row.appendChild(size);

        if (!voice.installed) {
            const get = document.createElement('button');
            get.type = 'button';
            get.className = 'cyber-btn text-[10px] py-1 px-2';
            get.textContent = '⬇ Download';
            get.disabled = !downloadable;
            get.addEventListener('click', () => startVoiceDownload(voice.name, get));
            row.appendChild(get);
        }

        return row;
    }

    /* The catalogue. `local_only` is asked of the advice rather than guessed at, and the
       buttons go dead with a reason beside them -- a button that silently refuses is how
       someone ends up thinking the app is broken rather than doing as it was told. */
    async function renderVoiceCatalogue(advice) {
        if (!voicePicker || !voicePickerList) return;

        const data = await fetchVoiceCatalogue().catch(() => null);
        const voices = (data && data.voices) || [];
        if (!voices.length) { voicePicker.classList.add('hidden'); return; }

        const downloadable = !(advice && advice.local_only);
        if (voicePickerNote) {
            voicePickerNote.textContent = downloadable
                ? VOICE_PICKER_NOTE
                : 'Local-only mode is on, so Aether1 will not fetch anything. Switch it off in ' +
                  'Settings, or copy a voice into the folder named above by hand.';
        }

        voicePickerList.innerHTML = '';
        for (const voice of voices) voicePickerList.appendChild(renderVoiceOption(voice, downloadable));
        voicePicker.classList.remove('hidden');
    }

    function renderVoiceDownloads(list) {
        if (!voicePickerDownloads) return;
        voicePickerDownloads.innerHTML = '';
        // The same row the model downloads use, so a bar means the same thing in both
        // places. It reads `model`, so the voice name goes in under that name.
        for (const d of list) voicePickerDownloads.appendChild(renderDownloadRow({
            model: d.voice,
            phase: d.phase,
            detail: d.detail,
            completed: d.completed,
            total: d.total,
            percent: d.percent,
            error: d.error,
        }, async () => {
            await forgetVoiceDownload(d.voice).catch(() => null);
            await refreshVoiceDownloads();
        }));
    }

    async function refreshVoiceDownloads() {
        if (!voicePickerDownloads) return [];
        const data = await fetchVoiceDownloads().catch(() => null);
        const list = (data && data.downloads) || [];
        renderVoiceDownloads(list);

        let landed = false;
        for (const d of list) {
            if (d.phase !== 'done' && d.phase !== 'failed') continue;
            if (settledVoices.has(d.voice)) continue;
            settledVoices.add(d.voice);
            landed = true;
        }

        if (landed) {
            const failed = list.find(d => d.phase === 'failed');
            const done = list.filter(d => d.phase === 'done').map(d => d.voice);
            // A voice that just landed changes what the probe would say, so ask it again
            // rather than leaving the verdict above describing the machine as it was.
            const advice = await refreshVoiceAdvice();
            if (failed) {
                setVoiceStatus(`⚠ ${failed.voice}: ${failed.error || 'the download failed'}`, 'bad');
            } else if (done.length) {
                voiceEngine.playSFX('incoming');
                setVoiceStatus(
                    `✔ ${done.join(', ')} downloaded. ${advice && advice.speaking && advice.speaking.working
                        ? 'Press "Say something" to hear it.'
                        : 'Piper itself is still missing -- follow the steps above, then check again.'}`,
                    'good');
            }
        }

        if (!list.some(d => d.phase !== 'done' && d.phase !== 'failed')) stopVoicePoll();
        return list;
    }

    function startVoicePoll() {
        stopVoicePoll();
        voicePollTimer = setInterval(() => { refreshVoiceDownloads(); }, 1000);
    }

    function stopVoicePoll() {
        if (voicePollTimer) { clearInterval(voicePollTimer); voicePollTimer = null; }
    }

    /* Starting one. Nothing here waits on the download: the bar appears immediately and
       fills in as bytes arrive, and closing this panel does not stop it. */
    async function startVoiceDownload(name, button) {
        voiceEngine.playSFX('click');
        if (button) button.disabled = true;
        settledVoices.delete(name);
        setVoiceStatus(`Starting the download of ${name}...`, 'busy');

        try {
            const data = IS_TAURI
                ? await tauriInvoke('start_voice_download_rust', { voice: name })
                : await (async () => {
                    const resp = await apiFetch(`/api/voice/download?voice=${encodeURIComponent(name)}`,
                        { method: 'POST' });
                    if (!resp.ok) throw new Error(`could not start: ${resp.status}`);
                    return resp.json();
                })();

            if (!data.ok) {
                setVoiceStatus(data.message || 'That voice could not be started.', 'bad');
                if (button) button.disabled = false;
                return;
            }
            setVoiceStatus(`Downloading ${name}. You can leave this open — it carries on either way.`, 'busy');
            await refreshVoiceDownloads();
            startVoicePoll();
        } catch (e) {
            setVoiceStatus(`Could not start that download: ${e.message || e}`, 'bad');
            if (button) button.disabled = false;
        }
    }

    function openVoiceWizard() {
        if (!voiceModal) return;
        voiceEngine.playSFX('click');
        voiceModal.classList.remove('hidden');
        renderVoiceAttempts(null);
        refreshVoiceAdvice();
        // A download started earlier is still going whether or not this panel was open,
        // so the first thing it does is ask rather than assume there is nothing running.
        refreshVoiceDownloads().then(list => {
            if (list.some(d => d.phase !== 'done' && d.phase !== 'failed')) startVoicePoll();
        }).catch(() => null);
    }

    document.getElementById('btn-voice-setup')?.addEventListener('click', openVoiceWizard);
    if (btnVoiceRecheck) btnVoiceRecheck.addEventListener('click', () => { voiceEngine.playSFX('click'); refreshVoiceAdvice(); });
    if (btnVoiceTest) btnVoiceTest.addEventListener('click', handleVoiceTest);
    if (btnVoiceEnable) btnVoiceEnable.addEventListener('click', handleVoiceEnable);
    document.getElementById('btn-close-voice')?.addEventListener('click', () => {
        voiceModal.classList.add('hidden');
        // Only the asking stops. The download itself runs on its own thread in the
        // backend and finishes whether this panel is open or not.
        stopVoicePoll();
    });
    document.getElementById('btn-open-voice-settings')?.addEventListener('click', () => {
        settingsModal.classList.add('hidden');
        openVoiceWizard();
    });

    /* The line at the top of The Brain saying what is actually connected. Settings that
       name a provider are not evidence that anything answers, and the difference is the
       whole reason someone opens this panel. */
    async function refreshBrainStatus() {
        const box = document.getElementById('brain-status');
        if (!box) return;
        box.className = 'text-[11px] font-mono p-2 rounded border border-slate-600/40 bg-slate-900/60 text-slate-400';
        box.textContent = 'Checking what is connected...';
        const advice = await fetchSetupAdvice().catch(() => null);
        if (!advice) { box.textContent = 'Could not check this computer.'; return; }
        if (advice.stage === 'configured') {
            box.className = 'text-[11px] font-mono p-2 rounded border border-green-500/40 bg-green-950/20 text-green-400';
            box.textContent = `✔ ${advice.headline}`;
        } else {
            box.className = 'text-[11px] font-mono p-2 rounded border border-amber-500/40 bg-amber-950/20 text-amber-300';
            box.textContent = `⚠ ${advice.headline} Press "Set it up for me" above.`;
        }
    }

    /* The missing-brain notice. Deliberately the loudest thing on the page: an app with
       no model behind it is not "mostly working", and a quiet grey line saying so is the
       reason someone spends an evening wondering why the answers are so bad. */
    function showNoBrainCard(advice) {
        document.getElementById('no-brain-card')?.remove();

        const card = document.createElement('div');
        card.id = 'no-brain-card';
        card.className = 'no-brain-card';

        const head = document.createElement('div');
        head.className = 'no-brain-head';
        head.textContent = '⚠ NO AI IS CONNECTED';
        card.appendChild(head);

        const body = document.createElement('div');
        body.className = 'no-brain-body';
        body.textContent = advice && advice.headline
            ? `${advice.headline} Until one is connected I cannot answer anything — there is no thinking behind this window yet. Setting one up takes a few minutes, runs entirely on this computer, and costs nothing.`
            : 'There is no AI model behind this window yet. Setting one up takes a few minutes, runs entirely on this computer, and costs nothing.';
        card.appendChild(body);

        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'cyber-btn cyber-btn-active text-sm py-2.5 px-5 mt-3 w-full sm:w-auto';
        btn.textContent = '🧠 SET IT UP FOR ME';
        btn.addEventListener('click', openSetupWizard);
        card.appendChild(btn);

        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
    }

    /* On load: find out whether anything is connected, and if not, say so in the stream
       and open the wizard straight away. The wizard opening by itself is the point --
       a notice you have to notice is a notice that gets missed. */
    async function announceIfNoBrain() {
        const advice = await fetchSetupAdvice().catch(() => null);
        brainNeedsAttention = !!(advice && advice.needs_attention);
        if (!brainNeedsAttention) return;

        showNoBrainCard(advice);
        // Opened once per launch, never again from here: re-opening a window somebody
        // just closed is how an app teaches people to close it without looking.
        openSetupWizard();
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

    /* Opens the vault folder in the file manager -- on the machine actually running
       Aether1 either way, so the browser fallback is a real HTTP call rather than
       something client-side, unlike openAvatarLab's plain-webview fallback above. */
    async function openVaultFolder() {
        try {
            if (IS_TAURI) {
                await tauriInvoke('open_vault_folder_rust');
            } else {
                const res = await apiFetch('/api/vault/open', { method: 'POST' });
                if (!res.ok) throw new Error(`status ${res.status}`);
            }
        } catch (err) {
            console.warn('Could not open the vault folder:', err);
            alert(`Could not open the vault folder: ${err.message || err}`);
        }
    }
    document.getElementById('btn-open-vault')?.addEventListener('click', openVaultFolder);

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
            showVoiceFailedCard(e);
            return null;
        }
    }

    /* Said once per launch, and only when speech was actually attempted and actually
       failed. The old behaviour was a console warning nobody opens: the app simply went
       quiet, and the only symptom was silence -- which is indistinguishable from a
       reply that had nothing to say. */
    let voiceFailureAnnounced = false;
    function showVoiceFailedCard(err) {
        if (voiceFailureAnnounced) return;
        voiceFailureAnnounced = true;

        document.getElementById('voice-failed-card')?.remove();

        const card = document.createElement('div');
        card.id = 'voice-failed-card';
        card.className = 'no-brain-card';

        const head = document.createElement('div');
        head.className = 'no-brain-head';
        head.textContent = '⚠ I COULD NOT SPEAK THAT';
        card.appendChild(head);

        const body = document.createElement('div');
        body.className = 'no-brain-body';
        const why = err && err.message ? String(err.message) : String(err || 'no reason given');
        body.textContent = `The words are on screen, but no voice came out. The reason given was: ${why}. The voice setup can test each voice in turn and tell you which one is missing.`;
        card.appendChild(body);

        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'cyber-btn cyber-btn-active text-sm py-2.5 px-5 mt-3 w-full sm:w-auto';
        btn.textContent = '🗣 FIX THE VOICE';
        btn.addEventListener('click', () => {
            card.remove();
            openVoiceWizard();
        });
        card.appendChild(btn);

        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
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

    // Reflects Game Mode's current on/off state on its Quick Commands button -- called both
    // from loadSettings (what was saved from a previous session) and from the
    // 'game-mode-changed' event (a live toggle, from this window or another).
    function setGameModeButtonState(active) {
        const btn = document.getElementById('btn-game-mode');
        if (!btn) return;
        btn.dataset.active = active ? 'true' : 'false';
        btn.textContent = active ? '🎮 Game Mode: ON' : '🎮 Game Mode: OFF';
        btn.classList.toggle('cyber-btn-active', active);
    }

    // Launch autostart, Ollama autostart and Game Mode are all native-process/window
    // lifecycle -- meaningless from a browser tab, same reasoning as
    // initSpriteMode/initVersionAndUpdates, so the whole settings group and the Game Mode
    // button stay hidden there.
    function initStartupPerformance() {
        if (!IS_TAURI) return;
        document.getElementById('settings-group-startup')?.classList.remove('hidden');

        const gameModeBtn = document.getElementById('btn-game-mode');
        if (gameModeBtn) {
            gameModeBtn.classList.remove('hidden');
            gameModeBtn.addEventListener('click', async () => {
                voiceEngine.playSFX('click');
                const turningOn = gameModeBtn.dataset.active !== 'true';
                gameModeBtn.disabled = true;
                try {
                    await tauriInvoke('set_game_mode_rust', { enabled: turningOn });
                    setGameModeButtonState(turningOn);
                } catch (e) {
                    console.warn('Could not toggle Game Mode', e);
                    appendMessage(currentAgentName, `⚠️ Could not switch Game Mode: ${e.message || e}`);
                } finally {
                    gameModeBtn.disabled = false;
                }
            });
        }

        // Game Mode can also be switched off from the tray/hotkey path (e.g. re-showing a
        // hidden window some other way) or from another undocked panel window -- listening
        // rather than only reacting to this button's own click keeps every window's button
        // label honest.
        if (window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.listen('game-mode-changed', (event) => {
                setGameModeButtonState(event.payload === true);
            });
        }
    }

    /**
     * Runs once per launch (native app only): synthesizes a short phrase through the exact
     * pipeline a real reply would use, and checks whether real, non-zero audio actually came
     * out the other end -- not just whether synthesis returned a path without throwing. That
     * gap is exactly what let TTS go silently unheard before: everything downstream of Piper
     * (webview → GStreamer/WebKitGTK → the audio device) could fail without a single error
     * anywhere in this app. `voice_startup_audible` only decides whether the check is heard;
     * the check itself always runs, and a failure is surfaced either way (see
     * showVoiceFailedCard) instead of the console.warn nobody used to see.
     */
    async function runVoiceStartupSelfTest() {
        if (!IS_TAURI) return;
        let audible = true;
        try {
            const data = await tauriInvoke('get_settings_rust');
            audible = data.settings.voice_startup_audible !== false;
        } catch (e) {
            // Fall through with the audible default -- loadSettings surfaces its own
            // failure to load settings; this self-test isn't the place to repeat it.
        }

        const url = await synthesizeSpeechUrl('Voice check.', null);
        if (!url) return; // synthesizeSpeechUrl already showed showVoiceFailedCard on failure

        const result = await voiceEngine.playTTSAudio(url, { audible });
        if (result && result.signalDetected === false) {
            showVoiceFailedCard(new Error(
                'Speech was synthesized, but no audio was actually heard -- the pipeline ' +
                'downstream of synthesis (the webview’s audio output) produced silence.'
            ));
        }
    }

    // Solo-panel mode: this window was opened by open_panel_window_rust with ?panel=<id>,
    // and the bootstrap script in <head> already stamped data-solo-panel on <html> before
    // anything painted. Mark the matching panel as the one CSS should expand to fill the
    // window, and (native app only) wire up the "Always on top" pin -- a bare popped-out
    // window has no title bar of its own to put that control on.
    function initSoloPanel() {
        const panelId = document.documentElement.getAttribute('data-solo-panel');
        if (!panelId) return;
        const panel = document.querySelector(`[data-panel="${CSS.escape(panelId)}"]`);
        if (panel) panel.classList.add('solo-target');

        if (!IS_TAURI) return;
        const pinControl = document.getElementById('solo-pin-control');
        const pinCheckbox = document.getElementById('solo-pin-checkbox');
        if (!pinControl || !pinCheckbox) return;
        pinControl.classList.remove('hidden');
        pinCheckbox.addEventListener('change', () => {
            tauriInvoke('set_window_always_on_top_rust', { enabled: pinCheckbox.checked })
                .catch((e) => console.warn('Could not toggle always-on-top', e));
        });
    }

    // The hologram panel's own visual state: while the avatar is floating on the desktop as
    // the sprite, this panel goes quiet instead of drawing a second live hologram nobody
    // asked for -- see the [data-panel="hologram"].avatar-floating rules in A1theme.css and
    // the #hologram-floating-notice button in index.html that this reveals in its place.
    function setHologramFloating(floating) {
        desktopSpriteEnabled = floating;
        const panel = document.querySelector('[data-panel="hologram"]');
        if (panel) panel.classList.toggle('avatar-floating', floating);
    }

    // Turns Desktop Sprite Mode on or off: persists the setting, tells Rust to open/close the
    // window, and updates this panel's own floating state to match. Shared by the panel's
    // Undock button (turns it on), the floating notice's "bring it back" click (turns it
    // off), and kept in sync with whatever the Settings modal's own Save button does (see
    // saveSettings) so all three ways of reaching this agree on what's showing.
    async function setDesktopSpriteMode(enabled) {
        const spriteModeToggle = document.getElementById('setting-sprite-mode');
        if (spriteModeToggle) spriteModeToggle.checked = enabled;
        try {
            await tauriInvoke('save_settings_rust', { settings: { desktop_sprite_enabled: enabled } });
            await tauriInvoke('toggle_sprite_window_rust', { enabled });
        } catch (e) {
            console.warn('Could not toggle Desktop Sprite Mode', e);
        }
        setHologramFloating(enabled);
    }

    // The floating notice that replaces the hologram panel's live content while the avatar
    // is out on the desktop -- clicking it is the way back, short of reopening Settings.
    function initHologramFloatingNotice() {
        const notice = document.getElementById('hologram-floating-notice');
        if (!notice) return;
        notice.addEventListener('click', () => setDesktopSpriteMode(false));
    }

    // The sprite has no chat of its own (see js/sprite.js) and its viewport is otherwise
    // spoken for by window-dragging, so clicking the avatar there instead asks this window
    // to toggle push-to-talk -- the same start/stop startTalking/stopTalking already do for
    // held Space or the mic button, just requested from the desktop instead of the HUD.
    function initSpriteListenBridge() {
        if (!IS_TAURI || !window.__TAURI__ || !window.__TAURI__.event) return;
        window.__TAURI__.event.listen('sprite-toggle-listen', () => {
            if (talkHeld) {
                stopTalking();
            } else {
                startTalking();
            }
        }).catch((e) => console.warn('Could not listen for sprite listen-toggle requests', e));
    }

    // Every panel's "Undock" button opens it in its own solo-panel window (see
    // open_panel_window_rust) -- except the hologram's, which turns on Desktop Sprite
    // Mode instead: the avatar leaves this window entirely and reappears as the floating
    // desktop sprite, a live mirror of this same hologram rather than a second, separate one.
    function initPanelUndock() {
        if (!IS_TAURI) return;
        document.querySelectorAll('[data-undock]').forEach((btn) => {
            btn.addEventListener('click', () => {
                const id = btn.dataset.undock;
                if (id === 'hologram') {
                    setDesktopSpriteMode(true);
                    return;
                }
                tauriInvoke('open_panel_window_rust', { panel: id })
                    .catch((e) => console.warn('Could not undock panel', id, e));
            });
        });
    }

    async function loadChatHistory() {
        try {
            const msgs = IS_TAURI
                ? await tauriInvoke('get_messages_rust', { limit: 25, sessionId: currentSessionId })
                : await (async () => {
                    const resp = await apiFetch(`/api/messages?limit=25&session_id=${encodeURIComponent(currentSessionId)}`);
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

    /// Wording that only makes sense on one operating system, keyed by the one the backend
    /// says it is running on.
    ///
    /// Two rules hold here. The folders have to be the ones this machine actually has --
    /// see tools/domain.rs::directories(), which resolves the same symbolic roots to real
    /// Windows and Unix paths -- because a help line naming /etc on Windows is not a small
    /// cosmetic slip: it tells the operator the app can reach somewhere that cannot exist,
    /// and they have no way to check. And the answer comes from the *backend*, never from
    /// navigator.platform, for the reason written on Os::current in setup.rs: when the
    /// Windows laptop browses to the Linux desktop's HUD, the machine being described is
    /// the Linux one.
    /// The example programs are deliberately ones that only look: the box's own rule is
    /// that no option you could pass the program changes anything, and `git`, `docker` and
    /// `systemctl` -- the previous examples here -- are the three the starter list turns
    /// away for exactly that reason. An example that contradicts the rule beside it teaches
    /// the wrong lesson to the one person reading it most carefully.
    const OS_WORDING = {
        windows: {
            readableRoots: "Only your user folder, the Windows event logs and the network " +
                "configuration files in System32",
            allowlistPlaceholder: "e.g. ping, nslookup",
        },
        mac: {
            readableRoots: "Only your home folder, /etc and the system logs",
            allowlistPlaceholder: "e.g. ping, dig",
        },
        linux: {
            readableRoots: "Only your home directory, /etc, /proc and /var/log",
            allowlistPlaceholder: "e.g. ping, dig",
        },
    };

    /// Applies OS_WORDING to the Settings panel. An unrecognised or missing `os` leaves the
    /// markup alone, which is why index.html ships wording that is true everywhere: a
    /// backend too old to send the field, or one built for a platform not listed above,
    /// should read vague rather than wrong.
    function applyOsWording(os) {
        const wording = OS_WORDING[os];
        if (!wording) return;
        const roots = document.getElementById('tools-readable-roots');
        if (roots) roots.textContent = wording.readableRoots;
        const allowlist = document.getElementById('setting-command-allowlist');
        if (allowlist) allowlist.placeholder = wording.allowlistPlaceholder;
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
            // Before the fields: this only rewrites static help text, but doing it first
            // means the panel is never briefly describing the wrong machine.
            applyOsWording(data.os);
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
            // Empty is the normal, working value for these two: it means "find Piper and
            // Whisper yourself". They are here so a machine where that search fails has a
            // way out that isn't editing the database by hand.
            document.getElementById('setting-tts-local-voice').value = s.tts_local_voice || '';
            document.getElementById('setting-stt-model').value = s.stt_model_path || '';
            document.getElementById('setting-stt-language').value = s.stt_language || 'en';
            document.getElementById('setting-vault-path').value = s.vault_path || '';
            // Absent means on, matching vault::journal_enabled -- a setting that has never
            // been saved must not read as "off" here when the vault is in fact writing.
            document.getElementById('setting-vault-journal').checked = s.vault_journal !== false;
            document.getElementById('setting-local-only').checked = s.local_only === true;
            // After the checkbox is set, not before: loadVoiceStatus is what discovers an
            // environment-forced mode and overrides the saved value on screen.
            loadVoiceStatus();
            document.getElementById('setting-tools').checked = s.tools_enabled === true;
            document.getElementById('setting-command-allowlist').value =
                Array.isArray(s.command_allowlist) ? s.command_allowlist.join(', ') : '';
            loadPersonaAccess();
            document.getElementById('setting-autospeak').checked = s.auto_speak !== false;
            autoSpeak = s.auto_speak !== false;
            // enable_sfx has been a stored setting -- and one the companion itself is
            // allowed to change -- since before anything read it: the HUD beeped either
            // way. Now the saved value reaches the engine that does the beeping.
            applySfx(s.enable_sfx !== false);
            // Likewise color_theme: set_aether_setting has been able to write it all
            // along, and the HUD only ever read the browser's own copy, so asking the
            // companion to change its colours changed nothing anyone could see.
            // Only when it disagrees with what this window is already wearing. The browser's
            // own copy is what paints the page before any request finishes and stays
            // authoritative for this window; a difference means something else wrote the
            // setting, and the only thing that can is the companion itself.
            if (s.color_theme && s.color_theme !== Aether1Theme.current().colours.preset) {
                paintTheme(Aether1Theme.setPreset(s.color_theme));
            }
            document.getElementById('setting-autostart-app').checked = s.autostart_app === true;
            document.getElementById('setting-autostart-ollama').checked = s.autostart_ollama === true;
            document.getElementById('setting-voice-startup-audible').checked = s.voice_startup_audible !== false;
            setGameModeButtonState(s.game_mode === true);
            const spriteModeToggle = document.getElementById('setting-sprite-mode');
            if (spriteModeToggle) spriteModeToggle.checked = s.desktop_sprite_enabled === true;
            // The Rust side reopens the sprite window itself on launch if it was left on
            // (see the sprite_was_enabled check in main.rs's setup()) -- this just makes the
            // panel agree with that from the moment settings load, instead of drawing a live
            // hologram here too until something else happens to call setHologramFloating.
            setHologramFloating(s.desktop_sprite_enabled === true);
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
                tts_local_voice: document.getElementById('setting-tts-local-voice').value.trim(),
                stt_model_path: document.getElementById('setting-stt-model').value.trim(),
                // Falls back rather than saving an empty language: transcribe_audio passes
                // this straight to the recognizer, which wants a code, not nothing.
                stt_language: document.getElementById('setting-stt-language').value.trim() || 'en',
                local_only: document.getElementById('setting-local-only').checked,
                vault_path: document.getElementById('setting-vault-path').value.trim(),
                vault_journal: document.getElementById('setting-vault-journal').checked,
                // Sent only from the native app: the browser fallback has no window for the
                // OS to summon, and saving a chord there would promise something that can't
                // happen. See setting-hotkey-wrap, hidden on that path.
                ...(IS_TAURI ? { hotkey_toggle: document.getElementById('setting-hotkey').value.trim() } : {}),
                tools_enabled: document.getElementById('setting-tools').checked,
                command_allowlist: document.getElementById('setting-command-allowlist').value
                    .split(',').map(p => p.trim()).filter(Boolean),
                auto_speak: document.getElementById('setting-autospeak').checked,
                enable_sfx: document.getElementById('setting-sfx')?.checked !== false,
                // The theme lives in the browser's own storage, which is where it has to
                // live for the page to paint before any request completes. Saved here as
                // well so the two agree -- otherwise the companion's own writes to it are
                // overwritten by whatever this window last had.
                // Empty when the colours have been hand-mixed rather than chosen from a
                // preset -- there is no preset name to save, and loadSettings ignores an
                // empty value rather than repainting over the mix.
                color_theme: Aether1Theme.current().colours.preset || '',
                desktop_sprite_enabled: spriteModeToggle ? spriteModeToggle.checked : false,
                autostart_app: document.getElementById('setting-autostart-app').checked,
                autostart_ollama: document.getElementById('setting-autostart-ollama').checked,
                voice_startup_audible: document.getElementById('setting-voice-startup-audible').checked
            }
        };
        autoSpeak = payload.settings.auto_speak;
        applySfx(payload.settings.enable_sfx);
        updateAgentNameDisplay(payload.settings.agent_name);

        try {
            if (IS_TAURI) {
                await tauriInvoke('save_settings_rust', { settings: payload.settings });
                await tauriInvoke('toggle_sprite_window_rust', { enabled: payload.settings.desktop_sprite_enabled });
                setHologramFloating(payload.settings.desktop_sprite_enabled);
            } else {
                const resp = await apiFetch('/api/settings', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(payload)
                });
                if (!resp.ok) throw new Error(`settings save failed: ${resp.status}`);
            }
            // After the settings, never before: the folder list attaches to the speciality
            // that was just saved, so sending it first would file it under the old one.
            const access = await savePersonaAccess();
            if (access && (access.refused || []).length) {
                appendMessage(currentAgentName,
                    `⚠️ These folders were not added: ${access.refused.join('; ')}`);
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

    /* ------------------------------------------------------------------ */
    /* Reading the vault.                                                   */
    /*                                                                      */
    /* The notes are plain markdown on disk and always have been -- Obsidian */
    /* or any editor opens the same folder, and the [[links]] between notes  */
    /* were written to be read that way. This is for when you do not want to */
    /* leave the app to look something up.                                   */
    /*                                                                      */
    /* Read-only, deliberately. Notes are changed through the companion's    */
    /* tools, which ask before they change anything and record a way back;   */
    /* a reader that could also save would be a second way into the same     */
    /* folder with neither of those things attached to it.                   */
    /*                                                                      */
    /* Nothing here builds markup out of note text. A note can contain       */
    /* anything -- the companion wrote half of it and you wrote the rest --  */
    /* so every piece of it reaches the page as a text node.                 */
    /* ------------------------------------------------------------------ */

    const notesModal = document.getElementById('notes-modal');
    const notesList = document.getElementById('notes-list');
    const notesBody = document.getElementById('notes-body');
    const notesTitle = document.getElementById('notes-title');
    const notesCount = document.getElementById('notes-count');
    const notesSearch = document.getElementById('notes-search');
    const notesBacklinks = document.getElementById('notes-backlinks');
    const btnNotesBack = document.getElementById('btn-notes-back');

    /* Where we came from, so Back after following a [[link]] means something. Capped
       because it is a browsing convenience, not a history anybody audits. */
    let noteTrail = [];
    let notesCache = [];

    async function fetchNotes() {
        if (IS_TAURI) return tauriInvoke('vault_notes_rust');
        const res = await apiFetch('/api/vault/notes');
        if (!res.ok) throw new Error(`status ${res.status}`);
        return res.json();
    }

    async function fetchNote(name) {
        if (IS_TAURI) return tauriInvoke('vault_note_rust', { name });
        const res = await apiFetch(`/api/vault/note?name=${encodeURIComponent(name)}`);
        if (!res.ok) throw new Error(res.status === 404 ? 'no such note' : `status ${res.status}`);
        return res.json();
    }

    async function fetchNoteSearch(query) {
        if (IS_TAURI) return tauriInvoke('vault_search_rust', { query });
        const res = await apiFetch(`/api/vault/search?q=${encodeURIComponent(query)}`);
        if (!res.ok) throw new Error(`status ${res.status}`);
        return res.json();
    }

    function noteAge(seconds) {
        if (!seconds) return '';
        const mins = Math.max(0, Math.round((Date.now() / 1000 - seconds) / 60));
        if (mins < 60) return `${mins}m ago`;
        if (mins < 1440) return `${Math.round(mins / 60)}h ago`;
        return `${Math.round(mins / 1440)}d ago`;
    }

    /* One row in the list. The ★ marks a note the companion reads at the start of every
       single turn, which is worth being able to see: those three are the ones where a
       stray line changes every answer it gives. */
    function renderNoteRow(note, meta) {
        const row = document.createElement('button');
        row.type = 'button';
        row.className = 'w-full text-left px-2 py-1.5 rounded border border-cyan-500/20 ' +
            'bg-slate-900/50 hover:border-cyan-400/60 hover:bg-slate-800/60 cursor-pointer';
        const name = document.createElement('div');
        name.className = 'font-mono text-[11px] text-cyan-200 truncate';
        name.textContent = (note.core ? '★ ' : '') + note.name;
        row.appendChild(name);
        const sub = document.createElement('div');
        sub.className = 'font-mono text-[10px] text-slate-500 truncate';
        sub.textContent = meta;
        row.appendChild(sub);
        row.addEventListener('click', () => {
            noteTrail = [];
            openNote(note.name);
        });
        return row;
    }

    function renderNoteList(notes) {
        notesList.textContent = '';
        notesCache = notes;
        if (!notes.length) {
            const empty = document.createElement('p');
            empty.className = 'text-[11px] font-mono text-slate-500';
            empty.textContent = 'No notes yet. They appear as you talk.';
            notesList.appendChild(empty);
            notesCount.textContent = '';
            return;
        }
        notes.forEach((n) => {
            const size = n.bytes < 1024 ? `${n.bytes} B` : `${Math.round(n.bytes / 1024)} KB`;
            notesList.appendChild(renderNoteRow(n, `${size} · ${noteAge(n.modified)}`));
        });
        notesCount.textContent = `${notes.length} note${notes.length === 1 ? '' : 's'}`;
    }

    function renderSearchResults(results) {
        notesList.textContent = '';
        const hits = (results && results.hits) || [];
        if (!hits.length) {
            const empty = document.createElement('p');
            empty.className = 'text-[11px] font-mono text-slate-500';
            empty.textContent = 'Nothing matched.';
            notesList.appendChild(empty);
            notesCount.textContent = '';
            return;
        }
        hits.forEach((h) => {
            notesList.appendChild(renderNoteRow({ name: h.note, core: false }, h.snippet));
        });
        /* `partial` means the scan hit its cap before the end of the vault, so saying
           "3 results" without saying that would be claiming to have looked everywhere. */
        notesCount.textContent = results.partial
            ? `${hits.length} of the first ${results.scanned} notes`
            : `${hits.length} match${hits.length === 1 ? '' : 'es'}`;
    }

    /* The inline bits of a line: **bold**, *italic*, `code` and [[links]]. Appends text
       nodes and elements to `parent`; nothing here ever touches innerHTML. */
    function renderInline(parent, text, onLink) {
        const pattern = /\[\[([^\]\n]+)\]\]|`([^`\n]+)`|\*\*([^*\n]+)\*\*|\*([^*\n]+)\*/g;
        let last = 0;
        let m;
        while ((m = pattern.exec(text)) !== null) {
            if (m.index > last) parent.appendChild(document.createTextNode(text.slice(last, m.index)));
            if (m[1] !== undefined) {
                /* A wiki link. The part before a | is the note; the part after is what the
                   writer wanted it called. */
                const target = m[1].split('|')[0].trim();
                const label = (m[1].split('|')[1] || target).trim();
                const a = document.createElement('a');
                a.className = 'text-cyan-300 underline decoration-cyan-500/40 cursor-pointer hover:text-cyan-100';
                a.textContent = label;
                a.addEventListener('click', (e) => { e.preventDefault(); onLink(target); });
                parent.appendChild(a);
            } else if (m[2] !== undefined) {
                const c = document.createElement('code');
                c.className = 'font-mono text-[12px] text-cyan-200 bg-slate-950/70 rounded px-1';
                c.textContent = m[2];
                parent.appendChild(c);
            } else if (m[3] !== undefined) {
                const b = document.createElement('strong');
                b.className = 'text-cyan-100';
                b.textContent = m[3];
                parent.appendChild(b);
            } else {
                const i = document.createElement('em');
                i.textContent = m[4];
                parent.appendChild(i);
            }
            last = pattern.lastIndex;
        }
        if (last < text.length) parent.appendChild(document.createTextNode(text.slice(last)));
    }

    /* A markdown renderer small enough to read in one sitting, rather than a library.
       The vault writes a known and narrow subset -- headings, lists, quotes, tables as
       plain lines, fenced code -- and a full parser would mean inheriting its opinions
       about raw HTML in particular, which is the one opinion that matters here. */
    function renderMarkdown(into, text, onLink) {
        into.textContent = '';
        const lines = text.split('\n');
        let list = null;
        let fence = null;
        for (const line of lines) {
            if (line.trimStart().startsWith('```')) {
                if (fence) { into.appendChild(fence); fence = null; } else {
                    fence = document.createElement('pre');
                    fence.className = 'font-mono text-[11px] text-cyan-200 bg-slate-950/70 rounded p-2 overflow-x-auto my-2';
                }
                continue;
            }
            if (fence) { fence.appendChild(document.createTextNode(line + '\n')); continue; }

            const heading = /^(#{1,6})\s+(.*)$/.exec(line);
            const bullet = /^\s*[-*+]\s+(.*)$/.exec(line);
            if (!bullet && list) { into.appendChild(list); list = null; }

            if (heading) {
                const h = document.createElement(`h${Math.min(4, heading[1].length + 1)}`);
                h.className = 'font-orbitron text-cyan-300 mt-3 mb-1 ' +
                    (heading[1].length === 1 ? 'text-sm' : 'text-xs');
                renderInline(h, heading[2], onLink);
                into.appendChild(h);
            } else if (bullet) {
                if (!list) {
                    list = document.createElement('ul');
                    list.className = 'list-disc pl-5 space-y-0.5 my-1 text-[13px]';
                }
                const li = document.createElement('li');
                renderInline(li, bullet[1], onLink);
                list.appendChild(li);
            } else if (line.trim() === '') {
                continue;
            } else if (line.startsWith('>')) {
                const q = document.createElement('blockquote');
                q.className = 'border-l-2 border-cyan-500/40 pl-2 text-slate-400 my-1 text-[13px]';
                renderInline(q, line.replace(/^>\s?/, ''), onLink);
                into.appendChild(q);
            } else {
                const p = document.createElement('p');
                p.className = 'my-1 text-[13px]';
                renderInline(p, line, onLink);
                into.appendChild(p);
            }
        }
        if (list) into.appendChild(list);
        if (fence) into.appendChild(fence);
    }

    function showNoteError(message) {
        notesBody.textContent = '';
        const p = document.createElement('p');
        p.className = 'text-[11px] font-mono text-amber-300';
        p.textContent = message;
        notesBody.appendChild(p);
        notesBacklinks.classList.add('hidden');
    }

    async function openNote(name) {
        notesTitle.textContent = name;
        try {
            const view = await fetchNote(name);
            renderMarkdown(notesBody, view.text, (target) => {
                /* A link the note names but nobody has written yet resolves to nothing.
                   Say so rather than opening an empty page or failing silently. */
                const link = (view.links || []).find((l) => l.target === target);
                if (link && !link.note) {
                    showNoteError(`"${target}" is linked from here but no such note exists yet.`);
                    notesTitle.textContent = target;
                    return;
                }
                noteTrail.push(name);
                if (noteTrail.length > 50) noteTrail.shift();
                openNote(link && link.note ? link.note : target);
            });
            if (view.truncated) {
                const cut = document.createElement('p');
                cut.className = 'text-[11px] font-mono text-amber-300 mt-3';
                cut.textContent = 'This note is too big to show all of — open the folder to read the rest.';
                notesBody.appendChild(cut);
            }
            notesBody.scrollTop = 0;
            renderBacklinks(view.backlinks || []);
        } catch (err) {
            showNoteError(`Could not open that note: ${err.message || err}`);
        }
        btnNotesBack.classList.toggle('hidden', noteTrail.length === 0);
    }

    /* What points *at* this note. The links out of a note are in the text where you can
       see them; the ones pointing in are the half of the graph a plain editor hides. */
    function renderBacklinks(names) {
        notesBacklinks.textContent = '';
        if (!names.length) { notesBacklinks.classList.add('hidden'); return; }
        notesBacklinks.classList.remove('hidden');
        const label = document.createElement('div');
        label.className = 'text-cyan-300';
        label.textContent = `Linked from (${names.length}):`;
        notesBacklinks.appendChild(label);
        const row = document.createElement('div');
        row.className = 'flex flex-wrap gap-x-3 gap-y-1';
        names.forEach((n) => {
            const a = document.createElement('a');
            a.className = 'text-cyan-400 underline decoration-cyan-500/40 cursor-pointer hover:text-cyan-100';
            a.textContent = n;
            a.addEventListener('click', () => {
                noteTrail.push(notesTitle.textContent);
                openNote(n);
            });
            row.appendChild(a);
        });
        notesBacklinks.appendChild(row);
    }

    btnNotesBack.addEventListener('click', () => {
        const previous = noteTrail.pop();
        if (previous) openNote(previous);
        btnNotesBack.classList.toggle('hidden', noteTrail.length === 0);
    });

    /* Typing runs the same search the companion uses on the vault, so what you find here
       is what it would have found. Debounced: every keystroke scanning the folder would
       make a big vault feel broken. */
    let notesSearchTimer = null;
    notesSearch.addEventListener('input', () => {
        clearTimeout(notesSearchTimer);
        const query = notesSearch.value.trim();
        notesSearchTimer = setTimeout(async () => {
            if (!query) { renderNoteList(notesCache); return; }
            try {
                renderSearchResults(await fetchNoteSearch(query));
            } catch (err) {
                console.warn('Note search failed:', err);
            }
        }, 250);
    });

    async function openNotesReader() {
        voiceEngine.playSFX('click');
        notesModal.classList.remove('hidden');
        showReaderView();
        notesSearch.value = '';
        noteTrail = [];
        btnNotesBack.classList.add('hidden');
        try {
            renderNoteList(await fetchNotes());
        } catch (err) {
            notesList.textContent = '';
            const p = document.createElement('p');
            p.className = 'text-[11px] font-mono text-amber-300';
            p.textContent = `Could not read the notes folder: ${err.message || err}`;
            notesList.appendChild(p);
        }
    }

    /* -------------------------------------------------------------------- */
    /* Conversations.                                                       */
    /*                                                                      */
    /* A conversation is a transcript with a name. Switching to one reloads  */
    /* the chat box from it and points every later message at it; the model  */
    /* is only ever handed the history of the one it is answering in, so a   */
    /* conversation is a privacy boundary and not only a filing cabinet.     */
    /*                                                                      */
    /* Nothing here is reachable by the companion. There is no tool for any  */
    /* of it and none of it is in the settings allowlist -- it cannot start, */
    /* switch, rename or delete a conversation, the same way it cannot turn  */
    /* its own tools or panels on.                                          */
    /* -------------------------------------------------------------------- */
    const sessionsModal = document.getElementById('sessions-modal');
    const sessionsList = document.getElementById('sessions-list');
    const sessionsCount = document.getElementById('sessions-count');

    async function fetchSessions() {
        if (IS_TAURI) return tauriInvoke('list_sessions_rust');
        const resp = await apiFetch('/api/sessions');
        if (!resp.ok) throw new Error(`conversations request failed: ${resp.status}`);
        return resp.json();
    }

    async function mintSession() {
        if (IS_TAURI) return tauriInvoke('new_session_rust');
        const resp = await apiFetch('/api/sessions/new', { method: 'POST' });
        if (!resp.ok) throw new Error(`could not start a conversation: ${resp.status}`);
        return (await resp.json()).session_id;
    }

    async function renameSession(id, title) {
        if (IS_TAURI) return tauriInvoke('rename_session_rust', { sessionId: id, title });
        const resp = await apiFetch('/api/sessions/rename', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ session_id: id, title }),
        });
        if (!resp.ok) throw new Error(await resp.text());
    }

    async function deleteSession(id) {
        if (IS_TAURI) return tauriInvoke('delete_session_rust', { sessionId: id });
        const resp = await apiFetch('/api/sessions/delete', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ session_id: id }),
        });
        if (!resp.ok) throw new Error(await resp.text());
    }

    /* Turns a database timestamp into something a person reads. SQLite hands back UTC
       without saying so, which every browser then reads as local time and shows an hour or
       ten out; the Z is what tells it the truth. */
    function whenText(raw) {
        if (!raw) return '';
        const t = Date.parse(raw.includes('T') ? raw : `${raw.replace(' ', 'T')}Z`);
        if (Number.isNaN(t)) return raw;
        const d = new Date(t);
        const mins = Math.round((Date.now() - t) / 60000);
        if (mins < 1) return 'just now';
        if (mins < 60) return `${mins} min ago`;
        if (mins < 60 * 24) return `${Math.round(mins / 60)} h ago`;
        return d.toLocaleDateString();
    }

    function renderSessions(rows) {
        sessionsList.textContent = '';
        sessionsCount.textContent = rows.length === 1
            ? '1 conversation'
            : `${rows.length} conversations`;

        if (rows.length === 0) {
            const p = document.createElement('p');
            p.className = 'text-[11px] font-mono text-slate-500';
            p.textContent = 'Nothing yet. Say something and this fills in.';
            sessionsList.appendChild(p);
            return;
        }

        rows.forEach((row) => {
            const here = row.id === currentSessionId;
            const wrap = document.createElement('div');
            wrap.className = here
                ? 'flex items-center gap-2 border border-cyan-400/60 bg-cyan-950/40 rounded p-2'
                : 'flex items-center gap-2 border border-cyan-500/20 hover:border-cyan-500/50 rounded p-2';

            const open = document.createElement('button');
            open.className = 'flex-1 min-w-0 text-left cursor-pointer';
            const title = document.createElement('div');
            title.className = 'font-mono text-xs text-cyan-200 truncate';
            title.textContent = row.title;
            const meta = document.createElement('div');
            meta.className = 'font-mono text-[10px] text-slate-500';
            const msgWord = row.messages === 1 ? 'message' : 'messages';
            meta.textContent = `${row.messages} ${msgWord} · ${whenText(row.last)}${here ? ' · you are here' : ''}`;
            open.append(title, meta);
            open.addEventListener('click', () => switchToSession(row.id));

            const rename = document.createElement('button');
            rename.className = 'shrink-0 text-[11px] font-mono text-slate-400 hover:text-cyan-300 cursor-pointer';
            rename.textContent = '✎';
            rename.title = 'Give this conversation a name';
            rename.addEventListener('click', async (ev) => {
                ev.stopPropagation();
                const next = prompt('Name this conversation (leave empty to go back to its first line):', row.named ? row.title : '');
                if (next === null) return;
                try {
                    await renameSession(row.id, next);
                    renderSessions(await fetchSessions());
                } catch (err) {
                    alert(`Could not rename it: ${err.message || err}`);
                }
            });

            const remove = document.createElement('button');
            remove.className = 'shrink-0 text-[11px] font-mono text-slate-400 hover:text-red-400 cursor-pointer';
            remove.textContent = '🗑';
            remove.title = 'Delete this conversation';
            remove.addEventListener('click', async (ev) => {
                ev.stopPropagation();
                /* Named so the operator sees which one they are about to lose. This deletes
                   the transcript outright -- the vault notes for those days stay, because
                   those are the part that was meant to be kept. */
                if (!confirm(`Delete "${row.title}" and everything said in it? This cannot be undone.`)) return;
                try {
                    await deleteSession(row.id);
                    /* Deleting the room you are standing in leaves you nowhere, so step
                       into a fresh one before the list redraws. */
                    if (row.id === currentSessionId) {
                        setCurrentSession(await mintSession());
                        await loadChatHistory();
                    }
                    renderSessions(await fetchSessions());
                } catch (err) {
                    alert(`Could not delete it: ${err.message || err}`);
                }
            });

            wrap.append(open, rename, remove);
            sessionsList.appendChild(wrap);
        });
    }

    async function switchToSession(id) {
        voiceEngine.playSFX('click');
        setCurrentSession(id);
        sessionsModal.classList.add('hidden');
        await loadChatHistory();
    }

    async function openSessions() {
        voiceEngine.playSFX('click');
        sessionsModal.classList.remove('hidden');
        sessionsList.textContent = '';
        try {
            renderSessions(await fetchSessions());
        } catch (err) {
            sessionsList.textContent = '';
            const p = document.createElement('p');
            p.className = 'text-[11px] font-mono text-amber-300';
            p.textContent = `Could not read the conversations: ${err.message || err}`;
            sessionsList.appendChild(p);
        }
    }

    document.getElementById('btn-conversations')?.addEventListener('click', openSessions);
    document.getElementById('btn-close-sessions')?.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        sessionsModal.classList.add('hidden');
    });
    document.getElementById('btn-session-new')?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        try {
            /* A new conversation is an id and nothing else. It starts existing when
               something is said in it, which is why it is not in the list yet. */
            setCurrentSession(await mintSession());
            sessionsModal.classList.add('hidden');
            await loadChatHistory();
        } catch (err) {
            alert(`Could not start a new conversation: ${err.message || err}`);
        }
    });

    /* -------------------------------------------------------------------- */
    /* The terminal.                                                        */
    /*                                                                      */
    /* A real shell on a real pty, and the one part of this app the         */
    /* companion cannot reach: no tool names it, no HTTP route exposes it,  */
    /* and nothing it prints is written anywhere the companion can read.    */
    /* scripts/check_terminal_isolation.sh fails the build if any of those  */
    /* three stops being true.                                              */
    /*                                                                      */
    /* In a browser the panel is removed from the DOM rather than disabled. */
    /* There is no route behind it there, and a terminal that looks like it */
    /* works is worse than no terminal at all -- it also keeps it out of    */
    /* the module switch list, which would otherwise offer to turn on       */
    /* something that cannot exist.                                         */
    /* -------------------------------------------------------------------- */
    const terminalPanel = document.querySelector('[data-panel="terminal"]');
    if (terminalPanel && !IS_TAURI) {
        terminalPanel.remove();
    } else if (terminalPanel) {
        const terminalSurface = document.getElementById('terminal-surface');
        const terminalState = document.getElementById('terminal-state');
        const terminalNote = document.getElementById('terminal-note');
        const btnTerminalStart = document.getElementById('btn-terminal-start');
        let shell = null;

        const setState = (text) => { if (terminalState) terminalState.textContent = text; };

        btnTerminalStart?.addEventListener('click', async () => {
            voiceEngine.playSFX('click');
            if (shell && shell.running()) {
                /* The button is the whole control surface, so it has to be able to undo
                   itself -- otherwise closing a shell means closing the app. */
                await shell.stop();
                terminalSurface.classList.add('hidden');
                terminalNote?.classList.remove('hidden');
                btnTerminalStart.textContent = '▶ Start a shell';
                setState('not started');
                return;
            }
            try {
                terminalSurface.classList.remove('hidden');
                terminalNote?.classList.add('hidden');
                if (!shell) {
                    shell = Aether1Terminal.mount(terminalSurface, {
                        invoke: tauriInvoke,
                        listen: (name, handler) => window.__TAURI__.event.listen(name, handler),
                        onState: setState,
                    });
                }
                await shell.start();
                btnTerminalStart.textContent = '⏹ Close the shell';
            } catch (err) {
                terminalSurface.classList.add('hidden');
                terminalNote?.classList.remove('hidden');
                setState('could not start');
                console.warn('Could not start a shell', err);
                alert(`Could not start a shell: ${err.message || err}`);
            }
        });

        /* A panel switched off has no layout box, so xterm cannot measure it; switched
           back on it needs to be told its size before the shell draws at the old one. */
        document.addEventListener('aether1:panels-changed', () => shell?.fit());
    }

    document.getElementById('btn-notes')?.addEventListener('click', openNotesReader);
    document.getElementById('btn-read-notes')?.addEventListener('click', () => {
        settingsModal.classList.add('hidden');
        openNotesReader();
    });
    document.getElementById('btn-notes-open-folder')?.addEventListener('click', openVaultFolder);

    /* -------------------------------------------------------------------- */
    /* The graph.                                                           */
    /*                                                                      */
    /* The same notes, drawn instead of listed: a dot per note, a line per   */
    /* [[link]]. It is the one view that shows what a list cannot -- which   */
    /* notes everything points at, and which ones are drifting on their own  */
    /* because nothing links to them any more.                              */
    /*                                                                      */
    /* The drawing lives in js/notes-graph.js and does no fetching. This is  */
    /* the only place that knows whether the answer comes over IPC or HTTP,  */
    /* which is the same split every other feature here uses.               */
    /* -------------------------------------------------------------------- */

    const notesReaderView = document.getElementById('notes-reader-view');
    const notesGraphView = document.getElementById('notes-graph-view');
    const notesGraphCanvas = document.getElementById('notes-graph-canvas');
    const notesGraphCount = document.getElementById('notes-graph-count');
    const notesGraphNote = document.getElementById('notes-graph-note');
    const btnNotesGraph = document.getElementById('btn-notes-graph');

    /* Mounted on first use and kept, so flipping between the list and the
       picture does not re-run the layout and hand you a different arrangement
       of the same vault every time. */
    let noteGraph = null;
    let graphShowing = false;

    async function fetchGraph() {
        if (IS_TAURI) return tauriInvoke('vault_graph_rust');
        const res = await apiFetch('/api/vault/graph');
        if (!res.ok) throw new Error(`status ${res.status}`);
        return res.json();
    }

    function showReaderView() {
        graphShowing = false;
        notesGraphView.classList.add('hidden');
        notesReaderView.classList.remove('hidden');
        if (noteGraph) noteGraph.stop();
        if (btnNotesGraph) btnNotesGraph.textContent = '🕸 Graph';
    }

    async function showGraphView() {
        graphShowing = true;
        notesReaderView.classList.add('hidden');
        notesGraphView.classList.remove('hidden');
        if (btnNotesGraph) btnNotesGraph.textContent = '📄 List';
        if (!noteGraph) {
            noteGraph = window.Aether1NoteGraph.mount(notesGraphCanvas, {
                onOpenNote: (name) => {
                    /* Clicking a dot is asking to read that note, so it lands in
                       the reader rather than opening something over the graph. */
                    showReaderView();
                    noteTrail = [];
                    openNote(name);
                },
            });
        } else {
            noteGraph.resume();
        }
        notesGraphCount.textContent = 'Reading the folder…';
        notesGraphNote.classList.add('hidden');
        try {
            const graph = await fetchGraph();
            const nodes = (graph.nodes || []).length;
            const edges = (graph.edges || []).length;
            notesGraphCount.textContent =
                `${nodes} note${nodes === 1 ? '' : 's'} · ${edges} link${edges === 1 ? '' : 's'}`;
            if (graph.partial) {
                /* Being told the picture is incomplete matters more than the
                   picture: a missing line here looks exactly like a note nobody
                   linked, and those two things mean opposite things. */
                notesGraphNote.textContent =
                    'This vault is bigger than the graph shows — only the first notes found are drawn.';
                notesGraphNote.classList.remove('hidden');
            }
            noteGraph.show(graph);
        } catch (err) {
            notesGraphCount.textContent = `Could not read the notes folder: ${err.message || err}`;
        }
    }

    btnNotesGraph?.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        if (graphShowing) showReaderView();
        else showGraphView();
    });

    document.getElementById('btn-notes-graph-fit')?.addEventListener('click', () => {
        if (noteGraph) noteGraph.fit();
    });

    document.getElementById('btn-close-notes').addEventListener('click', () => {
        voiceEngine.playSFX('click');
        notesModal.classList.add('hidden');
        /* Closed means stopped. The canvas keeps its layout, so this is the one
           place that has to say so. */
        if (noteGraph) noteGraph.stop();
    });

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
        refreshBrainStatus();
        showSettingsTab('customisation');
        settingsModal.classList.remove('hidden');
    });

    btnForgeIdentity.addEventListener('click', () => {
        handleGenesisForge(genesisPurposeInput.value);
    });

    btnScanSystem.addEventListener('click', () => {
        handleScanSystem();
    });

    /* Open Folder. The desktop app can hand the folder to the operator's own file manager;
       a browser cannot, and shouldn't -- a phone on the LAN asking the desktop to pop open a
       window is not a feature anybody wanted, so there is no HTTP route behind this. There
       the button reports the path and copies it instead, which is the whole of what a
       browser can honestly do. */
    const btnOpenVault = document.getElementById('open-vault-folder');
    const vaultFolderStatus = document.getElementById('vault-folder-status');
    if (btnOpenVault) {
        btnOpenVault.addEventListener('click', async () => {
            voiceEngine.playSFX('click');
            const say = (text, ok = true) => {
                if (!vaultFolderStatus) return;
                vaultFolderStatus.textContent = text;
                vaultFolderStatus.className = `text-[10px] font-mono ${ok ? 'text-cyan-300' : 'text-amber-300'}`;
            };
            if (IS_TAURI) {
                try {
                    say(`opened ${await tauriInvoke('open_vault_folder_rust')}`);
                } catch (e) {
                    say(`could not open it: ${e.message || e}`, false);
                }
                return;
            }
            const typed = document.getElementById('setting-vault-path').value.trim();
            const path = typed || '~/Aether1Vault';
            try {
                await navigator.clipboard.writeText(path);
                say(`${path} -- copied; open it yourself, this tab is not on that machine`);
            } catch (e) {
                say(`${path} -- open it yourself, this tab is not on that machine`);
            }
        });
    }

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
                await tauriInvoke('clear_messages_rust', { sessionId: currentSessionId });
            } else {
                await apiFetch(`/api/messages?session_id=${encodeURIComponent(currentSessionId)}`, { method: 'DELETE' });
            }
            chatContainer.innerHTML = '';
            appendMessage(currentAgentName, 'Conversation logs cleared. Ready.');
        }
    });

    /* Forgets every speed reading. Confirmed rather than instant: the readings are the only
       record of how this machine performs, they take real conversations to rebuild, and the
       button sits on a panel the operator may well be clicking around to read. */
    document.getElementById('speed-reset')?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        if (!confirm('Forget every model speed reading and start measuring again?')) return;
        try {
            if (IS_TAURI) {
                await tauriInvoke('reset_benchmarks_rust');
            } else {
                await apiFetch('/api/benchmarks/reset', { method: 'POST' });
            }
            const board = document.getElementById('speed-board');
            if (board) board.textContent = '';
        } catch (err) {
            console.warn('[AETHER1] could not reset the speed readings:', err);
        }
    });

    /* Sound effects are reachable from two places -- this menu button and the checkbox in
       Settings under Voice & Sound -- and both are the same switch. Before this the menu
       button only flipped a field on the engine: it forgot on reload, and `enable_sfx`,
       stored and writable by the companion itself, was read by nothing at all. */
    function applySfx(on) {
        voiceEngine.sfxEnabled = on;
        if (btnSfxToggle) btnSfxToggle.textContent = on ? '🔊 SFX: ON' : '🔇 SFX: OFF';
        const box = document.getElementById('setting-sfx');
        if (box) box.checked = on;
    }

    btnSfxToggle.addEventListener('click', () => {
        applySfx(!voiceEngine.sfxEnabled);
        voiceEngine.playSFX('click');
        // Saved without closing anything or announcing it: this is a menu toggle, and the
        // one thing it must do that it did not before is survive a restart.
        saveSettings(false);
    });

    document.getElementById('setting-sfx')?.addEventListener('change', (e) => {
        applySfx(e.target.checked);
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
    // Strictly after the approvals, for the same reason they come after the history: the
    // banner is appended to the chat container, and loadChatHistory empties it.
    loadChatHistory().then(refreshPendingApprovals).then(announceIfNoBrain);
    connectTelemetry();
    initVersionAndUpdates();
    initSpriteMode();
    initStartupPerformance();
    initSoloPanel();
    initPanelUndock();
    initHologramFloatingNotice();
    initSpriteListenBridge();
    runVoiceStartupSelfTest();

    document.body.addEventListener('click', () => {
        voiceEngine.playSFX('boot');
    }, { once: true });
});
