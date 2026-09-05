/* Movable HUD panels: drag a panel by its grip to move it, drag a divider to
 * resize the columns. Both are remembered in localStorage.
 *
 * Panels stay inside the three-column grid on purpose -- see css/layout.css for
 * why free x/y positioning is the wrong trade here.
 *
 * Nothing in this file talks to the backend. The layout is a property of this
 * screen, not of the companion, so localStorage is the right home for it: it
 * survives restarts, and a corrupt or missing value costs the operator the
 * default layout rather than an error. */
(function () {
    'use strict';

    const STORE_KEY = 'aether_hud_layout';
    const DEFAULT_WIDTHS = [3, 4, 5];
    /* A column narrower than this is not a column, it is a sliver you cannot
       read and cannot easily grab your way out of. */
    const MIN_COLUMN_PX = 140;
    /* Pointer travel before a press counts as a drag rather than a click. */
    const DRAG_THRESHOLD_PX = 4;
    /* Not zero: a zero-fraction track and a track holding a panel behave
       differently under `fr`, and this keeps the arithmetic in one shape. */
    const COLLAPSED_FR = 0.001;

    const layout = document.getElementById('hud-layout');
    if (!layout) return;

    const columns = Array.from(layout.querySelectorAll('.hud-column'));
    const resizers = Array.from(layout.querySelectorAll('.col-resizer'));

    /* Where each panel lives when nothing has been customised. Captured from the
       markup before any saved layout is applied, so the defaults are whatever the
       HTML says today -- adding a panel to index.html needs no change here. */
    const defaultHome = new Map();
    columns.forEach((column, index) => {
        column.querySelectorAll(':scope > [data-panel]').forEach((panel) => {
            defaultHome.set(panel.dataset.panel, index);
        });
    });

    function panelsOf(column) {
        return Array.from(column.querySelectorAll(':scope > [data-panel]'));
    }

    function allPanels() {
        return Array.from(layout.querySelectorAll('[data-panel]'));
    }

    /* The width an emptied column had before it collapsed, so dropping a panel
       back into it restores the size the operator chose rather than a default. */
    const stashedWidth = new Array(columns.length).fill(null);

    function columnWidth(i) {
        const raw = getComputedStyle(layout).getPropertyValue(`--col-${i}`).trim();
        return parseFloat(raw) || DEFAULT_WIDTHS[i];
    }

    function markEmptyColumns() {
        columns.forEach((column, i) => {
            const empty = panelsOf(column).length === 0;
            const wasEmpty = column.classList.contains('is-empty');
            column.classList.toggle('is-empty', empty);
            if (empty && !wasEmpty) {
                /* An empty column keeps its full share of the window otherwise,
                   which looks like a bug rather than a place to drop something.
                   Near-zero here, with the min-width in the stylesheet holding it
                   open enough to aim at. */
                stashedWidth[i] = columnWidth(i);
                layout.style.setProperty(`--col-${i}`, `${COLLAPSED_FR}fr`);
            } else if (!empty && wasEmpty) {
                layout.style.setProperty(`--col-${i}`, `${stashedWidth[i] ?? DEFAULT_WIDTHS[i]}fr`);
                stashedWidth[i] = null;
            }
        });
    }

    // ---- Persistence --------------------------------------------------------

    function save() {
        const state = {
            version: 1,
            columns: columns.map((column) => panelsOf(column).map((p) => p.dataset.panel)),
            widths: columns.map((column, i) => stashedWidth[i] ?? columnWidth(i)),
        };
        try {
            localStorage.setItem(STORE_KEY, JSON.stringify(state));
        } catch (err) {
            /* A full or disabled store costs the operator a remembered layout and
               nothing else, so it is not worth interrupting them over. */
            console.warn('Could not save the HUD layout:', err);
        }
    }

    function load() {
        let state;
        try {
            state = JSON.parse(localStorage.getItem(STORE_KEY) || 'null');
        } catch (err) {
            return null;
        }
        if (!state || state.version !== 1 || !Array.isArray(state.columns)) return null;
        return state;
    }

    function applyWidths(widths) {
        widths.forEach((w, i) => {
            stashedWidth[i] = null;
            layout.style.setProperty(`--col-${i}`, `${w}fr`);
        });
    }

    function applySaved() {
        const state = load();
        if (!state) return;

        if (Array.isArray(state.widths) && state.widths.length === columns.length &&
            state.widths.every((w) => Number.isFinite(w) && w > 0)) {
            applyWidths(state.widths);
        }

        const byId = new Map(allPanels().map((p) => [p.dataset.panel, p]));
        const placed = new Set();
        state.columns.forEach((ids, columnIndex) => {
            const column = columns[columnIndex];
            if (!column || !Array.isArray(ids)) return;
            ids.forEach((id) => {
                const panel = byId.get(id);
                /* An id in the saved layout that no longer exists in the markup is
                   simply skipped -- an old layout must never be able to break a
                   newer HUD. */
                if (!panel || placed.has(id)) return;
                column.appendChild(panel);
                placed.add(id);
            });
        });

        /* The mirror case: a panel added to the HUD since this layout was saved.
           It goes to the column the markup asks for rather than disappearing. */
        allPanels().forEach((panel) => {
            const id = panel.dataset.panel;
            if (placed.has(id)) return;
            const home = columns[defaultHome.get(id) ?? 0];
            if (home) home.appendChild(panel);
        });

        markEmptyColumns();
    }

    function resetLayout() {
        try {
            localStorage.removeItem(STORE_KEY);
        } catch (err) { /* nothing to undo */ }
        applyWidths(DEFAULT_WIDTHS);
        allPanels().forEach((panel) => {
            const home = columns[defaultHome.get(panel.dataset.panel) ?? 0];
            if (home) home.appendChild(panel);
        });
        /* Re-sort each column back into the order the markup declares. */
        columns.forEach((column, index) => {
            const order = Array.from(defaultHome.entries())
                .filter(([, home]) => home === index)
                .map(([id]) => id);
            order.forEach((id) => {
                const panel = layout.querySelector(`[data-panel="${CSS.escape(id)}"]`);
                if (panel) column.appendChild(panel);
            });
        });
        markEmptyColumns();
        settle();
    }

    /* Things that need a nudge after a panel changes place: moving a node in the
       DOM keeps its content but resets its scroll position, and a WebGL canvas
       needs to hear that its box changed. */
    function settle() {
        const messages = document.getElementById('chat-messages');
        if (messages) messages.scrollTop = messages.scrollHeight;
        window.dispatchEvent(new Event('resize'));
    }

    // ---- Dragging a panel ---------------------------------------------------

    let drag = null;
    const marker = document.createElement('div');
    marker.className = 'drop-marker';

    function beginDrag(panel, event) {
        drag = { panel, startX: event.clientX, startY: event.clientY, active: false };
    }

    function activateDrag() {
        drag.active = true;
        drag.panel.classList.add('is-dragging');
        document.body.classList.add('layout-dragging');
        showMarkerFor(drag.panel.parentElement, drag.panel);
    }

    /* Put the marker where the panel would land if released now. */
    function showMarkerFor(column, before) {
        if (!column) return;
        if (before) column.insertBefore(marker, before);
        else column.appendChild(marker);
        markEmptyColumns();
    }

    function updateDropTarget(x, y) {
        /* Aim by horizontal distance to each column rather than strict
           containment, so a pointer over a divider or past the last column still
           has an obvious answer instead of the marker vanishing. */
        let best = null;
        let bestDistance = Infinity;
        columns.forEach((column) => {
            const rect = column.getBoundingClientRect();
            const distance = x < rect.left ? rect.left - x : (x > rect.right ? x - rect.right : 0);
            if (distance < bestDistance) {
                bestDistance = distance;
                best = column;
            }
        });
        if (!best) return;

        const siblings = panelsOf(best).filter((p) => p !== drag.panel);
        const landAbove = siblings.find((panel) => {
            const rect = panel.getBoundingClientRect();
            return y < rect.top + rect.height / 2;
        });
        showMarkerFor(best, landAbove || null);
    }

    function endDrag(commit) {
        if (!drag) return;
        const { panel, active } = drag;
        drag = null;
        panel.classList.remove('is-dragging');
        document.body.classList.remove('layout-dragging');
        if (!active) return;

        if (commit && marker.parentElement) {
            marker.parentElement.insertBefore(panel, marker);
        }
        if (marker.parentElement) marker.remove();
        markEmptyColumns();
        settle();
        if (commit) save();
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
        updateDropTarget(event.clientX, event.clientY);
    });

    layout.addEventListener('pointerup', () => endDrag(true));
    layout.addEventListener('pointercancel', () => endDrag(false));

    document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape' && drag) endDrag(false);
    });

    // ---- Moving a panel from the keyboard -----------------------------------
    // The grips are buttons, so they can be tabbed to; without this they would be
    // focusable controls that do nothing, which is worse than no control at all.

    function movePanel(panel, dx, dy) {
        const column = panel.parentElement;
        const columnIndex = columns.indexOf(column);
        if (columnIndex === -1) return false;

        if (dx) {
            const target = columns[columnIndex + dx];
            if (!target) return false;
            target.appendChild(panel);
        } else {
            const siblings = panelsOf(column);
            const at = siblings.indexOf(panel);
            const to = at + dy;
            if (to < 0 || to >= siblings.length) return false;
            if (dy < 0) column.insertBefore(panel, siblings[to]);
            else column.insertBefore(panel, siblings[to].nextSibling);
        }
        markEmptyColumns();
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

    // ---- Resizing the columns -----------------------------------------------

    let resize = null;

    function currentWidthsPx() {
        return columns.map((column) => column.getBoundingClientRect().width);
    }

    resizers.forEach((resizer, index) => {
        resizer.addEventListener('pointerdown', (event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            resizer.setPointerCapture(event.pointerId);
            resizer.classList.add('is-resizing');
            resize = { index, startX: event.clientX, widths: currentWidthsPx() };
        });

        resizer.addEventListener('pointermove', (event) => {
            if (!resize || resize.index !== index) return;
            const dx = event.clientX - resize.startX;
            const widths = resize.widths.slice();
            /* A divider only ever trades width between the two columns it sits
               between, so the third column never moves under the operator. */
            const left = widths[index] + dx;
            const right = widths[index + 1] - dx;
            if (left < MIN_COLUMN_PX || right < MIN_COLUMN_PX) return;
            widths[index] = left;
            widths[index + 1] = right;
            applyWidths(widths);
        });

        function stop() {
            if (!resize || resize.index !== index) return;
            resize = null;
            resizer.classList.remove('is-resizing');
            settle();
            save();
        }
        resizer.addEventListener('pointerup', stop);
        resizer.addEventListener('pointercancel', stop);

        resizer.addEventListener('dblclick', () => {
            applyWidths(DEFAULT_WIDTHS);
            settle();
            save();
        });

        /* Keyboard equivalent, 24px a press. */
        resizer.addEventListener('keydown', (event) => {
            const step = event.key === 'ArrowLeft' ? -24 : (event.key === 'ArrowRight' ? 24 : 0);
            if (!step) return;
            event.preventDefault();
            const widths = currentWidthsPx();
            const left = widths[index] + step;
            const right = widths[index + 1] - step;
            if (left < MIN_COLUMN_PX || right < MIN_COLUMN_PX) return;
            widths[index] = left;
            widths[index + 1] = right;
            applyWidths(widths);
            settle();
            save();
        });
    });

    // ---- Wiring -------------------------------------------------------------

    const resetButton = document.getElementById('btn-reset-layout');
    if (resetButton) resetButton.addEventListener('click', resetLayout);

    applySaved();
    markEmptyColumns();

    /* Exposed for the settings panel and for anything that needs to know the
       boxes moved. */
    window.AetherLayout = { reset: resetLayout };
})();
