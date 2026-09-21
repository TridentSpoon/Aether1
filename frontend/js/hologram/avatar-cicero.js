/* Avatar: C.I.C.E.R.O. -- Centralized Intelligence & Computational Execution Response
 * Operator. The eXcelsior Class's second: a small hovering chassis, which is the first
 * avatar here that is actually a built object rather than a field of light. A domed
 * enamel head over a glass visor, two chrome tape reels turning behind it where eyes
 * would be, a tapered torso with a banded chest, and a nozzle underneath holding it up
 * on a plasma jet.
 *
 * The reels are the tell. They are what it does rather than what it looks at: they spin
 * with the work, idling slowly, running hard while it is thinking, and answering the
 * voice while it speaks. Nothing else in the avatar line reads a state as literally.
 *
 * The one place this file departs from the house style is its materials. Everything else
 * is MeshBasicMaterial, which ignores light -- right for a hologram, wrong for a machine
 * with a moulded shell, which needs a lit side and a shaded one to read as solid at all.
 * The shell and the chrome are MeshPhongMaterial and take the scene's own two lights (see
 * core.js); everything that is meant to glow -- visor, hubs, band, jet -- stays basic and
 * additive, so the lit parts and the emissive parts never fight.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'cicero',
    label: 'C.I.C.E.R.O.',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        /* Fixed, not theme-tinted: the moulded shell and its chrome. This is the same
           convention the obsidian cores follow -- a real material stays its own colour
           through a theme change, and the theme reaches the avatar through what glows.
           An enamel shell that turned crimson with the HUD would stop reading as enamel. */
        const ENAMEL = 0xe8eef6;
        const CHROME = 0xd6deea;
        // Darker than the chrome, so the lit hub at the centre of each reel actually reads.
        const REEL_FACE = 0x8d9ab0;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.15, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(170, 170, 1);
        glow.position.z = -30;
        group.add(glow);

        const enamelMat = new THREE.MeshPhongMaterial({ color: ENAMEL, shininess: 70, specular: 0x8fa4c8 });
        const chromeMat = new THREE.MeshPhongMaterial({ color: CHROME, shininess: 140, specular: 0xffffff });
        const reelMat = new THREE.MeshPhongMaterial({ color: REEL_FACE, shininess: 110, specular: 0xdfe8f5 });

        // --- Head: a drum with a domed crown, hung above the torso. Its own group, so
        // the whole head can look around without taking the body with it. ---
        const headGroup = new THREE.Group();
        headGroup.position.y = 26;
        group.add(headGroup);

        const HEAD_TOP = 28;
        const HEAD_BOTTOM = 26;
        headGroup.add(new THREE.Mesh(new THREE.CylinderGeometry(HEAD_TOP, HEAD_BOTTOM, 20, 32), enamelMat));
        const crown = new THREE.Mesh(
            new THREE.SphereGeometry(HEAD_TOP, 32, 12, 0, Math.PI * 2, 0, Math.PI / 2),
            enamelMat
        );
        crown.position.y = 10;
        headGroup.add(crown);

        /* The visor: an arc of the head's own curve laid across the front as a dark
           glass band, the reels standing in front of it. Dark on purpose, and the one
           thing here that is not additive: additive over near-white enamel is white, so
           a glowing faceplate on a white head is a faceplate nobody can see. A smoked
           pane reads as glass because it is darker than what surrounds it, and it still
           carries the theme -- the tint is the theme colour brought most of the way down,
           and brightening it is what "lit up" looks like on something dark. */
        const visorMat = new THREE.MeshBasicMaterial({
            color: 0x0b1524, transparent: true, opacity: 0.9, side: THREE.DoubleSide,
        });
        const visor = new THREE.Mesh(
            new THREE.CylinderGeometry(HEAD_TOP * 1.01, HEAD_BOTTOM * 1.01, 17, 32, 1, true, -1.3, 2.6),
            visorMat
        );
        headGroup.add(visor);
        const visorBase = new THREE.Color(api.palette.hex).multiplyScalar(0.38);

        // --- The reels: two chrome discs standing proud of the visor, each with a lit
        // hub at its centre, turning opposite ways like a tape running between them. ---
        const reelGeom = new THREE.CylinderGeometry(7, 7, 2.2, 20);
        const hubMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        const hubGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(48), color: api.palette.hex,
            transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending, depthWrite: false,
        });

        const reels = [];
        [-1, 1].forEach((side) => {
            const reel = new THREE.Group();
            reel.position.set(side * 12, 0, 27);

            const disc = new THREE.Mesh(reelGeom, reelMat);
            disc.rotation.x = Math.PI / 2;
            reel.add(disc);

            // Three spokes, so a turning reel actually reads as turning -- a plain disc
            // spinning about its own axis is indistinguishable from a still one.
            const spokeMat = new THREE.MeshBasicMaterial({ color: 0x1b2330 });
            const spokeGeom = new THREE.BoxGeometry(1.4, 6, 0.6);
            for (let i = 0; i < 3; i++) {
                // Each spoke sits in its own arm, offset outward along the arm's own y and
                // the arm then turned: rotating the mesh in place and solving for where it
                // should sit is the same picture by a longer road.
                const arm = new THREE.Group();
                arm.rotation.z = (i / 3) * Math.PI * 2;
                const spoke = new THREE.Mesh(spokeGeom, spokeMat);
                spoke.position.set(0, 3, 1.3);
                arm.add(spoke);
                reel.add(arm);
            }

            const hub = new THREE.Mesh(new THREE.SphereGeometry(3, 16, 12), hubMat);
            hub.position.z = 2;
            reel.add(hub);

            const hubGlow = new THREE.Sprite(hubGlowMat);
            hubGlow.scale.set(15, 15, 1);
            hubGlow.position.z = 2.8;
            reel.add(hubGlow);

            headGroup.add(reel);
            reels.push({ reel, direction: side });
        });

        // --- Body: a tapered torso, a lit band across the chest, and the nozzle. ---
        const bodyGroup = new THREE.Group();
        bodyGroup.position.y = -12;
        group.add(bodyGroup);

        bodyGroup.add(new THREE.Mesh(new THREE.CylinderGeometry(24, 15, 36, 32), enamelMat));

        const bandMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
        });
        const band = new THREE.Mesh(new THREE.CylinderGeometry(24.4, 22.6, 10.5, 32, 1, true), bandMat);
        band.position.y = 6;
        bodyGroup.add(band);

        const nozzle = new THREE.Mesh(new THREE.CylinderGeometry(13.5, 7.5, 12, 20), chromeMat);
        nozzle.position.y = -22.5;
        bodyGroup.add(nozzle);

        // The jet. Point-down cone, drawn additively so it burns rather than sits there,
        // with a sprite bloom at the nozzle mouth doing the work a real light would.
        const jetMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
        });
        const jet = new THREE.Mesh(new THREE.ConeGeometry(7.2, 28, 20, 1, true), jetMat);
        jet.rotation.x = Math.PI;
        jet.position.y = -39;
        bodyGroup.add(jet);

        const jetGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(48), color: api.palette.hex3,
            transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const jetGlow = new THREE.Sprite(jetGlowMat);
        jetGlow.scale.set(54, 54, 1);
        jetGlow.position.y = -30;
        bodyGroup.add(jetGlow);

        return {
            group, headGroup, bodyGroup, visor, visorMat, visorBase, reels, hubMat, hubGlowMat,
            band, bandMat, jet, jetMat, jetGlow, jetGlowMat, glow, glowMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        // Hover: never perfectly still, and never on one clean sine either -- two rates
        // beating against each other is what keeps it from looking like it is on a rail.
        model.group.position.y = Math.sin(ctx.time * 2) * 4 + Math.sin(ctx.time * 0.7) * 2 + ctx.click * 10;

        // Looking around. Listening turns it toward whoever is talking and holds it
        // there, tilted, rather than carrying on its idle sweep.
        const sweep = isListening ? 0.34 : 0.2;
        model.headGroup.rotation.y = Math.sin(ctx.time * (isListening ? 0.5 : 1.2)) * sweep;
        model.headGroup.rotation.x = Math.cos(ctx.time * 1.5) * 0.05 + (isListening ? 0.12 : 0);

        // The reels run with the work: idle turnover, hard while thinking, and driven by
        // the voice while speaking. Accumulated rather than set from ctx.time, so a change
        // of speed never snaps the reels to a new angle mid-turn.
        const rate = isThinking ? 7 : isSpeaking ? 2 + ctx.audio * 9 : 1.1;
        model.reelSpin = (model.reelSpin || 0) + rate * 0.016 + ctx.click * 0.25;
        model.reels.forEach(({ reel, direction }) => {
            reel.rotation.z = model.reelSpin * direction;
        });

        // What glows, glows harder when there is something going on.
        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 5)) * 0.6 : 0;
        model.visorMat.color.copy(model.visorBase).multiplyScalar(0.85 + heat * 1.5 + ctx.click * 0.6);
        model.hubMat.opacity = 0.75 + heat * 0.25;
        model.hubGlowMat.opacity = 0.4 + heat * 0.5;
        model.bandMat.opacity = 0.7 + heat * 0.3;

        // The jet answers the hover, not the voice: it flares when the chassis is pushing
        // itself up, and a click is a burst.
        const thrust = 1 + Math.sin(ctx.time * 15) * 0.12 + heat * 0.35 + ctx.click * 0.8;
        model.jet.scale.set(1, thrust, 1);
        model.jetMat.opacity = 0.72 + Math.sin(ctx.time * 20) * 0.1 + heat * 0.25;
        model.jetGlow.scale.setScalar(54 * (1 + heat * 0.25 + ctx.click * 0.5));
        model.jetGlowMat.opacity = 0.55 + heat * 0.4;

        model.glowMat.opacity = 0.13 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.3) * 0.04 + 0.04);
    },

    applyPalette(model, palette) {
        model.visorBase.setHex(palette.hex).multiplyScalar(0.38);
        model.hubMat.color.setHex(palette.hex);
        model.hubGlowMat.color.setHex(palette.hex);
        model.bandMat.color.setHex(palette.hex2);
        model.jetMat.color.setHex(palette.hex3);
        model.jetGlowMat.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        // The enamel shell and the chrome keep their own colours -- see build().
    },
});
