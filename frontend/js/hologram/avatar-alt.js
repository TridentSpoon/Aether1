/* Avatar: A1ter_nul -- a horizontal row of irregular dark-glass shards inside a firewall.
 *
 * Each shard stands edge-on as a thin vertical bar, tapering shortest at the two ends of
 * the row and tallest in the middle, with visible gaps between them: not a solid body but
 * a broken relic. Each pane is fixed obsidian glass with a theme-tinted glowing edge, and
 * while speaking every edge's brightness is driven by its own audio frequency bin, so the
 * row reads as a spectrum analyser built out of broken glass. Around it sit a ring of hex
 * ICE tiles on a slow scan sweep and a firewall perimeter of concentric rings fading
 * outward toward the edges of the window.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'alt',
    label: 'A1ter_nul',
    group: 'Singular Ascended Class',
    /* The chromatic-glitch scanline overlay this avatar is watched through -- see
       #hologram-viewport.glitch-active::after in A1theme.css and sprite.css. The engine
       puts the class on the viewport while this avatar is the one on screen. */
    viewportClass: 'glitch-active',

    build(api) {
        const THREE = api.THREE;
        const h = api.helpers;
        const group = new THREE.Group();

        /* Everything that sways with the avatar hangs off this, rather than off the group
           itself, because the firewall field below must not sway: it is a boundary bound to
           the window's own edges, and a boundary that tilts with the thing inside it reads
           as a halo instead. */
        const sway = new THREE.Group();
        group.add(sway);

        /* --- The Black Wall: a row of jittered, irregular glass-pane shards with gaps
           between them, tapering shortest at both ends like a relic chip's lens profile,
           oriented as a horizontal spectrum-analyser row rather than a column. */
        const shardGroup = new THREE.Group();
        const shards = [];
        const shardCount = 9;
        const totalSpan = 84; // the row's span along the stacking axis (horizontal, X)
        const slotSpan = totalSpan / shardCount;
        const gap = 2.6;
        const shardThickness = slotSpan - gap; // the thin dimension, collapsed into depth
        const maxLength = 54; // the tapered, visible (vertical) bar length
        const minLength = 24;

        for (let i = 0; i < shardCount; i++) {
            const t = shardCount > 1 ? i / (shardCount - 1) : 0.5;
            const taper = 1 - Math.pow(Math.abs(t - 0.5) * 2, 1.6); // lens profile, 0..1
            const length = minLength + (maxLength - minLength) * taper;
            const x = -totalSpan / 2 + i * slotSpan + slotSpan / 2;

            // An irregular quad -- jittered corners, so each pane reads as a broken glass
            // shard rather than a tidy rectangle in a row.
            const hw = length / 2, hh = shardThickness / 2;
            const jitter = () => (Math.random() - 0.5) * 3.5;
            const pts = [
                new THREE.Vector3(-hw + jitter(), -hh + jitter(), 0),
                new THREE.Vector3(hw + jitter(), -hh + jitter(), 0),
                new THREE.Vector3(hw + jitter(), hh + jitter(), 0),
                new THREE.Vector3(-hw + jitter(), hh + jitter(), 0),
            ];

            const fillGeom = new THREE.BufferGeometry();
            const positions = [
                pts[0].x, pts[0].y, 0, pts[1].x, pts[1].y, 0, pts[2].x, pts[2].y, 0,
                pts[0].x, pts[0].y, 0, pts[2].x, pts[2].y, 0, pts[3].x, pts[3].y, 0,
            ];
            fillGeom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));

            // Obsidian glass fill -- fixed, not theme-tinted (real glass does not change
            // hue with the UI theme), the same convention as R.E.D. 9000's core sphere.
            const fillMat = new THREE.MeshBasicMaterial({
                color: 0x0a0a0c, transparent: true, opacity: 0.55, side: THREE.DoubleSide,
            });
            const fillMesh = new THREE.Mesh(fillGeom, fillMat);

            // The glowing edge -- theme-tinted, and the one thing animate drives per frame
            // to turn the row into an equalizer while speaking.
            const outlineMat = new THREE.LineBasicMaterial({
                color: api.palette.hex2, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending,
            });
            const outline = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(pts), outlineMat);

            const shardPivot = new THREE.Group();
            /* Each shard's quad is built flat in local XY (see pts above), its long axis
               along local X and its thin axis along local Y. Rotating 90 degrees about Z
               swings the long axis onto world Y (a vertical bar); a further 90 degrees
               about Y tips the now-thin local axis into depth, so the bar is seen edge-on
               from the front instead of as a face-on rectangle. Net effect: a row of thin
               vertical glass bars, not a shelf of upright picture frames. The small extra
               jitter keeps them askew rather than a tidy, perfectly level row. */
            shardPivot.rotation.z = Math.PI / 2 + (Math.random() - 0.5) * 0.09;
            shardPivot.rotation.y = Math.PI / 2 + (Math.random() - 0.5) * 0.16;
            shardPivot.position.set(x, (Math.random() - 0.5) * 4, (Math.random() - 0.5) * 3);
            shardPivot.add(fillMesh);
            shardPivot.add(outline);
            shardGroup.add(shardPivot);

            shards.push({
                fillMat, outlineMat,
                baseOpacity: 0.3 + Math.random() * 0.15,
                phase: i * 0.9,
                binIndex: i % 16,
            });
        }
        sway.add(shardGroup);

        /* --- Firewall perimeter: a static boundary, not a scanning halo -- many concentric
           true circles (no elliptical stretch), packed close enough to read as a continuous
           dissipating field rather than a handful of discrete hoops, the outermost reaching
           out toward the window's own edges. Each ring is fainter than the one inside it,
           so the boundary reads as fading outward from a bright core.

           Only the two innermost rings sway with the avatar. Everything past that hangs off
           staticField, which sits outside the swaying group and counter-rotates the view's
           own drag-yaw in animate, so the field stays put however the avatar tilts and
           however far the hologram is spun. */
        const staticField = new THREE.Group();
        group.add(staticField);
        const firewallRings = [];
        const firewallRingCount = 28;
        // The shield tiles orbit at radius 56 (below); keep every ring outside that
        // boundary so the field never crowds in past the hex tiles.
        const firewallInnerRadius = 78;
        const firewallOuterRadius = 260;
        const firewallMovingLayers = 2;
        for (let i = 0; i < firewallRingCount; i++) {
            const t = i / (firewallRingCount - 1); // 0 (innermost) .. 1 (outermost)
            const radius = firewallInnerRadius + (firewallOuterRadius - firewallInnerRadius) * t;
            const fade = Math.pow(1 - t, 1.6); // brightest near the core, fading to the rim
            const mat = new THREE.MeshBasicMaterial({
                color: api.palette.hex, transparent: true, opacity: 0.5, side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
            });
            const mesh = new THREE.Mesh(new THREE.RingGeometry(radius, radius + radius * 0.012, 96), mat);
            (i < firewallMovingLayers ? sway : staticField).add(mesh);
            firewallRings.push({ mesh, mat, fade });
        }

        /* --- Shield tiles: small hex ICE nodes ringing the row, individually lit in
           sequence like a security scanner sweeping the perimeter. */
        const shieldGroup = new THREE.Group();
        const shieldFillMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending,
        });
        const shieldOutlineMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.85 });

        const shieldTiles = [];
        const tileCount = 9;
        const tileRadius = 56;
        for (let i = 0; i < tileCount; i++) {
            const angle = (i / tileCount) * Math.PI * 2;
            const x = Math.cos(angle) * tileRadius;
            const y = Math.sin(angle) * tileRadius;
            const tileGroup = new THREE.Group();
            tileGroup.add(h.hexFill(5.5, Math.PI / 2, shieldFillMat, x, y));
            tileGroup.add(h.hexOutline(5.5, Math.PI / 2, shieldOutlineMat, x, y));
            tileGroup.userData = { baseAngle: angle, phase: i * 0.7 };
            shieldTiles.push(tileGroup);
            shieldGroup.add(tileGroup);
        }
        sway.add(shieldGroup);

        return {
            group,
            /* Auto-fit measures this, not the whole group: the firewall field is sized to
               the window's own edges on purpose, and measuring it would pull the camera
               back until the row of shards inside it was a postage stamp. */
            fitGroup: sway,
            sway, staticField,
            shardGroup, shards,
            firewallRings,
            shieldGroup, shieldTiles, shieldFillMat, shieldOutlineMat,
        };
    },

    applyPalette(model, p) {
        /* Each shard's glass fill is fixed obsidian (see build) -- only the glowing edges
           retint, ramped from hex2 at one end of the row to hex3 at the other so the
           equalizer reads as a spectrum, like a real EQ's colour ramp, rather than one
           flat colour. */
        const bottom = new THREE.Color(p.hex2);
        const top = new THREE.Color(p.hex3);
        model.shards.forEach((shard, i) => {
            const t = model.shards.length > 1 ? i / (model.shards.length - 1) : 0;
            shard.outlineMat.color.copy(bottom).lerp(top, t);
        });
        model.firewallRings.forEach((ring) => ring.mat.color.setHex(p.hex));
        model.shieldFillMat.color.setHex(p.hex);
        model.shieldOutlineMat.color.setHex(p.hex3);
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isAlert = isThinking || isSpeaking;
        const t = ctx.time;

        if (isAlert) {
            model.sway.rotation.y = Math.sin(t * 0.5) * 0.12;
            model.sway.rotation.x = Math.sin(t * 0.35) * 0.08;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            model.sway.rotation.y += (ctx.click * 0.1 - model.sway.rotation.y) * 0.15;
            model.sway.rotation.x += (ctx.click * 0.06 - model.sway.rotation.x) * 0.15;
        }

        /* The field is already outside the swaying group, so the avatar's own tilt cannot
           reach it -- but a drag on the hologram spins the whole scene, which would carry
           the field along with it. Counter-rotating the view's yaw every frame keeps the
           field visually fixed however the operator spins the view. */
        model.staticField.rotation.y = -ctx.viewRotationY;

        // Whole-row breathing pulse -- subtle, so it does not fight the per-shard equalizer.
        const stackPulse = 1 + ctx.audio * 0.03 + ctx.click * 0.04;
        model.shardGroup.scale.setScalar(stackPulse);

        /* Each shard's glowing edge is its own equalizer bar: idle shards shimmer gently
           and out of phase with each other (an inert relic still flickering faintly);
           speaking drives each shard's brightness off its own audio frequency bin, so the
           row reads as a real spectrum analyser; thinking is a fast, uniform flicker across
           the whole row instead of a per-bin readout. */
        model.shards.forEach((shard) => {
            let opacity;
            if (isSpeaking) {
                const fVal = (ctx.audioData[shard.binIndex] || 0) / 255;
                opacity = 0.2 + fVal * 0.85 + Math.sin(t * 6 + shard.phase) * 0.04;
            } else if (isThinking) {
                opacity = shard.baseOpacity + Math.sin(t * 13 + shard.phase) * 0.35;
            } else {
                opacity = shard.baseOpacity + Math.sin(t * 1.1 + shard.phase) * 0.15 + ctx.click * 0.3;
            }
            shard.outlineMat.opacity = Math.max(0.05, Math.min(1, opacity));
        });

        /* Firewall perimeter -- the rings are static, bound to the window's own edges rather
           than scanning around, so only their brightness answers the current state; each
           ring keeps its own fixed fade relative to the others. */
        const baseOpacity = isSpeaking
            ? 0.5 + ctx.audio * 0.4
            : (isThinking ? 0.5 + Math.sin(t * 10) * 0.25 : 0.4 + ctx.click * 0.3);
        model.firewallRings.forEach((ring) => {
            ring.mat.opacity = baseOpacity * ring.fade;
        });

        // Shield tiles -- a sequential scan sweep around the perimeter, faster while alert.
        model.shieldGroup.rotation.z -= isAlert ? 0.012 : 0.0025;
        const scanSpeed = isAlert ? 6.0 : 2.0;
        model.shieldTiles.forEach((tile) => {
            const sweep = 0.5 + 0.5 * Math.sin(t * scanSpeed - tile.userData.phase);
            const litFactor = isAlert ? sweep : sweep * 0.5;
            tile.scale.setScalar(0.85 + litFactor * 0.45 + ctx.click * 0.15);
        });
    },
});
