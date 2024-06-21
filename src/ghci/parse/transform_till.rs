use winnow::combinator::eof;
use winnow::combinator::terminated;
use winnow::error::ErrMode;
use winnow::error::ErrorKind;
use winnow::error::ParserError;
use winnow::stream::Offset;
use winnow::stream::Stream;
use winnow::stream::StreamIsPartial;
use winnow::Parser;

/// Call the `repeat` parser until the `end` parser produces a result.
///
/// Then, return the input consumed until the `end` parser was called, and the result of the `end`
/// parser.
///
/// See: <https://github.com/winnow-rs/winnow/pull/541>
pub fn recognize_till<I, Discard, O, E>(
    mut repeat: impl Parser<I, Discard, E>,
    mut end: impl Parser<I, O, E>,
) -> impl Parser<I, (<I as Stream>::Slice, O), E>
where
    I: Stream,
    E: ParserError<I>,
{
    move |input: &mut I| {
        let start = input.checkpoint();

        loop {
            let before_end = input.checkpoint();
            match end.parse_next(input) {
                Ok(end_parsed) => {
                    let after_end = input.checkpoint();

                    let offset_to_before_end = before_end.offset_from(&start);
                    input.reset(start);
                    let input_until_end = input.next_slice(offset_to_before_end);
                    input.reset(after_end);

                    return Ok((input_until_end, end_parsed));
                }
                Err(ErrMode::Backtrack(_)) => {
                    input.reset(before_end);
                    match repeat.parse_next(input) {
                        Ok(_) => {}
                        Err(e) => return Err(e.append(input, ErrorKind::Many)),
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Like [`recognize_till`], but it also applies a `transform` parser to the recognized input.
pub fn transform_till<I, O1, O2, Discard, E>(
    mut repeat: impl Parser<I, Discard, E>,
    mut transform: impl Parser<<I as Stream>::Slice, O1, E>,
    mut end: impl Parser<I, O2, E>,
) -> impl Parser<I, (O1, O2), E>
where
    I: Stream,
    E: ParserError<I>,
    E: ParserError<<I as Stream>::Slice>,
    <I as Stream>::Slice: Stream + StreamIsPartial,
{
    move |input: &mut I| {
        let (mut until_end, end_parsed) =
            recognize_till(repeat.by_ref(), end.by_ref()).parse_next(input)?;

        let inner_parsed = terminated(transform.by_ref(), eof)
            .parse_next(&mut until_end)
            .map_err(|err_mode| match err_mode {
                ErrMode::Incomplete(_) => {
                    panic!("complete parsers should not report `ErrMode::Incomplete(_)`")
                }
                ErrMode::Backtrack(inner) | ErrMode::Cut(inner) => ErrMode::Cut(inner),
            })?;

        Ok((inner_parsed, end_parsed))
    }
}
