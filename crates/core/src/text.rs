//! Text that is about to become keystrokes. The one rule here is that nothing
//! enters a terminal by typing.

use crate::ports::TargetKind;

/// Terminal safety: never inject anything that could run as a command.
pub fn terminal_safe(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Conservative cleanup: fix obvious spacing, never add facts or dashes.
pub fn cleanup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\u{2014}' || c == '\u{2013}' {
            out.push(' ');
            continue;
        }
        out.push(c);
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return collapsed;
    }
    let mut chars: Vec<char> = collapsed.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = first.to_uppercase().next().unwrap_or(*first);
    }
    chars.into_iter().collect()
}

/// How a piece of text should reach the focused application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Injection {
    /// Keystroke by keystroke. Fine for an editor, wrong for a shell.
    Type(String),
    /// Clipboard plus a paste keystroke, with newlines flattened.
    Paste(String),
    /// Clipboard only. Used when the answer is a fallback nobody asked to type.
    CopyOnly(String),
}

/// The decision, separated from the injection itself so it can be checked
/// without a keyboard. A terminal never receives typed text, and text that is
/// not an intentional dictation is never typed anywhere.
pub fn plan_injection(text: &str, kind: TargetKind, deliberate: bool) -> Injection {
    if kind == TargetKind::Terminal {
        return Injection::Paste(terminal_safe(text));
    }
    if !deliberate {
        return Injection::CopyOnly(text.to_string());
    }
    Injection::Type(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_safety_flattens_newlines() {
        assert_eq!(terminal_safe("rm -rf /\n echo hi"), "rm -rf / echo hi");
        assert_eq!(terminal_safe("a\r\nb"), "a b");
    }

    #[test]
    fn cleanup_removes_dashes_and_capitalises() {
        assert_eq!(cleanup("hello — world"), "Hello world");
    }

    #[test]
    fn a_terminal_never_receives_typed_text() {
        let plan = plan_injection("rm -rf /\nnow", TargetKind::Terminal, true);
        assert_eq!(plan, Injection::Paste("rm -rf / now".to_string()));
    }

    #[test]
    fn an_editor_receives_keystrokes_when_the_text_was_asked_for() {
        assert_eq!(
            plan_injection("买牛奶", TargetKind::Editor, true),
            Injection::Type("买牛奶".to_string())
        );
    }

    #[test]
    fn a_fallback_answer_is_copied_and_never_typed() {
        assert_eq!(
            plan_injection("回答\n换行", TargetKind::Other, false),
            Injection::CopyOnly("回答\n换行".to_string())
        );
    }
}
