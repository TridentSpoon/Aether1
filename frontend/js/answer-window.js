/**
 * The Answer window: the settled reply, by itself, in a window of its own.
 *
 * Trident, 2026-11-07: *"Would it be possible to divert the final decided output to a
 * separate window?"* -- and then *"I also want it to be able to input it's answer into a 3rd
 * party text box of my choosing."* This is both halves. The chat panel keeps everything a
 * turn did: the thinking, the tool calls, the hand-offs, the memory it touched. This window
 * keeps the one thing that was decided.
 *
 * **A mirror, like js/sprite.js and js/face.js.** Nothing is asked from in here and nothing
 * is decided here. The answer, who said it and which model said it all arrive from the HUD
 * over Tauri events (see publishAnswer in js/app.js), because the HUD is the page that
 * rendered the answer in the first place and a second page deciding what the answer was
 * would be a second answer. The two switches at the bottom are the exception, and they are
 * written through a command rather than kept here -- see answer_prefs_rust in main.rs.
 *
 * Native only, like the sprite and the face: a browser tab cannot be a second window of an
 * app it is not part of, and the hand-off needs a process on the machine. Over the LAN
 * server this file is never loaded.
 */

document.addEventListener('DOMContentLoaded', () => {
    const tauri = window.__TAURI__;
    const events = tauri && tauri.event;
    const core = tauri && tauri.core;

    const whoEl = document.getElementById('answer-who');
    const modelEl = document.getElementById('answer-model');
    const whenEl = document.getElementById('answer-when');
    const bodyEl = document.getElementById('answer-body');
    const idleEl = document.getElementById('answer-idle');
    const textEl = document.getElementById('answer-text');
    const typeBtn = document.getElementById('answer-type');
    const copyBtn = document.getElementById('answer-copy');
    const flattenBox = document.getElementById('answer-flatten');
    const divertBox = document.getElementById('answer-divert');
    const noteEl = document.getElementById('answer-note');

    /* The answer as the model wrote it, markdown and all. Kept apart from what is on screen
       because everything this window *does* with an answer -- type it, copy it -- wants the
       source text, not the HTML it was drawn as. */
    let answerText = '';
    /* Whether the display server will take synthetic keystrokes at all. Asked once, because
       it is a property of the session, and remembered so the reason can be repeated every
       time the button is pressed rather than printed once at startup and scrolled past. */
    let reach = null;
    let handoffEnabled = false;
    let countdown = null;

    /* How long between pressing the button and the first keystroke.
       Long enough to click into another window without hurrying, short enough that nobody
       walks away from it. The chord (Super+Shift+V by default) has no countdown at all --
       there, the operator is already in the box they mean. */
    const HANDOFF_DELAY = 4;

    function note(message, kind) {
        noteEl.textContent = message || '';
        noteEl.classList.toggle('is-refused', kind === 'refused');
        noteEl.classList.toggle('is-done', kind === 'done');
    }

    /** Draws an answer, streaming or settled. */
    function paint(text, streaming) {
        answerText = text || '';
        if (idleEl) idleEl.hidden = answerText !== '';
        textEl.innerHTML = window.Aether1Markdown
            ? window.Aether1Markdown.render(answerText)
            : '';
        textEl.classList.toggle('is-streaming', !!streaming);
        /* Follow the text while it arrives, and only then: scrolling a settled answer back
           to the bottom is taking the window away from somebody reading it. */
        if (streaming) bodyEl.scrollTop = bodyEl.scrollHeight;
        refreshButtons();
    }

    function refreshButtons() {
        const have = answerText.trim() !== '';
        copyBtn.disabled = !have;
        typeBtn.disabled = !have || !handoffEnabled || (reach !== null && !reach.ok);
    }

    /* Why the Type button is off, when it is off. Three different reasons and three
       different ways out, so they are not collapsed into one sentence: nothing to send yet,
       the operator has not switched the hand-off on, or this display server refuses
       synthetic input (see handoff::availability). */
    function explainIfDisabled() {
        if (answerText.trim() === '') return note('There is no answer here yet.');
        if (!handoffEnabled) {
            return note(
                'Typing into other applications is switched off. Settings → Display turns it on.',
                'refused'
            );
        }
        if (reach && !reach.ok) return note(reach.why, 'refused');
        return false;
    }

    function stopCountdown() {
        if (countdown) clearInterval(countdown);
        countdown = null;
        typeBtn.classList.remove('is-counting');
        typeBtn.textContent = 'Type into a text box';
    }

    async function typeItNow() {
        try {
            const typed = await core.invoke('answer_handoff_rust', {
                text: answerText,
                flatten: !!flattenBox.checked
            });
            const short = typed < answerText.length;
            note(
                short
                    ? `Typed the first ${typed} characters — the rest was too long to send this way.`
                    : `Typed ${typed} characters into the focused window.`,
                'done'
            );
        } catch (e) {
            note(String(e && e.message ? e.message : e), 'refused');
        }
    }

    /* The button, which is the half of the hand-off for people who would rather not learn a
       chord. Pressing it means this window has the focus, so it cannot type straight away --
       it would type into itself. The countdown is the gap in which the operator clicks into
       the box they actually mean, and pressing the button again inside that gap cancels. */
    typeBtn.addEventListener('click', () => {
        if (countdown) {
            stopCountdown();
            return note('Cancelled.');
        }
        if (explainIfDisabled() !== false) return;

        let left = HANDOFF_DELAY;
        typeBtn.classList.add('is-counting');
        typeBtn.textContent = 'Cancel';
        note(`Click into the text box you want it in — typing in ${left}…`);
        countdown = setInterval(() => {
            left -= 1;
            if (left > 0) {
                note(`Click into the text box you want it in — typing in ${left}…`);
                return;
            }
            stopCountdown();
            note('Typing…');
            typeItNow();
        }, 1000);
    });

    copyBtn.addEventListener('click', async () => {
        try {
            await navigator.clipboard.writeText(answerText);
            note('Copied.', 'done');
        } catch (e) {
            // A webview that refuses the clipboard is not a dead end, and saying so is more
            // use than the exception: the text is on screen and can be selected.
            note('This window could not reach the clipboard — select the answer and copy it.', 'refused');
        }
    });

    /* Both switches are remembered rather than asked again next time, because each is a
       standing answer about how this operator works: which kind of box they usually send
       text to, and whether they want the chat to keep a copy. The HUD is told as well as
       the database, because the chat has to stop showing the answer *now* rather than after
       the next restart. */
    function remember(pref) {
        core.invoke('answer_prefs_rust', pref).catch((e) =>
            console.warn('Could not remember that preference', e)
        );
        if (events) events.emit('answer-prefs-changed', pref).catch(() => {});
    }

    flattenBox.addEventListener('change', () => {
        remember({ flatten: flattenBox.checked });
        note(
            flattenBox.checked
                ? 'Line breaks will be sent as spaces.'
                : 'Line breaks will be sent as they are written.'
        );
    });

    divertBox.addEventListener('change', () => {
        remember({ divert: divertBox.checked });
        note(
            divertBox.checked
                ? 'The chat will keep the working-out, and the answer only appears here.'
                : 'The chat shows the answer as well.'
        );
    });

    if (!events || !core) {
        // The window exists but has no way to be fed. Said plainly rather than left looking
        // like a companion that has gone quiet.
        note('This window needs the desktop app to feed it.', 'refused');
        return;
    }

    /* What the HUD sends, and the one thing this window sends back.
       `answer-window-ready` is the handshake: the HUD answers it with the last answer and
       the current switches, so a window opened in the middle of a session -- or reopened
       after a restart -- is not blank until the next question. */
    events.listen('answer-begin', (event) => {
        const payload = event.payload || {};
        if (payload.who) whoEl.textContent = String(payload.who).toUpperCase();
        modelEl.textContent = payload.model || '';
        whenEl.textContent = new Date().toLocaleTimeString();
        paint('', true);
        note('');
    }).catch((e) => console.warn('Could not listen for the start of an answer', e));

    events.listen('answer-progress', (event) => {
        paint((event.payload || {}).text || '', true);
    }).catch((e) => console.warn('Could not listen for an answer in progress', e));

    events.listen('answer-settled', (event) => {
        const payload = event.payload || {};
        if (payload.who) whoEl.textContent = String(payload.who).toUpperCase();
        if (payload.model) modelEl.textContent = payload.model;
        whenEl.textContent = payload.at || new Date().toLocaleTimeString();
        paint(payload.text || '', false);
    }).catch((e) => console.warn('Could not listen for the settled answer', e));

    events.listen('answer-prefs', (event) => {
        const payload = event.payload || {};
        if (typeof payload.flatten === 'boolean') flattenBox.checked = payload.flatten;
        if (typeof payload.divert === 'boolean') divertBox.checked = payload.divert;
        if (typeof payload.handoff === 'boolean') handoffEnabled = payload.handoff;
        refreshButtons();
    }).catch((e) => console.warn('Could not listen for the answer preferences', e));

    core.invoke('handoff_reach_rust')
        .then((value) => {
            reach = value;
            refreshButtons();
            /* Said once, at the top, only when it is bad news. On a machine where this
               works there is nothing to report and the line stays empty for the countdown
               to use. */
            if (reach && !reach.ok) note(reach.why, 'refused');
        })
        .catch((e) => console.warn('Could not ask whether typing into other windows works', e));

    events.emit('answer-window-ready', {}).catch((e) =>
        console.warn('Could not tell the HUD this window is open', e)
    );
});
