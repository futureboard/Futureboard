use super::model::{KeyBinding, KeymapConflict, KeymapSource, ResolvedKeyBinding};
use super::normalize::canonical_accel;
use std::collections::HashMap;

pub fn contexts_overlap(a: Option<&str>, b: Option<&str>) -> bool {
    match (
        a.map(str::trim).filter(|s| !s.is_empty()),
        b.map(str::trim).filter(|s| !s.is_empty()),
    ) {
        (None, _) | (_, None) => true,
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
    }
}

pub fn find_conflicts_for_binding(
    candidate: &KeyBinding,
    resolved: &[ResolvedKeyBinding],
    exclude_action: Option<&str>,
) -> Vec<KeymapConflict> {
    let mut out = Vec::new();
    for key in &candidate.keys {
        let Some(token) = canonical_accel(key) else {
            continue;
        };
        for existing in resolved {
            if exclude_action.is_some_and(|action| action == existing.action) {
                continue;
            }
            if !existing
                .keys
                .iter()
                .any(|k| canonical_accel(k).as_deref() == Some(token.as_str()))
            {
                continue;
            }
            if !contexts_overlap(candidate.context.as_deref(), existing.context.as_deref()) {
                continue;
            }
            out.push(KeymapConflict {
                keystroke: key.clone(),
                action: existing.action.clone(),
                action_label: existing.action.clone(),
                context: existing.context.clone(),
                source: existing.source,
            });
        }
    }
    out
}

pub fn annotate_row_conflicts(
    rows: &mut [super::model::KeymapRow],
    bindings: &[ResolvedKeyBinding],
) {
    let mut by_token: HashMap<String, Vec<&ResolvedKeyBinding>> = HashMap::new();
    for binding in bindings {
        for key in &binding.keys {
            if let Some(token) = canonical_accel(key) {
                by_token.entry(token).or_default().push(binding);
            }
        }
    }
    for row in rows.iter_mut() {
        row.is_conflict = false;
        row.conflict_with.clear();
        for key in &row.keystrokes {
            let Some(token) = canonical_accel(key) else {
                continue;
            };
            let Some(entries) = by_token.get(&token) else {
                continue;
            };
            for other in entries {
                if other.action == row.action_id {
                    continue;
                }
                if !contexts_overlap(row.context.as_deref(), other.context.as_deref()) {
                    continue;
                }
                row.is_conflict = true;
                if !row.conflict_with.iter().any(|id| id == &other.action) {
                    row.conflict_with.push(other.action.clone());
                }
            }
        }
    }
}
