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
         whole pages -- quote them as what the search said, and give the operator the URL \
         to open for anything that turns on the detail.\n\n",
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
}
