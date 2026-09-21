/**
 * Voice & Audio Synthesis Pipeline for Project AETHER1.
 * Features:
 * - Web Audio API Sci-Fi Sound Effects Synthesizer
 * - Microphone capture for push-to-talk, transcribed locally (see startCapture below)
 * - Neural TTS Audio Stream Player with Real-Time Frequency Analyser
 *
 * Listening does not use the browser's Web Speech API. In Chrome that is a cloud service:
 * the audio is uploaded to Google and the transcript sent back. Aether1 records here and
 * transcribes with whisper.cpp on this machine instead, so holding the mic key never puts
 * the operator's voice on someone else's server.
 */

class VoiceAudioEngine {
    constructor() {
        this.audioCtx = null;
        this.analyser = null;
        this.dataArray = new Uint8Array(64);
        this.currentAudio = null;
        this.capture = null;
        this.ttsQueue = [];
        this.isDrainingQueue = false;
        this.sfxEnabled = true;

        this.onStateChange = null;
        this.onAudioFrequency = null;

        this.initAudioContext();
    }

    initAudioContext() {
        try {
            const AudioContext = window.AudioContext || window.webkitAudioContext;
            if (AudioContext) {
                this.audioCtx = new AudioContext();
                this.analyser = this.audioCtx.createAnalyser();
                this.analyser.fftSize = 128;
                this.dataArray = new Uint8Array(this.analyser.frequencyBinCount);
                // A gain stage between the analyser and the speakers, wired once here
                // rather than reconnected on every clip. It lets a silent voice self-test
                // (see runVoiceStartupSelfTest in app.js) mute what's actually heard while
                // the analyser -- which taps the signal upstream of this node -- still
                // sees the real decoded audio, so it can tell a genuine synthesis/playback
                // failure from a clip that simply wasn't meant to be heard.
                this.gainNode = this.audioCtx.createGain();
                this.analyser.connect(this.gainNode);
                this.gainNode.connect(this.audioCtx.destination);
            }
        } catch (e) {
            console.warn("Web Audio API not supported", e);
        }
    }

    /**
     * Queues one clip of a streamed reply. Sentences are synthesized as they complete, so
     * they can arrive out of order relative to playback -- this plays them strictly in the
     * order they were queued, and keeps the avatar in SPEAKING across the gaps between
     * clips instead of flickering back to IDLE between every sentence.
     */
    async enqueueTTS(audioUrl) {
        if (!audioUrl) return;
        this.ttsQueue.push(audioUrl);
        if (this.isDrainingQueue) return;

        this.isDrainingQueue = true;
        try {
            while (this.ttsQueue.length) {
                await this.playTTSAudio(this.ttsQueue.shift());
            }
        } finally {
            this.isDrainingQueue = false;
            if (this.onStateChange) this.onStateChange('IDLE');
            if (this.onAudioFrequency) this.onAudioFrequency(new Uint8Array(64));
        }
    }

    /** Drops anything queued but not yet played (a new turn supersedes the old one). */
    stopSpeech() {
        this.ttsQueue = [];
        if (this.currentAudio) {
            this.currentAudio.pause();
            this.currentAudio = null;
        }
    }

    // --- Local push-to-talk ---------------------------------------------------------
    // Hold a key, talk, release. Audio is captured here and assembled into a 16 kHz mono
    // WAV in the page, then posted to Aether1 to be transcribed by a local model. Nothing
    // is sent anywhere -- which is the difference between this and the Web Speech API
    // above, which in most browsers is a cloud service wearing a local-looking API.

    async startCapture() {
        if (this.capture) return true;
        try {
            const stream = await navigator.mediaDevices.getUserMedia({
                audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true }
            });
            const AudioContext = window.AudioContext || window.webkitAudioContext;
            const ctx = new AudioContext();
            const source = ctx.createMediaStreamSource(stream);
            // ScriptProcessor is deprecated but universally available and adequate here;
            // an AudioWorklet would need a separate module file for no practical gain at
            // this sample rate.
            const processor = ctx.createScriptProcessor(4096, 1, 1);
            const chunks = [];

            processor.onaudioprocess = (event) => {
                chunks.push(new Float32Array(event.inputBuffer.getChannelData(0)));
            };
            source.connect(processor);
            processor.connect(ctx.destination);

            this.capture = { stream, ctx, source, processor, chunks, sampleRate: ctx.sampleRate };
            if (this.onStateChange) this.onStateChange('LISTENING');
            this.playSFX('listen_start');
            return true;
        } catch (e) {
            console.warn('microphone unavailable', e);
            return false;
        }
    }

    /** Stops capture and returns the recording as a 16 kHz mono WAV, or null. */
    stopCapture() {
        if (!this.capture) return null;
        const { stream, ctx, source, processor, chunks, sampleRate } = this.capture;
        this.capture = null;

        processor.disconnect();
        source.disconnect();
        stream.getTracks().forEach((track) => track.stop());
        ctx.close();
        this.playSFX('listen_end');
        if (this.onStateChange) this.onStateChange('IDLE');

        const total = chunks.reduce((n, c) => n + c.length, 0);
        if (!total) return null;
        const samples = new Float32Array(total);
        let offset = 0;
        for (const chunk of chunks) { samples.set(chunk, offset); offset += chunk.length; }

        return VoiceAudioEngine.encodeWav(VoiceAudioEngine.resampleTo16k(samples, sampleRate));
    }

    /** Nearest-neighbour downsample to 16 kHz, which is what whisper.cpp expects. */
    static resampleTo16k(samples, sampleRate) {
        const target = 16000;
        if (sampleRate === target) return samples;
        const ratio = sampleRate / target;
        const out = new Float32Array(Math.floor(samples.length / ratio));
        for (let i = 0; i < out.length; i++) out[i] = samples[Math.floor(i * ratio)];
        return out;
    }

    /** 16-bit PCM mono WAV. */
    static encodeWav(samples) {
        const buffer = new ArrayBuffer(44 + samples.length * 2);
        const view = new DataView(buffer);
        const writeText = (offset, text) => {
            for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i));
        };

        writeText(0, 'RIFF');
        view.setUint32(4, 36 + samples.length * 2, true);
        writeText(8, 'WAVE');
        writeText(12, 'fmt ');
        view.setUint32(16, 16, true);        // PCM header size
        view.setUint16(20, 1, true);         // PCM
        view.setUint16(22, 1, true);         // mono
        view.setUint32(24, 16000, true);     // sample rate
        view.setUint32(28, 16000 * 2, true); // byte rate
        view.setUint16(32, 2, true);         // block align
        view.setUint16(34, 16, true);        // bits per sample
        writeText(36, 'data');
        view.setUint32(40, samples.length * 2, true);

        let offset = 44;
        for (const sample of samples) {
            const clamped = Math.max(-1, Math.min(1, sample));
            view.setInt16(offset, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true);
            offset += 2;
        }
        return new Blob([buffer], { type: 'audio/wav' });
    }

    /**
     * Plays one TTS clip and reports what actually happened, not just whether a JS
     * exception was thrown: `signalDetected` (via the analyser, which taps the signal
     * upstream of the gain node so this still works when muted) says whether real,
     * non-zero audio was ever decoded -- distinguishing a clip that genuinely played from
     * one that "succeeded" into a broken output device and produced silence. That gap is
     * exactly what let TTS failures go unnoticed before: `audio.play()` resolving, or
     * `onended` firing, only ever meant no exception was thrown, never that anything was
     * actually heard.
     *
     * `opts.audible` (default true) controls whether the clip is actually heard through
     * the gain node; pass false to run the exact same check silently (see
     * runVoiceStartupSelfTest in app.js). The returned promise always resolves (never
     * rejects) with `{ played, error, signalDetected }` -- `signalDetected` is `null` when
     * there is no analyser to ask (Web Audio unsupported).
     */
    async playTTSAudio(audioUrl, opts = {}) {
        const audible = opts.audible !== false;

        if (this.currentAudio) {
            this.currentAudio.pause();
            this.currentAudio = null;
        }

        if (this.audioCtx && this.audioCtx.state === 'suspended') {
            await this.audioCtx.resume();
        }

        if (this.gainNode) {
            this.gainNode.gain.value = audible ? 1 : 0;
        }

        return new Promise((resolve) => {
            // `crossOrigin` before `src`, and it is load-bearing rather than tidy.
            //
            // This is the whole reason speech was inaudible in the native app. Routing the
            // element through `createMediaElementSource` below means the graph is its *only*
            // output -- the element no longer reaches the speakers by itself. WebKit mutes
            // that node outright when the media would taint the page's origin:
            // MediaElementAudioSourceNode::setFormat does `m_muted = wouldTaintOrigin()`
            // and process() then zeroes its output bus, with no error, no rejected play()
            // and no failed load anywhere. And the media *is* cross-origin here:
            // convertFileSrc hands back `asset://localhost/...` on Linux and macOS (and
            // `http://asset.localhost/...` on Windows) while the page itself is
            // `tauri://localhost`. HTMLMediaElement::taintsOrigin only forgives that when
            // the resource passed a CORS check, which an element with no crossOrigin
            // attribute never attempts.
            //
            // So every engine synthesized correctly, every status field said "installed",
            // the voice test reported that it spoke, and the machine was silent.
            //
            // Tauri's asset protocol already answers with `Access-Control-Allow-Origin` set
            // to the window's origin, so asking for the CORS check is all that was missing.
            // The HTTP transport serves audio from the page's own origin (API_BASE is empty
            // in a browser), where a same-origin request passes regardless and nothing is
            // ever tainted -- so this is safe on both transports and needs no CORS layer on
            // the axum side.
            const audio = new Audio();
            audio.crossOrigin = 'anonymous';
            audio.src = audioUrl;
            this.currentAudio = audio;

            let signalDetected = false;
            let playbackError = null;
            // A byte frequency bin sits at 0-255; real decoded audio -- even a quiet
            // sentence -- clears a few counts somewhere across the spectrum, so this
            // threshold only fails to trip on genuine silence (a muted device, an empty
            // clip, a pipeline that produced nothing).
            const SIGNAL_THRESHOLD = 2;

            if (this.audioCtx && this.analyser) {
                try {
                    const source = this.audioCtx.createMediaElementSource(audio);
                    source.connect(this.analyser);
                } catch (e) {
                    // Only thrown when this element already has a source node -- a fresh
                    // Audio is created per clip above, so this is unreachable in practice.
                    // Note what it does *not* catch: a cross-origin clip does not throw
                    // here, it silently mutes the node, which is what the crossOrigin
                    // assignment above exists to prevent. Leaving the analyser unattached
                    // costs the frequency readout, never the audio.
                }
            }

            if (this.onStateChange) this.onStateChange('SPEAKING');

            // Frequency polling loop
            const pollFrequency = () => {
                if (!this.currentAudio || this.currentAudio.paused) return;
                if (this.analyser) {
                    this.analyser.getByteFrequencyData(this.dataArray);
                    if (!signalDetected) {
                        for (let i = 0; i < this.dataArray.length; i++) {
                            if (this.dataArray[i] > SIGNAL_THRESHOLD) {
                                signalDetected = true;
                                break;
                            }
                        }
                    }
                    if (this.onAudioFrequency) this.onAudioFrequency(this.dataArray);
                }
                requestAnimationFrame(pollFrequency);
            };

            const finish = () => {
                this.currentAudio = null;
                // Mid-queue, the next clip is about to start: staying SPEAKING keeps the
                // avatar steady across the seam. enqueueTTS emits IDLE when it drains.
                if (!this.ttsQueue.length) {
                    if (this.onStateChange) this.onStateChange('IDLE');
                    if (this.onAudioFrequency) this.onAudioFrequency(new Uint8Array(64));
                }
                resolve({
                    played: !playbackError,
                    error: playbackError,
                    signalDetected: this.analyser ? signalDetected : null,
                });
            };

            audio.onplay = () => {
                pollFrequency();
            };

            audio.onended = finish;

            audio.onerror = () => {
                playbackError = (audio.error && audio.error.message) || 'audio element error';
                finish();
            };

            audio.play().catch(e => {
                console.warn("Audio playback blocked or failed:", e);
                playbackError = (e && e.message) || String(e);
                finish();
            });
        });
    }

    /**
     * Synthesize futuristic sci-fi sound effects using Web Audio API oscillators.
     */
    playSFX(type) {
        if (!this.sfxEnabled || !this.audioCtx) return;
        try {
            if (this.audioCtx.state === 'suspended') {
                this.audioCtx.resume();
            }

            const now = this.audioCtx.currentTime;
            const osc = this.audioCtx.createOscillator();
            const gain = this.audioCtx.createGain();

            osc.connect(gain);
            gain.connect(this.audioCtx.destination);

            if (type === 'boot') {
                // Sci-fi boot chime
                osc.type = 'sine';
                osc.frequency.setValueAtTime(440, now);
                osc.frequency.exponentialRampToValueAtTime(880, now + 0.15);
                osc.frequency.exponentialRampToValueAtTime(1760, now + 0.3);
                gain.gain.setValueAtTime(0.2, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.5);
                osc.start(now);
                osc.stop(now + 0.5);
            } else if (type === 'click') {
                // Cyber click
                osc.type = 'triangle';
                osc.frequency.setValueAtTime(1200, now);
                osc.frequency.exponentialRampToValueAtTime(300, now + 0.05);
                gain.gain.setValueAtTime(0.15, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.05);
                osc.start(now);
                osc.stop(now + 0.05);
            } else if (type === 'listen_start') {
                // High futuristic ping
                osc.type = 'sine';
                osc.frequency.setValueAtTime(600, now);
                osc.frequency.exponentialRampToValueAtTime(1200, now + 0.12);
                gain.gain.setValueAtTime(0.2, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.15);
                osc.start(now);
                osc.stop(now + 0.15);
            } else if (type === 'listen_end') {
                // Confirmation tone
                osc.type = 'sine';
                osc.frequency.setValueAtTime(1200, now);
                osc.frequency.exponentialRampToValueAtTime(600, now + 0.12);
                gain.gain.setValueAtTime(0.15, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.15);
                osc.start(now);
                osc.stop(now + 0.15);
            } else if (type === 'incoming') {
                // Incoming AI message chime
                osc.type = 'sine';
                osc.frequency.setValueAtTime(523.25, now); // C5
                osc.frequency.setValueAtTime(659.25, now + 0.08); // E5
                osc.frequency.setValueAtTime(783.99, now + 0.16); // G5
                gain.gain.setValueAtTime(0.15, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.35);
                osc.start(now);
                osc.stop(now + 0.35);
            } else if (type === 'alert') {
                // Approval request: deliberately unlike the message chime. Something is
                // waiting on the operator, and it should not sound like an answer arriving.
                osc.type = 'square';
                osc.frequency.setValueAtTime(880, now);
                osc.frequency.setValueAtTime(660, now + 0.1);
                osc.frequency.setValueAtTime(880, now + 0.2);
                gain.gain.setValueAtTime(0.08, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.32);
                osc.start(now);
                osc.stop(now + 0.32);
            }
        } catch (e) {
            console.warn("SFX synthesis error:", e);
        }
    }
}

window.VoiceAudioEngine = VoiceAudioEngine;
