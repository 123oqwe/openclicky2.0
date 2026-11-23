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
}
