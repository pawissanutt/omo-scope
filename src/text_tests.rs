use super::*;

#[test]
fn sanitize_strips_escapes_and_controls() {
    let input = format!(
        "{e}[31mred{e}[0m\tx\r\n{e}]0;title{b}ok{n}",
        e = '\u{1b}',
        b = '\u{7}',
        n = '\u{0}'
    );
    assert_eq!(sanitize(&input), "red    x\nok");
}

#[test]
fn wrap_prefers_spaces_and_hard_splits_long_words() {
    assert_eq!(wrap("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
    assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap("aa bbbbb", 4), ["aa", "bbbb", "b"]);
    assert_eq!(wrap("", 4), [""]);
    assert_eq!(wrap("a\nb", 4), ["a", "b"]);
}

#[test]
fn clip_marks_truncation() {
    assert_eq!(clip("hello", 10), "hello");
    assert_eq!(clip("hello world", 6), "hello…");
    assert_eq!(clip("hello", 0), "");
}

#[test]
fn durations_are_compact() {
    assert_eq!(fmt_duration(5), "5s");
    assert_eq!(fmt_duration(65), "1m05s");
    assert_eq!(fmt_duration(3720), "1h02m");
}
