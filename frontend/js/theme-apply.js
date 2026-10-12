/* Applies the stored theme to the document, before anything is painted.
 *
 * One line, but it is the same one line on all four pages, and it has to run after theme.js
 * has defined Aether1Theme and before the body renders -- otherwise the page paints once in
 * the fallback palette first, which on a light theme is a dark flash.
 *
 * A file rather than an inline call because the pages are served both over Tauri's asset
 * protocol and over the HTTP server, and `script-src 'self'` is the one rule that holds on
 * both without build-time hashes. */
Aether1Theme.apply(document);
