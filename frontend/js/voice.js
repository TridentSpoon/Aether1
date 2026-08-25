/**
 * Voice & Audio Synthesis Pipeline for Project AETHER / CORTANA.
 * Features:
 * - Web Audio API Sci-Fi Sound Effects Synthesizer
 * - Speech-to-Text (STT) via Web Speech API
 * - Neural TTS Audio Stream Player with Real-Time Frequency Analyser
 */

class VoiceAudioEngine {
    constructor() {
        this.audioCtx = null;
        this.analyser = null;
        this.dataArray = new Uint8Array(64);
        this.currentAudio = null;
        this.recognition = null;
        this.isListening = false;
        this.sfxEnabled = true;

        this.onSpeechResult = null;
        this.onStateChange = null;
        this.onAudioFrequency = null;

        this.initAudioContext();
        this.initSpeechRecognition();
    }

    initAudioContext() {
        try {
            const AudioContext = window.AudioContext || window.webkitAudioContext;
            if (AudioContext) {
                this.audioCtx = new AudioContext();
                this.analyser = this.audioCtx.createAnalyser();
                this.analyser.fftSize = 128;
                this.dataArray = new Uint8Array(this.analyser.frequencyBinCount);
            }
        } catch (e) {
            console.warn("Web Audio API not supported", e);
        }
    }

    initSpeechRecognition() {
        const SpeechRec = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (SpeechRec) {
            this.recognition = new SpeechRec();
            this.recognition.continuous = false;
            this.recognition.interimResults = true;
            this.recognition.lang = 'en-US';

            this.recognition.onstart = () => {
                this.isListening = true;
                this.playSFX('listen_start');
                if (this.onStateChange) this.onStateChange('LISTENING');
            };

            this.recognition.onresult = (event) => {
                let interim = '';
                let finalTranscript = '';
                for (let i = event.resultIndex; i < event.results.length; ++i) {
                    if (event.results[i].isFinal) {
                        finalTranscript += event.results[i][0].transcript;
                    } else {
                        interim += event.results[i][0].transcript;
                    }
                }
                if (this.onSpeechResult) {
                    this.onSpeechResult(finalTranscript || interim, !!finalTranscript);
                }
            };

            this.recognition.onerror = (event) => {
                console.warn("Speech recognition error:", event.error);
                this.isListening = false;
                if (this.onStateChange) this.onStateChange('IDLE');
            };

            this.recognition.onend = () => {
                this.isListening = false;
                this.playSFX('listen_end');
                if (this.onStateChange) this.onStateChange('IDLE');
            };
        } else {
            console.warn("Speech Recognition API not supported in this browser.");
        }
    }

    toggleListening() {
        if (!this.recognition) {
            alert("Speech recognition is not supported in this browser. Please use Chrome/Edge or type your message.");
            return false;
        }

        if (this.audioCtx && this.audioCtx.state === 'suspended') {
            this.audioCtx.resume();
        }

        if (this.isListening) {
            this.recognition.stop();
            return false;
        } else {
            try {
                this.recognition.start();
                return true;
            } catch (e) {
                console.warn("Recognition start failed:", e);
                return false;
            }
        }
    }

    async playTTSAudio(audioUrl) {
        if (this.currentAudio) {
            this.currentAudio.pause();
            this.currentAudio = null;
        }

        if (this.audioCtx && this.audioCtx.state === 'suspended') {
            await this.audioCtx.resume();
        }

        return new Promise((resolve) => {
            const audio = new Audio(audioUrl);
            this.currentAudio = audio;

            if (this.audioCtx && this.analyser) {
                try {
                    const source = this.audioCtx.createMediaElementSource(audio);
                    source.connect(this.analyser);
                    this.analyser.connect(this.audioCtx.destination);
                } catch (e) {
                    // Fallback if CORS or already connected
                }
            }

            if (this.onStateChange) this.onStateChange('SPEAKING');

            // Frequency polling loop
            const pollFrequency = () => {
                if (!this.currentAudio || this.currentAudio.paused) return;
                if (this.analyser) {
                    this.analyser.getByteFrequencyData(this.dataArray);
                    if (this.onAudioFrequency) this.onAudioFrequency(this.dataArray);
                }
                requestAnimationFrame(pollFrequency);
            };

            audio.onplay = () => {
                pollFrequency();
            };

            audio.onended = () => {
                this.currentAudio = null;
                if (this.onStateChange) this.onStateChange('IDLE');
                if (this.onAudioFrequency) this.onAudioFrequency(new Uint8Array(64));
                resolve();
            };

            audio.onerror = () => {
                this.currentAudio = null;
                if (this.onStateChange) this.onStateChange('IDLE');
                resolve();
            };

            audio.play().catch(e => {
                console.warn("Audio playback blocked or failed:", e);
                if (this.onStateChange) this.onStateChange('IDLE');
                resolve();
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
            }
        } catch (e) {
            console.warn("SFX synthesis error:", e);
        }
    }
}

window.VoiceAudioEngine = VoiceAudioEngine;
