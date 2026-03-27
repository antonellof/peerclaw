//! Context compaction engine for long agent conversations.
//!
//! Prevents context window overflow by intelligently pruning conversation history.
//! Two strategies are provided:
//!
//! - **Pruning** (fast, no LLM needed): removes old tool results and truncates large
//!   outputs while preserving the system prompt and recent turns.
//!
//! - **LLM-powered summary** (optional): compresses old history into a single summary
//!   message for higher-quality compaction.

use crate::executor::task::{ChatMessage, MessageRole};

/// Maximum character length for a single tool result before it gets truncated.
const TOOL_RESULT_TRUNCATE_CHARS: usize = 500;

/// Number of recent turns (user + assistant pairs) to always preserve.
const KEEP_RECENT_TURNS: usize = 4;

/// Prefix that identifies a message as a tool result in our conversation format.
const TOOL_RESULT_PREFIX: &str = "Tool result for ";

/// Prefix for tool error messages.
const TOOL_ERROR_PREFIX: &str = "Tool '";

/// Estimate token count from character length (rough: 1 token ~ 4 chars).
pub fn estimate_tokens(chars: usize) -> usize {
    chars / 4
}

/// Estimate total character length of a conversation.
pub fn conversation_char_size(messages: &[ChatMessage]) -> usize {
    messages.iter().map(|m| m.content.len() + 20).sum()
}

/// Returns `true` if the conversation should be compacted given the model's context window.
///
/// Triggers at 75% of the context window (in characters).
pub fn needs_compaction(messages: &[ChatMessage], context_window_tokens: u32) -> bool {
    let max_chars = (context_window_tokens as usize) * 4;
    let threshold = max_chars * 3 / 4;
    conversation_char_size(messages) > threshold
}

/// Returns `true` if a user message looks like a tool result or tool error.
fn is_tool_result_message(msg: &ChatMessage) -> bool {
    msg.role == MessageRole::User
        && (msg.content.starts_with(TOOL_RESULT_PREFIX)
            || msg.content.starts_with(TOOL_ERROR_PREFIX)
            || msg.content.starts_with("Tool error: "))
}

/// Truncate a tool result string to `max_chars`, appending an ellipsis marker.
fn truncate_tool_result(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    // Find a clean break point (newline or space) near the limit.
    let break_at = content[..max_chars]
        .rfind('\n')
        .or_else(|| content[..max_chars].rfind(' '))
        .unwrap_or(max_chars);
    format!(
        "{}...(truncated, {} chars omitted)",
        &content[..break_at],
        content.len() - break_at
    )
}

/// **Strategy A: Pruning** -- fast, no LLM call required.
///
/// Reduces conversation size to fit within `max_chars` by:
/// 1. Truncating large tool results to 500 chars each.
/// 2. Dropping old tool results entirely (keeping the tool call in the assistant message).
/// 3. Dropping old assistant reasoning messages.
///
/// Always preserves:
/// - The system prompt (index 0).
/// - The last `KEEP_RECENT_TURNS` user+assistant exchanges.
/// - Duplicate-merge system notes.
pub fn prune_conversation(messages: &mut Vec<ChatMessage>, max_chars: usize) {
    if messages.is_empty() {
        return;
    }

    let current_size = conversation_char_size(messages);
    if current_size <= max_chars {
        return;
    }

    // Identify the boundary: everything before `protected_start` is eligible for pruning.
    // We protect the system prompt (index 0) and the last N user+assistant exchanges.
    let protected_start = find_protected_boundary(messages);

    // Pass 1: Truncate all tool results in the pruneable range.
    let mut size = current_size;
    for msg in messages[1..protected_start].iter_mut() {
        if is_tool_result_message(msg) && msg.content.len() > TOOL_RESULT_TRUNCATE_CHARS {
            let old_len = msg.content.len();
            msg.content = truncate_tool_result(&msg.content, TOOL_RESULT_TRUNCATE_CHARS);
            size = size.saturating_sub(old_len - msg.content.len());
        }
    }

    if size <= max_chars {
        return;
    }

    // Pass 2: Drop old tool result messages entirely (replace with a short note).
    let mut indices_to_summarize = Vec::new();
    for i in 1..protected_start {
        if is_tool_result_message(&messages[i]) {
            indices_to_summarize.push(i);
        }
    }

    // Remove from back to front to keep indices valid.
    for &i in indices_to_summarize.iter().rev() {
        let removed_len = messages[i].content.len() + 20;
        messages.remove(i);
        size = size.saturating_sub(removed_len);
    }

    if size <= max_chars {
        return;
    }

    // Recalculate protected boundary after removals.
    let protected_start = find_protected_boundary(messages);

    // Pass 3: Drop old assistant reasoning messages (keep system + recent).
    let mut drop_indices = Vec::new();
    for i in 1..protected_start {
        if messages[i].role == MessageRole::Assistant {
            drop_indices.push(i);
        }
    }

    for &i in drop_indices.iter().rev() {
        let removed_len = messages[i].content.len() + 20;
        messages.remove(i);
        size = size.saturating_sub(removed_len);
    }

    if size <= max_chars {
        return;
    }

    // Recalculate protected boundary after removals.
    let protected_start = find_protected_boundary(messages);

    // Pass 4: Drop remaining old user messages (not system, not recent).
    let mut drop_indices = Vec::new();
    for i in 1..protected_start {
        if messages[i].role == MessageRole::User {
            drop_indices.push(i);
        }
    }

    for &i in drop_indices.iter().rev() {
        let removed_len = messages[i].content.len() + 20;
        messages.remove(i);
        size = size.saturating_sub(removed_len);
    }

    // If we still exceed the limit after all passes, truncate the system prompt
    // as a last resort (keep first 2000 chars).
    if size > max_chars && !messages.is_empty() && messages[0].role == MessageRole::System {
        if messages[0].content.len() > 2000 {
            messages[0].content = truncate_tool_result(&messages[0].content, 2000);
        }
    }
}

/// Find the index where the "protected" recent tail starts.
/// We protect the last `KEEP_RECENT_TURNS` user messages and everything after
/// the first of those user messages.
fn find_protected_boundary(messages: &[ChatMessage]) -> usize {
    // Count user messages from the end.
    let mut user_count = 0;
    let mut boundary = messages.len();
    for i in (1..messages.len()).rev() {
        if messages[i].role == MessageRole::User && !is_tool_result_message(&messages[i]) {
            user_count += 1;
            if user_count >= KEEP_RECENT_TURNS {
                boundary = i;
                break;
            }
        }
    }
    // Clamp: at minimum protect the last message.
    boundary.min(messages.len().saturating_sub(1)).max(1)
}

/// **Strategy B: LLM-powered compaction** -- produces a summary message.
///
/// Takes old conversation messages and returns a system message containing a
/// condensed summary that can replace the old history.
///
/// The caller is responsible for actually sending `summary_prompt` to the LLM
/// and obtaining the summary text, then passing it here to build the replacement
/// message.
pub fn build_summary_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::from(
        "Summarize the following conversation history in a concise paragraph. \
         Focus on: what the user asked, what tools were called and their key results, \
         and any conclusions reached. Be factual and brief.\n\n---\n\n",
    );

    for msg in messages {
        let role = match msg.role {
            MessageRole::System => "System",
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
        };
        prompt.push_str(&format!("{}: {}\n\n", role, msg.content));
    }

    prompt.push_str("---\n\nConcise summary:");
    prompt
}

/// Create a system message containing the conversation summary, suitable for
/// replacing old history.
pub fn summary_to_system_message(summary: &str) -> ChatMessage {
    ChatMessage::system(format!(
        "(Conversation summary from earlier turns: {})",
        summary
    ))
}

/// **String-based pruning** for the web agentic loop which uses a single `String`
/// conversation rather than `Vec<ChatMessage>`.
///
/// Preserves the system prefix (first `prefix_len` characters) and the most recent
/// conversation content, intelligently dropping old tool results first.
pub fn prune_string_conversation(
    conversation: &str,
    prefix_len: usize,
    max_chars: usize,
) -> String {
    if conversation.len() <= max_chars {
        return conversation.to_string();
    }

    let prefix = &conversation[..prefix_len.min(conversation.len())];
    let body = &conversation[prefix_len.min(conversation.len())..];

    // Split body into sections delimited by double newlines.
    let sections: Vec<&str> = body.split("\n\n").collect();

    // Classify sections: tool results are heavier candidates for removal.
    let mut tool_result_indices = Vec::new();
    let mut other_indices = Vec::new();

    for (i, section) in sections.iter().enumerate() {
        let trimmed = section.trim();
        if trimmed.starts_with("Tool result for ")
            || trimmed.starts_with("Tool '")
            || trimmed.starts_with("Tool error:")
        {
            tool_result_indices.push(i);
        } else {
            other_indices.push(i);
        }
    }

    // First: truncate large tool results.
    let mut owned_sections: Vec<String> = sections.iter().map(|s| s.to_string()).collect();
    let mut size: usize = prefix.len() + owned_sections.iter().map(|s| s.len() + 2).sum::<usize>();

    for &i in &tool_result_indices {
        if size <= max_chars {
            break;
        }
        if owned_sections[i].len() > TOOL_RESULT_TRUNCATE_CHARS {
            let old_len = owned_sections[i].len();
            owned_sections[i] =
                truncate_tool_result(&owned_sections[i], TOOL_RESULT_TRUNCATE_CHARS);
            size = size.saturating_sub(old_len - owned_sections[i].len());
        }
    }

    if size <= max_chars {
        let mut result = prefix.to_string();
        result.push_str(&owned_sections.join("\n\n"));
        return result;
    }

    // Second: drop old tool results entirely (keep recent ones).
    // Protect the last quarter of sections.
    let protect_from = sections.len().saturating_sub(sections.len() / 4).max(1);
    let mut dropped = vec![false; sections.len()];

    for &i in &tool_result_indices {
        if size <= max_chars {
            break;
        }
        if i < protect_from {
            size = size.saturating_sub(owned_sections[i].len() + 2);
            dropped[i] = true;
        }
    }

    if size <= max_chars {
        let mut result = prefix.to_string();
        result.push_str("\n\n(Earlier tool results compacted.)\n\n");
        for (i, section) in owned_sections.iter().enumerate() {
            if !dropped[i] {
                result.push_str(section);
                result.push_str("\n\n");
            }
        }
        return result;
    }

    // Third: drop old non-tool sections too (keep recent quarter).
    for &i in &other_indices {
        if size <= max_chars {
            break;
        }
        if i < protect_from {
            size = size.saturating_sub(owned_sections[i].len() + 2);
            dropped[i] = true;
        }
    }

    let mut result = prefix.to_string();
    result.push_str("\n\n(Earlier conversation compacted.)\n\n");
    for (i, section) in owned_sections.iter().enumerate() {
        if !dropped[i] {
            result.push_str(section);
            result.push_str("\n\n");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
        }
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(4000), 1000);
        assert_eq!(estimate_tokens(0), 0);
    }

    #[test]
    fn test_conversation_char_size() {
        let msgs = vec![
            make_msg(MessageRole::System, "You are helpful."),
            make_msg(MessageRole::User, "Hello"),
        ];
        let size = conversation_char_size(&msgs);
        // 16 + 20 + 5 + 20 = 61
        assert_eq!(size, 61);
    }

    #[test]
    fn test_needs_compaction() {
        // 4096 tokens = 16384 chars, threshold = 12288
        let small_msgs = vec![
            make_msg(MessageRole::System, "sys"),
            make_msg(MessageRole::User, "hi"),
        ];
        assert!(!needs_compaction(&small_msgs, 4096));

        // Create messages that exceed 75% of 4096 tokens
        let big_content = "x".repeat(14000);
        let big_msgs = vec![
            make_msg(MessageRole::System, &big_content),
            make_msg(MessageRole::User, "hi"),
        ];
        assert!(needs_compaction(&big_msgs, 4096));
    }

    #[test]
    fn test_truncate_tool_result() {
        let short = "short result";
        assert_eq!(truncate_tool_result(short, 500), short);

        let long = "word ".repeat(200); // 1000 chars
        let truncated = truncate_tool_result(&long, 100);
        assert!(truncated.len() < long.len());
        assert!(truncated.contains("...(truncated,"));
    }

    #[test]
    fn test_prune_keeps_system_and_recent() {
        let mut msgs = vec![
            make_msg(MessageRole::System, "System prompt here"),
            make_msg(MessageRole::User, "Question 1"),
            make_msg(MessageRole::Assistant, "Answer 1 with tool call"),
            make_msg(
                MessageRole::User,
                &format!("Tool result for search:\n{}", "data ".repeat(2000)),
            ),
            make_msg(MessageRole::Assistant, "Based on that, here is more"),
            make_msg(MessageRole::User, "Question 2"),
            make_msg(MessageRole::Assistant, "Answer 2"),
            make_msg(MessageRole::User, "Old question 3"),
            make_msg(MessageRole::Assistant, "Old answer 3"),
            make_msg(MessageRole::User, "Old question 4"),
            make_msg(MessageRole::Assistant, "Old answer 4"),
            make_msg(MessageRole::User, "Recent question 5"),
            make_msg(MessageRole::Assistant, "Recent answer 5"),
            make_msg(MessageRole::User, "Recent question 6"),
            make_msg(MessageRole::Assistant, "Recent answer 6"),
        ];

        let original_size = conversation_char_size(&msgs);
        // Use 25% of original size to force aggressive pruning.
        prune_conversation(&mut msgs, original_size / 4);

        // System prompt should still be first.
        assert_eq!(msgs[0].role, MessageRole::System);
        // Conversation should be smaller.
        let new_size = conversation_char_size(&msgs);
        assert!(
            new_size < original_size,
            "Expected compaction: {original_size} -> {new_size}"
        );
        // Last message should still be present.
        assert_eq!(msgs.last().unwrap().content, "Recent answer 6");
    }

    #[test]
    fn test_prune_truncates_tool_results_first() {
        let big_result = "x".repeat(5000);
        let mut msgs = vec![
            make_msg(MessageRole::System, "sys"),
            make_msg(
                MessageRole::User,
                &format!("Tool result for fetch:\n{}", big_result),
            ),
            make_msg(MessageRole::User, "Recent question"),
            make_msg(MessageRole::Assistant, "Recent answer"),
        ];

        // Generous limit that should be achievable by truncating the tool result.
        prune_conversation(&mut msgs, 2000);

        // The tool result message should still exist but be truncated.
        let tool_msg = msgs
            .iter()
            .find(|m| m.content.contains("Tool result for fetch"));
        if let Some(m) = tool_msg {
            assert!(m.content.len() < big_result.len());
            assert!(m.content.contains("truncated"));
        }
        // If it was removed entirely, that's also fine -- the point is we're under limit.
        assert!(conversation_char_size(&msgs) <= 2000);
    }

    #[test]
    fn test_prune_empty_messages() {
        let mut msgs: Vec<ChatMessage> = vec![];
        prune_conversation(&mut msgs, 100);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_prune_no_op_when_under_limit() {
        let mut msgs = vec![
            make_msg(MessageRole::System, "sys"),
            make_msg(MessageRole::User, "hi"),
            make_msg(MessageRole::Assistant, "hello"),
        ];
        let original = msgs.clone();
        prune_conversation(&mut msgs, 100_000);
        assert_eq!(msgs.len(), original.len());
    }

    #[test]
    fn test_build_summary_prompt() {
        let msgs = vec![
            make_msg(MessageRole::User, "What is Rust?"),
            make_msg(MessageRole::Assistant, "Rust is a systems language."),
        ];
        let prompt = build_summary_prompt(&msgs);
        assert!(prompt.contains("User: What is Rust?"));
        assert!(prompt.contains("Assistant: Rust is a systems language."));
        assert!(prompt.contains("Concise summary:"));
    }

    #[test]
    fn test_summary_to_system_message() {
        let msg = summary_to_system_message("User asked about Rust. Assistant explained.");
        assert_eq!(msg.role, MessageRole::System);
        assert!(msg.content.contains("Conversation summary"));
    }

    #[test]
    fn test_prune_string_conversation_no_op() {
        let conv = "PREFIX\n\nUser: hi\n\nAssistant: hello";
        let result = prune_string_conversation(conv, 6, 100_000);
        assert_eq!(result, conv);
    }

    #[test]
    fn test_prune_string_conversation_drops_tool_results() {
        let big_result = "x".repeat(3000);
        let conv = format!(
            "SYSTEM PREFIX\n\nTool result for search:\n{}\n\nUser: recent question\n\nAssistant: recent answer",
            big_result
        );
        let result = prune_string_conversation(&conv, 13, 500);
        // Should not contain the huge tool result.
        assert!(result.len() < conv.len());
        assert!(result.contains("SYSTEM PREFIX"));
        assert!(result.contains("recent answer"));
    }

    #[test]
    fn test_is_tool_result_message() {
        assert!(is_tool_result_message(&make_msg(
            MessageRole::User,
            "Tool result for web_search:\nsome data"
        )));
        assert!(is_tool_result_message(&make_msg(
            MessageRole::User,
            "Tool 'web_fetch' failed: timeout"
        )));
        assert!(!is_tool_result_message(&make_msg(
            MessageRole::User,
            "What is the weather?"
        )));
        assert!(!is_tool_result_message(&make_msg(
            MessageRole::Assistant,
            "Tool result for web_search:\ndata"
        )));
    }
}
