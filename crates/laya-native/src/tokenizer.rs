//! GPT-2 byte-level BPE, driven entirely by the vocab + merges baked into the GGUF
//! (`tokenizer.ggml.tokens` / `tokenizer.ggml.merges`, `tokenizer.ggml.pre == "gpt2"`).
//! Pure logic: no ggml involved, unit-testable on its own.
use std::collections::HashMap;

/// GPT-2's byte<->unicode table: every byte maps to a printable char so BPE can run on text.
fn byte_to_unicode() -> [char; 256] {
    let mut used = [false; 256];
    let mut map = ['\0'; 256];
    let mut n = 0u32;
    for r in [(b'!', b'~'), (0xA1u8, 0xACu8), (0xAEu8, 0xFFu8)] {
        for b in r.0..=r.1 {
            used[b as usize] = true;
            map[b as usize] = char::from_u32(b as u32).unwrap();
        }
    }
    for b in 0..256usize {
        if !used[b] {
            map[b] = char::from_u32(256 + n).unwrap();
            n += 1;
        }
    }
    map
}

pub struct Bpe {
    vocab: HashMap<String, i32>,
    ranks: HashMap<(String, String), u32>,
    byte_enc: [char; 256],
    unk: i32,
}

impl Bpe {
    pub fn new(tokens: &[String], merges: &[String], unk: i32) -> Self {
        let vocab = tokens
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), i as i32))
            .collect();
        let mut ranks = HashMap::with_capacity(merges.len());
        for (i, m) in merges.iter().enumerate() {
            if let Some((a, b)) = m.split_once(' ') {
                ranks.insert((a.to_string(), b.to_string()), i as u32);
            }
        }
        Self { vocab, ranks, byte_enc: byte_to_unicode(), unk }
    }

    pub fn encode(&self, text: &str) -> Vec<i32> {
        let mut out = Vec::new();
        for piece in pretokenize(text) {
            let enc: String = piece.bytes().map(|b| self.byte_enc[b as usize]).collect();
            for sym in self.bpe(&enc) {
                out.push(*self.vocab.get(&sym).unwrap_or(&self.unk));
            }
        }
        out
    }

    /// Classic GPT-2 BPE: repeatedly merge the lowest-ranked adjacent pair.
    fn bpe(&self, word: &str) -> Vec<String> {
        let mut parts: Vec<String> = word.chars().map(|c| c.to_string()).collect();
        if parts.len() < 2 {
            return parts;
        }
        loop {
            let mut best: Option<(u32, usize)> = None;
            for i in 0..parts.len() - 1 {
                let key = (parts[i].clone(), parts[i + 1].clone());
                if let Some(&r) = self.ranks.get(&key) {
                    if best.map_or(true, |(br, _)| r < br) {
                        best = Some((r, i));
                    }
                }
            }
            let Some((_, _)) = best else { break };
            let (a, b) = {
                let (_, i) = best.unwrap();
                (parts[i].clone(), parts[i + 1].clone())
            };
            let mut next: Vec<String> = Vec::with_capacity(parts.len());
            let mut i = 0;
            while i < parts.len() {
                if i + 1 < parts.len() && parts[i] == a && parts[i + 1] == b {
                    next.push(format!("{a}{b}"));
                    i += 2;
                } else {
                    next.push(parts[i].clone());
                    i += 1;
                }
            }
            parts = next;
            if parts.len() < 2 {
                break;
            }
        }
        parts
    }
}

const CONTRACTIONS: [&str; 7] = ["'s", "'t", "'re", "'ve", "'m", "'ll", "'d"];

/// Hand-rolled equivalent of GPT-2's pre-tokenizer regex
/// `'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+`,
/// written out so the crate needs no regex dependency.
pub fn pretokenize(text: &str) -> Vec<String> {
    let ch: Vec<char> = text.chars().collect();
    let n = ch.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    let take = |a: usize, b: usize| -> String { ch[a..b].iter().collect() };

    while i < n {
        // `\s+(?!\S)` / `\s+`: a whitespace run keeps all but its last char when a
        // non-space follows, leaving that last char for the next ` ?X` alternative.
        if ch[i].is_whitespace() {
            let mut j = i;
            while j < n && ch[j].is_whitespace() {
                j += 1;
            }
            if j == n {
                out.push(take(i, j));
                break;
            }
            if j - 1 > i {
                out.push(take(i, j - 1));
            }
            if ch[j - 1] == ' ' {
                i = j - 1; // fall through: consumed as the optional leading space below
            } else {
                out.push(take(j - 1, j));
                i = j;
                continue;
            }
        }
        // contractions
        let rest: String = ch[i..n.min(i + 3)].iter().collect();
        if let Some(c) = CONTRACTIONS.iter().find(|c| rest.starts_with(**c)) {
            out.push((*c).to_string());
            i += c.chars().count();
            continue;
        }
        let start = i;
        let mut k = i;
        if ch[k] == ' ' {
            k += 1;
        }
        if k >= n {
            out.push(take(start, n));
            break;
        }
        let cls: fn(char) -> bool = if ch[k].is_alphabetic() {
            |c| c.is_alphabetic()
        } else if ch[k].is_numeric() {
            |c| c.is_numeric()
        } else {
            |c: char| !c.is_whitespace() && !c.is_alphabetic() && !c.is_numeric()
        };
        while k < n && cls(ch[k]) {
            k += 1;
        }
        out.push(take(start, k));
        i = k;
    }
    out
}
