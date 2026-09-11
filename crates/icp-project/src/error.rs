//! Reading an error's `source()` chain, for the places that have to hand an
//! error to something that carries no chain of its own.

/// The rendered `source()` chain of an error, outermost cause first. The
/// error's own message is not included.
pub fn causes(error: &dyn std::error::Error) -> Vec<String> {
    let mut causes = Vec::new();
    let mut cause = error.source();
    while let Some(err) = cause {
        causes.push(err.to_string());
        cause = err.source();
    }
    causes
}

/// An error and its causes rendered as one `: `-separated string, for a
/// boundary that takes a single message — a wasm guest's `result<_, string>`,
/// say. A bare `to_string()` there drops everything the chain holds, which for
/// the error types whose message names only the action attempted is all of the
/// reason.
pub fn flatten(error: &dyn std::error::Error) -> String {
    let mut rendered = error.to_string();
    for cause in causes(error) {
        rendered.push_str(": ");
        rendered.push_str(&cause);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use snafu::prelude::*;

    #[derive(Debug, Snafu)]
    #[snafu(display("the innermost thing went wrong"))]
    struct Inner;

    #[derive(Debug, Snafu)]
    #[snafu(display("the outer action failed"))]
    struct Outer {
        source: Inner,
    }

    fn outer() -> Outer {
        Err::<(), _>(Inner).context(OuterSnafu).unwrap_err()
    }

    #[test]
    fn causes_omit_the_error_itself() {
        assert_eq!(super::causes(&outer()), ["the innermost thing went wrong"]);
    }

    #[test]
    fn flatten_joins_the_whole_chain() {
        assert_eq!(
            super::flatten(&outer()),
            "the outer action failed: the innermost thing went wrong"
        );
    }

    #[test]
    fn flatten_of_a_lone_error_is_its_message() {
        assert_eq!(super::flatten(&Inner), "the innermost thing went wrong");
    }
}
