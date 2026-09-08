/* Avatar: Nexus Sent -- a small, square-tube brass emitter ring casting a column of
 * light that gathers into a hovering, wireframe/point-cloud hull with a burst of
 * tentacles trailing from its tail.
 *
 * The hull, its rib bands and its forward eye-lens cluster reuse the Nexus's own
 * technique (see avatar-nexus.js's buildNexusAvatar) at a smaller scale, with a small
 * obsidian core nested in its belly -- the one piece of hardware every avatar here
 * keeps at its center. The tentacles are a dense, asynchronous swimming cluster: each
 * one ripples on its own traveling wave (root anchored, amplitude growing toward the
 * tip, its own frequency and amplitude slightly off from its neighbors') so the whole
 * burst reads as many independent limbs swimming rather than one choreographed ripple.
 * Inspired by the general idea of a many-limbed creature swimming/whipping through
 * water -- not a copy of any one specific design -- and built throughout in this
 * engine's own point-cloud/wireframe language.
 *
 * The ring and the hull are both fixed/lit (Phong) like every avatar's obsidian core;
 * the beam, hull wireframe and tentacle strands are drawn light and follow the color
 * theme. The nested core, the eye cluster and the tentacle joints/tips stay a fixed
 * red regardless of theme, the same "hot accents don't retint" convention R.E.D. 9000's
 * lens and the Nexus's eyes follow.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'senti',
    label: 'Nexus Sent',

    build(api) {
        const group = new THREE.Group();
        const HOT = 0xff2418; // the eye/claw accent -- fixed, not theme-tinted

        // --- Emitter ring: a circular loop (like a torus) swept with a square cross-section
        // instead of a round one, via ExtrudeGeometry's extrudePath along a circular curve. ---
        class CircularPath extends THREE.Curve {
            constructor(radius) { super(); this.radius = radius; }
            getPoint(t, target = new THREE.Vector3()) {
                const angle = t * Math.PI * 2;
                return target.set(Math.cos(angle) * this.radius, 0, Math.sin(angle) * this.radius);
            }
        }

        function squareTubeRingGeometry(pathRadius, half, segments = 64) {
            const crossSection = new THREE.Shape();
            crossSection.moveTo(-half, -half);
            crossSection.lineTo(half, -half);
            crossSection.lineTo(half, half);
            crossSection.lineTo(-half, half);
            crossSection.closePath();

            return new THREE.ExtrudeGeometry(crossSection, {
                steps: segments,
                bevelEnabled: false,
                extrudePath: new CircularPath(pathRadius)
            });
        }

        const ringMat = new THREE.MeshPhongMaterial({ color: 0x8a6a35, specular: 0xffdca0, shininess: 70 });
        const ring = new THREE.Mesh(squareTubeRingGeometry(40, 5), ringMat);
        ring.position.y = -50;
        group.add(ring);

        const lipMat = new THREE.MeshPhongMaterial({ color: 0x2e2013, specular: 0xd8a45c, shininess: 40 });
        const lip = new THREE.Mesh(squareTubeRingGeometry(29, 2.4), lipMat);
        lip.position.y = -50;
        group.add(lip);

        // --- Projection beam: a tapering column of points rising from the ring. ---
        const beamCount = 900;
        const beamBottomY = -46;
        const beamTopY = -2;
        const beamPositions = new Float32Array(beamCount * 3);
        for (let i = 0; i < beamCount; i++) {
            const t = Math.random();
            const y = THREE.MathUtils.lerp(beamBottomY, beamTopY, t);
            const r = THREE.MathUtils.lerp(26, 3, t) * (0.35 + Math.random() * 0.65);
            const a = Math.random() * Math.PI * 2;
            beamPositions[i * 3] = Math.cos(a) * r;
            beamPositions[i * 3 + 1] = y;
            beamPositions[i * 3 + 2] = Math.sin(a) * r;
        }
        const beamGeo = new THREE.BufferGeometry();
        beamGeo.setAttribute('position', new THREE.BufferAttribute(beamPositions, 3));
        const beamMaterial = new THREE.PointsMaterial({
            color: api.palette.hex, map: api.helpers.glowTexture(24), size: 1.8,
            transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const beamPoints = new THREE.Points(beamGeo, beamMaterial);
        group.add(beamPoints);

        // --- The body: built at the Nexus hull's own native scale (roughly 46 units
        // nose-to-tail), then this one group scales the whole assembly down to sit
        // proportionately atop the beam. bodyBaseY is animate()'s bob baseline. ---
        const bodyScale = 0.62;
        const bodyBaseY = 18;
        const bodyGroup = new THREE.Group();
        bodyGroup.scale.setScalar(bodyScale);
        bodyGroup.position.y = bodyBaseY;
        group.add(bodyGroup);

        // A fixed local tilt so the hull's own elongation axis isn't staring straight down
        // the camera's Z axis (see avatar-nexus.js's nexusRestTiltGroup for why).
        const tiltGroup = new THREE.Group();
        tiltGroup.rotation.set(0.18, 0.4, 0.06);
        bodyGroup.add(tiltGroup);

        const hullProfile = [
            [0.8, -24], [6, -19], [10, -11], [12.5, -2], [13.5, 5],
            [12, 11], [8.5, 16], [4.5, 19.5], [1.2, 22]
        ].map(([r, z]) => new THREE.Vector2(r, z));
        const hullGeom = new THREE.LatheGeometry(hullProfile, 24);
        hullGeom.rotateX(Math.PI / 2);

        // Vertical gradient (dim underside, bright crown) via vertex colors, multiplying
        // with material.color so theme tinting still tints a gradient rather than a flat fill.
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

        const hullFillMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, vertexColors: true, transparent: true, opacity: 0.06, depthWrite: false
        });
        const hullMesh = new THREE.Mesh(hullGeom, hullFillMat);
        tiltGroup.add(hullMesh);

        const hullInnerWireMat = new THREE.LineBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.22
        });
        const hullInnerWire = new THREE.LineSegments(new THREE.WireframeGeometry(hullGeom), hullInnerWireMat);
        tiltGroup.add(hullInnerWire);

        const hullOutlineMat = new THREE.LineBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending
        });
        const hullOutline = new THREE.LineSegments(new THREE.EdgesGeometry(hullGeom, 30), hullOutlineMat);
        tiltGroup.add(hullOutline);

        const hullPointsMat = new THREE.PointsMaterial({
            color: api.palette.hex3, map: api.helpers.glowTexture(24), size: 2.6,
            transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const hullPoints = new THREE.Points(hullGeom, hullPointsMat);
        tiltGroup.add(hullPoints);

        // Squash to an oval head-on, same 0.6 factor the Nexus uses.
        [hullMesh, hullInnerWire, hullOutline, hullPoints].forEach((obj) => obj.scale.set(1, 0.6, 1));

        // Ribbed carapace bands, theme-tinted.
        const ribMat = new THREE.LineBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending
        });
        [{ z: -10, r: 11.7 }, { z: 0, r: 13.2 }, { z: 9, r: 12.2 }].forEach(({ z, r }) => {
            const ribGeom = new THREE.TorusGeometry(r, 0.3, 6, 32);
            const rib = new THREE.LineSegments(new THREE.EdgesGeometry(ribGeom, 20), ribMat);
            rib.position.z = z;
            hullMesh.add(rib);
        });

        // A small obsidian core nested inside the hull's belly, glimpsed through the
        // sparse wireframe -- the one fixed-dark, Phong-lit core every avatar here keeps.
        const nestedCoreMat = new THREE.MeshPhongMaterial({
            color: 0x0a0a0f, specular: HOT, shininess: 90, transparent: true, opacity: 0.97
        });
        const nestedCore = new THREE.Mesh(new THREE.IcosahedronGeometry(6, 1), nestedCoreMat);
        nestedCore.position.z = -2;
        hullMesh.add(nestedCore);

        // Forward eye-lens cluster, fixed red regardless of theme.
        const eyeMat = new THREE.MeshBasicMaterial({
            color: HOT, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
        });
        const eyeGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(32), color: HOT, transparent: true,
            opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const eyeLayout = [
            [-7.5, -4, 15.5, 2.2], [-4.5, -6.5, 17, 1.8], [-1, -3.5, 18.5, 2.6],
            [2.5, -6, 17.5, 1.9], [6, -3.5, 16, 2.3], [8, -6.5, 14, 1.5],
            [0, -8, 15.5, 1.6], [-2.5, -1.5, 18.8, 1.4]
        ];
        const eyeXyScale = 0.75;
        eyeLayout.forEach(([rawEx, rawEy, rawEz, er]) => {
            const ex = rawEx * eyeXyScale, ey = rawEy * eyeXyScale, ez = rawEz + 1;
            const eye = new THREE.Mesh(new THREE.SphereGeometry(er, 8, 8), eyeMat);
            eye.position.set(ex, ey, ez);
            hullMesh.add(eye);

            const glow = new THREE.Sprite(eyeGlowMat);
            glow.scale.setScalar(er * 3.2);
            glow.position.set(ex, ey, ez);
            hullMesh.add(glow);
        });

        // --- Tentacles: a dense, asynchronous swimming cluster trailing from the hull's
        // tail, mostly backward (-Z) with a wide fan. Each tentacle is a chain of joints
        // sharing one BufferGeometry -- a thin Line for the strand itself (theme-tinted)
        // and a Points overlay at the same joints for brighter node markers (fixed red).
        // animate() rewrites the position attribute every frame -- a cheap kinematic
        // update, not a rebuild -- with each tentacle's own frequency/amplitude/phase
        // slightly off from its neighbors', so the burst reads as many independent limbs
        // swimming rather than one choreographed ripple. ---
        const tentacleLineMat = new THREE.LineBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.6
        });
        const tentacleNodeMat = new THREE.PointsMaterial({
            color: HOT, map: api.helpers.glowTexture(20), size: 2.8,
            transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const tentacleTipMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(24), color: HOT, transparent: true,
            opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false
        });

        const tailPoint = new THREE.Vector3(0, 0, -24); // the hull profile's own tail tip
        const tentacleCount = 15;
        const jointCount = 10;
        const tentacles = [];
        for (let i = 0; i < tentacleCount; i++) {
            const azimuth = (i / tentacleCount) * Math.PI * 2 + (Math.random() - 0.5) * 0.5;
            const fan = 0.62; // how wide the tentacles splay from straight-back
            const dir = new THREE.Vector3(
                Math.cos(azimuth) * fan,
                Math.sin(azimuth) * fan * 0.65,
                -1
            ).normalize();
            const perpA = new THREE.Vector3().crossVectors(dir, new THREE.Vector3(0, 1, 0));
            if (perpA.lengthSq() < 1e-6) perpA.set(1, 0, 0);
            perpA.normalize();
            const perpB = new THREE.Vector3().crossVectors(dir, perpA).normalize();

            const positions = new Float32Array(jointCount * 3);
            const geom = new THREE.BufferGeometry();
            geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));

            const line = new THREE.Line(geom, tentacleLineMat);
            const nodes = new THREE.Points(geom, tentacleNodeMat);
            tiltGroup.add(line, nodes);

            const tip = new THREE.Sprite(tentacleTipMat);
            tip.scale.set(4.2, 4.2, 1);
            tiltGroup.add(tip);

            tentacles.push({
                positions, geom, tip, dir, perpA, perpB,
                length: 27 + Math.random() * 15,
                phase: Math.random() * Math.PI * 2,
                // Independent variance per tentacle -- not just a shared phase offset --
                // is what keeps the whole cluster from reading as one wave repeated N
                // times; each limb genuinely swims to its own rhythm.
                speedMult: 0.7 + Math.random() * 0.6,
                freqMult: 0.75 + Math.random() * 0.5,
                ampMult: 0.75 + Math.random() * 0.5
            });
        }

        const glowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true,
            opacity: 0.2, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const glow = new THREE.Sprite(glowMaterial);
        glow.scale.set(170, 170, 1);
        glow.position.set(0, -20, -30);
        group.add(glow);

        return {
            group, bodyGroup, bodyBaseY, beamPoints, beamMaterial,
            hullFillMat, hullInnerWireMat, hullOutlineMat, hullPointsMat, ribMat,
            tentacleLineMat, tentacles, tailPoint, eyeGlowMat, glow, glowMaterial
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.rotation.y = ctx.time * (isThinking ? 1.1 : 0.25);

        const bob = Math.sin(ctx.time * (isThinking ? 3.2 : 1.1)) * (isThinking ? 3 : 5);
        const audioBob = isSpeaking ? ctx.audio * 8 : 0;
        model.bodyGroup.position.y = model.bodyBaseY + bob + audioBob + ctx.click * 10;

        // Tentacles are always swimming, even at rest -- a lower amplitude/speed floor
        // than Senti's original resting droop, since "swimming" reads as continuous
        // motion rather than something that only animates once alert. Thinking/speaking
        // push the whole cluster wider and faster on top of that baseline.
        const droop = (isThinking || isSpeaking) ? -0.1 : -0.3;
        const waveAmpBase = isThinking ? 11 : isSpeaking ? 8 + ctx.audio * 7 : 6;
        const waveFreqBase = isThinking ? 2.8 : 2.1;
        const waveSpeedBase = isThinking ? 5.5 : isSpeaking ? 4 : 2.2;

        model.tentacles.forEach((t) => {
            const waveAmp = waveAmpBase * t.ampMult;
            const waveFreq = waveFreqBase * t.freqMult;
            const waveSpeed = waveSpeedBase * t.speedMult;

            const segments = t.positions.length / 3 - 1;
            let tipX = 0, tipY = 0, tipZ = 0;
            for (let j = 0; j <= segments; j++) {
                const jt = j / segments;
                const reach = jt * t.length;
                const wavePhase = jt * waveFreq * Math.PI * 2 - ctx.time * waveSpeed + t.phase;
                const spread = Math.pow(jt, 1.4);
                const swingA = Math.sin(wavePhase) * waveAmp * spread;
                const swingB = Math.cos(wavePhase * 0.7) * waveAmp * 0.6 * spread;

                const px = model.tailPoint.x + t.dir.x * reach + t.perpA.x * swingA + t.perpB.x * swingB;
                const py = model.tailPoint.y + t.dir.y * reach + t.perpA.y * swingA + t.perpB.y * swingB + droop * reach * jt;
                const pz = model.tailPoint.z + t.dir.z * reach + t.perpA.z * swingA + t.perpB.z * swingB;

                t.positions[j * 3] = px;
                t.positions[j * 3 + 1] = py;
                t.positions[j * 3 + 2] = pz;
                if (j === segments) { tipX = px; tipY = py; tipZ = pz; }
            }
            t.geom.attributes.position.needsUpdate = true;
            t.tip.position.set(tipX, tipY, tipZ);
        });

        let beamOpacity = 0.5;
        if (isSpeaking) beamOpacity = 0.5 + ctx.audio * 0.4;
        else if (isThinking) beamOpacity = 0.5 + Math.abs(Math.sin(ctx.time * 6)) * 0.25;
        model.beamMaterial.opacity = beamOpacity;
        model.beamMaterial.size = 1.8 + (isSpeaking ? ctx.audio * 1.2 : 0);

        model.eyeGlowMat.opacity = isSpeaking
            ? 0.7 + ctx.audio * 0.5
            : 0.5 + Math.abs(Math.sin(ctx.time * 1.6)) * 0.25;

        let glowIntensity;
        if (isSpeaking) glowIntensity = 0.2 + ctx.audio * 0.3;
        else if (isThinking) glowIntensity = 0.2 + Math.sin(ctx.time * 8) * 0.1;
        else glowIntensity = 0.18 + ctx.click * 0.2;
        model.glowMaterial.opacity = glowIntensity;
    },

    applyPalette(model, palette) {
        model.beamMaterial.color.setHex(palette.hex);
        model.hullFillMat.color.setHex(palette.hex);
        model.hullInnerWireMat.color.setHex(palette.hex);
        model.hullOutlineMat.color.setHex(palette.hex3);
        model.hullPointsMat.color.setHex(palette.hex3);
        model.ribMat.color.setHex(palette.hex3);
        model.tentacleLineMat.color.setHex(palette.hex2);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
