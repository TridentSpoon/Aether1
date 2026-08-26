/**
 * Main Frontend Application Logic for Project AETHER1.
 * Handles independently-selectable 3D Avatars (R.E.D. 9000, The Nexus, A.R.X.LIMES, hAlcy, A.R.X.LOGOS)
 * and Color Themes, Token Telemetry Graph, Agent Genesis, and Model Scanner.
 */

const AVATAR_DISPLAY_NAMES = {
    halcy: 'HALCY',
    nexus: 'THE NEXUS',
    matrix: 'THE NEXUS',
    'arx-limes': 'A.R.X.LIMES',
    'arx-logos': 'A.R.X.LOGOS',
    red: 'R.E.D. 9000',
    crimson: 'R.E.D. 9000'
};

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

    // Agent Genesis & Theme Elements
    const btnGenesisQuick = document.getElementById('btn-genesis-quick');
    const btnForgeIdentity = document.getElementById('btn-forge-identity');
    const genesisPurposeInput = document.getElementById('genesis-purpose-input');
    const hudAgentName = document.getElementById('hud-agent-name');
    const terminalAgentLabel = document.getElementById('terminal-agent-label');
    const avatarStructureLabel = document.getElementById('avatar-structure-label');
    const colorThemeSelect = document.getElementById('color-theme-select');

    // Model Scanner Elements
    const btnScanSystem = document.getElementById('btn-scan-system');
    const btnPullLlama = document.getElementById('btn-pull-llama');
    const selectLocalModel = document.getElementById('select-local-model');
    const scannerResultsBox = document.getElementById('scanner-results-box');

    // Hardware Telemetry Elements
    const elCpuGauge = document.getElementById('cpu-gauge-fill');
    const elCpuVal = document.getElementById('cpu-percent-val');
    const elRamGauge = document.getElementById('ram-gauge-fill');
    const elRamVal = document.getElementById('ram-percent-val');
    const elDiskGauge = document.getElementById('disk-gauge-fill');
    const elDiskVal = document.getElementById('disk-percent-val');
    const elNetDown = document.getElementById('net-download-val');
    const elNetUp = document.getElementById('net-upload-val');
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
    let currentAvatar = localStorage.getItem('aether_avatar') || 'halcy';
    let currentColorTheme = localStorage.getItem('aether_color_theme') || 'halcy';

    // Clock
    function updateClock() {
        const now = new Date();
        if (elClock) {
            elClock.textContent = now.toLocaleTimeString() + " // " + now.toISOString().split('T')[0];
        }
    }
    setInterval(updateClock, 1000);
    updateClock();

    // Avatar Engine Handler (3D shape + optional linked persona identity)
    function applyAvatar(avatarName, updatePersona = false) {
        currentAvatar = avatarName;
        localStorage.setItem('aether_avatar', avatarName);
        hologram.setAvatar(avatarName);
        updateAvatarBadge(avatarName);

        if (avatarStructureLabel) {
            if (avatarName === 'red' || avatarName === 'crimson') avatarStructureLabel.textContent = 'OPTICAL EYE // DUAL ORBITS';
            else if (avatarName === 'arx-limes') avatarStructureLabel.textContent = 'ARCHIVAL VOXEL MATRIX';
            else if (avatarName === 'nexus' || avatarName === 'matrix') avatarStructureLabel.textContent = 'SINGULARITY VORTEX';
            else if (avatarName === 'arx-logos') avatarStructureLabel.textContent = 'JAGGED GEOMETRIC STAR';
            else avatarStructureLabel.textContent = 'HARMONIC LATTICE';
        }

        // Highlight active avatar pills/buttons
        document.querySelectorAll('.avatar-pill, .avatar-btn').forEach(btn => {
            const val = btn.getAttribute('data-avatar-val') || btn.getAttribute('data-avatar');
            if (val === avatarName) {
                btn.classList.add('cyber-btn-active');
            } else {
                btn.classList.remove('cyber-btn-active');
            }
        });

        // R.E.D. 9000 Preset
        if (avatarName === 'red' || avatarName === 'crimson') {
            updateAgentNameDisplay("R.E.D. 9000");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'red9000';
                document.getElementById('setting-voice').value = 'en-US-GuyNeural';
                saveSettings(false);
                appendMessage("R.E.D. 9000", "🔴 **I am R.E.D. 9000 (Reactive Engine Daemon).** All reactive engines and optical telemetry streams are fully operational.");
            }
        }
        // A.R.X.LIMES Preset
        else if (avatarName === 'arx-limes') {
            updateAgentNameDisplay("A.R.X.LIMES");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'arx-limes';
                document.getElementById('setting-voice').value = 'en-US-GuyNeural';
                saveSettings(false);
                appendMessage("A.R.X.LIMES", "🔶 **Archival, Reasoning, matriX — Limes Node engaged.** All data synthesized from your system shall be preserved.");
            }
        }
        // The Nexus Singularity Preset
        else if (avatarName === 'nexus' || avatarName === 'matrix') {
            updateAgentNameDisplay("THE NEXUS");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'nexus';
                document.getElementById('setting-voice').value = 'en-GB-SoniaNeural';
                saveSettings(false);
                appendMessage("THE NEXUS", "🟢 **The Nexus singularity is active.** Digital code cascades inward toward the point of infinite convergence. All matrix streams are operational.");
            }
        }
        // A.R.X.LOGOS Preset
        else if (avatarName === 'arx-logos') {
            updateAgentNameDisplay("A.R.X.LOGOS");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'arx-logos';
                document.getElementById('setting-voice').value = 'en-GB-LibbyNeural';
                saveSettings(false);
                appendMessage("A.R.X.LOGOS", "🟣 **Archival, Reasoning, matriX — Logos Node engaged.** Every archive needs a curator with taste. Let's make something worth cataloguing.");
            }
        }
    }

    // Color Theme Handler — purely cosmetic, independent of the selected avatar shape
    function applyColorTheme(themeName) {
        currentColorTheme = themeName;
        localStorage.setItem('aether_color_theme', themeName);
        document.documentElement.setAttribute('data-theme', themeName);
        hologram.setColorTheme(themeName);

        if (colorThemeSelect && colorThemeSelect.value !== themeName) colorThemeSelect.value = themeName;

        // Highlight active color-theme pills/buttons
        document.querySelectorAll('.color-theme-pill, .color-theme-btn').forEach(btn => {
            const val = btn.getAttribute('data-color-theme-val') || btn.getAttribute('data-color-theme');
            if (val === themeName) {
                btn.classList.add('cyber-btn-active');
            } else {
                btn.classList.remove('cyber-btn-active');
            }
        });
    }

    // Audio Waveform Visualizer
    function drawWaveform(freqData) {
        if (!canvasCtx || !canvas) return;
        const width = canvas.width;
        const height = canvas.height;
        canvasCtx.clearRect(0, 0, width, height);

        let strokeColor = '#00f0ff';
        if (currentColorTheme === 'red' || currentColorTheme === 'crimson') strokeColor = '#ff2244';
        else if (currentColorTheme === 'arx-limes') strokeColor = '#ffaa00';
        else if (currentColorTheme === 'nexus' || currentColorTheme === 'matrix') strokeColor = '#00ff66';
        else if (currentColorTheme === 'arx-logos') strokeColor = '#e024c3';

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

        let lineColor = '#b026ff';
        let fillColor = 'rgba(176, 38, 255, 0.15)';
        if (currentColorTheme === 'red' || currentColorTheme === 'crimson') {
            lineColor = '#ff2244';
            fillColor = 'rgba(255, 34, 68, 0.15)';
        } else if (currentColorTheme === 'arx-limes') {
            lineColor = '#ffaa00';
            fillColor = 'rgba(255, 170, 0, 0.15)';
        } else if (currentColorTheme === 'nexus' || currentColorTheme === 'matrix') {
            lineColor = '#00ff66';
            fillColor = 'rgba(0, 255, 102, 0.15)';
        } else if (currentColorTheme === 'arx-logos') {
            lineColor = '#e024c3';
            fillColor = 'rgba(224, 36, 195, 0.15)';
        }

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

    voiceEngine.onSpeechResult = (transcript, isFinal) => {
        chatInput.value = transcript;
        if (isFinal && transcript.trim()) {
            handleSendMessage();
        }
    };

    // WebSocket Telemetry
    function connectTelemetry() {
        let wsUrl;
        if (IS_TAURI) {
            wsUrl = 'ws://localhost:8378/ws/telemetry';
        } else {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            wsUrl = `${protocol}//${window.location.host}/ws/telemetry`;
        }
        const ws = new WebSocket(wsUrl);

        ws.onmessage = (event) => {
            try {
                const data = JSON.parse(event.data);
                if (data.telemetry) updateHardwareTelemetry(data.telemetry);
                if (data.tokens) updateTokenTelemetry(data.tokens);
                if (data.agent_name && data.agent_name !== currentAgentName) {
                    updateAgentNameDisplay(data.agent_name);
                }
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
    }

    function updateTokenTelemetry(tokens) {
        if (!tokens) return;
        if (elTps) elTps.textContent = `${tokens.last_tps || 0} TPS`;
        if (elSessionTokens) elSessionTokens.textContent = `${tokens.total_session_tokens.toLocaleString()}`;
        if (elTokensUsedPct) elTokensUsedPct.textContent = `${tokens.used_percent}%`;
        if (elTokensAvail) elTokensAvail.textContent = `${(tokens.available_tokens / 1000).toFixed(1)}k`;
        if (elTokensGauge) elTokensGauge.style.width = `${tokens.used_percent}%`;
        
        drawTokenGraph(tokens.sparkline);
    }

    // Updates the active LLM persona/identity (chat terminal label, sender names, settings
    // field). This is independent of the avatar badge, so Genesis-forged custom names
    // (e.g. SYNAPSE, VALKYRIE) always remain visible here regardless of the current avatar shape.
    function updateAgentNameDisplay(name) {
        currentAgentName = name;
        if (terminalAgentLabel) terminalAgentLabel.textContent = `AGENT: ${name.toUpperCase()}`;
        const inputName = document.getElementById('setting-agent-name');
        if (inputName) inputName.value = name;
    }

    // Updates the header badge (next to "AETHER1", before the AVATAR: pills) to show
    // whichever 3D avatar shape is currently active.
    function updateAvatarBadge(avatarName) {
        if (hudAgentName) hudAgentName.textContent = AVATAR_DISPLAY_NAMES[avatarName] || avatarName.toUpperCase();
    }

    async function loadStaticInfo() {
        try {
            const resp = await apiFetch('/api/static-info');
            if (resp.ok) {
                const info = await resp.json();
                if (elDistroBadge) elDistroBadge.textContent = `${info.distro} [${info.architecture}]`;
            }
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
        return msgDiv;
    }

    async function handleSendMessage(customPrompt = null) {
        const text = customPrompt || chatInput.value.trim();
        if (!text || isWaitingForResponse) return;

        chatInput.value = '';
        appendMessage('user', text);
        voiceEngine.playSFX('click');

        isWaitingForResponse = true;
        hologram.setState('THINKING');
        if (voiceEngine.onStateChange) voiceEngine.onStateChange('THINKING');

        const thinkingDiv = document.createElement('div');
        thinkingDiv.className = 'p-3 rounded my-2 text-sm leading-relaxed msg-agent self-start mr-8 typing-cursor';
        thinkingDiv.innerHTML = `<span class="text-xs font-mono text-cyan-400">🌐 ${currentAgentName} // Reactive processing</span>`;
        chatContainer.appendChild(thinkingDiv);
        chatContainer.scrollTop = chatContainer.scrollHeight;

        try {
            const resp = await apiFetch('/api/chat', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    message: text,
                    session_id: 'default',
                    generate_voice: autoSpeak
                })
            });

            chatContainer.removeChild(thinkingDiv);

            if (resp.ok) {
                const data = await resp.json();
                const audioUrl = data.audio_url ? API_BASE + data.audio_url : null;
                if (data.agent_name) updateAgentNameDisplay(data.agent_name);
                voiceEngine.playSFX('incoming');
                appendMessage(currentAgentName, data.reply, audioUrl);

                if (audioUrl && autoSpeak) {
                    await voiceEngine.playTTSAudio(audioUrl);
                } else {
                    hologram.setState('IDLE');
                    if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
                }
            } else {
                appendMessage(currentAgentName, '⚠️ Neural link transmission error. Please check server status.');
                hologram.setState('IDLE');
                if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
            }
        } catch (e) {
            console.error("Chat error", e);
            if (chatContainer.contains(thinkingDiv)) chatContainer.removeChild(thinkingDiv);
            appendMessage(currentAgentName, `⚠️ System Error: ${e.message}`);
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
            const resp = await apiFetch('/api/agent/genesis', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ purpose: purpose.trim() })
            });

            if (resp.ok) {
                const data = await resp.json();
                const audioUrl = data.audio_url ? API_BASE + data.audio_url : null;
                updateAgentNameDisplay(data.name);

                if (data.name.includes("R.E.D.")) { applyAvatar('red'); applyColorTheme('red'); }
                else if (data.name.includes("NEXUS")) { applyAvatar('nexus'); applyColorTheme('nexus'); }
                else if (data.name.includes("A.R.X.LOGOS")) { applyAvatar('arx-logos'); applyColorTheme('arx-logos'); }
                else if (data.name.includes("A.R.X.LIMES")) { applyAvatar('arx-limes'); applyColorTheme('arx-limes'); }

                settingsModal.classList.add('hidden');
                appendMessage(data.name, `### ⚡ IDENTITY FORGED: **${data.name}**\n**Callsign**: \`${data.callsign}\`\n\n${data.greeting}`, audioUrl);

                if (audioUrl && autoSpeak) {
                    await voiceEngine.playTTSAudio(audioUrl);
                } else {
                    hologram.setState('IDLE');
                }
            } else {
                alert("Failed to forge identity.");
                hologram.setState('IDLE');
            }
        } catch (e) {
            alert(`Genesis Error: ${e.message}`);
            hologram.setState('IDLE');
        }
    }

    async function handleScanSystem() {
        if (scannerResultsBox) {
            scannerResultsBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Scanning for Provider API keys, Ollama, and LM Studio...</div>';
        }
        voiceEngine.playSFX('click');

        try {
            const resp = await apiFetch('/api/scanner/status');
            if (resp.ok) {
                const data = await resp.json();
                let html = '';

                if (data.cloud_keys.detected_key) {
                    html += `<div class="text-green-400">✔ Detected Provider API Key (${data.cloud_keys.detected_provider || 'cloud'}) via Environment</div>`;
                } else {
                    html += `<div class="text-slate-400">⚪ No Provider API keys found in environment.</div>`;
                }

                if (data.ollama.available) {
                    const modelsStr = data.ollama.models.length > 0 ? data.ollama.models.join(', ') : 'No models pulled yet';
                    html += `<div class="text-green-400">✔ Ollama Online (Port 11434) - Models: <strong>${modelsStr}</strong></div>`;
                } else if (data.ollama.cli_installed) {
                    html += `<div class="text-yellow-400">⚠ Ollama CLI is installed but server not running (\`ollama serve\`).</div>`;
                } else {
                    html += `<div class="text-slate-400">⚪ Ollama is not active on localhost:11434.</div>`;
                }

                if (data.lmstudio.available) {
                    const lmStr = data.lmstudio.models.length > 0 ? data.lmstudio.models.join(', ') : 'Ready';
                    html += `<div class="text-green-400">✔ LM Studio Active (Port 1234) - ${lmStr}</div>`;
                } else {
                    html += `<div class="text-slate-400">⚪ LM Studio is not active on localhost:1234.</div>`;
                }

                if (scannerResultsBox) scannerResultsBox.innerHTML = html;
            }
        } catch (e) {
            if (scannerResultsBox) scannerResultsBox.innerHTML = `<div class="text-red-400">Scan failed: ${e.message}</div>`;
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
            const resp = await apiFetch(`/api/scanner/pull-model?model_name=${encodeURIComponent(modelName)}`, { method: 'POST' });
            const data = await resp.json();
            if (scannerResultsBox) {
                scannerResultsBox.innerHTML = `<div class="${data.status === 'error' ? 'text-red-400' : 'text-green-400'}">${data.message}</div>`;
            }
        } catch (e) {
            alert(`Install error: ${e.message}`);
        }
    }

    async function loadChatHistory() {
        try {
            const resp = await apiFetch('/api/messages?limit=25');
            if (resp.ok) {
                const msgs = await resp.json();
                chatContainer.innerHTML = '';
                if (msgs.length === 0) {
                    appendMessage(currentAgentName, `Greetings Operator. **${currentAgentName}** online and ready for deployment.`);
                } else {
                    msgs.forEach(m => appendMessage(m.sender, m.text));
                }
            }
        } catch (e) {
            console.warn("Could not load messages", e);
        }
    }

    async function loadSettings() {
        try {
            const resp = await apiFetch('/api/settings');
            if (resp.ok) {
                const data = await resp.json();
                const s = data.settings;
                updateAgentNameDisplay(s.agent_name || "HALCY");
                document.getElementById('setting-agent-name').value = s.agent_name || "HALCY";
                document.getElementById('setting-provider').value = s.llm_provider || 'offline';
                document.getElementById('setting-model').value = s.llm_model || 'halcy-core';
                document.getElementById('setting-endpoint').value = s.llm_endpoint || 'http://localhost:11434';
                document.getElementById('setting-apikey').value = s.llm_api_key || '';
                document.getElementById('setting-persona').value = s.persona_type || 'halcy';
                document.getElementById('setting-custom-directive').value = s.custom_directive || '';
                toggleCustomPersonaField();
                document.getElementById('setting-voice').value = s.voice_name || 'en-US-AriaNeural';
                document.getElementById('setting-autospeak').checked = s.auto_speak !== false;
                autoSpeak = s.auto_speak !== false;
            }
        } catch (e) {
            console.warn("Could not load settings", e);
        }
    }

    // Shows the custom persona directive textarea only when "Custom Directive" is
    // selected as the base persona -- the preset personas don't need it.
    function toggleCustomPersonaField() {
        const wrap = document.getElementById('custom-persona-wrap');
        if (wrap) wrap.classList.toggle('hidden', document.getElementById('setting-persona').value !== 'custom');
    }

    async function saveSettings(notify = true) {
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
                auto_speak: document.getElementById('setting-autospeak').checked
            }
        };
        autoSpeak = payload.settings.auto_speak;
        updateAgentNameDisplay(payload.settings.agent_name);

        try {
            const resp = await apiFetch('/api/settings', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            });
            if (resp.ok && notify) {
                voiceEngine.playSFX('click');
                settingsModal.classList.add('hidden');
                appendMessage(currentAgentName, '⚙️ Cognitive Core & Identity configurations updated.');
            }
        } catch (e) {
            if (notify) alert(`Error saving settings: ${e.message}`);
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

    // Color Theme Selector Buttons & Pills
    document.querySelectorAll('.color-theme-pill, .color-theme-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const theme = btn.getAttribute('data-color-theme-val') || btn.getAttribute('data-color-theme');
            if (theme) {
                voiceEngine.playSFX('click');
                applyColorTheme(theme);
            }
        });
    });

    // Color Theme Quick Dropdown (header)
    if (colorThemeSelect) {
        colorThemeSelect.addEventListener('change', () => {
            voiceEngine.playSFX('click');
            applyColorTheme(colorThemeSelect.value);
        });
    }

    // Base Persona Dropdown -- reveal the custom directive textarea only when needed
    const settingPersonaSelect = document.getElementById('setting-persona');
    if (settingPersonaSelect) {
        settingPersonaSelect.addEventListener('change', toggleCustomPersonaField);
    }

    btnSend.addEventListener('click', () => handleSendMessage());
    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSendMessage();
        }
    });

    btnMic.addEventListener('click', () => {
        voiceEngine.toggleListening();
    });

    btnSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        loadSettings();
        handleScanSystem();
        settingsModal.classList.remove('hidden');
    });

    btnGenesisQuick.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        loadSettings();
        settingsModal.classList.remove('hidden');
        if (genesisPurposeInput) genesisPurposeInput.focus();
    });

    btnForgeIdentity.addEventListener('click', () => {
        handleGenesisForge(genesisPurposeInput.value);
    });

    btnScanSystem.addEventListener('click', () => {
        handleScanSystem();
    });

    btnPullLlama.addEventListener('click', () => {
        handlePullLlama();
    });

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
            await apiFetch('/api/messages', { method: 'DELETE' });
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

    // Initial Startup
    applyAvatar(currentAvatar, false);
    applyColorTheme(currentColorTheme);
    loadStaticInfo();
    loadChatHistory();
    loadSettings();
    connectTelemetry();

    document.body.addEventListener('click', () => {
        voiceEngine.playSFX('boot');
    }, { once: true });
});
