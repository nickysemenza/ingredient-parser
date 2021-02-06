extern crate nom;

fn hello_parser(i: &str) -> nom::IResult<&str, &str> {
    nom::bytes::complete::tag("hello")(i)
}

fn main() {
    println!("{:?}", hello_parser("hello"));
    println!("{:?}", hello_parser("hello world"));
    println!("{:?}", hello_parser("goodbye hello again"));
}

pub(self) mod parsers {
    use nom::{
        bytes::complete::{is_not, tag},
        character::complete::char,
        character::complete::{alpha1, digit1, one_of},
        error::context,
        multi::{count, many_m_n},
        number::complete::float,
        sequence::{delimited, separated_pair, terminated, tuple},
        IResult,
    };

    extern crate nom;

    // use super::Mount;

    fn not_whitespace(i: &str) -> nom::IResult<&str, &str> {
        nom::bytes::complete::is_not(" \t")(i)
    }
    // fn is_number(i: &str) -> nom::IResult<&str, &u64> {
    //     nom::number::complete::be_u64(i)
    // }

    // https://docs.rs/nom/6.1.0/nom/#parser-combinators
    fn parens(input: &str) -> IResult<&str, &str> {
        delimited(char('('), is_not(")"), char(')'))(input)
    }

    fn num(input: &str) -> IResult<&str, f32> {
        float(input)
    }

    fn ip_num(input: &str) -> nom::IResult<&str, u8> {
        context("ip number", n_to_m_digits(1, 3))(input).and_then(|(next_input, result)| {
            match result.parse::<u8>() {
                Ok(n) => Ok((next_input, n)),
                Err(_) => Err(nom::Err::Error(nom::error::Error::new(
                    " abcdefg",
                    nom::error::ErrorKind::IsNot,
                ))),
            }
        })
    }

    // fn num(input: &str) -> nom::IResult<&str, u8> {
    //     context("ip number", n_to_m_digits(1, 3))(input).and_then(|(next_input, result)| {
    //         match result.parse::<u8>() {
    //             Ok(n) => Ok((next_input, n)),
    //             Err(_) => Err(nom::Err::Error(nom::error::Error::new(" abcdefg",nom::error::ErrorKind::IsNot))),
    //         }
    //     })
    // }

    fn n_to_m_digits<'a>(n: usize, m: usize) -> impl FnMut(&'a str) -> nom::IResult<&str, String> {
        move |input| {
            many_m_n(n, m, one_of("0123456789"))(input)
                .map(|(next_input, result)| (next_input, result.into_iter().collect()))
        }
    }

    fn ip(input: &str) -> nom::IResult<&str, [u8; 4]> {
        context(
            "ip",
            tuple((count(terminated(ip_num, tag(".")), 3), ip_num)),
        )(input)
        .map(|(next_input, res)| {
            let mut result: [u8; 4] = [0, 0, 0, 0];
            res.0
                .into_iter()
                .enumerate()
                .for_each(|(i, v)| result[i] = v);
            result[3] = res.1;
            (next_input, result)
        })
    }
    // fn foo(input: &str) -> IResult<&str, &str> {
    // nom::sequence::separated_pair(num(str))
    // }

    #[cfg(test)]
    mod tests {
        use nom::{branch::alt, character::complete::space0, error::ErrorKind};

        use super::*;

        #[test]
        fn test_not_whitespace() {
            let mut num_unit = tuple((num, space0, alpha1));

            let mut a = separated_pair(
                tuple((num, space0, alpha1)),
                alt((tag("; "), tag(" / "))),
                tuple((num, space0, alpha1)),
            );

            assert_eq!(
                num_unit("foo bar"),
                Err(nom::Err::Error(nom::error::Error::new(
                    "foo bar",
                    nom::error::ErrorKind::Float
                )))
            );
            assert_eq!(
                a("1.2 g; 2.3g"),
                Ok(("", ((1.2, " ", "g"), (2.3, "", "g"))))
            );
            assert_eq!(a("1.2 g; 2.3g"), a("1.2 g / 2.3g"));
            assert_eq!(num_unit("1.2 g foo"), Ok((" foo", (1.2, " ", "g"))));
            assert_eq!(num_unit("1.2g foo"), Ok((" foo", (1.2, "", "g"))));
            assert_eq!(
                num_unit("1.2g foo (bar)"),
                Ok((" foo (bar)", (1.2, "", "g")))
            );
            assert_eq!(num("1.2bar"), Ok(("bar", 1.2)));
            assert_eq!(num("2bar"), Ok(("bar", 2.0)));
            assert_eq!(parens("(foo)bar"), Ok(("bar", "foo")));
            assert_eq!(parens("(23 grams)bar"), Ok(("bar", "23 grams")));
            assert_eq!(not_whitespace("abcd efg"), Ok((" efg", "abcd")));
            assert_eq!(not_whitespace("abcd\tefg"), Ok(("\tefg", "abcd")));
            assert_eq!(
                not_whitespace(" abcdefg"),
                Err(nom::Err::Error(nom::error::Error::new(
                    " abcdefg",
                    nom::error::ErrorKind::IsNot
                )))
            );
        }
    }
}
