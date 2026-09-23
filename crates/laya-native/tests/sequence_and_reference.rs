//! Pure-logic tests always run. The end-to-end test runs only where the real GGUF exists
//! (the Linux server) — it uses the real model, never a stub.
use laya_native::sequence::{render_options, QType, Question};
use laya_native::tokenizer::pretokenize;

const MODEL: &str = "/root/.cache/laya-models/laya_english_q8_0.gguf";

#[test]
fn gpt2_pretokenizer_splits_like_the_reference_regex() {
    assert_eq!(pretokenize("choice question: Pick the correct option."),
        vec!["choice", " question", ":", " Pick", " the", " correct", " option", "."]);
    assert_eq!(pretokenize(" A"), vec![" A"]);
    assert_eq!(pretokenize("The capital of France is: A) London B) Paris"),
        vec!["The", " capital", " of", " France", " is", ":", " A", ")", " London", " B", ")", " Paris"]);
    // contractions, digit runs, and the `\s+(?!\S)` trailing-whitespace rule
    assert_eq!(pretokenize("don't 42  x"), vec!["don", "'t", " 42", " ", " x"]);
    assert_eq!(pretokenize("a \n"), vec!["a", " \n"]);
}

#[test]
fn render_options_matches_common_py() {
    let choice = Question {
        qtype: QType::Choice,
        instructions: String::new(),
        criteria: vec![("A".into(), None), ("B".into(), Some("the other one".into()))],
    };
    assert_eq!(render_options(&choice), vec!["A", "B: the other one"]);

    let noul = Question { qtype: QType::Noul, instructions: String::new(), criteria: vec![] };
    assert_eq!(render_options(&noul),
        vec!["false: no, the statement does not hold", "true: yes, the statement holds"]);

    let score = Question {
        qtype: QType::Score,
        instructions: String::new(),
        criteria: vec![("0".into(), Some("bad".into())), ("1".into(), Some("good".into()))],
    };
    assert_eq!(render_options(&score), vec!["level 0: bad", "level 1: good"]);
}

/// End-to-end against the real Q8_0 GGUF. `laya serve` reports `input_tokens: 28` and
/// `choice: B` for this exact request; both are asserted here.
#[test]
fn reference_question_picks_b_with_28_tokens() {
    if !std::path::Path::new(MODEL).exists() {
        eprintln!("skipping: {MODEL} not present on this host");
        return;
    }
    let mut m = laya_native::LayaNative::load(MODEL, 28).expect("load");
    let opts = vec!["A".to_string(), "B".to_string()];
    let q = laya_native::choice_question("Pick the correct option.", &opts);
    let r = m.score("The capital of France is: A) London B) Paris", &q).expect("score");
    assert_eq!(r.n_tokens, 28, "token count must match `laya serve`'s reported input_tokens");
    assert_eq!(r.seq, 64, "min_seq bucket");
    assert_eq!(r.choice, 1, "option B must win, got {:?}", r.probabilities);
    assert!(r.probabilities[1] > 0.55 && r.probabilities[1] < 0.65,
        "P(B) should land near the 0.586 reference, got {}", r.probabilities[1]);
}
