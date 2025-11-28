//! Traced wrapper combinators for nom parsers
//!
//! These wrappers add tracing support to standard nom combinators,
//! allowing debug visibility into which parsers are called and their outcomes.

use nom::{error::ParseError, Parser};

use crate::trace::{is_tracing_enabled, trace_enter, trace_exit_failure, trace_exit_success};

/// A wrapper that adds tracing to any parser
pub struct TracedParser<F, N> {
    parser: F,
    name: N,
}

impl<I, O, E, F, N> Parser<I> for TracedParser<F, N>
where
    I: Clone + AsRef<str>,
    F: Parser<I, Output = O, Error = E>,
    E: ParseError<I>,
    N: AsRef<str>,
{
    type Output = O;
    type Error = E;

    fn process<OM: nom::OutputMode>(
        &mut self,
        input: I,
    ) -> nom::PResult<OM, I, Self::Output, Self::Error> {
        if !is_tracing_enabled() {
            return self.parser.process::<OM>(input);
        }

        let input_str = input.as_ref();
        trace_enter(self.name.as_ref(), input_str);

        match self.parser.process::<OM>(input.clone()) {
            Ok((remaining, output)) => {
                let consumed = input_str.len() - remaining.as_ref().len();
                trace_exit_success(consumed, "ok");
                Ok((remaining, output))
            }
            Err(e) => {
                trace_exit_failure("parse error");
                Err(e)
            }
        }
    }
}

/// Wrap a parser with tracing
///
/// # Example
/// ```ignore
/// use ingredient::parser::traced::traced;
///
/// let parser = traced("my_parser", tag("hello"));
/// ```
pub fn traced<I, O, E, F, N>(name: N, parser: F) -> TracedParser<F, N>
where
    F: Parser<I, Output = O, Error = E>,
    N: AsRef<str>,
{
    TracedParser { parser, name }
}
