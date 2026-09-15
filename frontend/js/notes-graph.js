/* The picture of your notes: every note a dot, every [[link]] a line.
 *
 * This file draws and nothing else. It is handed a graph -- the same
 * {nodes, edges, partial} the backend already builds for the backlinks --
 * and a callback for "the operator clicked this note". It does no fetching
 * and knows nothing about Tauri or HTTP, so the one place that decides how
 * to talk to the backend stays app.js.
 *
 * The layout is a small force simulation: links pull the notes they join
 * together, every note pushes its neighbours away, and a weak pull towards
 * the middle stops the whole thing drifting off the canvas. That is the
 * whole of it. It is not trying to be a graph-theory tool -- it is trying to
 * show you, at a glance, which notes are the hubs and which are floating on
 * their own, because on a vault that a companion has been writing into for
 * months those two things are genuinely hard to see in a list.
 *
 * The loop stops. A settled graph draws nothing, a hidden graph draws
 * nothing, and a closed one is torn down -- the same discipline the avatar
 * canvas follows, and for the same reason: this runs on a machine that is
 * also running a language model, and a picture nobody is looking at has no
 * business holding a core at 100%.
 *
 * Positions are seeded deterministically, so opening the graph twice in a
 * row gives you the same picture rather than a new arrangement to relearn. */
(function () {
    'use strict';

    /* Force constants, in world units. A "world unit" is roughly a pixel at
       zoom 1; the view transform is applied at draw time, never to the
       simulation, so zooming never changes how the graph settles. */
    const SPRING_LEN = 70;      /* how far apart a linked pair wants to sit */
    const SPRING_K = 0.035;     /* how hard the link pulls */
    const REPEL_RADIUS = 110;   /* beyond this, two notes ignore each other  */
    const REPEL_K = 900;        /* how hard they push inside that radius     */
    /* Two notes that start almost on top of each other are a divide-by-nearly-
       nothing, and without a ceiling the pair leaves at a speed no spring can
       reverse before the simulation cools -- which on a big vault shows up as a
       single hub sitting a thousand pixels from everything it links to. The cap
       is the whole fix: crowded notes still separate, they just cannot be
       launched. */
    const REPEL_MAX = 8;
    const CENTER_K = 0.006;     /* the weak pull that keeps the cloud on screen */
    const DAMPING = 0.82;
    const MAX_STEP = 12;        /* px per tick; stops a dense cluster exploding */

    /* The simulation cools. alpha scales every force, so the graph moves a lot
       at first and then stops moving at all -- which is also how the render
       loop knows it is allowed to stop.
    
       The step is a fixed 60th of a second and the loop runs as many of them as
       the elapsed time asks for, rather than one per frame. Tying the physics to
       the frame rate would mean the graph settled in two seconds on a 144Hz
       screen and twenty on a throttled tab, and settled into a *different*
       arrangement in each case. */
    const ALPHA_START = 1;
    const ALPHA_DECAY = 0.975;
    const ALPHA_MIN = 0.006;
    const STEP_MS = 1000 / 60;
    /* After a stall -- a dragged window, a busy model -- catch up by at most this
       many steps and then let the clock go. Trying to replay a lost second of
       simulation in one frame just stalls it again. */
    const MAX_STEPS_PER_FRAME = 4;

    /* The graph is laid out a good way before it is ever painted, so opening it
       shows you a picture rather than a knot of dots untangling itself. The rest
       of the settling is animated, because watching the last of it move is how
       you see which notes are pulling on which. A big vault gets fewer of these:
       on two thousand notes they are the difference between opening instantly
       and appearing to hang. */
    /* Roughly the spacing a settled graph ends up with, used to seed the
       opening spiral at about its final density. */
    const SEED_SPACING = 40;

    const PRESETTLE_STEPS = 140;
    const PRESETTLE_STEPS_BIG = 50;
    const BIG_GRAPH = 600;

    const MIN_ZOOM = 0.15;
    const MAX_ZOOM = 4;
    const DRAG_THRESHOLD_PX = 4;
    /* Above this many notes, labels are drawn only for the hubs and whatever
       the pointer is over: a thousand overlapping filenames is not a label,
       it is a grey rectangle. */
    const LABEL_ALL_BELOW = 60;

    /* A folder gets a colour of its own so the shape of the vault reads at a
       glance -- daily/ one colour, projects/ another. Derived from the name
       rather than assigned from a list, because the folders are the
       operator's to invent and a fixed list would run out. */
    function folderHue(folder) {
        if (!folder) return 188;            /* the vault root: HUD cyan */
        let h = 0;
        for (let i = 0; i < folder.length; i++) {
            h = (h * 31 + folder.charCodeAt(i)) >>> 0;
        }
        return h % 360;
    }

    /* The canvas cannot inherit a colour, so it has to go and read the theme.
       Every one of these is a variable the themes already redefine (see
       css/A1theme.css), which is why the graph changes colour with the rest of
       the HUD instead of staying HUD-cyan on a white Solar background where
       nothing would be legible. */
    function readTheme(el) {
        const cs = getComputedStyle(el);
        const pick = (name, fallback) => (cs.getPropertyValue(name) || '').trim() || fallback;
        const ground = pick('--bg-core', '#040711');
        return {
            text: pick('--text-main', '#e2f1ff'),
            dim: pick('--text-dim', '#7da5c9'),
            accent: pick('--text-accent', '#00f0ff'),
            core: pick('--neon-amber', '#ffaa00'),
            /* A light theme needs dark ink and darker dots; a dark one needs the
               opposite. Decided from the ground's own brightness rather than
               from a theme name, so a theme added later gets it right for free. */
            light: luminance(ground) > 0.5,
        };
    }

    function luminance(color) {
        const m = /^#?([0-9a-f]{6})$/i.exec(color.trim());
        if (m) {
            const v = parseInt(m[1], 16);
            return (0.2126 * ((v >> 16) & 255) + 0.7152 * ((v >> 8) & 255) + 0.0722 * (v & 255)) / 255;
        }
        const rgb = /rgba?\(([^)]+)\)/.exec(color);
        if (rgb) {
            const parts = rgb[1].split(',').map(Number);
            return (0.2126 * parts[0] + 0.7152 * parts[1] + 0.0722 * parts[2]) / 255;
        }
        return 0;
    }

    /* Deterministic scatter: same vault, same opening picture. */
    function seededUnit(i) {
        const x = Math.sin((i + 1) * 12.9898) * 43758.5453;
        return x - Math.floor(x);
    }

    function mount(canvas, options) {
        const opts = options || {};
        const onOpenNote = typeof opts.onOpenNote === 'function' ? opts.onOpenNote : function () {};
        const ctx = canvas.getContext('2d');

        let nodes = [];
        let edges = [];
        let index = new Map();      /* note name -> node */
        let running = false;
        let frame = 0;
        let alpha = 0;
        let dirty = false;          /* something changed that is not motion */

        let view = { x: 0, y: 0, k: 1 };
        let hover = null;
        let theme = { text: '#e2f1ff', dim: '#7da5c9', accent: '#00f0ff', core: '#ffaa00', light: false };
        /* Once, after the layout stops moving: frame the graph. Doing it before
           it has settled would frame a cloud that is still collapsing. */
        let pendingFit = false;
        let lastTime = 0;
        let debt = 0;
        let dragNode = null;
        let panning = false;
        let pointerStart = null;
        let moved = 0;

        /* ---- geometry -------------------------------------------------- */

        function sizeToBox() {
            const dpr = window.devicePixelRatio || 1;
            const rect = canvas.getBoundingClientRect();
            const w = Math.max(1, Math.round(rect.width * dpr));
            const h = Math.max(1, Math.round(rect.height * dpr));
            if (canvas.width !== w || canvas.height !== h) {
                canvas.width = w;
                canvas.height = h;
                dirty = true;
            }
            return { w: rect.width, h: rect.height, dpr };
        }

        function toWorld(clientX, clientY) {
            const rect = canvas.getBoundingClientRect();
            return {
                x: (clientX - rect.left - view.x) / view.k,
                y: (clientY - rect.top - view.y) / view.k,
            };
        }

        function nodeAt(clientX, clientY) {
            const p = toWorld(clientX, clientY);
            let best = null;
            let bestDist = Infinity;
            for (const n of nodes) {
                const dx = p.x - n.x;
                const dy = p.y - n.y;
                const d = dx * dx + dy * dy;
                /* A generous grab radius: these dots are small, and missing by
                   two pixels and panning the whole graph instead is horrible. */
                const reach = (n.r + 6) * (n.r + 6);
                if (d < reach && d < bestDist) { best = n; bestDist = d; }
            }
            return best;
        }

        /* ---- the simulation -------------------------------------------- */

        /* Repulsion between every pair would be n² -- four million sums a tick
           on a big vault. Instead each note is dropped into a grid cell the
           size of its own reach, and only notes in the nine cells around it
           can push it. Beyond that radius the force was rounding to nothing
           anyway, so this changes the arithmetic and not the picture. */
        function repel() {
            const cell = REPEL_RADIUS;
            const buckets = new Map();
            for (const n of nodes) {
                const key = Math.floor(n.x / cell) + ':' + Math.floor(n.y / cell);
                let b = buckets.get(key);
                if (!b) { b = []; buckets.set(key, b); }
                b.push(n);
            }
            for (const n of nodes) {
                const cx = Math.floor(n.x / cell);
                const cy = Math.floor(n.y / cell);
                for (let gx = cx - 1; gx <= cx + 1; gx++) {
                    for (let gy = cy - 1; gy <= cy + 1; gy++) {
                        const b = buckets.get(gx + ':' + gy);
                        if (!b) continue;
                        for (const m of b) {
                            if (m === n) continue;
                            let dx = n.x - m.x;
                            let dy = n.y - m.y;
                            let d2 = dx * dx + dy * dy;
                            if (d2 > REPEL_RADIUS * REPEL_RADIUS) continue;
                            if (d2 < 0.01) {
                                /* Two notes exactly on top of each other have no
                                   direction to separate along. Give them one. */
                                dx = seededUnit(n.i) - 0.5;
                                dy = seededUnit(n.i + 7) - 0.5;
                                d2 = 0.01;
                            }
                            const d = Math.sqrt(d2);
                            const f = Math.min(REPEL_MAX, REPEL_K / d2) * alpha;
                            n.vx += (dx / d) * f;
                            n.vy += (dy / d) * f;
                        }
                    }
                }
            }
        }

        function tick() {
            repel();
            for (const e of edges) {
                const a = e.a;
                const b = e.b;
                const dx = b.x - a.x;
                const dy = b.y - a.y;
                const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
                const f = (d - SPRING_LEN) * SPRING_K * alpha;
                const ux = (dx / d) * f;
                const uy = (dy / d) * f;
                a.vx += ux; a.vy += uy;
                b.vx -= ux; b.vy -= uy;
            }
            for (const n of nodes) {
                if (n === dragNode) { n.vx = 0; n.vy = 0; continue; }
                n.vx -= n.x * CENTER_K * alpha;
                n.vy -= n.y * CENTER_K * alpha;
                n.vx *= DAMPING;
                n.vy *= DAMPING;
                n.vx = Math.max(-MAX_STEP, Math.min(MAX_STEP, n.vx));
                n.vy = Math.max(-MAX_STEP, Math.min(MAX_STEP, n.vy));
                n.x += n.vx;
                n.y += n.vy;
            }
            alpha *= ALPHA_DECAY;
        }

        /* ---- drawing ---------------------------------------------------- */

        function draw() {
            const box = sizeToBox();
            ctx.setTransform(box.dpr, 0, 0, box.dpr, 0, 0);
            ctx.clearRect(0, 0, box.w, box.h);
            ctx.save();
            ctx.translate(view.x, view.y);
            ctx.scale(view.k, view.k);

            const near = hover ? neighbours(hover) : null;

            ctx.lineWidth = 1 / view.k;
            ctx.strokeStyle = theme.accent;
            for (const e of edges) {
                const lit = hover && (e.a === hover || e.b === hover);
                /* Alpha rather than a paler colour, so the link stays whatever
                   colour the theme's accent is and only its weight changes. */
                ctx.globalAlpha = lit ? 0.9 : (theme.light ? 0.3 : 0.18);
                ctx.beginPath();
                ctx.moveTo(e.a.x, e.a.y);
                ctx.lineTo(e.b.x, e.b.y);
                ctx.stroke();
            }
            ctx.globalAlpha = 1;

            for (const n of nodes) {
                const lit = !hover || n === hover || (near && near.has(n));
                ctx.globalAlpha = lit ? 1 : 0.28;
                ctx.beginPath();
                ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2);
                const light = theme.light ? (n.core ? 46 : 38) : (n.core ? 68 : 56);
                ctx.fillStyle = `hsl(${n.hue} ${theme.light ? 70 : 80}% ${light}%)`;
                ctx.fill();
                /* The three notes it reads before every single answer get a ring.
                   A stray line in one of those changes every reply it gives, so
                   they are worth being able to pick out of the cloud. */
                if (n.core) {
                    ctx.lineWidth = 2 / view.k;
                    ctx.strokeStyle = theme.core;
                    ctx.stroke();
                    ctx.lineWidth = 1 / view.k;
                }
                if (n === hover) {
                    ctx.lineWidth = 2 / view.k;
                    ctx.strokeStyle = theme.text;
                    ctx.stroke();
                    ctx.lineWidth = 1 / view.k;
                }
            }
            ctx.globalAlpha = 1;

            /* Labels last, so nothing is drawn over them. */
            /* On a small vault every note is named. On a big one the names are
               drawn only once you have zoomed in far enough for them not to sit
               on top of each other -- a thousand overlapping filenames is not a
               label, it is a grey smudge -- plus whichever one the pointer is
               over, which is how you read a dense graph. */
            const labelAll = nodes.length <= LABEL_ALL_BELOW || view.k >= 1.6;
            const pad = 40 / view.k;
            const left = -view.x / view.k - pad;
            const top = -view.y / view.k - pad;
            const right = left + box.w / view.k + pad * 2;
            const bottom = top + box.h / view.k + pad * 2;
            /* Divided by the zoom, so a label is the same size on screen however
               far in you are: the point of zooming is to separate the dots, not
               to grow the text. */
            ctx.font = `${11 / view.k}px ui-monospace, monospace`;
            ctx.textAlign = 'center';
            ctx.textBaseline = 'top';
            ctx.lineJoin = 'round';
            ctx.lineWidth = 3 / view.k;
            ctx.strokeStyle = theme.light ? 'rgba(255,255,255,0.85)' : 'rgba(4, 7, 17, 0.85)';
            for (const n of nodes) {
                if (n !== hover && !labelAll) continue;
                /* Text costs far more to draw than a dot; skip the ones that
                   would land outside the canvas entirely. */
                if (n.x < left || n.x > right || n.y < top || n.y > bottom) continue;
                const y = n.y + n.r + 3 / view.k;
                /* The ground's own colour, painted around the glyphs first, so a
                   name crossing a link is still readable. */
                ctx.strokeText(n.label, n.x, y);
                ctx.fillStyle = n === hover ? theme.text : theme.dim;
                ctx.fillText(n.label, n.x, y);
            }
            ctx.lineWidth = 1 / view.k;
            ctx.restore();
        }

        function neighbours(n) {
            const set = new Set([n]);
            for (const e of edges) {
                if (e.a === n) set.add(e.b);
                else if (e.b === n) set.add(e.a);
            }
            return set;
        }

        /* ---- the loop --------------------------------------------------- */

        function loop() {
            if (!running) return;
            /* A tab nobody is looking at, or a canvas with no layout box
               because the graph is behind the reader: draw nothing, and keep
               the frame cheap rather than stopping, so coming back is instant. */
            if (document.hidden || canvas.offsetParent === null) {
                lastTime = performance.now();
                debt = 0;
                frame = requestAnimationFrame(loop);
                return;
            }
            if (alpha > ALPHA_MIN) {
                const now = performance.now();
                debt += Math.min(200, now - lastTime);
                lastTime = now;
                let steps = 0;
                while (debt >= STEP_MS && steps < MAX_STEPS_PER_FRAME && alpha > ALPHA_MIN) {
                    debt -= STEP_MS;
                    steps++;
                    tick();
                }
                if (debt > STEP_MS * MAX_STEPS_PER_FRAME) debt = 0;
                draw();
            } else if (dirty) {
                dirty = false;
                draw();
            } else if (pendingFit) {
                pendingFit = false;
                fit();
            } else {
                /* Settled and untouched. Stop; any interaction calls kick(). */
                running = false;
                frame = 0;
                return;
            }
            frame = requestAnimationFrame(loop);
        }

        function kick(heat) {
            if (typeof heat === 'number') alpha = Math.max(alpha, heat);
            dirty = true;
            if (!running) {
                running = true;
                /* Starting from now, not from whenever the loop last stopped:
                   otherwise the first frame back tries to pay off the whole
                   time the graph spent closed. */
                lastTime = performance.now();
                debt = 0;
                frame = requestAnimationFrame(loop);
            }
        }

        /* ---- input ------------------------------------------------------ */

        function onPointerDown(ev) {
            canvas.setPointerCapture(ev.pointerId);
            pointerStart = { x: ev.clientX, y: ev.clientY };
            moved = 0;
            const hit = nodeAt(ev.clientX, ev.clientY);
            if (hit) {
                dragNode = hit;
            } else {
                panning = true;
            }
        }

        function onPointerMove(ev) {
            if (pointerStart) {
                moved = Math.max(moved, Math.abs(ev.clientX - pointerStart.x) +
                                        Math.abs(ev.clientY - pointerStart.y));
            }
            if (dragNode) {
                const p = toWorld(ev.clientX, ev.clientY);
                dragNode.x = p.x;
                dragNode.y = p.y;
                kick(0.35);
                return;
            }
            if (panning) {
                view.x += ev.movementX;
                view.y += ev.movementY;
                kick();
                return;
            }
            const hit = nodeAt(ev.clientX, ev.clientY);
            if (hit !== hover) {
                hover = hit;
                canvas.style.cursor = hit ? 'pointer' : 'grab';
                kick();
            }
        }

        function onPointerUp(ev) {
            const wasNode = dragNode;
            dragNode = null;
            panning = false;
            pointerStart = null;
            if (canvas.hasPointerCapture(ev.pointerId)) canvas.releasePointerCapture(ev.pointerId);
            /* A press that did not travel is a click, not a drag. */
            if (wasNode && moved < DRAG_THRESHOLD_PX) onOpenNote(wasNode.name);
        }

        function onWheel(ev) {
            ev.preventDefault();
            const rect = canvas.getBoundingClientRect();
            const mx = ev.clientX - rect.left;
            const my = ev.clientY - rect.top;
            const before = { x: (mx - view.x) / view.k, y: (my - view.y) / view.k };
            const step = ev.deltaY < 0 ? 1.12 : 1 / 1.12;
            view.k = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, view.k * step));
            /* Zoom about the pointer: the note under the cursor stays put. */
            view.x = mx - before.x * view.k;
            view.y = my - before.y * view.k;
            kick();
        }

        canvas.addEventListener('pointerdown', onPointerDown);
        canvas.addEventListener('pointermove', onPointerMove);
        canvas.addEventListener('pointerup', onPointerUp);
        canvas.addEventListener('pointercancel', onPointerUp);
        canvas.addEventListener('wheel', onWheel, { passive: false });
        canvas.style.cursor = 'grab';

        const observer = typeof ResizeObserver === 'function'
            ? new ResizeObserver(() => { center(); kick(); })
            : null;
        if (observer) observer.observe(canvas);

        function center() {
            const rect = canvas.getBoundingClientRect();
            /* The simulation is centred on the origin, so centring the view is
               just putting the origin in the middle of the box. */
            view.x = rect.width / 2;
            view.y = rect.height / 2;
        }

        /* ---- api --------------------------------------------------------- */

        function show(graph) {
            const raw = (graph && graph.nodes) || [];
            nodes = raw.map((n, i) => {
                /* A spiral rather than a ring: a ring of 500 notes starts as a
                   single hard circle that takes an age to relax inwards. */
                const angle = i * 2.399963;         /* the golden angle */
                /* Spaced so the starting cloud is already about as dense as a
                   settled one. Packing nine hundred notes into the room for
                   nine and letting repulsion sort it out is what blows a graph
                   apart in its first second. */
                const radius = SEED_SPACING * Math.sqrt(i + 1);
                return {
                    i,
                    name: n.name,
                    label: (n.name || '').replace(/\.md$/i, ''),
                    folder: n.folder || '',
                    core: !!n.core,
                    links_in: n.links_in || 0,
                    hue: folderHue(n.folder || ''),
                    r: Math.min(16, 4 + Math.sqrt(n.links_in || 0) * 2.4),
                    x: Math.cos(angle) * radius + (seededUnit(i) - 0.5) * 6,
                    y: Math.sin(angle) * radius + (seededUnit(i + 101) - 0.5) * 6,
                    vx: 0,
                    vy: 0,
                };
            });
            index = new Map(nodes.map((n) => [n.name, n]));
            edges = [];
            for (const e of (graph && graph.edges) || []) {
                const a = index.get(e.from);
                const b = index.get(e.to);
                /* An edge whose other end is not in the node list has nothing to
                   join. That happens when the scan was truncated, and dropping
                   it here is why `partial` is worth showing the operator. */
                if (a && b && a !== b) edges.push({ a, b });
            }
            hover = null;
            dragNode = null;
            view.k = 1;
            theme = readTheme(canvas);
            center();
            alpha = ALPHA_START;
            const presettle = nodes.length > BIG_GRAPH ? PRESETTLE_STEPS_BIG : PRESETTLE_STEPS;
            for (let i = 0; i < presettle && alpha > ALPHA_MIN; i++) tick();
            fit();
            pendingFit = true;
            kick();
        }

        function stop() {
            running = false;
            if (frame) cancelAnimationFrame(frame);
            frame = 0;
        }

        function resume() {
            /* Coming back to a settled graph: one draw is all it needs -- but the
               theme may have been changed while it was away. */
            theme = readTheme(canvas);
            kick();
        }

        function fit() {
            if (!nodes.length) { center(); kick(); return; }
            let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
            for (const n of nodes) {
                minX = Math.min(minX, n.x - n.r); maxX = Math.max(maxX, n.x + n.r);
                minY = Math.min(minY, n.y - n.r); maxY = Math.max(maxY, n.y + n.r);
            }
            const rect = canvas.getBoundingClientRect();
            const pad = 40;
            const kx = (rect.width - pad) / Math.max(1, maxX - minX);
            const ky = (rect.height - pad) / Math.max(1, maxY - minY);
            view.k = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.min(kx, ky)));
            view.x = rect.width / 2 - ((minX + maxX) / 2) * view.k;
            view.y = rect.height / 2 - ((minY + maxY) / 2) * view.k;
            kick();
        }

        function destroy() {
            stop();
            if (observer) observer.disconnect();
            canvas.removeEventListener('pointerdown', onPointerDown);
            canvas.removeEventListener('pointermove', onPointerMove);
            canvas.removeEventListener('pointerup', onPointerUp);
            canvas.removeEventListener('pointercancel', onPointerUp);
            canvas.removeEventListener('wheel', onWheel);
        }

        return { show, stop, resume, fit, destroy };
    }

    window.Aether1NoteGraph = { mount };
})();
