#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn paths_are_quoted_for_the_shell() {
    assert_eq!(shell_quoted("/home/a/shop"), "/home/a/shop");
    assert_eq!(shell_quoted("/home/a/my shop"), "'/home/a/my shop'");
    assert_eq!(shell_quoted("/it's"), "'/it'\\''s'");
}
