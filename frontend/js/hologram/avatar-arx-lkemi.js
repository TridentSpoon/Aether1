/* Avatar: A.R.X.L'KEMI -- a downward-pointing triangle (flat edge on top, a single
 * point at the bottom): a hexagonal centre panel with its three corners cut back,
 * and a pyramid rebuilt at each of those three corners to fill the cut-away wedge
 * back in -- so the panel and the three pyramids read as one solid triangle
 * silhouette rather than three separate spikes with gaps between them. A small
 * glowing aperture sits at the centre, framed by two counter-rotating halo rings and
 * a column of slow-rising data particles.
 *
 * The uploaded reference for this one pushed its three corner pyramids outward past
 * the edges of its own central shape, leaving a visible gap all the way around
 * (its "void" was meant to read as a hole, not the body of the figure) -- the
 * opposite of what was asked for here. This build instead gives each pyramid the
 * exact triangular wedge the cut removed as its base, so in the XY plane it closes
 * that gap completely; only then does it rise into a point in Z, so the corner
 * reads as a real pyramid rather than a flat patch.
 *
 * Loosely inspired by faceted geometric alchemical-symbol motifs in general -- a
 * triangle built from cut-cornered panel plus corner pyramids -- not a copy of any
 * one specific design, and redrawn from scratch in this engine's own flat-shape/
 * unlit-material language (MeshBasicMaterial, no scene lighting) the same way every
 * other avatar here is hand built; see js/hologram/core.js's note that only hAlcy's
 * obsidian core is actually lit.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar
 * file uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-lkemi',
    label: "A.R.X.L'KEMI",
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted: the aperture's glow, this avatar's one identity
        // colour that never retints, the same convention every other Umbral's hot
        // accent follows.
        const CORE_HOT = 0x33ffb2;

        // Flat edge on top, single point at the bottom.
        const A = new THREE.Vector2(-34, 20); // top-left corner
        const B = new THREE.Vector2(34, 20);  // top-right corner
        const C = new THREE.Vector2(0, -40);  // bottom apex
        const PANEL_THICKNESS = 6;

        // Each corner is cut back a fixed distance along its two edges, leaving a
        // hexagonal centre panel -- the wedge cut off each corner is exactly what
        // the corner pyramid below fills back in.
        const CUT = 14;
        const towards = (from, to, dist) => from.clone().add(to.clone().sub(from).normalize().multiplyScalar(dist));
        const panelPoints = [
            towards(A, B, CUT), towards(B, A, CUT),
            towards(B, C, CUT), towards(C, B, CUT),
            towards(C, A, CUT), towards(A, C, CUT),
        ];
        const panelShape = new THREE.Shape();
        panelShape.moveTo(panelPoints[0].x, panelPoints[0].y);
        for (let i = 1; i < panelPoints.length; i++) panelShape.lineTo(panelPoints[i].x, panelPoints[i].y);
        panelShape.closePath();
        const panelGeo = new THREE.ExtrudeGeometry(panelShape, { depth: PANEL_THICKNESS, bevelEnabled: false });

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.15, blending: THREE.AdditiveBlending, depthWrite: false });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(150, 150, 1);
        glow.position.z = -14;
        group.add(glow);

        // The panel, corner pyramids and core all share one slow tumble in animate()
        // -- the halo rings live outside this group so they keep their own
        // independent spin instead of tumbling with the rest.
        const spinGroup = new THREE.Group();
        group.add(spinGroup);

        // --- Centre panel: a real extruded solid (not a flat pane), the same
        // genuine-volume convention A.R.X.LOGOS's core and every cube in
        // A.R.X.LEXICO/A.R.X.LUCRE follow, so it keeps its shape and shows real
        // depth from any angle instead of only from face-on. DoubleSide means the
        // extrusion's face-winding direction doesn't need to be tracked by hand. ---
        const shellFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.24, depthWrite: false });
        const wireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.9 });

        const panelMesh = new THREE.Mesh(panelGeo, shellFillMat);
        spinGroup.add(panelMesh);
        const panelWire = new THREE.LineSegments(new THREE.EdgesGeometry(panelGeo, 20), wireMat);
        spinGroup.add(panelWire);

        // --- Three corner pyramids. Each one's base is the exact triangular wedge
        // the cut above removed (the true corner plus the two adjacent cut points),
        // sitting flush against the panel's front face -- so there is no XY gap
        // between panel and corner, whatever angle it's viewed from. The apex then
        // rises in Z off the centre of that base, so the corner reads as a real
        // pyramid poking forward rather than as a flat patch. ---
        const PYRAMID_HEIGHT = 12;
        const buildCornerPyramid = (cutPt1, corner, cutPt2) => {
            const cx = (cutPt1.x + corner.x + cutPt2.x) / 3;
            const cy = (cutPt1.y + corner.y + cutPt2.y) / 3;
            const b1 = new THREE.Vector3(cutPt1.x - cx, cutPt1.y - cy, 0);
            const b2 = new THREE.Vector3(corner.x - cx, corner.y - cy, 0);
            const b3 = new THREE.Vector3(cutPt2.x - cx, cutPt2.y - cy, 0);
            const apex = new THREE.Vector3(0, 0, PYRAMID_HEIGHT);
            const tris = [b1, b2, apex, b2, b3, apex, b3, b1, apex, b1, b3, b2];
            const positions = new Float32Array(tris.length * 3);
            tris.forEach((v, i) => { positions[i * 3] = v.x; positions[i * 3 + 1] = v.y; positions[i * 3 + 2] = v.z; });
            const geo = new THREE.BufferGeometry();
            geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
            geo.computeVertexNormals();
            return { geo, center: new THREE.Vector3(cx, cy, PANEL_THICKNESS) };
        };
        const cornerWedges = [
            buildCornerPyramid(panelPoints[5], A, panelPoints[0]),
            buildCornerPyramid(panelPoints[1], B, panelPoints[2]),
            buildCornerPyramid(panelPoints[3], C, panelPoints[4]),
        ];
        const corners = cornerWedges.map(({ geo, center }, i) => {
            const mesh = new THREE.Mesh(geo, shellFillMat);
            mesh.position.copy(center);
            spinGroup.add(mesh);
            const wire = new THREE.LineSegments(new THREE.EdgesGeometry(geo), wireMat);
            wire.position.copy(center);
            spinGroup.add(wire);
            return { mesh, wire, phase: i * 1.3 };
        });

        // --- Central glowing aperture, fixed hot colour, sitting just in front of
        // the panel where all three cut corners would have met. ---
        const coreFillMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending });
        const coreWireMat = new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9 });
        const coreGeo = new THREE.OctahedronGeometry(9, 0);
        coreGeo.scale(1, 0.6, 0.32);
        const corePos = new THREE.Vector3(0, -1, PANEL_THICKNESS + 3);
        const coreMesh = new THREE.Mesh(coreGeo, coreFillMat);
        coreMesh.position.copy(corePos);
        spinGroup.add(coreMesh);
        const coreWire = new THREE.LineSegments(new THREE.EdgesGeometry(coreGeo), coreWireMat);
        coreWire.position.copy(corePos);
        spinGroup.add(coreWire);

        const pupilMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95 });
        const pupil = new THREE.Mesh(new THREE.SphereGeometry(2.2, 14, 14), pupilMat);
        pupil.position.copy(corePos).setZ(corePos.z + 1.5);
        spinGroup.add(pupil);

        // A sprite pinned to the core's own position -- its billboard quad never has
        // to move as spinGroup tumbles, only ever glow brighter or dimmer in place
        // (see A.R.X.LEXICO/A.R.X.LUCRE for the same reasoning).
        const coreGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#d4fff0', '#33ffb2'), transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreGlowScale = 28;
        const coreGlow = new THREE.Sprite(coreGlowMat);
        coreGlow.position.copy(corePos);
        coreGlow.scale.set(coreGlowScale, coreGlowScale, 1);
        spinGroup.add(coreGlow);

        // --- Two halo rings, counter-rotating in the picture plane, framing the
        // whole triangle. ---
        const ringOuterMat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.035), color: api.palette.hex2, transparent: true, opacity: 0.4, depthWrite: false });
        const ringOuter = new THREE.Sprite(ringOuterMat);
        ringOuter.scale.set(128, 128, 1);
        group.add(ringOuter);

        const ringInnerMat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.06), color: api.palette.hex3, transparent: true, opacity: 0.55, depthWrite: false });
        const ringInner = new THREE.Sprite(ringInnerMat);
        ringInner.scale.set(106, 106, 1);
        group.add(ringInner);

        // --- A column of slow-rising data particles, wrapping from bottom to top --
        // standing in for the reference's floating binary streams. ---
        const STREAM_HALF_HEIGHT = 55;
        const streamCount = 80;
        const streamPositions = new Float32Array(streamCount * 3);
        for (let i = 0; i < streamCount; i++) {
            const angle = Math.random() * Math.PI * 2;
            const radius = 40 + Math.random() * 25;
            streamPositions[i * 3] = Math.cos(angle) * radius;
            streamPositions[i * 3 + 1] = (Math.random() * 2 - 1) * STREAM_HALF_HEIGHT;
            streamPositions[i * 3 + 2] = Math.sin(angle) * radius;
        }
        const streamGeo = new THREE.BufferGeometry();
        streamGeo.setAttribute('position', new THREE.BufferAttribute(streamPositions, 3));
        const streamMat = new THREE.PointsMaterial({ color: api.palette.hex, size: 1.3, transparent: true, opacity: 0.55 });
        const stream = new THREE.Points(streamGeo, streamMat);
        group.add(stream);

        return {
            group, spinGroup, stream, streamHalfHeight: STREAM_HALF_HEIGHT,
            glowMat, shellFillMat, wireMat, corners,
            coreMesh, coreWire, coreFillMat, coreWireMat, pupilMat,
            coreGlow, coreGlowMat, coreGlowScale,
            ringOuterMat, ringOuter, ringInnerMat, ringInner, streamMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * 1.5) * 3;
        model.spinGroup.rotation.y = ctx.time * 0.12;
        model.spinGroup.rotation.x = Math.sin(ctx.time * 0.4) * 0.1;

        model.ringOuter.rotation.z = ctx.time * 0.15;
        model.ringInner.rotation.z = -ctx.time * 0.22;

        // Rising data-stream particles, wrapping from bottom to top of the column.
        const positions = model.stream.geometry.attributes.position.array;
        const half = model.streamHalfHeight;
        for (let i = 1; i < positions.length; i += 3) {
            positions[i] += 0.35;
            if (positions[i] > half) positions[i] -= half * 2;
        }
        model.stream.geometry.attributes.position.needsUpdate = true;
        model.stream.rotation.y = ctx.time * 0.05;

        // A steady energy hum, boosted by speaking (audio) or a faster thinking pulse.
        const idlePulse = (Math.sin(ctx.time * 3) + 1) / 2;
        const speakPulse = isSpeaking ? ctx.audio : 0;
        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 6)) : 0;
        const pulse = idlePulse * 0.35 + speakPulse * 0.5 + thinkPulse * 0.4;

        model.shellFillMat.opacity = 0.2 + pulse * 0.16;
        model.wireMat.opacity = 0.75 + pulse * 0.2 + ctx.click * 0.1;
        model.corners.forEach(({ mesh, wire, phase }) => {
            const s = 1 + Math.sin(ctx.time * 2 + phase) * 0.03;
            mesh.scale.set(s, s, s);
            wire.scale.copy(mesh.scale);
        });

        model.coreFillMat.opacity = Math.min(1, 0.65 + pulse * 0.3 + ctx.click * 0.15);
        model.coreWireMat.opacity = 0.75 + ctx.click * 0.25;
        model.pupilMat.opacity = 0.85 + ctx.click * 0.15;
        const coreBreathe = 1 + Math.sin(ctx.time * 2) * 0.06 + ctx.click * 0.12;
        model.coreMesh.scale.set(coreBreathe, coreBreathe, coreBreathe);
        model.coreWire.scale.copy(model.coreMesh.scale);
        model.coreGlowMat.opacity = Math.min(1, 0.35 + pulse * 0.45);
        const coreGlowScale = 1 + pulse * 0.15 + ctx.click * 0.25;
        model.coreGlow.scale.set(model.coreGlowScale * coreGlowScale, model.coreGlowScale * coreGlowScale, 1);

        model.glowMat.opacity = 0.13 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.shellFillMat.color.setHex(palette.hex);
        model.wireMat.color.setHex(palette.hex2);
        model.glowMat.color.setHex(palette.hex);
        model.ringOuterMat.color.setHex(palette.hex2);
        model.ringInnerMat.color.setHex(palette.hex3);
        model.streamMat.color.setHex(palette.hex);
        // The core, its wireframe, pupil and glow stay fixed, this avatar's one hot
        // accent that never retints.
    },
});
