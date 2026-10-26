// One web search, shared by the companion's `search_web` and AETHER CODE's.
//
// This replaces two identical copies of a search that never searched. Both pointed at
// `api.duckduckgo.com/?format=json`, which is DuckDuckGo's *Instant Answer* API: Wikipedia
// abstracts, disambiguation topics and "official site" links. For anything news-shaped --
// the latest race, this week's release, today's price -- its `Results` array is empty and
// its `AbstractText` is blank, so both copies printed "No results found." every time.
//
// That alone would have been a visible failure. What made it an invisible one is what a
// model does with an empty result: it answers from memory and presents the answer as the
// search's. An operator in 2026 was told the most recent Grand Prix was in Baku in 2023,
// which was true of the model's training data and of nothing else.
//
// So two things changed. The request goes to the endpoint that actually searches, and an
// empty result now says what it means rather than leaving the model to fill the silence.
//
// The endpoint returns HTML, and parsing somebody else's HTML is a thing that breaks when
// they change it. That is accepted deliberately: the alternative keyless options are the
// Instant Answer API, which is what was wrong, and no search at all. The parser is written
// to fail by returning nothing rather than by returning rubbish, the markup it depends on
// is listed in one place below, and `search_endpoint` lets an operator point this at a
// SearXNG instance or a keyed API without waiting for a release.

use serde_json::Value;

use crate::llm::MemoryDb;

/// Where the search goes when the operator has not said otherwise.
///
/// The HTML endpoint rather than the API one, because this is the one that searches. `/html/`
/// is DuckDuckGo's own no-JavaScript interface, so it is a documented surface for clients
/// without a browser rather than a page scraped out from under a web app.
pub const DEFAULT_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// Setting holding an alternative endpoint taking `?q=`. Anything that answers with a page
/// of links in the markup below works; a SearXNG instance is the obvious one.
pub const ENDPOINT_SETTING: &str = "search_endpoint";

/// How many results are worth handing a model. Past about this many the list stops being
/// read and starts being context spent.
const MAX_HITS: usize = 8;

/// A snippet long enough to be a paragraph is a page, not a summary.
const MAX_SNIPPET_CHARS: usize = 400;

#[derive(Debug, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn endpoint(db: &MemoryDb) -> String {
    let configured = db.get_setting_string(ENDPOINT_SETTING, "");
    if configured.trim().is_empty() {
        DEFAULT_ENDPOINT.to_string()
    } else {
        configured.trim().to_string()
    }
}

/// The request URL for one query.
pub fn url_for(endpoint: &str, query: &str) -> String {
    let joiner = if endpoint.contains('?') { '&' } else { '?' };
    format!("{endpoint}{joiner}q={}", urlencoding::encode(query.trim()))
}

/// Reads the query out of a tool's arguments, refusing an empty one.
pub fn query_of(args: &Value) -> Result<&str, String> {
    let query = args.get("query").and_then(Value::as_str).ok_or_else(|| {
        "search_web needs a query: {\"query\": \"your search terms\"}".to_string()
    })?;
    if query.trim().is_empty() {
        return Err("search query cannot be empty".to_string());
    }
    Ok(query)
}

/// Pulls the results out of a DuckDuckGo HTML page.
///
/// The markup this depends on, all of it:
///
///   <a class="result__a" href="<link>"><b>Title</b> words</a>
///   <a class="result__snippet" ...>Snippet <b>words</b></a>
///
/// A result is only kept once it has both a link and a title, so a layout change produces
/// an empty list -- which `render` reports honestly -- rather than a list of blanks that
/// reads like a search that found nothing.
pub fn parse(html: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut cursor = 0usize;

    while let Some(found) = html[cursor..].find("result__a") {
        let marker = cursor + found;
        // Everything about this result lives between its own anchor and the next one, so
        // the snippet is looked for inside that window and cannot be borrowed from the
        // result below it.
        let next = html[marker + "result__a".len()..]
            .find("result__a")
            .map(|n| marker + "result__a".len() + n)
            .unwrap_or(html.len());
        cursor = marker + "result__a".len();

        let Some(tag_start) = html[..marker].rfind("<a") else {
            continue;
        };
        let block = &html[tag_start..next];

        let Some(url) = attribute(block, "href").map(|href| resolve_redirect(&href)) else {
            continue;
        };
        if url.is_empty() {
            continue;
        }

        let Some(title) = inner_text(block).filter(|t| !t.is_empty()) else {
            continue;
        };

        let snippet = block
            .find("result__snippet")
            .and_then(|at| block[at..].find('>').map(|gt| at + gt + 1))
            .and_then(|text_start| {
                let rest = &block[text_start..];
                rest.find("</a>").map(|end| strip_markup(&rest[..end]))
            })
            .unwrap_or_default();

        hits.push(Hit {
            title,
            url,
            snippet: truncate_chars(&snippet, MAX_SNIPPET_CHARS),
        });

        if hits.len() == MAX_HITS {
            break;
        }
    }

    hits
}

/// The value of one attribute in a tag, with its entities decoded.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))?;
    let rest = &tag[at + name.len() + 2..];
    let end = rest.find('"')?;
    Some(decode_entities(&rest[..end]))
}

/// The text of the first element in `block`, markup removed.
fn inner_text(block: &str) -> Option<String> {
    let gt = block.find('>')?;
    let rest = &block[gt + 1..];
    let end = rest.find("</a>")?;
    Some(strip_markup(&rest[..end]))
}

/// DuckDuckGo wraps a result's link in its own redirector, carrying the real destination in
/// a `uddg` parameter. The operator is given the page, not the redirect: a URL they cannot
/// read is a URL they cannot decide whether to open.
fn resolve_redirect(href: &str) -> String {
    let Some(at) = href.find("uddg=") else {
        // Not a redirect. A protocol-relative link still has to become one that opens.
        return if href.starts_with("//") {
            format!("https:{href}")
        } else {
            href.to_string()
        };
    };
    let rest = &href[at + "uddg=".len()..];
    let encoded = rest.split('&').next().unwrap_or(rest);
    urlencoding::decode(encoded)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| href.to_string())
}

/// Tags out, entities decoded, whitespace collapsed. Titles and snippets arrive with `<b>`
/// around the matched words.
fn strip_markup(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut depth = 0usize;
    for ch in fragment.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(ch),
            _ => {}
        }
    }
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The five entities that actually turn up in a title or a URL. Not an HTML parser, and the
/// consequence of missing one is a stray `&copy;` in a title rather than a wrong answer.
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Turns a fetched page into something a model can read.
///
/// `search_web` hands back titles, URLs and one-line snippets, which answers "what exists"
/// and not "what does it say". A companion that can search and not read has to either stop
/// at the snippet or fill the rest in from memory, and filling it in from memory is the
/// failure this whole area of the code keeps coming back to. So `fetch_url` reads one page,
/// and this is what it reads it with.
///
/// Not an HTML parser, deliberately, and the same trade `parse` makes: `script`, `style`,
/// `head` and the obvious page furniture are cut, every remaining tag is dropped, entities
/// are decoded and whitespace is collapsed. The failure mode is a page that reads badly,
/// never a page that reads as something it is not. A site that needs JavaScript to say
/// anything produces almost nothing, which is the honest answer for a client with no
/// browser -- and better than a plausible paragraph of navigation links.
pub fn readable_text(html: &str) -> String {
    // Cut the elements whose *content* is not prose before dropping tags, because once the
    // tags are gone a stylesheet is indistinguishable from a paragraph.
    let mut page = html.to_string();
    for element in [
        "script", "style", "head", "nav", "footer", "svg", "noscript",
    ] {
        page = without_element(&page, element);
    }

    // A block-level tag is a line break; everything else just goes.
    let mut text = String::with_capacity(page.len());
    let mut inside_tag = false;
    let mut tag = String::new();
    for ch in page.chars() {
        match ch {
            '<' => {
                inside_tag = true;
                tag.clear();
            }
            '>' if inside_tag => {
                inside_tag = false;
                let name = tag.trim_start_matches('/').to_lowercase();
                let name = name.split([' ', '\t', '\n', '/']).next().unwrap_or("");
                if BREAKS_THE_LINE.contains(&name) {
                    text.push('\n');
                } else {
                    text.push(' ');
                }
            }
            _ if inside_tag => tag.push(ch),
            _ => text.push(ch),
        }
    }

    collapse(&decode_entities(&text))
}

/// One element and everything inside it, removed.
fn without_element(page: &str, element: &str) -> String {
    let open = format!("<{element}");
    let close = format!("</{element}>");
    let mut kept = String::with_capacity(page.len());
    let mut remaining = page;
    loop {
        let lowered = remaining.to_lowercase();
        let Some(start) = lowered.find(&open) else {
            kept.push_str(remaining);
            return kept;
        };
        // `<script` matches `<scriptish` too, so the character after the name has to be one
        // that can end a tag name.
        let after = lowered[start + open.len()..].chars().next();
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-') {
            kept.push_str(&remaining[..start + open.len()]);
            remaining = &remaining[start + open.len()..];
            continue;
        }
        kept.push_str(&remaining[..start]);
        match lowered[start..].find(&close) {
            Some(offset) => remaining = &remaining[start + offset + close.len()..],
            // An unclosed one takes the rest of the page with it, which is the right
            // reading: whatever follows an unterminated <script> is inside it.
            None => return kept,
        }
    }
}

/// Tags whose boundary is a line break rather than a space. Without this a page of list
/// items arrives as one paragraph with the words run together.
const BREAKS_THE_LINE: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "header",
    "blockquote",
    "pre",
    "table",
    "ul",
    "ol",
    "dd",
    "dt",
];

/// Runs of blank space become one space, runs of blank lines become one blank line.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_lines = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank_lines += 1;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blank_lines > 0 {
                out.push('\n');
            }
        }
        blank_lines = 0;
        out.push_str(&line);
    }
    out
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    format!("{}…", text.chars().take(limit).collect::<String>())
}

/// The results as the model reads them.
///
/// The empty case is the one that matters. "No results found." was a true statement that
/// taught a model nothing about what to do next, and what it did next was answer from
/// memory and label the answer as the search's. So the empty case carries an instruction,
/// and it is the instruction rather than the wording that is load-bearing: say the search
/// found nothing, do not substitute what you remember.
pub fn render(query: &str, hits: &[Hit]) -> String {
    if hits.is_empty() {
        return format!(
            "Search results for: {query}\n\n\
             The search returned nothing. This is not evidence that the answer does not \
             exist -- it means this search did not find it.\n\n\
             Tell the operator the search came back empty. Do not answer from memory as \
             though these results had supported it: your training ended before today, so \
             anything you recall as current may be years out of date. If you give what you \
             remember, say that is what it is and say roughly how old it is."
        );
    }

    let mut out = format!(
        "Search results for: {query}\n\n{} result(s). These are titles and snippets, not \
         whole pages. Quote them as what the search said; for anything that turns on the \
         detail, call `fetch_url` on the result most likely to carry it and answer from the \
         page. Do not hand the operator a URL instead of an answer -- you can open it \
         yourself, and a link is what they would have got from searching themselves.\n\n",
        hits.len()
    );
    for (index, hit) in hits.iter().enumerate() {
        out.push_str(&format!("{}. {}\n   {}\n", index + 1, hit.title, hit.url));
        if !hit.snippet.is_empty() {
            out.push_str(&format!("   {}\n", hit.snippet));
        }
    }
    out
}

/// A browser-shaped User-Agent. The old one was the string "AETHER1", which the HTML
/// endpoint answers with a block page rather than results -- the API endpoint did not care,
/// so nothing noticed until the request moved.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real `html.duckduckgo.com/html/` response: the two classes the parser
    /// reads, the redirector around the link, `<b>` inside the title, and an entity.
    const PAGE: &str = r#"
      <div class="result results_links">
        <h2 class="result__title">
          <a class="result__a" rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.formula1.com%2Fen%2Fresults&amp;rut=abc">
            <b>Formula 1</b> Race Results &amp; Standings
          </a>
        </h2>
        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">
          Official <b>F1</b> results for every round of the season.
        </a>
      </div>
      <div class="result results_links">
        <h2 class="result__title">
          <a class="result__a" href="https://example.org/two">Second Result</a>
        </h2>
        <a class="result__snippet">Plain snippet.</a>
      </div>
    "#;

    #[test]
    fn a_result_gives_up_its_title_url_and_snippet() {
        let hits = parse(PAGE);
        assert_eq!(hits.len(), 2, "{hits:#?}");
        assert_eq!(hits[0].title, "Formula 1 Race Results & Standings");
        assert_eq!(hits[0].url, "https://www.formula1.com/en/results");
        assert!(hits[0].snippet.contains("every round of the season"));
    }

    /// The whole point of touching this file: the link the operator is shown has to be the
    /// page, not DuckDuckGo's redirector wrapped around it.
    #[test]
    fn the_redirector_is_unwrapped_and_a_plain_link_is_left_alone() {
        let hits = parse(PAGE);
        assert!(
            !hits[0].url.contains("duckduckgo.com/l/"),
            "left the redirect in: {}",
            hits[0].url
        );
        assert_eq!(hits[1].url, "https://example.org/two");
    }

    /// A snippet belongs to the result above it. Searching the whole page from each anchor
    /// would give every result the first snippet on it.
    #[test]
    fn a_snippet_does_not_leak_from_the_next_result() {
        let hits = parse(PAGE);
        assert_eq!(hits[1].snippet, "Plain snippet.");
    }

    /// Markup this parser does not recognise must produce nothing, so `render` reports an
    /// empty search. A list of blank entries would read as a search that worked.
    #[test]
    fn unrecognised_markup_yields_no_results_rather_than_empty_ones() {
        assert!(parse("<html><body><p>nothing here</p></body></html>").is_empty());
        // The class is there but the anchor carries no usable link.
        assert!(parse(r#"<a class="result__a" href="">Title</a>"#).is_empty());
    }

    /// The failure this whole module exists to stop. An empty result has to forbid the
    /// substitution, not merely report the emptiness -- "No results found." was true and
    /// the model answered from 2023 anyway.
    #[test]
    fn an_empty_search_forbids_answering_from_memory() {
        let text = render("latest F1 race results", &[]);
        assert!(text.contains("came back empty"), "{text}");
        assert!(text.contains("Do not answer from memory"), "{text}");
        assert!(text.contains("training ended before today"), "{text}");
    }

    #[test]
    fn results_are_numbered_with_their_urls() {
        let text = render("f1", &parse(PAGE));
        assert!(text.contains("1. Formula 1 Race Results & Standings"));
        assert!(text.contains("https://www.formula1.com/en/results"));
        assert!(text.contains("2. Second Result"));
    }

    #[test]
    fn the_query_is_escaped_into_the_url_and_an_existing_one_is_appended_to() {
        assert_eq!(
            url_for(DEFAULT_ENDPOINT, "latest F1 & results"),
            "https://html.duckduckgo.com/html/?q=latest%20F1%20%26%20results"
        );
        assert!(url_for("https://searx.example/search?format=json", "x").contains("&q=x"));
    }

    #[test]
    fn a_long_snippet_is_cut_rather_than_carried_whole() {
        let page = format!(
            r#"<a class="result__a" href="https://e.org">T</a><a class="result__snippet">{}</a>"#,
            "word ".repeat(400)
        );
        let hits = parse(&page);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.chars().count() <= MAX_SNIPPET_CHARS + 1);
    }

    #[test]
    fn a_missing_or_blank_query_is_refused() {
        assert!(query_of(&serde_json::json!({})).is_err());
        assert!(query_of(&serde_json::json!({"query": "   "})).is_err());
        assert_eq!(query_of(&serde_json::json!({"query": "f1"})).unwrap(), "f1");
    }

    #[test]
    fn a_page_becomes_readable_text_without_its_machinery() {
        let html = r#"<html><head><title>t</title><style>body{color:red}</style></head>
            <body><nav>Home About</nav><h1>Race result</h1>
            <p>Driver A won the Grand&nbsp;Prix.</p>
            <ul><li>First: A</li><li>Second: B</li></ul>
            <script>var tracking = "Driver Z won";</script>
            <footer>&copy; nobody</footer></body></html>"#;
        let text = readable_text(html);
        assert!(text.contains("Race result"), "{text}");
        assert!(text.contains("Driver A won the Grand Prix."), "{text}");
        // Each list item on its own line, not run together into one word.
        assert!(text.contains("First: A"), "{text}");
        assert!(text.contains("Second: B"), "{text}");
        // The furniture and the script are gone. A tracking variable that happens to
        // contain a sentence is the worst thing that could reach a model from a page.
        assert!(!text.contains("Driver Z"), "{text}");
        assert!(!text.contains("color:red"), "{text}");
        assert!(!text.contains("Home About"), "{text}");
    }

    /// `<script` is a prefix of `<scription`, and cutting on the prefix alone would delete
    /// the rest of a page from the first word that merely started the same way.
    #[test]
    fn an_element_whose_name_merely_starts_the_same_is_kept() {
        let text = readable_text("<p>before</p><scriptish>kept</scriptish><p>after</p>");
        assert!(text.contains("kept"), "{text}");
        assert!(text.contains("before") && text.contains("after"), "{text}");
    }

    /// A page that says nothing readable produces nothing, rather than a guess. The tool
    /// turns this into a sentence telling the model not to describe the page.
    #[test]
    fn a_page_with_no_prose_produces_no_text() {
        assert!(
            readable_text("<html><body><script>x=1</script></body></html>")
                .trim()
                .is_empty()
        );
        assert!(readable_text("").is_empty());
    }

    /// An unterminated script takes the rest of the page with it: whatever follows one is
    /// inside it, and leaking JavaScript into a model's context as prose is the failure
    /// this is guarding, not a missing paragraph.
    #[test]
    fn an_unclosed_script_does_not_leak_its_contents() {
        let text = readable_text("<p>kept</p><script>var a = 'not prose';");
        assert!(text.contains("kept"), "{text}");
        assert!(!text.contains("not prose"), "{text}");
    }

    /// `render`'s own header told the model to "give the operator the URL to open for
    /// anything that turns on the detail" -- written before `fetch_url` existed, and still
    /// being obeyed after it did. Trident's 2026-10-26 log is a search that worked followed
    /// by a link and an offer, which is exactly what this text asked for.
    ///
    /// This wording is unconditional, which is safe because `search_web` and `fetch_url`
    /// sit together in every persona's tool list -- pinned by
    /// `persona::tests::every_persona_that_can_search_can_also_read_a_page`.
    #[test]
    fn rendered_results_send_the_model_to_the_page_rather_than_to_the_operator() {
        let rendered = render(
            "latest f1 race results",
            &[Hit {
                title: "F1 results".to_string(),
                url: "https://www.formula1.com/en/results".to_string(),
                snippet: "latest grand prix classification".to_string(),
            }],
        );
        assert!(
            rendered.contains("fetch_url"),
            "the snippets have to point at the follow-up: {rendered}"
        );
        assert!(
            !rendered.contains("give the operator the URL"),
            "handing over a link is what the operator could already do themselves: \
             {rendered}"
        );
    }

    /// An empty search is the one case where there is nothing to open, so it must not be
    /// sent chasing a page -- and must still refuse to answer from memory.
    #[test]
    fn an_empty_search_is_not_sent_to_fetch_a_page() {
        let rendered = render("latest f1 race results", &[]);
        assert!(rendered.contains("came back empty"), "{rendered}");
        assert!(
            !rendered.contains("fetch_url"),
            "there is no result to read: {rendered}"
        );
    }
}
