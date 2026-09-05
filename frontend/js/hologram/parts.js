/* Avatar parts.
 *
 * A kit of pieces an avatar can be assembled from, rather than modelled by hand. The
 * six built-in avatars are hand-built and stay that way -- they do things no kit could
 * anticipate. This is for the other case: someone who wants an avatar of their own and
 * would rather choose than write three.js.
 *
 * Every part answers the same three questions, which is the whole reason they can be
 * mixed freely:
 *
 *   build(api, options) -> { object, applyPalette(palette), animate(ctx) }
 *
 * `object` is a THREE.Object3D to hang in the scene. `applyPalette` retints it when the
 * colour theme changes. `animate` is called once a frame with the same context an avatar
 * gets: { time, audio, audioData, click, state, palette }.
 *
 * Parts never read the palette at build time and keep it -- they are always told, so
 * that a theme switched later reaches every part including the ones not on screen.
 *
 * Used by avatar-custom.js (which renders a saved recipe) and available to any
 * hand-written avatar that wants a ring without writing one.
 */
(function () {
    'use strict';

    /* Audio comes in as 64 frequency bins, quiet to loud, low to high. Parts that
       react to sound read it through this so they all behave the same way: a value
       from 0 to 1 for a given band, with a floor so an idle avatar still breathes. */
    function band(ctx, index, count) {
        const data = ctx.audioData;
        if (!data || !data.length) return 0;
        const per = Math.max(1, Math.floor(data.length / count));
        const start = Math.min(data.length - 1, index * per);
        let sum = 0;
        for (let i = 0; i < per; i++) sum += data[Math.min(data.length - 1, start + i)] || 0;
        return sum / (per * 255);
    }

    // ---- Cores: the thing at the middle -------------------------------------

    const CORES = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        orb: {
            label: 'Obsidian orb',
            build(api, options) {
                /* Phong rather than Basic so the scene's lights actually land on it --
                   this is the one part meant to look like a solid object rather than
                   drawn light, the same trick the built-in hAlcy core uses. */
                const mat = new THREE.MeshPhongMaterial({
                    color: 0x05070f, emissive: 0x0a1030, shininess: 90, specular: 0x8fa8ff,
                });
                const mesh = new THREE.Mesh(new THREE.SphereGeometry(options.size, 48, 48), mat);
                return {
                    object: mesh,
                    applyPalette() { /* fixed by design: an obsidian core is not tinted */ },
                    animate(ctx) {
                        const pulse = 1 + ctx.audio * 0.12 + ctx.click * 0.1;
                        mesh.scale.setScalar(pulse);
                    },
                };
            },
        },

        crystal: {
            label: 'Faceted crystal',
            build(api, options) {
                const geom = new THREE.IcosahedronGeometry(options.size, 1);
                const fill = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.28,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                const edge = new THREE.LineBasicMaterial({ color: 0x00f0ff, transparent: true, opacity: 0.9 });
                const group = new THREE.Group();
                const mesh = new THREE.Mesh(geom, fill);
                const wire = new THREE.LineSegments(new THREE.EdgesGeometry(geom, 12), edge);
                group.add(mesh, wire);
                return {
                    object: group,
                    applyPalette(p) { fill.color.setHex(p.hex); edge.color.setHex(p.hex3); },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.4;
                        group.rotation.x = Math.sin(ctx.time * 0.3) * 0.3;
                        group.scale.setScalar(1 + ctx.audio * 0.2 + ctx.click * 0.15);
                    },
                };
            },
        },

        eye: {
            label: 'Eye lens',
            build(api, options) {
                const group = new THREE.Group();
                const shell = new THREE.Mesh(
                    new THREE.SphereGeometry(options.size, 40, 40),
                    new THREE.MeshPhongMaterial({ color: 0x05070f, shininess: 120, specular: 0x7f95ff })
                );
                const lensMat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.85, side: THREE.DoubleSide,
                });
                const lens = new THREE.Mesh(new THREE.CircleGeometry(options.size * 0.55, 48), lensMat);
                lens.position.z = options.size * 0.86;
                /* The hot centre is a sprite, not geometry: it has to read as light
                   spilling toward the viewer rather than a disc sitting in space. */
                const glowMat = new THREE.SpriteMaterial({
                    map: api.helpers.radialGlowTexture(128, 'rgba(255,240,180,1)', 'rgba(255,40,20,0)'),
                    transparent: true, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const glow = new THREE.Sprite(glowMat);
                glow.scale.setScalar(options.size * 2.4);
                glow.position.z = options.size * 0.95;
                group.add(shell, lens, glow);
                return {
                    object: group,
                    applyPalette(p) { lensMat.color.setHex(p.hex); },
                    animate(ctx) {
                        const heat = 0.7 + ctx.audio * 1.6 + ctx.click * 0.5
                            + (ctx.state === 'THINKING' ? Math.abs(Math.sin(ctx.time * 3)) * 0.4 : 0);
                        glow.scale.setScalar(options.size * 2.4 * (0.8 + heat * 0.5));
                        glowMat.opacity = Math.min(1, 0.45 + heat * 0.4);
                    },
                };
            },
        },
    };

    // ---- Bodies: the structure around the core ------------------------------

    const BODIES = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        lattice: {
            label: 'Particle lattice',
            build(api, options) {
                const count = 2200;
                const geom = new THREE.BufferGeometry();
                const positions = new Float32Array(count * 3);
                const colors = new Float32Array(count * 3);
                const base = [];
                for (let i = 0; i < count; i++) {
                    /* Even spread over a sphere: picking a random latitude directly
                       clusters points at the poles, so cosine-distribute it. */
                    const theta = Math.random() * Math.PI * 2;
                    const phi = Math.acos(2 * Math.random() - 1);
                    const r = options.radius * (0.85 + Math.random() * 0.15);
                    const v = new THREE.Vector3(
                        r * Math.sin(phi) * Math.cos(theta),
                        r * Math.sin(phi) * Math.sin(theta),
                        r * Math.cos(phi)
                    );
                    base.push(v);
                    positions.set([v.x, v.y, v.z], i * 3);
                }
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
                const mat = new THREE.PointsMaterial({
                    size: 1.7, vertexColors: true, transparent: true, opacity: 0.9,
                    blending: THREE.AdditiveBlending, depthWrite: false,
                    map: api.helpers.glowTexture(32),
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette(p) {
                        for (let i = 0; i < count; i++) {
                            colors[i * 3] = p.r;
                            colors[i * 3 + 1] = Math.min(1, p.g + Math.random() * 0.15);
                            colors[i * 3 + 2] = p.b;
                        }
                        geom.attributes.color.needsUpdate = true;
                    },
                    animate(ctx) {
                        const push = 1 + ctx.audio * 0.5 + ctx.click * 0.3;
                        for (let i = 0; i < count; i++) {
                            const v = base[i];
                            const wave = Math.sin(ctx.time * 1.5 + v.x * 0.05 + v.y * 0.05) * 3;
                            positions[i * 3] = v.x * push + wave * 0.3;
                            positions[i * 3 + 1] = v.y * push + wave * 0.3;
                            positions[i * 3 + 2] = v.z * push;
                        }
                        geom.attributes.position.needsUpdate = true;
                        points.rotation.y = ctx.time * 0.12;
                    },
                };
            },
        },

        rings: {
            label: 'Orbital rings',
            build(api, options) {
                const group = new THREE.Group();
                const mats = [];
                const rings = [];
                for (let i = 0; i < 3; i++) {
                    const mat = new THREE.MeshBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.75,
                        side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                    });
                    const r = options.radius * (1 + i * 0.22);
                    const ring = new THREE.Mesh(new THREE.TorusGeometry(r, 0.9, 8, 96), mat);
                    ring.rotation.x = Math.PI / 2 * (i === 0 ? 1 : 0.4 * i);
                    ring.rotation.y = i * 0.5;
                    mats.push(mat);
                    rings.push({ ring, speed: 0.2 + i * 0.15, tilt: i });
                    group.add(ring);
                }
                return {
                    object: group,
                    applyPalette(p) {
                        mats[0].color.setHex(p.hex);
                        mats[1].color.setHex(p.hex2);
                        mats[2].color.setHex(p.hex3);
                    },
                    animate(ctx) {
                        rings.forEach(({ ring, speed, tilt }) => {
                            ring.rotation.z = ctx.time * speed;
                            ring.rotation.y = Math.sin(ctx.time * 0.3 + tilt) * 0.6 + tilt * 0.5;
                            ring.scale.setScalar(1 + ctx.audio * 0.18 + ctx.click * 0.12);
                        });
                    },
                };
            },
        },

        shards: {
            label: 'Shard stack',
            build(api, options) {
                const group = new THREE.Group();
                const shards = [];
                const COUNT = 9;
                for (let i = 0; i < COUNT; i++) {
                    const t = i / (COUNT - 1);
                    const w = options.radius * (0.5 + Math.sin(t * Math.PI) * 0.8);
                    const geom = new THREE.PlaneGeometry(w, options.radius * 0.16);
                    const fill = new THREE.MeshBasicMaterial({
                        color: 0x05070f, transparent: true, opacity: 0.55, side: THREE.DoubleSide,
                    });
                    const outlineMat = new THREE.LineBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.95,
                    });
                    const mesh = new THREE.Mesh(geom, fill);
                    const outline = new THREE.LineSegments(new THREE.EdgesGeometry(geom), outlineMat);
                    const shard = new THREE.Group();
                    shard.add(mesh, outline);
                    shard.position.y = (t - 0.5) * options.radius * 2.1;
                    shard.rotation.z = (Math.random() - 0.5) * 0.14;
                    group.add(shard);
                    shards.push({ shard, outlineMat, index: i, baseY: shard.position.y });
                }
                return {
                    object: group,
                    applyPalette(p) {
                        /* Ramped bottom to top so the stack reads as a spectrum rather
                           than one flat colour -- the same idea as a real EQ's ramp. */
                        const bottom = new THREE.Color(p.hex2);
                        const top = new THREE.Color(p.hex3);
                        shards.forEach(({ outlineMat, index }) => {
                            outlineMat.color.copy(bottom).lerp(top, index / (COUNT - 1));
                        });
                    },
                    animate(ctx) {
                        shards.forEach(({ shard, outlineMat, index, baseY }) => {
                            const level = band(ctx, index, COUNT);
                            shard.scale.x = 1 + level * 1.1 + ctx.click * 0.2;
                            shard.position.y = baseY + Math.sin(ctx.time * 1.2 + index) * 1.5;
                            outlineMat.opacity = 0.5 + level * 0.5;
                        });
                        group.rotation.y = Math.sin(ctx.time * 0.25) * 0.35;
                    },
                };
            },
        },

        swarm: {
            label: 'Orbiting swarm',
            build(api, options) {
                const group = new THREE.Group();
                const mat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const nodes = [];
                for (let i = 0; i < 26; i++) {
                    const mesh = new THREE.Mesh(new THREE.OctahedronGeometry(options.radius * 0.06), mat);
                    nodes.push({
                        mesh,
                        radius: options.radius * (1.05 + Math.random() * 0.5),
                        angle: Math.random() * Math.PI * 2,
                        height: (Math.random() - 0.5) * options.radius * 1.2,
                        speed: 0.15 + Math.random() * 0.35,
                        bob: Math.random() * Math.PI * 2,
                    });
                    group.add(mesh);
                }
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        const spread = 1 + ctx.audio * 0.35 + ctx.click * 0.25;
                        nodes.forEach((n) => {
                            const a = n.angle + ctx.time * n.speed;
                            n.mesh.position.set(
                                Math.cos(a) * n.radius * spread,
                                n.height + Math.sin(ctx.time + n.bob) * 4,
                                Math.sin(a) * n.radius * spread
                            );
                            n.mesh.rotation.y = ctx.time * n.speed * 2;
                        });
                    },
                };
            },
        },
    };

    // ---- Equalisers: the part that answers the voice -------------------------

    const EQUALISERS = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        ring: {
            label: 'Ring of bars',
            build(api, options) {
                const group = new THREE.Group();
                const mat = new THREE.MeshBasicMaterial({
                    color: 0x0066ff, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                const BARS = 48;
                const BAR_LENGTH = 8;
                const bars = [];
                for (let i = 0; i < BARS; i++) {
                    const angle = (i / BARS) * Math.PI * 2;
                    /* Each bar is a box standing along its own local Y, rotated so that
                       Y points away from the centre. Growing it then reads as a spike
                       pushing outward -- which is what an equaliser bar should do. */
                    const bar = new THREE.Mesh(new THREE.BoxGeometry(1.4, BAR_LENGTH, 1.4), mat);
                    bar.rotation.z = angle - Math.PI / 2;
                    group.add(bar);
                    bars.push({ bar, angle, index: i });
                }
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex2); },
                    animate(ctx) {
                        bars.forEach(({ bar, angle, index }) => {
                            const level = band(ctx, index % 32, 32);
                            const scale = 1 + level * 5 + ctx.click * 0.6;
                            bar.scale.y = scale;
                            /* Scaling grows a box about its own centre, so the bar has
                               to move out by half of what it gained -- otherwise it eats
                               inward across the ring as it rises. */
                            const out = options.radius + (BAR_LENGTH * scale) / 2;
                            bar.position.set(Math.cos(angle) * out, Math.sin(angle) * out, 0);
                        });
                        group.rotation.z = ctx.time * 0.1;
                    },
                };
            },
        },

        stack: {
            label: 'Vertical bars',
            build(api, options) {
                const group = new THREE.Group();
                const BARS = 12;
                const bars = [];
                const mats = [];
                /* The row's width and its drop below the body are both capped. A row
                   that simply scaled with the avatar ran off both edges of the viewport
                   at the larger sizes, where it is exactly the part you want to watch. */
                const span = Math.min(options.radius, 76);
                for (let i = 0; i < BARS; i++) {
                    const mat = new THREE.MeshBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                    });
                    const bar = new THREE.Mesh(new THREE.BoxGeometry(span * 0.09, 2, 2), mat);
                    bar.position.x = (i - (BARS - 1) / 2) * span * 0.13;
                    group.add(bar);
                    bars.push({ bar, index: i });
                    mats.push(mat);
                }
                /* Sat directly under the body, but not so far down that a large avatar
                   pushes it off the bottom of the viewport -- the cap is what keeps the
                   bars visible across the whole size range. */
                group.position.y = -Math.min(options.radius * 1.15, 76);
                return {
                    object: group,
                    applyPalette(p) {
                        const lo = new THREE.Color(p.hex2);
                        const hi = new THREE.Color(p.hex3);
                        mats.forEach((m, i) => m.color.copy(lo).lerp(hi, i / (BARS - 1)));
                    },
                    animate(ctx) {
                        bars.forEach(({ bar, index }) => {
                            const level = band(ctx, index, BARS);
                            const h = 2 + level * span * 1.1 + ctx.click * 4;
                            bar.scale.y = h / 2;
                            bar.position.y = h / 2; // grow upward from the baseline, not both ways
                        });
                    },
                };
            },
        },

        spikes: {
            label: 'Radial spikes',
            build(api, options) {
                const group = new THREE.Group();
                const mat = new THREE.LineBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                const SPIKES = 64;
                const geom = new THREE.BufferGeometry();
                const positions = new Float32Array(SPIKES * 2 * 3);
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                const lines = new THREE.LineSegments(geom, mat);
                group.add(lines);
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex3); },
                    animate(ctx) {
                        for (let i = 0; i < SPIKES; i++) {
                            const a = (i / SPIKES) * Math.PI * 2 + ctx.time * 0.15;
                            const level = band(ctx, i % 32, 32);
                            const inner = options.radius;
                            const outer = inner + 6 + level * options.radius * 0.9 + ctx.click * 8;
                            positions.set([Math.cos(a) * inner, Math.sin(a) * inner, 0], i * 6);
                            positions.set([Math.cos(a) * outer, Math.sin(a) * outer, 0], i * 6 + 3);
                        }
                        geom.attributes.position.needsUpdate = true;
                    },
                };
            },
        },
    };

    /* One place for a picker to read, so adding a part here makes it appear in the
       builder without the builder knowing anything about it. */
    function catalogue(kinds) {
        return Object.keys(kinds).map((id) => ({ id, label: kinds[id].label }));
    }

    window.AvatarParts = {
        cores: CORES,
        bodies: BODIES,
        equalisers: EQUALISERS,
        band,
        options: {
            cores: () => catalogue(CORES),
            bodies: () => catalogue(BODIES),
            equalisers: () => catalogue(EQUALISERS),
        },
    };
})();
