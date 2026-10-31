/* Avatar: The Nexus -- an elongated, bulbous cephalopod hull tapering into a forward
 * eye-lens cluster, drawn as a semi-transparent wireframe and point cloud rather than a
 * solid mesh, with ten long chain-link tentacles trailing behind it.
 *
 * Idle, the tentacles drift in slow jellyfish waves and the head bobs gently. Alert --
 * thinking, speaking, or tracking the pointer -- they spread outward with sharp angular
 * bends and swing forward toward whatever it is locked onto. It is set against a falling
 * NEXUS letter rain and a faint radar grid, with the CRT scanline dressing applied in CSS
 * through the viewportClass below.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'nexus',
    label: 'The Nexus',
    group: 'Trace Protocols',
    /* The CRT scanline and flicker overlay this one is watched through -- see
       #hologram-viewport.crt-active::after in A1theme.css and sprite.css. Only this
       avatar's live-sensor-feed look asks for it. */
    viewportClass: 'crt-active',

    build(api) {
        const THREE = api.THREE;
        const h = api.helpers;
        const group = new THREE.Group();

        /* --- Radar-grid backdrop: a single large, dim, additive sprite well behind the
           rain, reinforcing the dark translucent grid the rain alone does not give. */
        const gridMat = new THREE.SpriteMaterial({
            map: h.gridTexture(256, 10),
            color: api.palette.hex, transparent: true, opacity: 0.1,
            blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const gridSprite = new THREE.Sprite(gridMat);
        gridSprite.scale.set(340, 340, 1);
        gridSprite.position.set(0, 0, -150);
        group.add(gridSprite);

        /* --- NEXUS letter rain: real glyphs, not abstract points, falling straight down
           and wrapping top to bottom. */
        const letters = ['N', 'E', 'X', 'U', 'S'];
        const rainMaterials = letters.map((ch) => new THREE.SpriteMaterial({
            map: h.letterTexture(ch),
            color: api.palette.hex,
            transparent: true,
            opacity: 0.72,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
        }));

        const rainDrops = [];
        const rainCount = 220;
        const rainHalfWidth = 130;
        const rainTopY = 110;
        const rainBottomY = -110;
        const rainMinZ = -80;
        const rainMaxZ = -30;
        for (let i = 0; i < rainCount; i++) {
            const mat = rainMaterials[Math.floor(Math.random() * letters.length)];
            const sprite = new THREE.Sprite(mat);
            const scale = (6 + Math.random() * 5) * 1.1;
            sprite.scale.set(scale, scale, 1);
            sprite.position.set(
                (Math.random() - 0.5) * rainHalfWidth * 2,
                rainTopY + Math.random() * (rainTopY - rainBottomY),
                rainMinZ + Math.random() * (rainMaxZ - rainMinZ)
            );
            sprite.userData = {
                speed: 26 + Math.random() * 38,
                flickerPhase: Math.random() * Math.PI * 2,
            };
            rainDrops.push(sprite);
            group.add(sprite);
        }

        /* --- The hull and tentacles, in their own sub-group so they can turn without
           spinning the rain and grid backdrop along with them. Faces the camera (+Z) at
           rest; the tentacles trail toward -Z. */
        const creatureGroup = new THREE.Group();

        /* A fixed local tilt, one level inside creatureGroup and never touched by the
           per-frame look-around (which sets creatureGroup's own rotation directly).
           Without it the hull's elongation axis lines up exactly with the camera's view
           axis -- both Z -- and the elongated, bulbous, narrowing-at-the-nose shape is
           invisible: it reads as plain concentric rings instead of a torpedo profile.
           Tilting the body a few tens of degrees off-axis puts that profile back in view
           while the creature still reads as facing the camera or the pointer. */
        const restTilt = new THREE.Group();
        restTilt.rotation.set(0.18, 0.55, 0.06);
        creatureGroup.add(restTilt);

        /* Elongated, bulbous chassis: a lathe-revolved profile (radius, height along the
           axis) gives a smooth torpedo hull that a warped sphere cannot -- wide through the
           mid-body, narrowing at both ends, tapering to a near-point at the nose. Built
           with height along the profile's own Y, then rotated so that axis becomes Z
           (forward), matching every other avatar's front-facing convention. */
        const hullProfile = [
            [0.8, -24], [6, -19], [10, -11], [12.5, -2], [13.5, 5],
            [12, 11], [8.5, 16], [4.5, 19.5], [1.2, 22],
        ].map(([r, z]) => new THREE.Vector2(r, z));
        const hullGeom = new THREE.LatheGeometry(hullProfile, 24);
        hullGeom.rotateX(Math.PI / 2); // profile height -> Z, matching the dir convention below

        /* A vertical gradient (dim underside fading to a bright crown) through vertex
           colours -- it multiplies with material.color, so theme tinting still tints a
           gradient rather than flattening it to one fill. */
        const hullPos = hullGeom.attributes.position;
        const hullBounds = new THREE.Box3().setFromBufferAttribute(hullPos);
        const hullMinY = hullBounds.min.y, hullMaxY = hullBounds.max.y;
        const hullColors = new Float32Array(hullPos.count * 3);
        for (let i = 0; i < hullPos.count; i++) {
            const t = (hullPos.getY(i) - hullMinY) / (hullMaxY - hullMinY || 1);
            const lum = 0.32 + t * 0.68;
            hullColors[i * 3] = lum;
            hullColors[i * 3 + 1] = lum;
            hullColors[i * 3 + 2] = lum;
        }
        hullGeom.setAttribute('color', new THREE.BufferAttribute(hullColors, 3));

        /* Wireframe and point-cloud rendering, not a solid textured mesh:
             - a near-invisible fill, just enough to give the point cloud something to sit on
             - a dense inner wireframe (every triangle edge) at low opacity, the inner
               structural lines
             - a sparser outer edge set (edges past a wide angle threshold, which reads as
               the hull's structural ribs and silhouette) at high additive opacity, brighter
               than the inner lines: the depth cue the whole look rests on
             - a sparse point cloud at every vertex, brighter still, so the nodes read as
               the brightest thing on the hull */
        const hullFillMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, vertexColors: true, transparent: true, opacity: 0.05, depthWrite: false,
        });
        const headMesh = new THREE.Mesh(hullGeom, hullFillMat);
        restTilt.add(headMesh);

        const hullInnerWireMat = new THREE.LineBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.22,
        });
        const headInnerWire = new THREE.LineSegments(new THREE.WireframeGeometry(hullGeom), hullInnerWireMat);
        restTilt.add(headInnerWire);

        const headOutlineMat = new THREE.LineBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
        });
        const headOutline = new THREE.LineSegments(new THREE.EdgesGeometry(hullGeom, 30), headOutlineMat);
        restTilt.add(headOutline);

        const hullPointsMat = new THREE.PointsMaterial({
            color: api.palette.hex3, map: h.glowTexture(24), size: 2.6,
            transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        const headPoints = new THREE.Points(hullGeom, hullPointsMat);
        restTilt.add(headPoints);

        /* Squashed to an oval head-on: same width, 60% of the height. Applied as an
           object-level scale rather than baked into the geometry, so it is one shared
           factor across all four hull representations, and so the ribs and eye cluster --
           children of headMesh -- flatten along with it instead of needing their own
           hand-adjusted coordinates. The breathing pulse in animate multiplies on top of
           this base scale rather than overwriting it, so the proportions hold through it. */
        const headHeightScale = 0.6;
        const headParts = [headMesh, headInnerWire, headOutline, headPoints];
        headParts.forEach((obj) => obj.scale.set(1, headHeightScale, 1));

        /* Ribbed carapace bands -- three thin wireframe rings encircling the hull like a
           segmented armoured shell, their radii approximating the hull's own profile at
           each Z so they sit flush on the surface instead of floating inside or outside it. */
        const ribMat = new THREE.LineBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending,
        });
        [{ z: -10, r: 11.7 }, { z: 0, r: 13.2 }, { z: 9, r: 12.2 }].forEach(({ z, r }) => {
            const ribGeom = new THREE.TorusGeometry(r, 0.3, 6, 32);
            const rib = new THREE.LineSegments(new THREE.EdgesGeometry(ribGeom, 20), ribMat);
            rib.position.z = z;
            headMesh.add(rib); // a child of the hull, so it pulses and scales along with it
        });

        /* Forward eye-lens cluster -- an irregular, hand-placed scatter of eight lenses of
           varied size, sitting close to the hull's narrow nose. Fixed phosphor red whatever
           the theme, like every avatar's always-lit accent. */
        const eyeMat = new THREE.MeshBasicMaterial({
            color: 0xff2418, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        const eyeGlowMat = new THREE.SpriteMaterial({
            map: h.glowTexture(32), color: 0xff2418, transparent: true, opacity: 0.55,
            blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const eyeRingMat = new THREE.SpriteMaterial({
            map: h.ringTexture(64, 0.16), color: 0xffdca0, transparent: true, opacity: 0.8,
            blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const eyeHighlightMat = new THREE.SpriteMaterial({
            map: h.glowTexture(32), color: 0xffffff, transparent: true, opacity: 0.95,
            depthWrite: false,
        });
        // [x, y, z, radius] before the nose-fitting scale below.
        const eyeLayout = [
            [-7.5, -4, 15.5, 2.2], [-4.5, -6.5, 17, 1.8], [-1, -3.5, 18.5, 2.6],
            [2.5, -6, 17.5, 1.9], [6, -3.5, 16, 2.3], [8, -6.5, 14, 1.5],
            [0, -8, 15.5, 1.6], [-2.5, -1.5, 18.8, 1.4],
        ];
        const eyeXyScale = 0.75;
        const eyes = [];
        eyeLayout.forEach(([rawEx, rawEy, rawEz, er]) => {
            const ex = rawEx * eyeXyScale;
            const ey = rawEy * eyeXyScale;
            const ez = rawEz + 1;

            const eye = new THREE.Mesh(new THREE.SphereGeometry(er, 8, 8), eyeMat);
            eye.position.set(ex, ey, ez);
            headMesh.add(eye); // a child of the hull, so the cluster pulses and scales with it

            const glow = new THREE.Sprite(eyeGlowMat);
            glow.scale.setScalar(er * 3.2);
            glow.position.set(ex, ey, ez);
            headMesh.add(glow);

            const ring = new THREE.Sprite(eyeRingMat);
            ring.scale.setScalar(er * 2.6);
            ring.position.set(ex, ey, ez + 0.3);
            headMesh.add(ring);

            const highlight = new THREE.Sprite(eyeHighlightMat);
            highlight.scale.setScalar(er * 0.85);
            highlight.position.set(ex - er * 0.35, ey + er * 0.35, ez + 0.6);
            headMesh.add(highlight);

            eyes.push({ mesh: eye, glow, ring, highlight });
        });

        /* --- Tentacles: ten long, segmented, chain-like tendrils trailing from the hull's
           tail. Each tentacle's joint positions live in one shared Float32Array, rewritten
           every frame by animate; a chain of tapered cylinder links spans every consecutive
           pair for real, controllable thickness -- a plain Line's width cannot be relied on
           across browsers and GPUs in WebGL, so this is deliberately actual geometry rather
           than a wide line. A point cloud at the same positions gives the glowing joint
           markers, brighter than the links between them: the same nodes-brighter-than-lines
           cue as the hull, and together they read as banded chain links. */
        const tentacles = [];
        const tentacleCount = 10;
        const segmentsPerTentacle = 21;
        // One unit cylinder along Y, shared and rescaled per link per frame.
        const linkUnitGeom = new THREE.CylinderGeometry(1, 1, 1, 6);
        const tentacleLinkMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.65,
        });
        const tentaclePointMat = new THREE.PointsMaterial({
            color: api.palette.hex3, map: h.glowTexture(24), size: 3.2,
            transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        // Pincer tips -- three wireframe prongs per tentacle, the same fixed red as the eye
        // cluster, so every always-lit part of this avatar reads as one accent colour.
        const clawMat = new THREE.LineBasicMaterial({
            color: 0xff2418, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        const clawGlowMat = new THREE.SpriteMaterial({
            map: h.glowTexture(24), color: 0xff2418, transparent: true, opacity: 0.4,
            blending: THREE.AdditiveBlending, depthWrite: false,
        });
        for (let t = 0; t < tentacleCount; t++) {
            const baseAngle = (t / tentacleCount) * Math.PI * 2;
            const geom = new THREE.BufferGeometry();
            const positions = new Float32Array(segmentsPerTentacle * 3);
            geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));

            const points = new THREE.Points(geom, tentaclePointMat);
            restTilt.add(points);

            // One link per gap between consecutive joints; animate rescales and reorients
            // each one every frame to span its two endpoints, tapering toward the tip.
            const links = [];
            for (let l = 0; l < segmentsPerTentacle - 1; l++) {
                const link = new THREE.Mesh(linkUnitGeom, tentacleLinkMat);
                restTilt.add(link);
                links.push(link);
            }

            const claws = [];
            for (let c = 0; c < 3; c++) {
                const clawGeom = new THREE.ConeGeometry(0.75, 3.4, 6);
                const claw = new THREE.LineSegments(new THREE.EdgesGeometry(clawGeom, 1), clawMat);
                const clawGlow = new THREE.Sprite(clawGlowMat);
                clawGlow.scale.setScalar(2.4);
                claw.add(clawGlow);
                restTilt.add(claw);
                claws.push(claw);
            }

            tentacles.push({
                geom, positions, points, links, claws, baseAngle,
                // Per-tentacle speed and phase variance, so they never move in lockstep
                // whether drifting calmly or lunging.
                speedMult: 0.85 + Math.random() * 0.4,
                phaseSeed: Math.random() * Math.PI * 2,
                aggroT: 0, // eased 0 (drifting) .. 1 (lunging), see animate
            });
        }

        group.add(creatureGroup);

        return {
            group,
            gridMat, rainDrops, rainMaterials,
            creatureGroup, restTilt,
            headMesh, headInnerWire, headOutline, headPoints, headParts, headHeightScale,
            hullFillMat, hullInnerWireMat, headOutlineMat, hullPointsMat, ribMat,
            eyes, eyeRingMat,
            tentacles, tentacleLinkMat, tentaclePointMat,
            // Where the head is actually pointing, eased toward its target every frame.
            facing: { yaw: 0, pitch: 0, roll: 0 },
            // Scratch vectors, built once rather than per frame -- animate runs sixty times
            // a second over ten tentacles and would otherwise allocate thousands of these.
            tmp: {
                tip: new THREE.Vector3(), prev: new THREE.Vector3(),
                linkA: new THREE.Vector3(), linkB: new THREE.Vector3(), linkGap: new THREE.Vector3(),
                up: new THREE.Vector3(0, 1, 0),
            },
        };
    },

    applyPalette(model, p) {
        /* The near-invisible fill and the dense inner wireframe pick up the base hue; the
           sparser structural outline, the points and the ribs pick up hex3 so they read
           distinctly brighter -- the inner-lines-dimmer-than-outer-edges depth cue. The
           eye-lens cluster and the pincer tips are a fixed red whatever the theme (see
           build), like every avatar's always-lit accent. */
        model.hullFillMat.color.setHex(p.hex);
        model.hullInnerWireMat.color.setHex(p.hex);
        model.headOutlineMat.color.setHex(p.hex3);
        model.hullPointsMat.color.setHex(p.hex3);
        model.ribMat.color.setHex(p.hex3);
        model.gridMat.color.setHex(p.hex);
        model.tentacleLinkMat.color.setHex(p.hex);
        model.tentaclePointMat.color.setHex(p.hex3);
        model.rainMaterials.forEach((mat) => mat.color.setHex(p.hex));
    },

    animate(model, ctx) {
        const THREE = ctx.THREE;
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const t = ctx.time;
        const pointerActive = ctx.pointer.idleFor < 1.2;
        /* Idle and alone it just drifts. The moment it is thinking, speaking or actively
           tracking the pointer it counts as aggressive: the tentacles spread and point
           forward at whatever it is locked onto. */
        const isAggressive = isThinking || isSpeaking || pointerActive;

        /* Rain falls straight down and wraps top to bottom -- ambient background, always
           running, since it is the scene behind the creature rather than the creature
           reacting. The radar grid behind it sweeps slowly and continuously, like a live
           CRT scan. */
        model.gridMat.rotation += 0.0015;
        const rainSpeedMult = isThinking ? 1.8 : (isSpeaking ? 1.3 : 1.0);
        model.rainDrops.forEach((drop) => {
            drop.position.y -= drop.userData.speed * 0.016 * rainSpeedMult;
            if (drop.position.y < -110) {
                drop.position.y = 110;
                drop.position.x = (Math.random() - 0.5) * 260;
            }
            /* A mostly-dim field with occasional brighter glyphs standing out, rather than
               a smooth sine flicker that spends equal time bright and dim. Raising a
               clamped sine to a power keeps it near the dim floor for most of the cycle and
               spikes toward the ceiling only briefly -- it reads as a sensor feed's static. */
            const flicker = Math.max(0, Math.sin(t * 4 + drop.userData.flickerPhase));
            drop.material.opacity = 0.22 + Math.pow(flicker, 6) * 0.68;
        });

        /* A shared, fast, low-amplitude jitter applied to every wireframe and point
           material's opacity below -- the slight flickering of a live sensor feed, without
           needing a post-processing shader pass. */
        const crtFlicker = 0.88 + 0.12 * Math.sin(t * 41 + Math.sin(t * 6.7) * 3);

        /* Looks around while thinking, gives a slight attentive tilt while speaking, and
           tracks the pointer directly while it is active over the viewport. The instant
           none of that applies the head eases into a slow drift instead of hunting for
           anything -- the calm half of the drift-or-lunge split. */
        let targetYaw = 0;
        let targetPitch = 0;
        let targetRoll = 0;
        if (isThinking) {
            targetYaw = Math.sin(t * 0.8) * 0.35;
            targetPitch = Math.cos(t * 0.6) * 0.2;
        } else if (isSpeaking) {
            targetYaw = Math.sin(t * 1.4) * 0.15;
            targetPitch = Math.sin(t * 1.1) * 0.1;
        } else if (pointerActive) {
            targetYaw = ctx.pointer.x * 0.55;
            targetPitch = -ctx.pointer.y * 0.38;
        } else {
            const driftPhase = t * 0.18;
            targetYaw = Math.sin(driftPhase) * 0.14;
            targetPitch = Math.sin(driftPhase * 0.7 + 1.1) * 0.1;
            targetRoll = Math.sin(driftPhase * 0.5 + 0.6) * 0.06;
        }
        targetYaw += ctx.click * 0.3;
        targetPitch -= ctx.click * 0.15;
        const facing = model.facing;
        facing.yaw += (targetYaw - facing.yaw) * 0.05;
        facing.pitch += (targetPitch - facing.pitch) * 0.05;
        facing.roll += (targetRoll - facing.roll) * 0.05;
        model.creatureGroup.rotation.set(facing.pitch, facing.yaw, facing.roll);
        model.creatureGroup.position.y = ctx.click * 6;

        /* The hull's fill, wireframe, outline and points share one geometry but are
           separate siblings rather than parent and child, so the breathing pulse is applied
           to all four explicitly and the flicker opacities are re-applied every frame. */
        let headScale;
        if (isSpeaking) headScale = 1 + ctx.audio * 0.3;
        else if (isThinking) headScale = 1 + Math.sin(t * 10) * 0.08;
        else headScale = 1 + ctx.click * 0.12;
        // Keep the oval proportions through the pulse.
        const headScaleY = headScale * model.headHeightScale;
        model.headParts.forEach((obj) => obj.scale.set(headScale, headScaleY, headScale));
        model.hullInnerWireMat.opacity = 0.22 * crtFlicker;
        model.headOutlineMat.opacity = 0.9 * crtFlicker;
        model.hullPointsMat.opacity = 0.85 * crtFlicker;
        model.ribMat.opacity = 0.55 * crtFlicker;

        /* The sensor-node cluster blinks together every few seconds while idle -- a quick
           vertical squash and recover. Held open while thinking, speaking or tracking, so
           it does not compete with the more reactive motion. */
        let closeAmt = 0;
        if (!isAggressive) {
            const blinkPeriod = 4.2;
            const blinkWindow = 0.18;
            const blinkCycle = t % blinkPeriod;
            if (blinkCycle > blinkPeriod - blinkWindow) {
                const bt = (blinkCycle - (blinkPeriod - blinkWindow)) / blinkWindow;
                closeAmt = Math.sin(bt * Math.PI);
            }
        }
        const eyeScaleY = 1 - closeAmt * 0.9;
        model.eyeRingMat.rotation += isThinking ? 0.028 : (isSpeaking ? 0.02 : 0.008);
        model.eyes.forEach(({ mesh, glow, ring, highlight }) => {
            mesh.scale.y = eyeScaleY;
            glow.scale.y = glow.scale.x * eyeScaleY;
            ring.scale.y = ring.scale.x * eyeScaleY;
            highlight.scale.y = highlight.scale.x * eyeScaleY;
        });

        /* Tentacles: idle, a slow single-frequency wave curves them gently backward with a
           light vertical bob -- a jellyfish drifting. Aggressive, the wave sharpens into
           angular zigzagging bends, they spread wider, and they swing from trailing behind
           to pointing forward at the tracked target. Every tentacle eases its own aggroT
           independently toward the shared target so the cluster does not snap in lockstep. */
        const aimX = pointerActive ? ctx.pointer.x : 0;
        const aimY = pointerActive ? -ctx.pointer.y : 0;
        const tailR = 6.5;
        const tailZ = -20;
        const idleSpeed = 0.9, aggroSpeed = 5.5;
        const idleSway = 0.55, aggroSway = 1.05;
        const idleSpreadPerAlong = 1.1, aggroSpreadPerAlong = 3.4;
        const idleForwardPerAlong = -2.1, aggroForwardPerAlong = 3.0;
        const idleSharpness = 1.0, aggroSharpness = 0.35;
        const LINK_BASE_RADIUS = 0.55;
        const LINK_TIP_RADIUS = 0.22;
        const tmp = model.tmp;

        model.tentacles.forEach((tentacle) => {
            tentacle.aggroT += ((isAggressive ? 1 : 0) - tentacle.aggroT) * 0.06;
            const at = tentacle.aggroT;
            const tSpeed = (idleSpeed + (aggroSpeed - idleSpeed) * at) * tentacle.speedMult;
            const swayScale = idleSway + (aggroSway - idleSway) * at;
            const spreadPerAlong = idleSpreadPerAlong + (aggroSpreadPerAlong - idleSpreadPerAlong) * at;
            const forwardPerAlong = idleForwardPerAlong + (aggroForwardPerAlong - idleForwardPerAlong) * at;
            const sharpness = idleSharpness + (aggroSharpness - idleSharpness) * at;

            const dirX = Math.cos(tentacle.baseAngle);
            const dirY = Math.sin(tentacle.baseAngle) * 0.6;
            const perpX = -dirY;
            const segCount = tentacle.positions.length / 3;

            for (let sIdx = 0; sIdx < segCount; sIdx++) {
                const along = sIdx + 1;
                const wavePhase = t * tSpeed + tentacle.baseAngle * 3 + tentacle.phaseSeed;
                const raw = Math.sin(wavePhase - along * 0.6);
                /* sign(raw) * |raw|^sharpness is exactly raw at sharpness 1 (a plain smooth
                   sine, the drifting case) and squares toward an angular zigzag as sharpness
                   falls to the aggressive value -- one formula covers both extremes and
                   everything eased between them. */
                const shaped = Math.sign(raw) * Math.pow(Math.abs(raw), sharpness);
                const bend = shaped * along * swayScale;
                const bobV = Math.sin(wavePhase * 0.7 + along * 0.3 + 1.6) * along * swayScale * 0.5;

                tentacle.positions[sIdx * 3] = dirX * (tailR + along * spreadPerAlong) + perpX * bend + aimX * along * 1.6 * at;
                tentacle.positions[sIdx * 3 + 1] = dirY * (tailR * 0.6) - along * 0.4 * (1 - at) + bobV + aimY * along * 1.1 * at;
                tentacle.positions[sIdx * 3 + 2] = tailZ + along * forwardPerAlong;
            }
            tentacle.geom.attributes.position.needsUpdate = true;
            tentacle.geom.computeBoundingSphere();

            // Chain links: one tapered cylinder per gap between consecutive joints,
            // rescaled and reoriented to span its two endpoints every frame.
            tentacle.links.forEach((link, lIdx) => {
                tmp.linkA.set(tentacle.positions[lIdx * 3], tentacle.positions[lIdx * 3 + 1], tentacle.positions[lIdx * 3 + 2]);
                tmp.linkB.set(tentacle.positions[(lIdx + 1) * 3], tentacle.positions[(lIdx + 1) * 3 + 1], tentacle.positions[(lIdx + 1) * 3 + 2]);
                tmp.linkGap.subVectors(tmp.linkB, tmp.linkA);
                const len = tmp.linkGap.length();
                const lt = lIdx / (tentacle.links.length - 1); // 0 at the base, 1 at the tip
                const radius = LINK_BASE_RADIUS + (LINK_TIP_RADIUS - LINK_BASE_RADIUS) * lt;
                link.position.copy(tmp.linkA).addScaledVector(tmp.linkGap, 0.5);
                link.scale.set(radius, len, radius);
                if (len > 0.0001) {
                    link.quaternion.setFromUnitVectors(tmp.up, tmp.linkGap.normalize());
                }
            });

            // Clawed pincers at the tip -- three prongs fanned around the last segment,
            // oriented to continue the tentacle's current direction of travel.
            tmp.tip.set(tentacle.positions[(segCount - 1) * 3], tentacle.positions[(segCount - 1) * 3 + 1], tentacle.positions[(segCount - 1) * 3 + 2]);
            tmp.prev.set(tentacle.positions[(segCount - 2) * 3], tentacle.positions[(segCount - 2) * 3 + 1], tentacle.positions[(segCount - 2) * 3 + 2]);
            const dir = new THREE.Vector3().subVectors(tmp.tip, tmp.prev).normalize();
            const arbitrary = Math.abs(dir.y) < 0.9 ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(1, 0, 0);
            const fanA = new THREE.Vector3().crossVectors(dir, arbitrary).normalize();
            const fanB = new THREE.Vector3().crossVectors(dir, fanA).normalize();
            tentacle.claws.forEach((claw, ci) => {
                const clawAngle = (ci / tentacle.claws.length) * Math.PI * 2;
                const spread = fanA.clone().multiplyScalar(Math.cos(clawAngle) * 1.1)
                    .add(fanB.clone().multiplyScalar(Math.sin(clawAngle) * 1.1));
                claw.position.copy(tmp.tip).addScaledVector(dir, 1.6).add(spread);
                claw.quaternion.setFromUnitVectors(
                    new THREE.Vector3(0, 1, 0),
                    dir.clone().addScaledVector(spread, 0.35).normalize()
                );
            });
        });
        model.tentacleLinkMat.opacity = 0.65 * crtFlicker;
        model.tentaclePointMat.opacity = 0.9 * crtFlicker;
    },
});
