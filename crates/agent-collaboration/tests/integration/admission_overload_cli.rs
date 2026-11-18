//! A request the API sheds at its request limit was not run. The CLI reports it as an
//! `overloaded` failure to retry later: a board command in the board's typed refusal, and an
//! untyped command as the retryable overload, never as a connection failure or protocol
//! violation.
