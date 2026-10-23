//! Retrieval: finding what a vault knows about a question, by its words, by
//! their meaning, and through the links between notes.
//!
//! A question goes through three stages, each cheap enough that the whole
//! answers in milliseconds on a vault of thousands of notes:
//!
//! 1. **Recall.** Two independent searches over passages: BM25 over the words,
//!    stemmed, which finds names, identifiers and error messages exactly, and
//!    a vector search over meanings, which finds what was said in other words
//!    or in another language. The vector search reads only the first
//!    components of each vector, streamed past the question.
//! 2. **Score.** Each note found is scored by its best passage's cosine with
//!    the question, over whole vectors, and a little more for the words it
//!    shares with it. A note whose best cosine is under a floor has nothing to
//!    do with the question, so a question the vault can't answer gets no
//!    answer rather than the nearest thing to one.
//! 3. **Graph.** Personalized PageRank from the best notes, over the vault's
//!    own links, the way HippoRAG uses a knowledge graph: an answer that the
//!    others link to rises above one that stands alone, and a hub every note
//!    links to rises less than a note linked for a reason.
//!
//! The graph costs no language-model tokens to build: it is the links the
//! notes already have, resolved once whenever the index changes. Vectors come
//! from an embedding model reached through [`Embed`]; without one, the index
//! still answers by words and links.

pub mod graph;
pub mod search;
pub mod segment;
pub mod store;
pub mod text;

/// Turns text into vectors. Implemented by whatever reaches the model.
pub trait Embed {
    /// One vector per text, in the order given.
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

/// How texts are introduced to the model.
///
/// Retrieval models are trained to see a question and a passage marked as
/// such, and score noticeably worse without it. The defaults are
/// EmbeddingGemma's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompts {
    /// Put before a question.
    pub query: String,
    /// Put before a passage; `{title}` is replaced by the note's title.
    pub document: String,
}

impl Default for Prompts {
    fn default() -> Self {
        Self {
            query: "task: search result | query: ".into(),
            document: "title: {title} | text: ".into(),
        }
    }
}

impl Prompts {
    #[must_use]
    pub fn query(&self, text: &str) -> String {
        format!("{}{text}", self.query)
    }

    #[must_use]
    pub fn document(&self, title: &str, text: &str) -> String {
        format!("{}{text}", self.document.replace("{title}", title))
    }
}

/// A stand-in model for tests: each word, lowercased, lights up one
/// component, so texts that share words point the same way. Crude, but it
/// makes "closer in meaning" checkable without a real model.
///
/// Every text also shares one component, as a real model's vectors share a
/// direction, so the cosines fall where EmbeddingGemma's do: a half for
/// texts with nothing in common, and over recall's floor for texts with
/// most of their words in common.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use super::Embed;

    #[derive(Debug, Default)]
    pub struct Words {
        /// How many texts it has embedded, over every call.
        pub embedded: usize,
        /// Fails every call when set, as a server that has gone away does.
        pub broken: bool,
        /// Fails every call once it has embedded this many texts, as a
        /// server that goes away part-way through does.
        pub fails_after: Option<usize>,
    }

    pub const DIMS: usize = 256;

    impl Embed for Words {
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            if self.broken || self.fails_after.is_some_and(|n| self.embedded >= n) {
                return Err("connection refused".into());
            }
            self.embedded += texts.len();
            Ok(texts.iter().map(|text| vector(text)).collect())
        }
    }

    /// A text's words, minus the prompt around them, hashed into components
    /// from the third on, and the first, which every text shares, as long as
    /// they are together: so the cosine of two texts is half of one more than
    /// the cosine of their words. A text without words has the second
    /// component for them, so it is like nothing else either.
    #[must_use]
    pub fn vector(text: &str) -> Vec<f32> {
        let text = text.rsplit(" | ").next().unwrap_or(text);
        let text = text.split_once(": ").map_or(text, |(_, rest)| rest);
        let mut words = vec![0.0f32; DIMS];
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
        {
            let slot = 2 + super::store::fnv(word.to_lowercase().as_bytes()) as usize % (DIMS - 2);
            words[slot] += 1.0;
        }
        let mut vector = super::store::fit(&words, DIMS);
        if vector.iter().all(|&x| x == 0.0) {
            vector[1] = 1.0;
        }
        vector[0] = 1.0;
        super::store::fit(&vector, DIMS)
    }
}
