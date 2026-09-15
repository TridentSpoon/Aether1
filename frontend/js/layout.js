/* Movable HUD panels, Android-home-screen style: drag a panel by its grip to
 * move it anywhere on the grid, drag its corner handle to resize it. Both are
 * remembered in localStorage.
 *
 * Panels never move in the DOM -- their on-screen position is entirely the
 * --gcol/--grow/--gw/--gh custom properties this file sets on each one (see
 * css/layout.css, which turns those into a CSS Grid placement). That keeps
 * "where does this panel live" and "what does the panel contain" independent:
 * a panel can be repositioned by touching four numbers, nothing else.
 *
 * Nothing in this file talks to the backend. The layout is a property of this
 * screen, not of the companion, so localStorage is the right home for it: it
 * survives restarts, and a corrupt or missing value costs the operator the
 * default layout rather than an error. */
(function () {
    'use strict';

    const STORE_KEY = 'aether_hud_layout';
    const STORE_VERSION = 2;

    /* Keep these in sync with the custom properties #hud-layout sets in
       css/layout.css -- the drag/resize math converts pointer pixels to
       cells using these same numbers. */
    const COLS = 12;
    const ROW_PX = 24;
    const GAP_PX = 10;

    /* A panel smaller than this is not a panel, it is a sliver you cannot
       read and cannot easily grab your way out of. */
    const MIN_W = 2;
    const MIN_H = 4;
    /* Pointer travel before a press counts as a drag rather than a click. */
    const DRAG_THRESHOLD_PX = 4;

    const layout = document.getElementById('hud-layout');
    if (!layout) return;
    /* Solo-panel windows show exactly one panel and none of the grid chrome
       around it -- dragging, resizing and persistence all assume a real
       multi-panel grid that doesn't exist here. */
    if (document.documentElement.hasAttribute('data-solo-panel')) return;

    const panels = Array.from(layout.querySelectorAll(':scope > [data-panel]'));

    /* Where and how big each panel is when nothing has been customised.
       Captured from the markup before any saved layout is applied, so the
       defaults are whatever the HTML says today -- adding a panel to
       index.html needs no change here. */
    const defaults = new Map();
    panels.forEach((panel) => {
        defaults.set(panel.dataset.panel, {
            col: parseInt(panel.dataset.gridCol, 10) || 1,
            row: parseInt(panel.dataset.gridRow, 10) || 1,
            w: parseInt(panel.dataset.gridW, 10) || 4,
            h: parseInt(panel.dataset.gridH, 10) || 12,
        });
    });

    /* Live placement, keyed by panel id. This is the single source of truth
       for where things are; the custom properties on each panel are only a
       reflection of it. */
    const state = new Map();
    const byId = new Map(panels.map((p) => [p.dataset.panel, p]));

    /* Panels switched off in Settings. Held as ids rather than as a class on the
       element so the grid can ask "is this one on the screen at all" without
       reading the DOM in the middle of a drag. A panel in here is display:none
       (see .panel-off in css/layout.css), which means it is genuinely not
       running: a hidden WebGL canvas has no layout box, and animate.js skips a
       frame for a canvas with no layout box. */
    const off = new Set();

    function apply(panel) {
        const rect = state.get(panel.dataset.panel);
        if (!rect) return;
        panel.style.setProperty('--gcol', rect.col);
        panel.style.setProperty('--grow', rect.row);
        panel.style.setProperty('--gw', rect.w);
        panel.style.setProperty('--gh', rect.h);
    }

    function applyAll() {
        panels.forEach(apply);
    }

    function collides(a, b) {
        return a.col < b.col + b.w && b.col < a.col + a.w &&
               a.row < b.row + b.h && b.row < a.row + a.h;
    }

    function overlapsAny(rect, excludeId) {
        for (const [id, other] of state) {
            if (id === excludeId) continue;
            /* A panel that is switched off is not on the screen, so it cannot be
               in the way of one that is. Without this, dragging around a HUD with
               half its panels off would keep bouncing off empty space. */
            if (off.has(id)) continue;
            if (collides(rect, other)) return true;
        }
        return false;
    }

    function clampCol(col, w) {
        return Math.max(1, Math.min(col, COLS - w + 1));
    }

    /* Nearest free spot to (col, row) for a panel of size w x h, searching
       outward ring by ring. The grid has no bottom edge (rows are implicit),
       so unlike columns this can always find something eventually. */
    function findFreeSpot(col, row, w, h, excludeId) {
        col = clampCol(col, w);
        row = Math.max(1, row);
        if (!overlapsAny({ col, row, w, h }, excludeId)) return { col, row };

        const maxRadius = COLS + 8;
        for (let r = 1; r <= maxRadius; r++) {
            for (let dy = -r; dy <= r; dy++) {
                for (let dx = -r; dx <= r; dx++) {
                    if (Math.max(Math.abs(dx), Math.abs(dy)) !== r) continue;
                    const candCol = clampCol(col + dx, w);
                    const candRow = Math.max(1, row + dy);
                    const cand = { col: candCol, row: candRow, w, h };
                    if (!overlapsAny(cand, excludeId)) return cand;
                }
            }
        }
        return null;
    }

    /* The largest w/h no bigger than the requested size that fits at
       (col, row) without overlapping anything else. */
    function resolveResize(col, row, wTarget, hTarget, excludeId) {
        let w = wTarget;
        while (w > MIN_W && overlapsAny({ col, row, w, h: hTarget }, excludeId)) w--;
        let h = hTarget;
        while (h > MIN_H && overlapsAny({ col, row, w, h }, excludeId)) h--;
        return { w, h };
    }

    // ---- Persistence --------------------------------------------------------

    function save() {
        const out = {};
        for (const [id, rect] of state) out[id] = rect;
        try {
            /* Stored as the list of panels switched OFF rather than the list left
               on. A panel added to the HUD in a later version is then on by
               default for somebody with a saved layout, instead of silently
               missing because their stored "on" list was written before it
               existed. */
            localStorage.setItem(STORE_KEY, JSON.stringify({
                version: STORE_VERSION,
                panels: out,
                off: Array.from(off),
            }));
        } catch (err) {
            /* A full or disabled store costs the operator a remembered layout and
               nothing else, so it is not worth interrupting them over. */
            console.warn('Could not save the HUD layout:', err);
        }
    }

    function load() {
        let parsed;
        try {
            parsed = JSON.parse(localStorage.getItem(STORE_KEY) || 'null');
        } catch (err) {
            return null;
        }
        if (!parsed || parsed.version !== STORE_VERSION || typeof parsed.panels !== 'object' || !parsed.panels) {
            return null;
        }
        return parsed;
    }

    function isValidRect(rect) {
        return rect && Number.isFinite(rect.col) && Number.isFinite(rect.row) &&
               Number.isFinite(rect.w) && Number.isFinite(rect.h) &&
               rect.w >= MIN_W && rect.h >= MIN_H;
    }

    function seedDefaults() {
        for (const [id, rect] of defaults) state.set(id, { ...rect });
    }

    function applySaved() {
        seedDefaults();
        const stored = load();
        const saved = stored && stored.panels;
        /* The off-set first: the placement pass below asks overlapsAny which
           panels are in the way, and a panel switched off is not in the way. */
        off.clear();
        if (stored && Array.isArray(stored.off)) {
            stored.off.filter((id) => byId.has(id)).forEach((id) => off.add(id));
        }
        applyVisibility();
        if (saved) {
            /* An id in the saved layout that no longer exists in the markup is
               simply skipped -- an old layout must never be able to break a
               newer HUD. Anything malformed falls back to that panel's default
               rather than corrupting the grid. */
            for (const id of state.keys()) {
                const rect = saved[id];
                if (!isValidRect(rect)) continue;
                state.set(id, {
                    col: clampCol(Math.round(rect.col), Math.round(rect.w)),
                    row: Math.max(1, Math.round(rect.row)),
                    w: Math.max(MIN_W, Math.min(COLS, Math.round(rect.w))),
                    h: Math.max(MIN_H, Math.round(rect.h)),
                });
            }
        }
        applyAll();
        renderToggles();
    }

    // ---- Which panels exist at all ------------------------------------------

    function applyVisibility() {
        panels.forEach((panel) => {
            panel.classList.toggle('panel-off', off.has(panel.dataset.panel));
        });
    }

    /* The human name for a panel, for the checkbox beside it. Falls back to the
       id, which is at least something, rather than to an empty label. */
    function panelLabel(panel) {
        return panel.dataset.panelName || panel.dataset.panel;
    }

    function setPanelOn(id, on) {
        if (on) off.delete(id); else off.add(id);
        applyVisibility();
        save();
        settle();
    }

    /* One checkbox per panel, built from the markup rather than from a list kept
       here -- a panel added to index.html turns up in this list without anyone
       having to remember to write it down twice. */
    function renderToggles() {
        const host = document.getElementById('panel-toggles');
        if (!host) return;
        host.textContent = '';
        panels.forEach((panel) => {
            const id = panel.dataset.panel;
            const row = document.createElement('label');
            row.className = 'flex items-center gap-2 cursor-pointer';
            const box = document.createElement('input');
            box.type = 'checkbox';
            box.checked = !off.has(id);
            box.className = 'rounded bg-slate-900 border-cyan-500 text-cyan-400 focus:ring-0';
            box.addEventListener('change', () => setPanelOn(id, box.checked));
            const text = document.createElement('span');
            text.className = 'text-[11px] font-mono text-slate-300';
            text.textContent = panelLabel(panel);
            row.appendChild(box);
            row.appendChild(text);
            host.appendChild(row);
        });
    }

    function resetLayout() {
        try {
            localStorage.removeItem(STORE_KEY);
        } catch (err) { /* nothing to undo */ }
        seedDefaults();
        /* Reset means the HUD as shipped, which includes every panel being on --
           a reset that left a panel switched off would look like it had not
           worked to the one person most likely to be pressing it. */
        off.clear();
        applyVisibility();
        applyAll();
        renderToggles();
        settle();
    }

    /* Things that need a nudge after a panel changes place or size: a WebGL
       canvas needs to hear that its box changed. */
    function settle() {
        window.dispatchEvent(new Event('resize'));
    }

    // ---- Grid geometry --------------------------------------------------------

    function metrics() {
        const rect = layout.getBoundingClientRect();
        const cs = getComputedStyle(layout);
        const padLeft = parseFloat(cs.paddingLeft) || 0;
        const padTop = parseFloat(cs.paddingTop) || 0;
        const padRight = parseFloat(cs.paddingRight) || 0;
        const contentWidth = rect.width - padLeft - padRight;
        return {
            left: rect.left + padLeft,
            top: rect.top + padTop,
            colPitch: (contentWidth + GAP_PX) / COLS,
            rowPitch: ROW_PX + GAP_PX,
        };
    }

    // ---- Dragging a panel ---------------------------------------------------

    let drag = null;

    function beginDrag(panel, event) {
        const rect = panel.getBoundingClientRect();
        drag = {
            panel,
            id: panel.dataset.panel,
            startX: event.clientX,
            startY: event.clientY,
            grabX: event.clientX - rect.left,
            grabY: event.clientY - rect.top,
            active: false,
            target: null,
        };
    }

    function activateDrag() {
        drag.active = true;
        drag.panel.classList.add('is-dragging');
        document.body.classList.add('layout-dragging');
    }

    function updateDragTarget(x, y) {
        const m = metrics();
        const home = state.get(drag.id);
        const left = x - drag.grabX;
        const top = y - drag.grabY;
        const col = clampCol(Math.round((left - m.left) / m.colPitch) + 1, home.w);
        const row = Math.max(1, Math.round((top - m.top) / m.rowPitch) + 1);
        const spot = findFreeSpot(col, row, home.w, home.h, drag.id);
        drag.target = spot;
        const show = spot || home;
        drag.panel.style.setProperty('--gcol', show.col);
        drag.panel.style.setProperty('--grow', show.row);
    }

    function endDrag(commit) {
        if (!drag) return;
        const { panel, id, active, target } = drag;
        drag = null;
        panel.classList.remove('is-dragging');
        document.body.classList.remove('layout-dragging');
        if (!active) return;

        if (commit && target) {
            const home = state.get(id);
            state.set(id, { ...home, col: target.col, row: target.row });
            save();
        }
        apply(panel);
        settle();
    }

    layout.addEventListener('pointerdown', (event) => {
        const grip = event.target.closest('.panel-grip');
        if (!grip || event.button !== 0) return;
        const panel = grip.closest('[data-panel]');
        if (!panel) return;
        event.preventDefault();
        grip.setPointerCapture(event.pointerId);
        beginDrag(panel, event);
    });

    layout.addEventListener('pointermove', (event) => {
        if (!drag) return;
        if (!drag.active) {
            const moved = Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY);
            if (moved < DRAG_THRESHOLD_PX) return;
            activateDrag();
        }
        updateDragTarget(event.clientX, event.clientY);
    });

    layout.addEventListener('pointerup', () => endDrag(true));
    layout.addEventListener('pointercancel', () => endDrag(false));

    document.addEventListener('keydown', (event) => {
        if (event.key !== 'Escape') return;
        if (drag) endDrag(false);
        if (resize) endResize(false);
    });

    // ---- Moving a panel from the keyboard -----------------------------------
    // The grips are buttons, so they can be tabbed to; without this they would be
    // focusable controls that do nothing, which is worse than no control at all.

    function movePanel(panel, dx, dy) {
        const id = panel.dataset.panel;
        const home = state.get(id);
        const col = clampCol(home.col + dx, home.w);
        const row = Math.max(1, home.row + dy);
        if (col === home.col && row === home.row) return false;
        if (overlapsAny({ col, row, w: home.w, h: home.h }, id)) return false;
        state.set(id, { ...home, col, row });
        apply(panel);
        settle();
        save();
        return true;
    }

    layout.addEventListener('keydown', (event) => {
        const grip = event.target.closest('.panel-grip');
        if (!grip) return;
        const panel = grip.closest('[data-panel]');
        if (!panel) return;
        const moves = {
            ArrowLeft: [-1, 0], ArrowRight: [1, 0],
            ArrowUp: [0, -1], ArrowDown: [0, 1],
        };
        const move = moves[event.key];
        if (!move) return;
        if (movePanel(panel, move[0], move[1])) {
            event.preventDefault();
            grip.focus();
        }
    });

    // ---- Resizing a panel ----------------------------------------------------

    let resize = null;

    function beginResize(panel, event) {
        resize = {
            panel,
            id: panel.dataset.panel,
            startX: event.clientX,
            startY: event.clientY,
            orig: { ...state.get(panel.dataset.panel) },
            active: false,
            pending: null,
        };
    }

    function updateResizeTarget(x, y) {
        const m = metrics();
        const { orig } = resize;
        const dCols = Math.round((x - resize.startX) / m.colPitch);
        const dRows = Math.round((y - resize.startY) / m.rowPitch);
        const wTarget = Math.max(MIN_W, Math.min(orig.w + dCols, COLS - orig.col + 1));
        const hTarget = Math.max(MIN_H, orig.h + dRows);
        const resolved = resolveResize(orig.col, orig.row, wTarget, hTarget, resize.id);
        resize.pending = resolved;
        resize.panel.style.setProperty('--gw', resolved.w);
        resize.panel.style.setProperty('--gh', resolved.h);
    }

    function endResize(commit) {
        if (!resize) return;
        const { panel, id, active, orig, pending } = resize;
        resize = null;
        panel.classList.remove('is-resizing');
        document.body.classList.remove('layout-resizing');
        if (!active) return;

        if (commit && pending) {
            state.set(id, { ...orig, w: pending.w, h: pending.h });
            save();
        }
        apply(panel);
        settle();
    }

    layout.addEventListener('pointerdown', (event) => {
        const handle = event.target.closest('.panel-resize');
        if (!handle || event.button !== 0) return;
        const panel = handle.closest('[data-panel]');
        if (!panel) return;
        event.preventDefault();
        handle.setPointerCapture(event.pointerId);
        beginResize(panel, event);
    });

    layout.addEventListener('pointermove', (event) => {
        if (!resize) return;
        if (!resize.active) {
            const moved = Math.hypot(event.clientX - resize.startX, event.clientY - resize.startY);
            if (moved < DRAG_THRESHOLD_PX) return;
            resize.active = true;
            resize.panel.classList.add('is-resizing');
            document.body.classList.add('layout-resizing');
        }
        updateResizeTarget(event.clientX, event.clientY);
    });

    layout.addEventListener('pointerup', () => endResize(true));
    layout.addEventListener('pointercancel', () => endResize(false));

    // ---- Wiring -------------------------------------------------------------

    const resetButton = document.getElementById('btn-reset-layout');
    if (resetButton) resetButton.addEventListener('click', resetLayout);

    applySaved();
})();
