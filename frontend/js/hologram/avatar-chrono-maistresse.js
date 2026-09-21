/* Avatar: Chrono-mAIstresse -- The eXcelsior Class's fourth. A clock with a face, in both
 * senses: a lit dial with real hands on it, two dark lens eyes above the centre and a
 * smile below, the whole thing bobbing and tilting as it talks to you.
 *
 * Two decisions make this one what it is.
 *
 * The hands tell the actual time. A clock avatar whose hands spin decoratively is a
 * cartoon of a clock; one that reads the machine's own clock is a clock, and an operator
 * glancing at the HUD gets something out of it. They only leave the real time when she
 * is thinking, which is when they race -- and they are back on the hour the moment the
 * thought lands.
 *
 * The face is geometry, not a drawn picture. The obvious way to build this is a canvas
 * redrawn every frame and uploaded as a texture, which costs a full repaint and a texture
 * upload sixty times a second to move two eyelids. Eyes that are meshes blink by being
 * scaled, cost nothing, stay sharp at any zoom, and -- the part that actually matters
 * here -- can be retinted when the colour theme changes, which a drawn-in picture cannot.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'chrono-maistresse',
    label: 'Chrono-mAIstresse',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const DIAL = 54;    // dial radius; everything else is measured off this, and it
                            // is what sets her scale against the rest of the line -- the
                            // halo it puts at DIAL * 1.32 is the outer edge of the avatar.

        // Fixed, not theme-tinted: the eyes and the mouth are the dark features read
        // against a lit dial, the same way every other avatar keeps its obsidian. If they
        // took the theme they would vanish into the face behind them.
        const INK = 0x0a0610;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.18, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(180, 180, 1);
        glow.position.z = -20;
        group.add(glow);

        // Everything that is "her" sits in one group, so the bob and the tilt move the
        // whole face together and the bloom behind stays put.
        const face = new THREE.Group();
        group.add(face);

        // --- The dial: a lit disc with a bright rim and twelve ticks. ---
        const dialMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.3, blending: THREE.AdditiveBlending,
        });
        const dial = new THREE.Mesh(new THREE.CircleGeometry(DIAL, 64), dialMat);
        face.add(dial);

        const rimMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.95 });
        const rimPoints = [];
        for (let i = 0; i <= 64; i++) {
            const a = (i / 64) * Math.PI * 2;
            rimPoints.push(new THREE.Vector3(Math.cos(a) * DIAL, Math.sin(a) * DIAL, 0));
        }
        const rim = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(rimPoints), rimMat);
        rim.position.z = 0.5;
        face.add(rim);

        const tickMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.8 });
        const tickPoints = [];
        for (let i = 0; i < 12; i++) {
            // Straight up is twelve o'clock, and the hour marks run clockwise from there --
            // the same frame the hands below use, so the two cannot disagree.
            const a = Math.PI / 2 - (i / 12) * Math.PI * 2;
            const long = i % 3 === 0;
            const inner = DIAL * (long ? 0.82 : 0.88);
            tickPoints.push(new THREE.Vector3(Math.cos(a) * inner, Math.sin(a) * inner, 0));
            tickPoints.push(new THREE.Vector3(Math.cos(a) * DIAL * 0.95, Math.sin(a) * DIAL * 0.95, 0));
        }
        const ticks = new THREE.LineSegments(new THREE.BufferGeometry().setFromPoints(tickPoints), tickMat);
        ticks.position.z = 0.5;
        face.add(ticks);

        // --- The features. Eyes above the middle, mouth below it, all in fixed ink. ---
        const inkMat = new THREE.MeshBasicMaterial({ color: INK, transparent: true, opacity: 0.94 });
        const catchMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.75 });

        const eyes = [-1, 1].map((side) => {
            const eye = new THREE.Group();
            eye.position.set(side * DIAL * 0.31, DIAL * 0.26, 2);
            const ball = new THREE.Mesh(new THREE.CircleGeometry(DIAL * 0.14, 24), inkMat);
            eye.add(ball);
            const catchlight = new THREE.Mesh(new THREE.CircleGeometry(DIAL * 0.045, 16), catchMat);
            catchlight.position.set(-DIAL * 0.05, DIAL * 0.05, 0.5);
            eye.add(catchlight);
            face.add(eye);
            return { eye, ball, catchlight };
        });

        /* The mouth is two pieces that do different jobs: a fixed arc that is the smile
           line, and a filled shape behind it that grows when she speaks. Widening an arc
           would mean rebuilding its geometry every frame; growing a shape behind a fixed
           arc is a scale, and it reads the same. */
        const mouth = new THREE.Group();
        mouth.position.set(0, -DIAL * 0.22, 2);
        face.add(mouth);

        const openMat = new THREE.MeshBasicMaterial({ color: INK, transparent: true, opacity: 0.92 });
        const open = new THREE.Mesh(new THREE.CircleGeometry(DIAL * 0.22, 32), openMat);
        open.position.z = -0.4;
        open.scale.set(1, 0.06, 1);
        mouth.add(open);

        // A ring arc across the bottom half: theta runs counter-clockwise from +x, so the
        // lower semicircle, inset at both ends, is a smile.
        const smile = new THREE.Mesh(
            new THREE.RingGeometry(DIAL * 0.28, DIAL * 0.34, 40, 1, Math.PI + 0.35, Math.PI - 0.7),
            inkMat
        );
        mouth.add(smile);

        // --- The hands, on their own pivots at the centre of the dial. ---
        const handMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        function hand(length, width) {
            const pivot = new THREE.Group();
            const bar = new THREE.Mesh(new THREE.BoxGeometry(width, length, 1), handMat);
            // Drawn from its own centre, so half its length is what puts its tail on the
            // pivot and the rest of it out toward the rim.
            bar.position.y = length / 2;
            pivot.add(bar);
            pivot.position.z = 3;
            face.add(pivot);
            return pivot;
        }
        const hourHand = hand(DIAL * 0.5, 3.4);
        const minuteHand = hand(DIAL * 0.72, 2.4);

        const capMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 1, blending: THREE.AdditiveBlending,
        });
        const cap = new THREE.Mesh(new THREE.CircleGeometry(3.2, 16), capMat);
        cap.position.z = 4;
        face.add(cap);

        // --- The projection halo, standing off the dial. ---
        const haloMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.4,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const halo = new THREE.Mesh(new THREE.RingGeometry(DIAL * 1.2, DIAL * 1.32, 64), haloMat);
        halo.position.z = -2;
        group.add(halo);

        return {
            group, face, glow, glowMat, dial, dialMat, rimMat, tickMat, eyes, mouth,
            open, openMat, smile, inkMat, catchMat, hourHand, minuteHand, handMat,
            cap, capMat, halo, haloMat,
            dialRadius: DIAL,
            blinkUntil: 0,
            nextBlink: 2,
            raceAngle: 0,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        /* A dial is a flat thing, and a flat thing turned edge-on is a line. Cancelling
           the scene's drag-yaw keeps her facing whoever is looking, the same trick the
           Operator's tunnel uses -- see core.js's pointermove handler for where
           ctx.viewRotationY comes from. */
        model.group.rotation.y = -ctx.viewRotationY;

        // Bob and sway. She leans into listening and holds there.
        model.face.position.y = Math.sin(ctx.time * 2.5) * 4;
        model.face.rotation.z = Math.sin(ctx.time * 1.5) * 0.06 + (isListening ? 0.12 : 0);

        /* The hands read the machine's own clock. Smooth rather than ticking: the minute
           hand carries the seconds and the hour hand carries the minutes, so neither ever
           sits at a time the other contradicts. Thinking sends them racing away from it,
           and they are back on the real hour the moment it stops. */
        const now = new Date();
        const seconds = now.getSeconds() + now.getMilliseconds() / 1000;
        const minutes = now.getMinutes() + seconds / 60;
        const hours = (now.getHours() % 12) + minutes / 60;
        model.raceAngle = isThinking ? model.raceAngle + 0.14 : 0;
        // Clock hands run the opposite way to the maths convention, hence the negation.
        model.minuteHand.rotation.z = -(minutes / 60) * Math.PI * 2 - model.raceAngle * 4;
        model.hourHand.rotation.z = -(hours / 12) * Math.PI * 2 - model.raceAngle;

        /* Blinking on its own irregular schedule rather than off a sine: a sine blinks in
           a rhythm you start to predict, and two eyes that blink predictably stop reading
           as alive. Each blink books the next one somewhere between two and six seconds
           out. A click spends one immediately. */
        if (ctx.click > 0.6 && ctx.time > model.blinkUntil) model.blinkUntil = ctx.time + 0.12;
        if (ctx.time > model.nextBlink) {
            model.blinkUntil = ctx.time + 0.12;
            model.nextBlink = ctx.time + 2 + Math.random() * 4;
        }
        const blinking = ctx.time < model.blinkUntil;
        const lidOpen = blinking ? 0.08 : 1;
        const widen = isListening ? 1.15 : isSpeaking ? 1 + ctx.audio * 0.15 : 1;
        model.eyes.forEach(({ eye, catchlight }, i) => {
            eye.scale.set(widen, lidOpen * widen, 1);
            catchlight.visible = !blinking;
            // Thinking looks away and back rather than staring through you.
            eye.position.x = (i === 0 ? -1 : 1) * model.dialRadius * 0.31
                + (isThinking ? Math.sin(ctx.time * 1.6) * model.dialRadius * 0.04 : 0);
        });

        // The mouth opens with the voice and settles back to the smile line otherwise.
        const openness = isSpeaking ? 0.06 + ctx.audio * 1.1 : 0.06 + ctx.click * 0.5;
        model.open.scale.set(1, Math.min(1.2, openness), 1);

        // Dial brightness, and the CRT flutter that keeps it a projection rather than a
        // painted disc.
        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.5 : 0;
        const flutter = Math.sin(ctx.time * 30) * 0.03;
        model.dialMat.opacity = 0.26 + heat * 0.24 + flutter;
        model.rimMat.opacity = 0.85 + heat * 0.15 + flutter;
        model.haloMat.opacity = 0.3 + heat * 0.4 + ctx.click * 0.25;
        model.halo.rotation.z = ctx.time * 0.25;
        model.handMat.opacity = 0.8 + heat * 0.2;
        model.glowMat.opacity = 0.15 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.3) * 0.04 + 0.04);
    },

    applyPalette(model, palette) {
        model.dialMat.color.setHex(palette.hex);
        model.rimMat.color.setHex(palette.hex3);
        model.tickMat.color.setHex(palette.hex3);
        model.handMat.color.setHex(palette.hex2);
        model.capMat.color.setHex(palette.hex3);
        model.haloMat.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        // The eyes, the smile and the catchlights keep their own colours -- see build().
    },
});
