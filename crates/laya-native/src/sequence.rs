//! Faithful port of `laya/common.py`'s `render_options` + `build_sequence`.
//! Format: `[CLS] <type> question: <ins> [SEP] [MASK] opt0 [MASK] opt1 ... [SEP] state [SEP]`.
use crate::tokenizer::Bpe;

pub const MASK_TOKEN: &str = "[MASK]";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QType {
    Choice = 0,
    Score = 1,
    Noul = 2,
}

impl QType {
    pub fn name(self) -> &'static str {
        match self {
            QType::Choice => "choice",
            QType::Score => "score",
            QType::Noul => "noul",
        }
    }
}

pub struct Question {
    pub qtype: QType,
    pub instructions: String,
    /// Criteria in label-index order: `(label, optional description)`.
    pub criteria: Vec<(String, Option<String>)>,
}

/// `render_options`: option texts in label-index order.
pub fn render_options(q: &Question) -> Vec<String> {
    match q.qtype {
        QType::Choice => q
            .criteria
            .iter()
            .map(|(k, v)| match v {
                Some(d) if !d.is_empty() => format!("{k}: {d}"),
                _ => k.clone(),
            })
            .collect(),
        QType::Score => q
            .criteria
            .iter()
            .enumerate()
            .map(|(i, (_, v))| format!("level {i}: {}", v.clone().unwrap_or_default()))
            .collect(),
        QType::Noul => {
            let get = |name: &str, dflt: &str| {
                q.criteria
                    .iter()
                    .find(|(k, _)| k == name)
                    .and_then(|(_, v)| v.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| dflt.to_string())
            };
            vec![
                format!("false: {}", get("false", "no, the statement does not hold")),
                format!("true: {}", get("true", "yes, the statement holds")),
            ]
        }
    }
}

pub struct BuiltSequence {
    pub ids: Vec<i32>,
    /// Index of each option's `[MASK]` marker inside `ids`.
    pub markers: Vec<i32>,
}

pub struct SpecialIds {
    pub cls: i32,
    pub sep: i32,
    pub mask: i32,
}

/// `build_sequence`, including the option-budget truncation branch.
pub fn build_sequence(
    tok: &Bpe,
    sp: &SpecialIds,
    state: &str,
    q: &Question,
    max_len: usize,
    head_max_len: usize,
) -> BuiltSequence {
    let opts = render_options(q);
    let ins = q.instructions.replace(MASK_TOKEN, " ");
    let mut head_ids = tok.encode(&format!("{} question: {}", q.qtype.name(), ins));

    let mut opt_ids: Vec<Vec<i32>> = opts
        .iter()
        .map(|o| {
            let mut v = vec![sp.mask];
            let mut body = tok.encode(&format!(" {}", o.replace(MASK_TOKEN, " ")));
            body.truncate(48);
            v.extend(body);
            v
        })
        .collect();

    let total: usize = opt_ids.iter().map(|o| o.len()).sum();
    let mut opt_budget = head_max_len as isize - total as isize;
    if opt_budget < 16 {
        let per = std::cmp::max(4, (head_max_len - 16) / std::cmp::max(1, opt_ids.len()));
        for o in opt_ids.iter_mut() {
            o.truncate(per);
        }
        let total: usize = opt_ids.iter().map(|o| o.len()).sum();
        opt_budget = head_max_len as isize - total as isize;
    }
    head_ids.truncate(std::cmp::max(8, opt_budget.max(0) as usize));

    let mut ids = vec![sp.cls];
    ids.extend(&head_ids);
    ids.push(sp.sep);
    let mut markers = Vec::with_capacity(opt_ids.len());
    for o in &opt_ids {
        markers.push(ids.len() as i32);
        ids.extend(o);
    }
    ids.push(sp.sep);

    let room = max_len.saturating_sub(ids.len() + 1);
    let mut st = tok.encode(&state.replace(MASK_TOKEN, " "));
    st.truncate(room);
    ids.extend(st);
    ids.push(sp.sep);
    ids.truncate(max_len);

    let lim = max_len as i32;
    markers.retain(|m| *m < lim);
    BuiltSequence { ids, markers }
}
