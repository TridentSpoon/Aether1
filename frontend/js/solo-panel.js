/* Opens the HUD on one panel when the URL says so (`?panel=voice`), before anything is
   painted.
 *
 * Lives in a file of its own rather than inline in the head because the page is served two
 * ways -- Tauri's asset protocol and the HTTP server -- and only one of those gets build-time
 * hashes for inline scripts. A file is allowed by `script-src 'self'` on both.
 *
 * See the [data-solo-panel] rules in css/layout.css and the script further down index.html
 * that adds .solo-target to the matching panel once it exists in the DOM. */
(function () {
    var m = /(?:^|[?&])panel=([a-z]+)(?:&|$)/.exec(location.search);
    if (m) document.documentElement.setAttribute('data-solo-panel', m[1]);
    /* `&hologram=1` says this window was built frameless and transparent so the page could
       draw its own edges -- the Cyberpunk board. Stamped here with the panel, before
       anything paints, because the fade and the frame are the window's background and a
       frame that appears a moment later is a flash of the wrong thing. */
    if (/(?:^|[?&])hologram=1(?:&|$)/.test(location.search)) {
        document.documentElement.setAttribute('data-solo-hologram', '');
    }
})();
