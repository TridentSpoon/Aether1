/* Avatar: m.A.I.r.a.d. -- Mutative Autonomous Intrusion Response and Defense, the
 * eXcelsior Class's fifth. A faceted white shell with a lit lattice caged inside it, one
 * optic burning at the front, and two targeting rings sweeping it on crossed tilts.
 *
 * Mutative is the word the whole thing is built around. The shell and the lattice inside
 * it turn against each other and never settle into the same relationship twice; the rings
 * tighten and quicken with the work; the optic answers the voice. Nothing about it is a
 * face, and that is deliberate -- the rest of this line has eyes, and this one has a lens
 * watching the door.
 *
 * Three things about how it is put together.
 *
 * The shell is faceted and lit, not a hologram plane. Flat shading on a subdivided
 * icosahedron is what makes one colour read as machined plates, since every facet takes
 * the scene's light at its own angle -- PRAXIS's core uses the same trick. It is also
 * slightly transparent, which is the one place this departs from the sketch it came from:
 * an opaque shell hides the lattice completely, and the lattice is the part that shows it
 * is thinking.
 *
 * The optic and the inner ring keep a fixed hot colour through a theme change, the same
 * convention R.E.D. 9000's lens and enXephalon's signature points follow: what a thing
 * watches with is its own. The outer ring and the ticks are where the theme reaches it.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'mairad',
    label: 'm.A.I.r.a.d.',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const R = 44;                 // the shell; everything else is measured off it
        const SHELL = 0xeef3fa;       // fixed, not theme-tinted -- see the header
        const OPTIC = 0xff0e8c;       // the hot colour of the optic and the inner ring
        const CAGE = 0xc02cff;        // the lattice caged inside the shell

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(190, 190, 1);
        glow.position.z = -40;
        group.add(glow);

        /* The lattice, added before the shell that contains it so it is drawn first: a
           transparent shell sorts against what is already in the depth buffer, and a cage
           drawn afterwards can be discarded where the shell covers it. */
        const cageMat = new THREE.MeshBasicMaterial({
            color: CAGE, wireframe: true, transparent: true, opacity: 0.75,
            blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const cage = new THREE.Mesh(new THREE.IcosahedronGeometry(R * 0.86, 2), cageMat);
        group.add(cage);

        // A bloom inside the cage, so the shell is lit from within rather than being a
        // pale lump with a wire ball in it.
        const heartMat = new THREE.SpriteMaterial({
            map: api.helpers.radialGlowTexture(64, 'rgba(255,190,240,0.95)', 'rgba(190,0,190,0)'),
            transparent: true, opacity: 0.32, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const heart = new THREE.Sprite(heartMat);
        heart.scale.set(R * 1.1, R * 1.1, 1);
        group.add(heart);

        // The shell: machined plates, lit by the scene's own two lights (see core.js).
        const shellMat = new THREE.MeshPhongMaterial({
            color: SHELL, shininess: 150, specular: 0xffffff, flatShading: true,
            transparent: true, opacity: 0.82,
        });
        const shell = new THREE.Mesh(new THREE.IcosahedronGeometry(R, 2), shellMat);
        group.add(shell);

        /* The optic: one lens on the front of the shell, standing just clear of the
           facets so it never half-sinks into one, with its own bloom over it. This is the
           whole face -- there is no second eye and no mouth. */
        const opticGroup = new THREE.Group();
        // Proud of the shell, not flush with it. A subdivided icosahedron's faces sit
        // well inside its radius, so a lens at R lands behind the facets and the
        // transparent shell then draws over it and mutes it.
        opticGroup.position.z = R * 1.07;
        group.add(opticGroup);

        /* A dark socket under the lens. The shell is near-white and the optic is
           additive, and additive light over a pale surface is white -- the same trap
           C.I.C.E.R.O.'s visor fell into. Sinking the lens into something dark first is
           what lets a hot colour read as hot. */
        const socketMat = new THREE.MeshBasicMaterial({ color: 0x0a0410, transparent: true, opacity: 0.92 });
        const socket = new THREE.Mesh(new THREE.CircleGeometry(R * 0.29, 32), socketMat);
        socket.position.z = -0.4;
        opticGroup.add(socket);

        const irisMat = new THREE.MeshBasicMaterial({
            color: OPTIC, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
        });
        opticGroup.add(new THREE.Mesh(new THREE.CircleGeometry(R * 0.2, 32), irisMat));

        // A white pinpoint inside the hot iris: the thing that reads as aimed at you.
        const pupilMat = new THREE.MeshBasicMaterial({
            color: 0xffffff, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        const pupil = new THREE.Mesh(new THREE.CircleGeometry(R * 0.075, 24), pupilMat);
        pupil.position.z = 0.6;
        opticGroup.add(pupil);

        const opticGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: OPTIC,
            transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const opticGlow = new THREE.Sprite(opticGlowMat);
        opticGlow.scale.set(R * 1.3, R * 1.3, 1);
        opticGlow.position.z = 1.2;
        opticGroup.add(opticGlow);

        /* The targeting rings, each fixed on its own tilt and turning within it. The inner
           one keeps the optic's hot colour; the outer one is the theme's, so a change of
           HUD colour reaches the avatar without touching what it watches with. */
        const ringInnerMat = new THREE.MeshBasicMaterial({
            color: OPTIC, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
        });
        const ringInner = new THREE.Mesh(new THREE.TorusGeometry(R * 1.28, 0.55, 8, 96), ringInnerMat);
        // Not quite edge-on: at exactly PI / 2 the HUD's level camera sees a ring as a
        // straight line drawn through the avatar rather than as a ring around it.
        ringInner.rotation.x = Math.PI / 2.55;
        group.add(ringInner);

        const ringOuterMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending,
        });
        const ringOuter = new THREE.Mesh(new THREE.TorusGeometry(R * 1.55, 0.4, 8, 96), ringOuterMat);
        ringOuter.rotation.x = -Math.PI / 2.9;
        group.add(ringOuter);

        /* Four ticks in the picture plane, standing off the front. They are what turns two
           rings into a sight: a ring on its own is decoration, a ring with marks on it is
           an instrument reading something. */
        const tickMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
        });
        const ticks = new THREE.Group();
        ticks.position.z = R * 1.15;
        for (let i = 0; i < 4; i++) {
            ticks.add(new THREE.Mesh(
                new THREE.RingGeometry(R * 0.78, R * 1.02, 8, 1, i * Math.PI / 2 - 0.06, 0.12),
                tickMat
            ));
        }
        group.add(ticks);

        return {
            group, shell, shellMat, cage, cageMat, heart, heartMat, opticGroup,
            irisMat, pupil, pupilMat, opticGlow, opticGlowMat,
            ringInner, ringInnerMat, ringOuter, ringOuterMat, ticks, tickMat, glow, glowMat,
            radius: R,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        model.group.position.y = Math.sin(ctx.time * 0.7) * 3;

        /* Shell and cage turn against each other, and the cage turns faster while it is
           working something out -- which is the only outward sign of it, since the shell
           itself has no expression to give. */
        const churn = isThinking ? 2.6 : isListening ? 0.5 : 1;
        model.shell.rotation.y = ctx.time * 0.15;
        model.shell.rotation.x = Math.sin(ctx.time * 0.2) * 0.12;
        model.cage.rotation.y = -ctx.time * 0.25 * churn;
        model.cage.rotation.x = ctx.time * 0.1 * churn;

        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.6 : 0;
        model.cageMat.opacity = 0.55 + heat * 0.4 + ctx.click * 0.2;
        model.heartMat.opacity = 0.25 + heat * 0.35;
        model.shellMat.opacity = 0.86 - heat * 0.14;        // the plates thin as it lights up
        model.shell.scale.setScalar(1 + heat * 0.05 + ctx.click * 0.06);

        /* The optic. A slow pulse at rest so it is never dead, the voice while it speaks,
           and a hard blink-rate while it thinks. The pupil carries the pulse -- an iris
           that changed size would push into the facets around it. */
        const pulse = 0.5 + Math.sin(ctx.time * (isThinking ? 12 : 4)) * 0.5;
        model.irisMat.opacity = 0.7 + heat * 0.3;
        model.pupil.scale.setScalar(0.85 + pulse * 0.3 + heat * 0.35 + ctx.click * 0.3);
        model.pupilMat.opacity = 0.75 + pulse * 0.25;
        model.opticGlowMat.opacity = 0.45 + heat * 0.45 + ctx.click * 0.25;
        model.opticGlow.scale.setScalar(model.radius * (1.3 + heat * 0.4));

        /* The rings sweep, and listening draws them in: a sight closing on whoever is
           talking. They breathe on a fast beat too, which keeps them reading as a live
           instrument rather than a pair of hoops. */
        const speed = isThinking ? 1.8 : isListening ? 0.7 : 1;
        model.ringInner.rotation.z = -ctx.time * 0.6 * speed;
        model.ringOuter.rotation.y = ctx.time * 0.3 * speed;
        const tighten = (isListening ? 0.86 : 1) + Math.sin(ctx.time * 6) * 0.02 - ctx.click * 0.06;
        model.ringInner.scale.setScalar(tighten);
        model.ringOuter.scale.setScalar(tighten);
        model.ringInnerMat.opacity = 0.7 + heat * 0.3;
        model.ringOuterMat.opacity = 0.45 + heat * 0.3 + (isListening ? 0.2 : 0);

        model.ticks.rotation.z = ctx.time * 0.25 * speed;
        model.ticks.scale.setScalar(tighten);
        model.tickMat.opacity = 0.6 + heat * 0.3 + (isListening ? 0.2 : 0);

        model.glowMat.opacity = 0.14 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.04 + 0.04);
    },

    applyPalette(model, palette) {
        model.ringOuterMat.color.setHex(palette.hex);
        model.tickMat.color.setHex(palette.hex);
        model.glowMat.color.setHex(palette.hex);
        // The shell, the caged lattice and the hot optic keep their own colours -- see
        // build().
    },
});
