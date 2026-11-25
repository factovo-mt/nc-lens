use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{CodeSettings, Diagnostic};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateWord {
    Last,
    First,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateM {
    All,
    Last,
    First,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Key {
    Address(u8),
    Group(u32),
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Address(address) => write!(f, "{}", *address as char),
            Self::Group(group) => write!(f, "G group {group}"),
        }
    }
}

pub(super) type EvaluatedWord = (u8, f64, bool, Option<u32>);
pub(super) type SourceWord = ((u8, f64), (u8, f64));

pub(super) fn retained_words(
    words: &[EvaluatedWord],
    source: &[SourceWord],
    codes: &CodeSettings,
    line: usize,
    warnings: &mut Vec<Diagnostic>,
) -> Result<Vec<bool>, String> {
    let original = |index: usize| {
        let (_, (_, value)) = source[index];
        value
    };
    let mut keep = vec![true; words.len()];
    let mut selected = HashMap::new();
    let mut duplicates = Vec::new();
    let mut m_values = Vec::new();
    let m_policy = match codes.duplicate_m {
        DuplicateM::All => None,
        DuplicateM::Last => Some(DuplicateWord::Last),
        DuplicateM::First => Some(DuplicateWord::First),
    };

    // Resolve modal groups first, since an effective G may turn other words into macro arguments.
    let mut macro_arguments = false;
    for g_pass in [true, false] {
        for (index, &(address, _, _, group)) in words.iter().enumerate() {
            if g_pass != (address == b'G') {
                continue;
            }
            let (key, policy) = match address {
                b'G' => {
                    let Some(group) = group else { continue };
                    (Key::Group(group), codes.duplicate_g_group)
                }
                b'M' if !macro_arguments => {
                    m_values.push(original(index));
                    let Some(policy) = m_policy else { continue };
                    (Key::Address(address), policy)
                }
                addr if !macro_arguments || matches!(addr, b'P' | b'L') => {
                    (Key::Address(addr), codes.duplicate_word)
                }
                _ => continue,
            };
            match selected.entry(key) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(index);
                }
                std::collections::hash_map::Entry::Occupied(mut slot) => {
                    let old = *slot.get();
                    let ignored = match policy {
                        DuplicateWord::Last => slot.insert(index),
                        DuplicateWord::First => index,
                        DuplicateWord::Error => {
                            return Err(format!(
                                "Duplicate {key}: {}, {}; block rejected",
                                original(old),
                                original(index),
                            ));
                        }
                    };
                    keep[ignored] = false;
                    if codes.warn_on_duplicate {
                        duplicates.push((ignored, key));
                    }
                }
            }
        }
        if g_pass {
            macro_arguments = words
                .iter()
                .zip(&keep)
                .any(|(&(address, value, _, _), &keep)| {
                    keep && address == b'G'
                        && ([65., 66.].contains(&value)
                            || (!codes.g_calls.is_empty()
                                && codes.g_calls.contains_key(&value.to_string())))
                });
        }
    }
    duplicates.sort_unstable_by_key(|&(index, _)| index);
    warnings.extend(duplicates.into_iter().map(|(index, key)| Diagnostic {
        src_line: line,
        message: format!(
            "Duplicate {key}: ignored {}, kept {}",
            original(index),
            original(selected[&key])
        ),
    }));
    if m_values.len() > codes.max_m_per_block
        || (codes.warn_on_duplicate && codes.duplicate_m == DuplicateM::All && m_values.len() > 1)
    {
        let adopted = match codes.duplicate_m {
            DuplicateM::All => m_values.as_slice(),
            DuplicateM::Last => &m_values[m_values.len() - 1..],
            DuplicateM::First => &m_values[..1],
        };
        let message = if m_values.len() > codes.max_m_per_block {
            format!(
                "M word count {} exceeds limit {}: kept {adopted:?}",
                m_values.len(),
                codes.max_m_per_block,
            )
        } else {
            format!("Multiple M words: kept {adopted:?}")
        };
        warnings.push(Diagnostic {
            src_line: line,
            message,
        });
    }
    Ok(keep)
}
