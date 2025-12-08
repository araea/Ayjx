use std::collections::HashSet;
use std::sync::OnceLock;

const ALL_WORDS_JSON: &str = include_str!("../../../res/ciyi/all_words.json");
const QUESTION_WORDS_JSON: &str = include_str!("../../../res/ciyi/question_words.json");

static ALL_WORDS: OnceLock<HashSet<String>> = OnceLock::new();
static QUESTION_WORDS: OnceLock<Vec<String>> = OnceLock::new();

pub fn get_all_words() -> &'static HashSet<String> {
    ALL_WORDS.get_or_init(|| {
        let mut json = ALL_WORDS_JSON.to_string();
        let words: Vec<String> =
            unsafe { simd_json::from_str(&mut json).expect("Failed to parse all_words.json") };
        words.into_iter().collect()
    })
}

pub fn get_question_words() -> &'static Vec<String> {
    QUESTION_WORDS.get_or_init(|| {
        let mut json = QUESTION_WORDS_JSON.to_string();
        unsafe { simd_json::from_str(&mut json).expect("Failed to parse question_words.json") }
    })
}
