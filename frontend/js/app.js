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

/* Every request to the backend goes through here, which is what makes one pairing gate
 * enough: under `--serve --lan` the server refuses anything without a per-device token, and
 * this is the single place a token can be attached and a refusal noticed. lan-auth.js is a
 * no-op on loopback and in the desktop shell, so nothing changes for either. */
function apiFetch(path, options) {
    const auth = window.LanAuth;
    return fetch(API_BASE + path, auth ? auth.authorize(options) : options)
        .then(response => {
            if (auth && auth.isUnauthorized(response)) auth.onUnauthorized();
            return response;
        });
}

/* The credential for the `/ws/*` routes. Not the same one: a socket is opened with a
 * single-use ticket, bought with the device token at `/api/ws-ticket`, so a handshake that
 * ends up in a log is worth nothing a minute later. It travels as a WebSocket subprotocol
 * because the constructor's second argument is the one handshake header a browser lets a
 * page set.
 *
 * Asynchronous because buying the ticket is a round trip. Every caller must await it; a
 * socket handed this Promise instead of the array it resolves to fails obscurely. */
function apiWsProtocols() {
    return window.LanAuth ? window.LanAuth.wsProtocols() : Promise.resolve(['aether1']);
}

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');
    const voiceEngine = new VoiceAudioEngine();

    /* Which turn's speech is allowed to reach the queue.
       A reply is spoken sentence by sentence as it streams, so at any moment some of it is
       still inside a synthesizer and has no clip yet. `stopSpeech()` can only drop what has
       already been queued; the sentences still being synthesized arrive afterwards and queue
       themselves, which is one reply's words landing in the middle of the next one's. Every
       chunk carries the turn it belongs to and is discarded on arrival if the turn has moved
       on. */
    let speechTurn = 0;
    function supersedeSpeech() {
        speechTurn += 1;
        voiceEngine.stopSpeech();
    }

    /* Stop talking, on purpose, with nothing taking its place.

       This is the same cut a new question makes, and it exists separately because the only
       ways to get silence were to ask something else or to hold the talk key -- both of which
       start something. Ask for `diagnostics` on a machine with a bad morning behind it and the
       answer is minutes long; there was no way to say "enough". Escape, or the button that
       appears beside Send while there is something to stop.

       Both halves matter. `stopSpeech` drops what is queued and cuts the clip playing; the
       turn bump is what stops the sentences still inside the synthesizer from queueing
       themselves a moment later, which is what made an earlier attempt at this feel broken. */
    let speaking = false;
    function hush() {
        if (!speaking) return false;
        supersedeSpeech();
        setSpeakingUi(false);
        return true;
    }

    /* Whether there is anything to stop. Driven by the engine's own state rather than by
       "we started a reply", so it is false again the moment the last clip drains, and the
       button does not linger over silence. */
    function setSpeakingUi(isSpeaking) {
        speaking = isSpeaking;
        const btn = document.getElementById('btn-hush');
        if (btn) btn.classList.toggle('hidden', !isSpeaking);
    }

    // DOM Elements
    const chatContainer = document.getElementById('chat-messages');
    const chatInput = document.getElementById('chat-input');
    const chatMediaInput = document.getElementById('chat-media-input');
    const chatMediaButton = document.getElementById('btn-chat-media');
    const chatMediaPreviews = document.getElementById('chat-media-previews');
    let pendingChatImages = [];
    const btnSend = document.getElementById('btn-send');
    const btnMic = document.getElementById('btn-mic');
    const btnSettings = document.getElementById('btn-settings');
    const btnClearChat = document.getElementById('btn-clear-chat');
    const btnSfxToggle = document.getElementById('btn-sfx');
    const settingsModal = document.getElementById('settings-modal');
    const btnCloseSettings = document.getElementById('btn-close-settings');
    const btnSaveSettings = document.getElementById('btn-save-settings');
    const btnAgentBrowser = document.getElementById('btn-agent-browser');
    const agentBrowserModal = document.getElementById('agent-browser-modal');
    const btnCloseAgentBrowser = document.getElementById('btn-close-agent-browser');
    const agentBrowserContainer = document.getElementById('agent-browser-container');

    /* ---- Settings: the section rail ------------------------------------------
     * Headings on the left, the chosen section's detail on the right. Every pane
     * stays in the DOM; this only ever moves the is-active class, so nothing that
     * reads or writes a settings field by id needs to know the layout changed.
     *
     * One pane is platform-dependent (Startup & Performance) and starts with its rail
     * entry hidden -- revealSettingsSection is how initStartupPerformance turns it on.
     * An entry that is hidden cannot be chosen, including out of the remembered choice
     * below. Desktop Sprite and Network & Remote used to be two more such entries; they are
     * cards inside Display and Network & Remote now (see SETTINGS_SECTION_ALIASES), since
     * each answered the same question as the section it joined -- where this thing is drawn
     * on screen, and what crosses the edge of this machine.
     *
     * A pane whose body is still a <details> is opened when it is chosen, which is
     * what keeps the two groups that probe the machine on open (the coding group,
     * the doctor) probing exactly when someone goes looking at them.
     */
    const SETTINGS_SECTION_KEY = 'aether_settings_section';
    // Sections that have been folded into another one. Only the remembered choice can still
    // name one, so this is what stops somebody who was last in Desktop Sprite from being
    // dropped back at the top of the rail the first time they open Settings after updating.
    const SETTINGS_SECTION_ALIASES = { sprite: 'layout', lan: 'network', avatars: 'appearance' };
    const settingsNav = document.getElementById('settings-nav');
    const settingsNavEmpty = document.getElementById('settings-nav-empty');
    const settingsSearch = document.getElementById('settings-search');
    const connectionsHost = document.getElementById('settings-connections-host');
    const connectionsGroup = document.getElementById('settings-group-connections');
    if (connectionsHost && connectionsGroup) connectionsHost.appendChild(connectionsGroup);
    const settingsNavItems = () => Array.from(document.querySelectorAll('.settings-nav-item'));
    const settingsPaneFor = (name) => document.querySelector(`.settings-pane[data-settings-section="${name}"]`);
    const settingsNavFor = (name) => document.getElementById(`settings-nav-${name}`);
    // The Advanced group in the rail: the sections from "What it may do" down, folded away
    // because they are set once and then only visited when something is wrong. Nothing
    // about a section changes by being in it -- it is a <details> around five rail entries,
    // and it is opened whenever one of them is the section being shown.
    const settingsNavAdvanced = document.getElementById('settings-nav-advanced');
    const settingsNavIsAdvanced = (item) => !!item && !!settingsNavAdvanced?.contains(item);

    function showSettingsSection(name) {
        const item = settingsNavFor(name === 'avatars' ? 'appearance' : name);
        if (!item || item.classList.contains('hidden')) return false;
        // An entry folded inside Advanced would otherwise be marked active out of sight,
        // which is how the remembered section arrives after a restart.
        if (settingsNavIsAdvanced(item) && settingsNavAdvanced) settingsNavAdvanced.open = true;
        settingsNavItems().forEach(btn => {
            btn.classList.toggle('is-active', btn === item);
            btn.setAttribute('aria-current', btn === item ? 'true' : 'false');
        });
        document.querySelectorAll('.settings-pane').forEach(pane => {
            pane.classList.toggle('is-active', pane.dataset.settingsSection === name);
        });
        // Kept open rather than pressed: the rail is the heading now, so a group that
        // probes on open (coding, doctor) gets its one probe when the section is chosen.
        const group = settingsPaneFor(name)?.querySelector(':scope > .settings-group');
        if (group) group.open = true;
        // The avatar browser owns a WebGL context while its detail view is open, so
        // leaving the section has to put it down -- a rail that only changes which pane is
        // visible would otherwise leave a second hologram rendering behind the Network page.
        if (name === 'avatars') openAvatarBrowser();
        else if (typeof disposeAvatarPreview === 'function') disposeAvatarPreview();
        // Same reasoning as the avatar browser's: the stage you were on is only meaningful
        // while you are in the section. Coming back to Appearance should land on the pane,
        // not halfway inside the colour panel you left open yesterday.
        if (name !== 'appearance' && name !== 'avatars') showAppearanceStage('main');
        const detail = document.getElementById('settings-detail');
        if (detail) detail.scrollTop = 0;
        try { localStorage.setItem(SETTINGS_SECTION_KEY, name === 'avatars' ? 'appearance' : name); } catch (e) { /* private mode */ }
        // A section that fills itself when it is chosen listens for this rather than being
        // called from here: the panes are set up further down the file, and this runs
        // before them when the remembered section is restored.
        document.dispatchEvent(new CustomEvent('aether-settings-section', { detail: name }));
        return true;
    }

    // A section the platform can actually offer. Hidden in the markup, shown from
    // initSpriteMode/initStartupPerformance once IS_TAURI is confirmed.
    //
    // The search box below marks and unmarks .hidden as you type, so it has to be
    // able to tell "filtered out" from "this platform hasn't got one". That is what
    // the available flag is for: it is set here and read there, and nowhere else.
    function revealSettingsSection(name) {
        const item = settingsNavFor(name);
        if (!item) return;
        item.dataset.available = 'true';
        item.classList.remove('hidden');
    }

    settingsNav?.addEventListener('click', (event) => {
        const item = event.target.closest('.settings-nav-item');
        if (!item) return;
        voiceEngine.playSFX('click');
        showSettingsSection(item.dataset.settingsSection);
    });

    settingsModal.addEventListener('click', (event) => {
        const suggestion = event.target.closest('[data-prompt-suggestion]');
        if (!suggestion) return;
        const input = document.getElementById('chat-input');
        if (!input) return;
        input.value = suggestion.dataset.promptSuggestion;
        settingsModal.classList.add('hidden');
        input.focus();
    });

    // Filters the rail by heading, by the plain-words hint beside it, and by a list of
    // words for what is actually inside the section -- so "voice" finds Voice & Sound
    // and "model", "api key" or "ollama" all find The Brain, which is the search anyone
    // arriving with a problem will type. Filtering never changes which section is
    // showing, only which headings are offered.
    settingsSearch?.addEventListener('input', () => {
        const q = settingsSearch.value.trim().toLowerCase();
        let shown = 0;
        let advancedMatched = false;
        settingsNavItems().forEach(item => {
            if (item.dataset.available === 'false') return;
            const hay = `${item.textContent} ${item.dataset.hint || ''} ${item.dataset.keywords || ''}`.toLowerCase();
            const match = !q || hay.includes(q);
            item.classList.toggle('hidden', !match);
            if (match) shown += 1;
            if (match && settingsNavIsAdvanced(item)) advancedMatched = true;
        });
        settingsNavEmpty?.classList.toggle('hidden', shown > 0);
        // While there is something typed, Advanced follows the search: open when a match is
        // hidden inside it, shut when nothing in it matches. Clearing the box hands it back
        // to the section being shown rather than leaving it however the last query left it.
        if (settingsNavAdvanced) {
            settingsNavAdvanced.classList.toggle('hidden', !!q && !advancedMatched);
            settingsNavAdvanced.open = q
                ? advancedMatched
                : settingsNavIsAdvanced(document.querySelector('.settings-nav-item.is-active'));
        }
    });

    settingsNavItems().forEach(item => {
        item.dataset.available = item.classList.contains('hidden') ? 'false' : 'true';
    });

    // The section you were last in, so coming back to change a second thing does not
    // start from the top again. Falls through to the first heading in the rail if that
    // section has since been hidden, or nothing was remembered.
    function restoreSettingsSection() {
        let remembered = null;
        try { remembered = localStorage.getItem(SETTINGS_SECTION_KEY); } catch (e) { /* private mode */ }
        if (remembered) remembered = SETTINGS_SECTION_ALIASES[remembered] || remembered;
        if (remembered && showSettingsSection(remembered)) return;
        const first = settingsNavItems().find(item => !item.classList.contains('hidden'));
        if (first) showSettingsSection(first.dataset.settingsSection);
    }

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
    const scannerResultsBox = document.getElementById('scanner-results-box');

    // LLM Test Connection Elements
    const btnTestConnection = document.getElementById('btn-test-connection');
    const testConnectionStatus = document.getElementById('test-connection-status');

    // Version & Update Elements (native desktop app only -- see IS_TAURI below)
    const versionBadge = document.getElementById('version-badge');
    const headerUpdateButton = document.getElementById('btn-header-update');
    const headerUpdateProgress = document.getElementById('header-update-progress');
    const headerUpdateProgressBar = document.getElementById('header-update-progress-bar');
    const headerUpdateProgressText = document.getElementById('header-update-progress-text');
    const updateSection = document.getElementById('update-section');
    const settingsVersionLabel = document.getElementById('settings-version-label');
    const updateStatusBox = document.getElementById('update-status-box');
    const btnCheckUpdate = document.getElementById('btn-check-update');
    const btnGithubSignIn = document.getElementById('btn-github-signin');
    const btnGithubDecline = document.getElementById('btn-github-decline');
    const btnDownloadUpdate = document.getElementById('btn-download-update');
    const btnApplyUpdate = document.getElementById('btn-apply-update');

    // Hardware telemetry, which lives in the chin bar rather than in a panel of its own:
    // one cell per reading, each a label, a value and (where the reading is a percentage of
    // something) a hairline meter. Same payload it always read -- only where it is drawn
    // changed. See #chin-telemetry in index.html.
    const elCpuCell = document.getElementById('chin-stat-cpu');
    const elCpuVal = document.getElementById('chin-cpu-val');
    const elCpuMeter = document.getElementById('chin-cpu-meter');
    const elRamCell = document.getElementById('chin-stat-ram');
    const elRamVal = document.getElementById('chin-ram-val');
    const elRamMeter = document.getElementById('chin-ram-meter');
    const elDiskCell = document.getElementById('chin-stat-disk');
    const elDiskVal = document.getElementById('chin-disk-val');
    const elDiskMeter = document.getElementById('chin-disk-meter');
    const elNetCell = document.getElementById('chin-stat-net');
    const elNetVal = document.getElementById('chin-net-val');
    const elGpuCell = document.getElementById('chin-stat-gpu');
    const elGpuVal = document.getElementById('chin-gpu-val');
    const elBatteryCell = document.getElementById('chin-stat-battery');
    const elBatteryVal = document.getElementById('chin-battery-val');
    const elBatteryMeter = document.getElementById('chin-battery-meter');
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

    /* The operator's own name, as the profile knows it -- kept here once it has been read
       (loadOperatorNameAndGreeting) so that anything else wanting to address them by name
       does not fetch the profile again. Declared beside currentAvatar for the same reason
       the avatar browser's state is: it is read from code that runs earlier in the session
       than the function that fills it. */
    let operatorDisplayName = '';

    /* The avatar browser's state, declared up here with the avatar it follows rather than
       down beside its own functions: the first applyAvatar of the session runs before that
       point, and it asks the browser to redraw. A `let` further down would still be in its
       temporal dead zone at that moment, which is an exception thrown during startup for
       the sake of tidier grouping. */
    const AVATAR_STAGES = ['groups', 'members', 'detail'];
    let avatarBrowserGroup = null;      // which line stage 2 is showing
    let avatarBrowserSelection = null;  // which avatar stage 3 is showing
    let avatarPreviewEngine = null;
    let avatarBrowserBuilt = false;
    /* The last answer from /api/flow: whether hand-offs are on, the line they may move
       within, the line picked whole if there is one, and which lines have personas behind
       them at all. The browser draws from this copy rather than awaiting a fetch in the
       middle of a render; refreshFlowMode is what keeps it current. */
    let flowState = null;

    /* The persona catalogue's rows themselves, not just the <option>s built from them: the
       avatar browser wants a persona's speciality, access and voice beside its avatar, and
       reading them back off the dropdown's dataset would mean smuggling every field through
       an attribute first. Up here for the same reason as the state above. */
    let personaRows = new Map();
    function personaRow(key) { return personaRows.get(key) || null; }
    /* The two voice lists the avatar's own picker is built from, fetched once and kept:
       they are a table in the binary plus a directory listing, and the pane is opened
       and closed a dozen times while somebody is choosing. Up here with personaRows for
       the same reason -- refreshAvatarBrowser runs before this file's later declarations
       (see openAvatarDetail), and a `let` further down would be a TDZ error at startup. */
    let voicePickerLists = null;
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
        if (e.key !== 'Escape') return;
        /* Silence first, and only silence: if it was talking, Escape means "be quiet" and
           nothing else, so a press that stops a long answer does not also close the panel
           the operator was reading. Nothing is being spoken -- the usual case -- and Escape
           is the menu key it always was. */
        if (hush()) return;
        closeHudMenus();
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
        else if (avatarName === 'enxephalon') avatarStructureLabel.textContent = 'SYNAPTIC MIND LATTICE';
        else if (avatarName === 'cicero') avatarStructureLabel.textContent = 'HOVERING REEL CHASSIS';
        else if (avatarName === 'praxis') avatarStructureLabel.textContent = 'HARD-LIGHT TRAINING GRID';
        else if (avatarName === 'chrono-maistresse') avatarStructureLabel.textContent = 'ANIMATE CHRONOMETER DIAL';
        else if (avatarName === 'mairad') avatarStructureLabel.textContent = 'MUTATIVE RESPONSE SHELL';
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
        // Picking one character is a decision against the cast: a line picked whole
        // outranks whoever is on screen (see llm/flow.rs), so leaving it in force here
        // would send the next question back into the old line the moment it matched.
        // Only a real pick counts -- a restored avatar at startup is not one.
        if (updatePersona) releasePickedLineFor(avatarName);
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

        // Settings' avatar browser shows the current pick in three places at once, so
        // it redraws rather than having a class toggled on one card here.
        refreshAvatarBrowser();

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
            greeting: "🔵 **Archival, Reasoning, matriX — Locas Node engaged.** Your day, your plan and this machine, all on one desk. What do you need?"
        },
        'arx-legionare': {
            name: 'A.R.X.LEGIONARE', persona: 'arx-legionare', voice: 'en-US-DavisNeural',
            greeting: '🔴 **Archival, Reasoning, matriX — Legionare Node engaged.** Tell me what you run and I will tell you what reaches it first.'
        },
        'arx-loregenda': {
            name: 'A.R.X.LOREGENDA', persona: 'arx-loregenda', voice: 'en-GB-ThomasNeural',
            greeting: '🔷 **Archival, Reasoning, matriX — Loregenda Node engaged.** Bring me the world you are building. Nothing new gets to contradict what is already written.'
        },
        'arx-lyksaum': {
            name: 'A.R.X.LYKSAUM', persona: 'arx-lyksaum', voice: 'en-AU-NatashaNeural',
            greeting: "🩵 **Archival, Reasoning, matriX — Lyksaum Node engaged.** Ask me twice if the first answer did not land. I will write it down either way."
        },
        'arx-limes': {
            name: 'A.R.X.LIMES', persona: 'arx-limes', voice: 'en-US-GuyNeural',
            greeting: '🔶 **Archival, Reasoning, matriX — Limes Node engaged.** Name the target. I sweep it, and I bring back what is in it — with where each piece came from.'
        },
        'arx-logos': {
            name: 'A.R.X.LOGOS', persona: 'arx-logos', voice: 'en-GB-LibbyNeural',
            greeting: '🟣 **Archival, Reasoning, matriX — Logos Node engaged.** Sound, pattern, and the logic underneath both. Give me the signal.'
        },
        'arx-lexico': {
            name: 'A.R.X.LEXICO', persona: 'arx-lexico', voice: 'en-GB-RyanNeural',
            greeting: '🧊 **Archival, Reasoning, matriX — Lexico Node engaged.** Ask me what is true, and I will tell you how far the source it came from actually goes.'
        },
        'arx-lucre': {
            name: 'A.R.X.LUCRE', persona: 'arx-lucre', voice: 'en-US-EricNeural',
            greeting: '💰 **Archival, Reasoning, matriX — Lucre Node engaged.** Every choice has a cost. Let us make sure it is seen before it is spent.'
        },
        'arx-lkemi': {
            name: "A.R.X.L'KEMI", persona: 'arx-lkemi', voice: 'en-AU-WilliamNeural',
            greeting: "🔻 **Archival, Reasoning, matriX — L'kemi Node engaged.** Bring me what you are building, or what needs turning into something better."
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
        // The browser builds its grids from the catalogue rather than from hidden markup,
        // so it is filtered by avatarIsVisible and has to be asked again, not unhidden.
        refreshAvatarBrowser();
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
        if (Aether1AvatarUnlocks.unlock(avatarId)) {
            refreshTraceProtocolVisibility();
            refreshEmptyAvatarGroups();
        }

        if (traceProtocolFlashTimer) clearTimeout(traceProtocolFlashTimer);
        setHologramAvatar(avatarId);
        traceProtocolFlashTimer = setTimeout(() => {
            traceProtocolFlashTimer = null;
            setHologramAvatar(currentAvatar);
        }, TRACE_PROTOCOL_FLASH_MS);
    }

    /* A family heading with nothing under it is just a stray divider, so each one is shown
       only while its grid has a pill someone can actually press. That covers two cases: a
       group standing ready for avatars that have not been built yet (The eXcelsior Class),
       and one whose every member is still an undiscovered Trace Protocol. Each heading is
       paired with the grid immediately after it -- see #avatar-menu in index.html. */
    function refreshEmptyAvatarGroups() {
        document.querySelectorAll('#avatar-menu .avatar-group-label').forEach((heading) => {
            const grid = heading.nextElementSibling;
            // Compared against null rather than coerced with a double negation, because
            // Tailwind's scanner reads this file as plain text looking for class names and
            // a negated identifier can read as an important-flagged utility to it, which
            // would bake a rule nothing uses into vendor/tailwind.css on every rebuild.
            // See scripts/build_vendor_css.sh.
            const filled = grid !== null && grid.querySelector('.avatar-pill:not(.hidden)') !== null;
            heading.classList.toggle('hidden', !filled);
            if (grid) grid.classList.toggle('hidden', !filled);
        });
    }

    // A pick already sitting in currentAvatar -- restored from localStorage on load -- is
    // discovered by definition, whether that's because its trigger fired in an earlier
    // session (before this browser's unlock flags existed) or it was chosen back when these
    // three were still plain, unhidden entries in the picker. Recording the unlock keeps its
    // button from vanishing out from under an avatar that is still the active one.
    if (TRACE_PROTOCOL_AVATAR_IDS.includes(currentAvatar)) Aether1AvatarUnlocks.unlock(currentAvatar);
    refreshTraceProtocolVisibility();
    refreshEmptyAvatarGroups();

    /* ---- THE AVATAR BROWSER (Settings -> Avatars) -------------------------------
     *
     * Three stages in one pane: the lines, one line's members, one avatar in full. The
     * shape of it is Trident's: customisation leads to the group, the group leads to the
     * avatar, and the avatar opens into who it is and what it does.
     *
     * Everything rendered here comes from js/avatar-catalogue.js (the descriptions) joined
     * to the backend's persona catalogue (speciality, access and voice) on the avatar's
     * persona key. Neither half is restated in markup, so an avatar added to the catalogue
     * appears here with no change to this file, and a persona's speciality reworded in
     * persona.rs reaches this pane without anyone remembering to copy it across.
     *
     * The detail view runs a second, live HologramAvatar rather than showing a still: half
     * of what distinguishes one avatar from another is how it moves -- C.I.C.E.R.O.'s reels,
     * Chrono-mAIstresse's hands, the Operator's stream -- and a frozen frame of those three
     * is the same picture. It is built when a detail view opens and disposed the moment one
     * closes, so the second WebGL context exists only while it is on screen.
     */
    const avatarCatalogue = () => window.Aether1Avatars || null;

    /* An avatar the operator is allowed to see. The three Trace Protocols eggs stay out of
       every list until their trigger has fired once (see refreshTraceProtocolVisibility),
       and this is the one predicate that decides it for the whole browser -- the grid, the
       member count on the line's card, and the "something is missing here" note. */
    function avatarIsVisible(entry) {
        if (!entry.unlockable) return true;
        return Aether1AvatarUnlocks.isUnlocked(entry.id);
    }

    function visibleAvatarsIn(groupId) {
        const cat = avatarCatalogue();
        return cat ? cat.inGroup(groupId).filter(avatarIsVisible) : [];
    }

    /* ---- A whole line as the pick ------------------------------------------
       An avatar is a character; a line is a cast. Picking the cast means the question goes
       to whichever of its nodes owns the subject, and the one on screen is whoever is
       holding it at that moment -- so the pick survives every hand-off, which wearing a
       member of the line does not. The rule itself is llm/flow.rs; these three are the
       state it is drawn from. */
    function pickedLine() {
        return (flowState && flowState.line) || null;
    }

    /* Not every line is a cast. The eXcelsior Class is five shapes with no directives
       behind them yet, and Trace Protocols is one persona and three eggs that carry none --
       picking either would promise a group and deliver a single node. The backend says
       which lines it can actually pass a question around, rather than this being guessed
       at here from the catalogue. */
    function lineCanBePicked(groupName) {
        return Boolean(flowState && Array.isArray(flowState.lines)
            && flowState.lines.includes(groupName));
    }

    /* Releases the picked line unless the avatar being worn is one of its own. Wearing
       another member of the same cast is not a change of mind about the cast. */
    function releasePickedLineFor(avatarName) {
        const line = pickedLine();
        if (!line) return;
        const cat = avatarCatalogue();
        const entry = cat && cat.get(avatarName);
        const group = entry && cat.group(entry.group);
        if (group && group.name === line) return;
        setFlowLine(null);
    }

    async function setFlowLine(name) {
        let state = null;
        try {
            if (IS_TAURI) {
                state = await tauriInvoke('set_flow_line_rust', { line: name });
            } else {
                const resp = await apiFetch('/api/flow/line', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ line: name }),
                });
                if (resp.ok) state = await resp.json();
            }
        } catch (e) {
            console.error('Could not change the picked line', e);
        }
        if (!state) return;
        flowState = state;
        /* Picking a line the operator was standing outside of lands them on its anchor --
           the cast's generalist -- and the backend has already written that persona. The
           HUD wears the matching avatar so the hologram agrees with who is answering,
           greeting included: it is a change of who you are talking to, and a silent one
           would be the hologram lying about it. */
        const personaSelect = document.getElementById('setting-persona');
        const answering = state.persona || (personaSelect && personaSelect.value) || null;
        if (answering) {
            const cat = avatarCatalogue();
            const entry = cat && cat.all.find((a) => a.persona === answering);
            // Also when the backend moved nobody: the operator can have been inside the
            // line by persona while wearing a shape from somewhere else, and a hologram
            // from another cast while this one answers is the confusion the pick exists
            // to remove.
            if (entry && entry.id !== currentAvatar) applyAvatar(entry.id, true);
        }
        refreshFlowMode();
        refreshAvatarBrowser();
    }

    function showAvatarStage(name) {
        AVATAR_STAGES.forEach((stage) => {
            document.querySelectorAll(`[data-avatar-stage="${stage}"]`).forEach((el) => {
                el.classList.toggle('is-active', stage === name);
            });
        });
        // The preview engine belongs to the detail stage and to nothing else. Leaving it
        // running behind a stage nobody is looking at is a render loop and a WebGL context
        // spent on an invisible element.
        if (name !== 'detail') disposeAvatarPreview();
        const detail = document.getElementById('settings-detail');
        if (detail) detail.scrollTop = 0;
    }

    // ---- Stage 1: the lines -------------------------------------------------
    function renderAvatarGroups() {
        const cat = avatarCatalogue();
        const grid = document.getElementById('avatar-group-grid');
        if (!cat || !grid) return;
        grid.innerHTML = '';
        cat.groups.forEach((group) => {
            const members = visibleAvatarsIn(group.id);
            // A line with nothing showing yet is still worth a card: The eXcelsior Class
            // stood empty for a while, and a line that simply is not there reads as a bug
            // rather than as something not built yet. Only a line with no members at all in
            // the catalogue is skipped.
            if (cat.inGroup(group.id).length === 0) return;

            const card = document.createElement('button');
            card.type = 'button';
            card.className = 'avatar-group-card';
            card.dataset.avatarGroup = group.id;

            const faces = document.createElement('span');
            faces.className = 'avatar-group-faces';
            faces.setAttribute('aria-hidden', 'true');
            faces.textContent = members.map((m) => m.emoji).join(' ');

            const name = document.createElement('span');
            name.className = 'avatar-group-name';
            name.textContent = group.name;

            const tagline = document.createElement('span');
            tagline.className = 'avatar-group-tagline';
            tagline.textContent = group.tagline;

            const blurb = document.createElement('span');
            blurb.className = 'avatar-group-blurb';
            blurb.textContent = group.blurb;

            const count = document.createElement('span');
            count.className = 'avatar-group-count';
            count.textContent = members.length === 1 ? '1 avatar' : `${members.length} avatars`;
            if (members.some((m) => m.id === currentAvatar)) {
                count.textContent += ' · wearing one of these';
                card.classList.add('is-current');
            }
            // Two different states worth telling apart on the same card: wearing a member
            // of this line, and having picked the line itself.
            if (pickedLine() === group.name) {
                count.textContent += ' · picked as a line';
                card.classList.add('is-current');
            } else if (lineCanBePicked(group.name)) {
                count.textContent += ' · can be picked whole';
            }

            card.append(faces, name, tagline, blurb, count);
            card.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                openAvatarGroup(group.id);
            });
            grid.appendChild(card);
        });
    }

    // ---- Stage 2: one line's members ----------------------------------------
    function openAvatarGroup(groupId) {
        const cat = avatarCatalogue();
        const group = cat && cat.group(groupId);
        if (!group) return;
        avatarBrowserGroup = groupId;

        document.getElementById('avatar-members-title').textContent = group.name;
        document.getElementById('avatar-members-blurb').textContent = group.blurb;

        const grid = document.getElementById('avatar-member-grid');
        grid.innerHTML = '';
        visibleAvatarsIn(groupId).forEach((entry) => {
            grid.appendChild(buildAvatarCard(entry));
        });

        /* How many of this line are still undiscovered, said plainly. The alternative -- a
           line that silently shows one of its four members -- is the version that reads as
           the list being wrong. It says the count and not which ones, which is the whole
           point of an egg. */
        const hidden = cat.inGroup(groupId).filter((a) => !avatarIsVisible(a)).length;
        const note = document.getElementById('avatar-members-locked');
        note.classList.toggle('hidden', hidden === 0);
        if (hidden > 0) {
            note.textContent = hidden === 1
                ? 'One more member of this line has not turned up yet.'
                : `${hidden} more members of this line have not turned up yet.`;
        }

        renderLinePick(group);
        showAvatarStage('members');
    }

    /* The button that picks a line whole, and the sentence explaining what that does. It
       says the consequence rather than the setting: what changes is who answers the next
       question, and "flow_group" would mean nothing to the person reading it. */
    function renderLinePick(group) {
        const button = document.getElementById('btn-avatar-line');
        const note = document.getElementById('avatar-line-note');
        if (!button || !note) return;

        const picked = pickedLine() === group.name;
        const available = lineCanBePicked(group.name);
        button.classList.toggle('is-picked', picked);
        button.disabled = !available;

        if (!available) {
            const cat = avatarCatalogue();
            // One persona in the line and one alone is a different thing from none: the
            // first is a cast that has not grown yet, the second is shapes. Counted from
            // the catalogue rather than the visible list, so an unlocked egg does not
            // change the sentence and a locked one is not given away by it.
            const backed = cat ? cat.inGroup(group.id).filter((a) => a.persona).length : 0;
            button.textContent = 'Not a cast yet';
            note.textContent = backed === 1
                ? 'Only one of these carries a persona so far, so picking the line would be '
                    + 'the same as wearing that one. Do that instead, for now.'
                : 'These have shapes but no directives behind them, so there is nobody here '
                    + 'to hand a question to. Wearing one leaves whoever you had selected '
                    + 'answering.';
        } else if (picked) {
            button.textContent = 'Release this line';
            note.textContent = `${group.name} is answering. Each question goes to whichever of `
                + 'them owns the subject, and the one holding it says so before it moves. '
                + 'Releasing leaves you with whoever is answering at the time.';
        } else {
            button.textContent = 'Use this whole line';
            note.textContent = 'Pick the cast instead of one of its members: the question goes '
                + `to whichever node of ${group.name} owns it, and the hologram follows. You `
                + 'can still wear a single one of them below.';
        }

        button.onclick = () => {
            voiceEngine.playSFX('click');
            setFlowLine(picked ? null : group.name);
        };
    }

    function buildAvatarCard(entry) {
        const card = document.createElement('button');
        card.type = 'button';
        card.className = 'avatar-card';
        card.dataset.avatar = entry.id;
        if (entry.id === currentAvatar) card.classList.add('is-current');

        const face = document.createElement('span');
        face.className = 'avatar-card-face';
        face.setAttribute('aria-hidden', 'true');
        face.textContent = entry.emoji;

        const name = document.createElement('span');
        name.className = 'avatar-card-name';
        name.textContent = entry.label;

        const role = document.createElement('span');
        role.className = 'avatar-card-role';
        role.textContent = avatarRoleLine(entry);

        card.append(face, name, role);
        if (entry.id === currentAvatar) {
            const worn = document.createElement('span');
            worn.className = 'avatar-card-worn';
            worn.textContent = 'WEARING';
            card.appendChild(worn);
        }
        card.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            openAvatarDetail(entry.id);
        });
        return card;
    }

    /* The one line under an avatar's name in the grid. The persona's own short_name is the
       truest version of it -- it is what the backend calls that job -- so it wins whenever
       the avatar has a persona, and the catalogue's structure line stands in when it has
       none. */
    function avatarRoleLine(entry) {
        const persona = entry.persona ? personaRow(entry.persona) : null;
        if (persona && persona.short_name) return persona.short_name;
        return entry.structure;
    }

    // ---- Stage 3: one avatar, in full ---------------------------------------
    function openAvatarDetail(avatarId) {
        const cat = avatarCatalogue();
        const entry = cat && cat.get(avatarId);
        if (!entry) return;
        avatarBrowserSelection = avatarId;
        avatarBrowserGroup = entry.group;

        const group = cat.group(entry.group);
        document.getElementById('avatar-detail-line').textContent = group ? group.name : '';
        document.getElementById('avatar-back-group').textContent = group ? group.name : 'Back';
        document.getElementById('avatar-detail-name').textContent = `${entry.emoji} ${entry.label}`;
        document.getElementById('avatar-detail-form').textContent = entry.form;
        document.getElementById('avatar-detail-who').textContent = entry.who;
        document.getElementById('avatar-detail-does').textContent = entry.does;
        document.getElementById('avatar-preview-structure').textContent = `STRUCTURE: ${entry.structure}`;

        const persona = entry.persona ? personaRow(entry.persona) : null;

        // The persona's own one-liner, under the catalogue's. Two sentences about the same
        // job from two sources, which is worth it: one is what this avatar is for and the
        // other is the directive the model actually receives.
        const speciality = document.getElementById('avatar-detail-speciality');
        speciality.classList.toggle('hidden', !persona);
        if (persona) speciality.textContent = `Directive: ${persona.speciality}`;

        // Access and voice are facts about a persona, so an avatar without one has neither
        // to show. Showing the rows empty would suggest it reaches nothing and speaks in
        // nothing, when what is true is that it inherits both from whoever is selected.
        const accessRow = document.getElementById('avatar-detail-access-row');
        accessRow.classList.toggle('hidden', !persona || !persona.field);
        if (persona && persona.field) {
            document.getElementById('avatar-detail-access').textContent = persona.field;
        }

        const voiceRow = document.getElementById('avatar-detail-voice-row');
        voiceRow.classList.toggle('hidden', !persona);
        if (persona) renderAvatarVoice(persona);

        const isCurrent = avatarId === currentAvatar;
        const useBtn = document.getElementById('btn-avatar-use');
        useBtn.textContent = isCurrent ? 'Already wearing it' : 'Wear this avatar';
        useBtn.disabled = isCurrent;
        useBtn.onclick = () => {
            voiceEngine.playSFX('click');
            applyAvatar(avatarId, true);
            openAvatarDetail(avatarId);
        };
        document.getElementById('avatar-detail-current').classList.toggle('hidden', !isCurrent);

        // Every A.R.X. avatar, and five others, have a colour preset carrying their own id;
        // A1ter_nul's is Night. An avatar with no preset of its own simply does not offer
        // the button rather than offering one that would repaint the HUD in someone else's
        // colours.
        const presetId = avatarThemePreset(entry);
        const themeBtn = document.getElementById('btn-avatar-theme');
        themeBtn.classList.toggle('hidden', !presetId);
        if (presetId) {
            themeBtn.onclick = () => {
                voiceEngine.playSFX('click');
                applyThemePreset(presetId);
            };
        }

        // The workbench button belongs to the avatar you design and to nothing else.
        const labBtn = document.getElementById('btn-open-avatar-lab');
        if (labBtn) labBtn.classList.toggle('hidden', !entry.custom);

        const note = document.getElementById('avatar-preview-note');
        note.classList.toggle('hidden', Boolean(persona));
        if (!persona) {
            note.textContent = 'This one is a shape, not a persona: wearing it changes the '
                + 'hologram and leaves whoever is answering exactly as they are.';
        }

        showAvatarStage('detail');
        buildAvatarPreview(avatarId);
    }

    function avatarThemePreset(entry) {
        const byId = window.THEME_PRESETS_BY_ID || {};
        if (byId[entry.id]) return entry.id;
        if (entry.id === 'alt') return 'night-city';
        return null;
    }

    /* Both voices, named, because they are two different answers to "what does it sound
       like": the cloud voice and the Piper model on this machine. Which of the two actually
       speaks is decided at the moment of speech (see commands::synthesize_speech), and an
       operator running local-only wants to read the second one. */
    function avatarVoiceLine(persona) {
        const parts = [];
        if (persona.voice) parts.push(persona.voice);
        if (persona.local_voice) parts.push(`${persona.local_voice} (Piper)`);
        if (parts.length === 0) return 'Whatever voice you have chosen in Voice & Sound.';
        return parts.join('  ·  ');
    }

    /* ---- Giving one avatar a voice of your own choosing ---------------------
       The identity table in genesis.rs is where an avatar's voices come from, and it is the
       author's taste, not the operator's. This is where they disagree with it: two selects,
       saved the moment one changes, and a reset that removes the choice rather than writing
       today's default into it -- so an avatar put back to its own voice follows the table if
       the table ever changes. Nothing here touches the settings form, so it is safe to save
       on change even though the window has one Save Changes button. */
    async function fetchVoicePickerLists() {
        if (voicePickerLists) return voicePickerLists;
        const data = IS_TAURI
            ? await tauriInvoke('voice_pickers_rust')
            : await (await apiFetch('/api/voice/pickers')).json();
        voicePickerLists = data;
        return voicePickerLists;
    }

    function renderAvatarVoice(persona) {
        document.getElementById('avatar-detail-voice').textContent = avatarVoiceLine(persona);

        // What it would speak in if you changed nothing, shown only when you have: two
        // identical lines, one labelled "default", is a pane telling you nothing twice.
        const defaults = document.getElementById('avatar-detail-voice-default');
        defaults.classList.toggle('hidden', !persona.voice_customised);
        if (persona.voice_customised) {
            const hasOwn = persona.default_voice || persona.default_local_voice;
            defaults.textContent = hasOwn
                ? `Its own voice: ${avatarVoiceLine({
                    voice: persona.default_voice,
                    local_voice: persona.default_local_voice,
                })}`
                : 'This one was written without a voice of its own: reset it and it speaks '
                    + 'in whatever Voice & Sound says.';
        }

        const reset = document.getElementById('btn-avatar-voice-reset');
        reset.classList.toggle('hidden', !persona.voice_customised);
        reset.onclick = () => {
            voiceEngine.playSFX('click');
            saveAvatarVoice(persona.key, null);
        };

        const note = document.getElementById('avatar-voice-note');
        const cloud = document.getElementById('avatar-voice-cloud');
        const local = document.getElementById('avatar-voice-local');
        note.textContent = '';

        fetchVoicePickerLists().then((lists) => {
            fillVoiceSelect(cloud, lists.cloud.map((v) => ({
                value: v.name,
                label: `${v.name} — ${v.label}`,
            })), persona.default_voice, persona.voice);
            fillVoiceSelect(local, lists.local.map((v) => ({
                value: v.name,
                // A voice that is not downloaded is still offered: it is a real choice that
                // needs fetching first, and hiding it would make this picker disagree with
                // the download list in Voice & Sound.
                label: `${v.name} — ${v.label}${v.installed ? '' : ' (not downloaded)'}`,
            })), persona.default_local_voice, persona.local_voice);
        }).catch((e) => {
            console.warn('Could not load the voice lists', e);
            note.textContent = 'The voice lists could not be loaded, so this avatar keeps '
                + 'the voice it was written with.';
        });

        cloud.onchange = () => saveAvatarVoice(persona.key, {
            voice: cloud.value,
            local_voice: local.value,
        });
        local.onchange = () => saveAvatarVoice(persona.key, {
            voice: cloud.value,
            local_voice: local.value,
        });
    }

    /* The first option is always "leave it alone", and it says what leaving it alone sounds
       like -- an avatar with no voice of its own in the identity table speaks in whatever
       Voice & Sound says, and that is worth reading rather than inferring from a blank. */
    function fillVoiceSelect(select, options, defaultValue, current) {
        select.innerHTML = '';
        const first = document.createElement('option');
        first.value = '';
        first.textContent = defaultValue
            ? `Its own: ${defaultValue}`
            : 'Whatever Voice & Sound says';
        select.appendChild(first);
        options.forEach((opt) => {
            const el = document.createElement('option');
            el.value = opt.value;
            el.textContent = opt.label;
            select.appendChild(el);
        });
        // `current` is the voice it actually speaks in; it only counts as a *choice* when it
        // is not simply the default, or the picker would show every avatar as customised.
        select.value = current && current !== defaultValue ? current : '';
        if (select.value !== '' && !options.some((o) => o.value === select.value)) {
            select.value = '';
        }
    }

    /* `choice` null resets this avatar; otherwise each half is a voice name or '' for "leave
       that half at its default". The reply is the whole persona list, so the pane redraws
       from what was stored rather than from what it hoped would be. */
    async function saveAvatarVoice(personaKey, choice) {
        const note = document.getElementById('avatar-voice-note');
        try {
            let personas;
            if (choice === null) {
                personas = IS_TAURI
                    ? await tauriInvoke('clear_persona_voice_rust', { persona: personaKey })
                    : await postJson('/api/personas/voice/reset', { persona: personaKey });
            } else {
                const body = {
                    persona: personaKey,
                    voice: choice.voice || null,
                    localVoice: choice.local_voice || null,
                };
                personas = IS_TAURI
                    ? await tauriInvoke('set_persona_voice_rust', body)
                    : await postJson('/api/personas/voice', {
                        persona: personaKey,
                        voice: choice.voice || null,
                        local_voice: choice.local_voice || null,
                    });
            }
            if (Array.isArray(personas)) {
                personaRows = new Map(personas.map((p) => [p.key, p]));
            }
            // Redrawn first, because redrawing clears the note -- and the note is the only
            // thing on screen that says the choice reached the database.
            const row = personaRow(personaKey);
            if (row) renderAvatarVoice(row);
            note.textContent = choice === null
                ? 'Back to the voice it was written with.'
                : 'Saved. It speaks in this from its next answer.';
        } catch (e) {
            console.warn('Could not save the avatar voice', e);
            note.textContent = `That voice could not be saved: ${e.message || e}`;
        }
    }

    async function postJson(path, body) {
        const resp = await apiFetch(path, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body),
        });
        if (!resp.ok) throw new Error(await resp.text() || `${resp.status}`);
        return resp.json();
    }

    // ---- The live preview ---------------------------------------------------
    function buildAvatarPreview(avatarId) {
        const viewport = document.getElementById('avatar-preview-viewport');
        if (!viewport || typeof HologramAvatar === 'undefined') return;
        disposeAvatarPreview();
        try {
            avatarPreviewEngine = new HologramAvatar('avatar-preview-viewport');
            avatarPreviewEngine.setAvatar(avatarId);
            avatarPreviewEngine.setColorPalette(Aether1Theme.paletteFor(Aether1Theme.current().colours));
            avatarPreviewEngine.setState('IDLE');
        } catch (e) {
            // A browser out of WebGL contexts is a bad preview, not a broken settings pane.
            console.warn('Could not build the avatar preview', e);
            avatarPreviewEngine = null;
        }
    }

    function disposeAvatarPreview() {
        if (!avatarPreviewEngine) return;
        try { avatarPreviewEngine.dispose(); } catch (e) { /* already gone */ }
        avatarPreviewEngine = null;
    }

    // ---- The card in Appearance that leads here -----------------------------
    function refreshAvatarEntryCard() {
        const cat = avatarCatalogue();
        const entry = cat && cat.get(currentAvatar);
        const face = document.getElementById('avatar-entry-face');
        const name = document.getElementById('avatar-entry-name');
        const meta = document.getElementById('avatar-entry-meta');
        if (!face || !name || !meta) return;
        if (!entry) {
            face.textContent = '✨';
            name.textContent = currentAvatar;
            meta.textContent = '';
            return;
        }
        const group = cat.group(entry.group);
        face.textContent = entry.emoji;
        name.textContent = entry.label;
        meta.textContent = `${group ? group.name : ''} · ${avatarRoleLine(entry)}`;
    }

    /* Called whenever the pick changes or an egg is unlocked. Re-rendering rather than
       toggling a class on one card: the current avatar shows up in three places here (the
       line's card, the grid card's badge, the detail view's button) and one of them going
       stale is exactly the kind of thing nobody notices until it is wrong. */
    function refreshAvatarBrowser() {
        if (!avatarBrowserBuilt) return;
        refreshAvatarEntryCard();
        renderAvatarGroups();
        if (avatarBrowserGroup) {
            const cat = avatarCatalogue();
            const group = cat && cat.group(avatarBrowserGroup);
            if (group) renderLinePick(group);
            const active = document.querySelector('[data-avatar-stage="members"].is-active')
                || document.querySelector('[data-avatar-stage="detail"].is-active');
            const grid = document.getElementById('avatar-member-grid');
            if (grid) {
                grid.innerHTML = '';
                visibleAvatarsIn(avatarBrowserGroup).forEach((e) => grid.appendChild(buildAvatarCard(e)));
            }
            if (active && active.dataset.avatarStage === 'detail' && avatarBrowserSelection) {
                // Re-opening would rebuild the preview engine mid-view; only the button
                // state and the badge need saying again.
                const isCurrent = avatarBrowserSelection === currentAvatar;
                const useBtn = document.getElementById('btn-avatar-use');
                if (useBtn) {
                    useBtn.textContent = isCurrent ? 'Already wearing it' : 'Wear this avatar';
                    useBtn.disabled = isCurrent;
                }
                const line = document.getElementById('avatar-detail-current');
                if (line) line.classList.toggle('hidden', !isCurrent);
            }
        }
    }

    /* Built once, the first time the section is opened. The catalogue is static and the
       persona rows are fetched once for the whole window, so there is nothing here worth
       redoing on every visit -- but there is a WebGL context worth not creating until
       somebody actually asks to look at an avatar. */
    function initAvatarBrowser() {
        if (avatarBrowserBuilt) return;
        if (!avatarCatalogue()) {
            console.warn('js/avatar-catalogue.js did not load; the avatar browser is empty');
            return;
        }
        avatarBrowserBuilt = true;
        document.querySelectorAll('[data-avatar-back]').forEach((btn) => {
            btn.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                const target = btn.dataset.avatarBack;
                if (target === 'members' && avatarBrowserGroup) openAvatarGroup(avatarBrowserGroup);
                else if (target === 'appearance') showSettingsSection('appearance');
                else showAvatarStage('groups');
            });
        });
        renderAvatarGroups();
        refreshAvatarEntryCard();
        showAvatarStage('groups');
    }

    /* Opening the section. It lands on the lines rather than wherever it was left: the
       stack behind you is only meaningful while you are in it, and coming back to a detail
       view of an avatar you were reading about yesterday is not where anyone means to start.
       The one exception is the avatar you are wearing -- see the entry card in Appearance,
       which opens straight onto it. */
    function openAvatarBrowser(avatarId) {
        initAvatarBrowser();
        if (!avatarBrowserBuilt) return;
        if (avatarId && avatarCatalogue().get(avatarId)) openAvatarDetail(avatarId);
        else showAvatarStage('groups');
    }

    const avatarEntryCard = document.getElementById('avatar-entry-card');
    if (avatarEntryCard) {
        avatarEntryCard.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            if (showSettingsSection('avatars')) openAvatarBrowser(currentAvatar);
        });
    }

    /* ---- Appearance: the pane, and the colour panel behind its theme card -------------
       The avatar got its own section because there are twenty-two of them with something to
       say about each. The theme is one decision with a lot of controls, so it stays in
       Appearance and drills down one level instead. */
    const APPEARANCE_STAGES = ['main', 'theme'];

    function showAppearanceStage(name) {
        APPEARANCE_STAGES.forEach((stage) => {
            document.querySelectorAll(`[data-appearance-stage="${stage}"]`).forEach((el) => {
                el.classList.toggle('is-active', stage === name);
            });
        });
        const detail = document.getElementById('settings-detail');
        if (detail) detail.scrollTop = 0;
    }

    const themeEntryCard = document.getElementById('theme-entry-card');
    if (themeEntryCard) {
        themeEntryCard.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            showAppearanceStage('theme');
        });
    }
    document.querySelectorAll('[data-appearance-back]').forEach((btn) => {
        btn.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            showAppearanceStage(btn.dataset.appearanceBack || 'main');
        });
    });

    /* The named palettes, built from the preset list rather than from markup.
       What a swatch *does* depends on the mode, which is the whole of the change here:
       in Cyberpunk it is the palette, ground included; in Daylight and Midnight it is the
       two accents, worn on the shell those modes were designed with. The two flat presets
       are not offered as swatches -- they are what those modes already are, which is what
       the mode buttons above and the reset button below already say. */
    function renderThemePalette(theme) {
        const grid = document.getElementById('theme-palette-grid');
        if (!grid) return;
        const accentOnly = Aether1Theme.groundIsFixed(theme.mode);
        const list = Aether1Theme.presets().filter((p) => p.mode === 'cyberpunk');
        if (accentOnly) {
            // The mode's own accents belong in the row too, or the palette it ships with is
            // the one thing you cannot pick.
            const own = Aether1Theme.preset(theme.mode);
            if (own) list.unshift(own);
        }
        grid.innerHTML = '';
        list.forEach((p) => {
            const btn = document.createElement('button');
            btn.type = 'button';
            btn.className = 'theme-palette-btn';
            btn.dataset.colorTheme = p.id;
            btn.title = accentOnly
                ? `${p.label} accents, on the ${Aether1Theme.MODE_LABELS[theme.mode]} shell`
                : `${p.label}: the whole palette, background included`;
            const bars = document.createElement('span');
            bars.className = 'theme-palette-bars';
            bars.setAttribute('aria-hidden', 'true');
            // In accent mode the ground bar is the mode's own, not the preset's, so the
            // swatch shows what pressing it would actually paint.
            const ground = accentOnly ? theme.colours.background : p.background;
            [ground, p.main, p.highlight].forEach((hex) => {
                const bar = document.createElement('span');
                bar.className = 'theme-palette-bar';
                bar.style.background = hex;
                bars.appendChild(bar);
            });
            /* A swatch lights up only while the colours still match it exactly -- nudge one
               picker and nothing is selected, which is the honest state: what is on screen is
               no longer any of these. Decided here rather than in a later pass, because the
               grid is rebuilt on every theme change and anything marked afterwards would be
               marked on nodes about to be replaced. */
            btn.classList.toggle('is-selected', p.id === theme.colours.preset);
            const label = document.createElement('span');
            label.className = 'theme-palette-name';
            label.textContent = p.label;
            btn.appendChild(bars);
            btn.appendChild(label);
            btn.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                paintTheme(accentOnly ? Aether1Theme.setAccents(p.id) : Aether1Theme.setPreset(p.id));
            });
            grid.appendChild(btn);
        });
    }

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

        /* Update the Neural Dialogue Stream title based on theme.
           In Light and Dark modes, show "chat or interaction window"; in Cyberpunk, show "Neural Dialogue Stream". */
        const chatPanelTitle = document.getElementById('chat-panel-title');
        if (chatPanelTitle) {
            chatPanelTitle.textContent = theme.mode === 'cyberpunk' ? 'NEURAL DIALOGUE STREAM' : 'CHAT OR INTERACTION WINDOW';
        }

        /* Game Mode is called Sleep Mode outside Cyberpunk, so the name follows the theme.
           Re-read from the button rather than kept here: the state is the machine's, and it
           can be changed from the tray, the hotkey or another window. */
        const gameModeBtn = document.getElementById('btn-game-mode');
        if (gameModeBtn) setGameModeButtonState(gameModeBtn.dataset.active === 'true');

        // Covers both the three buttons in Settings and the three in the top bar's slide-out.
        document.querySelectorAll('.theme-mode-btn').forEach(btn => {
            btn.classList.toggle('cyber-btn-active', btn.getAttribute('data-theme-mode') === theme.mode);
        });

        Object.keys(themeColourInputs).forEach(slot => {
            const input = themeColourInputs[slot];
            if (input && input.value.toLowerCase() !== theme.colours[slot]) input.value = theme.colours[slot];
        });

        /* How much of the palette this mode hands over. Cyberpunk is made of its colours and
           gives you all three; Daylight and Midnight are the light and the dark shell, so the
           ground is theirs and the accents are yours. The pickers are removed rather than
           disabled -- a greyed-out background swatch still showing a colour invites the
           question of why it will not move. */
        const slots = Aether1Theme.slotsFor(theme.mode);
        const accentOnly = Aether1Theme.groundIsFixed(theme.mode);
        ['background', 'main', 'highlight'].forEach(slot => {
            const row = document.getElementById('theme-colour-row-' + slot);
            if (row) row.classList.toggle('hidden', slots.indexOf(slot) === -1);
        });
        const colourGrid = document.getElementById('theme-colour-grid');
        if (colourGrid) {
            colourGrid.classList.toggle('grid-cols-3', slots.length > 2);
            colourGrid.classList.toggle('grid-cols-2', slots.length === 2);
        }
        // The two accents are called Main and Highlight while there is a background beside
        // them to be the other thing; on their own they are simply the accent and its
        // companion, which is what they are doing in those two modes.
        const mainLabel = document.getElementById('theme-colour-label-main');
        const highlightLabel = document.getElementById('theme-colour-label-highlight');
        if (mainLabel) mainLabel.textContent = accentOnly ? 'Accent' : 'Main';
        if (highlightLabel) highlightLabel.textContent = accentOnly ? 'Companion' : 'Highlight';

        const groundNote = document.getElementById('theme-ground-note');
        if (groundNote) {
            groundNote.textContent = accentOnly
                ? `${Aether1Theme.MODE_LABELS[theme.mode]} keeps the page it was designed with, so only the accents are yours here. Depth below still moves how dark that page sits.`
                : '';
        }

        const themeStageLede = document.getElementById('theme-stage-lede');
        if (themeStageLede) {
            themeStageLede.textContent = accentOnly
                ? 'The flat window shell, light or dark, with an accent of your choosing.'
                : 'Neon, scanlines and corner brackets, and every colour of it yours -- the ground included, because in this mode the ground is part of the look.';
        }

        renderThemePalette(theme);

        /* The card back in the pane. The three swatches are read out of the derived variables
           rather than off theme.colours, so they are the palette as painted -- tone and all --
           which is the only version worth previewing. */
        const themeEntryName = document.getElementById('theme-entry-name');
        const themeEntryMeta = document.getElementById('theme-entry-meta');
        if (themeEntryName) themeEntryName.textContent = Aether1Theme.MODE_LABELS[theme.mode] || theme.mode;
        if (themeEntryMeta) {
            const named = Aether1Theme.preset(theme.colours.preset);
            // "Daylight / Daylight accents" says one thing twice. A mode wearing its own
            // palette is simply untouched, which is worth saying instead.
            themeEntryMeta.textContent = !named
                ? 'Colours of your own'
                : named.id === theme.mode
                    ? 'As it was designed'
                    : (accentOnly ? `${named.label} accents` : `${named.label} palette`);
        }
        const painted = Aether1Theme.variablesFor(theme.mode, theme.colours);
        [['main', '--neon-cyan'], ['mid', '--neon-blue'], ['highlight', '--neon-purple']].forEach(([name, variable]) => {
            const swatch = document.getElementById('theme-entry-swatch-' + name);
            if (swatch) swatch.style.background = painted[variable];
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
    // "Float the avatar on your desktop" setting (see setHologramFloating below).
    let desktopSpriteEnabled = false;

    /* Every window other than this one that is currently drawing this avatar: the desktop
       sprite, the fullscreen face (js/face.js), or both at once. The two push functions
       below check it before bothering to emit, because state and audio are the only two
       mirror messages frequent enough for that to matter -- avatar and theme changes happen
       when a human clicks something and are sent unconditionally.

       A set rather than a counter so that a window announcing itself twice (a reload of the
       face, say) doesn't leave the HUD emitting to a listener that is no longer there. The
       sprite is added by setHologramFloating, which already knows; other windows say so
       themselves, over the two events below. */
    const avatarMirrors = new Set();

    function anyoneMirroringTheAvatar() {
        return desktopSpriteEnabled || avatarMirrors.size > 0;
    }

    /* Sets the hologram's state and, if anything is mirroring this avatar, pushes it there
       over a Tauri event -- the same "push, don't poll" pattern setHologramAvatar already
       uses for avatar changes. Every hologram.setState call in this file goes through here
       instead of calling the hologram directly, so a mirror always shows what the main
       window's avatar is actually doing (IDLE/LISTENING/THINKING/SPEAKING) rather than
       running its own idea of "busy" from a separate conversation. */
    let currentAvatarState = 'IDLE';
    function setAvatarState(newState) {
        currentAvatarState = newState;
        hologram.setState(newState);
        if (anyoneMirroringTheAvatar() && window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.emit('hologram-state-changed', { state: newState }).catch(() => {});
        }
    }

    // Audio data arrives every animation frame; emitting all of it across the Tauri IPC
    // boundary for windows that mostly aren't open would be wasted work, so this both gates
    // on something actually mirroring the avatar and thins the frames it does send -- a
    // reactive glow doesn't need 60fps to read as alive.
    let audioFrameCount = 0;
    function pushAudioToSprite(freqData) {
        if (!anyoneMirroringTheAvatar() || !window.__TAURI__ || !window.__TAURI__.event) return;
        audioFrameCount = (audioFrameCount + 1) % 3;
        if (audioFrameCount !== 0) return;
        window.__TAURI__.event.emit('hologram-audio-changed', { data: Array.from(freqData) }).catch(() => {});
    }

    /* A mirror window saying hello. It gets a snapshot of where the avatar is right now in
       reply, because everything else about this arrangement is a *change* notification: a
       face opened while the companion is mid-sentence would otherwise sit on IDLE in the
       wrong colours until the next thing happened to change.

       Deliberately not gated on IS_TAURI having a window open: a mirror that never announces
       itself simply never gets these, which is the failure mode we want. The matching
       'detached' comes from the mirror's own beforeunload. If a mirror window is killed
       hard enough that beforeunload never runs, the worst case is the HUD emitting state and
       audio events that nobody receives -- a few small messages a second into nothing, until
       the HUD itself is restarted. */
    if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
        window.__TAURI__.event.listen('avatar-mirror-attached', (event) => {
            const name = (event.payload && event.payload.window) || 'unknown';
            avatarMirrors.add(name);
            const emit = (channel, payload) =>
                window.__TAURI__.event.emit(channel, payload).catch(() => {});
            emit('avatar-changed', { avatar: currentAvatar });
            if (currentTheme) emit('color-theme-changed', currentTheme);
            emit('hologram-state-changed', { state: currentAvatarState });
        }).catch((e) => console.warn('Could not listen for avatar mirrors attaching', e));

        window.__TAURI__.event.listen('avatar-mirror-detached', (event) => {
            avatarMirrors.delete((event.payload && event.payload.window) || 'unknown');
        }).catch((e) => console.warn('Could not listen for avatar mirrors detaching', e));
    }

    // Voice Callbacks
    voiceEngine.onStateChange = (state) => {
        setAvatarState(state);
        setSpeakingUi(state === 'SPEAKING');
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
        // Step 19: almost always absent. It carries one line when a model the operator
        // chose has been uninstalled. The backend hands it over once per model per session
        // and clears it as it does, so this cannot repeat it -- uninstalling a model must
        // not silently change who you are talking to, and must not nag either.
        if (data.routing_notice) {
            appendSystemNotice(data.routing_notice);
            refreshSpecialityModel();
        }
    }

    /* Step 19: the model dropdown beside the avatar ---------------------------------
       Shows the model for the speciality that is active right now, not a table of all
       sixteen -- the avatar already says which speciality you are in, so the row reads as
       "this one, on that model". Changing persona re-reads it.

       The whole row hides when the model server has not answered. A dropdown offering
       nothing is worse than no dropdown: it looks broken, and it invites a click that
       cannot do anything. */
    const elAvatarModelRow = document.getElementById('avatar-model-row');
    const elAvatarModel = document.getElementById('avatar-model');

    async function refreshSpecialityModel() {
        if (!IS_TAURI || !elAvatarModel || !elAvatarModelRow) return;
        let data;
        try {
            data = await tauriInvoke('speciality_models_rust');
        } catch (e) {
            elAvatarModelRow.classList.add('hidden');
            return;
        }
        // null means nothing answered; [] means a server with nothing loaded. Neither can
        // offer a choice, and both are hidden rather than shown as an empty control.
        if (!Array.isArray(data.available) || data.available.length === 0) {
            elAvatarModelRow.classList.add('hidden');
            return;
        }

        const active = document.getElementById('setting-persona')?.value || 'default';
        const row = (data.specialities || []).find(s => s.key === active);
        if (!row) {
            elAvatarModelRow.classList.add('hidden');
            return;
        }

        elAvatarModel.innerHTML = '';
        // "Suggested" is the first option and the one selected when nothing was chosen, so
        // the default state of the control is the design's own recommendation rather than
        // a blank the operator has to interpret.
        const auto = document.createElement('option');
        auto.value = '';
        auto.textContent = row.suggested
            ? `${row.suggested}  (suggested)`
            : 'the general model';
        elAvatarModel.appendChild(auto);

        for (const entry of data.available) {
            const option = document.createElement('option');
            option.value = entry.model;
            option.textContent = entry.good_at
                ? `${entry.model} -- ${entry.good_at}`
                : entry.model;
            elAvatarModel.appendChild(option);
        }

        elAvatarModel.value = row.chosen || '';
        // A chosen model that is no longer installed is not in the list, so the select
        // would fall back to the first option and quietly look like a choice was never
        // made. Say it instead.
        if (row.chosen && elAvatarModel.value !== row.chosen) {
            const gone = document.createElement('option');
            gone.value = row.chosen;
            gone.textContent = `${row.chosen}  (not installed)`;
            elAvatarModel.appendChild(gone);
            elAvatarModel.value = row.chosen;
        }
        elAvatarModel.title = `${row.speciality}\nRunning on: ${row.running || 'the general model'}`;
        elAvatarModelRow.classList.remove('hidden');
    }

    function initSpecialityModel() {
        if (!elAvatarModel) return;
        elAvatarModel.addEventListener('change', async () => {
            const model = elAvatarModel.value;
            const persona = document.getElementById('setting-persona')?.value || 'default';
            try {
                // An empty value is "no pick", which is a real setting and not the same as
                // picking nothing -- it is what lets the suggestion apply again.
                await tauriInvoke('set_speciality_model_rust', {
                    persona,
                    model: model || null,
                });
            } catch (e) {
                console.error('Could not set the model for this speciality', e);
            }
            refreshSpecialityModel();
        });
        const personaSelect = document.getElementById('setting-persona');
        if (personaSelect) {
            personaSelect.addEventListener('change', refreshSpecialityModel);
        }
        refreshSpecialityModel();
    }

    /* The STATIC / FLOW toggle in the chin bar ---------------------------------------
       STATIC is the behaviour AETHER1 has always had: the avatar you picked answers
       everything. FLOW lets the question move to the specialist inside that avatar's own
       line, announced first by whoever is holding it. The rule that decides lives in
       src-tauri/src/llm/flow.rs; this is only the switch and the label.

       The button hides itself when the current avatar belongs to no line -- A1, Model's
       Own, your own design -- because there would be nothing to flow between, and a toggle
       that can be pressed but changes nothing is worse than no toggle. */
    async function refreshFlowMode() {
        const button = document.getElementById('flow-toggle');
        if (!button) return;
        let state = null;
        try {
            if (IS_TAURI) {
                state = await tauriInvoke('flow_mode_rust');
            } else {
                const resp = await apiFetch('/api/flow');
                if (resp.ok) state = await resp.json();
            }
        } catch (e) {
            console.error('Could not read the flow mode setting', e);
        }
        flowState = state;
        // Settings' Avatars pane draws the picked line in two places (the line's card and
        // the button on its members stage), so a change of mode has to reach it too.
        refreshAvatarBrowser();
        if (!state || !state.group) {
            button.classList.add('hidden');
            return;
        }
        button.classList.remove('hidden');
        // FLOW is the state worth seeing across the room: it means the avatar on screen is
        // not necessarily the one that answers next. So it is lit -- filled, bright, with a
        // dot -- while STATIC stays the quiet outline the rest of the chin bar wears. Two
        // labels in identical styling read as a caption rather than a switch that is on.
        button.classList.toggle('flow-on', !!state.enabled);
        button.textContent = state.enabled ? '\u25cf FLOW' : 'STATIC';
        const picked = state.line
            ? `${state.group} is picked whole, so the cast answers rather than one of them. `
            : '';
        button.title = state.enabled
            ? `${picked}The question can move to any node of ${state.group}, and whoever is holding it says so first. Click for STATIC.`
            : `${picked}This avatar answers everything. Click for FLOW, and the question moves to whichever node of ${state.group} it belongs to.`;
    }

    function initFlowMode() {
        const button = document.getElementById('flow-toggle');
        if (!button) return;
        button.addEventListener('click', async () => {
            const turningOn = !button.classList.contains('flow-on');
            try {
                if (IS_TAURI) {
                    await tauriInvoke('set_flow_mode_rust', { enabled: turningOn });
                } else {
                    await apiFetch('/api/flow', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ enabled: turningOn }),
                    });
                }
            } catch (e) {
                console.error('Could not change the flow mode setting', e);
            }
            refreshFlowMode();
        });
        // The line a hand-off may move within is the current avatar's, so the label has to
        // follow a change of persona as well as a change of mode.
        const personaSelect = document.getElementById('setting-persona');
        if (personaSelect) personaSelect.addEventListener('change', refreshFlowMode);
        refreshFlowMode();
    }

    /* Async because a browser socket now has to buy a ticket first. Both callers treat it
       as fire-and-forget -- startup and the reconnect timer -- so nothing awaits it; what
       matters is that the await inside happens before the socket is constructed. */
    async function connectTelemetry() {
        if (IS_TAURI) {
            if (!window.__TAURI__ || !window.__TAURI__.event) {
                console.error('Tauri event bridge unavailable; live telemetry will not update.');
                return;
            }
            window.__TAURI__.event.listen('telemetry-update', (event) => {
                handleTelemetryPayload(event.payload);
            });

            /* The panel is only fed while a window is on screen -- see the telemetry loop in
               src-tauri/src/main.rs, which parks when there is nobody to feed. A window event
               is usually what wakes it, but the webview knows it is back before the window
               manager tells anyone, and on some desktops it is the only one that knows. So
               say so, and the next reading is taken immediately instead of up to half a
               minute later. */
            const sayWeAreBack = () => {
                if (document.hidden) return;
                tauriInvoke('wake_telemetry_rust').catch(() => null);
            };
            document.addEventListener('visibilitychange', sayWeAreBack);
            window.addEventListener('focus', sayWeAreBack);
            return;
        }

        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws/telemetry`;
        /* Awaited, so the socket is opened with a ticket rather than with a Promise. The
           reconnect below calls this function again, which buys a fresh ticket each time --
           which is what single-use means. */
        const ws = new WebSocket(wsUrl, await apiWsProtocols());

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

    let gpuCellDrawn = false;

    /* One cell of the chin-bar strip: the number, the meter under it, and the colour both
       wear. Warn and crit go on the cell rather than on the meter, so the value and the bar
       always make the same claim -- a red bar under a cyan number reads as two readings. */
    function setChinStat(cell, valueEl, meterEl, text, percent, level, detail) {
        if (valueEl) valueEl.textContent = text;
        if (meterEl && typeof percent === 'number') {
            meterEl.style.width = `${Math.max(0, Math.min(100, percent))}%`;
        }
        if (cell) {
            cell.classList.toggle('warn', level === 'warn');
            cell.classList.toggle('crit', level === 'crit');
            // The detail the gauges used to spell out in full (6.5/16 GB, the card and its
            // memory) has to go somewhere once the strip shows only the percentage.
            if (detail) cell.title = detail;
        }
    }

    function loadLevel(pct) {
        if (pct > 85) return 'crit';
        if (pct > 65) return 'warn';
        return '';
    }

    function updateHardwareTelemetry(data) {
        if (!data) return;
        // The hub's chips report the machine from this same reading rather than probing
        // for it. Redrawn only once the hub has something to draw beside them.
        lastTelemetry = data;
        if (hub.loaded) renderHubChips();

        const cpuPct = data.cpu ? data.cpu.total_percent : 0;
        setChinStat(elCpuCell, elCpuVal, elCpuMeter, `${cpuPct}%`, cpuPct, loadLevel(cpuPct),
            `Processor load: ${cpuPct}%`);

        const ramPct = data.ram ? data.ram.percent : 0;
        setChinStat(elRamCell, elRamVal, elRamMeter, `${ramPct}%`, ramPct, loadLevel(ramPct),
            data.ram ? `System memory: ${ramPct}% — ${data.ram.used_gb}/${data.ram.total_gb} GB`
                     : 'System memory');

        const diskPct = data.disk ? data.disk.percent : 0;
        setChinStat(elDiskCell, elDiskVal, elDiskMeter, `${diskPct}%`, diskPct, loadLevel(diskPct),
            data.disk ? `Storage on /: ${diskPct}% — ${data.disk.used_gb}/${data.disk.total_gb} GB`
                      : 'Storage on /');

        const down = data.network ? data.network.download_kbps : 0;
        const up = data.network ? data.network.upload_kbps : 0;
        setChinStat(elNetCell, elNetVal, null, `↓${down} ↑${up} KB/s`, null, '',
            `Network: ${down} KB/s down, ${up} KB/s up`);

        // The card AETHER1 would run a model on, which is the number the whole hardware
        // monitor was missing. Written once and then left alone: the adapters are read at
        // startup and never re-probed, so re-setting this every tick would be work for a
        // string that cannot have changed. An empty list means nothing answered -- a
        // machine with no card, or a probe this platform does not have -- and the cell stays
        // hidden rather than printing a zero that reads as a fault.
        if (!gpuCellDrawn && Array.isArray(data.gpus) && data.gpus.length > 0) {
            gpuCellDrawn = true;
            const summary = data.gpus.map(gpu => gpu.summary).join(', ');
            // The strip has room for one card's worth of text. The whole list, however many
            // there are, stays on the cell's tooltip.
            setChinStat(elGpuCell, elGpuVal, null, data.gpus[0].summary, null, '',
                `Graphics: ${summary}`);
            if (elGpuCell) elGpuCell.classList.remove('hidden');
        }

        // data.battery is null on a desktop (see Telemetry::to_wire_json) -- the cell stays
        // hidden for the life of the app in that case rather than showing a permanent,
        // meaningless 0%. On a laptop it appears the first reading in and stays shown, since
        // a battery doesn't unplug itself from the machine mid-session.
        if (data.battery) {
            if (elBatteryCell) elBatteryCell.classList.remove('hidden');
            const pct = Math.round(data.battery.percent);
            const level = data.battery.on_battery && pct < 25 ? 'crit'
                        : (data.battery.on_battery ? 'warn' : '');
            setChinStat(elBatteryCell, elBatteryVal, elBatteryMeter, `${pct}%`, pct, level,
                `Battery: ${pct}% (${data.battery.state})`);
            // on_battery (state === Discharging) rather than state !== 'charging': "full"
            // and "unknown" are both still plugged in, and a warning that never clears on
            // a battery that reports "unknown" while on AC would just be noise.
            if (elBatteryCell) elBatteryCell.classList.toggle('on-battery', data.battery.on_battery);
        }
    }

    /* Both halves of the performance tab are on screen together, so there is no view to
       pick between and nothing to cycle. Until PR #165 this was a four-column panel that
       could show only one at a time and alternated on a timer, with pinning while a reply
       was generating and a hold after a click so the reader was not pulled off what they had
       just chosen. All of that machinery existed to work around the width; the tab has the
       room, so it is gone. */
    let lastTokens = null;

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

        /* Which model is answering, and where it is answering from, said in the tab's own
           header. The panel never named it: it stood in a column beside a scoreboard whose
           current row was highlighted, and that was enough to work it out. A tab is opened
           on its own and has to say what it is describing. `mode` rather than the provider
           alone, because "on this machine" and "through openai" are the distinction every
           number below turns on. */
        const elPerfModel = document.getElementById('perf-model');
        if (elPerfModel) {
            const where = tokens.mode === 'local'
                ? `on this machine via ${tokens.provider || 'a local server'}`
                : `through ${tokens.provider || 'the cloud'}`;
            elPerfModel.textContent = tokens.mode === 'offline'
                ? 'No model connected. Connect one in Settings.'
                : `${tokens.model || 'unnamed model'} \u00b7 ${where}`;
            elPerfModel.title = tokens.requests
                ? `${tokens.requests.toLocaleString()} ${tokens.requests === 1 ? 'reply' : 'replies'} this session.`
                : 'Nothing has been asked of it this session.';
        }

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

    /* A line from AETHER1 the program, not from the persona.
       Deliberately not appendMessage: that one puts the agent's own name and avatar on
       what it renders, and attributing "your model was uninstalled" to the character is
       how an operator learns to distrust what the character says. This is plainly the
       machinery talking. */
    function appendSystemNotice(text) {
        if (!chatContainer) return;
        const row = document.createElement('div');
        row.className = 'my-2 px-3 py-2 text-xs font-mono leading-relaxed msg-system self-stretch';
        const label = document.createElement('span');
        label.className = 'opacity-70';
        label.textContent = 'AETHER1 — ';
        row.appendChild(label);
        row.appendChild(document.createTextNode(text));
        chatContainer.appendChild(row);
        chatContainer.scrollTop = chatContainer.scrollHeight;
    }

    /* The one line of a flow-mode hand-off. Deliberately not appendMessage: that stamps
       the *current* agent's name on whatever it draws, and this is the previous one
       speaking. Deliberately not appendSystemNotice either -- a hand-off is a character
       talking, not the program, and rendering it as machinery would throw away the whole
       reason for announcing it. It is inserted above `before`, the reply that is already on
       screen waiting to be filled. */
    function appendHandoverLine(fromName, line, before) {
        if (!chatContainer) return;
        const row = document.createElement('div');
        row.className = 'p-3 rounded my-2 text-sm leading-relaxed msg-agent self-start mr-8 opacity-70';
        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 border-b border-cyan-500/20 text-xs font-mono text-cyan-400/80';
        const who = document.createElement('span');
        who.innerHTML = `🌐 <strong>${(fromName || '').toUpperCase()}</strong>`;
        header.appendChild(who);
        const when = document.createElement('span');
        when.textContent = new Date().toLocaleTimeString();
        header.appendChild(when);
        const body = document.createElement('div');
        body.textContent = line;
        row.appendChild(header);
        row.appendChild(body);
        if (before && before.parentNode === chatContainer) chatContainer.insertBefore(row, before);
        else chatContainer.appendChild(row);
        chatContainer.scrollTop = chatContainer.scrollHeight;
    }

    /* Which avatar wears a persona. The pairing already lives in AVATAR_PRESETS, so it is
       read back from there rather than written down a second time -- a second copy is how
       the two drift apart. */
    function avatarIdForPersona(personaKey) {
        return Object.keys(AVATAR_PRESETS).find((id) => AVATAR_PRESETS[id].persona === personaKey) || null;
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
            // Held so trimChatHistory can free the decoded clip when this message
            // eventually scrolls out of the kept window. On the native transport this is
            // a blob of the whole sentence's audio, and the Replay button is the only
            // reason it is still alive.
            rememberClip(msgDiv, audioUrl);
        }

        chatContainer.appendChild(msgDiv);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        msgDiv.bodyDiv = bodyDiv;
        trimChatHistory();
        return msgDiv;
    }

    /* How many messages stay in the page. Everything older is dropped from the DOM.

       The transcript itself is not lost -- it is on disk, and the History tab reads it
       from there -- so this is only about what the renderer is asked to hold. What it was
       asked to hold before was everything: a session that ran all day accumulated every
       message node, every formatted-markdown subtree, and, for every reply that was
       spoken, the decoded audio of that reply behind its Replay button. None of it is
       reachable by scrolling in any way a person actually does, and all of it is renderer
       memory that only ever goes up. WebKitWebProcess is the process that pays, and when
       it runs out it does not degrade -- it dies, and takes the window with it.

       200 is far past anything anyone scrolls back through by hand and still bounds the
       page to something flat. */
    const CHAT_HISTORY_LIMIT = 200;

    /** Notes a clip URL on its message so trimming can revoke it. */
    function rememberClip(msgDiv, url) {
        if (typeof url !== 'string' || !url.startsWith('blob:')) return;
        (msgDiv.clipUrls || (msgDiv.clipUrls = [])).push(url);
    }

    /* Drops the oldest messages once the log is over the limit, freeing any audio they
       were keeping alive on the way out. Cheap to call on every append: over the limit it
       removes one node, and under it does nothing at all. */
    function trimChatHistory() {
        if (!chatContainer) return;
        while (chatContainer.children.length > CHAT_HISTORY_LIMIT) {
            const oldest = chatContainer.firstElementChild;
            if (!oldest) break;
            (oldest.clipUrls || []).forEach((url) => {
                try {
                    URL.revokeObjectURL(url);
                } catch (e) {
                    // Already revoked, or a page with no URL support -- nothing to recover.
                }
            });
            oldest.remove();
        }
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
                    // Synthesized on demand, and then kept for as long as the message is
                    // on screen -- so it is the message's to free, like a clip that came
                    // with the reply.
                    rememberClip(msgDiv, cachedUrl);
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
        cancelHandsFreeListen(); // a held key takes the microphone over; see listenHandsFree
        talkHeld = true;
        supersedeSpeech(); // talking over the companion interrupts it
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
        await transcribeAndSend(wav);
    }

    /* The half of a recording that has nothing to do with how it was started: transcribe it
       locally and send what came back as a message. Shared by push-to-talk above and by the
       hands-free listen below, so the two cannot drift apart. */
    async function transcribeAndSend(wav) {
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
            if (text && text.trim()) {
                window.AETHER_VOICE_REQUESTED = true;
                try { await handleSendMessage(text.trim()); }
                finally { window.AETHER_VOICE_REQUESTED = false; }
            }
        } catch (e) {
            appendMessage(currentAgentName, `⚠️ Could not make that out: ${e.message || e}`);
        }
    }

    // --- Hands-free listening -------------------------------------------------------
    // One recording, opened by the app rather than by a held key: the avatar has just asked
    // what you want, so something other than your finger has to decide when your answer
    // ended. The level of the incoming audio does it -- wait for talking to start, then for
    // it to stay quiet. Still one recording and still a deliberate act on your part (you
    // clicked the avatar); this is not an always-on microphone, for the reasons in the push
    // to talk note above.
    const HANDS_FREE_SPEECH_LEVEL = 0.02;   // RMS above this counts as somebody talking
    const HANDS_FREE_WAIT_FOR_SPEECH_MS = 6000;  // nothing said at all -> close it again
    const HANDS_FREE_TRAILING_SILENCE_MS = 1200; // quiet this long after speech -> that was it
    const HANDS_FREE_MAX_MS = 20000;        // a hard ceiling, whatever the level is doing
    const HANDS_FREE_OPEN_TIMEOUT_MS = 6000;

    let handsFreeListening = false;

    /* Push to talk wins: a key held while the hands-free mic is open takes the microphone
       over rather than fighting it for the one capture the engine supports. */
    function cancelHandsFreeListen() {
        if (!handsFreeListening) return;
        handsFreeListening = false;
        voiceEngine.stopCapture();
    }

    async function listenHandsFree() {
        if (handsFreeListening || talkHeld || isWaitingForResponse) return;
        handsFreeListening = true;

        let level = 0;
        // In the native window WebKitGTK can simply never answer the permission request (the
        // microphone bug -- see the Sound hub note), and an await that never returns would
        // leave this locked on for the rest of the session. Bounded, and the stream released
        // if the answer turns up afterwards.
        const opening = voiceEngine.startCapture({ onLevel: (rms) => { level = rms; } });
        const started = await Promise.race([
            opening,
            new Promise((resolve) => setTimeout(() => resolve('timeout'), HANDS_FREE_OPEN_TIMEOUT_MS)),
        ]);
        if (started !== true) {
            handsFreeListening = false;
            if (started === 'timeout') opening.then(() => voiceEngine.stopCapture()).catch(() => {});
            appendMessage(currentAgentName, '⚠️ No microphone available.');
            return;
        }

        setAvatarState('LISTENING');
        const openedAt = Date.now();
        let heardSpeech = false;
        let lastLoudAt = 0;
        await new Promise((resolve) => {
            const timer = setInterval(() => {
                if (!handsFreeListening) { clearInterval(timer); resolve(); return; }
                const now = Date.now();
                if (level >= HANDS_FREE_SPEECH_LEVEL) { heardSpeech = true; lastLoudAt = now; }
                const done = now - openedAt > HANDS_FREE_MAX_MS
                    || (!heardSpeech && now - openedAt > HANDS_FREE_WAIT_FOR_SPEECH_MS)
                    || (heardSpeech && now - lastLoudAt > HANDS_FREE_TRAILING_SILENCE_MS);
                if (done) { clearInterval(timer); resolve(); }
            }, 100);
        });

        if (!handsFreeListening) return;  // taken over by push to talk, which owns the mic now
        handsFreeListening = false;
        const wav = voiceEngine.stopCapture();
        setAvatarState('IDLE');
        // Silence is an answer too: nothing was said, so nothing is sent and nothing is said
        // about it either.
        if (wav && heardSpeech) await transcribeAndSend(wav);
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
            // switch is shown for what it is rather than left looking like a live control.
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
    async function streamChat(text, sessionId, onDelta, onHandover, media = []) {
        if (IS_TAURI) {
            const streamId = `s${Date.now()}${Math.random().toString(16).slice(2)}`;
            const unlisten = await window.__TAURI__.event.listen('chat-delta', (event) => {
                if (event.payload && event.payload.stream_id === streamId) onDelta(event.payload.delta);
            });
            /* Flow mode. Its own event rather than a first delta, because it is a different
               speaker -- the node leaving says it, and every delta after belongs to the one
               arriving. It always lands before the first delta, so the reply can be
               relabelled while it is still empty. */
            const unlistenHandover = await window.__TAURI__.event.listen('flow-handover', (event) => {
                if (event.payload && event.payload.stream_id === streamId) onHandover(event.payload);
            });
            try {
                return await tauriInvoke('generate_response_streaming_rust', {
                    prompt: text, sessionId, streamId, media
                });
            } finally {
                unlisten();
                unlistenHandover();
            }
        }

        // Browser fallback: the same conversation over a WebSocket, since the socket
        // plumbing already exists here for telemetry (see /ws/chat in server.rs).
        const chatProtocols = await apiWsProtocols();
        return await new Promise((resolve, reject) => {
            const wsBase = (API_BASE || window.location.origin).replace(/^http/, 'ws');
            const socket = new WebSocket(`${wsBase}/ws/chat`, chatProtocols);
            socket.onopen = () => socket.send(JSON.stringify({
                message: text, session_id: sessionId, generate_voice: false, media
            }));
            socket.onmessage = (event) => {
                let data;
                try { data = JSON.parse(event.data); } catch (e) { return; }
                if (data.type === 'delta') onDelta(data.delta);
                else if (data.type === 'handover') onHandover(data);
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

    function renderChatImagePreviews() {
        if (!chatMediaPreviews) return;
        chatMediaPreviews.replaceChildren();
        chatMediaPreviews.classList.toggle('hidden', pendingChatImages.length === 0);
        pendingChatImages.forEach((image, index) => {
            const wrap = document.createElement('div');
            wrap.className = 'relative border border-cyan-500/40 rounded overflow-hidden';
            const img = document.createElement('img');
            img.src = image.dataUrl;
            img.alt = image.name || 'Image attachment preview';
            img.className = 'w-16 h-16 object-cover';
            const remove = document.createElement('button');
            remove.type = 'button'; remove.textContent = '×';
            remove.className = 'absolute top-0 right-0 bg-slate-950/90 text-cyan-200 px-1';
            remove.setAttribute('aria-label', `Remove ${img.alt}`);
            remove.addEventListener('click', () => { pendingChatImages.splice(index, 1); renderChatImagePreviews(); });
            wrap.append(img, remove); chatMediaPreviews.appendChild(wrap);
        });
    }

    async function addChatImageFiles(files) {
        const accepted = ['image/png', 'image/jpeg', 'image/webp', 'image/gif'];
        for (const file of Array.from(files || [])) {
            if (!accepted.includes(file.type)) { alert('Attach PNG, JPEG, WebP, or GIF images.'); continue; }
            if (file.size > 8 * 1024 * 1024) { alert(`${file.name} is larger than 8 MB.`); continue; }
            if (pendingChatImages.length >= 4) { alert('Attach up to four images per message.'); break; }
            if (pendingChatImages.reduce((sum, item) => sum + item.size, file.size) > 16 * 1024 * 1024) {
                alert('Images must total 16 MB or less.'); continue;
            }
            const dataUrl = await new Promise((resolve, reject) => {
                const reader = new FileReader(); reader.onload = () => resolve(reader.result); reader.onerror = reject; reader.readAsDataURL(file);
            }).catch(() => null);
            if (typeof dataUrl === 'string') pendingChatImages.push({ name: file.name, size: file.size, mime_type: file.type, dataUrl });
        }
        renderChatImagePreviews();
    }

    async function handleSendMessage(customPrompt = null) {
        const media = pendingChatImages.map(({ mime_type, dataUrl }) => ({ mime_type, data_base64: dataUrl.split(',', 2)[1] || '' }));
        const text = (customPrompt || chatInput.value.trim() || (media.length ? 'Please analyze the attached image(s).' : '')).trim();
        if ((!text && !media.length) || isWaitingForResponse) return;

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
        const userMessage = appendMessage('user', `${text}${media.length ? `\n\n[${media.length} image${media.length === 1 ? '' : 's'} attached]` : ''}`);
        media.forEach((image, index) => {
            const preview = document.createElement('img');
            preview.src = pendingChatImages[index].dataUrl;
            preview.alt = `Attached image ${index + 1}`;
            preview.className = 'mt-2 mr-2 inline-block max-w-48 max-h-48 rounded border border-cyan-500/30 object-contain';
            userMessage.bodyDiv.appendChild(preview);
        });
        pendingChatImages = [];
        renderChatImagePreviews();
        voiceEngine.playSFX('click');
        // A new question supersedes anything still being spoken -- and anything still being
        // synthesized. Dropping the queue alone left the previous reply's remaining
        // sentences in flight at Piper, and each one queued itself on arrival, in among the
        // sentences of the reply being spoken now.
        supersedeSpeech();

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

        const synthesizeChunk = async (chunk) => {
            // Trace lines are the machine narrating its own plumbing -- `\u2699 vault notes
            // loaded: INDEX.md, profile.md, machine.md`. They belong on screen and never in
            // the ear: the Rust sanitizer keeps the *contents* of inline code (so speech
            // says "nominal" rather than skipping it), which means a trace line arrives at
            // Piper as a list of filenames to read out. Stripped here, where what is a
            // trace and what is the answer is already known.
            chunk = withoutTraceLines(chunk);
            if (!autoSpeak || !chunk.trim()) return;
            const turn = speechTurn;
            try {
                const url = await synthesizeSpeechUrl(chunk);
                // Checked after the await, not before: the operator can ask the next
                // question while this sentence is still at the synthesizer, and this is
                // where that sentence finds out it is no longer wanted.
                if (url && turn === speechTurn) {
                    voiceEngine.enqueueTTS(url);
                    audioQueued = true;
                }
            } catch (e) {
                console.warn('sentence TTS failed', e);
            }
        };

        // Sentences are synthesized one after another, not all at once. The queue plays
        // strictly in the order things were pushed onto it, so whichever synthesis finishes
        // first is the sentence that gets spoken first -- and a four-word sentence comes
        // back from Piper well before the long one in front of it. Two sentences was enough
        // to hear the reply out of order; a paragraph of short ones made it unintelligible.
        // Chaining costs nothing in practice: playback of sentence one covers the synthesis
        // of sentence two.
        let speechChain = Promise.resolve();
        const speakChunk = (chunk) => {
            speechChain = speechChain.then(() => synthesizeChunk(chunk));
            return speechChain;
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

        /* Flow mode handed this question to somebody else. Three things happen, in this
           order: the line goes up attributed to the node leaving (above the reply, which
           is already on screen and still empty), the HUD becomes the node arriving, and
           the empty reply is relabelled so the answer is not signed by the wrong one.
           Rust has already persisted the switch, so nothing here saves anything -- this is
           the display catching up with a decision that has been made. */
        const onHandover = (handover) => {
            if (!handover || !handover.line) return;
            appendHandoverLine(handover.from, handover.line, replyDiv);
            const avatarId = avatarIdForPersona(handover.to_key);
            if (avatarId) applyAvatar(avatarId, false);
            updateAgentNameDisplay(handover.to);
            const personaField = document.getElementById('setting-persona');
            if (personaField) personaField.value = handover.to_key;
            refreshFlowMode();
            const senderSpan = replyDiv.querySelector('span');
            if (senderSpan) senderSpan.innerHTML = `🌐 <strong>${handover.to.toUpperCase()}</strong>`;
        };

        try {
            const data = await streamChat(text, currentSessionId, onDelta, onHandover, media);
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

            /* Speak whatever never reached a sentence boundary. There is almost always
               something: takeSpeakableChunk needs MIN_SPEAKABLE characters before it will cut,
               so a reply's last sentence is usually still sitting in `pending` -- which makes
               this the line that finishes every answer, not an edge case.

               Two sources for it, and the fallback is the point. `reply` is authoritative and
               repairs a delta that never arrived, but slicing it at `spoken.length` is only
               right while it really does start with what was said; when it does not, that
               slice begins mid-word somewhere in the middle of the answer. `pending` cannot
               be wrong -- it is, by construction, exactly the text no chunk has taken -- but
               it only knows about deltas that arrived. So: the authoritative tail when the
               offset lines up, and the text we know was never spoken when it does not.
               Dropping it was tried and it truncates the reply, which is worse than both. */
            const tail = reply.startsWith(spoken) ? reply.slice(spoken.length) : pending;
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

    /* ====================== WRITING CODE ================================
     * The third of the wizards, and the one with the narrowest reason to exist: the week
     * somebody cannot pay for a cloud coding assistant, their machine can still do the
     * work, and the only thing in the way is knowing which model fits in their memory and
     * what to type to point an editor at it.
     *
     * It lives inside The Brain rather than beside it because it is the same server and
     * the same download path -- a second model on the machine that is already answering,
     * chosen for a different job. Everything below is drawn from one probe of the machine,
     * re-asked after every action, for the reason setup.rs gives: a wizard that remembers
     * which page it is on can claim a step that is not true.
     *
     * Aether1 does not become the editor. It chooses, fetches, and hands over a command
     * with this machine's real address and real model name already in it.
     */

    const codeHeadline = document.getElementById('code-headline');
    const codeSteps = document.getElementById('code-steps');
    const codeOwnModel = document.getElementById('code-own-model');
    const codeAgents = document.getElementById('code-agents');
    const codeConventionsWrap = document.getElementById('code-conventions-wrap');
    const codeConventions = document.getElementById('code-conventions');
    const codeStatus = document.getElementById('code-status');
    const btnCodeRecheck = document.getElementById('btn-code-recheck');

    let codeAdvice = null;

    function setCodeStatus(text, tone = 'info') {
        if (!codeStatus) return;
        codeStatus.classList.remove('hidden', 'text-cyan-300', 'text-green-400', 'text-red-400', 'text-slate-300', 'animate-pulse');
        if (!text) { codeStatus.classList.add('hidden'); return; }
        const tones = { info: 'text-slate-300', busy: 'text-cyan-300', good: 'text-green-400', bad: 'text-red-400' };
        codeStatus.classList.add(tones[tone] || tones.info);
        if (tone === 'busy') codeStatus.classList.add('animate-pulse');
        codeStatus.textContent = text;
    }

    async function fetchCodeAdvice() {
        if (IS_TAURI) return tauriInvoke('code_advice_rust');
        const resp = await apiFetch('/api/code/advice');
        if (!resp.ok) throw new Error(`coding check failed: ${resp.status}`);
        return resp.json();
    }

    async function fetchCodeConventions() {
        if (IS_TAURI) return tauriInvoke('code_conventions_rust');
        const resp = await apiFetch('/api/code/conventions');
        if (!resp.ok) throw new Error(`could not read the house rules: ${resp.status}`);
        return resp.text();
    }

    /* The coding catalogue no longer draws a list of its own here. It is in the model
       hub at the top of The Brain, beside the models to talk to, because they are the
       same decision about the same machine -- and because only one list can say "this
       one is big enough to do both", which is the answer that saves a second download. */

    /* One coding program, with the half of its instructions that applies: how to get it
       when it is missing, how to point it at this machine when it is here. Showing both at
       once is how somebody ends up pasting an install command over a working install. */
    function codeAgentCard(agent) {
        const card = document.createElement('div');
        card.className = 'border rounded p-3 space-y-2 ' +
            (agent.installed ? 'border-green-500/40 bg-green-950/10' : 'border-slate-600/40 bg-slate-900/40');

        const head = document.createElement('div');
        head.className = 'flex items-center justify-between gap-2';

        const name = document.createElement('span');
        name.className = 'text-xs font-mono text-cyan-200';
        name.textContent = agent.label + (agent.recommended && !agent.installed ? ' — suggested' : '');
        head.appendChild(name);

        const state = document.createElement('span');
        state.className = 'text-[11px] font-mono whitespace-nowrap ' + (agent.installed ? 'text-green-400' : 'text-slate-500');
        state.textContent = agent.installed ? '✔ installed' : 'not installed';
        head.appendChild(state);
        card.appendChild(head);

        const blurb = document.createElement('div');
        blurb.className = 'text-[11px] font-mono text-slate-400 leading-snug';
        blurb.textContent = agent.blurb;
        card.appendChild(blurb);

        const steps = document.createElement('ol');
        steps.className = 'space-y-2';
        const which = agent.installed ? (agent.connect || []) : (agent.install || []);
        which.forEach((step, index) => steps.appendChild(renderSetupStep(step, index)));
        card.appendChild(steps);

        return card;
    }

    function renderCodeAdvice(advice) {
        codeAdvice = advice;
        if (codeHeadline) {
            codeHeadline.textContent = advice.headline;
            codeHeadline.classList.toggle('text-green-400', !advice.needs_attention);
            codeHeadline.classList.toggle('text-slate-400', advice.needs_attention);
        }

        if (codeSteps) {
            codeSteps.innerHTML = '';
            (advice.steps || []).forEach((step, index) => codeSteps.appendChild(renderSetupStep(step, index)));
        }


        // What Aether1 is running on now, when that is a model on this machine. Shown only
        // when it is a fact the reader can act on: the panel is about code, and a line
        // about the brain earns its place by saying the brain cannot do the other half.
        if (codeOwnModel) {
            const current = advice.aether1_model;
            codeOwnModel.classList.toggle('hidden', !current);
            if (current) {
                codeOwnModel.textContent = advice.aether1_model_too_small
                    ? `Aether1 itself is running on ${current}, which is too small to troubleshoot or to drive an agent offline.`
                    : `Aether1 itself is running on ${current}.`;
                codeOwnModel.classList.toggle('text-amber-300', !!advice.aether1_model_too_small);
                codeOwnModel.classList.toggle('text-slate-400', !advice.aether1_model_too_small);
            }
        }

        if (codeAgents) {
            codeAgents.innerHTML = '';
            // Nothing to drive until there is a model to drive it with, and a command
            // naming a model that is not downloaded is a command that fails.
            if (advice.stage !== 'no-server') {
                for (const agent of advice.agents || []) codeAgents.appendChild(codeAgentCard(agent));
            }
        }

        codeConventionsWrap?.classList.toggle('hidden', advice.stage === 'no-server');
    }

    async function refreshCodeAdvice({ quiet = false } = {}) {
        if (!quiet) setCodeStatus('Looking at this computer...', 'busy');
        try {
            const advice = await fetchCodeAdvice();
            renderCodeAdvice(advice);
            if (!quiet) setCodeStatus('');
            return advice;
        } catch (e) {
            setCodeStatus(`⚠ ${e.message || e}`, 'bad');
            return null;
        }
    }

    btnCodeRecheck?.addEventListener('click', () => { voiceEngine.playSFX('click'); refreshCodeAdvice(); });

    // Probed when the group is opened rather than when Settings is, because the probe
    // touches the network and most visits to Settings are not about this.
    document.getElementById('settings-group-coding')?.addEventListener('toggle', async (event) => {
        if (!event.target.open || codeAdvice) return;
        await refreshCodeAdvice();
        if (codeConventions && !codeConventions.textContent) {
            codeConventions.textContent = await fetchCodeConventions().catch(() => '');
        }
    });

    document.getElementById('btn-code-copy-conventions')?.addEventListener('click', async (event) => {
        const button = event.currentTarget;
        try {
            await navigator.clipboard.writeText(codeConventions?.textContent || '');
            button.textContent = '✔ Copied';
            setTimeout(() => { button.textContent = '📋 Copy'; }, 1500);
        } catch {
            button.textContent = 'Select it and copy';
        }
    });

    /* ====================================================================== */
    /* AETHER CODE -- the second conversation in the chat panel.               */
    /*                                                                        */
    /* Same act as the companion's chat, different model behind it: whichever  */
    /* coding model is downloaded, with the repository's house rules already   */
    /* in front of it (src-tauri/src/code_chat.rs). Kept as a tab rather than  */
    /* a panel because it is a conversation, and the conversation panel is     */
    /* where the operator already is.                                          */
    /*                                                                        */
    /* The one thing here that is not ordinary chat is the row of commands     */
    /* under a reply. Each button TYPES its command into the operator's        */
    /* terminal and stops -- no newline, so nothing runs until they press      */
    /* Return. That is the whole consent model, and it is why the buttons do   */
    /* not exist in a browser: there is no terminal there to type into.        */
    /* ====================================================================== */

    /* Set by the terminal block below, which owns the shell. null until then,
       and null forever in a browser. */
    let terminalBridge = null;

    const codeChatMessages = document.getElementById('code-chat-messages');
    const codeChatInput = document.getElementById('code-chat-input');
    const codeChatModel = document.getElementById('code-chat-model');
    let codeChatBusy = false;
    let codeChatLoaded = false;

    /* Set by the terminal block below. A tab's view has no layout box while it is hidden,
       so xterm measures zero there and the shell would keep that size once it is shown --
       the same reason a switched-off panel gets `aether1:panels-changed`. */
    let onTerminalTabShown = null;

    function switchChatTab(which) {
        document.querySelectorAll('[data-chat-view]').forEach(view => {
            const mine = view.getAttribute('data-chat-view') === which;
            view.classList.toggle('hidden', !mine);
            view.classList.toggle('flex', mine);
        });
        document.querySelectorAll('.chat-tab').forEach(tab => {
            const mine = tab.getAttribute('data-chat-tab') === which;
            tab.setAttribute('aria-selected', mine ? 'true' : 'false');
            tab.classList.toggle('text-cyan-300', mine);
            tab.classList.toggle('border-cyan-400', mine);
            tab.classList.toggle('text-slate-400', !mine);
            tab.classList.toggle('border-transparent', !mine);
        });
        if (which === 'terminal') {
            if (typeof onTerminalTabShown === 'function') onTerminalTabShown();
        }
        if (which === 'code') {
            /* Nothing is probed until the tab is opened: a scan of the machine's ports
               on every launch, for a panel nobody looked at, is work the operator did
               not ask for. */
            if (!codeChatLoaded) { codeChatLoaded = true; loadCodeChat(); }
            codeChatInput?.focus();
        }
        /* The four reading tabs are re-read on every visit rather than cached. Each is a
           list of something that changes while you are looking away -- a note written by
           the last answer, a conversation started in another window, a tool call made a
           second ago -- and a stale list is worse here than a short wait. Nothing is read
           until a tab is actually opened. */
        if (which === 'memories') loadMemories();
        if (which === 'history') openSessions();
        if (which === 'notes') openNotesReader();
        if (which === 'activity') loadActivityLog();
        /* Left the notes tab: the graph is drawing frames nobody is looking at. It keeps
           its layout, so coming back shows the same arrangement rather than a new one. */
        if (which !== 'notes') stopNoteGraph();
    }

    document.querySelectorAll('.chat-tab').forEach(tab => {
        tab.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            switchChatTab(tab.getAttribute('data-chat-tab'));
        });
    });

    /* Which model is answering, said above the conversation rather than discovered by
       asking it. Reuses the coding panel's own advice so the tab and the setup panel can
       never disagree about what is installed. */
    async function refreshCodeChatModel() {
        if (!codeChatModel) return;
        try {
            const advice = IS_TAURI
                ? await tauriInvoke('code_advice_rust')
                : await (await apiFetch('/api/code/advice')).json();
            if (!advice || !advice.model_installed) {
                codeChatModel.textContent = advice && advice.model
                    ? `no coding model downloaded yet -- ${advice.model} is the one for this machine`
                    : 'no coding model downloaded yet';
                codeChatModel.className = 'text-amber-400/80 truncate';
                return;
            }
            codeChatModel.textContent = `answering: ${advice.model}`;
            codeChatModel.className = 'text-slate-400 truncate';
        } catch (e) {
            codeChatModel.textContent = 'could not reach the model server';
            codeChatModel.className = 'text-amber-400/80 truncate';
        }
    }

    /* One command, with the button that puts it in the terminal.
       In a browser the button is replaced by the command alone: there is no terminal
       behind it, and a button that cannot work is worse than no button. */
    function codeCommandRow(command) {
        const wrapper = document.createElement('div');
        wrapper.className = 'mt-1';
        const row = document.createElement('div');
        row.className = 'flex items-center gap-2';

        const text = document.createElement('code');
        text.className = 'flex-1 min-w-0 truncate font-mono text-[11px] text-cyan-200 bg-slate-950/60 border border-cyan-500/20 rounded px-2 py-1';
        text.textContent = command;
        text.title = command;
        row.appendChild(text);

        if (!IS_TAURI) { wrapper.appendChild(row); return wrapper; }

        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'cyber-btn text-[10px] py-1 px-2 whitespace-nowrap shrink-0';
        button.textContent = '⌨ To terminal';
        button.title = 'Types this into the terminal. It does not run until you press Return.';
        button.addEventListener('click', async () => {
            voiceEngine.playSFX('click');
            const typed = await sendToTerminal(command);
            /* Said on the button rather than in the conversation: the operator is looking
               at the button they just pressed, and the terminal is where the answer is. */
            button.textContent = typed ? '✔ in the terminal' : '⚠ no shell';
            setTimeout(() => { button.textContent = '⌨ To terminal'; }, 2500);
        });
        row.appendChild(button);

        const githubCommand = /^\s*gh\s+/.test(command);
        const externalWrite = /^\s*(?:gh\s+pr\s+(?:create|merge)|git\s+push)\b/.test(command);
        const approve = document.createElement('button');
        approve.type = 'button';
        approve.className = 'cyber-btn cyber-btn-active text-[10px] py-1 px-2 whitespace-nowrap shrink-0';
        approve.textContent = externalWrite ? 'Review remote action' : (githubCommand ? 'Review GitHub command' : 'Run with Aether1');
        approve.title = 'Review the exact command before Aether1 runs it.';
        row.appendChild(approve);

        const approval = document.createElement('div');
        approval.className = 'hidden mt-2 p-2 border border-amber-500/30 rounded bg-amber-950/20';
        const exact = document.createElement('code');
        exact.className = 'block break-all whitespace-pre-wrap text-[11px] font-mono text-cyan-100 bg-slate-950/80 border border-slate-600/40 rounded p-2 mb-2';
        exact.textContent = command;
        const explain = document.createElement('p');
        explain.className = 'text-[10px] font-mono text-amber-200 mb-2';
        explain.textContent = externalWrite
            ? 'This will change a remote GitHub repository. Confirm this exact action; remote writes cannot be remembered.'
            : githubCommand
                ? 'This uses the GitHub CLI with your signed-in account. Confirm the exact command; GitHub commands cannot be remembered.'
                : 'This runs the exact command above in the selected workspace and sandbox. Allow once, or remember this build/test command class for this project.';
        const choices = document.createElement('div');
        choices.className = 'flex items-center gap-2 flex-wrap';
        const runOnce = document.createElement('button');
        runOnce.type = 'button';
        runOnce.className = 'cyber-btn cyber-btn-active text-[10px] py-1 px-2';
        runOnce.textContent = externalWrite ? 'Approve once' : 'Allow once & run';
        choices.appendChild(runOnce);
        if (!externalWrite && !githubCommand) {
            const remember = document.createElement('button');
            remember.type = 'button';
            remember.className = 'cyber-btn text-[10px] py-1 px-2';
            remember.textContent = 'Allow similar & run';
            choices.appendChild(remember);
            remember.addEventListener('click', () => runApprovedCodeCommand(command, true, approval, choices));
        }
        const cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'text-[10px] font-mono text-slate-400 hover:text-slate-200';
        cancel.textContent = 'Cancel';
        cancel.addEventListener('click', () => approval.classList.add('hidden'));
        choices.appendChild(cancel);
        approval.append(exact, explain, choices);
        wrapper.append(row, approval);
        approve.addEventListener('click', () => approval.classList.toggle('hidden'));
        runOnce.addEventListener('click', () => runApprovedCodeCommand(command, false, approval, choices));
        return wrapper;
    }

    async function runApprovedCodeCommand(command, rememberSimilar, approval, choices) {
        choices.querySelectorAll('button').forEach(button => { button.disabled = true; });
        const output = document.createElement('pre');
        output.className = 'mt-2 max-h-48 overflow-auto whitespace-pre-wrap text-[10px] font-mono text-slate-200';
        output.textContent = 'Running the approved command…';
        approval.appendChild(output);
        try {
            let result;
            if (IS_TAURI) {
                result = await tauriInvoke('code_run_approved_rust', { command, rememberSimilar });
            } else {
                throw new Error('Run approvals are available only in the Aether1 desktop window.');
            }
            output.textContent = result;
        } catch (err) {
            output.textContent = `Refused or failed: ${err.message || err}`;
            output.classList.add('text-amber-300');
        } finally {
            choices.querySelectorAll('button').forEach(button => { button.disabled = false; });
            approval.classList.remove('hidden');
        }
    }

    function appendCodeMessage(sender, text, commands) {
        if (!codeChatMessages) return null;
        const isUser = sender === 'user';
        const msgDiv = document.createElement('div');
        msgDiv.className = `p-3 rounded my-2 text-sm leading-relaxed ${isUser ? 'msg-user self-end ml-8' : 'msg-agent self-start mr-8'}`;

        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 border-b border-cyan-500/20 text-xs font-mono text-cyan-400/80';
        const who = document.createElement('span');
        who.innerHTML = isUser ? '👤 <strong>OPERATOR</strong>' : '⌨ <strong>AETHER CODE</strong>';
        header.appendChild(who);
        const when = document.createElement('span');
        when.textContent = new Date().toLocaleTimeString();
        header.appendChild(when);
        msgDiv.appendChild(header);

        const body = document.createElement('div');
        body.innerHTML = formatMarkdown(text);
        msgDiv.appendChild(body);

        const strip = document.createElement('div');
        strip.className = 'mt-2 pt-1 border-t border-cyan-500/10';
        msgDiv.appendChild(strip);
        msgDiv.bodyDiv = body;
        msgDiv.commandStrip = strip;
        renderCodeCommands(msgDiv, commands);

        codeChatMessages.appendChild(msgDiv);
        codeChatMessages.scrollTop = codeChatMessages.scrollHeight;
        return msgDiv;
    }

    function renderCodeCommands(msgDiv, commands) {
        const strip = msgDiv?.commandStrip;
        if (!strip) return;
        strip.innerHTML = '';
        if (!Array.isArray(commands) || commands.length === 0) {
            strip.classList.add('hidden');
            return;
        }
        strip.classList.remove('hidden');
        const label = document.createElement('div');
        label.className = 'text-[10px] font-mono text-slate-500';
        label.textContent = IS_TAURI
            ? 'Nothing below has been run. A button types it in; your Return key runs it.'
            : 'Copy these into a terminal -- the button only exists in the desktop app.';
        strip.appendChild(label);
        commands.forEach(command => strip.appendChild(codeCommandRow(command)));
    }

    /* What was said before, so reopening the tab is not a blank page. The commands are
       worked out again by the backend from the stored text, so a reply loaded from the
       database offers exactly what it offered when it arrived. */
    async function loadCodeChat() {
        refreshCodeChatModel();
        if (!codeChatMessages) return;
        try {
            const history = IS_TAURI
                ? await tauriInvoke('code_chat_history_rust')
                : await (await apiFetch('/api/code/chat/history')).json();
            (history || []).forEach(m => appendCodeMessage(m.sender, m.text, m.commands));
        } catch (e) {
            /* An empty tab is the right failure here: the conversation is a convenience,
               and the composer below it still works. */
        }
    }

    async function streamCodeChat(text, onDelta) {
        if (IS_TAURI) {
            const streamId = `c${Date.now()}${Math.random().toString(16).slice(2)}`;
            const unlisten = await window.__TAURI__.event.listen('code-chat-delta', (event) => {
                if (event.payload && event.payload.stream_id === streamId) onDelta(event.payload.delta);
            });
            try {
                return await tauriInvoke('code_chat_ask_rust', { prompt: text, streamId });
            } finally {
                unlisten();
            }
        }
        const resp = await apiFetch('/api/code/chat', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ message: text }),
        });
        if (!resp.ok) throw new Error((await resp.text()) || 'the coding model did not answer');
        return await resp.json();
    }

    async function sendCodeChat() {
        if (codeChatBusy) return;
        const text = (codeChatInput?.value || '').trim();
        if (!text) return;
        codeChatInput.value = '';
        codeChatBusy = true;

        appendCodeMessage('user', text, []);
        const replyDiv = appendCodeMessage('agent', '', []);
        replyDiv?.classList.add('typing-cursor');
        let rendered = '';

        try {
            const reply = await streamCodeChat(text, (delta) => {
                if (!delta) return;
                rendered += delta;
                replyDiv.bodyDiv.innerHTML = formatMarkdown(rendered);
                codeChatMessages.scrollTop = codeChatMessages.scrollHeight;
            });
            /* The return value is authoritative, not the accumulated deltas: the browser
               path has no deltas at all, and the command list only exists here. */
            replyDiv.bodyDiv.innerHTML = formatMarkdown(reply.text || rendered);
            renderCodeCommands(replyDiv, reply.commands);
        } catch (err) {
            const why = (err && (err.message || err)) || 'the coding model did not answer';
            replyDiv.bodyDiv.innerHTML = formatMarkdown(String(why));
            replyDiv.classList.add('text-amber-300');
            refreshCodeChatModel();
        } finally {
            replyDiv?.classList.remove('typing-cursor');
            codeChatBusy = false;
            codeChatMessages.scrollTop = codeChatMessages.scrollHeight;
        }
    }

    document.getElementById('btn-code-chat-send')?.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        sendCodeChat();
    });
    codeChatInput?.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendCodeChat(); }
    });
    document.getElementById('btn-code-chat-clear')?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        try {
            if (IS_TAURI) await tauriInvoke('code_chat_clear_rust');
            else await apiFetch('/api/code/chat/clear', { method: 'POST' });
            if (codeChatMessages) codeChatMessages.innerHTML = '';
        } catch (e) { /* nothing to forget */ }
    });

    /* Puts a command in front of the operator, in their own shell.
     *
     * Starts the shell if it is not running -- being told "start a terminal first" by the
     * program that just offered you the button is a pointless extra step -- and then types
     * and stops. Returns false when there is no terminal at all, which in practice means a
     * browser, where the button is never drawn in the first place. */
    async function sendToTerminal(command) {
        if (!terminalBridge) return false;
        try {
            /* The shell shares this panel with the conversation now, so the tab has to come
               forward: a command typed into a hidden view is a command the operator cannot
               see, and their Return key is the only thing that runs it. */
            switchChatTab('terminal');
            if (!terminalBridge.running()) await terminalBridge.ensureStarted();
            return terminalBridge.type(command);
        } catch (e) {
            return false;
        }
    }


    /* ====================== IS IT WORKING? ==============================
     * Step 47's list, in the HUD. The backend does all the deciding -- every verdict,
     * detail and proposed repair arrives as text from doctor.rs -- so this file draws
     * rows and presses buttons and has no opinion of its own about what is broken.
     *
     * Two rules it does enforce, because they are about the interface rather than the
     * diagnosis. A repair whose kind is `hand-over` gets a command to copy and never a
     * button: it needs root, and AETHER1 does not ask for passwords. And a button is
     * pressed once -- the backend refuses the same repair twice in a session anyway, and
     * a button that can be mashed invites exactly the loop that refusal exists to stop.
     */

    const doctorHeadline = document.getElementById('doctor-headline');
    const doctorChecks = document.getElementById('doctor-checks');
    const doctorStatus = document.getElementById('doctor-status');
    const btnDoctorRun = document.getElementById('btn-doctor-run');
    const btnDoctorHeal = document.getElementById('btn-doctor-heal');

    function setDoctorStatus(text, tone = 'info') {
        if (!doctorStatus) return;
        doctorStatus.classList.remove('hidden', 'text-cyan-300', 'text-green-400', 'text-red-400', 'text-slate-300', 'animate-pulse');
        if (!text) { doctorStatus.classList.add('hidden'); return; }
        const tones = { info: 'text-slate-300', busy: 'text-cyan-300', good: 'text-green-400', bad: 'text-red-400' };
        doctorStatus.classList.add(tones[tone] || tones.info);
        if (tone === 'busy') doctorStatus.classList.add('animate-pulse');
        doctorStatus.textContent = text;
    }

    async function fetchDoctorReport() {
        if (IS_TAURI) return tauriInvoke('doctor_report_rust');
        const resp = await apiFetch('/api/doctor');
        if (!resp.ok) throw new Error(`the self-check failed: ${resp.status}`);
        return resp.json();
    }

    async function fetchDiagnostics() {
        if (IS_TAURI) return tauriInvoke('run_diagnostics_rust');
        const resp = await apiFetch('/api/diagnostics');
        if (!resp.ok) throw new Error(`diagnostics failed: ${resp.status}`);
        return resp.json();
    }

    /* Every repair it can make, in one press. The same call the startup pass makes when the
       switch above is on, so what the button does and what the switch does cannot drift. */
    async function requestDoctorAttend() {
        if (IS_TAURI) return tauriInvoke('doctor_attend_rust');
        const resp = await apiFetch('/api/doctor/attend', { method: 'POST' });
        if (!resp.ok) throw new Error(await resp.text());
        return resp.json();
    }

    async function requestDoctorRepair(check, repair) {
        if (IS_TAURI) return tauriInvoke('doctor_repair_rust', { check, repair });
        const resp = await apiFetch('/api/doctor/repair', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ check, repair }),
        });
        if (!resp.ok) throw new Error(await resp.text());
        return resp.json();
    }

    /* One row. Built as nodes rather than markup because every string in it -- a detail, a
       command, a model name -- came from the machine, and a path with a < in it must not
       become part of this page. */
    function doctorRow(check) {
        const tone = {
            ok: ['border-green-500/30', 'text-green-400'],
            unknown: ['border-slate-500/30', 'text-slate-400'],
            degraded: ['border-amber-500/40', 'text-amber-400'],
            failed: ['border-red-500/40', 'text-red-400'],
        }[check.verdict] || ['border-cyan-500/20', 'text-cyan-300'];

        const row = document.createElement('div');
        row.className = `bg-slate-900/70 border rounded p-2 space-y-1 ${tone[0]}`;

        const title = document.createElement('span');
        title.className = `text-xs font-mono ${tone[1]}`;
        const mark = check.verdict === 'ok' ? '✔' : check.verdict === 'unknown' ? '?' : '✖';
        title.textContent = `${mark} ${check.title}`;
        row.appendChild(title);

        const detail = document.createElement('p');
        detail.className = 'text-[11px] font-mono text-slate-300 leading-snug';
        detail.textContent = check.detail;
        row.appendChild(detail);

        for (const step of check.steps || []) {
            const line = document.createElement('p');
            line.className = 'text-[10px] font-mono text-slate-400 leading-snug';
            line.textContent = `→ ${step.title}: ${step.detail}`;
            row.appendChild(line);
            if (step.command) row.appendChild(doctorCommand(step.command));
            if (step.url) {
                const link = document.createElement('a');
                link.className = 'text-[10px] font-mono text-cyan-400 underline';
                link.href = step.url;
                link.target = '_blank';
                link.rel = 'noreferrer';
                link.textContent = step.url;
                row.appendChild(link);
            }
        }

        if (check.repair) row.appendChild(doctorRepairRow(check));
        return row;
    }

    function doctorCommand(command) {
        const box = document.createElement('code');
        box.className = 'block text-[10px] font-mono text-cyan-200 bg-slate-950 border border-cyan-500/20 rounded p-1.5 whitespace-pre-wrap break-all select-all';
        box.textContent = command;
        return box;
    }

    /* The repair, offered rather than made. `hand-over` prints the command and stops there
       -- that is the line this whole feature does not cross. */
    function doctorRepairRow(check) {
        const repair = check.repair;
        const wrap = document.createElement('div');
        wrap.className = 'pt-1 space-y-1 border-t border-cyan-500/10';

        const what = document.createElement('p');
        what.className = 'text-[10px] font-mono text-slate-300 leading-snug';
        what.textContent = repair.detail;
        wrap.appendChild(what);

        if (repair.kind === 'hand-over') {
            const why = document.createElement('p');
            why.className = 'text-[10px] font-mono text-amber-400 leading-snug';
            why.textContent = 'AETHER1 will not run this one: it needs root, and AETHER1 never asks for your password. Run it yourself, then restart AETHER1.';
            wrap.appendChild(why);
            if (repair.command) wrap.appendChild(doctorCommand(repair.command));
            return wrap;
        }

        const button = document.createElement('button');
        button.className = 'cyber-btn text-[11px] py-1 px-3 text-cyan-300';
        button.textContent = repair.title;
        button.addEventListener('click', async () => {
            button.disabled = true;
            setDoctorStatus(`${repair.title}...`, 'busy');
            try {
                const outcome = await requestDoctorRepair(check.key, repair.id);
                // The re-check is the answer, not the repair's own exit code: a fix whose
                // check still fails is a failed fix, and saying otherwise is how a repair
                // table turns into a pile of workarounds.
                const better = outcome.rechecked === 'ok';
                setDoctorStatus(
                    better ? `${outcome.message} -- fixed.` : `${outcome.message} -- still not right: ${outcome.recheck_detail || ''}`,
                    better ? 'good' : 'bad',
                );
                await refreshDoctor({ quiet: true });
            } catch (e) {
                setDoctorStatus(`⚠ ${e.message || e}`, 'bad');
                button.disabled = false;
            }
        });
        wrap.appendChild(button);
        return wrap;
    }

    /* One line under the panel offering the model the things repair could not fix. An
       offer rather than an automatic question: asking a local model costs the operator's
       own graphics card for a minute, and a panel that starts doing that on its own is a
       panel people stop pressing buttons in. */
    function doctorOfferToAsk(attended) {
        if (!doctorChecks) return;
        const row = document.createElement('div');
        row.className = 'pt-2';
        const ask = document.createElement('button');
        ask.className = 'cyber-btn text-xs py-1 px-3 text-cyan-300';
        ask.textContent = `💬 Ask ${currentAgentName.toUpperCase()} about the rest`;
        ask.onclick = () => {
            ask.disabled = true;
            const left = (attended.remaining || []).map(r => `${r.title}: ${r.detail}`).join('\n');
            handleSendMessage(
                'AETHER1 just repaired what it could of itself and these are still wrong. '
                + 'Look into them and tell me what to do:\n\n' + left,
            );
        };
        row.appendChild(ask);
        doctorChecks.appendChild(row);
    }

    async function refreshDoctor({ quiet = false } = {}) {
        if (!doctorChecks) return;
        if (!quiet) setDoctorStatus('Checking every part of AETHER1...', 'busy');
        if (btnDoctorRun) btnDoctorRun.disabled = true;
        try {
            const report = await fetchDoctorReport();
            const health = report.health || report;
            if (doctorHeadline) doctorHeadline.textContent = health.headline || '';
            doctorChecks.replaceChildren();
            // Broken first, then everything that is fine: the order somebody reads in when
            // they came here because something is wrong.
            const order = { failed: 0, degraded: 1, unknown: 2, ok: 3 };
            const checks = [...(health.checks || [])].sort(
                (a, b) => (order[a.verdict] ?? 9) - (order[b.verdict] ?? 9),
            );
            for (const check of checks) doctorChecks.appendChild(doctorRow(check));
            for (const line of health.repeated || []) {
                const warn = document.createElement('p');
                warn.className = 'text-[11px] font-mono text-amber-400 leading-snug';
                warn.textContent = `⚠ ${line}`;
                doctorChecks.appendChild(warn);
            }
            if (!quiet) setDoctorStatus(health.needs_attention ? '' : 'Nothing needs doing.', 'good');
        } catch (e) {
            setDoctorStatus(`⚠ ${e.message || e}`, 'bad');
        } finally {
            if (btnDoctorRun) btnDoctorRun.disabled = false;
        }
    }

    /* Verify: the self-check without a model, reported into the conversation.
     *
     * The chin's Diagnostics chip sends a question to the companion, which is the right
     * thing when there is a companion -- it reads the crash and the logs behind it and
     * says what to do. But it is useless in the one case someone most wants to press it:
     * a machine with no model connected, or one that has just restarted after an update
     * and put a CRASH DETECTED card on the screen. The doctor needs no model at all, so
     * this runs it directly and lands the answer where the operator is looking.
     *
     * Deliberately a summary rather than the full panel: the rows, the details and the
     * repair buttons already exist in Settings and are better there. What this answers is
     * "is this install all right", in the smallest form that can honestly answer it. */
    function appendVerifyCard(health) {
        const checks = Array.isArray(health.checks) ? health.checks : [];
        const bad = checks.filter((c) => c.verdict === 'failed' || c.verdict === 'degraded');
        const unknown = checks.filter((c) => c.verdict === 'unknown');
        const good = checks.length - bad.length - unknown.length;
        const clean = bad.length === 0;

        const card = document.createElement('div');
        card.className = 'p-3 rounded my-2 text-sm leading-relaxed self-start mr-8 border '
            + (clean ? 'border-green-500/40 bg-green-950/20' : 'border-amber-500/40 bg-amber-950/20');

        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 border-b text-xs font-mono '
            + (clean ? 'border-green-500/20 text-green-400/90' : 'border-amber-500/20 text-amber-400/90');
        const what = document.createElement('span');
        what.textContent = clean ? '✔ VERIFIED' : '⚠ VERIFIED WITH FINDINGS';
        header.appendChild(what);
        const when = document.createElement('span');
        when.textContent = new Date().toLocaleTimeString();
        header.appendChild(when);
        card.appendChild(header);

        const line = document.createElement('p');
        line.className = clean ? 'text-green-100/90' : 'text-amber-100/90';
        /* The doctor's own headline, not one written here: it is the same sentence the
           Settings panel shows, and two different summaries of one report is how an
           operator ends up not trusting either. */
        line.textContent = health.headline
            || (clean ? 'Everything checked out.' : 'Some checks did not pass.');
        card.appendChild(line);

        const tally = document.createElement('p');
        tally.className = 'text-[11px] font-mono text-slate-400 mt-1';
        const parts = [good + ' passed'];
        if (bad.length) parts.push(bad.length + ' to look at');
        /* Named rather than counted silently: an unknown is a check that could not look,
           which is a different thing from a check that looked and was happy. */
        if (unknown.length) parts.push(unknown.length + ' could not be checked from here');
        tally.textContent = parts.join(' · ');
        card.appendChild(tally);

        for (const check of bad) {
            const row = document.createElement('p');
            row.className = 'text-[11px] font-mono text-amber-100 leading-snug mt-1';
            row.textContent = '✖ ' + check.title + ' -- ' + check.detail;
            card.appendChild(row);
        }

        if (bad.length) {
            const where = document.createElement('p');
            where.className = 'text-[10px] font-mono text-slate-400 mt-2';
            where.textContent = 'Settings, under the self-check, has the detail and the repairs.';
            card.appendChild(where);
        }

        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
    }

    /* Runs the self-check and puts the result in the conversation. Returns nothing and
       throws nothing: every path it can take ends in something the operator can read, and
       a verify that fails silently is worse than no verify at all. */
    async function verifyInstall() {
        /* Its own node rather than appendMessage: that one attributes everything it draws
           to the operator or to the companion, and neither of them said this. */
        const waiting = document.createElement('p');
        waiting.className = 'text-[11px] font-mono text-cyan-300 my-2 self-start animate-pulse';
        waiting.textContent = 'Verifying AETHER1...';
        chatContainer.appendChild(waiting);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        try {
            const report = await fetchDoctorReport();
            appendVerifyCard(report.health || report);
        } catch (e) {
            const failed = document.createElement('p');
            failed.className = 'text-[11px] font-mono text-red-400 my-2 self-start';
            failed.textContent = `⚠ The self-check could not run: ${e.message || e}`;
            chatContainer.appendChild(failed);
        } finally {
            waiting.remove();
            chatContainer.scrollTop = chatContainer.scrollHeight;
        }
    }

    document.getElementById('btn-verify')?.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        verifyInstall();
    });

    btnDoctorRun?.addEventListener('click', () => { voiceEngine.playSFX('click'); refreshDoctor(); });

    btnDoctorHeal?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        btnDoctorHeal.disabled = true;
        setDoctorStatus('Fixing what it can...', 'busy');
        try {
            const attended = await requestDoctorAttend();
            // The headline is the honest summary -- including "tried three, none worked",
            // which is the outcome most worth showing rather than hiding behind a refresh.
            setDoctorStatus(attended.headline || 'Done.', (attended.remaining || []).length ? 'info' : 'good');
            await refreshDoctor({ quiet: true });
            // What deterministic repair could not reach goes to the companion, which has
            // self_check and recent_crashes and can read the logs behind them. The pass
            // above is the half that needs no model; this is the half that needs one.
            if ((attended.remaining || []).length) doctorOfferToAsk(attended);
        } catch (e) {
            setDoctorStatus(`⚠ ${e.message || e}`, 'bad');
        } finally {
            btnDoctorHeal.disabled = false;
        }
    });

    /* The startup pass, when the switch is on, reports what it did. It arrives whether or
       not Settings is open, so it lands in the conversation rather than in a panel nobody
       is looking at -- and only when it actually attempted something. */
    if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
        window.__TAURI__.event.listen('self-repair-done', (event) => {
            const payload = (event && event.payload) || {};
            const attended = payload.attended || {};
            if (!(attended.outcomes || []).length) return;
            appendMessage('agent', `🛠 ${payload.headline}\n\n${payload.report || ''}`);
        });
    }

    // Checked when the group is opened, not when Settings is: the probe spawns processes and
    // opens connections, and most visits to Settings are not about this. Same reason the
    // coding group probes on open, and the same reason doctor.rs has no timer.
    document.getElementById('settings-group-doctor')?.addEventListener('toggle', (event) => {
        if (!event.target.open || doctorChecks?.childElementCount) return;
        refreshDoctor();
    });

    /* ========================== SETTINGS -> PROFILE ==========================
     * The operator's own pane. Three things live here and they belong together: what to
     * call whoever is typing, what they have actually done with AETHER1, and which
     * machines have paired with this one over --lan.
     *
     * Every number is counted server-side out of this machine's database (profile.rs) --
     * the browser is only allowed to draw it. Nothing here is estimated: tokens are what
     * a provider reported generating, and a model server that reports nothing leaves the
     * tile saying "not measured" rather than showing a confident zero.
     *
     * Built when the section is first opened rather than when Settings is, in the same
     * spirit as the coding and doctor groups -- it reads the whole messages table and the
     * device file, and most visits to Settings are not about this.
     */

    const profileStatsEl = document.getElementById('profile-stats');
    const profileActivityEl = document.getElementById('profile-activity');
    const profileActivityMonthsEl = document.getElementById('profile-activity-months');
    const profileActivityCaption = document.getElementById('profile-activity-caption');
    const profileInsightsEl = document.getElementById('profile-insights');
    const profileModelsEl = document.getElementById('profile-models');
    const profileDevicesEl = document.getElementById('profile-devices');
    const profileMonogram = document.getElementById('profile-monogram');
    const operatorNameInput = document.getElementById('setting-operator-name');
    const btnRevokeAllDevices = document.getElementById('btn-revoke-all-devices');

    async function fetchProfile() {
        if (IS_TAURI) return tauriInvoke('profile_report_rust');
        const resp = await apiFetch('/api/profile');
        if (!resp.ok) throw new Error(`profile request failed: ${resp.status}`);
        return resp.json();
    }

    // Load operator name and show personalized greeting
    async function loadOperatorNameAndGreeting() {
        try {
            const profile = await fetchProfile();
            const operatorName = profile?.operator?.name || '';
            operatorDisplayName = operatorName;
            if (operatorName) {
                // Display operator name in the HUD (if element exists)
                const operatorDisplay = document.getElementById('hud-operator-name');
                if (operatorDisplay) {
                    operatorDisplay.textContent = operatorName;
                    operatorDisplay.classList.remove('hidden');
                }

                // Show personalized greeting once per day
                const today = new Date().toISOString().split('T')[0];
                const lastGreetingDate = localStorage.getItem('last_greeting_date');
                if (lastGreetingDate !== today) {
                    localStorage.setItem('last_greeting_date', today);
                    const greetings = [
                        `Welcome back, ${operatorName}. I read this machine.`,
                        `${operatorName}, systems are standing by.`,
                        `Good to see you, ${operatorName}. What shall we explore today?`,
                        `${operatorName}, I have been waiting. What would you like to know?`,
                    ];
                    const greeting = greetings[Math.floor(Math.random() * greetings.length)];
                    appendMessage(currentAgentName, greeting);
                }
            }
        } catch (e) {
            // Silently fail - operator name is optional
            console.debug('Could not load operator name:', e);
        }
    }

    async function requestRevoke(id) {
        if (IS_TAURI) return tauriInvoke('revoke_device_rust', { id });
        const resp = await apiFetch('/api/profile/revoke', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ id }),
        });
        if (!resp.ok) throw new Error(await resp.text());
        return resp.json();
    }

    /* The circle beside the name field. Follows the field as it is typed rather than
       waiting for Save, because the whole point of it is to show what the name looks
       like. An empty name gets a dash, not a stray letter from somewhere else. */
    function updateProfileMonogram() {
        // Both elements are looked up here rather than read from the consts above:
        // loadSettings calls this during startup, which can run before this part of the
        // file has been evaluated, and a const read early throws rather than being
        // undefined. Same trap the avatar browser hit.
        const circle = document.getElementById('profile-monogram');
        if (!circle) return;
        const name = (document.getElementById('setting-operator-name')?.value || '').trim();
        circle.textContent = name ? Array.from(name)[0] : '—';
    }

    operatorNameInput?.addEventListener('input', updateProfileMonogram);

    /* The same phrase profile.rs::describe_machine builds, so the line under the fields
       shows what the model will actually be told without a round trip. The two have to stay
       in step; the backend's is the one that counts, and this is refreshed from its
       `machine_described` whenever the pane is read. */
    function updateMachineDescribed() {
        const line = document.getElementById('profile-machine-described');
        if (!line) return;
        const host = document.getElementById('profile-hostname')?.textContent?.trim() || '';
        const nickname = (document.getElementById('setting-machine-nickname')?.value || '').trim();
        const kind = (document.getElementById('setting-machine-kind')?.value || '').trim();
        if (!host || host === '\u2014') { line.textContent = '\u2014'; return; }
        let described = nickname ? `${nickname} (hostname ${host})` : host;
        if (kind) described = `${described}, a ${kind}`;
        line.textContent = described;
    }

    document.getElementById('setting-machine-nickname')?.addEventListener('input', updateMachineDescribed);
    document.getElementById('setting-machine-kind')?.addEventListener('change', updateMachineDescribed);

    function statTile(value, label) {
        const tile = document.createElement('div');
        tile.className = 'profile-stat';
        const number = document.createElement('div');
        number.className = 'profile-stat-value';
        number.textContent = value;
        const caption = document.createElement('div');
        caption.className = 'profile-stat-label';
        caption.textContent = label;
        tile.append(number, caption);
        return tile;
    }

    function insightRow(label, value) {
        const row = document.createElement('div');
        row.className = 'profile-insight';
        const term = document.createElement('dt');
        term.textContent = label;
        const detail = document.createElement('dd');
        detail.textContent = value;
        row.append(term, detail);
        return row;
    }

    /* 12345 -> "12.3k". Five stat tiles side by side have room for four characters and
       no more, and an exact lifetime token count is not a number anybody reads digit by
       digit anyway. */
    function compactNumber(value) {
        if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
        if (value >= 10_000) return `${Math.round(value / 1000)}k`;
        if (value >= 1000) return `${(value / 1000).toFixed(1)}k`;
        return String(value);
    }

    function plural(count, word) {
        return `${count} ${word}${count === 1 ? '' : 's'}`;
    }

    /* A YYYY-MM-DD from the backend, shown the way a person writes a date. Parsed by hand
       rather than through Date(string): a bare date string is read as UTC, which in the
       Americas renders as the day before the one it says. */
    function formatDay(day) {
        if (!day) return '—';
        const [year, month, date] = day.split('-').map(Number);
        if (!year || !month || !date) return day;
        return new Date(year, month - 1, date).toLocaleDateString(undefined, {
            year: 'numeric', month: 'short', day: 'numeric',
        });
    }

    /* The contribution grid. The backend sends only the days something was said on, so
       this walks every day in the window and looks each one up -- an empty day has to be
       drawn as an empty cell, and a gap in a sparse list is exactly what a gap looks
       like. The window starts on a Sunday so each column is one week. */
    function renderActivity(activity) {
        if (!profileActivityEl) return;
        profileActivityEl.replaceChildren();
        profileActivityMonthsEl?.replaceChildren();

        const counts = new Map((activity.counts || []).map(row => [row.day, row.messages]));
        const busiest = Math.max(1, ...counts.values());
        const today = new Date();
        today.setHours(0, 0, 0, 0);
        const start = new Date(today);
        start.setDate(start.getDate() - (activity.days - 1));
        start.setDate(start.getDate() - start.getDay());

        const months = [];
        let lastMonth = -1;
        for (let day = new Date(start); day <= today; day.setDate(day.getDate() + 1)) {
            const key = [
                day.getFullYear(),
                String(day.getMonth() + 1).padStart(2, '0'),
                String(day.getDate()).padStart(2, '0'),
            ].join('-');
            const messages = counts.get(key) || 0;
            const cell = document.createElement('div');
            cell.className = 'profile-activity-cell';
            if (messages > 0) {
                // Quartered against the busiest day rather than against a fixed count: a
                // machine that sees three messages a day and one that sees three hundred
                // should both get a readable grid.
                const level = Math.min(4, Math.ceil((messages / busiest) * 4));
                cell.classList.add(`level-${level}`);
            }
            cell.title = `${formatDay(key)} — ${plural(messages, 'message')}`;
            profileActivityEl.appendChild(cell);
            if (day.getDay() === 0 && day.getMonth() !== lastMonth) {
                lastMonth = day.getMonth();
                months.push(day.toLocaleDateString(undefined, { month: 'short' }));
            }
        }

        months.forEach(month => {
            const label = document.createElement('span');
            label.textContent = month;
            profileActivityMonthsEl?.appendChild(label);
        });
    }

    function renderModels(models) {
        if (!profileModelsEl) return;
        profileModelsEl.replaceChildren();
        if (!models.length) {
            const empty = document.createElement('li');
            empty.className = 'profile-empty';
            empty.textContent =
                'Nothing measured yet. A model counts here once its server reports how many '
                + 'tokens it generated — Ollama and the OpenAI-shaped APIs do; some local '
                + 'servers report nothing at all, and those are never guessed at.';
            profileModelsEl.appendChild(empty);
            return;
        }
        const most = Math.max(...models.map(model => model.tokens), 1);
        models.slice(0, 5).forEach(model => {
            const row = document.createElement('li');
            row.className = 'profile-model-row';
            const line = document.createElement('div');
            line.className = 'profile-model-line';
            const name = document.createElement('span');
            name.textContent = model.model;
            const meta = document.createElement('span');
            meta.className = 'profile-model-meta';
            meta.textContent = `${compactNumber(model.tokens)} tokens · ${plural(model.samples, 'reply')}`;
            line.append(name, meta);
            const bar = document.createElement('div');
            bar.className = 'profile-model-bar';
            const fill = document.createElement('span');
            fill.style.width = `${Math.max(2, (model.tokens / most) * 100)}%`;
            bar.appendChild(fill);
            row.append(line, bar);
            profileModelsEl.appendChild(row);
        });
    }

    /* Drawn from one function into whichever pane is asking: Profile shows the devices
       because that is where an operator looks for their own things, and Network & Remote
       shows them because that is where the pairing that created them lives. Two copies
       of this list would be two chances to disagree about who is paired. */
    function renderDevices(payload, listEl, revokeAllBtn, refresh) {
        if (!listEl) return;
        listEl.replaceChildren();
        const devices = payload.devices || [];
        revokeAllBtn?.classList.toggle('hidden', devices.length === 0);

        if (!devices.length) {
            const empty = document.createElement('p');
            empty.className = 'profile-empty';
            /* Both panes draw this list, and only one of them has the sequence on it, so
               this names where to go rather than assuming a button is in reach. It used to
               tell the operator to run `aether1 --serve --lan` in a terminal and points at
               a pane that has since been renamed -- and on the Network & Remote pane it
               said so directly underneath a notice saying the same thing better. */
            empty.textContent = payload.pairing_set_up
                ? 'Nothing has paired yet. Pair a device, in Network & Remote, walks through it.'
                : 'No pairing phrase yet. Pair a device, in Network & Remote, makes one as it goes.';
            listEl.appendChild(empty);
            return;
        }

        devices.forEach(device => {
            const row = document.createElement('div');
            row.className = 'profile-device';
            const text = document.createElement('div');
            const name = document.createElement('div');
            name.className = 'profile-device-name';
            name.textContent = device.label;
            const meta = document.createElement('div');
            meta.className = 'profile-device-meta';
            const paired = new Date(device.paired_at * 1000);
            meta.textContent = `${device.id} · paired ${paired.toLocaleString()}`;
            text.append(name, meta);
            const revoke = document.createElement('button');
            revoke.type = 'button';
            revoke.className = 'cyber-btn text-[10px] py-1 px-2 border-red-400 text-red-400 hover:bg-red-900/40';
            revoke.textContent = 'Revoke';
            revoke.addEventListener('click', async () => {
                voiceEngine.playSFX('click');
                if (!confirm(`Take "${device.label}" off this machine? It will have to pair again.`)) return;
                revoke.disabled = true;
                try {
                    await requestRevoke(device.id);
                    await refresh();
                } catch (e) {
                    revoke.disabled = false;
                    alert(`Could not revoke that device: ${e.message || e}`);
                }
            });
            row.append(text, revoke);
            listEl.appendChild(row);
        });
    }

    /* The "revoke everything" press, shared by both panes for the same reason the list
       is: one confirmation, one call, one consequence, wherever it was pressed from. */
    async function revokeEveryDevice(refresh) {
        voiceEngine.playSFX('click');
        // Worth spelling out on this one: run from a browser over the LAN, "all" includes
        // the browser doing the revoking, which will be logged out by its own click.
        if (!confirm('Take every paired device off this machine, including this one if you are '
            + 'on the LAN? The pairing phrase is unchanged, so each can pair again.')) return;
        try {
            await requestRevoke('all');
            await refresh();
        } catch (e) {
            alert(`Could not revoke: ${e.message || e}`);
        }
    }

    btnRevokeAllDevices?.addEventListener('click', () => revokeEveryDevice(refreshProfile));

    async function refreshProfile() {
        if (!profileStatsEl) return;
        let report;
        try {
            report = await fetchProfile();
        } catch (e) {
            profileStatsEl.replaceChildren();
            const failed = document.createElement('p');
            failed.className = 'profile-empty';
            failed.textContent = `Could not read your profile: ${e.message || e}`;
            profileStatsEl.appendChild(failed);
            return;
        }

        const stats = report.stats || {};
        const operator = report.operator || {};
        document.getElementById('profile-agent-name').textContent = operator.agent_name || 'AETHER1';
        // The short form up here — the nickname when there is one, the hostname otherwise.
        // The full phrase, with both, is under the machine card where it is being edited.
        document.getElementById('profile-machine').textContent =
            operator.machine_nickname || operator.machine || 'this machine';
        document.getElementById('profile-hostname').textContent = operator.machine || 'this machine';
        // The backend's phrase, not the local one: it is what the model is actually told.
        document.getElementById('profile-machine-described').textContent =
            operator.machine_described || operator.machine || 'this machine';
        updateProfileMonogram();

        profileStatsEl.replaceChildren(
            statTile(compactNumber(stats.messages || 0), 'Messages'),
            statTile(compactNumber(stats.conversations || 0), 'Conversations'),
            // A dash rather than 0: no measured model means nobody counted, which is a
            // different thing from having generated nothing.
            statTile(stats.measured_models ? compactNumber(stats.tokens || 0) : '—', 'Tokens generated'),
            statTile(plural(stats.current_streak || 0, 'day'), 'Current streak'),
            statTile(plural(stats.longest_streak || 0, 'day'), 'Longest streak'),
        );

        const messages = stats.messages || 0;
        if (profileActivityCaption) {
            profileActivityCaption.textContent = messages
                ? `${plural(messages, 'message')} since ${formatDay(stats.first_day)}`
                : 'Nothing said yet.';
        }
        renderActivity(report.activity || { days: 308, counts: [] });

        profileInsightsEl?.replaceChildren(
            insightRow('Conversations', String(stats.conversations || 0)),
            insightRow('You said', plural(stats.sent || 0, 'message')),
            insightRow('It answered', plural(stats.received || 0, 'message')),
            insightRow('Longest conversation', plural(stats.longest_chat || 0, 'message')),
            insightRow('Busiest day', stats.busiest_day
                ? `${formatDay(stats.busiest_day)} (${stats.busiest_day_messages})`
                : '—'),
            insightRow('First message', formatDay(stats.first_day)),
        );

        renderModels(report.models || []);
        renderDevices(report.devices || { devices: [], pairing_set_up: false },
            profileDevicesEl, btnRevokeAllDevices, refreshProfile);
    }

    // Read when the section is opened, and read again on each visit after the first: the
    // numbers move with every conversation, and a stats pane showing what was true when
    // Settings was first opened is worse than one that takes a moment to fill.
    document.addEventListener('aether-settings-section', event => {
        if (event.detail === 'profile') refreshProfile();
    });

    /* ====================== NETWORK & REMOTE =============================
     * Step 45 in the window: start the server, stop the one AETHER1 started, make a
     * pairing phrase, and see who has used one.
     *
     * Native app only. Starting `aether1 --serve --lan` means starting a child process,
     * which a browser tab cannot do -- and a browser that got here over the LAN is
     * talking *through* the very server the Stop button would kill. The rail entry is
     * hidden in the markup and revealed below once IS_TAURI is confirmed, the same way
     * the sprite and startup sections are.
     *
     * Everything the pane knows comes from one read (lan_status_rust). Nothing is
     * inferred on this side: whether the port is answering, whether the process is one
     * AETHER1 may stop, and whether a phrase exists are all questions only the Rust side
     * can answer honestly, and all three change the buttons.
     */

    const lanStateEl = document.getElementById('lan-state');
    const lanPhraseStateEl = document.getElementById('lan-phrase-state');
    const lanNoticeEl = document.getElementById('lan-notice');
    const lanAddressEl = document.getElementById('lan-address');
    const lanFingerprintEl = document.getElementById('lan-fingerprint');
    const lanPortEl = document.getElementById('lan-port');
    const lanDevicesEl = document.getElementById('lan-devices');
    const btnLanToggle = document.getElementById('btn-lan-toggle');
    const btnLanNewPhrase = document.getElementById('btn-lan-new-phrase');
    const btnLanRevokeAll = document.getElementById('btn-lan-revoke-all');
    const lanPhraseBox = document.getElementById('lan-phrase-box');
    const lanPhraseWords = document.getElementById('lan-phrase-words');
    const lanPhraseNote = document.getElementById('lan-phrase-note');
    const lanPhraseInput = document.getElementById('lan-phrase-input');
    const btnLanUsePhrase = document.getElementById('btn-lan-use-phrase');
    const lanPhraseInputNote = document.getElementById('lan-phrase-input-note');

    // What the last read said, so a button press knows whether it is starting or
    // stopping without asking again.
    let lanReport = null;

    /* The dot and the word are one state told twice -- the colour for a glance, the
       word for anyone who cannot use the colour. The dot is markup and stays; only the
       text node after it is rewritten. */
    function setNetState(el, state, label) {
        if (!el) return;
        el.dataset.state = state;
        if (el.lastChild && el.lastChild.nodeType === Node.TEXT_NODE) {
            el.lastChild.textContent = label;
        } else {
            el.append(label);
        }
    }

    function renderLan(report) {
        lanReport = report;
        if (!lanStateEl) return;
        const running = report.running === true;
        const ours = report.managed === true;
        const starting = report.starting === true;
        const devices = report.devices || { devices: [], pairing_set_up: false };

        setNetState(lanStateEl, starting ? 'busy' : (running ? 'on' : 'off'),
            starting ? 'Starting' : (running ? (ours ? 'On' : 'On, started elsewhere') : 'Off'));
        setNetState(lanPhraseStateEl, devices.pairing_set_up ? 'on' : 'off',
            devices.pairing_set_up ? 'Set' : 'Not set');

        if (btnLanToggle) {
            btnLanToggle.textContent = running ? 'Stop' : 'Start';
            // A server AETHER1 did not start is not AETHER1's to stop -- the Rust side
            // refuses it, and the button says so rather than offering a press that fails.
            btnLanToggle.disabled = starting || (running && !ours);
            btnLanToggle.title = running && !ours
                ? 'Started outside AETHER1, so AETHER1 will not stop it.'
                : '';
        }

        let notice = '';
        if (!devices.pairing_set_up) {
            notice = 'Make a pairing phrase before putting this machine on the network — '
                + 'without one there is nothing for another device to type. Pair a device '
                + 'makes one and starts the server for you.';
        } else if (running && !ours) {
            notice = `Something is already answering on port ${report.port}. AETHER1 did not `
                + 'start it, so it will not stop it either.';
        }
        // "Nothing has paired yet" is deliberately NOT here: the device list says it, a few
        // rows below, and having both said it at once was the same sentence twice.
        lanNoticeEl?.classList.toggle('hidden', !notice);
        if (lanNoticeEl) lanNoticeEl.textContent = notice;

        if (lanAddressEl) {
            const addresses = report.addresses || [];
            lanAddressEl.textContent = addresses.length
                ? addresses.map(ip => `https://${ip}:${report.port}`).join('  ·  ')
                : 'no network address on this machine';
        }
        if (lanPortEl) lanPortEl.textContent = String(report.port ?? 8378);
        if (lanFingerprintEl) {
            // Null until --lan has run once: the certificate is made the first time this
            // machine goes on the network, and a pane being looked at should not make one.
            lanFingerprintEl.textContent = report.fingerprint
                || 'made the first time this machine goes on the network';
        }

        renderDevices(devices, lanDevicesEl, btnLanRevokeAll, refreshLan);
    }

    async function refreshLan() {
        if (!IS_TAURI || !lanStateEl) return;
        // The phrase is shown by the press that made it and by nothing else. Leaving it on
        // screen until Settings closes would put a working credential in front of whoever
        // walks past next -- so coming back to the pane takes it down.
        lanPhraseBox?.classList.add('hidden');
        // A phrase half-typed and left in the box is the same credential sitting on screen,
        // so coming back to the pane starts the field empty as well.
        if (lanPhraseInput) lanPhraseInput.value = '';
        setUsePhraseNote('');
        if (btnLanUsePhrase) btnLanUsePhrase.disabled = true;
        resetPeers();
        try {
            renderLan(await tauriInvoke('lan_status_rust'));
        } catch (e) {
            if (lanNoticeEl) {
                lanNoticeEl.textContent = `Could not read the server's state: ${e.message || e}`;
                lanNoticeEl.classList.remove('hidden');
            }
        }
    }

    btnLanToggle?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        const stopping = lanReport?.running === true;
        btnLanToggle.disabled = true;
        // Said while it happens: starting waits for the child to actually bind the port,
        // which on a cold start is seconds, and a button that just sat there would read
        // as a press that did nothing.
        btnLanToggle.textContent = stopping ? 'Stopping…' : 'Starting…';
        try {
            renderLan(await tauriInvoke(stopping ? 'lan_stop_rust' : 'lan_start_rust'));
        } catch (e) {
            // In the card rather than in an alert box: the card has a line for exactly this,
            // and the commonest refusal -- no pairing phrase yet -- is one the notice is
            // already showing, so an alert was the same sentence twice with an OK button.
            await refreshLan();
            // Only when the refreshed card has nothing to say for itself. The commonest
            // refusal -- no pairing phrase yet -- is one the notice already explains at
            // more length, and replacing that with the shorter sentence loses the way out.
            if (lanNoticeEl && lanNoticeEl.classList.contains('hidden')) {
                lanNoticeEl.textContent = String(e.message || e);
                lanNoticeEl.classList.remove('hidden');
            }
        }
    });

    btnLanNewPhrase?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        const paired = lanReport?.devices?.devices?.length || 0;
        const warning = paired
            ? `This unpairs ${paired === 1 ? 'the one device' : `all ${paired} devices`} `
              + 'paired with the old phrase. Each can pair again with the new one. Continue?'
            : 'Make a new pairing phrase? It is shown once and never again.';
        if (!confirm(warning)) return;
        btnLanNewPhrase.disabled = true;
        try {
            const report = await tauriInvoke('lan_new_phrase_rust');
            renderLan(report);
            if (lanPhraseWords) lanPhraseWords.textContent = report.phrase || '';
            if (lanPhraseNote) {
                lanPhraseNote.textContent = report.unpaired
                    ? (report.unpaired === 1
                        ? 'One device was unpaired by this and will have to pair again.'
                        : `All ${report.unpaired} devices were unpaired by this and will each `
                          + 'have to pair again.')
                    : 'Type it into a browser on the other device when it asks. Pair a device '
                      + 'hands out a one-time code instead, which does not replace this.';
            }
            lanPhraseBox?.classList.remove('hidden');
        } catch (e) {
            alert(`Could not make a pairing phrase: ${e.message || e}`);
        } finally {
            btnLanNewPhrase.disabled = false;
        }
    });

    /* The line under the box, which is the only report the press gets: nothing is shown
       once here, because the operator already had the phrase before they typed it. */
    function setUsePhraseNote(text, tone) {
        if (!lanPhraseInputNote) return;
        lanPhraseInputNote.textContent = text;
        if (tone) lanPhraseInputNote.dataset.tone = tone;
        else delete lanPhraseInputNote.dataset.tone;
        lanPhraseInputNote.classList.toggle('hidden', !text);
    }

    // Empty is the only thing checked here. Whether twelve words are *the* twelve words is a
    // question only the Rust side can answer, and a guess on this side would either refuse a
    // valid phrase or promise a bad one.
    lanPhraseInput?.addEventListener('input', () => {
        if (btnLanUsePhrase) btnLanUsePhrase.disabled = !lanPhraseInput.value.trim();
    });
    lanPhraseInput?.addEventListener('keydown', event => {
        if (event.key === 'Enter' && !btnLanUsePhrase?.disabled) btnLanUsePhrase?.click();
    });

    btnLanUsePhrase?.addEventListener('click', async () => {
        const phrase = lanPhraseInput?.value.trim();
        if (!phrase) return;
        voiceEngine.playSFX('click');
        const paired = lanReport?.devices?.devices?.length || 0;
        if (paired && !confirm(
            `Answering to this phrase instead unpairs ${paired === 1 ? 'the one device' : `all ${paired} devices`} `
            + 'paired with this machine\'s current phrase. Each can pair again with the new one. Continue?'
        )) return;
        btnLanUsePhrase.disabled = true;
        setUsePhraseNote('');
        try {
            const report = await tauriInvoke('lan_set_phrase_rust', { phrase });
            renderLan(report);
            // A phrase kept on screen is a working credential kept on screen, and this one
            // was typed rather than shown, so there is nothing to read back.
            lanPhraseInput.value = '';
            lanPhraseBox?.classList.add('hidden');
            setUsePhraseNote(report.unpaired
                ? 'This machine now answers to that phrase. '
                  + (report.unpaired === 1
                      ? 'The one device paired with the old phrase will have to pair again.'
                      : `All ${report.unpaired} devices paired with the old phrase will have `
                        + 'to pair again.')
                : 'This machine now answers to that phrase. Nothing was paired, so nothing '
                  + 'was cut off.');
        } catch (e) {
            setUsePhraseNote(String(e.message || e), 'bad');
            btnLanUsePhrase.disabled = !lanPhraseInput.value.trim();
        }
    });

    /* ---- The pairing sequence ---------------------------------------------------------
     *
     * The cards below it are the same settings as facts, which is right for changing one
     * and wrong for doing the job. Pairing a phone used to mean: read three cards, work out
     * that New phrase is the button, press it and unpair every device already in, copy
     * twelve words off the screen before they vanish, then type an address by hand into a
     * phone browser. Each of those is defensible on its own and together they are a puzzle.
     *
     * So the sequence does the parts that belong to this machine itself, and asks for the
     * two that can only happen on the other one. Two things make that possible:
     *
     * - **A one-time code, not the phrase.** The phrase is shown by the press that replaces
     *   it, because nothing stores it -- which made "let one more device in" and "cut every
     *   device off" the same button. The code costs nothing: it expires by itself, is spent
     *   by the first device that uses it, and leaves the phrase and every paired device
     *   alone. The phrase is still there for anyone who wants one thing to keep.
     * - **A QR code.** Typing `https://192.168.1.44:8378` into a phone is where this was
     *   actually being lost, and a camera does not mistype.
     *
     * The last step is the one that was missing entirely: it watches, and says the device
     * is in. Before this the only way to know pairing had worked was that the other screen
     * stopped refusing.
     */

    const pairSequence = document.getElementById('pair-sequence');
    const btnPairStart = document.getElementById('btn-pair-start');
    const btnPairDone = document.getElementById('btn-pair-done');
    const btnPairNewCode = document.getElementById('btn-pair-new-code');
    const pairQrEl = document.getElementById('pair-qr');
    const pairAddressEl = document.getElementById('pair-address');
    const pairCodeEl = document.getElementById('pair-code');
    const pairCodeLifeEl = document.getElementById('pair-code-life');
    const pairOutcomeEl = document.getElementById('pair-outcome');
    const pairStep1Note = document.getElementById('pair-step-1-note');

    // The ids paired when the sequence opened. A device is "the one that just arrived" only
    // against this: revoking from another window, or a second device pairing off the same
    // code, would otherwise both read as the arrival being waited for.
    let pairKnownIds = null;
    let pairWatch = null;
    let pairCountdown = null;
    let pairCodeExpiresAt = 0;
    // The machine's clock minus this browser's, so a countdown is against the clock that set
    // the expiry. They are the same machine here, but the arithmetic should say what it means.
    let pairClockSkew = 0;

    function setPairStep(step, state) {
        document.getElementById(`pair-step-${step}`)?.setAttribute('data-state', state);
    }

    function setPairOutcome(text, tone = 'waiting') {
        if (!pairOutcomeEl) return;
        pairOutcomeEl.textContent = text;
        pairOutcomeEl.dataset.tone = tone;
    }

    /* Drawn as SVG rather than a canvas: it scales to whatever the panel is, prints, and
       survives a theme change without being redrawn. */
    function drawPairQr(text) {
        if (!pairQrEl) return;
        pairQrEl.innerHTML = '';
        if (!text || typeof qrcode !== 'function') return;
        // Type 0 lets the library pick the smallest version the text fits; M correction is
        // the usual choice for a code read off a screen rather than off a parcel.
        const code = qrcode(0, 'M');
        code.addData(text);
        code.make();
        const count = code.getModuleCount();
        let path = '';
        for (let row = 0; row < count; row++) {
            for (let col = 0; col < count; col++) {
                if (code.isDark(row, col)) path += `M${col} ${row}h1v1h-1z`;
            }
        }
        pairQrEl.innerHTML =
            `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${count} ${count}" `
            + `shape-rendering="crispEdges"><path d="${path}" fill="#000000"/></svg>`;
    }

    function pairAddressFor(report) {
        const addresses = report?.addresses || [];
        return addresses.length ? `https://${addresses[0]}:${report.port ?? 8378}` : '';
    }

    function formatPairCode(code) {
        // Halved for reading, the way every code of this length is shown. The server strips
        // the dash again, so it makes no difference to what gets typed.
        return code && code.length === 8 ? `${code.slice(0, 4)}-${code.slice(4)}` : (code || '');
    }

    function stopPairTimers() {
        if (pairWatch) { clearInterval(pairWatch); pairWatch = null; }
        if (pairCountdown) { clearInterval(pairCountdown); pairCountdown = null; }
    }

    function renderPairCountdown() {
        if (!pairCodeLifeEl) return;
        if (!pairCodeExpiresAt) { pairCodeLifeEl.textContent = ''; return; }
        const left = Math.round(pairCodeExpiresAt - (Date.now() / 1000 + pairClockSkew));
        if (left <= 0) {
            pairCodeLifeEl.textContent = 'This code has run out. Press New code for another.';
            if (pairCodeEl) pairCodeEl.textContent = '—';
            pairCodeExpiresAt = 0;
            return;
        }
        const minutes = Math.floor(left / 60);
        const seconds = String(left % 60).padStart(2, '0');
        pairCodeLifeEl.textContent = `Works for another ${minutes}:${seconds}.`;
    }

    /* Asks for a code and puts it on screen. Separate from opening the sequence because
       "New code" is the same act, and because a code that ran out while the operator was
       walking to the other room should cost one press, not a restart. */
    async function issuePairCode() {
        if (btnPairNewCode) btnPairNewCode.disabled = true;
        try {
            const report = await tauriInvoke('lan_new_pairing_code_rust');
            renderLan(report);
            pairClockSkew = (report.now || 0) - Math.floor(Date.now() / 1000);
            pairCodeExpiresAt = report.code_expires_at || 0;
            if (pairCodeEl) pairCodeEl.textContent = formatPairCode(report.code);
            setPairStep(3, 'doing');
            renderPairCountdown();
            return true;
        } catch (e) {
            if (pairCodeEl) pairCodeEl.textContent = '—';
            setPairOutcome(String(e.message || e), 'bad');
            return false;
        } finally {
            if (btnPairNewCode) btnPairNewCode.disabled = false;
        }
    }

    /* Step 1, which is the machine's own to do. Starting the server is the slow part, so it
       is said while it happens; a phrase is made only when there is none, and that is free
       precisely because nothing can be paired yet when it is true. */
    async function pairPrepareMachine() {
        let report = lanReport;
        if (!report?.devices?.pairing_set_up) {
            if (pairStep1Note) pairStep1Note.textContent = 'Making this machine a pairing phrase…';
            report = await tauriInvoke('lan_new_phrase_rust');
            renderLan(report);
        }
        if (!report?.running) {
            if (pairStep1Note) pairStep1Note.textContent = 'Starting the server…';
            report = await tauriInvoke('lan_start_rust');
            renderLan(report);
        }
        return report;
    }

    async function openPairSequence() {
        if (!pairSequence) return;
        voiceEngine.playSFX('click');
        pairSequence.classList.remove('hidden');
        if (btnPairStart) btnPairStart.disabled = true;
        stopPairTimers();
        setPairStep(1, 'doing');
        setPairStep(2, 'todo');
        setPairStep(3, 'todo');
        setPairOutcome('Waiting for a device…', 'waiting');
        if (pairCodeEl) pairCodeEl.textContent = '—';
        if (pairCodeLifeEl) pairCodeLifeEl.textContent = '';
        if (btnPairNewCode) btnPairNewCode.textContent = 'New code';

        let report;
        try {
            report = await pairPrepareMachine();
        } catch (e) {
            setPairStep(1, 'doing');
            if (pairStep1Note) pairStep1Note.textContent = String(e.message || e);
            setPairOutcome('This machine is not on the network, so nothing can pair yet.', 'bad');
            if (btnPairStart) btnPairStart.disabled = false;
            return;
        }

        pairKnownIds = new Set((report.devices?.devices || []).map(d => d.id));
        const address = pairAddressFor(report);
        setPairStep(1, 'done');
        if (pairStep1Note) {
            pairStep1Note.textContent = address
                ? 'Done — this machine is answering on your network.'
                : 'The server is up, but this machine has no network address, so nothing on '
                  + 'your network can reach it.';
        }
        setPairStep(2, address ? 'doing' : 'todo');
        if (pairAddressEl) pairAddressEl.textContent = address || '—';
        drawPairQr(address);

        if (!address) {
            setPairOutcome('No network address on this machine — check its Wi-Fi or cable.', 'bad');
            if (btnPairStart) btnPairStart.disabled = false;
            return;
        }

        if (!await issuePairCode()) {
            if (btnPairStart) btnPairStart.disabled = false;
            return;
        }

        pairCountdown = setInterval(renderPairCountdown, 1000);
        // Three seconds is under the time it takes to look up from the phone, and the read
        // is a status call on loopback, so the cost of asking is not worth economising on.
        pairWatch = setInterval(watchForPairedDevice, 3000);
    }

    async function watchForPairedDevice() {
        let report;
        try {
            report = await tauriInvoke('lan_status_rust');
        } catch (e) {
            return; // A read that failed says nothing; the next one in three seconds might.
        }
        renderLan(report);
        // Stopping LAN access with the sequence open used to leave it saying "waiting for a
        // device" with a live code on screen, directly below a card reading Off: an
        // instruction to go and type a code into a machine that stopped answering.
        if (!report.running) {
            stopPairTimers();
            pairCodeExpiresAt = 0;
            setPairStep(1, 'todo');
            setPairStep(2, 'todo');
            setPairStep(3, 'todo');
            if (pairCodeEl) pairCodeEl.textContent = '—';
            if (pairCodeLifeEl) pairCodeLifeEl.textContent = '';
            setPairOutcome(
                'LAN access was stopped, so nothing can pair. Press Start above, then Pair a '
                + 'device again.',
                'bad'
            );
            return;
        }
        const arrived = (report.devices?.devices || []).find(d => !pairKnownIds.has(d.id));
        if (!arrived) {
            // The code being spent with nothing new on the list means the device paired and
            // was revoked, or the code ran out. Either way the sequence should stop implying
            // something is still on its way.
            if (!report.code_expires_at && !pairCodeExpiresAt) {
                setPairOutcome('No code is live. Press New code to try again.', 'bad');
            }
            return;
        }
        stopPairTimers();
        pairKnownIds.add(arrived.id);
        setPairStep(2, 'done');
        setPairStep(3, 'done');
        setPairOutcome(`Paired — ${arrived.label}. It stays paired until you revoke it.`, 'good');
        if (pairCodeEl) pairCodeEl.textContent = '—';
        if (pairCodeLifeEl) pairCodeLifeEl.textContent = 'The code has been used and no longer works.';
        // The same button, renamed to what pressing it would now mean: one code is one
        // device, so a second device is a second code rather than a second sequence.
        if (btnPairNewCode) btnPairNewCode.textContent = 'Pair another';
        voiceEngine.playSFX('click');
    }

    /* Closing takes the code down with it, here and on disk. A code still working after the
       screen showing it is gone is an invitation nobody can see to withdraw. */
    async function closePairSequence() {
        stopPairTimers();
        pairCodeExpiresAt = 0;
        pairSequence?.classList.add('hidden');
        if (btnPairStart) btnPairStart.disabled = false;
        try {
            renderLan(await tauriInvoke('lan_clear_pairing_code_rust'));
        } catch (e) {
            /* The pane is closing either way; a code that outlives it expires by itself. */
        }
    }

    btnPairStart?.addEventListener('click', openPairSequence);
    btnPairDone?.addEventListener('click', () => { voiceEngine.playSFX('click'); closePairSequence(); });
    btnPairNewCode?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        setPairOutcome('Waiting for a device…', 'waiting');
        setPairStep(2, 'doing');
        setPairStep(3, 'doing');
        if (!await issuePairCode()) return;
        // Watching stopped when the last device arrived, so a fresh code needs it back.
        stopPairTimers();
        pairCountdown = setInterval(renderPairCountdown, 1000);
        pairWatch = setInterval(watchForPairedDevice, 3000);
    });

    /* ---- Who else is out there -------------------------------------------------
     *
     * discovery.rs has announced and browsed for AETHER1s since step 45, but only the
     * `aether1 discover` subcommand ever called it -- so the window could show a pairing
     * code while having no way to say which machines were on the network to type it into.
     * This is that call, in the pane.
     *
     * Reading only, and on demand. A scan browses for a few seconds and stops; nothing is
     * contacted, nothing about this machine changes, and no scan runs unless the button is
     * pressed. Pairing to a machine in this list is the next piece, not this one.
     */

    const lanPeersEl = document.getElementById('lan-peers');
    const lanScanNoticeEl = document.getElementById('lan-scan-notice');
    const btnLanScan = document.getElementById('btn-lan-scan');

    function setScanNotice(text) {
        if (!lanScanNoticeEl) return;
        lanScanNoticeEl.textContent = text || '';
        lanScanNoticeEl.classList.toggle('hidden', !text);
    }

    /* Which machines this one has already paired with, from the last status read. The
       answer lives on the Rust side, so a row can say "paired" without this side keeping
       its own idea of who is. */
    function pairedWith(peer) {
        const paired = (lanReport && lanReport.paired_peers) || [];
        return paired.some(p => p.address === peer.address && p.port === peer.port);
    }

    /* Built as nodes rather than markup: every string here comes off the network, from a
       name another machine chose for itself. */
    function peerRow(peer) {
        const row = document.createElement('div');
        row.className = 'net-row net-peer';

        const text = document.createElement('div');
        text.className = 'net-row-text';
        const title = document.createElement('p');
        title.className = 'net-row-title';
        title.textContent = peer.is_this_machine
            ? `${peer.name} (this machine)`
            : String(peer.name || 'an AETHER1');
        const hint = document.createElement('p');
        hint.className = 'net-row-hint';
        const addresses = Array.isArray(peer.addresses) ? peer.addresses : [];
        const where = addresses.length
            ? addresses.map(address => `${address}:${peer.port}`).join('  ')
            : `port ${peer.port}`;
        hint.textContent = peer.version ? `${where} — AETHER1 ${peer.version}` : where;
        text.append(title, hint);
        row.append(text);

        // Nothing to offer for this machine: it is already reachable from here, and
        // pairing with yourself would hand out a token nothing would ever send.
        if (peer.is_this_machine || !addresses.length) return row;

        if (pairedWith({ address: addresses[0], port: peer.port })) {
            const state = document.createElement('span');
            state.className = 'net-state';
            state.dataset.state = 'on';
            state.append(document.createElement('i'), 'Paired');
            // Whether questions this machine cannot answer go to that one. Off unless it
            // is asked for: a question leaving this machine is the operator's choice.
            const helper = document.createElement('button');
            helper.type = 'button';
            const chosen = (lanReport && lanReport.chat_peer) === `${addresses[0]}:${peer.port}`;
            helper.className = chosen
                ? 'net-card-action'
                : 'net-card-action net-card-action-quiet';
            helper.textContent = chosen ? 'Answering chat' : 'Let it answer chat';
            helper.title = chosen
                ? 'Questions this machine cannot answer are sent here. Press to stop.'
                : 'When this machine has no model of its own, send questions here instead.';
            helper.addEventListener('click', () =>
                setChatPeer(chosen ? '' : addresses[0], peer.port, helper));

            const models = document.createElement('button');
            models.type = 'button';
            models.className = 'net-card-action net-card-action-quiet';
            models.textContent = 'What it can run';
            models.addEventListener('click', () => askPeerModels(row, addresses[0], peer.port, models));
            const forget = document.createElement('button');
            forget.type = 'button';
            forget.className = 'net-card-action net-card-action-quiet';
            forget.textContent = 'Forget';
            forget.addEventListener('click', () => forgetPeer(addresses[0], peer.port, forget));
            const side = document.createElement('div');
            side.className = 'net-peer-side';
            side.append(state, helper, models, forget);
            row.append(side);
            return row;
        }

        const pair = document.createElement('button');
        pair.type = 'button';
        pair.className = 'net-card-action';
        pair.textContent = 'Pair';
        pair.addEventListener('click', () => openPeerPairing(row, peer, addresses[0], pair));
        row.append(pair);
        return row;
    }

    /* The code goes in on the row of the machine it belongs to. Asking for it anywhere else
       is how the old flow ended up with codes on one screen and a box on another. */
    function openPeerPairing(row, peer, address, pairButton) {
        if (row.querySelector('.net-peer-pair')) return;
        pairButton.disabled = true;

        const box = document.createElement('div');
        box.className = 'net-peer-pair';
        const label = document.createElement('p');
        label.className = 'net-row-hint';
        label.textContent =
            `On ${peer.name}, open Settings, Network & Remote, Pair a device. Type the code `
            + 'it shows here. Its twelve-word phrase works too.';

        /* A machine is found by asking the network who is out there, and anything on the
           network can answer in anyone's name. So the name on this row proves nothing and
           the fingerprint is the only thing that does: it is a digest of a certificate
           whose private key that machine has, which an impostor cannot copy. The other
           machine prints it when it starts and shows it in its own pane, so the two can be
           compared by eye -- and until they have been, there is nothing to press. */
        const identity = document.createElement('div');
        identity.className = 'net-peer-identity';
        const identityLabel = document.createElement('p');
        identityLabel.className = 'net-row-hint';
        identityLabel.textContent = 'Checking which machine this is…';
        const print = document.createElement('p');
        print.className = 'net-peer-print hidden';
        const confirmRow = document.createElement('label');
        confirmRow.className = 'net-peer-confirm hidden';
        const confirm = document.createElement('input');
        confirm.type = 'checkbox';
        const confirmText = document.createElement('span');
        confirmText.textContent = `This is what ${peer.name} shows`;
        confirmRow.append(confirm, confirmText);
        identity.append(identityLabel, print, confirmRow);

        const field = document.createElement('input');
        field.type = 'text';
        // The same field the phrase box uses, so it is the one input style in the pane.
        field.className = 'w-full bg-slate-900 border border-cyan-500/40 rounded p-2 text-sm text-cyan-100 font-mono';
        field.autocomplete = 'off';
        field.spellcheck = false;
        field.placeholder = 'the code, or twelve words';
        const go = document.createElement('button');
        go.type = 'button';
        go.className = 'net-card-action';
        go.textContent = 'Pair';
        // Nothing is typed at a machine whose identity has not been read back yet.
        go.disabled = true;
        const cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'net-card-action net-card-action-quiet';
        cancel.textContent = 'Cancel';
        const outcome = document.createElement('p');
        outcome.className = 'net-field-note hidden';

        const close = () => { box.remove(); pairButton.disabled = false; };
        cancel.addEventListener('click', close);

        /* Read before anything secret is typed, and kept: what goes to the Rust side with
           the code is the fingerprint the operator actually looked at, so a machine that
           swaps certificates between the look and the press is refused rather than pinned. */
        let seen = '';
        confirm.addEventListener('change', () => {
            go.disabled = !(confirm.checked && seen);
        });
        tauriInvoke('lan_peer_fingerprint_rust', { address, port: peer.port })
            .then(report => {
                seen = String(report?.fingerprint || '');
                if (!seen) throw new Error('that machine did not identify itself');
                identityLabel.textContent =
                    `Check this against the fingerprint ${peer.name} shows under Network & `
                    + 'Remote, or printed when it started:';
                print.textContent = seen;
                print.classList.remove('hidden');
                confirmRow.classList.remove('hidden');
            })
            .catch(e => {
                identityLabel.textContent =
                    `Could not ask ${peer.name} which machine it is: ${e.message || e}`;
                identityLabel.dataset.tone = 'bad';
            });

        async function submit() {
            const secret = field.value.trim();
            if (!secret || go.disabled) return;
            go.disabled = true;
            field.disabled = true;
            go.textContent = 'Pairing…';
            outcome.classList.add('hidden');
            try {
                const report = await tauriInvoke('lan_pair_with_rust', {
                    name: peer.name, address, port: peer.port, secret,
                    expectFingerprint: seen,
                });
                // The row is redrawn from the fresh status, so "Paired" is the Rust side's
                // answer rather than this side assuming the press worked.
                renderLan(report);
                renderPeers(lastScan);
            } catch (e) {
                outcome.textContent = String(e.message || e);
                outcome.dataset.tone = 'bad';
                outcome.classList.remove('hidden');
                go.disabled = false;
                field.disabled = false;
                go.textContent = 'Pair';
                field.focus();
            }
        }
        go.addEventListener('click', submit);
        field.addEventListener('keydown', event => {
            if (event.key === 'Enter') { event.preventDefault(); submit(); }
            if (event.key === 'Escape') close();
        });

        const controls = document.createElement('div');
        controls.className = 'net-field-row';
        controls.append(field, go, cancel);
        box.append(label, identity, controls, outcome);
        row.append(box);
        field.focus();
    }

    /* The first thing the token this machine was given is actually for. It reads the other
       machine's model scan and changes nothing on either end, which is why it is a button
       rather than a question. */
    async function askPeerModels(row, address, port, button) {
        const previous = row.querySelector('.net-peer-models');
        if (previous) previous.remove();
        button.disabled = true;
        button.textContent = 'Asking…';
        const answer = document.createElement('p');
        answer.className = 'net-row-hint net-peer-models';
        try {
            const report = await tauriInvoke('lan_peer_models_rust', { address, port });
            const models = Array.isArray(report?.models) ? report.models : [];
            answer.textContent = models.length
                ? `Can run: ${models.join(', ')}`
                : 'Reached it, and it has no model loaded right now.';
        } catch (e) {
            answer.textContent = String(e.message || e);
            answer.dataset.tone = 'bad';
        } finally {
            button.disabled = false;
            button.textContent = 'What it can run';
            row.append(answer);
        }
    }

    async function setChatPeer(address, port, button) {
        button.disabled = true;
        try {
            renderLan(await tauriInvoke('lan_set_chat_peer_rust', { address, port }));
            renderPeers(lastScan);
        } catch (e) {
            setScanNotice(`Could not remember that choice: ${e.message || e}`);
            button.disabled = false;
        }
    }

    async function forgetPeer(address, port, button) {
        button.disabled = true;
        try {
            renderLan(await tauriInvoke('lan_forget_peer_rust', { address, port }));
            renderPeers(lastScan);
        } catch (e) {
            setScanNotice(`Could not forget that machine: ${e.message || e}`);
            button.disabled = false;
        }
    }

    // What the last scan found, so a row can be redrawn after pairing without scanning
    // again -- three more seconds of waiting to see a word change.
    let lastScan = [];

    function renderPeers(peers) {
        lastScan = peers;
        if (!lanPeersEl) return;
        lanPeersEl.textContent = '';
        if (!peers.length) {
            const empty = document.createElement('p');
            empty.className = 'profile-empty';
            empty.textContent =
                'Nothing answered. Only a machine that is serving announces itself, so '
                + 'start LAN access over there and scan again.';
            lanPeersEl.append(empty);
            return;
        }
        peers.forEach(peer => lanPeersEl.append(peerRow(peer)));
    }

    async function scanForPeers() {
        if (!IS_TAURI || !btnLanScan) return;
        btnLanScan.disabled = true;
        btnLanScan.textContent = 'Scanning…';
        setScanNotice('');
        try {
            const report = await tauriInvoke('lan_discover_rust');
            renderPeers(Array.isArray(report?.peers) ? report.peers : []);
        } catch (e) {
            setScanNotice(`Could not scan the network: ${e.message || e}`);
        } finally {
            btnLanScan.disabled = false;
            btnLanScan.textContent = 'Scan';
        }
    }

    /* Cleared on every visit to the pane. A list of machines is a fact about the moment
       it was gathered, and one left over from last time would be read as current. */
    function resetPeers() {
        if (!lanPeersEl) return;
        lanPeersEl.textContent = '';
        lastScan = [];
        const prompt = document.createElement('p');
        prompt.className = 'profile-empty';
        prompt.textContent = 'Press Scan to see which machines are serving right now.';
        lanPeersEl.append(prompt);
        setScanNotice('');
    }

    btnLanScan?.addEventListener('click', scanForPeers);

    btnLanRevokeAll?.addEventListener('click', () => revokeEveryDevice(refreshLan));

    // The LAN cards live inside Network & Remote, and only the native app can start or
    // stop the server behind them -- so it is the cards that are revealed here, not a rail
    // entry: local-only mode on the same page is a question in a browser tab too.
    function initRemoteLan() {
        if (!IS_TAURI) return;
        document.getElementById('remote-lan-cards')?.classList.remove('hidden');
    }
    initRemoteLan();

    // Read when the section is opened, and on each visit after: whether a server is up is
    // a fact about right now, and it can change from a terminal while Settings is open.
    document.addEventListener('aether-settings-section', event => {
        if (!IS_TAURI) return;
        if (event.detail === 'network') {
            refreshLan();
        } else if (pairSequence && !pairSequence.classList.contains('hidden')) {
            // Walking away from the pane ends the sequence, for the same reason closing it
            // does: the code should not outlive the screen that was showing it.
            closePairSequence();
        }
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
        if (!resp.ok) throw new Error(`platform initialization failed: ${resp.status}`);
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
                // Played through the same engine and the same Web Audio graph a reply
                // goes through, so a test that is audible here is proof the reply path is
                // audible too -- a separate player would only prove that a separate player
                // works. It supersedes any reply still being spoken rather than queueing
                // behind it, which is what you want from a button marked "test".
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
        // Two places show the same bars: the wizard, and the Voice & Sound hub in
        // Settings. One download, drawn wherever it is being watched from -- a bar that
        // only appeared in the panel the button was pressed in would make a download
        // started in the hub look like nothing happened.
        const boxes = [voicePickerDownloads, document.getElementById('vhub-downloads')]
            .filter(Boolean);
        if (!boxes.length) return;
        for (const box of boxes) box.innerHTML = '';
        for (const box of boxes) for (const d of list) box.appendChild(renderDownloadRow({
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
            // The hub's list says "already here" per voice, so it is stale the moment one
            // lands. Quiet, because this is already inside the poll that noticed.
            refreshSoundHub({ quiet: true }).catch(() => null);
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

    /* ====================== THE VOICE & SOUND HUB =========================
     * The Brain's hub, applied to the other half of the companion.
     *
     * Two tabs, because the pane answers two questions and they had been stirred
     * together into one column of fields. Voices is *which voice it speaks in*: the
     * fixed Piper catalogue, with a detail panel instead of a row of download buttons.
     * Devices is *which speaker and which microphone*, which the app had never asked
     * about at all -- it used whatever the system handed it, which on a desk with a
     * headset, an interface and an HDMI monitor is a coin toss, and the symptom is
     * silence.
     *
     * The device list comes from two places at once and neither is sufficient alone:
     *
     *   - The operating system, through audio_devices.rs. Always available, names every
     *     device whether or not this page has been given permission to see them, and is
     *     the only list the native window can get -- WebKitGTK never answers the media
     *     permission request there (the microphone bug), so enumerateDevices comes back
     *     with blank labels.
     *   - The browser, through enumerateDevices. The only list whose identifiers can
     *     actually be used: setSinkId and getUserMedia take a browser deviceId and
     *     nothing else.
     *
     * So the choice is stored as an id *and* a label, and resolveAudioDevice matches the
     * two lists up by label at the moment of use. A stored device that is unplugged
     * resolves to nothing and the system default is used, which is the right answer and
     * not an error.
     */

    const soundHub = {
        tab: 'voices',
        search: '',
        filter: 'all',
        voices: [],
        selected: null,
        system: { outputs: [], inputs: [], source: 'none', note: '' },
        browser: { outputs: [], inputs: [] },
        loading: false,
        loaded: false,
        // What loadSettings last read, so the selects can be filled before the device
        // lists arrive and still end up on the right entry once they do.
        chosen: { output: '', outputLabel: '', input: '', inputLabel: '' },
    };

    async function fetchAudioDevices() {
        if (IS_TAURI) return tauriInvoke('audio_devices_rust');
        const resp = await apiFetch('/api/audio/devices');
        if (!resp.ok) throw new Error(`device list failed: ${resp.status}`);
        return resp.json();
    }

    /* What this page can see. Labels are blank until something has been granted
       microphone permission, which is normal rather than broken -- the system list above
       carries the names in that case. */
    async function enumerateBrowserDevices() {
        if (!navigator.mediaDevices?.enumerateDevices) return { outputs: [], inputs: [] };
        try {
            const all = await navigator.mediaDevices.enumerateDevices();
            const pick = (kind) => all
                .filter(d => d.kind === kind && d.deviceId && d.deviceId !== 'default')
                .map(d => ({ deviceId: d.deviceId, label: d.label || '' }));
            return { outputs: pick('audiooutput'), inputs: pick('audioinput') };
        } catch (e) {
            return { outputs: [], inputs: [] };
        }
    }

    /* The stored choice turned into something the browser will accept, or '' for "let
       the system decide". The id is tried first because it is exact; the label is the
       fallback that carries a choice made from the system list across to the browser's
       own identifiers, which are a different namespace entirely. */
    function resolveAudioDevice(list, id, label) {
        if (!id && !label) return '';
        const exact = list.find(d => d.deviceId === id);
        if (exact) return exact.deviceId;
        const want = String(label || '').trim().toLowerCase();
        if (!want) return '';
        const loose = list.find(d => {
            const have = d.label.trim().toLowerCase();
            return have && (have === want || have.includes(want) || want.includes(have));
        });
        return loose ? loose.deviceId : '';
    }

    /* One select's worth of options: every device the system named, then any the browser
       can see and the system did not. Each option carries its own label, because that is
       what gets saved alongside the id. */
    function audioOptionsFor(direction) {
        const system = direction === 'input' ? soundHub.system.inputs : soundHub.system.outputs;
        const browser = direction === 'input' ? soundHub.browser.inputs : soundHub.browser.outputs;
        const options = system.map(d => ({
            value: d.id,
            label: d.label,
            suffix: d.is_default ? ' — system default' : '',
        }));
        for (const d of browser) {
            if (!d.label) continue;
            const already = options.some(o => {
                const have = o.label.trim().toLowerCase();
                const mine = d.label.trim().toLowerCase();
                return have === mine || have.includes(mine) || mine.includes(have);
            });
            if (!already) options.push({ value: d.deviceId, label: d.label, suffix: '' });
        }
        return options;
    }

    function fillDeviceSelect(id, direction, chosenValue, chosenLabel) {
        const select = document.getElementById(id);
        if (!select) return;
        const options = audioOptionsFor(direction);
        select.innerHTML = '';

        const first = document.createElement('option');
        first.value = '';
        first.textContent = 'System default';
        first.dataset.label = '';
        select.appendChild(first);

        for (const option of options) {
            const el = document.createElement('option');
            el.value = option.value;
            el.textContent = option.label + option.suffix;
            el.dataset.label = option.label;
            select.appendChild(el);
        }

        // A device that was chosen and is now unplugged stays in the list, marked, rather
        // than silently reverting to the default: "why is it not using my headset" has a
        // visible answer that way.
        if (chosenValue && !Array.from(select.options).some(o => o.value === chosenValue)) {
            const gone = document.createElement('option');
            gone.value = chosenValue;
            gone.textContent = `${chosenLabel || chosenValue} — not plugged in`;
            gone.dataset.label = chosenLabel || '';
            select.appendChild(gone);
        }
        select.value = chosenValue || '';
    }

    function setSoundHubStatus(message, tone) {
        const box = document.getElementById('vhub-status');
        if (!box) return;
        if (!message) { box.classList.add('hidden'); box.textContent = ''; return; }
        box.classList.remove('hidden');
        box.className = 'text-xs font-mono p-2.5 rounded border leading-snug ' + ({
            good: 'border-green-500/40 bg-green-950/20 text-green-300',
            bad: 'border-red-500/40 bg-red-950/20 text-red-300',
            busy: 'border-cyan-500/30 bg-slate-900/80 text-cyan-200',
        }[tone] || 'border-cyan-500/20 bg-slate-900/80 text-slate-300');
        box.textContent = message;
    }

    function renderSoundChips() {
        const box = document.getElementById('vhub-machine');
        if (!box) return;
        box.innerHTML = '';
        const chip = (value, label, state) => {
            const el = document.createElement('span');
            el.className = 'hub-chip';
            if (state) el.dataset.state = state;
            const v = document.createElement('span');
            v.className = 'hub-chip-value';
            v.textContent = value;
            const l = document.createElement('span');
            l.className = 'hub-chip-label';
            l.textContent = label;
            el.append(v, l);
            box.appendChild(el);
        };

        const installed = soundHub.voices.filter(v => v.installed).length;
        chip(`${installed}/${soundHub.voices.length}`, 'VOICES', installed ? 'good' : 'warn');

        const named = (direction) => {
            const chosenLabel = direction === 'input'
                ? soundHub.chosen.inputLabel : soundHub.chosen.outputLabel;
            const chosenId = direction === 'input'
                ? soundHub.chosen.input : soundHub.chosen.output;
            if (chosenLabel || chosenId) return chosenLabel || chosenId;
            const list = direction === 'input' ? soundHub.system.inputs : soundHub.system.outputs;
            const fallback = list.find(d => d.is_default);
            return fallback ? fallback.label : 'System default';
        };
        chip(soundHubShort(named('output')), 'SPEAKER');
        chip(soundHubShort(named('input')), 'MIC');
    }

    /* A chip is one line on a narrow window, and a device name can run to forty
       characters of chipset model. The full name is in the select below it. */
    function soundHubShort(text) {
        const value = String(text || '').trim();
        return value.length > 22 ? `${value.slice(0, 21)}…` : (value || '—');
    }

    function soundHubVisibleVoices() {
        const needle = soundHub.search.trim().toLowerCase();
        return soundHub.voices.filter(voice => {
            if (soundHub.filter === 'installed' && !voice.installed) return false;
            if (soundHub.filter === 'missing' && voice.installed) return false;
            if (!needle) return true;
            return `${voice.name} ${voice.label}`.toLowerCase().includes(needle);
        });
    }

    function renderSoundHubList() {
        const list = document.getElementById('vhub-list');
        if (!list) return;
        list.innerHTML = '';
        const voices = soundHubVisibleVoices();

        const title = document.getElementById('vhub-list-title');
        if (title) title.textContent = `Offline voices (${voices.length})`;

        if (!voices.length) {
            const empty = document.createElement('div');
            empty.className = 'hub-empty';
            empty.textContent = soundHub.loaded
                ? 'No voice here matches that.'
                : 'Reading the voices folder...';
            list.appendChild(empty);
            return;
        }

        for (const voice of voices) {
            const row = document.createElement('button');
            row.type = 'button';
            row.className = 'hub-row';
            row.setAttribute('role', 'option');
            const selected = voice.name === soundHub.selected;
            row.classList.toggle('is-selected', selected);
            row.setAttribute('aria-selected', selected ? 'true' : 'false');

            const glyph = document.createElement('span');
            glyph.className = 'hub-row-glyph';
            glyph.textContent = voice.installed ? '◉' : '○';
            row.appendChild(glyph);

            const body = document.createElement('span');
            body.className = 'hub-row-body';
            const name = document.createElement('span');
            name.className = 'hub-row-name';
            name.textContent = voice.label;
            const meta = document.createElement('span');
            meta.className = 'hub-row-meta';
            meta.textContent = voice.installed
                ? `${voice.name} · already here`
                : `${voice.name} · ${voice.size_hint}`;
            body.append(name, meta);
            row.appendChild(body);

            row.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                soundHub.selected = voice.name;
                renderSoundHubList();
                renderSoundHubDetail();
            });
            list.appendChild(row);
        }
    }

    /* Which avatars already speak in this voice. It comes from the backend persona
       catalogue rather than from anything written here -- the same rows the avatar
       browser joins on -- so downloading a voice can say who it is for. */
    function avatarsUsingVoice(name) {
        return Array.from(personaRows.values())
            .filter(row => row.local_voice === name)
            .map(row => row.label || row.key)
            .filter(Boolean);
    }

    function renderSoundHubDetail() {
        const panel = document.getElementById('vhub-detail');
        if (!panel) return;
        panel.innerHTML = '';

        const voice = soundHub.voices.find(v => v.name === soundHub.selected);
        if (!voice) {
            const empty = document.createElement('div');
            empty.className = 'hub-empty';
            empty.textContent = soundHub.voices.length
                ? 'Pick a voice to see what it is and how big it is.'
                : 'Nothing to show yet.';
            panel.appendChild(empty);
            return;
        }

        const name = document.createElement('div');
        name.className = 'hub-detail-name';
        name.textContent = voice.label;
        const sub = document.createElement('div');
        sub.className = 'hub-detail-sub';
        sub.textContent = voice.name;
        panel.append(name, sub);

        const tags = document.createElement('div');
        tags.className = 'hub-tags';
        const tag = (text, tone) => {
            const el = document.createElement('span');
            el.className = 'hub-tag';
            if (tone) el.dataset.tone = tone;
            el.textContent = text;
            tags.appendChild(el);
        };
        tag(voice.installed ? 'Downloaded' : 'Not here yet', voice.installed ? 'on' : 'off');
        tag('Piper · offline', 'pick');
        panel.appendChild(tags);

        const users = avatarsUsingVoice(voice.name);
        const blurb = document.createElement('p');
        blurb.className = 'hub-blurb';
        blurb.textContent = users.length
            ? `${users.join(', ')} speak${users.length === 1 ? 's' : ''} in this one, once it is downloaded.`
            : 'No avatar asks for this voice by name; it is available to the ones that have none of their own.';
        panel.appendChild(blurb);

        const facts = document.createElement('div');
        facts.className = 'hub-facts';
        const fact = (label, value) => {
            const box = document.createElement('div');
            box.className = 'hub-fact';
            const l = document.createElement('div');
            l.className = 'hub-fact-label';
            l.textContent = label;
            const v = document.createElement('div');
            v.className = 'hub-fact-value';
            v.textContent = value;
            box.append(l, v);
            facts.appendChild(box);
        };
        fact('SIZE', voice.installed ? 'on disk' : voice.size_hint);
        fact('FILE', voice.path ? voice.path.split(/[\\/]/).pop() : `${voice.name}.onnx`);
        panel.appendChild(facts);

        const action = document.createElement('div');
        action.className = 'hub-action';
        const size = document.createElement('span');
        size.className = 'hub-action-size';
        // The size is already a fact above, so this line says *where it goes* instead --
        // the one thing a download decision needs that the row does not carry.
        size.textContent = voice.path
            ? (voice.installed ? voice.path : `Downloads to ${voice.path}`)
            : 'In the voices folder';
        action.appendChild(size);

        if (voice.installed) {
            // Fills the Piper voice-file box rather than saving by itself: this panel
            // shares one Save Changes with every other pane, and a panel that saved on
            // its own would also commit half-typed edits somewhere else.
            const use = document.createElement('button');
            use.type = 'button';
            use.className = 'cyber-btn cyber-btn-active text-[11px] py-1.5 px-3';
            use.textContent = '✔ Use this one';
            use.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                const box = document.getElementById('setting-tts-local-voice');
                if (box) box.value = voice.path || '';
                setSoundHubStatus(
                    `${voice.label} is now the Piper voice file. Press Save Changes to keep it.`,
                    'good');
            });
            action.appendChild(use);
        } else {
            const get = document.createElement('button');
            get.type = 'button';
            get.className = 'cyber-btn cyber-btn-active text-[11px] py-1.5 px-3';
            get.textContent = '⬇ Download';
            get.addEventListener('click', () => startVoiceDownload(voice.name, get));
            action.appendChild(get);
        }
        panel.appendChild(action);
    }

    function renderSoundHubDevices() {
        fillDeviceSelect('setting-audio-output', 'output',
            soundHub.chosen.output, soundHub.chosen.outputLabel);
        fillDeviceSelect('setting-audio-input', 'input',
            soundHub.chosen.input, soundHub.chosen.inputLabel);

        const sources = {
            pactl: 'Read from PipeWire/PulseAudio.',
            wpctl: 'Read from WirePlumber.',
            powershell: 'Read from Windows.',
            system_profiler: 'Read from macOS.',
            none: 'This computer would not list its devices.',
        };
        const note = soundHub.system.note
            || sources[soundHub.system.source]
            || '';
        const blind = !soundHub.browser.outputs.some(d => d.label);
        const outNote = document.getElementById('vhub-output-note');
        if (outNote) {
            // Two different facts, and conflating them is how somebody concludes the
            // setting does nothing: the device list is read from the system and always
            // works, but *moving the sound from this window* needs the browser to offer
            // the same device, which it will not do until it has been given audio
            // permission. The saved choice still steers `aether1 say` either way.
            outNote.textContent = blind
                ? `${soundHub.system.outputs.length} found. ${note} Press "Check the level" once so this window can use them too.`.trim()
                : `${soundHub.system.outputs.length} found. ${note}`.trim();
        }
        const inNote = document.getElementById('vhub-input-note');
        if (inNote) {
            inNote.textContent = blind
                ? `${soundHub.system.inputs.length} found. Names come from the system; this window has not been given microphone access.`
                : `${soundHub.system.inputs.length} found. ${note}`.trim();
        }
    }

    /* ------------------------- How to say a word -------------------------
       Trident: "the equivalent of a spell check in a pronunciation check list? There are a
       few words that I find hard to discern when not using my local pronunciations."

       A table of say-this-as-that, stored whole rather than row by row. A row has no
       identity of its own while it is being typed -- saving per keystroke would store
       `Aeth` as a rule -- so the whole list goes at once, on blur and on remove, and the
       reply is what gets drawn back. Rust trims it, drops the empty row every editor leaves
       behind, and collapses duplicates, so redrawing from its answer is the only way the
       pane and the voice cannot disagree. */
    let pronunciations = { words: [], maxEntries: 200, maxLength: 120 };

    async function fetchPronunciations() {
        if (IS_TAURI) return tauriInvoke('pronunciations_rust');
        const resp = await apiFetch('/api/speech/pronunciations');
        if (!resp.ok) throw new Error(`could not read the pronunciations: ${resp.status}`);
        return resp.json();
    }

    async function storePronunciations(words) {
        if (IS_TAURI) return tauriInvoke('set_pronunciations_rust', { words });
        const resp = await apiFetch('/api/speech/pronunciations', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ words }),
        });
        if (!resp.ok) throw new Error((await resp.text()) || `save failed: ${resp.status}`);
        return resp.json();
    }

    function takePronunciations(payload) {
        pronunciations = {
            words: Array.isArray(payload?.words) ? payload.words : [],
            maxEntries: payload?.max_entries || pronunciations.maxEntries,
            maxLength: payload?.max_length || pronunciations.maxLength,
        };
    }

    async function refreshPronunciations() {
        try {
            takePronunciations(await fetchPronunciations());
            renderPronunciations();
        } catch (e) {
            setPronunciationNote(`Could not read the list: ${e.message || e}`, 'bad');
        }
    }

    function setPronunciationNote(text, tone) {
        const note = document.getElementById('words-note');
        if (!note) return;
        note.textContent = text;
        note.dataset.tone = tone || '';
    }

    /* What is on screen right now, including the row somebody is still typing into. Read
       from the inputs rather than from `pronunciations.words`, because the point of saving
       is to store what they typed, not what was last stored. */
    function pronunciationsOnScreen() {
        return Array.from(document.querySelectorAll('#words-rows .words-row')).map((row) => ({
            from: row.querySelector('.words-from')?.value || '',
            to: row.querySelector('.words-to')?.value || '',
        }));
    }

    async function savePronunciations() {
        const onScreen = pronunciationsOnScreen();
        /* Only the rows that are finished. A row with one half filled is a row somebody is
           still typing -- tabbing from the word to the respelling would otherwise fire a
           save that gets refused, and flash "nothing was given for how to say Aether1" in
           the middle of them saying it. Rust still refuses a half-filled row, which is the
           right answer for the CLI and for anything else posting to the endpoint; it is just
           not an error to be halfway through a sentence. */
        const words = onScreen.filter((w) => w.from.trim() && w.to.trim());
        const unfinished = onScreen.filter((w) => (w.from.trim() ? 1 : 0) + (w.to.trim() ? 1 : 0) === 1).length;
        // Nothing finished and nothing stored: there is no list yet to write over.
        if (!words.length && !pronunciations.words.length) {
            setPronunciationNote(unfinished ? 'Fill in both halves and it saves itself.' : 'Nothing yet.', '');
            return;
        }
        try {
            takePronunciations(await storePronunciations(words));
            renderPronunciations();
            const total = pronunciations.words.length;
            const tail = unfinished ? ' One row still needs its other half.' : '';
            setPronunciationNote(
                total
                    ? `${total} saved. The next thing it says uses them.${tail}`
                    : `Nothing yet.${tail}`,
                total ? 'good' : '');
        } catch (e) {
            // The rows are left exactly as typed: a refused save should not take the ones
            // that were fine with it.
            setPronunciationNote(String(e.message || e), 'bad');
        }
    }

    /* Always one blank row at the bottom, the way a spreadsheet has one.

       It is not only for looks. Saving redraws from what Rust returned, and Rust drops the
       blank row -- so a blank row that existed only because somebody pressed Add was being
       erased by the save that the same click's blur had already started. The next thing they
       typed went into the row above, over the rule they had just written. Re-creating the
       blank row on every redraw makes that unlosable, and makes Add a convenience rather
       than the only way to reach an empty field. */
    function renderPronunciations() {
        const holder = document.getElementById('words-rows');
        if (!holder) return;
        holder.innerHTML = '';
        for (const word of pronunciations.words) holder.appendChild(pronunciationRow(word));
        holder.appendChild(pronunciationRow({ from: '', to: '' }));
        const total = pronunciations.words.length;
        setPronunciationNote(total ? `${total} saved.` : 'Nothing yet.', '');
    }

    function pronunciationRow(word) {
        const row = document.createElement('div');
        row.className = 'words-row';

        const from = document.createElement('input');
        from.type = 'text';
        from.className = 'words-from';
        from.placeholder = 'the word';
        from.maxLength = pronunciations.maxLength;
        from.value = word.from || '';
        from.setAttribute('aria-label', 'The word as it is written');

        const arrow = document.createElement('span');
        arrow.className = 'words-arrow';
        arrow.textContent = 'is said';

        const to = document.createElement('input');
        to.type = 'text';
        to.className = 'words-to';
        to.placeholder = 'how it sounds';
        to.maxLength = pronunciations.maxLength;
        to.value = word.to || '';
        to.setAttribute('aria-label', 'How the word should sound');

        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'words-remove';
        remove.title = 'Remove this one';
        remove.setAttribute('aria-label', `Remove ${word.from || 'this row'}`);
        remove.textContent = '✕';
        remove.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            row.remove();
            savePronunciations();
        });

        // Saved on leaving a field rather than on every keystroke: a rule is only a rule
        // once the whole word is in it, and Enter for people who never leave the keyboard.
        for (const field of [from, to]) {
            field.addEventListener('blur', () => savePronunciations());
            field.addEventListener('keydown', (event) => {
                if (event.key === 'Enter') { event.preventDefault(); field.blur(); }
            });
        }

        row.append(from, arrow, to, remove);
        return row;
    }

    function addPronunciationRow() {
        voiceEngine.playSFX('click');
        const holder = document.getElementById('words-rows');
        if (!holder) return;
        // There is already a blank row at the bottom; if it is untouched, Add means "put me
        // in it" rather than "give me another one nobody asked for".
        const rows = Array.from(holder.querySelectorAll('.words-row'));
        const last = rows[rows.length - 1];
        const blank = last
            && !last.querySelector('.words-from')?.value.trim()
            && !last.querySelector('.words-to')?.value.trim();
        if (blank) {
            last.querySelector('.words-from')?.focus();
            return;
        }
        if (rows.length >= pronunciations.maxEntries) {
            setPronunciationNote(`That is as many as it holds (${pronunciations.maxEntries}).`, 'bad');
            return;
        }
        const row = pronunciationRow({ from: '', to: '' });
        holder.appendChild(row);
        row.querySelector('.words-from')?.focus();
    }

    /* Speaks the list back, which is the only check that counts: the rest of this pane can
       only show that the text was stored, and the question is what it sounds like. Saves
       first, so what is heard is what is stored rather than what was stored a minute ago. */
    async function hearPronunciations() {
        voiceEngine.playSFX('click');
        await savePronunciations();
        if (!pronunciations.words.length) {
            setPronunciationNote('Add a word first, then this will read it back.', 'bad');
            return;
        }
        // The words as written, in a sentence, so what comes out of the speaker is the
        // substitution happening rather than a recital of the replacements.
        const sentence = `${pronunciations.words.map((w) => w.from).join(', ')}.`;
        setPronunciationNote('Speaking...', '');
        try {
            const url = await synthesizeSpeechUrl(sentence, null);
            if (!url) return; // synthesizeSpeechUrl has already shown its own card
            const result = await voiceEngine.playTTSAudio(url);
            setPronunciationNote(
                result && result.played
                    ? 'That is how it will say them. Change a spelling and press this again.'
                    : `It could not play that: ${result?.error || 'no reason given'}`,
                result && result.played ? 'good' : 'bad');
        } catch (e) {
            setPronunciationNote(String(e.message || e), 'bad');
        }
    }

    async function refreshSoundHub(options = {}) {
        if (!document.getElementById('vhub-list') || soundHub.loading) return;
        soundHub.loading = true;
        try {
            const [catalogue, devices, browser] = await Promise.all([
                fetchVoiceCatalogue().catch(() => null),
                fetchAudioDevices().catch(() => null),
                enumerateBrowserDevices(),
            ]);
            soundHub.voices = (catalogue && catalogue.voices) || [];
            if (devices && devices.devices) soundHub.system = devices.devices;
            soundHub.browser = browser;
            if (!soundHub.selected && soundHub.voices.length) {
                soundHub.selected = (soundHub.voices.find(v => v.installed) || soundHub.voices[0]).name;
            }
            soundHub.loaded = true;
        } finally {
            soundHub.loading = false;
        }
        renderSoundChips();
        renderSoundHubList();
        renderSoundHubDetail();
        renderSoundHubDevices();
        applyAudioDevices();
        if (!options.quiet) refreshVoiceDownloads().catch(() => null);
    }

    /* Points the audio engine at the chosen devices. Called after every load, save and
       device refresh, so the running window follows the setting without a restart. */
    function applyAudioDevices() {
        const out = resolveAudioDevice(soundHub.browser.outputs,
            soundHub.chosen.output, soundHub.chosen.outputLabel);
        const mic = resolveAudioDevice(soundHub.browser.inputs,
            soundHub.chosen.input, soundHub.chosen.inputLabel);
        voiceEngine.setOutputDevice(out);
        voiceEngine.setInputDevice(mic);
    }

    /* Reads the two selects into soundHub.chosen. Called from saveSettings, which is what
       decides what is stored -- this keeps the label beside the id. */
    function readAudioDeviceChoice() {
        const read = (id) => {
            const select = document.getElementById(id);
            if (!select) return { value: '', label: '' };
            return {
                value: select.value || '',
                label: select.selectedOptions[0]?.dataset.label || '',
            };
        };
        const output = read('setting-audio-output');
        const input = read('setting-audio-input');
        soundHub.chosen = {
            output: output.value,
            outputLabel: output.label,
            input: input.value,
            inputLabel: input.label,
        };
        return soundHub.chosen;
    }

    /* Out loud, on the device that is selected right now -- not on the one that was saved.
       Choosing a speaker and then finding out at the next answer whether it was the right
       one is the failure this whole tab exists to end. */
    async function testOutputDevice() {
        voiceEngine.playSFX('click');
        const select = document.getElementById('setting-audio-output');
        const chosen = select ? select.value : '';
        const label = select?.selectedOptions[0]?.dataset.label || 'the system default';
        const resolved = resolveAudioDevice(soundHub.browser.outputs, chosen,
            select?.selectedOptions[0]?.dataset.label || '');

        if (!chosen) {
            await voiceEngine.setOutputDevice('');
            voiceEngine.playSFX('incoming');
            setSoundHubStatus('Played a sound on whatever this computer picks.', 'good');
        } else if (!resolved) {
            // The device is real -- the system named it -- but this window cannot address
            // it, because the browser has not offered a matching one. Saying "played on
            // the headset" here would be a lie, and the exact lie somebody would then
            // spend an evening chasing.
            await voiceEngine.setOutputDevice('');
            voiceEngine.playSFX('incoming');
            setSoundHubStatus(
                `Played a sound, but on the system default: this window cannot address ` +
                `${label} itself. The choice is still saved and still used by \`aether1 say\`.`,
                'bad');
        } else {
            const moved = await voiceEngine.setOutputDevice(resolved);
            voiceEngine.playSFX('incoming');
            setSoundHubStatus(moved
                ? `Played a sound on ${label}. Press Save Changes to keep it.`
                : `Played a sound, but it went to the system default — this window could not ` +
                  `move it to ${label}. The choice is still saved and still used by ` +
                  '`aether1 say`.',
                moved ? 'good' : 'bad');
        }
        // Back to what is actually saved, so a test does not leave the window speaking
        // somewhere nobody chose.
        applyAudioDevices();
    }

    /* A live level bar for the selected microphone. Six seconds, then it stops the stream
       itself -- a settings pane must not leave a recording light on. */
    let levelStop = null;
    async function testInputDevice() {
        voiceEngine.playSFX('click');
        const bar = document.getElementById('vhub-level');
        const fill = document.getElementById('vhub-level-fill');
        if (levelStop) { levelStop(); levelStop = null; }
        const select = document.getElementById('setting-audio-input');
        const resolved = resolveAudioDevice(soundHub.browser.inputs, select?.value || '',
            select?.selectedOptions[0]?.dataset.label || '');

        let stream;
        try {
            stream = await navigator.mediaDevices.getUserMedia({
                audio: resolved ? { deviceId: { exact: resolved } } : true,
            });
        } catch (e) {
            setSoundHubStatus(
                `That microphone could not be opened: ${e.message || e}. In the desktop window ` +
                'the microphone is still blocked; it works in a browser.', 'bad');
            return;
        }

        // Permission granted means the labels exist now, so the list is worth re-reading.
        soundHub.browser = await enumerateBrowserDevices();
        renderSoundHubDevices();

        const AudioContextCtor = window.AudioContext || window.webkitAudioContext;
        const ctx = new AudioContextCtor();
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 512;
        ctx.createMediaStreamSource(stream).connect(analyser);
        const data = new Uint8Array(analyser.frequencyBinCount);
        bar?.classList.remove('hidden');
        setSoundHubStatus('Say something — the bar should move.', 'busy');

        let peak = 0;
        let frame = null;
        const tick = () => {
            analyser.getByteTimeDomainData(data);
            let max = 0;
            for (const v of data) max = Math.max(max, Math.abs(v - 128));
            peak = Math.max(peak, max);
            if (fill) fill.style.width = `${Math.min(100, Math.round((max / 64) * 100))}%`;
            frame = requestAnimationFrame(tick);
        };
        tick();

        levelStop = () => {
            if (frame) cancelAnimationFrame(frame);
            stream.getTracks().forEach(track => track.stop());
            ctx.close().catch(() => null);
            if (fill) fill.style.width = '0%';
            bar?.classList.add('hidden');
            levelStop = null;
            setSoundHubStatus(peak > 3
                ? 'That microphone is picking sound up. Press Save Changes to keep it.'
                : 'Nothing came through on that one — it may be muted, or the wrong device.',
                peak > 3 ? 'good' : 'bad');
        };
        setTimeout(() => { if (levelStop) levelStop(); }, 6000);
    }

    function initSoundHub() {
        const search = document.getElementById('vhub-search');
        if (!search) return;
        search.addEventListener('input', () => {
            soundHub.search = search.value;
            renderSoundHubList();
        });
        for (const tab of document.querySelectorAll('[data-vhub-tab]')) {
            tab.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                soundHub.tab = tab.dataset.vhubTab;
                document.querySelectorAll('[data-vhub-tab]').forEach(other => {
                    other.classList.toggle('is-active', other === tab);
                });
                document.getElementById('vhub-pane-voices')
                    ?.classList.toggle('hidden', soundHub.tab !== 'voices');
                document.getElementById('vhub-pane-devices')
                    ?.classList.toggle('hidden', soundHub.tab !== 'devices');
                document.getElementById('vhub-pane-words')
                    ?.classList.toggle('hidden', soundHub.tab !== 'words');
                if (soundHub.tab === 'words') refreshPronunciations();
                // The search box only means anything on one of the two tabs, and it is
                // the wrapper that goes -- hiding the input alone leaves its magnifying
                // glass sitting on the row with nothing to type into.
                search.closest('.hub-search')?.classList.toggle('hidden', soundHub.tab !== 'voices');
                document.getElementById('vhub-filter')
                    ?.classList.toggle('hidden', soundHub.tab !== 'voices');
            });
        }
        document.getElementById('vhub-filter')?.addEventListener('change', (event) => {
            soundHub.filter = event.target.value;
            renderSoundHubList();
        });
        document.getElementById('vhub-refresh')?.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            refreshSoundHub();
        });
        document.getElementById('vhub-test-output')?.addEventListener('click', testOutputDevice);
        document.getElementById('vhub-test-input')?.addEventListener('click', testInputDevice);
        document.getElementById('btn-words-add')?.addEventListener('click', addPronunciationRow);
        document.getElementById('btn-words-test')?.addEventListener('click', hearPronunciations);
        // Choosing a device points the window at it straight away, so the next thing the
        // companion says comes out of it. Save Changes is what makes it survive a restart.
        for (const id of ['setting-audio-output', 'setting-audio-input']) {
            document.getElementById(id)?.addEventListener('change', () => {
                readAudioDeviceChoice();
                applyAudioDevices();
                renderSoundChips();
            });
        }
        // A device plugged in while the window is open changes the list under it.
        navigator.mediaDevices?.addEventListener?.('devicechange', () => {
            refreshSoundHub({ quiet: true }).catch(() => null);
        });
    }

    initSoundHub();

    // Read when the section is opened, and read again on each visit: a headset plugged in
    // while AETHER1 was running is the whole reason somebody opens this list.
    document.addEventListener('aether-settings-section', event => {
        if (event.detail === 'voice') refreshSoundHub().catch(() => null);
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


    /* --- The model hub ---------------------------------------------------------------
       One list of models with one detail panel beside it, in place of the two separate
       lists The Brain used to carry (models to talk to, in the wizard; models to write
       code with, further down the panel). They were always the same kind of decision
       made against the same machine, and splitting them meant neither list could say
       "this one does both" -- which is the answer that saves a second download.

       Nothing here is fetched from a model index on the web. Both catalogues are compiled
       into the binary and the rest is this computer: the scan's endpoint, that server's
       own list of what it has, and the machine's memory and video memory. A hub that
       listed a public index would mostly be listing models this machine cannot run, and
       it would go blank the moment the network did. */

    const hub = {
        tab: 'discover',
        search: '',
        purpose: 'all',
        fit: 'fits',
        scale: 'all',
        sort: 'recommended',
        // The model name the detail panel is showing. Kept across a refresh, so a probe
        // that lands while somebody is reading does not throw them back to the top.
        selected: null,
        models: [],
        installed: [],
        localModels: [],
        endpoint: '',
        provider: '',
        canInstall: false,
        sizedAgainst: '',
        loading: false,
        loaded: false,
        pollTimer: null,
    };

    // The last telemetry reading, kept so the hub's chips can say what the machine has
    // without probing for it: this window is already receiving it every tick.
    let lastTelemetry = null;

    /* Does this server already have that model?
       Mirrors code_setup::has_model, and for the same reason: a tag names a size, so
       `qwen2.5:7b` is only satisfied by `qwen2.5:7b`, while a bare `mistral` is satisfied
       by whatever tag of mistral is there. Matching on the family in both directions would
       mark the 14b installed when only the 7b is. */
    function hubHasModel(installed, name) {
        const wanted = name.toLowerCase();
        return installed.some(have => {
            const got = have.toLowerCase();
            if (got === wanted) return true;
            if (wanted.includes(':')) return got === wanted;
            return got.split(':')[0] === wanted;
        });
    }

    function hubGb(value) {
        if (typeof value !== 'number' || value <= 0) return null;
        return Number.isInteger(value) ? `${value} GB` : `${value.toFixed(1)} GB`;
    }

    /* The two catalogues as one list. A model that appears in both -- a coding model big
       enough to be the companion as well -- is one entry that says so, not two rows with
       the same name and different buttons. */
    function hubMergeCatalogues(setup, code) {
        const installed = (setup && setup.installed_models) || [];
        const rows = [];
        const byName = new Map();

        const add = (entry) => {
            const existing = byName.get(entry.name);
            if (existing) {
                existing.purposes = Array.from(new Set(existing.purposes.concat(entry.purposes)));
                existing.recommended = existing.recommended || entry.recommended;
                existing.runs_aether1 = existing.runs_aether1 || entry.runs_aether1;
                return;
            }
            byName.set(entry.name, entry);
            rows.push(entry);
        };

        for (const model of (setup && setup.models) || []) {
            add({
                name: model.name,
                label: model.label,
                blurb: model.blurb,
                download: model.download,
                needs_gb: model.needs_gb,
                needs_vram_gb: model.needs_vram_gb,
                fits: !!model.fits,
                fits_on_gpu: !!model.fits_on_gpu,
                recommended: !!model.recommended,
                installed: hubHasModel(installed, model.name),
                purposes: ['chat'],
                // Everything in the chat catalogue is by definition something Aether1 can
                // run on; the flag only means something on the coding list, where a model
                // can be too small to hold the written tool protocol.
                runs_aether1: true,
            });
        }

        for (const model of (code && code.models) || []) {
            add({
                name: model.name,
                label: model.label,
                blurb: model.blurb,
                download: model.download,
                needs_gb: model.needs_gb,
                needs_vram_gb: null,
                fits: !!model.fits,
                fits_on_gpu: !!model.fits_on_gpu,
                recommended: false,
                installed: !!model.installed,
                purposes: ['code'],
                runs_aether1: !!model.runs_aether1,
                // The coding list's own recommendation, which is a separate decision from
                // the chat one -- they are different jobs and frequently different models.
                best_for_code: !!model.recommended,
            });
        }

        return rows;
    }

    /* What is actually on the machine. Anything the server reports that neither catalogue
       knows about is still listed, as itself: somebody who pulled a model by hand has it
       installed, and a hub that only admits to models it recommended is lying about the
       computer. */
    function hubDeviceRows() {
        const known = new Map(hub.models.map(model => [model.name, model]));
        return hub.installed.map(name => known.get(name) || {
            name,
            label: name,
            blurb: 'Downloaded on this computer, and not one of the models Aether1 suggests '
                + '-- so there is nothing here about what it needs or what it is good at.',
            download: '',
            needs_gb: 0,
            fits: true,
            fits_on_gpu: false,
            recommended: false,
            installed: true,
            purposes: [],
            runs_aether1: false,
            unknown: true,
        });
    }

    function hubVisibleRows() {
        let rows = hub.tab === 'device' ? hubDeviceRows() : hub.models.slice();

        const term = hub.search.trim().toLowerCase();
        if (term) {
            rows = rows.filter(model =>
                model.name.toLowerCase().includes(term)
                || (model.label || '').toLowerCase().includes(term)
                || (model.blurb || '').toLowerCase().includes(term));
        }
        if (hub.purpose !== 'all') {
            rows = rows.filter(model => (model.purposes || []).includes(hub.purpose));
        }
        if (hub.scale !== 'all') {
            rows = rows.filter(model => {
                const local = hub.localModels.find(choice => choice.name === model.name);
                const metadata = local?.parameter_size || model.name;
                const billions = Number((String(metadata).match(/(\d+(?:\.\d+)?)\s*[bB]/) || [])[1] || 0);
                const quantized = !!local?.quantization || /(?:q\d(?:_k)?(?:_m|_s|_l)?|q\d+_\d+)/i.test(model.name);
                return quantized && (hub.scale === 'multi-quant' ? billions >= 7 : billions >= 30);
            });
        }
        // The fit filter never hides something already downloaded: it is on the disk
        // whatever a memory heuristic thinks of it, and hiding it is how you get a hub
        // that cannot show you the model you are running.
        if (hub.fit === 'fits' && hub.tab !== 'device') {
            const fitting = rows.filter(model => model.fits || model.installed);
            // A machine under the floor fits nothing, and the control that would bring the
            // rest back is a dropdown two rows up -- so an empty result shows everything
            // rather than an empty list with no way out of it.
            if (fitting.length) rows = fitting;
        }

        if (hub.sort === 'smallest') rows.sort((a, b) => a.needs_gb - b.needs_gb);
        else if (hub.sort === 'largest') rows.sort((a, b) => b.needs_gb - a.needs_gb);
        else {
            // Recommended: the pick for this machine first, then what is downloaded, then
            // the rest by size -- the order somebody deciding actually reads in.
            rows.sort((a, b) => {
                const score = (m) => (m.recommended ? 0 : 0) + (m.recommended ? -4 : 0)
                    + (m.best_for_code ? -3 : 0) + (m.installed ? -2 : 0) + (m.fits ? -1 : 0);
                return score(a) - score(b) || a.needs_gb - b.needs_gb;
            });
        }
        return rows;
    }

    /* The machine's own numbers, along the top. These are what every other answer on this
       panel is measured against, so they are stated rather than implied. */
    function renderHubChips() {
        const box = document.getElementById('hub-machine');
        if (!box) return;
        box.innerHTML = '';

        const chip = (value, label, state) => {
            const el = document.createElement('span');
            el.className = 'hub-chip';
            if (state) el.dataset.state = state;
            const v = document.createElement('span');
            v.className = 'hub-chip-value';
            v.textContent = value;
            const l = document.createElement('span');
            l.className = 'hub-chip-label';
            l.textContent = label;
            el.append(v, l);
            box.appendChild(el);
        };

        const connected = hub.provider && hub.provider !== 'offline';
        chip(connected ? 'Connected' : 'Not set up', connected ? '' : '', connected ? 'good' : 'warn');
        chip(String(hub.installed.length), 'LOCAL');

        // The card, when there is one with its own memory. An integrated one is left out
        // on purpose: its memory is the system memory already in the next chip, and two
        // chips adding up to more than the machine has is a lie about the hardware.
        const card = (lastTelemetry?.gpus || []).find(gpu => !gpu.integrated && gpu.vram_gb);
        if (card) chip(hubGb(card.vram_gb), 'VRAM');
        if (lastTelemetry?.ram?.total_gb) chip(hubGb(lastTelemetry.ram.total_gb), 'RAM');
        if (lastTelemetry?.cpu?.cores) chip(String(lastTelemetry.cpu.cores), 'CPU');
    }

    function hubRow(model) {
        const row = document.createElement('button');
        row.type = 'button';
        row.className = 'hub-row';
        row.setAttribute('role', 'option');
        if (model.name === hub.selected) row.classList.add('is-selected');
        if (!model.fits && !model.installed) row.classList.add('is-unfit');
        row.setAttribute('aria-selected', model.name === hub.selected ? 'true' : 'false');

        const glyph = document.createElement('span');
        glyph.className = 'hub-row-glyph';
        glyph.textContent = model.installed ? '◉' : '○';
        row.appendChild(glyph);

        const body = document.createElement('span');
        body.className = 'hub-row-body';

        const name = document.createElement('span');
        name.className = 'hub-row-name';
        name.textContent = model.label || model.name;
        body.appendChild(name);

        const meta = document.createElement('span');
        meta.className = 'hub-row-meta';
        const size = document.createElement('span');
        size.textContent = model.download || model.name;
        meta.appendChild(size);
        if (model.installed) {
            const here = document.createElement('span');
            here.style.color = 'var(--neon-green)';
            here.textContent = 'downloaded';
            meta.appendChild(here);
        } else if (model.recommended) {
            const pick = document.createElement('span');
            pick.style.color = 'var(--text-accent)';
            pick.textContent = 'best fit';
            meta.appendChild(pick);
        } else if (model.best_for_code) {
            const pick = document.createElement('span');
            pick.style.color = 'var(--text-accent)';
            pick.textContent = 'best for code';
            meta.appendChild(pick);
        }
        body.appendChild(meta);
        row.appendChild(body);

        row.addEventListener('click', () => {
            hub.selected = model.name;
            renderHubList();
            renderHubDetail();
        });
        return row;
    }

    function renderHubList() {
        const list = document.getElementById('hub-list');
        const title = document.getElementById('hub-list-title');
        if (!list) return;
        const rows = hubVisibleRows();

        if (title) {
            title.textContent = hub.tab === 'device'
                ? `${rows.length} downloaded on this computer`
                : `${rows.length} models for this computer`;
        }

        list.innerHTML = '';
        if (!rows.length) {
            const empty = document.createElement('div');
            empty.className = 'hub-empty';
            empty.textContent = hub.tab === 'device'
                ? 'Nothing is downloaded yet, or no model server answered on this computer. '
                  + 'Pick one under Discover and press Download.'
                : 'Nothing matches that search.';
            list.appendChild(empty);
            return;
        }
        // Keep a selection that is still on screen; otherwise take the top row, which the
        // sort has already made the best answer for this machine.
        if (!rows.some(model => model.name === hub.selected)) hub.selected = rows[0].name;
        for (const model of rows) list.appendChild(hubRow(model));
    }

    function hubFact(label, value) {
        const box = document.createElement('div');
        box.className = 'hub-fact';
        const l = document.createElement('div');
        l.className = 'hub-fact-label';
        l.textContent = label;
        const v = document.createElement('div');
        v.className = 'hub-fact-value';
        v.textContent = value;
        box.append(l, v);
        return box;
    }

    function hubTag(text, tone) {
        const tag = document.createElement('span');
        tag.className = 'hub-tag';
        if (tone) tag.dataset.tone = tone;
        tag.textContent = text;
        return tag;
    }

    function renderHubDetail() {
        const panel = document.getElementById('hub-detail');
        if (!panel) return;
        panel.innerHTML = '';

        const model = hubVisibleRows().find(entry => entry.name === hub.selected);
        if (!model) {
            const empty = document.createElement('div');
            empty.className = 'hub-empty';
            empty.textContent = hub.loading
                ? 'Looking at this computer...'
                : 'Pick a model on the left to see what it needs.';
            panel.appendChild(empty);
            return;
        }

        const name = document.createElement('div');
        name.className = 'hub-detail-name';
        name.textContent = model.label || model.name;
        panel.appendChild(name);

        const sub = document.createElement('div');
        sub.className = 'hub-detail-sub';
        sub.textContent = model.name;
        panel.appendChild(sub);

        const localInfo = hub.localModels.find(choice => choice.name === model.name);
        if (localInfo && (localInfo.parameter_size || localInfo.quantization)) {
            const specs = document.createElement('div');
            specs.className = 'hub-tags';
            if (localInfo.parameter_size) specs.appendChild(hubTag(`${localInfo.parameter_size} parameters`));
            if (localInfo.quantization) specs.appendChild(hubTag(localInfo.quantization));
            panel.appendChild(specs);
        }

        const tags = document.createElement('div');
        tags.className = 'hub-tags';
        if ((model.purposes || []).includes('chat')) tags.appendChild(hubTag('To talk to'));
        if ((model.purposes || []).includes('code')) tags.appendChild(hubTag('To write code'));
        if (model.recommended) tags.appendChild(hubTag('Best fit for this computer', 'pick'));
        if (model.best_for_code) tags.appendChild(hubTag('Best coding model here', 'pick'));
        if (model.installed) tags.appendChild(hubTag('✔ Downloaded', 'on'));
        if (!model.fits && !model.unknown) tags.appendChild(hubTag('More memory than this computer has', 'off'));
        if (tags.childElementCount) panel.appendChild(tags);

        // The row that spends somebody's bandwidth, with the size on it rather than in a
        // footnote: three gigabytes over a home connection is the whole decision.
        const action = document.createElement('div');
        action.className = 'hub-action';
        const size = document.createElement('span');
        size.className = 'hub-action-size';
        size.textContent = model.installed
            ? 'On this computer already'
            : (model.download ? `Download is ${model.download.replace(/^about /, '')}` : 'Not in either list');
        action.appendChild(size);

        const buttons = document.createElement('div');
        buttons.className = 'flex gap-2 flex-wrap';

        // "Use this one" fills the connection form below rather than saving by itself:
        // this window has one Save Changes button and a panel that saves behind it would
        // also commit whatever else is half-typed on another pane.
        if (model.installed && (model.unknown || (model.purposes || []).includes('chat') || model.runs_aether1)) {
            const use = document.createElement('button');
            use.type = 'button';
            use.className = 'cyber-btn cyber-btn-active text-xs py-1.5 px-3 whitespace-nowrap';
            use.textContent = '✔ Use this one';
            use.addEventListener('click', () => hubUseModel(model));
            buttons.appendChild(use);
        }

        if (model.installed && (model.purposes || []).includes('code')) {
            const code = document.createElement('button');
            code.type = 'button';
            code.className = 'cyber-btn text-xs py-1.5 px-3 whitespace-nowrap';
            code.textContent = 'Use for coding';
            code.addEventListener('click', async () => {
                code.disabled = true;
                try {
                    if (IS_TAURI) await tauriInvoke('code_set_model_preference_rust', { model: model.name });
                    else {
                        const response = await apiFetch('/api/code/model', {
                            method: 'POST', headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ model: model.name }),
                        });
                        if (!response.ok) throw new Error((await response.text()) || 'Could not set coding model.');
                    }
                    setHubStatus(`${model.name} is now the preferred AETHER CODE model.`, 'good');
                    const advice = await fetchCodeAdvice().catch(() => null);
                    if (advice) renderCodeAdvice(advice);
                } catch (error) {
                    setHubStatus(`⚠ ${error.message || error}`, 'bad');
                } finally { code.disabled = false; }
            });
            buttons.appendChild(code);
        }

        if (!model.installed) {
            const download = document.createElement('button');
            download.type = 'button';
            download.className = 'cyber-btn text-xs py-1.5 px-3 whitespace-nowrap';
            download.textContent = '⬇ Download';
            // Ollama's own port is what the in-app download drives. Where that is not the
            // server in play the button would lie, so it says what to do instead.
            download.disabled = !hub.canInstall;
            download.title = hub.canInstall
                ? ''
                : 'Downloading from here needs Ollama on this computer. Any other server '
                  + 'loads its models its own way.';
            download.addEventListener('click', () => hubDownload(model, download));
            buttons.appendChild(download);
        }
        action.appendChild(buttons);
        panel.appendChild(action);

        const blurb = document.createElement('div');
        blurb.className = 'hub-blurb';
        blurb.textContent = model.blurb;
        panel.appendChild(blurb);

        const facts = document.createElement('div');
        facts.className = 'hub-facts';
        if (model.needs_gb) facts.appendChild(hubFact('MEMORY IT WANTS', hubGb(model.needs_gb)));
        if (model.needs_vram_gb) facts.appendChild(hubFact('ON A CARD', hubGb(model.needs_vram_gb)));
        facts.appendChild(hubFact('GRAPHICS CARD', model.fits_on_gpu
            ? '⚡ Whole model fits'
            : 'Partly on the processor'));
        if (!model.unknown) {
            facts.appendChild(hubFact('RUNS AETHER1 ITSELF', model.runs_aether1 ? 'Yes' : 'Too small'));
        }
        if (facts.childElementCount) panel.appendChild(facts);

        // What the recommendation was measured against. Worth saying on exactly the
        // machines where the two rules disagree -- plenty of memory, a modest card --
        // where a small recommendation otherwise reads as a panel that failed to notice.
        if (hub.sizedAgainst) {
            const note = document.createElement('div');
            note.className = 'text-[10px] font-mono text-slate-500 leading-snug';
            note.textContent = hub.sizedAgainst;
            panel.appendChild(note);
        }
    }

    function setHubStatus(message, tone) {
        const box = document.getElementById('hub-status');
        if (!box) return;
        box.classList.remove('hidden');
        box.className = 'text-xs font-mono p-2.5 rounded border '
            + (tone === 'bad' ? 'border-red-500/40 bg-red-950/20 text-red-400'
                : tone === 'good' ? 'border-green-500/40 bg-green-950/20 text-green-400'
                : 'border-cyan-500/20 bg-slate-900/80 text-cyan-200');
        box.textContent = message;
    }

    /* Fills the connection form with this model and the server it is on. Deliberately
       stops short of saving -- see the comment on the button. */
    function hubUseModel(model) {
        voiceEngine.playSFX('click');
        const modelBox = document.getElementById('setting-model');
        const providerBox = document.getElementById('setting-provider');
        const endpointBox = document.getElementById('setting-endpoint');
        if (modelBox) modelBox.value = model.name;
        if (providerBox && hub.provider) providerBox.value = hub.provider;
        if (endpointBox && hub.endpoint) endpointBox.value = hub.endpoint;
        const picker = document.getElementById('setting-model-picker');
        if (picker && Array.from(picker.options).some(option => option.value === model.name)) {
            picker.value = model.name;
        }
        document.getElementById('settings-group-connection')?.setAttribute('open', 'open');
        setHubStatus(`${model.name} is filled in below. Press Save Changes to start thinking with it.`, 'good');
    }

    async function hubRefreshDownloads() {
        const box = document.getElementById('hub-downloads');
        if (!box) return [];
        const data = await fetchDownloadStatus().catch(() => null);
        const list = (data && data.downloads) || [];
        box.innerHTML = '';
        for (const download of list) box.appendChild(renderDownloadRow(download, null));

        const running = list.filter(d => d.phase !== 'done' && d.phase !== 'failed');
        if (!running.length && hub.pollTimer) {
            clearInterval(hub.pollTimer);
            hub.pollTimer = null;
            // Something landed, so what is installed has changed -- which is the one thing
            // on this panel a download can change.
            if (list.some(d => d.phase === 'done')) refreshModelHub({ quiet: true });
        }
        return list;
    }

    async function hubDownload(model, button) {
        voiceEngine.playSFX('click');
        button.disabled = true;
        setHubStatus(`Starting the download of ${model.name}...`, 'busy');
        try {
            const endpoint = hub.endpoint || '';
            const modelName = model.name;
            const data = IS_TAURI
                ? await tauriInvoke('start_download_rust', { modelName, endpoint })
                : await (await apiFetch(
                    `/api/setup/download?model_name=${encodeURIComponent(modelName)}&endpoint=${encodeURIComponent(endpoint)}`,
                    { method: 'POST' }
                )).json();
            if (!data.ok) {
                setHubStatus(`⚠ ${data.message}`, 'bad');
                button.disabled = false;
                return;
            }
            setHubStatus(`Downloading ${modelName}. You can leave this open — the bar below is live, `
                + 'and the download carries on either way.', 'busy');
            await hubRefreshDownloads();
            if (!hub.pollTimer) hub.pollTimer = setInterval(() => { hubRefreshDownloads(); }, 1000);
        } catch (e) {
            setHubStatus(`⚠ ${e.message || e}`, 'bad');
            button.disabled = false;
        }
    }

    /* One probe of each catalogue, drawn into the whole panel. Both are cheap and both
       scan the machine, so they go together rather than one per tab. */
    async function refreshModelHub(options = {}) {
        if (!document.getElementById('hub-list') || hub.loading) return;
        hub.loading = true;
        if (!options.quiet) renderHubDetail();
        try {
            const [setup, code, localModels] = await Promise.all([
                fetchSetupAdvice().catch(() => null),
                fetchCodeAdvice().catch(() => null),
                (IS_TAURI
                    ? tauriInvoke('code_local_models_rust')
                    : apiFetch('/api/code/models').then(response => response.ok ? response.json() : []))
                    .catch(() => []),
            ]);
            hub.models = hubMergeCatalogues(setup, code);
            hub.installed = (setup && setup.installed_models) || [];
            hub.localModels = localModels || [];
            hub.endpoint = (setup && setup.endpoint) || '';
            hub.provider = (setup && setup.provider) || '';
            hub.canInstall = !!(setup && setup.can_install_from_here);
            hub.sizedAgainst = (code && code.sized_against) || '';
            hub.loaded = true;
        } finally {
            hub.loading = false;
        }
        renderHubChips();
        renderHubList();
        renderHubDetail();
        hubRefreshDownloads();
    }

    function initModelHub() {
        const search = document.getElementById('hub-search');
        if (!search) return;
        search.addEventListener('input', () => {
            hub.search = search.value;
            renderHubList();
            renderHubDetail();
        });
        for (const tab of document.querySelectorAll('[data-hub-tab]')) {
            tab.addEventListener('click', () => {
                hub.tab = tab.dataset.hubTab;
                document.querySelectorAll('[data-hub-tab]').forEach(other => {
                    other.classList.toggle('is-active', other === tab);
                });
                hub.selected = null;
                renderHubList();
                renderHubDetail();
            });
        }
        const bind = (id, key) => {
            const el = document.getElementById(id);
            el?.addEventListener('change', () => {
                hub[key] = el.value;
                renderHubList();
                renderHubDetail();
            });
        };
        bind('hub-filter-purpose', 'purpose');
        bind('hub-filter-fit', 'fit');
        bind('hub-sort', 'sort');
        document.getElementById('hub-refresh')?.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            refreshModelHub();
        });
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

    /* Step 13's last mile. The crash watcher has been finding crashes and the tray has been
       going amber since it shipped, and the notification it raises says "AETHER1 has the
       details. Open it to look into this together" -- but nothing in the HUD listened for
       the event carrying those details, so opening it showed an ordinary chat window with
       nothing in it. The promise in that notification is what this keeps.

       It is a card rather than a message from the companion: no model has seen this yet, and
       a line in AETHER1's own voice saying something crashed would be the app putting words
       in its mouth. The card states the fact and offers the one thing worth doing next. */
    function appendCrashCard(payload) {
        const headline = String(payload.headline || 'Something stopped unexpectedly');
        const context = String(payload.context || '');
        const program = String((payload.crash && payload.crash.program) || '').trim();

        const card = document.createElement('div');
        card.className = 'p-3 rounded my-2 text-sm leading-relaxed self-start mr-8 '
            + 'border border-amber-500/40 bg-amber-950/20';

        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 '
            + 'border-b border-amber-500/20 text-xs font-mono text-amber-400/90';
        const what = document.createElement('span');
        what.textContent = '⚠ CRASH DETECTED';
        header.appendChild(what);
        const when = document.createElement('span');
        when.textContent = new Date().toLocaleTimeString();
        header.appendChild(when);
        card.appendChild(header);

        const line = document.createElement('p');
        line.className = 'text-amber-100/90';
        line.textContent = headline;
        card.appendChild(line);

        /* The log tail is behind a fold. It is the most useful thing here and the least
           readable -- a wall of frames pasted into the conversation is exactly the thing
           that made this unreadable in the first place. */
        if (context) {
            const fold = document.createElement('details');
            fold.className = 'mt-2';
            const summary = document.createElement('summary');
            summary.className = 'text-[11px] font-mono text-amber-400/70 cursor-pointer';
            summary.textContent = 'what it wrote before it stopped';
            fold.appendChild(summary);
            const pre = document.createElement('pre');
            pre.className = 'mt-1 text-[10px] font-mono text-slate-300 whitespace-pre-wrap '
                + 'max-h-48 overflow-y-auto';
            pre.textContent = context;
            fold.appendChild(pre);
            card.appendChild(fold);
        }

        const ask = document.createElement('button');
        ask.className = 'mt-2 text-xs font-mono text-amber-200 hover:text-amber-50 '
            + 'border border-amber-500/40 px-2 py-0.5 rounded bg-amber-950/40 cursor-pointer';
        ask.textContent = 'Ask ' + currentAgentName.toUpperCase() + ' about this';
        ask.onclick = () => {
            ask.disabled = true;
            // The question, not the evidence: recent_crashes and self_check are in every
            // persona's domain, so it fetches the crash itself and whatever else is wrong
            // with the install -- which is the difference between a diagnosis and a paste.
            handleSendMessage(program
                ? `${program} just crashed on this machine. Look into what happened and what I should do about it.`
                : 'Something just crashed on this machine. Look into what happened and what I should do about it.');
        };
        card.appendChild(ask);

        /* Beside it, the answer that needs no model. The card most often appears right
           after an update relaunch, on an install someone is now unsure about, and asking
           the companion is no use when no companion is connected. */
        const verify = document.createElement('button');
        verify.className = 'mt-2 ml-2 text-xs font-mono text-amber-200 hover:text-amber-50 '
            + 'border border-amber-500/40 px-2 py-0.5 rounded bg-amber-950/40 cursor-pointer';
        verify.textContent = 'Verify AETHER1';
        verify.onclick = () => {
            verify.disabled = true;
            verifyInstall();
        };
        card.appendChild(verify);

        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
    }

    if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
        window.__TAURI__.event.listen('crash-detected', (event) => {
            const payload = (event && event.payload) || {};
            if (!payload.headline) return;
            appendCrashCard(payload);
        });
    }

    // Step 48: another copy of AETHER1 is installed somewhere on this machine. The startup
    // scan in main.rs finds them; this is where the operator is asked, because nothing is
    // ever removed without being asked and an uninstall cannot be taken back.
    if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
        window.__TAURI__.event.listen('old-installs-detected', async (event) => {
            const payload = (event && event.payload) || {};
            const installs = Array.isArray(payload.installs) ? payload.installs : [];
            if (!installs.length) return;
            const list = installs.map((install) => `  - ${install.description}`).join('\n');
            const question = `${payload.headline}\n\n${list}\n\n`
                + 'Remove them? Your vault, conversations and settings stay exactly where '
                + 'they are -- only the program files go.';
            if (!confirm(question)) {
                // "No" is an answer worth keeping. Without this the same prompt comes back
                // on every single launch, which is how a useful notice becomes noise.
                tauriInvoke('keep_other_installs_rust').catch(() => {});
                return;
            }
            const results = [];
            for (const install of installs) {
                try {
                    results.push(await tauriInvoke('remove_install_rust', { id: install.id }));
                } catch (error) {
                    results.push(`${install.path}: ${error}`);
                }
            }
            alert(results.join('\n\n'));
        });
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
                // Bytes over IPC, played from a blob, rather than convertFileSrc.
                // An asset:// URL is a different origin from this page, and the clip is
                // routed through a MediaElementAudioSourceNode for the analyser -- WebKit
                // mutes that node for anything that would taint the page's origin, so the
                // asset route plays silence with no error anywhere. See speech_clip_rust.
                const clip = await tauriInvoke('speech_clip_rust', { text, voice: voiceName || null });
                const blob = new Blob([new Uint8Array(clip.bytes)], { type: clip.mime });
                return URL.createObjectURL(blob);
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

    /* The start-up check, kept so its result can be said once in the chat as well as drawn
       in the Settings panel nobody has open at launch. Null in a browser tab, where this
       whole feature is hidden -- a tab cannot update this installation. */
    let startupUpdateCheck = null;
    let startupUpdateAnnounced = false;

    /* "On start-up, if the platform is capable, check the update status." The check itself
       has always run from initVersionAndUpdates; what was missing was anybody being told.
       Said in the chat, after the history has loaded (loadChatHistory empties the container,
       so anything appended beside it is erased), and only once per launch.

       Quiet about the one case that is neither news nor a fault: an operator who declined to
       sign in is not asked again every launch -- the Settings panel keeps the offer. */
    async function announceStartupUpdateStatus() {
        if (!IS_TAURI || !startupUpdateCheck || startupUpdateAnnounced) return;
        startupUpdateAnnounced = true;
        const status = await startupUpdateCheck;
        if (!status) {
            appendMessage(currentAgentName, '⚠️ I could not check for updates at start-up. Settings → Updates has the detail.');
            return;
        }
        if (!status.checked) {
            if (status.declined) return;
            const why = status.error || 'the check could not be made.';
            const how = status.needs_sign_in && status.sign_in_available
                ? ' Settings → The Brain → Connections can sign in for it.'
                : ' Settings → Updates has the detail.';
            appendMessage(currentAgentName, `⚠️ Update check: ${why}${how}`);
            return;
        }
        if (status.up_to_date) {
            const against = status.latest_tag ? ` with ${status.latest_tag}` : '';
            appendMessage(currentAgentName, `✔ Up to date${against} (${status.version}).`);
            return;
        }
        const latest = status.latest_tag
            || (status.latest_commit ? `build ${status.latest_commit.slice(0, 7)}` : 'a newer build');
        appendMessage(currentAgentName, `⬆ ${latest} is available; you are running ${status.version}. Settings → Updates can fetch it.`);
    }

    // The polling loop for a sign-in in progress, so a second click cannot start a second one
    // and closing the panel does not leave one running forever.
    let githubSignInPoll = null;

    function stopSignInPoll() {
        if (githubSignInPoll) {
            clearInterval(githubSignInPoll);
            githubSignInPoll = null;
        }
    }

    // Every button in this section is hidden before each render, so a state left over from the
    // previous check cannot offer something the current one does not support -- the shape of bug
    // that leaves a Download button on a copy with nothing to download.
    function resetUpdateButtons() {
        [btnApplyUpdate, btnDownloadUpdate, btnGithubSignIn, btnGithubDecline]
            .forEach((btn) => btn && btn.classList.add('hidden'));
    }

    function updateHeaderAction(text, action = 'check') {
        if (!headerUpdateButton) return;
        headerUpdateButton.textContent = text;
        headerUpdateButton.dataset.action = action;
        headerUpdateButton.title = action === 'check'
            ? 'Check for Aether1 updates'
            : action === 'download' ? 'Download the signed Aether1 update' : 'Apply the available Aether1 update';
    }

    function updateHeaderProgress(label, percent = null, indeterminate = false) {
        if (!headerUpdateProgress || !headerUpdateProgressText || !headerUpdateProgressBar) return;
        headerUpdateProgress.classList.remove('hidden');
        headerUpdateProgressText.textContent = label;
        headerUpdateProgressBar.classList.toggle('animate-pulse', indeterminate);
        headerUpdateProgressBar.style.width = indeterminate ? '35%' : `${Math.max(0, Math.min(100, percent || 0))}%`;
    }

    /* Returns the status it drew, or null if the check could not be made at all -- the
       start-up announcement (announceStartupUpdateStatus) reads the same result rather than
       asking GitHub a second time for it. */
    async function handleCheckForUpdate() {
        if (versionBadge) versionBadge.classList.add('animate-pulse');
        updateHeaderProgress('Checking for updates…', 15, true);
        if (updateStatusBox) {
            updateStatusBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Checking GitHub...</div>';
        }
        resetUpdateButtons();

        try {
            const status = await tauriInvoke('check_for_update_rust');
            if (settingsVersionLabel) settingsVersionLabel.textContent = `${status.version} (${status.built_commit_short})`;
            // Which of the two update mechanisms this copy is on, said rather than left to be
            // inferred from which buttons appeared.
            const modeNote = document.getElementById('update-mode-note');
            if (modeNote) {
                modeNote.textContent = status.mode === 'Checkout'
                    ? 'This is a git checkout: its update is a pull and a rebuild.'
                    : 'Installed copy: its update is a signed bundle from the project’s releases.';
            }

            if (!status.checked) {
                updateHeaderAction('↻ Retry', 'check');
                updateHeaderProgress('Update check needs attention', 0, false);
                if (updateStatusBox) {
                    // Declining is not a failure, so it is not drawn as one.
                    updateStatusBox.innerHTML = status.declined
                        ? `<div class="text-slate-400">${status.error || 'Updates are not being checked for.'}</div>`
                        : `<div class="text-yellow-400">⚠ ${status.error || 'Could not check for updates.'}</div>`;
                }
                if (versionBadge) versionBadge.classList.add('border-yellow-500/50', 'text-yellow-400');
                if (status.needs_sign_in && status.sign_in_available) {
                    if (btnGithubSignIn) btnGithubSignIn.classList.remove('hidden');
                    if (btnGithubDecline) btnGithubDecline.classList.remove('hidden');
                }
                // Somebody who declined and came back to this panel gets the offer again -- the
                // answer is remembered so nothing nags, not so it can never be changed.
                if (status.declined && status.sign_in_available && btnGithubSignIn) {
                    btnGithubSignIn.classList.remove('hidden');
                }
                return status;
            }

            if (status.up_to_date) {
                updateHeaderAction('↻ Check', 'check');
                updateHeaderProgress(`Up to date · ${status.version}`, 100, false);
                const against = status.latest_tag
                    ? `up to date with ${status.latest_tag}`
                    : `up to date (build ${status.built_commit_short})`;
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-green-400">✔ ${status.version} -- ${against}</div>`;
                }
                if (versionBadge) {
                    versionBadge.classList.remove('border-yellow-500/50', 'text-yellow-400');
                    versionBadge.classList.add('border-green-500/50', 'text-green-400');
                }
            } else if (status.latest_tag) {
                updateHeaderAction(status.asset_name && status.asset_signed ? '⬇ Update' : '↻ Check', status.asset_name && status.asset_signed ? 'download' : 'check');
                updateHeaderProgress(`Update available · ${status.latest_tag}`, 0, false);
                const size = status.asset_size
                    ? ` (${(status.asset_size / 1e9).toFixed(1)} GB)`
                    : '';
                const lines = [`<div class="text-yellow-400">⬆ ${status.latest_tag} is out; you are running ${status.version}.</div>`];
                if (status.release_url) {
                    lines.push(`<div class="text-[11px]"><a href="${status.release_url}" target="_blank" rel="noreferrer" class="underline text-cyan-300">What changed</a></div>`);
                }
                if (!status.asset_name) {
                    lines.push('<div class="text-[11px] text-slate-500">That release has no bundle for this platform attached to it.</div>');
                } else if (!status.asset_signed) {
                    // Refused up front rather than at the end of a half-gigabyte download: a
                    // bundle nothing can check is not offered at all.
                    lines.push('<div class="text-[11px] text-yellow-400">It has no signature attached, so there is no way to tell it is the one the project built. Not offering it.</div>');
                } else {
                    lines.push(`<div class="text-[11px] text-slate-500">Downloading ${status.asset_name}${size} checks its signature and stops there. Installing it is your own last step.</div>`);
                    if (btnDownloadUpdate) btnDownloadUpdate.classList.remove('hidden');
                }
                if (updateStatusBox) updateStatusBox.innerHTML = lines.join('');
                if (versionBadge) {
                    versionBadge.classList.remove('border-green-500/50', 'text-green-400');
                    versionBadge.classList.add('border-yellow-500/50', 'text-yellow-400');
                }
            } else {
                updateHeaderAction('⬆ Update', 'apply');
                updateHeaderProgress('Project update available', 0, false);
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
            return status;
        } catch (e) {
            updateHeaderAction('↻ Retry', 'check');
            updateHeaderProgress('Update check failed', 0, false);
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">Update check failed: ${e.message || e}</div>`;
            }
            return null;
        } finally {
            if (versionBadge) versionBadge.classList.remove('animate-pulse');
        }
    }

    // The device flow, from this side: ask for a code, show it, and poll. The polling happens
    // here rather than in a Rust call that blocks for the fifteen minutes a code stays valid --
    // a window that cannot be closed while somebody is deciding is worse than a timer.
    //
    // Drawn in The Brain -> Connections, which owns the sign-in. The update panel points at it
    // rather than keeping a second set of the same buttons: two places that can each say whether
    // you are signed in is two places that can disagree.
    async function handleSignIn() {
        voiceEngine.playSFX('click');
        stopSignInPoll();
        const box = document.getElementById('conn-github-code-box');
        const codeEl = document.getElementById('conn-github-code');
        const linkEl = document.getElementById('conn-github-link');
        const hintEl = document.getElementById('conn-github-hint');
        const btn = document.getElementById('conn-github-signin');
        if (btn) btn.disabled = true;

        try {
            const code = await tauriInvoke('github_sign_in_start_rust');
            if (codeEl) codeEl.textContent = code.user_code;
            if (linkEl) {
                linkEl.href = code.verification_uri;
                linkEl.textContent = code.verification_uri.replace(/^https?:\/\//, '');
            }
            if (hintEl) hintEl.textContent = 'Waiting for you to approve it. This window can stay open.';
            if (box) box.classList.remove('hidden');

            const deadline = Date.now() + code.expires_in * 1000;
            githubSignInPoll = setInterval(async () => {
                if (Date.now() > deadline) {
                    stopSignInPoll();
                    if (hintEl) hintEl.textContent = 'That code expired. Press Sign in again for a new one.';
                    if (btn) btn.disabled = false;
                    return;
                }
                try {
                    const result = await tauriInvoke('github_sign_in_poll_rust', { deviceCode: code.device_code });
                    if (!result.signed_in) return;
                    stopSignInPoll();
                    if (box) box.classList.add('hidden');
                    if (btn) btn.disabled = false;
                    renderConnections();
                    // Straight into the check that was blocked on this, rather than making the
                    // operator press the button they already pressed.
                    handleCheckForUpdate();
                } catch (e) {
                    stopSignInPoll();
                    if (hintEl) hintEl.textContent = `${e.message || e}`;
                    if (btn) btn.disabled = false;
                }
            }, Math.max(code.interval, 1) * 1000);
        } catch (e) {
            if (box) box.classList.add('hidden');
            if (hintEl) hintEl.textContent = `${e.message || e}`;
            if (btn) btn.disabled = false;
        }
    }

    // Both rows of The Brain -> Connections, from one probe. They answer two questions that are
    // easy to conflate: which GitHub account this copy is signed in to (so it can read releases),
    // and whether the `gh` program is on this machine (which is what AETHER CODE runs commands
    // through). Signing in does not install gh, and gh is not needed for updates.
    async function renderConnections() {
        if (!IS_TAURI) return;
        const stateEl = document.getElementById('conn-github-state');
        const ghEl = document.getElementById('conn-gh-state');
        const signIn = document.getElementById('conn-github-signin');
        const signOut = document.getElementById('conn-github-signout');
        try {
            const conn = await tauriInvoke('connections_rust');
            const gh = conn.github || {};
            if (stateEl) {
                if (gh.signed_in) {
                    stateEl.textContent = gh.storage === 'File'
                        ? 'Signed in. This machine has no keychain service running, so the sign-in is kept in a file only you can read.'
                        : 'Signed in. The sign-in is in this machine’s keychain.';
                    stateEl.className = 'text-[11px] font-mono text-green-400 leading-snug';
                } else if (!gh.sign_in_available) {
                    // A build made without the GitHub App's client ID. Says so rather than
                    // offering a button that cannot work.
                    stateEl.textContent = 'This build has no GitHub sign-in compiled into it, so there is nothing to sign in to.';
                    stateEl.className = 'text-[11px] font-mono text-slate-500 leading-snug';
                } else {
                    stateEl.textContent = 'Not signed in.';
                    stateEl.className = 'text-[11px] font-mono text-slate-400 leading-snug';
                }
            }
            if (signIn) signIn.classList.toggle('hidden', !gh.sign_in_available || gh.signed_in);
            if (signOut) signOut.classList.toggle('hidden', !gh.signed_in);

            const cli = conn.gh_cli || {};
            if (ghEl) {
                ghEl.textContent = cli.installed
                    ? `Installed at ${cli.path}.`
                    : 'Not on this computer. AETHER CODE will say so instead of running one; install it from cli.github.com if you want it.';
                ghEl.className = cli.installed
                    ? 'text-[11px] font-mono text-green-400 leading-snug'
                    : 'text-[11px] font-mono text-slate-400 leading-snug';
            }
        } catch (e) {
            if (stateEl) stateEl.textContent = `Could not check: ${e.message || e}`;
        }
    }

    // The update panel's Sign in button opens the one place that owns the sign-in, rather than
    // running a second copy of the flow.
    function revealConnections() {
        voiceEngine.playSFX('click');
        const group = document.getElementById('settings-group-connections');
        showSettingsSection('connections');
        if (group) {
            group.open = true;
            group.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
        renderConnections();
    }

    async function handleDeclineUpdates() {
        voiceEngine.playSFX('click');
        stopSignInPoll();
        document.getElementById('conn-github-code-box')?.classList.add('hidden');
        try {
            await tauriInvoke('github_decline_updates_rust', { declined: true });
        } catch (e) {
            console.warn('[AETHER1] Could not remember that answer:', e);
        }
        handleCheckForUpdate();
    }

    async function handleSignOut() {
        voiceEngine.playSFX('click');
        stopSignInPoll();
        try {
            await tauriInvoke('github_sign_out_rust');
        } catch (e) {
            console.warn('[AETHER1] Sign out failed:', e);
        }
        renderConnections();
        handleCheckForUpdate();
    }

    // Downloads and verifies; installs nothing. The progress bar is the point of doing it here
    // rather than telling somebody to run a command: half a gigabyte with no visible progress is
    // indistinguishable from a hang.
    async function handleDownloadUpdate() {
        voiceEngine.playSFX('click');
        if (btnDownloadUpdate) btnDownloadUpdate.disabled = true;
        if (btnCheckUpdate) btnCheckUpdate.disabled = true;
        const render = (text) => {
            if (updateStatusBox) updateStatusBox.innerHTML = `<div class="text-cyan-300">${text}</div>`;
        };
        render('Starting the download...');

        let unlisten = null;
        try {
            if (window.__TAURI__?.event?.listen) {
                unlisten = await window.__TAURI__.event.listen('update-download-progress', (event) => {
                    const { done = 0, total = 0 } = event.payload || {};
                    const pct = total ? Math.floor((done / total) * 100) : 0;
                    render(`Downloading -- ${pct}% of ${(total / 1e9).toFixed(1)} GB`);
                    updateHeaderProgress(`Downloading update · ${pct}%`, pct, false);
                });
            }
            updateHeaderProgress('Starting update download…', 0, true);
            const result = await tauriInvoke('download_update_rust');
            updateHeaderProgress(`Verified download · ${result.tag}`, 100, false);
            updateHeaderAction('✔ Downloaded', 'check');
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-green-400">✔ ${result.tag} downloaded, and its signature checks out.</div>`
                    + `<div class="text-[11px] text-slate-400 select-all">${result.path}</div>`
                    + '<div class="text-[11px] text-slate-500">Run it when you are ready -- AETHER1 does not install it for you.</div>';
            }
            // The folder, not the path: a location read off a panel is a location typed out by
            // hand.
            try {
                await tauriInvoke('reveal_update_download_rust');
            } catch (e) {
                console.warn('[AETHER1] Could not open the downloads folder:', e);
            }
        } catch (e) {
            updateHeaderProgress('Update download failed', 0, false);
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">⚠ ${e.message || e}</div>`;
            }
        } finally {
            if (unlisten) unlisten();
            if (btnDownloadUpdate) btnDownloadUpdate.disabled = false;
            if (btnCheckUpdate) btnCheckUpdate.disabled = false;
        }
    }

    async function handleApplyUpdate() {
        if (!confirm('Pull the latest changes, rebuild, and relaunch Aether1? The app will restart.')) return;
        voiceEngine.playSFX('click');
        updateHeaderProgress('Applying update · Aether1 will restart', 35, true);
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
                // The message is git's own now (see perform_update_core), and it is the
                // only place an operator will ever read it: a windowed build has no
                // terminal to be pointed at, which is what the old "see the terminal"
                // line amounted to on Windows. Shown whole, wrapped, and selectable.
                // Built as a node rather than interpolated: this string is git's stderr,
                // which contains branch names, paths and remote URLs, and none of that
                // belongs in innerHTML.
                const detail = String(e && e.message ? e.message : e);
                const line = document.createElement('div');
                line.className = 'text-red-400 whitespace-pre-wrap break-words select-all';
                line.textContent = `⚠ Update failed: ${detail}`;
                updateStatusBox.replaceChildren(line);
            }
            if (btnApplyUpdate) btnApplyUpdate.disabled = false;
            if (btnCheckUpdate) btnCheckUpdate.disabled = false;
        }
    }

    // WINDOW CHROME
    // The main window is built with `decorations: false`, so everything a title bar used to
    // do has to exist here: the three buttons, dragging the window by the top bar, and eight
    // invisible strips on the edges to resize by. Only the real desktop HUD gets any of it --
    // a browser under `--serve` has no window to move, and an undocked panel window keeps its
    // native frame, so the buttons would sit under a title bar that already has them.
    function initWindowChrome() {
        const controls = document.getElementById('window-controls');
        const grips = document.getElementById('window-resize-grips');
        if (!IS_TAURI || document.documentElement.hasAttribute('data-solo-panel')) return;
        if (!controls || !grips) return;

        controls.classList.replace('hidden', 'flex');
        grips.classList.remove('hidden');

        const btnMaximize = document.getElementById('btn-window-maximize');
        const glyph = btnMaximize?.querySelector('.window-btn-glyph');

        // The button says what pressing it will do, which is the opposite of the state the
        // window is in -- so it has to be redrawn after anything that can maximise it,
        // including a double-click on the bar or the window manager's own shortcut.
        function paintMaximize(isMaximized) {
            if (!btnMaximize) return;
            btnMaximize.setAttribute('aria-pressed', isMaximized ? 'true' : 'false');
            btnMaximize.title = isMaximized ? 'Restore' : 'Maximise';
            if (glyph) glyph.textContent = isMaximized ? '\u2750' : '\u25a1';
        }

        function refreshMaximize() {
            tauriInvoke('window_is_maximized_rust').then(paintMaximize).catch(() => {});
        }

        document.getElementById('btn-window-minimize')?.addEventListener('click', () => {
            tauriInvoke('minimize_window_rust').catch((err) => console.warn('Could not minimise', err));
        });

        btnMaximize?.addEventListener('click', () => {
            tauriInvoke('toggle_maximize_window_rust')
                .then(paintMaximize)
                .catch((err) => console.warn('Could not maximise', err));
        });

        document.getElementById('btn-window-close')?.addEventListener('click', () => {
            tauriInvoke('close_window_rust').catch((err) => console.warn('Could not close', err));
        });

        // Resizing the viewport is the one signal that covers every way the window can change
        // state, including the ones that never touch our buttons.
        window.addEventListener('resize', refreshMaximize);
        refreshMaximize();

        grips.querySelectorAll('[data-resize]').forEach((grip) => {
            grip.addEventListener('mousedown', (e) => {
                if (e.button !== 0) return;
                e.preventDefault();
                tauriInvoke('start_window_resize_rust', { direction: grip.dataset.resize }).catch(() => {});
            });
        });

        // The top bar is the window's handle now. Anything pressable in it is not: a drag
        // started on the avatar chip would swallow the click that opens its menu.
        const header = document.querySelector('header');
        header?.addEventListener('mousedown', (e) => {
            if (e.button !== 0) return;
            if (e.target.closest('button, a, input, select, textarea, .hud-slideout, [data-no-window-drag]')) return;
            tauriInvoke('start_window_drag_rust').catch(() => {});
        });
        header?.addEventListener('dblclick', (e) => {
            if (e.target.closest('button, a, input, select, textarea, .hud-slideout, [data-no-window-drag]')) return;
            tauriInvoke('toggle_maximize_window_rust').then(paintMaximize).catch(() => {});
        });
    }

    function initVersionAndUpdates() {
        if (!IS_TAURI) {
            document.getElementById('setting-hotkey-wrap')?.classList.add('hidden');
            return;
        }
        if (versionBadge) versionBadge.classList.replace('hidden', 'inline-flex');
        if (headerUpdateButton) headerUpdateButton.classList.remove('hidden');
        updateHeaderAction('↻ Update', 'check');
        if (updateSection) updateSection.classList.remove('hidden');
        loadVersionInfo();
        // Kept rather than dropped: announceStartupUpdateStatus says the result of this very
        // check in the chat once the history has loaded, instead of asking GitHub again.
        startupUpdateCheck = handleCheckForUpdate();
        // Connections is Tauri-only for the same reason the rest of this section is: a browser tab
        // has no keychain of this machine's and no `gh` on it.
        document.getElementById('settings-group-connections')?.classList.remove('hidden');
        renderConnections();
    }

    // Desktop Sprite Mode is a transparent/always-on-top native window -- meaningless in the
    // plain browser flow, so its card stays hidden there (mirrors initVersionAndUpdates). It
    // is a card in the Display section rather than a section of its own, so there is no rail
    // entry to reveal: on the web Display is simply the panel grid.
    function initSpriteMode() {
        if (!IS_TAURI) return;
        const section = document.getElementById('sprite-mode-section');
        if (section) section.classList.remove('hidden');
    }

    /* The same switch, named for the room it is in. In Cyberpunk it is Game Mode, which is
       what it is for: you are about to play something and you want the machine back. Daylight
       and Midnight are the modes you have open at work, and a gamepad in the corner of those
       reads as a toy -- worse, "Game Mode: OFF" reads as the machine being *held* by
       something. Sleep Mode says the plain thing in either room: the platform stands down and
       the resources are yours. Nothing about what it does changes with the name. */
    /* A function declaration rather than a const map: the first paintTheme runs long before
       this point in the file, and a `const` up here would still be in its dead zone then. */
    function gameModeWording() {
        if (currentTheme && currentTheme.mode === 'cyberpunk') {
            return {
                off: '🎮 Game Mode: OFF',
                on: '🎮 Game Mode: ON',
                title: 'Game Mode: stop the background services and hide the HUD, without quitting.',
            };
        }
        return {
            off: '💤 Sleep Mode: OFF',
            on: '💤 Sleep Mode: ON',
            title: 'Sleep Mode: hand the machine back — the background services stop and the '
                + 'window hides, and Aether1 keeps running.',
        };
    }

    // Reflects Game Mode's current on/off state on its button in the chin bar's command
    // line -- called both from loadSettings (what was saved from a previous session) and
    // from the 'game-mode-changed' event (a live toggle, from this window or another).
    // syncThemeControls calls it again when the mode changes, since the name goes with it.
    function setGameModeButtonState(active) {
        const btn = document.getElementById('btn-game-mode');
        if (!btn) return;
        const wording = gameModeWording();
        btn.dataset.active = active ? 'true' : 'false';
        btn.textContent = active ? wording.on : wording.off;
        btn.title = wording.title;
        btn.classList.toggle('is-on', active);
    }

    // Launch autostart, Ollama autostart and Game Mode are all native-process/window
    // lifecycle -- meaningless from a browser tab, same reasoning as
    // initSpriteMode/initVersionAndUpdates, so the whole settings group and the Game Mode
    // button stay hidden there.
    function initStartupPerformance() {
        if (!IS_TAURI) return;
        document.getElementById('settings-group-startup')?.classList.remove('hidden');
        revealSettingsSection('startup');

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
        let audible = false;
        try {
            const data = await tauriInvoke('get_settings_rust');
            audible = data.settings.voice_startup_audible === true;
        } catch (e) {
            // Fall through with the audible default -- loadSettings surfaces its own
            // failure to load settings; this self-test isn't the place to repeat it.
        }

        const url = await synthesizeSpeechUrl('Welcome to the Aether1 Platform.', null);
        if (!url) return; // synthesizeSpeechUrl already showed showVoiceFailedCard on failure

        const result = await voiceEngine.playTTSAudio(url, { audible });
        if (result && result.signalDetected === false) {
            // Silence here almost always has one cause on Linux, and it is one the backend
            // can check for directly rather than leave as a description of a symptom: the
            // webview's GStreamer decoders are missing (see media_playback_step in
            // voice_setup.rs). Ask, so the card can carry the command that fixes it instead
            // of a sentence about an audio pipeline nobody can act on.
            let detail = 'Speech was synthesized, but no audio was actually heard -- the ' +
                'pipeline downstream of synthesis (the webview’s audio output) produced ' +
                'silence.';
            try {
                const advice = await tauriInvoke('voice_advice_rust');
                const fix = advice?.speaking?.steps?.find((step) => step.command);
                if (advice?.speaking?.stage === 'unheard' && fix) {
                    detail = `${advice.speaking.headline} ${fix.detail}`;
                }
            } catch (e) {
                // Fall through with the generic wording -- a failed probe is not a reason
                // to say nothing about a failure we have already established.
            }
            showVoiceFailedCard(new Error(detail));
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
    /* ---- Clicking the avatar ------------------------------------------------------
     * A tap on the hologram -- not a drag; core.js tells the two apart and calls onTap only
     * for the taps -- is the one gesture the avatar had no answer for. Now it says who it
     * is, and when this machine has a microphone it asks what is wanted and opens it, so
     * the whole exchange can happen without touching the keyboard.
     *
     * The name is the companion's own (whatever identity is loaded, forged or picked), with
     * the avatar it is wearing named beside it when the two are different things -- the
     * avatar and the identity are separate choices, and an operator looking at LOREGENDA
     * while HALCY answers is entitled to hear both.
     */

    // Whether this machine has a microphone at all. Probed once per launch and cached: the
    // answer is a property of the machine, and both lists behind it cost a round trip.
    let microphonePresenceProbe = null;

    function microphonePresent() {
        if (!navigator.mediaDevices?.getUserMedia) return Promise.resolve(false);
        if (!microphonePresenceProbe) {
            microphonePresenceProbe = (async () => {
                // The system list first: it names every input whether or not this page has
                // been granted permission to see them, and in the native window it is the
                // only one of the two that ever has anything in it (see the Sound hub note
                // about WebKitGTK and enumerateDevices).
                try {
                    const devices = await fetchAudioDevices();
                    if (Array.isArray(devices?.inputs) && devices.inputs.length) return true;
                } catch (e) {
                    /* Fall through to the browser's own list rather than concluding there is
                       no microphone because one of the two ways of asking failed. */
                }
                const browser = await enumerateBrowserDevices();
                return browser.inputs.length > 0;
            })();
        }
        return microphonePresenceProbe;
    }

    /* Two names for the same thing, or two different things? Compared on letters and digits
       only, so "A.R.X.LOGOS" and "arx-logos" are recognised as one name rather than read out
       twice in the same sentence. */
    function namesMatch(a, b) {
        const plain = (s) => String(s || '').toLowerCase().replace(/[^a-z0-9]/g, '');
        return plain(a) === plain(b);
    }

    let avatarIntroductionBusy = false;

    async function introduceAvatar() {
        // Mid-answer, mid-recording or already introducing itself: a tap is not worth
        // talking over any of those.
        if (avatarIntroductionBusy || isWaitingForResponse || talkHeld || handsFreeListening) return;
        avatarIntroductionBusy = true;
        try {
            const entry = window.Aether1Avatars?.get(currentAvatar) || null;
            const name = (currentAgentName || '').trim() || entry?.label || 'AETHER1';
            const avatarLabel = entry?.label || '';
            const introduction = avatarLabel && !namesMatch(avatarLabel, name)
                ? `I am ${name}, on the ${avatarLabel} avatar.`
                : `I am ${name}.`;

            const mic = await microphonePresent();
            const addressed = operatorDisplayName ? `, ${operatorDisplayName}` : '';
            const spoken = mic
                ? `${introduction} How can I help${addressed}?`
                : introduction;

            // On screen as well as out loud: the voice is off for some operators and missing
            // on some machines, and a tap that produces nothing at all is indistinguishable
            // from a tap that missed.
            appendMessage(currentAgentName, spoken);

            if (autoSpeak) {
                const url = await synthesizeSpeechUrl(spoken, null);
                // playTTSAudio drives the SPEAKING state and the waveform itself; a failed
                // synthesis has already shown its own card.
                if (url) await voiceEngine.playTTSAudio(url);
            }

            // Only after it has finished asking -- an open microphone during the question
            // records the question.
            if (mic) await listenHandsFree();
        } finally {
            avatarIntroductionBusy = false;
        }
    }

    function initAvatarTapIntroduction() {
        hologram.onTap = () => { introduceAvatar(); };
    }

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

        // The sprite's own power button ("Send the avatar back to the main window") turns
        // Desktop Sprite Mode off from over there, and that window closing is invisible to
        // this one. Without this, the hologram panel stays quiet behind its floating notice
        // and the Settings toggle stays on: the avatar left the desktop and never arrived
        // here. The sprite persists the setting itself, so this is only the docking half --
        // no save, no toggle_sprite_window_rust, and so no loop back to the window that
        // sent it.
        window.__TAURI__.event.listen('sprite-mode-changed', (event) => {
            const enabled = !!(event.payload && event.payload.enabled);
            const spriteModeToggle = document.getElementById('setting-sprite-mode');
            if (spriteModeToggle) spriteModeToggle.checked = enabled;
            setHologramFloating(enabled);
            // The window the avatar is being sent back to may be sitting in the tray, so
            // this end puts it on screen. Asked for from here rather than only from the
            // sprite because this window is hidden, not gone, and its command bridge is the
            // one known to work -- see showMainHud in js/sprite.js for the other half.
            if (!enabled) {
                tauriInvoke('show_main_window_rust').catch((e) => console.warn('Could not show the main window', e));
            }
        }).catch((e) => console.warn('Could not listen for sprite mode changes', e));

        // The sprite's ⧉ button, coming the long way round for the same reason.
        window.__TAURI__.event.listen('sprite-open-hud', () => {
            tauriInvoke('show_main_window_rust').catch((e) => console.warn('Could not show the main window', e));
        }).catch((e) => console.warn('Could not listen for sprite HUD requests', e));

        window.__TAURI__.event.listen('sprite-open-settings', async () => {
            try {
                await tauriInvoke('show_main_window_rust');
                btnSettings.click();
                showSettingsSection('avatars');
                openAvatarBrowser(currentAvatar);
            } catch (e) {
                console.warn('Could not open avatar settings from the desktop sprite', e);
            }
        }).catch((e) => console.warn('Could not listen for sprite settings requests', e));
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

    /* The avatar's other exit: fullscreen on a spare screen (see frontend/face.html and
       build_face_window in main.rs). Unlike the sprite this does not take the avatar out of
       this panel -- the face is a mirror, and the HUD keeps its own hologram -- so there is
       no "bring it back" notice to show here, and nothing about this panel changes.

       Removed outright in a browser rather than disabled: a button that cannot do anything
       is worse than no button, and there is no second native window to open from a tab. */
    function initAvatarFullscreen() {
        const btn = document.getElementById('btn-avatar-fullscreen');
        if (!btn) return;
        if (!IS_TAURI) {
            btn.remove();
            return;
        }
        btn.addEventListener('click', () => {
            tauriInvoke('toggle_face_window_rust', { enabled: true })
                .catch((e) => console.warn('Could not open the fullscreen face', e));
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
    /* The catalogue rows themselves, not just the <option>s built from them. Settings'
       avatar browser needs a persona's speciality, access and voice beside its avatar, and
       re-reading them off the dropdown's dataset would mean every field the browser wants
       has to be smuggled through an attribute first. */
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
    // What this machine can actually do to a command, said plainly under the Run switch.
    // The backend decides the words (code_sandbox.rs) so the CLI and the HUD cannot
    // drift apart on the one question an operator has to be able to trust.
    function applySandboxState(sandbox) {
        const line = document.getElementById('code-sandbox-state');
        if (!line || !sandbox) return;
        line.textContent = sandbox.description;
        line.classList.toggle('text-amber-300', sandbox.confines === false);
    }

    // The autonomy level: what this project is trusted with. The four names, their
    // descriptions and the answer all come from the backend (code_policy.rs), so this is
    // display and one POST -- picking a level writes the project's own file and the four
    // switches it stands for, which is why the panel is reloaded afterwards rather than
    // having its switches set here.
    function applyAutonomy(autonomy, levels) {
        const select = document.getElementById('setting-code-level');
        const hint = document.getElementById('code-level-hint');
        const sharesRow = document.getElementById('code-level-shares-row');
        const shares = document.getElementById('code-level-shares');
        if (!select || !hint) return;
        const chosen = autonomy && autonomy.level;
        if (!chosen) {
            // No project folder nominated, so there is nowhere to write an answer. Say that
            // rather than offering a choice that would go nowhere.
            select.classList.add('hidden');
            hint.textContent = 'Set a project folder below, and this is where you say what it may do.';
            if (sharesRow) sharesRow.classList.add('hidden');
            return;
        }
        select.classList.remove('hidden');
        select.innerHTML = '';
        for (const level of levels || []) {
            const option = document.createElement('option');
            option.value = level.key;
            option.textContent = level.title;
            option.title = level.description;
            select.appendChild(option);
        }
        select.value = chosen;
        const described = (levels || []).find((level) => level.key === chosen);
        hint.textContent = described ? described.description : '';
        // A switch moved by hand is reported, not reinterpreted: the level is still what the
        // project's file says, and the operator is told something below it has moved.
        const drifted = autonomy.matches === false;
        hint.classList.toggle('text-amber-300', drifted);
        if (drifted) {
            hint.textContent += ' -- one of the switches below has been changed by hand, so this is no longer what is in force. Pick the level again to restore it.';
        }
        if (sharesRow && shares) {
            const list = autonomy.shares || [];
            sharesRow.classList.toggle('hidden', list.length === 0);
            shares.textContent = list
                .map((share) => `${share.path} (${share.write ? 'read and write' : 'read only'})`)
                .join(', ') + '. Add one with `aether1 code share <folder>`.';
        }
    }

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
            applySandboxState(data.sandbox);
            applyAutonomy(data.autonomy, data.levels);
            // The domain policy is its own request: it lives in the project's file rather
            // than in settings, and it is the one block on this panel that can change
            // without anybody touching the panel. Asking here also restarts the poll, so
            // switching the sandbox network on and saving starts the cards working without
            // a restart.
            refreshNetPolicy().then(syncNetPoll);
            updateAgentNameDisplay(s.agent_name || "HALCY");
            document.getElementById('setting-agent-name').value = s.agent_name || "HALCY";
            document.getElementById('setting-operator-name').value = s.operator_name || '';
            document.getElementById('setting-machine-nickname').value = s.machine_nickname || '';
            document.getElementById('setting-machine-kind').value = s.machine_kind || '';
            updateProfileMonogram();
            updateMachineDescribed();
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
            // The device choice is an id and the label that was beside it, because the two
            // lists it is matched against -- the operating system's and the browser's --
            // do not share identifiers. See resolveAudioDevice.
            soundHub.chosen = {
                output: s.audio_output_device || '',
                outputLabel: s.audio_output_label || '',
                input: s.audio_input_device || '',
                inputLabel: s.audio_input_label || '',
            };
            renderSoundHubDevices();
            renderSoundChips();
            applyAudioDevices();
            document.getElementById('setting-vault-path').value = s.vault_path || '';
            // Absent means on, matching vault::journal_enabled -- a setting that has never
            // been saved must not read as "off" here when the vault is in fact writing.
            document.getElementById('setting-vault-journal').checked = s.vault_journal !== false;
            document.getElementById('setting-graft-autorefresh').checked = s.graft_auto_refresh !== false;
            document.getElementById('setting-local-only').checked = s.local_only === true;
            // Network & Remote. Saved by Save Changes with everything else rather than the
            // moment the switch moves: Settings has one Save button, and a panel that
            // committed on its own would also commit half-typed edits on another pane.
            const lanAutostart = document.getElementById('setting-lan-autostart');
            if (lanAutostart) lanAutostart.checked = s.lan_autostart === true;
            // After the checkbox is set, not before: loadVoiceStatus is what discovers an
            // environment-forced mode and overrides the saved value on screen.
            loadVoiceStatus();
            document.getElementById('setting-tools').checked = s.tools_enabled === true;
            // Absent means on, matching code_perms::granted -- these three were asked for
            // and default to granted, so an unsaved key must not read as "off" here while
            // the panel is in fact allowed to look.
            document.getElementById('setting-code-perm-system').checked = s.code_perm_system !== false;
            document.getElementById('setting-code-perm-github').checked = s.code_perm_github !== false;
            document.getElementById('setting-code-perm-internet').checked = s.code_perm_internet !== false;
            // And the mirror of that rule for the two that change things: absent means
            // OFF, matching Grant::default_on. A switch that reads as on before anybody
            // touched it would be the one dishonest control on this page.
            // Absent means off, matching doctor::self_repair_enabled: repairing itself
            // unattended is something the operator switches on, never a default.
            document.getElementById('setting-doctor-self-repair').checked = s.doctor_self_repair === true;
            document.getElementById('setting-code-perm-edit').checked = s.code_perm_edit === true;
            document.getElementById('setting-code-perm-run').checked = s.code_perm_run === true;
            document.getElementById('setting-code-run-network').checked = s.code_run_network === true;
            document.getElementById('setting-code-run-unconfined').checked = s.code_run_unconfined === true;
            document.getElementById('setting-code-workspace-root').value =
                typeof s.code_workspace_root === 'string' ? s.code_workspace_root : '';
            document.getElementById('setting-code-run-allowlist').value =
                Array.isArray(s.code_run_allowlist) ? s.code_run_allowlist.join(', ') : '';
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
            // Blank rather than 0 would read as "never", which is a different answer.
            document.getElementById('setting-ollama-idle-minutes').value =
                Number.isFinite(Number(s.ollama_idle_minutes)) ? Number(s.ollama_idle_minutes) : 15;
            document.getElementById('setting-voice-startup-audible').checked = s.voice_startup_audible === true;
            const voiceRate = Number(s.voice_playback_rate || 1);
            window.AETHER_VOICE_RATE = Number.isFinite(voiceRate) ? Math.min(1.25, Math.max(0.75, voiceRate)) : 1;
            const voiceRateInput = document.getElementById('setting-voice-speed');
            const voiceRateLabel = document.getElementById('voice-speed-value');
            if (voiceRateInput) voiceRateInput.value = String(window.AETHER_VOICE_RATE);
            if (voiceRateLabel) voiceRateLabel.textContent = `${window.AETHER_VOICE_RATE.toFixed(2).replace(/0$/, '')}×`;
            window.AETHER_VOICE_PITCH = Math.max(-4, Math.min(4, Number(s.voice_pitch_semitones) || 0));
            const voicePitchInput = document.getElementById('setting-voice-pitch');
            const voicePitchLabel = document.getElementById('voice-pitch-value');
            if (voicePitchInput) voicePitchInput.value = String(window.AETHER_VOICE_PITCH);
            if (voicePitchLabel) voicePitchLabel.textContent = `${window.AETHER_VOICE_PITCH} st`;
            const quietEnabled = s.quiet_hours_enabled === true;
            const quietStart = s.quiet_hours_start || '22:00';
            const quietEnd = s.quiet_hours_end || '07:00';
            const headphonesConnected = s.headphones_connected === true;
            document.getElementById('setting-quiet-hours').checked = quietEnabled;
            document.getElementById('setting-quiet-start').value = quietStart;
            document.getElementById('setting-quiet-end').value = quietEnd;
            document.getElementById('setting-headphones-connected').checked = headphonesConnected;
            window.AETHER_QUIET_HOURS = { enabled: quietEnabled, start: quietStart, end: quietEnd };
            window.AETHER_HEADPHONES_CONNECTED = headphonesConnected;
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
       the markup. Each entry leads with what it is *for* -- "Coding", "Signal & Logic" -- with
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
        personaRows = new Map(personas.map((p) => [p.key, p]));
        refreshAvatarBrowser();

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
                // Saved empty when it is empty: an operator who clears their name is
                // asking to go back to the model being told nothing about them.
                operator_name: document.getElementById('setting-operator-name').value.trim(),
                machine_nickname: document.getElementById('setting-machine-nickname').value.trim(),
                machine_kind: document.getElementById('setting-machine-kind').value,
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
                ...(() => {
                    const chosen = readAudioDeviceChoice();
                    return {
                        audio_output_device: chosen.output,
                        audio_output_label: chosen.outputLabel,
                        audio_input_device: chosen.input,
                        audio_input_label: chosen.inputLabel,
                    };
                })(),
                local_only: document.getElementById('setting-local-only').checked,
                lan_autostart: document.getElementById('setting-lan-autostart')?.checked === true,
                vault_path: document.getElementById('setting-vault-path').value.trim(),
                vault_journal: document.getElementById('setting-vault-journal').checked,
                graft_auto_refresh: document.getElementById('setting-graft-autorefresh').checked,
                // Sent only from the native app: the browser fallback has no window for the
                // OS to summon, and saving a chord there would promise something that can't
                // happen. See setting-hotkey-wrap, hidden on that path.
                ...(IS_TAURI ? { hotkey_toggle: document.getElementById('setting-hotkey').value.trim() } : {}),
                tools_enabled: document.getElementById('setting-tools').checked,
                code_perm_system: document.getElementById('setting-code-perm-system').checked,
                code_perm_github: document.getElementById('setting-code-perm-github').checked,
                code_perm_internet: document.getElementById('setting-code-perm-internet').checked,
                doctor_self_repair: document.getElementById('setting-doctor-self-repair').checked,
                code_perm_edit: document.getElementById('setting-code-perm-edit').checked,
                code_perm_run: document.getElementById('setting-code-perm-run').checked,
                code_run_network: document.getElementById('setting-code-run-network').checked,
                code_run_unconfined: document.getElementById('setting-code-run-unconfined').checked,
                code_workspace_root: document.getElementById('setting-code-workspace-root').value.trim(),
                code_run_allowlist: document.getElementById('setting-code-run-allowlist').value
                    .split(',').map(p => p.trim()).filter(Boolean),
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
                // A field left empty, or filled with something that is not a whole number of
                // minutes, means the default rather than "never" -- 0 has to be typed.
                ollama_idle_minutes: (() => {
                    const typed = Number(document.getElementById('setting-ollama-idle-minutes').value);
                    return Number.isInteger(typed) && typed >= 0 ? typed : 15;
                })(),
                voice_startup_audible: document.getElementById('setting-voice-startup-audible').checked,
                voice_playback_rate: Number(document.getElementById('setting-voice-speed')?.value || 1),
                voice_pitch_semitones: Number(document.getElementById('setting-voice-pitch')?.value || 0),
                quiet_hours_enabled: document.getElementById('setting-quiet-hours').checked,
                quiet_hours_start: document.getElementById('setting-quiet-start').value || '22:00',
                quiet_hours_end: document.getElementById('setting-quiet-end').value || '07:00',
                headphones_connected: document.getElementById('setting-headphones-connected').checked
            }
        };
        autoSpeak = payload.settings.auto_speak;
        applyAudioDevices();
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

    // Mode: the three buttons in Settings and the same three in the top bar's slide-out.
    document.querySelectorAll('.theme-mode-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            paintTheme(Aether1Theme.setMode(btn.getAttribute('data-theme-mode')));
        });
    });

    /* Applying a whole named palette, mode included. This is what forging an identity does
       when the name it lands on has a palette of its own (see the A.R.X. names below), and it
       is what a Cyberpunk swatch does. It can move you between modes, which is the point when
       the preset is a light one. The accent-only path in Daylight and Midnight is
       Aether1Theme.setAccents instead -- see renderThemePalette. */
    function applyThemePreset(id) {
        paintTheme(Aether1Theme.setPreset(id));
    }

    /* The palette swatches are wired as they are built -- see renderThemePalette, which is
       re-run on every theme change because what a swatch means depends on the mode.

       The pickers, though, are here and permanent. 'input' rather than 'change' so the page repaints while the
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
    chatMediaButton?.addEventListener('click', () => chatMediaInput?.click());
    chatMediaInput?.addEventListener('change', async () => { await addChatImageFiles(chatMediaInput.files); chatMediaInput.value = ''; });
    chatInput.addEventListener('paste', (event) => {
        const images = Array.from(event.clipboardData?.items || []).filter((item) => item.type.startsWith('image/')).map((item) => item.getAsFile()).filter(Boolean);
        if (images.length) { event.preventDefault(); void addChatImageFiles(images); }
    });
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
    /* Memories.                                                            */
    /*                                                                      */
    /* The core notes, which vault::prime pastes into every system prompt   */
    /* whether or not the question calls for them. Same folder as NOTES and */
    /* the same read, filtered on the `core` flag the reader already sends  */
    /* -- there is no second store behind this and nothing here that the    */
    /* notes tab cannot also show. It is a tab of its own because "what it  */
    /* always knows about me" is a different question from "what is in my   */
    /* vault", and answering the first by scrolling a list looking for a    */
    /* marker is how you end up not asking it.                              */
    /*                                                                      */
    /* Read-only, like the reader: a note is changed through the tools,     */
    /* behind the consent path, with a way back.                            */
    /* -------------------------------------------------------------------- */

    const memoriesList = document.getElementById('memories-list');
    const memoriesCount = document.getElementById('memories-count');

    function renderMemories(notes) {
        memoriesList.textContent = '';
        const core = notes.filter((n) => n.core);
        memoriesCount.textContent = core.length
            ? `${core.length} note${core.length === 1 ? '' : 's'}, always loaded`
            : 'nothing always-loaded yet';

        if (!core.length) {
            const empty = document.createElement('p');
            empty.className = 'text-[11px] font-mono text-slate-500 leading-snug';
            empty.textContent =
                'Nothing is marked always-loaded yet. Notes it writes about you land here '
                + 'once they are, and everything else is under NOTES.';
            memoriesList.appendChild(empty);
            return;
        }

        core.forEach((note) => {
            const size = note.bytes < 1024 ? `${note.bytes} B` : `${Math.round(note.bytes / 1024)} KB`;
            const row = document.createElement('button');
            row.type = 'button';
            row.className = 'w-full text-left px-2 py-1.5 rounded border border-cyan-500/20 '
                + 'bg-slate-900/50 hover:border-cyan-400/60 hover:bg-slate-800/60 cursor-pointer';
            const name = document.createElement('div');
            name.className = 'font-mono text-[11px] text-cyan-200 truncate';
            name.textContent = note.name;
            const meta = document.createElement('div');
            meta.className = 'font-mono text-[10px] text-slate-500 truncate';
            meta.textContent = `${size} · ${noteAge(note.modified)}`;
            row.append(name, meta);
            /* One reader, one place a note is read. This list hands off to it rather
               than growing a second body pane that could disagree with the first. */
            row.addEventListener('click', () => {
                voiceEngine.playSFX('click');
                switchChatTab('notes');
                noteTrail = [];
                openNote(note.name);
            });
            memoriesList.appendChild(row);
        });
    }

    async function loadMemories() {
        memoriesCount.textContent = 'reading the folder…';
        try {
            renderMemories(await fetchNotes());
        } catch (err) {
            memoriesList.textContent = '';
            memoriesCount.textContent = 'could not read the folder';
            const p = document.createElement('p');
            p.className = 'text-[11px] font-mono text-amber-300';
            p.textContent = `Could not read the notes folder: ${err.message || err}`;
            memoriesList.appendChild(p);
        }
    }

    document.getElementById('btn-memories-refresh')?.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        loadMemories();
    });

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
        /* Picking a conversation is asking to read it, so the panel goes back to the
           conversation itself rather than leaving you on the list you just used. */
        switchChatTab('conversation');
        await loadChatHistory();
    }

    async function openSessions() {
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

    document.getElementById('btn-session-new')?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        try {
            /* A new conversation is an id and nothing else. It starts existing when
               something is said in it, which is why it is not in the list yet. */
            setCurrentSession(await mintSession());
            switchChatTab('conversation');
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
    /* It lives in the conversation panel, as the third tab beside          */
    /* CONVERSATION and AETHER CODE, because that is where the operator     */
    /* already is when a command is offered to them -- and because a shell  */
    /* they would rather have elsewhere is one they can open natively.      */
    /*                                                                      */
    /* In a browser the tab and its view are removed from the DOM rather    */
    /* than disabled. There is no route behind it there, and a terminal     */
    /* that looks like it works is worse than no terminal at all.           */
    /* -------------------------------------------------------------------- */
    const terminalView = document.getElementById('chat-view-terminal');
    if (terminalView && !IS_TAURI) {
        terminalView.remove();
        document.getElementById('tab-chat-terminal')?.remove();
    } else if (terminalView) {
        const terminalSurface = document.getElementById('terminal-surface');
        const terminalState = document.getElementById('terminal-state');
        const terminalNote = document.getElementById('terminal-note');
        const btnTerminalStart = document.getElementById('btn-terminal-start');
        let shell = null;

        const setState = (text) => { if (terminalState) terminalState.textContent = text; };

        /* Starting a shell is the same three steps whether the button or Aether Code asks
           for it, and they have to be the same three: a shell started one way and revealed
           the other leaves the panel showing its "not started" note over a running
           terminal. */
        async function openShell() {
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
        }

        /* The only way anything outside this block reaches the shell, and it is deliberately
           two verbs wide: start one if there is none, and type. There is no "run" here and
           there is not going to be one -- see terminal.js's type(). */
        terminalBridge = {
            running: () => !!shell && shell.running(),
            ensureStarted: openShell,
            type: (text) => (shell ? shell.type(text) : false),
        };

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
                await openShell();
            } catch (err) {
                terminalSurface.classList.add('hidden');
                terminalNote?.classList.remove('hidden');
                setState('could not start');
                console.warn('Could not start a shell', err);
                alert(`Could not start a shell: ${err.message || err}`);
            }
        });

        /* A panel switched off has no layout box, so xterm cannot measure it; switched
           back on it needs to be told its size before the shell draws at the old one. A
           hidden tab is the same thing one level down, hence the hook. */
        document.addEventListener('aether1:panels-changed', () => shell?.fit());
        onTerminalTabShown = () => {
            if (!shell) return;
            /* After the browser has laid the view out, not before it. */
            requestAnimationFrame(() => shell.fit());
        };
    }

    document.getElementById('btn-read-notes')?.addEventListener('click', () => {
        settingsModal.classList.add('hidden');
        openNotesReader();
    });
    document.getElementById('btn-notes-open-folder')?.addEventListener('click', openVaultFolder);

    /* ================================================================ */
    /* Graft integration for code analysis                             */
    /* ================================================================ */

    async function loadGraftStatus() {
        try {
            const statusEl = document.getElementById('graft-status');
            if (!statusEl) return;

            // Check Graft version
            try {
                if (IS_TAURI) {
                    const version = await tauriInvoke('graft_version_rust');
                    statusEl.textContent = `✓ Graft ${version.version} installed`;
                } else {
                    statusEl.textContent = `ℹ️ Graft status requires Tauri`;
                }
            } catch (e) {
                statusEl.textContent = '✗ Graft not found. Install it with: npm install -g @nanonets/graft';
            }
        } catch (e) {
            console.error('Error loading Graft status:', e);
        }
    }

    async function loadGraftProjects() {
        try {
            const projectsList = document.getElementById('graft-projects-list');
            if (!projectsList) return;

            if (!IS_TAURI) {
                projectsList.innerHTML = '<div class="text-slate-400">Project detection requires Tauri</div>';
                return;
            }

            projectsList.innerHTML = '<div class="text-slate-400">Detecting projects...</div>';
            const projects = await tauriInvoke('graft_detect_projects_rust');

            if (!projects || projects.length === 0) {
                projectsList.innerHTML = '<div class="text-slate-400">No repositories found in common locations</div>';
                return;
            }

            const selected = await tauriInvoke('graft_get_selected_project_rust');
            const selectedPath = selected.path;

            projectsList.innerHTML = projects.map(p => `
                <div class="p-2 border-b border-slate-700 cursor-pointer hover:bg-slate-800 transition-colors"
                     data-project-path="${p.path}"
                     onclick="graftSelectProject('${p.path.replace(/'/g, "\\'")}')">
                    <div class="flex justify-between items-start">
                        <span class="font-semibold text-cyan-300">${p.name}</span>
                        <span class="text-[9px] text-slate-400">${p.graft_status}</span>
                    </div>
                    <div class="text-[9px] text-slate-500 break-all">${p.path}</div>
                    ${selectedPath === p.path ? '<div class="text-[9px] text-green-400 mt-1">✓ Selected</div>' : ''}
                </div>
            `).join('');
        } catch (e) {
            console.error('Error loading Graft projects:', e);
            document.getElementById('graft-projects-list').innerHTML =
                `<div class="text-red-400 text-[10px]">Error: ${e.message || e}</div>`;
        }
    }

    window.graftSelectProject = async function(projectPath) {
        try {
            if (!IS_TAURI) return;

            await tauriInvoke('graft_select_project_rust', { projectPath });

            // Re-enable build button
            const buildBtn = document.getElementById('btn-graft-build');
            if (buildBtn) buildBtn.disabled = false;

            // Reload project list to show selection
            await loadGraftProjects();

            appendMessage(currentAgentName, `📊 Selected project: ${projectPath}`);
        } catch (e) {
            console.error('Error selecting project:', e);
            appendMessage(currentAgentName, `❌ Error selecting project: ${e.message || e}`);
        }
    };

    document.getElementById('btn-graft-detect')?.addEventListener('click', loadGraftProjects);

    document.getElementById('btn-graft-build')?.addEventListener('click', async () => {
        try {
            if (!IS_TAURI) return;

            const selected = await tauriInvoke('graft_get_selected_project_rust');
            if (!selected.path) {
                appendMessage(currentAgentName, '⚠️ Please select a project first');
                return;
            }

            const buildBtn = document.getElementById('btn-graft-build');
            buildBtn.disabled = true;
            buildBtn.textContent = '⏳ Building...';

            await tauriInvoke('graft_build_graph_rust', { projectPath: selected.path });

            // Reload status to show "Ready"
            await loadGraftStatus();
            await loadGraftProjects();

            appendMessage(currentAgentName, `✓ Graft graph built for: ${selected.path}`);
            buildBtn.textContent = '🔨 Build Graph';
            buildBtn.disabled = false;
        } catch (e) {
            console.error('Error building Graft graph:', e);
            appendMessage(currentAgentName, `❌ Failed to build Graft graph: ${e.message || e}`);
            const buildBtn = document.getElementById('btn-graft-build');
            buildBtn.textContent = '🔨 Build Graph';
            buildBtn.disabled = false;
        }
    });

    // Load initial Graft status when settings are opened
    if (settingsModal) {
        const observer = new MutationObserver(() => {
            if (!settingsModal.classList.contains('hidden')) {
                loadGraftStatus();
                loadGraftProjects();
            }
        });
        observer.observe(settingsModal, { attributes: true, attributeFilter: ['class'] });
    }

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

    /* Leaving the tab means stopped. The canvas keeps its layout, so this is the
       one place that has to say so -- switchChatTab calls it on every tab that
       isn't this one. */
    function stopNoteGraph() {
        if (noteGraph) noteGraph.stop();
    }

    document.getElementById('btn-activity-refresh')?.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        loadActivityLog();
    });

    btnSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        // The scan waits for the settings: it preselects whichever found server matches
        // the configured endpoint, and that field has to be filled in before it looks.
        loadSettings().then(handleScanSystem);
        refreshBrainStatus();
        refreshModelHub();
        // Chosen on open rather than at startup: the platform-dependent sections are
        // revealed during init, and a remembered choice may be one of them.
        restoreSettingsSection();
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

    if (versionBadge) {
        versionBadge.addEventListener('click', () => handleCheckForUpdate());
    }
    headerUpdateButton?.addEventListener('click', () => {
        if (headerUpdateButton.dataset.action === 'download') handleDownloadUpdate();
        else if (headerUpdateButton.dataset.action === 'apply') handleApplyUpdate();
        else handleCheckForUpdate();
    });
    if (btnCheckUpdate) {
        btnCheckUpdate.addEventListener('click', () => handleCheckForUpdate());
    }
    if (btnGithubSignIn) {
        btnGithubSignIn.addEventListener('click', () => revealConnections());
    }
    document.getElementById('conn-github-signin')?.addEventListener('click', () => handleSignIn());
    document.getElementById('btn-open-model-hub')?.addEventListener('click', () => {
        settingsModal.classList.remove('hidden');
        showSettingsSection('brain');
    });
    document.getElementById('conn-github-signout')?.addEventListener('click', () => handleSignOut());
    if (btnGithubDecline) {
        btnGithubDecline.addEventListener('click', () => handleDeclineUpdates());
    }
    if (btnDownloadUpdate) {
        btnDownloadUpdate.addEventListener('click', () => handleDownloadUpdate());
    }
    if (btnApplyUpdate) {
        btnApplyUpdate.addEventListener('click', () => handleApplyUpdate());
    }

    btnCloseSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        settingsModal.classList.add('hidden');
    });

    settingsModal.addEventListener('click', (event) => {
        if (event.target === settingsModal) settingsModal.classList.add('hidden');
    });

    const voiceRateInput = document.getElementById('setting-voice-speed');
    const voiceRateLabel = document.getElementById('voice-speed-value');
    voiceRateInput?.addEventListener('input', () => {
        const rate = Math.min(1.25, Math.max(0.75, Number(voiceRateInput.value) || 1));
        window.AETHER_VOICE_RATE = rate;
        if (voiceRateLabel) voiceRateLabel.textContent = `${rate.toFixed(2).replace(/0$/, '')}×`;
    });
    const voicePitchInput = document.getElementById('setting-voice-pitch');
    const voicePitchLabel = document.getElementById('voice-pitch-value');
    voicePitchInput?.addEventListener('input', () => {
        window.AETHER_VOICE_PITCH = Math.max(-4, Math.min(4, Number(voicePitchInput.value) || 0));
        if (voicePitchLabel) voicePitchLabel.textContent = `${window.AETHER_VOICE_PITCH} st`;
    });
    document.getElementById('btn-voice-audition')?.addEventListener('click', async () => {
        const url = await synthesizeSpeechUrl('This is how the current Aether1 voice sounds.', null);
        if (url) await voiceEngine.playTTSAudio(url, {
            playbackRate: window.AETHER_VOICE_RATE || 1,
            pitchSemitones: window.AETHER_VOICE_PITCH || 0,
        });
    });
    const syncQuietHours = () => {
        window.AETHER_QUIET_HOURS = {
            enabled: document.getElementById('setting-quiet-hours').checked,
            start: document.getElementById('setting-quiet-start').value || '22:00',
            end: document.getElementById('setting-quiet-end').value || '07:00',
        };
        window.AETHER_HEADPHONES_CONNECTED = document.getElementById('setting-headphones-connected').checked;
    };
    ['setting-quiet-hours', 'setting-quiet-start', 'setting-quiet-end', 'setting-headphones-connected']
        .forEach((id) => document.getElementById(id)?.addEventListener('change', syncQuietHours));

    btnSaveSettings.addEventListener('click', () => {
        saveSettings(true);
    });

    if (btnAgentBrowser) {
        btnAgentBrowser.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            if (agentBrowserContainer && AgentBrowser) {
                // Render the agent browser when opened
                AgentBrowser.render('agent-browser-container');
            }
            agentBrowserModal.classList.remove('hidden');
        });
    }

    if (btnCloseAgentBrowser) {
        btnCloseAgentBrowser.addEventListener('click', () => {
            voiceEngine.playSFX('click');
            agentBrowserModal.classList.add('hidden');
        });
    }

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

    /* No click SFX on this one. It is the button you press to make it stop making noise. */
    document.getElementById('btn-hush')?.addEventListener('click', () => hush());

    btnSfxToggle.addEventListener('click', () => {
        applySfx(!voiceEngine.sfxEnabled);
        voiceEngine.playSFX('click');
        // Saved without closing anything or announcing it: this is a menu toggle, and the
        // one thing it must do that it did not before is survive a restart.
        saveSettings(false);
    });

    // Picking a level is its own request, not part of Save: it writes the project's file
    // and the four switches at once, and those switches are fields on this same panel, so
    // the panel is reloaded to show what the choice did.
    document.getElementById('setting-code-level')?.addEventListener('change', async (e) => {
        const level = e.target.value;
        try {
            if (IS_TAURI) {
                await tauriInvoke('code_set_level_rust', { level });
            } else {
                const resp = await apiFetch('/api/code/level', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ level }),
                });
                if (!resp.ok) throw new Error(await resp.text());
            }
            await loadSettings();
        } catch (err) {
            const hint = document.getElementById('code-level-hint');
            if (hint) {
                hint.textContent = `That level was not set: ${err.message || err}`;
                hint.classList.add('text-amber-300');
            }
        }
    });


    // --- The sandbox's domain policy --------------------------------------------------
    //
    // Two surfaces over one thing. The Settings card lists what this project's sandbox may
    // connect to and lets a domain be added or taken away. The HUD card is what appears
    // when a command asks for a host that is not on the list: the proxy holds the
    // connection open while the card is up, and the choice on it is what releases or
    // refuses the connection.
    //
    // Before this, the only way past a refused host was to read a 403 out of a build log,
    // work out which host it was about, and type `aether1 code net-allow`. That friction
    // is what makes people switch on "Run without a sandbox" -- so the card is a security
    // control, not a convenience. See src-tauri/src/code_proxy.rs.

    /* The last status seen, which is what the poll below is gated on. */
    let lastNetStatus = null;

    /* The one transport split, in one place. Every call below returns the same status
       object -- the read does, and so does each write, so that a change is drawn from what
       the backend says is now true rather than from what the frontend asked for. */
    async function netPolicyApi(path, tauriCommand, body) {
        if (IS_TAURI) return tauriInvoke(tauriCommand, body || {});
        const options = body
            ? { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }
            : (path === '/api/code/net' ? {} : { method: 'POST' });
        const resp = await apiFetch(path, options);
        if (!resp.ok) throw new Error((await resp.text()) || `request failed: ${resp.status}`);
        return resp.json();
    }

    function netPolicyHint(text, bad) {
        const hint = document.getElementById('code-net-hint');
        if (!hint) return;
        hint.textContent = text || '';
        hint.classList.toggle('hidden', !text);
        hint.classList.toggle('text-amber-300', !!bad);
        hint.classList.toggle('text-slate-400', !bad);
    }

    /* One domain, as a chip. A starter domain and one the operator typed look the same in
       the effective list but are not removed the same way, so the chip says which it is --
       and a `deny` row is shown too, because a starter domain that has been taken away is
       invisible otherwise and looks like a bug. */
    function netDomainChip(domain, kind) {
        const chip = document.createElement('span');
        const tone = kind === 'removed'
            ? 'border-rose-500/40 text-rose-300/80 bg-rose-950/20 line-through'
            : kind === 'added'
                ? 'border-emerald-500/40 text-emerald-200 bg-emerald-950/20'
                : 'border-cyan-500/30 text-cyan-200/80 bg-slate-900/60';
        chip.className = `inline-flex items-center gap-1 text-[10px] font-mono border rounded px-1.5 py-0.5 ${tone}`;
        chip.title = kind === 'removed'
            ? `${domain} is refused: this project's policy takes it away.`
            : kind === 'added'
                ? `${domain} was allowed for this project.`
                : `${domain} is on the starter list -- a host a build reaches on its own.`;
        const label = document.createElement('span');
        label.textContent = domain;
        chip.appendChild(label);

        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'opacity-60 hover:opacity-100 cursor-pointer';
        // Taking away and putting back are the same button in two directions, because the
        // chip already says which state it is in.
        button.textContent = kind === 'removed' ? '+' : '×';
        button.title = kind === 'removed' ? `Allow ${domain} again` : `Stop allowing ${domain}`;
        button.onclick = async () => {
            button.disabled = true;
            try {
                const status = kind === 'removed'
                    ? await netPolicyApi('/api/code/net/allow', 'code_net_allow_rust', { domain })
                    : await netPolicyApi('/api/code/net/forget', 'code_net_forget_rust', { domain });
                applyNetPolicy(status);
                netPolicyHint(kind === 'removed' ? `${domain} is allowed again.` : `${domain} is no longer allowed.`, false);
            } catch (e) {
                button.disabled = false;
                netPolicyHint(`That did not change: ${e.message || e}`, true);
            }
        };
        chip.appendChild(button);
        return chip;
    }

    /* Draws the Settings card from a status object. */
    function applyNetPolicy(status) {
        if (!status) return;
        lastNetStatus = status;
        const state = document.getElementById('code-net-state');
        const list = document.getElementById('code-net-domains');
        const add = document.getElementById('code-net-add');
        const addBtn = document.getElementById('code-net-add-btn');
        if (!state || !list) return;

        // No project folder, so there is no file to hold a policy. Say that rather than
        // listing the starter domains as though they applied to something.
        if (!status.root) {
            state.textContent = 'Set a project folder above, and the policy lives in that project’s own .aether/policy.json.';
            state.classList.add('text-amber-300');
            list.innerHTML = '';
            if (add) add.disabled = true;
            if (addBtn) addBtn.disabled = true;
            return;
        }
        if (add) add.disabled = false;
        if (addBtn) addBtn.disabled = false;
        state.classList.remove('text-amber-300');
        const where = `Kept in ${status.file} under ${status.root}.`;
        // An allowed-domain list is a confusing thing to read while the sandbox has no
        // network at all, so the switch above is reported here rather than left to be
        // inferred from a list that is not in force.
        state.textContent = status.network
            ? where
            : `${where} The network switch above is off, so nothing reaches any of these yet.`;

        list.innerHTML = '';
        const starter = new Set(status.starter || []);
        const added = new Set(status.added || []);
        for (const domain of status.allowed || []) {
            list.appendChild(netDomainChip(domain, added.has(domain) && !starter.has(domain) ? 'added' : 'starter'));
        }
        for (const domain of status.removed || []) {
            list.appendChild(netDomainChip(domain, 'removed'));
        }

        // What a build asked for and did not get, which is the question an operator
        // arrives with. `pending` is "it wanted this"; `denied` is "and the answer was
        // no, for this reason" -- the reason matters, because "nobody answered" sends you
        // to the HUD and "refused" sends you here.
        const refusedRow = document.getElementById('code-net-refused-row');
        const refused = document.getElementById('code-net-refused');
        if (refusedRow && refused) {
            const words = { refused: 'you refused it', unanswered: 'the card went unanswered', 'not asked': 'no window was open to ask' };
            const lines = (status.denied || []).slice().reverse()
                .map((entry) => `${entry.host} — ${words[entry.why] || entry.why}`);
            // A host that asked and was never decided on at all still belongs here.
            const decided = new Set((status.denied || []).map((entry) => entry.host));
            for (const host of status.pending || []) {
                if (!decided.has(host)) lines.push(`${host} — asked for, not allowed`);
            }
            refusedRow.classList.toggle('hidden', lines.length === 0);
            refused.textContent = lines.join('; ');
        }
    }

    /* The HUD card. Three choices and no default: the connection is being held open while
       this is on screen, and picking for the operator is the thing this whole mechanism
       exists not to do. */
    function renderNetAskCard(ask) {
        const card = document.createElement('div');
        card.className = 'p-3 rounded my-2 text-sm msg-agent self-start mr-8 border border-amber-500/50 bg-amber-950/20';
        card.dataset.netAskId = ask.id;

        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 border-b border-amber-500/30 text-xs font-mono text-amber-300';
        header.innerHTML = `<span>🌐 <strong>A COMMAND WANTS THE NETWORK</strong></span><span>${new Date().toLocaleTimeString()}</span>`;
        card.appendChild(header);

        const body = document.createElement('div');
        body.className = 'text-cyan-100 font-mono text-xs my-2 break-all';
        body.textContent = ask.port === 443 || ask.port === 80 ? ask.host : `${ask.host}:${ask.port}`;
        card.appendChild(body);

        const reason = document.createElement('div');
        reason.className = 'text-[10px] font-mono text-amber-200/80 my-1';
        // Said on the card, not left in the docs: allowing a domain allows it entirely,
        // and what the sandbox can read is the thing the operator is actually risking.
        reason.textContent = `A command in the sandbox is trying to reach ${ask.host}, which is not on this project’s allowed list. `
            + 'Allowing it allows that host entirely — it can be sent whatever the sandbox can read, which is this project’s folder.'
            + (ask.waiting > 1 ? ` ${ask.waiting} connections are waiting on this.` : '');
        card.appendChild(reason);

        const status = document.createElement('div');
        status.className = 'text-xs font-mono text-slate-400 mt-2';

        const buttons = document.createElement('div');
        buttons.className = 'flex flex-wrap gap-2 mt-2';
        const choices = [
            ['once', '↻ Allow once', 'border-amber-500/50 text-amber-200 bg-amber-950/40 hover:bg-amber-900/40', 'Lets this attempt through and writes nothing. The next one asks again.'],
            ['project', '✔ Allow for this project', 'border-emerald-500/50 text-emerald-300 bg-emerald-950/40 hover:bg-emerald-900/40', 'Writes the domain into this project’s .aether/policy.json. Nothing asks again.'],
            ['deny', '✖ Refuse', 'border-rose-500/50 text-rose-300 bg-rose-950/40 hover:bg-rose-900/40', 'Refuses the connection and records it in Settings.'],
        ];
        const made = choices.map(([key, label, tone, title]) => {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = `text-xs font-mono border px-3 py-1 rounded cursor-pointer ${tone}`;
            button.textContent = label;
            button.title = title;
            button.dataset.decision = key;
            buttons.appendChild(button);
            return button;
        });

        const settle = (text, tone) => {
            buttons.remove();
            status.className = `text-xs font-mono mt-2 ${tone}`;
            status.textContent = text;
        };

        for (const button of made) {
            button.onclick = async () => {
                for (const other of made) other.disabled = true;
                status.textContent = 'Telling the command…';
                try {
                    const report = await netPolicyApi('/api/code/net/decide', 'code_net_decide_rust', {
                        id: ask.id,
                        decision: button.dataset.decision,
                    });
                    applyNetPolicy(report.status);
                    // "settled" is false when the command gave up between the click and
                    // the answer reaching it. For `project` the policy was still written,
                    // and saying "allowed" about a command that has already failed would
                    // send the operator looking for output that never comes.
                    if (!report.settled) {
                        settle(button.dataset.decision === 'project'
                            ? `${ask.host} is allowed for this project now, but the command had already given up waiting. Run it again.`
                            : 'The command gave up waiting before this was answered. Run it again.', 'text-amber-300');
                        return;
                    }
                    settle({
                        once: `↻ ${ask.host} allowed for this attempt only`,
                        project: `✔ ${ask.host} allowed for this project`,
                        deny: `✖ ${ask.host} refused`,
                    }[button.dataset.decision], button.dataset.decision === 'deny' ? 'text-slate-400' : 'text-emerald-300');
                } catch (e) {
                    settle(`✖ Failed: ${e.message || e}`, 'text-rose-300');
                }
            };
        }

        card.appendChild(buttons);
        card.appendChild(status);
        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        voiceEngine.playSFX('alert');
        return card;
    }

    /* Fetches the policy and draws a card for anything waiting that isn't already up.
       Asking is also what marks this window as present -- see commands::code_net_status --
       so this is the heartbeat the proxy holds connections open against. */
    async function refreshNetPolicy() {
        let status;
        try {
            status = await netPolicyApi('/api/code/net', 'code_net_status_rust');
        } catch (e) {
            console.warn('[AETHER1] could not read the sandbox network policy:', e);
            return null;
        }
        applyNetPolicy(status);
        for (const ask of status.asks || []) {
            if (!chatContainer.querySelector(`[data-net-ask-id="${ask.id}"]`)) {
                renderNetAskCard(ask);
            }
        }
        return status;
    }

    document.getElementById('code-net-add-btn')?.addEventListener('click', async () => {
        const box = document.getElementById('code-net-add');
        const domain = (box?.value || '').trim();
        if (!domain) return;
        voiceEngine.playSFX('click');
        try {
            applyNetPolicy(await netPolicyApi('/api/code/net/allow', 'code_net_allow_rust', { domain }));
            if (box) box.value = '';
            netPolicyHint(`${domain} is allowed for this project.`, false);
        } catch (e) {
            netPolicyHint(`${domain} was not added: ${e.message || e}`, true);
        }
    });

    document.getElementById('code-net-add')?.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            document.getElementById('code-net-add-btn')?.click();
        }
    });

    document.getElementById('code-net-clear-btn')?.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        try {
            applyNetPolicy(await netPolicyApi('/api/code/net/clear', 'code_net_clear_rust', null));
        } catch (e) {
            netPolicyHint(`That list was not cleared: ${e.message || e}`, true);
        }
    });

    /* The poll, gated twice.
       The sandbox's network switch being off means the proxy can never be asked anything,
       so there is nothing to poll for -- and `document.hidden` keeps a parked window out
       of it, for the same reason the telemetry sampler parks: a HUD behind a game should
       cost nothing. One consequence worth knowing: with no window polling, the proxy does
       not wait at all. An unlisted host is refused immediately, with a message that says
       so. That is the fail-closed direction, and it is what keeps the CLI and the test
       suite behaving as they did before any of this existed. */
    let netPollTimer = null;
    const NET_POLL_MS = 5000;

    function syncNetPoll() {
        const wanted = !document.hidden && !!(lastNetStatus && lastNetStatus.network);
        if (wanted && !netPollTimer) {
            netPollTimer = setInterval(() => { refreshNetPolicy().then(syncNetPoll); }, NET_POLL_MS);
        } else if (!wanted && netPollTimer) {
            clearInterval(netPollTimer);
            netPollTimer = null;
        }
    }

    document.addEventListener('visibilitychange', () => {
        // Coming back from hidden: ask once straight away rather than waiting out an
        // interval, because a command may have been blocked the whole time the window was
        // parked.
        if (!document.hidden) refreshNetPolicy().then(syncNetPoll);
        else syncNetPoll();
    });

    refreshNetPolicy().then(syncNetPoll);

    document.getElementById('setting-sfx')?.addEventListener('change', (e) => {
        applySfx(e.target.checked);
    });

    document.querySelectorAll('.quick-chip').forEach(chip => {
        chip.addEventListener('click', () => {
            const cmd = chip.getAttribute('data-cmd');
            if (cmd) handleSendMessage(cmd);
        });
    });

    // The hologram's chin: zoom, pitch, tilt, and nothing else. Three ways of saying
    // "show me it like this" -- how big it reads, how far it tips toward you, how far it
    // leans -- so they are one strip rather than three unrelated controls. All three are
    // kept in localStorage: an angle you set by hand is a preference, and having to set it
    // again every launch would make it not worth setting.
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

    /* Pitch and tilt are the same control twice over, so they are wired once. The engine
       clamps the angle itself; this only has to keep the slider, the readout and the
       stored value agreeing with each other. */
    function initAvatarAngleSlider(id, storageKey, apply) {
        const slider = document.getElementById(id);
        const readout = document.getElementById(`${id}-readout`);
        if (!slider) return;
        const stored = parseFloat(localStorage.getItem(storageKey));
        const start = Number.isFinite(stored) ? Math.min(45, Math.max(-45, stored)) : 0;
        slider.value = String(Math.round(start));
        const paint = () => {
            const degrees = Number(slider.value) || 0;
            if (readout) readout.textContent = `${degrees}\u00b0`;
            apply(degrees);
        };
        paint();
        slider.addEventListener('input', () => {
            localStorage.setItem(storageKey, slider.value);
            paint();
        });
    }

    initAvatarAngleSlider('avatar-pitch', 'aether_avatar_pitch', (d) => hologram.setViewPitch(d));
    initAvatarAngleSlider('avatar-tilt', 'aether_avatar_tilt', (d) => hologram.setViewTilt(d));

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
    loadChatHistory().then(refreshPendingApprovals).then(announceIfNoBrain).then(loadOperatorNameAndGreeting)
        .then(announceStartupUpdateStatus);
    connectTelemetry();
    initWindowChrome();
    initVersionAndUpdates();
    initModelHub();
    initSpriteMode();
    initStartupPerformance();
    initSoloPanel();
    initPanelUndock();
    initAvatarFullscreen();
    initHologramFloatingNotice();
    initAvatarTapIntroduction();
    initSpecialityModel();
    initFlowMode();
    initSpriteListenBridge();
    runVoiceStartupSelfTest();

    document.body.addEventListener('click', () => {
        voiceEngine.playSFX('boot');
    }, { once: true });
});
