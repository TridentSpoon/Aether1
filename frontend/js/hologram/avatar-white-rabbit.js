/* Avatar: White Rabbit -- Nexus Sent and the Nexus, merged into one avatar.
 *
 * The bottom half is Nexus Sent's: the square-tube brass emitter ring and the tapering
 * column of light rising out of it (see avatar-senti.js). What floats at the top of
 * that beam is no longer Nexus Sent's small probe core -- it's the Nexus's own elongated,
 * bulbous hull (see avatar-nexus.js's buildNexusAvatar), scaled down to sit there
 * instead, wireframe/point-cloud rendered the same way, with the same forward
 * eye-lens cluster on its nose. Nexus Sent's small obsidian core is nested inside the
 * hull's belly, glimpsed through the sparse wireframe rather than out in the open --
 * the one physical piece of hardware the two avatars share. Trailing from the hull's
 * tail is a burst of tentacles, using Nexus Sent's fluid traveling-wave joints (root
 * anchored, amplitude growing toward the tip) rather than the Nexus's own rebuilt
 * chain-link segments, tipped in the Nexus's fixed red instead of Nexus Sent's icy blue.
 *
 * Left out of the merge: the Nexus's letter rain and radar-grid backdrop -- CRT/hacker
 * dressing that belonged to the Nexus specifically and doesn't carry a meaning here.
 *
 * The ring and the hull are both fixed/lit (Phong) like every avatar's obsidian core;
 * the beam, hull wireframe and tentacle strands are drawn light and follow the color
 * theme. The nested core, the eye cluster and the tentacle joints/tips stay a fixed
 * red regardless of theme -- the Nexus's own hot-accent color, carried over as the one
 * fixed color this merged avatar answers to.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'white-rabbit',
    label: 'White Rabbit',

    build(api) {
        const group = new THREE.Group();
        const HOT = 0xff2418; // the Nexus's own eye/claw red, carried over as the fixed accent here

        // --- Emitter ring + beam: identical technique to Nexus Sent's (see avatar-senti.js for
        // the square-tube-around-a-circle explanation). ---
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

        // --- The body: everything below is built at the Nexus's own native scale (a hull
        // roughly 46 units nose-to-tail), then this one group scales the whole assembly
        // down to sit proportionately atop Nexus Sent's beam, the same way Nexus Sent's own core
        // used to. bodyBaseY is animate()'s bob baseline, same role coreBaseY played there. ---
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

        // Ribbed carapace bands, theme-tinted (the Nexus hardcoded these to its own green;
        // here they follow applyPalette like the rest of the body).
        const ribMat = new THREE.LineBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending
        });
        [{ z: -10, r: 11.7 }, { z: 0, r: 13.2 }, { z: 9, r: 12.2 }].forEach(({ z, r }) => {
            const ribGeom = new THREE.TorusGeometry(r, 0.3, 6, 32);
            const rib = new THREE.LineSegments(new THREE.EdgesGeometry(ribGeom, 20), ribMat);
            rib.position.z = z;
            hullMesh.add(rib);
        });

        // Nexus Sent's core, nested inside the hull's belly rather than out in the open --
        // glimpsed through the sparse wireframe instead of being its own free-floating
        // probe. Still the one fixed-dark, Phong-lit "obsidian core" every avatar here
        // keeps at its center.
        const nestedCoreMat = new THREE.MeshPhongMaterial({
            color: 0x0a0a0f, specular: HOT, shininess: 90, transparent: true, opacity: 0.97
        });
        const nestedCore = new THREE.Mesh(new THREE.IcosahedronGeometry(6, 1), nestedCoreMat);
        nestedCore.position.z = -2;
        hullMesh.add(nestedCore);

        // Forward eye-lens cluster, the Nexus's own layout, fixed red regardless of theme.
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

        // --- Tentacles: Nexus Sent's fluid traveling-wave joints (see avatar-senti.js), trailing
        // from the hull's tail instead of bursting from a small core -- mostly backward
        // (-Z, toward the hull's narrow tail end) with a moderate fan, rather than Nexus
        // Sent's full spherical burst. ---
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
        const tentacleCount = 8;
        const jointCount = 9;
        const tentacles = [];
        for (let i = 0; i < tentacleCount; i++) {
            const azimuth = (i / tentacleCount) * Math.PI * 2 + (Math.random() - 0.5) * 0.4;
            const fan = 0.5; // how wide the tentacles splay from straight-back
            const dir = new THREE.Vector3(
                Math.cos(azimuth) * fan,
                Math.sin(azimuth) * fan * 0.6,
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
            tip.scale.set(4.5, 4.5, 1);
            tiltGroup.add(tip);

            tentacles.push({
                positions, geom, tip, dir, perpA, perpB,
                length: 28 + Math.random() * 12,
                phase: Math.random() * Math.PI * 2,
                speedMult: 0.85 + Math.random() * 0.3
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

        // The whole body drifts in a slow turn at rest, sharpening into a faster spin
        // while thinking -- Nexus Sent's own rotation/bob formulas, now driving the hull
        // instead of the small probe core.
        model.group.rotation.y = ctx.time * (isThinking ? 1.1 : 0.25);

        const bob = Math.sin(ctx.time * (isThinking ? 3.2 : 1.1)) * (isThinking ? 3 : 5);
        const audioBob = isSpeaking ? ctx.audio * 8 : 0;
        model.bodyGroup.position.y = model.bodyBaseY + bob + audioBob + ctx.click * 10;

        // Tentacles ripple with the same traveling wave as Nexus Sent's, offset from the hull's
        // tail point instead of a central core.
        const droop = (isThinking || isSpeaking) ? -0.12 : -0.45;
        const waveAmp = isThinking ? 9 : isSpeaking ? 7 + ctx.audio * 6 : 4;
        const waveFreq = isThinking ? 2.6 : 1.8;
        const waveSpeed = isThinking ? 5 : isSpeaking ? 3.5 : 1.6;

        model.tentacles.forEach((t) => {
            const segments = t.positions.length / 3 - 1;
            let tipX = 0, tipY = 0, tipZ = 0;
            for (let j = 0; j <= segments; j++) {
                const jt = j / segments;
                const reach = jt * t.length;
                const wavePhase = jt * waveFreq * Math.PI * 2 - ctx.time * waveSpeed * t.speedMult + t.phase;
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
