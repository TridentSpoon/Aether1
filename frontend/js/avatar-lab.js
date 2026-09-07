/* The avatar workbench.
 *
 * Everything the HUD normally supplies to the avatar engine -- a state, a voice, a
 * colour theme -- faked here by hand, so avatar work needs no backend, no model server
 * and no network. See avatar-lab.html.
 */
(function () {
    'use strict';

    const BUILT_IN = [
        { id: 'halcy', label: 'hAlcy' },
        { id: 'arx-limes', label: 'A.R.X.LIMES' },
        { id: 'nexus', label: 'The Nexus' },
        { id: 'red', label: 'R.E.D. 9000' },
        { id: 'arx-logos', label: 'A.R.X.LOGOS' },
        { id: 'alt', label: 'A1ter_nul' },
    ];

    const $ = (id) => document.getElementById(id);
    const status = $('lab-status');

    let engine = null;
    let currentAvatar = 'custom';
    /* Empty means "whatever the HUD is wearing", which is where the workbench starts: you came
       here from the HUD, and an avatar previewed in a theme you do not use tells you less than
       one previewed in the theme you will actually see it in. The picker below then offers the
       presets, because checking a design against a colour you might switch to is the other
       thing this window is for. */
    let currentTheme = '';
    let currentState = 'IDLE';

    // ---- The engine ---------------------------------------------------------

    /* Rebuilt from scratch whenever the recipe changes, because build() runs once per
       engine -- that is the right contract for the HUD, which builds one and keeps it,
       and it makes rebuilding here the honest way to preview a change. Disposing the
       old one first is what stops a render loop piling up per slider nudge. */
    function rebuild() {
        if (engine) engine.dispose();
        engine = new HologramAvatar('hologram-viewport');
        engine.setAvatar(currentAvatar);
        engine.setColorPalette(Aether1Theme.paletteFor(activeColours()));
        engine.setState(currentState);
        say(`showing ${currentAvatar}`);
    }

    function say(text) {
        status.textContent = text;
    }

    function fillAvatarPicker() {
        const picker = $('lab-avatar');
        const registered = HologramAvatar.registeredAvatars();
        picker.innerHTML = '';
        BUILT_IN.concat(registered).forEach(({ id, label }) => {
            const option = document.createElement('option');
            option.value = id;
            option.textContent = registered.some((r) => r.id === id) ? `${label}  (yours)` : label;
            picker.appendChild(option);
        });
        picker.value = currentAvatar;
    }

    /* Which mode and which three colours are on screen: the HUD's own when nothing is being
       previewed, otherwise the chosen preset. Presets carry a mode as well as colours, so
       previewing Daylight puts the workbench in the flat shell too -- the same thing picking
       it in the HUD would do. */
    function activeTheme() {
        const preset = Aether1Theme.preset(currentTheme);
        if (preset) return { mode: preset.mode, colours: preset };
        return Aether1Theme.current();
    }

    function activeColours() {
        return activeTheme().colours;
    }

    /* The presets rather than every key in THEME_PALETTES: that object also carries the
       aliases kept for old saved settings, and offering "crimson" and "red" as two entries
       that do the same thing is just confusing in a picker. */
    function fillThemePicker() {
        const picker = $('lab-theme');
        picker.innerHTML = '';
        const asIs = document.createElement('option');
        asIs.value = '';
        asIs.textContent = 'Your theme (as set in the HUD)';
        picker.appendChild(asIs);
        Aether1Theme.presets().forEach((preset) => {
            const option = document.createElement('option');
            option.value = preset.id;
            option.textContent = `${preset.label} (${Aether1Theme.MODE_LABELS[preset.mode]})`;
            picker.appendChild(option);
        });
        picker.value = currentTheme;
        paintLabTheme();
    }

    /* Painted the same way the HUD paints itself. A preset id in data-theme would name no mode
       and leave the page with no colours at all, since the modes are what the stylesheet keys
       on now. */
    function paintLabTheme() {
        const theme = activeTheme();
        Aether1Theme.paint(document, theme.mode, theme.colours);
    }

    $('lab-avatar').addEventListener('change', (e) => {
        currentAvatar = e.target.value;
        engine.setAvatar(currentAvatar);
        say(`showing ${currentAvatar}`);
    });

    /* The HUD and this window are separate documents on one origin, so changing the theme over
       there fires a storage event here. Only worth acting on while nothing is being previewed:
       a preview is a deliberate override, and yanking it out from under someone mid-check would
       be worse than being briefly out of date. */
    window.addEventListener('storage', (e) => {
        if (e.key !== Aether1Theme.STORAGE_KEY || currentTheme) return;
        paintLabTheme();
        if (engine) engine.setColorPalette(Aether1Theme.paletteFor(activeColours()));
    });

    $('lab-theme').addEventListener('change', (e) => {
        currentTheme = e.target.value;
        engine.setColorPalette(Aether1Theme.paletteFor(activeColours()));
        paintLabTheme();
    });

    document.querySelectorAll('.state-btn').forEach((button) => {
        button.addEventListener('click', () => {
            currentState = button.dataset.state;
            engine.setState(currentState);
            document.querySelectorAll('.state-btn').forEach((b) => {
                b.classList.toggle('is-on', b === button);
            });
        });
    });

    // ---- The voice simulator ------------------------------------------------
    //
    // The real HUD hands the avatar 64 frequency bins from the Web Audio API, quiet to
    // loud, low to high. These shapes are made up, but they move the way the real thing
    // moves -- which is all an equaliser needs to be built against.

    const BINS = 64;
    const audioData = new Uint8Array(BINS);
    const spectrum = $('lab-spectrum');
    const spectrumCtx = spectrum.getContext('2d');

    function fillAudio(now) {
        const source = $('lab-audio-source').value;
        const level = Number($('lab-level').value) / 100;
        const t = now / 1000;

        for (let i = 0; i < BINS; i++) {
            const fraction = i / BINS;
            let value = 0;
            switch (source) {
                case 'silence':
                    value = 0;
                    break;
                case 'speech':
                    /* Voice is mostly low, with syllables arriving in bursts -- the
                       envelope matters more than the spectrum for anything reacting.
                       The floor on the envelope is deliberate: a simulator that keeps
                       falling to silence makes an avatar look broken rather than
                       quiet, and you cannot judge a shape you only glimpse. */
                    value = Math.exp(-fraction * 3.2)
                        * (0.55 + 0.45 * Math.sin(t * 7 + fraction * 4))
                        * (0.55 + 0.45 * Math.abs(Math.sin(t * 2.3)));
                    break;
                case 'music':
                    // A bass line, a mid body, and a bit of sparkle up top.
                    value = Math.exp(-fraction * 6) * (0.6 + 0.4 * Math.sin(t * 3))
                        + 0.35 * Math.exp(-Math.pow((fraction - 0.35) * 4, 2)) * (0.5 + 0.5 * Math.sin(t * 5))
                        + 0.18 * fraction * Math.abs(Math.sin(t * 11 + fraction * 20));
                    break;
                case 'sweep': {
                    // One moving peak: the clearest way to see which bin drives what.
                    const peak = (t * 0.15) % 1;
                    value = Math.exp(-Math.pow((fraction - peak) * 12, 2));
                    break;
                }
                case 'manual':
                default:
                    value = 1;
                    break;
            }
            audioData[i] = Math.max(0, Math.min(255, Math.round(value * level * 255)));
        }
    }

    function drawSpectrum() {
        const w = spectrum.width;
        const h = spectrum.height;
        spectrumCtx.clearRect(0, 0, w, h);
        const barWidth = w / BINS;
        spectrumCtx.fillStyle = getComputedStyle(document.documentElement)
            .getPropertyValue('--neon-cyan').trim() || '#00f0ff';
        for (let i = 0; i < BINS; i++) {
            const barHeight = (audioData[i] / 255) * h;
            spectrumCtx.fillRect(i * barWidth, h - barHeight, Math.max(1, barWidth - 1), barHeight);
        }
    }

    function pump(now) {
        fillAudio(now);
        if (engine) engine.updateAudioData(audioData);
        drawSpectrum();
        requestAnimationFrame(pump);
    }

    $('lab-level').addEventListener('input', (e) => {
        $('lab-level-readout').textContent = `${e.target.value}%`;
    });

    // ---- The builder --------------------------------------------------------

    const RECIPE_FIELDS = ['core', 'body', 'equaliser', 'size', 'radius', 'spin', 'bob'];

    function fillPartPickers() {
        const map = {
            'build-core': window.AvatarParts.options.cores(),
            'build-body': window.AvatarParts.options.bodies(),
            'build-equaliser': window.AvatarParts.options.equalisers(),
        };
        Object.keys(map).forEach((id) => {
            const picker = $(id);
            picker.innerHTML = '';
            map[id].forEach(({ id: value, label }) => {
                const option = document.createElement('option');
                option.value = value;
                option.textContent = label;
                picker.appendChild(option);
            });
        });
    }

    function readRecipeFromControls() {
        return {
            core: $('build-core').value,
            body: $('build-body').value,
            equaliser: $('build-equaliser').value,
            size: Number($('build-size').value),
            radius: Number($('build-radius').value),
            // Spin is a slider of whole numbers because a slider of 0.01 steps is
            // unusable; the recipe wants the small number.
            spin: Number($('build-spin').value) / 100,
            bob: Number($('build-bob').value),
        };
    }

    function writeRecipeToControls(recipe) {
        $('build-core').value = recipe.core;
        $('build-body').value = recipe.body;
        $('build-equaliser').value = recipe.equaliser;
        $('build-size').value = recipe.size;
        $('build-radius').value = recipe.radius;
        $('build-spin').value = Math.round(recipe.spin * 100);
        $('build-bob').value = recipe.bob;
        updateReadouts();
    }

    function updateReadouts() {
        $('build-size-readout').textContent = $('build-size').value;
        $('build-radius-readout').textContent = $('build-radius').value;
        $('build-spin-readout').textContent = (Number($('build-spin').value) / 100).toFixed(2);
        $('build-bob-readout').textContent = $('build-bob').value;
    }

    /* Previewing sets the recipe the custom avatar reads at build time, without saving
       it -- you should be able to try something on before committing to it. */
    function preview() {
        const recipe = window.CustomAvatarRecipe.normalise(readRecipeFromControls());
        window.AETHER_CUSTOM_AVATAR_RECIPE = recipe;
        $('share-recipe').value = JSON.stringify(recipe, null, 2);
        $('build-saved-note').classList.add('hidden');
        currentAvatar = 'custom';
        $('lab-avatar').value = 'custom';
        rebuild();
    }

    RECIPE_FIELDS.forEach((field) => {
        const control = $(`build-${field}`);
        if (!control) return;
        control.addEventListener('change', preview);
        control.addEventListener('input', updateReadouts);
    });

    $('build-save').addEventListener('click', () => {
        window.CustomAvatarRecipe.save(readRecipeFromControls());
        $('build-saved-note').classList.remove('hidden');
        say('saved');
    });

    $('build-reset').addEventListener('click', () => {
        window.CustomAvatarRecipe.clear();
        writeRecipeToControls(window.CustomAvatarRecipe.defaults);
        preview();
        say('back to the default recipe');
    });

    // ---- Sharing ------------------------------------------------------------

    $('share-apply').addEventListener('click', () => {
        let parsed;
        try {
            parsed = JSON.parse($('share-recipe').value);
        } catch (err) {
            say('that is not a recipe -- check the punctuation');
            return;
        }
        writeRecipeToControls(window.CustomAvatarRecipe.normalise(parsed));
        preview();
        say('recipe applied');
    });

    /* An exported avatar is a standalone file with the recipe baked in, so whoever
       receives it does not need this page or a saved setting. It is also the shortest
       real example of the plug-in interface, which makes it a decent starting point
       for someone who then wants to hand-edit it. */
    function exportAsFile() {
        const recipe = window.CustomAvatarRecipe.normalise(readRecipeFromControls());
        const id = `custom-${recipe.core}-${recipe.body}`;
        const source = `/* An Aether1 avatar, exported from the avatar workbench.
 *
 * Install: put this file in frontend/js/hologram/ and add one line to
 * frontend/index.html next to the other avatar scripts, after core.js and parts.js
 * and before app.js:
 *
 *     <script src="js/hologram/${id}.js?v=1"><\/script>
 *
 * Then add a picker entry using data-avatar-val="${id}".
 */
HologramAvatar.registerAvatar({
    id: '${id}',
    label: 'Exported avatar',

    build(api) {
        const recipe = ${JSON.stringify(recipe, null, 8).replace(/\n/g, '\n        ')};
        const group = new THREE.Group();
        const options = { size: recipe.size, radius: recipe.radius };
        const parts = [
            AvatarParts.cores[recipe.core].build(api, options),
            AvatarParts.bodies[recipe.body].build(api, options),
            AvatarParts.equalisers[recipe.equaliser].build(api, options),
        ];
        parts.forEach((part) => group.add(part.object));
        return { group, parts, recipe };
    },

    applyPalette(model, palette) {
        model.parts.forEach((part) => part.applyPalette(palette));
    },

    animate(model, ctx) {
        model.parts.forEach((part) => part.animate(ctx));
        model.group.rotation.y += model.recipe.spin * 0.01;
        model.group.position.y = Math.sin(ctx.time * 0.8) * model.recipe.bob;
    },
});
`;
        /* Offered as a download, and shown in the box as well: a download is quietly
           refused in some webviews, and a file you cannot get out of the page is not
           an export. */
        try {
            const blob = new Blob([source], { type: 'text/javascript' });
            const url = URL.createObjectURL(blob);
            const link = document.createElement('a');
            link.href = url;
            link.download = `${id}.js`;
            link.click();
            setTimeout(() => URL.revokeObjectURL(url), 1000);
        } catch (err) {
            console.warn('Download refused; the code is in the box instead:', err);
        }
        $('share-recipe').value = source;
        say('exported -- the file is also in the box, if the download did not start');
    }

    $('share-export').addEventListener('click', exportAsFile);

    // ---- Importing someone else's avatar ------------------------------------

    $('import-file').addEventListener('change', (event) => {
        const file = event.target.files && event.target.files[0];
        if (!file) return;
        const reader = new FileReader();
        reader.onload = () => {
            const before = new Set(HologramAvatar.avatarPlugins.keys());
            try {
                /* Run it the way the page would have, rather than with eval: a script
                   element gives the file its own scope and real syntax-error reporting
                   in the console, with a line number that matches the file. */
                const script = document.createElement('script');
                script.textContent = String(reader.result);
                document.body.appendChild(script);
                script.remove();
            } catch (err) {
                $('import-note').textContent = `That file did not load: ${err.message}`;
                return;
            }
            const added = HologramAvatar.registeredAvatars().filter((a) => !before.has(a.id));
            if (!added.length) {
                $('import-note').textContent =
                    'The file loaded but registered no avatar. It needs a HologramAvatar.registerAvatar({...}) call.';
                return;
            }
            fillAvatarPicker();
            currentAvatar = added[0].id;
            $('lab-avatar').value = currentAvatar;
            rebuild();
            $('import-note').textContent = `Loaded: ${added.map((a) => a.id).join(', ')}`;
        };
        reader.onerror = () => { $('import-note').textContent = 'That file could not be read.'; };
        reader.readAsText(file);
    });

    // ---- Start --------------------------------------------------------------

    fillPartPickers();
    writeRecipeToControls(window.CustomAvatarRecipe.load());
    fillAvatarPicker();
    fillThemePicker();
    document.querySelector('.state-btn').classList.add('is-on');
    $('share-recipe').value = JSON.stringify(window.CustomAvatarRecipe.load(), null, 2);
    rebuild();
    requestAnimationFrame(pump);
})();
