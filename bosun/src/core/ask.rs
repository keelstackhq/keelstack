//! The owner step for `bosun approve --list` (spec section 8, approval batches). Chat approvals are
//! retired (D-025): agents never ask the owner for an approval, and a form answer never counts.
//! The owner approves with a `/bosun approve <sha>` comment on the PR. Pure.

/// One ready PR as the owner step names it.
pub struct Item<'a> {
    pub pr: u64,
    pub head: &'a str,
}

/// The comment the owner posts on the PR to approve this exact head.
pub fn owner_comment(head: &str) -> String {
    format!("/bosun approve {head}")
}

/// What happens next for the ready PRs: the owner comments on each PR. `None` when nothing is
/// ready. The text asks nothing of the owner and returns no tool call.
pub fn owner_step(items: &[Item]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let lines: Vec<String> = items
        .iter()
        .map(|i| {
            format!(
                "- #{}: the owner comments `{}` on the PR",
                i.pr,
                owner_comment(i.head)
            )
        })
        .collect();
    Some(format!(
        "Waiting for the owner; agents never ask for an approval in chat (D-025): the owner comments /bosun approve <sha> on the PR (the full head SHA):\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "abcdef1234567890abcdef1234567890abcdef12";

    #[test]
    fn none_when_empty() {
        assert!(owner_step(&[]).is_none());
    }

    #[test]
    fn names_the_comment_with_the_full_head_and_no_form() {
        let t = owner_step(&[Item { pr: 12, head: H }]).unwrap();
        assert!(t.contains(&format!("`/bosun approve {H}`")), "{t}");
        assert!(
            t.contains("the owner comments /bosun approve <sha> on the PR"),
            "{t}"
        );
        assert!(
            !t.contains("AskUserQuestion") && !t.contains("answer \"approve"),
            "{t}"
        );
    }
}
