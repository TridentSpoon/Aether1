/* Avatar: A.R.X.LOGOS -- a central hexagon eye with six clockwise-spiralling hexagon arms
 * inside a dotted hex frame.
 *
 * The centre reads as an eye: a solid extruded hex gem with a fixed dark pupil and a
 * catchlight nested in it. The arm hexes double as camera-aperture blades, pulling in and
 * twisting shut over the pupil every few seconds, and immediately on a click. Two tilted
 * hex-frame rings and a small orbiting swarm give the structure depth beyond the single
 * flat plane the spiral lives in.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-logos',
    label: 'A.R.X.LOGOS',
    group: 'The Umbrals',

    build(api) {
        const THREE = api.THREE;
        const h = api.helpers;
        const group = new THREE.Group();

        /* --- Centrepiece: large regular hexagon, flat top/bottom, pointy left/right -- a
           real extruded prism (helpers.hexPrism) rather than a single-sided flat pane, so
           the core reads as a solid gem with actual depth and doesn't vanish when viewed
           from behind (e.g. after a drag-rotate of the scene). Front cap sits at z=0, so
           the pupil and catchlight in front of it sit where they always did; the depth
           recedes backward, away from the viewer. Edges of the whole prism (not just the
           front face) trace the outline so the vertical side edges show the depth too. */
        const centralRadius = 26;
        const centralThickness = 12;
        const centralFillMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex,
            transparent: true,
            opacity: 0.22,
            blending: THREE.AdditiveBlending,
        });
        const centralOutlineMat = new THREE.LineBasicMaterial({
            color: api.palette.hex,
            transparent: true,
            opacity: 0.95,
        });
        const centralFill = h.hexPrism(centralRadius, 0, centralFillMat, centralThickness);
        /* 12 degrees, not EdgesGeometry's 1-degree default: at the default every triangle
           boundary counts as an edge and the "outline" comes back as the whole mesh. */
        const centralEdges = new THREE.EdgesGeometry(centralFill.geometry, 12);
        const centralOutline = new THREE.LineSegments(centralEdges, centralOutlineMat);
        group.add(centralFill);
        group.add(centralOutline);

        /* --- Core eye: a fixed dark pupil nested in the central hex, plus a small
           catchlight, so the centrepiece reads as an eye rather than just a glowing panel
           -- fixed colour regardless of theme, the same real-material convention every
           other avatar's obsidian core follows. The catchlight also flashes brighter at the
           peak of an aperture-close (see animate). Both sit a hair in front of the central
           hex (explicit z, not just draw order) so they never flicker against it in
           three.js's transparent-object depth sort -- three flat panes would otherwise sit
           at exactly the same z. */
        const pupilMat = new THREE.MeshBasicMaterial({ color: 0x050208, transparent: true, opacity: 0.92 });
        const pupil = h.hexFill(9, 0, pupilMat);
        pupil.position.z = 0.5;
        group.add(pupil);

        const catchlightMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.5 });
        const catchlight = h.hexFill(2.4, 0, catchlightMat, -4, 4);
        catchlight.position.z = 1;
        group.add(catchlight);

        // --- Spiralling arms: 6 identical arms radiating from the central hex's 6 flat edges
        const armMatNear = new THREE.LineBasicMaterial({ color: api.palette.hex, transparent: true, opacity: 0.9 });
        const armMatMid = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.85 });
        const armMatFar = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.8 });

        const armHexes = [];
        const hexPerArm = 8;
        const apothem = centralRadius * Math.cos(Math.PI / 6); // centre to a flat edge

        for (let a = 0; a < 6; a++) {
            const baseAngle = (Math.PI / 6) + a * (Math.PI / 3); // edge normals: 30,90,...,330 deg
            let curAngle = baseAngle;
            let hexSize = 15;
            let curDist = apothem + hexSize * 0.8;

            for (let i = 0; i < hexPerArm; i++) {
                const mat = i < 3 ? armMatNear : (i < 6 ? armMatMid : armMatFar);
                const x = Math.cos(curAngle) * curDist;
                const y = Math.sin(curAngle) * curDist;
                const hex = h.hexOutline(hexSize, 0, mat, x, y);
                hex.userData = { armIndex: a, stepIndex: i, baseX: x, baseY: y, phase: a * 0.9 + i * 0.5 };
                armHexes.push(hex);
                group.add(hex);

                // Clockwise spiral: tight curve near the core, straightening toward the tail
                const angleStep = (0.62 * Math.pow(0.7, i));
                curAngle -= angleStep;
                curDist += hexSize * 1.3;
                hexSize *= 0.8;
            }
        }

        // --- Outer boundary ring: large hex frame, rotated 90 deg, traced by tiny dot-hexagons
        const outerRadius = 112;
        const dotsPerEdge = 10;
        const outerDotMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3,
            transparent: true,
            opacity: 0.75,
            blending: THREE.AdditiveBlending,
        });
        const outerDots = [];
        const outerVerts = h.hexVertices(outerRadius, Math.PI / 2);
        for (let e = 0; e < 6; e++) {
            const v0 = outerVerts[e];
            const v1 = outerVerts[(e + 1) % 6];
            for (let d = 0; d < dotsPerEdge; d++) {
                const t = d / dotsPerEdge;
                const x = v0.x + (v1.x - v0.x) * t;
                const y = v0.y + (v1.y - v0.y) * t;
                const dot = h.hexFill(2.6, 0, outerDotMat, x, y);
                dot.userData = { baseScale: 1.0, phase: (e * dotsPerEdge + d) * 0.35 };
                outerDots.push(dot);
                group.add(dot);
            }
        }

        /* --- Two hex-frame rings beyond the dotted boundary, each tilted onto its own axis
           and tumbling at its own speed (see animate) -- unlike everything above, which is
           flat in one plane, these read as depth wrapped around the whole structure. */
        const shellConfigs = [
            { radius: 132, rotOffset: 0, tiltX: 0.5, tiltY: 0.15, color: api.palette.hex2, speed: { x: 0.006, y: 0.010, z: 0.0 } },
            { radius: 156, rotOffset: Math.PI / 6, tiltX: -0.35, tiltY: 0.4, color: api.palette.hex3, speed: { x: -0.004, y: 0.008, z: 0.005 } },
        ];
        const shellRings = [];
        const shellRingMats = [];
        shellConfigs.forEach((cfg) => {
            const mat = new THREE.LineBasicMaterial({ color: cfg.color, transparent: true, opacity: 0.5 });
            const ring = h.hexOutline(cfg.radius, cfg.rotOffset, mat);
            ring.rotation.x = cfg.tiltX;
            ring.rotation.y = cfg.tiltY;
            ring.userData = { speed: cfg.speed };
            shellRingMats.push(mat);
            shellRings.push(ring);
            group.add(ring);
        });

        /* --- Orbiting hex swarm: small nodes drifting freely in 3D (x/z from an orbit
           angle, y from a fixed height plus a gentle bob) around the whole structure,
           unlike the arm hexes which are locked to the flat spiral -- real depth, not just
           another flat layer. */
        const swarmGroup = new THREE.Group();
        const swarmMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.65, blending: THREE.AdditiveBlending,
        });
        const swarmNodes = [];
        const swarmCount = 20;
        for (let i = 0; i < swarmCount; i++) {
            const radius = 60 + Math.random() * 70;
            const angle = Math.random() * Math.PI * 2;
            const heightOffset = (Math.random() - 0.5) * 70;
            const node = h.hexFill(2.2 + Math.random() * 1.6, Math.random() * Math.PI, swarmMat);
            node.position.set(Math.cos(angle) * radius, heightOffset, Math.sin(angle) * radius);
            node.userData = {
                radius, angle, heightOffset,
                speed: 0.15 + Math.random() * 0.25,
                bobPhase: Math.random() * Math.PI * 2,
            };
            swarmNodes.push(node);
            swarmGroup.add(node);
        }
        group.add(swarmGroup);

        return {
            group,
            centralFill, centralOutline, centralFillMat, centralOutlineMat,
            pupil, catchlight,
            armHexes, armMatNear, armMatMid, armMatFar,
            outerDots, outerDotMat,
            shellRings, shellRingMats,
            swarmGroup, swarmNodes, swarmMat,
            /* The aperture clock. It lived on the engine before this avatar carried its own
               state; starting in the past means the first close is scheduled rather than
               playing on frame one. */
            apertureStart: -10,
            nextAperture: 5,
            handledClick: -999,
        };
    },

    applyPalette(model, p) {
        // The pupil and the catchlight are the fixed "real eye" materials -- see build.
        model.centralFillMat.color.setHex(p.hex);
        model.centralOutlineMat.color.setHex(p.hex);
        model.armMatNear.color.setHex(p.hex);
        model.armMatMid.color.setHex(p.hex2);
        model.armMatFar.color.setHex(p.hex3);
        model.outerDotMat.color.setHex(p.hex3);
        if (model.shellRingMats[0]) model.shellRingMats[0].color.setHex(p.hex2);
        if (model.shellRingMats[1]) model.shellRingMats[1].color.setHex(p.hex3);
        model.swarmMat.color.setHex(p.hex);
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const t = ctx.time;

        const spinSpeed = isThinking ? 0.02 : (isSpeaking ? 0.01 : ctx.click * 0.012);
        model.group.rotation.z -= spinSpeed; // clockwise, matching the arm winding
        if (isThinking || isSpeaking) {
            model.group.rotation.x = Math.sin(t * 0.4) * 0.1;
            model.group.rotation.y = Math.cos(t * 0.35) * 0.1;
        } else {
            /* Front-facing and still at rest -- only a faint click-triggered nod, plus the
               aperture-close below. A click triggers one immediately; otherwise it closes
               on its own every several seconds so the eye never reads as a fixed prop. */
            model.group.rotation.x += (ctx.click * 0.08 - model.group.rotation.x) * 0.15;
            model.group.rotation.y += (0 - model.group.rotation.y) * 0.15;

            if (ctx.clickTime !== model.handledClick) {
                model.handledClick = ctx.clickTime;
                model.apertureStart = t;
                model.nextAperture = t + 5 + Math.random() * 4;
            }
            if (t >= model.nextAperture) {
                model.apertureStart = t;
                model.nextAperture = t + 5 + Math.random() * 6;
            }
        }

        // How far into an aperture-close/reopen cycle we are (0 = fully open, 1 = fully
        // closed), on the same sine close-then-reopen curve as R.E.D. 9000's blink.
        const APERTURE_DURATION = 0.5;
        const apertureAge = t - model.apertureStart;
        const closeness = (apertureAge >= 0 && apertureAge < APERTURE_DURATION)
            ? Math.sin((apertureAge / APERTURE_DURATION) * Math.PI)
            : 0;

        let coreScale;
        if (isSpeaking) coreScale = 1 + ctx.audio * 0.5;
        else if (isThinking) coreScale = 1 + Math.sin(t * 16) * 0.18;
        else coreScale = 1 + ctx.click * 0.15;
        model.centralFill.scale.setScalar(coreScale);
        model.centralOutline.scale.setScalar(coreScale);

        // The catchlight flashes brighter right at the peak of an aperture-close -- a glint
        // off the core-eye as the blades sweep shut over it.
        model.catchlight.material.opacity = 0.5 + closeness * 0.45;

        /* Arm hexagons -- they double as camera-aperture blades: energy pulses outward along
           each arm while speaking or thinking (or a brief pulse on a click), and on top of
           that every hex is pulled in from its resting spot toward the core-eye and twisted
           during an aperture-close, then released back out. */
        model.armHexes.forEach((hex) => {
            let pulseFactor;
            if (isSpeaking) {
                const fVal = (ctx.audioData[hex.userData.stepIndex % 16] || 0) / 255;
                pulseFactor = 1 + fVal * 0.6 + Math.sin(t * 10 + hex.userData.phase) * 0.15;
            } else if (isThinking) {
                pulseFactor = 1 + Math.sin(t * 12 + hex.userData.phase) * 0.3;
            } else {
                pulseFactor = 1 + ctx.click * 0.2;
            }
            hex.scale.setScalar(pulseFactor);

            hex.position.x = -hex.userData.baseX * closeness * 0.92;
            hex.position.y = -hex.userData.baseY * closeness * 0.92;
            hex.rotation.z = closeness * 1.1 * (hex.userData.armIndex % 2 === 0 ? 1 : -1);
        });

        // Outer dotted boundary ring -- shimmers while speaking, otherwise still.
        model.outerDots.forEach((dot) => {
            const shimmer = isSpeaking
                ? 1 + Math.sin(t * 2 + dot.userData.phase) * 0.35
                : 1 + ctx.click * 0.25;
            dot.scale.setScalar(shimmer);
        });

        /* Two hex-frame shell rings, each tumbling on its own axis at its own speed, always
           -- like hAlcy's rings they never go fully still, just faster while thinking or
           speaking. */
        const shellSpinMult = isThinking ? 2.2 : (isSpeaking ? 1.5 : 1.0);
        model.shellRings.forEach((ring) => {
            ring.rotation.x += ring.userData.speed.x * shellSpinMult;
            ring.rotation.y += ring.userData.speed.y * shellSpinMult;
            ring.rotation.z += ring.userData.speed.z * shellSpinMult;
        });

        // Orbiting hex swarm -- drifts around the whole structure in 3D continuously, each
        // node also slowly tumbling on its own.
        model.swarmNodes.forEach((node) => {
            node.userData.angle += node.userData.speed * 0.01;
            node.position.x = Math.cos(node.userData.angle) * node.userData.radius;
            node.position.z = Math.sin(node.userData.angle) * node.userData.radius;
            node.position.y = node.userData.heightOffset + Math.sin(t * 1.5 + node.userData.bobPhase) * 4;
            node.rotation.x += 0.015;
            node.rotation.y += 0.02;
        });
    },
});
