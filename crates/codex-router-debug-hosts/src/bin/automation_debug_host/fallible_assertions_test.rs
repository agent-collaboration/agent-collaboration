//! Assertion conditions for tests whose fixture failures return through Result.
macro_rules! ensure_test {
    ($condition:expr $(,)?) => {
        if !$condition {
            return Err(concat!("assertion failed: ", stringify!($condition)).into());
        }
    };
    ($condition:expr, $($message:tt)+) => {
        if !$condition {
            return Err(format!($($message)+).into());
        }
    };
}

macro_rules! ensure_test_eq {
    ($left:expr, $right:expr $(,)?) => {
        match (&$left, &$right) {
            (left, right) if *left != *right => {
                return Err(format!(
                    "assertion `left == right` failed\n  left: {left:?}\n right: {right:?}"
                )
                .into());
            }
            _ => {}
        }
    };
}

macro_rules! ensure_test_ne {
    ($left:expr, $right:expr $(,)?) => {
        match (&$left, &$right) {
            (left, right) if *left == *right => {
                return Err(format!(
                    "assertion `left != right` failed\n  left: {left:?}\n right: {right:?}"
                )
                .into());
            }
            _ => {}
        }
    };
}
