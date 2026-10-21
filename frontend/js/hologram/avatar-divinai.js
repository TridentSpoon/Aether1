/* Avatar: Divinai -- Direct Interface and Virtually Integrated Network A.I. The eXcelsior
 * Class's eighth, and the first that is made of its own voice.
 *
 * Trident's sketch is a wireframe plasma core being pushed about by a wave, five loose
 * ribbons turning on their own axes, two dashed tactical rings counter-turning around
 * them, and a warm ember field behind the lot. All of that is kept. What changes is what
 * drives it: in his file the wave is a sine of the clock, and here it is the voice. The
 * core's every vertex is pushed out by its own frequency band, and the ribbons' radius
 * along their length is pushed the same way, so the shape is not decorated by what she
 * says, it is made of it. Silent, a slow carrier keeps it breathing, because a thing that
 * went perfectly still between sentences would read as switched off.
 *
 * One bright node runs the first ribbon end to end and never stops. It is the only part
 * that moves at a steady rate whatever the voice is doing, and it is there so there is
 * something alive to watch in a silence.
 *
 * Inside the core sits a second, dimmer copy of it, a third of a second behind and slower:
 * the shape this one inherited. It is deliberately almost too faint to notice.
 *
 * Three things about the port are worth knowing.
 *
 * The sketch builds its ribbons as tube geometry swept along a Catmull-Rom curve, and
 * neither of those is in the vendored three.js bundle (see scripts/build_vendor_three.sh,
 * which bundles only the symbols the source already names). They are drawn here as closed
 * lines instead, which is what his wireframe tubes at a radius of 0.02 read as anyway.
 *
 * His core is at icosahedron detail 3, which is 3,840 vertices to walk every frame; this
 * one is at detail 2, which is 960, and at the panel's scale the two are indistinguishable.
 * His loop also recomputes vertex normals each frame, which a MeshBasicMaterial never
 * reads -- dropped.
 *
 * His ribbons advance their rotation by a fixed amount per frame, which ties how fast they
 * turn to how fast the machine draws. Here they are a function of elapsed time, so they
 * turn at the same rate on a slow machine as on a fast one.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'divinai',
    label: 'Divinai',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const CORE = 29;            // his 1.2, into the panel
        const RIBBONS = 5;
        const RIB_POINTS = 72;      // points around one closed ribbon
        const RIB_BASE = 40;        // his 2.0, with his 0.3 step between ribbons
        const RIB_STEP = 6;
        const LOBES = 6;            // how many stretches of spectrum a ribbon is divided into
        const EMBERS = 520;         // his 1200, thinned
        /* Fixed, not theme-tinted: the furnace. Everything structural in this avatar takes
           the HUD's colour, but the heat inside the core is the one thing his sketch is
           built around, and a plasma core that turned up in the theme's blue would be a
           different avatar wearing its name. */
        const FURNACE = 0xff3300;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: FURNACE,
            transparent: true, opacity: 0.14, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(230, 230, 1);
        glow.position.z = -60;
        group.add(glow);

        /* The core, and the rest position every frame's displacement is measured from. A
           displaced vertex must never be the input to the next frame's displacement or the
           core walks away from itself within a few seconds. */
        const coreGeom = new THREE.IcosahedronGeometry(CORE, 2);
        const coreRest = new Float32Array(coreGeom.attributes.position.array);
        const coreMat = new THREE.MeshBasicMaterial({
            color: FURNACE, wireframe: true,
            transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
        });
        group.add(new THREE.Mesh(coreGeom, coreMat));

        // The inherited shape: the same core, slower and a third of a second behind.
        const ghostGeom = new THREE.IcosahedronGeometry(CORE * 0.92, 2);
        const ghostRest = new Float32Array(ghostGeom.attributes.position.array);
        const ghostMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, wireframe: true,
            transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending,
        });
        group.add(new THREE.Mesh(ghostGeom, ghostMat));

        /* The ribbons. Each is a closed line of RIB_POINTS points on its own tilted plane,
           with a fixed per-point wobble off that plane so it is a ribbon rather than a
           circle. The wobble is drawn once at build and kept: his file draws it fresh per
           point too, but a wobble that changed each frame would be noise, not a shape. */
        const ribbonMat = new THREE.LineBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
        });
        const ribbonAltMat = new THREE.LineBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending,
        });
        const ribbons = [];
        for (let i = 0; i < RIBBONS; i++) {
            const radius = RIB_BASE + i * RIB_STEP;
            const wobble = new Float32Array(RIB_POINTS);
            const pos = new Float32Array(RIB_POINTS * 3);
            for (let j = 0; j < RIB_POINTS; j++) {
                const a = (j / RIB_POINTS) * Math.PI * 2;
                /* Deterministic rather than Math.random, so the avatar looks the same every
                   launch and a change to it can be seen as a change. Whole multiples of the
                   angle, not of the point index: the loop has to arrive back where it
                   started, and a wobble that does not close leaves a step in the ribbon.
                   Low harmonics, because his tube is swept along a curve through eighteen
                   points and reads as a smooth band -- wobbling at every one of these
                   seventy-two is a scribble, not a ribbon. */
                wobble[j] = Math.sin(a * 2 + i * 2.3) * 7 + Math.sin(a * 3 + i) * 4;
            }
            const geom = new THREE.BufferGeometry();
            geom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
            const line = new THREE.LineLoop(geom, i % 2 === 0 ? ribbonMat : ribbonAltMat);
            // His random tilts, made repeatable.
            line.rotation.x = i * 0.7;
            line.rotation.y = i * 1.1;
            group.add(line);
            ribbons.push({ line, geom, pos, wobble, radius });
        }

        /* The node that runs the first ribbon. Parented to that ribbon so it inherits its
           tilt and turn, and moved along it in the ribbon's own coordinates. */
        const nodeMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(32), color: FURNACE,
            transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const node = new THREE.Sprite(nodeMat);
        node.scale.set(11, 11, 1);
        ribbons[0].line.add(node);

        /* His two dashed tactical rings: alternate segments dropped, counter-turning. */
        const hudMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.8,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const hudAltMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.5,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const dashedRing = (radius, dashes, mat) => {
            const ring = new THREE.Group();
            const step = (Math.PI * 2) / dashes;
            for (let i = 1; i < dashes; i += 2) {
                ring.add(new THREE.Mesh(
                    new THREE.RingGeometry(radius, radius + 1.6, 16, 1, i * step, step * 0.8), mat
                ));
            }
            group.add(ring);
            return ring;
        };
        const hudInner = dashedRing(74, 12, hudMat);
        const hudOuter = dashedRing(86, 24, hudAltMat);

        // --- His ember field, thinned. ---
        const emberPos = new Float32Array(EMBERS * 3);
        for (let i = 0; i < EMBERS; i++) {
            const r = 43 + Math.random() * 49;
            const theta = Math.random() * Math.PI * 2;
            // Arc cosine of a uniform number, so the embers spread evenly over the sphere
            // instead of bunching at its poles.
            const phi = Math.acos(Math.random() * 2 - 1);
            emberPos[i * 3] = r * Math.sin(phi) * Math.cos(theta);
            emberPos[i * 3 + 1] = r * Math.sin(phi) * Math.sin(theta);
            emberPos[i * 3 + 2] = r * Math.cos(phi);
        }
        const emberGeom = new THREE.BufferGeometry();
        emberGeom.setAttribute('position', new THREE.BufferAttribute(emberPos, 3));
        // Mapped, not bare: an unmapped point sprite is a hard square.
        const emberMat = new THREE.PointsMaterial({
            map: api.helpers.glowTexture(32), color: 0xffaa00, size: 3,
            transparent: true, opacity: 0.75, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        const embers = new THREE.Points(emberGeom, emberMat);
        group.add(embers);

        const model = {
            group, glow, glowMat, coreGeom, coreRest, coreMat, ghostGeom, ghostRest, ghostMat,
            ribbons, ribbonMat, ribbonAltMat, node, nodeMat,
            hudInner, hudOuter, hudMat, hudAltMat, embers, emberMat,
            coreRadius: CORE, ribPoints: RIB_POINTS,
        };
        model.lobes = LOBES;
        divinaiShapeRibbons(model, 0, null, 0);
        return model;
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';
        const data = ctx.audioData;
        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.5 : 0;

        /* The core, pushed out band by band. Each vertex is carried outward along its own
           rest direction, so the core swells rather than shearing -- which is his wave,
           with the voice where his clock was. The carrier underneath it never stops. */
        divinaiSwell(model.coreGeom, model.coreRest, data, ctx.time, 1, isListening ? 0.4 : 1);
        divinaiSwell(model.ghostGeom, model.ghostRest, data, ctx.time - 0.33, 0.55, 0.7);
        model.coreMat.opacity = 0.55 + heat * 0.35 + ctx.click * 0.1;
        model.ghostMat.opacity = 0.12 + heat * 0.08;

        divinaiShapeRibbons(model, ctx.time, data, isListening ? 0.35 : 1);
        const spin = isThinking ? 1.8 : isListening ? 0.4 : 1;
        for (let i = 0; i < model.ribbons.length; i++) {
            // Time-based, not accumulated per frame -- see the file header.
            model.ribbons[i].line.rotation.x = i * 0.7 + ctx.time * 0.16 * (i + 1) * spin;
            model.ribbons[i].line.rotation.y = i * 1.1 + ctx.time * 0.22 * (i + 1) * spin;
        }
        model.ribbonMat.opacity = 0.6 + heat * 0.3;
        model.ribbonAltMat.opacity = 0.5 + heat * 0.28;

        /* The node's constant walk. Read straight out of the ribbon's own buffer so it sits
           exactly on the line rather than on a circle the line no longer is. */
        const first = model.ribbons[0];
        const at = Math.floor(ctx.time * 14) % model.ribPoints;
        model.node.position.set(first.pos[at * 3], first.pos[at * 3 + 1], first.pos[at * 3 + 2]);
        model.nodeMat.opacity = 0.8 + heat * 0.2;

        model.hudInner.rotation.z = ctx.time * 0.3 * spin;
        model.hudOuter.rotation.z = -ctx.time * 0.15 * spin;
        model.hudMat.opacity = 0.65 + heat * 0.3;
        model.hudAltMat.opacity = 0.4 + heat * 0.25;

        model.embers.rotation.y = ctx.time * 0.03;
        model.embers.rotation.x = Math.sin(ctx.time * 0.2) * 0.1;
        model.emberMat.opacity = 0.6 + heat * 0.3 + ctx.click * 0.15;

        model.glowMat.opacity = 0.12 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.03 + 0.03);
    },

    applyPalette(model, palette) {
        model.ghostMat.color.setHex(palette.hex2);
        model.ribbonMat.color.setHex(palette.hex);
        model.ribbonAltMat.color.setHex(palette.hex2);
        model.hudMat.color.setHex(palette.hex);
        model.hudAltMat.color.setHex(palette.hex);
        // The core, the running node and the embers keep the furnace's own heat -- see build().
    },
});

/* Pushes every vertex of a wireframe shell out along its own rest direction, by how loud
   its band is, over a slow carrier that keeps the shell alive in a silence. `rest` is the
   untouched build-time geometry: displacing a displaced vertex compounds, and a core fed
   its own output drifts off the screen inside a few seconds. */
function divinaiSwell(geom, rest, data, time, scale, gain) {
    const attr = geom.attributes.position;
    const arr = attr.array;
    const bins = data && data.length ? data.length : 0;
    for (let i = 0; i < arr.length; i += 3) {
        const x = rest[i];
        const y = rest[i + 1];
        const z = rest[i + 2];
        const carrier = Math.sin(time * 3 + x * 0.07 + y * 0.07) * 0.08;
        let band = 0;
        if (bins) {
            /* Which band this vertex answers to, from where it sits: the index is derived
               from its own position so neighbouring vertices share a band and the shell
               swells in patches rather than speckling. */
            const idx = Math.abs(Math.floor((x * 0.9 + y * 1.7 + z * 0.4))) % bins;
            band = (data[idx] || 0) / 255;
        }
        const push = 1 + (carrier + band * 0.3 * gain) * scale;
        arr[i] = x * push;
        arr[i + 1] = y * push;
        arr[i + 2] = z * push;
    }
    attr.needsUpdate = true;
}

/* Lays each ribbon's points out around its own circle, with its fixed wobble off the plane
   and its radius pushed out by the band that stretch of the loop answers to. Shared by the
   first lay-out at build and by every frame after it, because they are the same operation
   with a different loudness. */
function divinaiShapeRibbons(model, time, data, gain) {
    const bins = data && data.length ? data.length : 0;
    for (let r = 0; r < model.ribbons.length; r++) {
        const rib = model.ribbons[r];
        for (let j = 0; j < model.ribPoints; j++) {
            const a = (j / model.ribPoints) * Math.PI * 2;
            /* The loudness this stretch of the loop answers to. Read at LOBES points
               around the ribbon and blended between them rather than once per vertex:
               neighbouring bins of a spectrum differ freely, and a radius taken straight
               from them turns the ribbon into a sawtooth. */
            let band = 0;
            if (bins) {
                const at = (j / model.ribPoints) * model.lobes;
                const lo = Math.floor(at);
                const mix = at - lo;
                const binOf = (k) => (data[Math.floor(((k % model.lobes) / model.lobes) * bins)] || 0) / 255;
                band = binOf(lo) * (1 - mix) + binOf(lo + 1) * mix;
            }
            const breath = Math.sin(time * 2 + a * 3 + r) * 0.02;
            const radius = rib.radius * (1 + breath + band * 0.22 * gain);
            rib.pos[j * 3] = Math.cos(a) * radius;
            rib.pos[j * 3 + 1] = Math.sin(a) * radius;
            rib.pos[j * 3 + 2] = rib.wobble[j];
        }
        rib.geom.attributes.position.needsUpdate = true;
    }
}
