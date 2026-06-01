//! Shared text extraction utilities for memory backends.

use adk_core::Part;
use std::collections::HashSet;

/// Extract all text parts from a [`Content`](adk_core::Content) into a single string.
///
/// Parts are joined with a single space. Non-text parts (images, function calls,
/// etc.) are silently skipped.
pub fn extract_text(content: &adk_core::Content) -> String {
    content
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns `true` if the character is in a CJK Unified Ideographs block.
fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4e00}'..='\u{9fff}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4dbf}' // CJK Unified Ideographs Extension A
        | '\u{f900}'..='\u{faff}' // CJK Compatibility Ideographs
        | '\u{2e80}'..='\u{2eff}' // CJK Radicals Supplement
        | '\u{3000}'..='\u{303f}' // CJK Symbols and Punctuation
        | '\u{3040}'..='\u{309f}' // Hiragana
        | '\u{30a0}'..='\u{30ff}' // Katakana
        | '\u{ac00}'..='\u{d7af}' // Hangul Syllables
    )
}

/// Tokenize text into a set of lowercase words for keyword matching.
///
/// For whitespace-separated languages (English, etc.), splits on whitespace.
/// For CJK text (Chinese, Japanese, Korean) which has no word-separating
/// whitespace, generates character-level unigrams and bigrams to enable
/// substring matching.
pub fn extract_words(text: &str) -> HashSet<String> {
    let mut words = HashSet::new();

    for token in text.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        let lower = token.to_lowercase();

        // Check if this token contains CJK characters
        let has_cjk = lower.chars().any(is_cjk_char);

        if has_cjk {
            // For CJK tokens, generate character unigrams and bigrams
            // This enables partial matching: "编程" matches within "用户喜欢用Rust编程"
            let chars: Vec<char> = lower.chars().collect();
            for c in &chars {
                if is_cjk_char(*c) {
                    words.insert(c.to_string());
                }
            }
            for window in chars.windows(2) {
                if window.iter().any(|c| is_cjk_char(*c)) {
                    let bigram: String = window.iter().collect();
                    words.insert(bigram);
                }
            }
            // Also insert the full token for exact matches
            words.insert(lower);
        } else {
            words.insert(lower);
        }
    }

    // Handle text with no whitespace at all (pure CJK string)
    if !text.contains(char::is_whitespace) && text.chars().any(is_cjk_char) {
        let lower = text.to_lowercase();
        let chars: Vec<char> = lower.chars().collect();
        for c in &chars {
            if is_cjk_char(*c) {
                words.insert(c.to_string());
            }
        }
        for window in chars.windows(2) {
            if window.iter().any(|c| is_cjk_char(*c)) {
                let bigram: String = window.iter().collect();
                words.insert(bigram);
            }
        }
        // Also add the full string
        words.insert(lower);
    }

    words
}

/// Extract and tokenize all text from a [`Content`](adk_core::Content) into word set.
pub fn extract_words_from_content(content: &adk_core::Content) -> HashSet<String> {
    let mut words = HashSet::new();
    for part in &content.parts {
        if let Part::Text { text } = part {
            words.extend(extract_words(text));
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_words_english() {
        let words = extract_words("Hello World foo bar");
        assert!(words.contains("hello"));
        assert!(words.contains("world"));
        assert!(words.contains("foo"));
        assert!(words.contains("bar"));
    }

    #[test]
    fn test_extract_words_cjk_bigram_matching() {
        // "用户喜欢用Rust编程" should produce bigrams that include "编程"
        let stored = extract_words("用户喜欢用Rust编程");
        let query = extract_words("编程");

        // The query "编程" should match because it's a bigram in the stored text
        let matches: HashSet<_> = stored.intersection(&query).collect();
        assert!(
            !matches.is_empty(),
            "CJK search should find matches. Stored: {stored:?}, Query: {query:?}"
        );
    }

    #[test]
    fn test_extract_words_cjk_single_char() {
        let stored = extract_words("今天天气很好");
        let query = extract_words("天气");

        let matches: HashSet<_> = stored.intersection(&query).collect();
        assert!(
            !matches.is_empty(),
            "CJK bigram '天气' should match. Stored: {stored:?}, Query: {query:?}"
        );
    }

    #[test]
    fn test_extract_words_mixed_cjk_english() {
        let words = extract_words("Hello 你好 World");
        assert!(words.contains("hello"));
        assert!(words.contains("world"));
        assert!(words.contains("你"));
        assert!(words.contains("好"));
        assert!(words.contains("你好"));
    }

    #[test]
    fn test_extract_words_japanese() {
        let stored = extract_words("東京タワー");
        let query = extract_words("東京");

        let matches: HashSet<_> = stored.intersection(&query).collect();
        assert!(!matches.is_empty(), "Japanese bigram should match");
    }
}
