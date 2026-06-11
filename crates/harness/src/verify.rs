//! The hallucination guard: a signal whose raw_excerpt is not a verbatim
//! span of the source document does not exist. Normalization is limited to
//! Unicode NFC + whitespace collapse + typographic-quote folding so honest
//! copies survive HTML mangling while paraphrases still fail.

use unicode_normalization::UnicodeNormalization;

pub fn normalize(s: &str) -> String {
    s.nfc()
        .collect::<String>()
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True if `excerpt` appears verbatim (post-normalization) in `document`.
pub fn excerpt_in_document(excerpt: &str, document: &str) -> bool {
    let e = normalize(excerpt);
    if e.len() < 10 {
        return false; // schema minimum; defense in depth
    }
    normalize(document).contains(&e)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "MOUNT VERNON — St. Anne's pantry announced Tuesday it will cut its
        distribution schedule from five days a week to two, citing a 40 percent drop
        in donations. \u{201c}We simply cannot keep the shelves stocked,\u{201d} the director said.";

    #[test]
    fn verbatim_span_passes() {
        assert!(excerpt_in_document(
            "cut its distribution schedule from five days a week to two",
            DOC
        ));
    }

    #[test]
    fn whitespace_and_quote_mangling_tolerated() {
        assert!(excerpt_in_document(
            "\"We simply  cannot keep the shelves stocked,\"",
            DOC
        ));
    }

    #[test]
    fn paraphrase_fails() {
        assert!(!excerpt_in_document(
            "the pantry reduced its schedule from 5 days to 2",
            DOC
        ));
    }

    #[test]
    fn fabrication_fails() {
        assert!(!excerpt_in_document("400 families will lose access to food", DOC));
    }
}
