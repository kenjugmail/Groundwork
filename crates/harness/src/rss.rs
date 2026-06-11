//! RSS/Atom feed parsing and article-body extraction. Pure functions over
//! recorded captures — replayable by construction.

use chrono::{DateTime, Utc};
use scraper::{Html, Selector};

#[derive(Debug, Clone)]
pub struct FeedItem {
    pub url: String,
    pub title: String,
    pub published: Option<DateTime<Utc>>,
}

pub fn parse_feed(bytes: &[u8]) -> anyhow::Result<Vec<FeedItem>> {
    let feed = feed_rs::parser::parse(bytes)?;
    Ok(feed
        .entries
        .into_iter()
        .filter_map(|e| {
            let url = e.links.first()?.href.clone();
            Some(FeedItem {
                url,
                title: e.title.map(|t| t.content).unwrap_or_default(),
                published: e.published.or(e.updated).map(|d| d.with_timezone(&Utc)),
            })
        })
        .collect())
}

/// HTML → readable plain text. Prefers <article>, falls back to <p> soup.
/// Capped so a pathological page can't blow the prompt budget.
pub fn article_text(html_bytes: &[u8]) -> String {
    const MAX_CHARS: usize = 12_000;
    let html = String::from_utf8_lossy(html_bytes);
    let doc = Html::parse_document(&html);
    let para = Selector::parse("p").unwrap();
    let article = Selector::parse("article").unwrap();

    let paragraphs: Vec<String> = match doc.select(&article).next() {
        Some(node) => node
            .select(&para)
            .map(|p| p.text().collect::<String>())
            .collect(),
        None => doc.select(&para).map(|p| p.text().collect::<String>()).collect(),
    };
    let mut text = paragraphs
        .iter()
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| p.len() > 40) // drop nav/boilerplate fragments
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.len() > MAX_CHARS {
        // truncate on a char boundary
        let mut cut = MAX_CHARS;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_article_paragraphs() {
        let html = br#"<html><body><nav><p>Menu</p></nav><article>
            <p>MOUNT VERNON - The pantry announced Tuesday it will cut its distribution schedule from five days a week to two.</p>
            <p>Officials said they were exploring emergency funding options for the families affected.</p>
            </article></body></html>"#;
        let text = article_text(html);
        assert!(text.contains("cut its distribution schedule"));
        assert!(!text.contains("Menu"));
    }
}
