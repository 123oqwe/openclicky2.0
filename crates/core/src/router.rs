//! Model routing: fast vs deep, no user-facing model picker (SRC-PRD B6).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Deep,
}

#[derive(Debug, Clone, Copy)]
pub struct RouteInput<'a> {
    pub text: &'a str,
    pub has_screen: bool,
    pub is_task: bool,
}

const DEEP_KEYWORDS: &[&str] = &[
    "为什么",
    "原理",
    "分析",
    "比较",
    "设计",
    "写一段",
    "检查",
    "排查",
    "debug",
    "explain why",
    "how does",
    "architecture",
];
const DEICTIC: &[&str] = &["这个", "这里", "这一步", "在哪", "this", "here"];
const GREETING: &[&str] = &["hi", "hello", "hey", "你好", "嗨", "谢谢", "thanks"];

/// Imperative openings: the user is asking for work, not for an explanation.
const TASK_PREFIXES: &[&str] = &[
    "帮我",
    "替我",
    "给我",
    "帮我把",
    "把",
    "整理",
    "清理",
    "汇总",
    "安排",
    "生成",
    "发一封",
    "写一封",
    "查一下",
    "查查",
    "列一个",
    "做一份",
    "做个",
];

/// Anything that turns a sentence into a question. A question is never a task,
/// even when it opens with an imperative ("帮我看看这个为什么报错").
const QUESTION_MARKERS: &[&str] = &["吗", "？", "?", "怎么", "为什么", "为何"];

/// True when the utterance reads as a to-do: an imperative opening with no
/// question anywhere in it. Used to decide whether the text goes to the task
/// runner instead of the answer path.
pub fn looks_like_task(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if QUESTION_MARKERS.iter().any(|m| t.contains(m)) {
        return false;
    }
    let lowered = t.to_lowercase();
    if TASK_PREFIXES.iter().any(|p| lowered.starts_with(p)) {
        return true;
    }
    // 「给…做/发/整理」 is an imperative about someone else's stuff.
    if lowered.starts_with('给')
        && (lowered.contains('做') || lowered.contains("整理") || lowered.contains("发"))
    {
        return true;
    }
    false
}

pub fn route(input: RouteInput<'_>) -> Tier {
    if input.is_task {
        return Tier::Deep;
    }
    let text = input.text.trim().to_lowercase();
    let chars = text.chars().count();
    if chars > 120 {
        return Tier::Deep;
    }
    if DEEP_KEYWORDS.iter().any(|k| text.contains(k)) {
        return Tier::Deep;
    }
    if input.has_screen && DEICTIC.iter().any(|k| text.contains(k)) {
        return Tier::Fast;
    }
    if chars <= 12 && GREETING.iter().any(|k| text.starts_with(k)) {
        return Tier::Fast;
    }
    Tier::Fast
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(t: &str, screen: bool, task: bool) -> Tier {
        route(RouteInput {
            text: t,
            has_screen: screen,
            is_task: task,
        })
    }

    #[test]
    fn tasks_are_always_deep() {
        assert_eq!(r("hi", false, true), Tier::Deep);
    }

    #[test]
    fn deictic_with_screen_is_fast() {
        assert_eq!(r("这个按钮是干什么的", true, false), Tier::Fast);
    }

    #[test]
    fn explicative_questions_are_deep() {
        assert_eq!(r("为什么这里会报错", true, false), Tier::Deep);
    }

    #[test]
    fn long_text_is_deep() {
        let long = "a".repeat(130);
        assert_eq!(r(&long, false, false), Tier::Deep);
    }

    #[test]
    fn short_greeting_is_fast() {
        assert_eq!(r("hi", false, false), Tier::Fast);
    }

    #[test]
    fn english_debug_keyword_is_deep() {
        assert_eq!(r("please debug this", true, false), Tier::Deep);
    }

    #[test]
    fn imperative_about_a_file_is_a_task() {
        assert!(looks_like_task("帮我把这两周的邮件整理成一个表格"));
    }

    #[test]
    fn imperative_about_sending_is_a_task() {
        assert!(looks_like_task("帮我发一封邮件给客户"));
        assert!(looks_like_task("发一封道歉信给王工"));
    }

    #[test]
    fn lookup_imperative_is_a_task() {
        assert!(looks_like_task("查一下今天的汇率"));
        assert!(looks_like_task("给我做一份周报"));
        assert!(looks_like_task("给客户发一封报价单"));
    }

    #[test]
    fn a_question_mark_makes_it_not_a_task() {
        assert!(!looks_like_task("帮我把这个删掉，好吗？"));
        assert!(!looks_like_task("帮我整理一下这个文件夹?"));
    }

    #[test]
    fn why_questions_are_not_tasks() {
        assert!(!looks_like_task("为什么这里会报错"));
        assert!(!looks_like_task("帮我看看这里为什么报错"));
        assert!(!looks_like_task("怎么整理这个文件夹"));
    }

    #[test]
    fn plain_lookups_are_not_tasks() {
        assert!(!looks_like_task("这个按钮是干什么的"));
        assert!(!looks_like_task(""));
        assert!(!looks_like_task("hi"));
    }
}
