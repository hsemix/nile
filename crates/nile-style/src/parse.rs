//! CSS parsing on top of `cssparser`'s tokenizer.

use crate::selector::{NileImpl, NileParser};
use crate::Color;
use cssparser::{Delimiter, ParseError, Parser, Token};
use selectors::parser::{ParseRelative, SelectorList};

type PResult<T> = Result<T, ParseError<()>>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f32),
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Ident(String),
    Str(String),
    Color(Color),
}

impl Value {
    pub fn ident(&self) -> Option<&str> {
        match self {
            Value::Ident(s) => Some(s),
            _ => None,
        }
    }

    pub fn color(&self) -> Option<Color> {
        match self {
            Value::Color(c) => Some(*c),
            Value::Ident(name) => named_color(name),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub values: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub selectors: SelectorList<NileImpl>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

impl Stylesheet {
    pub fn parse(css: &str) -> Stylesheet {
        let mut parser = Parser::new(css);
        let mut rules = Vec::new();

        while !parser.is_exhausted() {
            match parse_rule(&mut parser) {
                Ok(Some(rule)) => rules.push(rule),
                Ok(None) => {}
                // Skip the broken rule and keep going, as browsers do.
                Err(_) => {
                    let _ = parser.parse_until_after(Delimiter::CurlyBracketBlock, |p| {
                        while p.next().is_ok() {}
                        Ok::<_, ParseError<()>>(())
                    });
                }
            }
        }
        Stylesheet { rules }
    }
}

/// Parses `selector, selector { decls }`. Returns `Ok(None)` for rules we
/// recognise but don't support (unsupported selectors, at-rules).
fn parse_rule<'i>(p: &mut Parser<'i>) -> PResult<Option<Rule>> {
    let selectors = p.parse_until_before(Delimiter::CurlyBracketBlock | Delimiter::Semicolon, |p| {
        SelectorList::parse(&NileParser, p, ParseRelative::No).map_err(|_| ParseError::<()>::custom(()))
    });
    match p.next()? {
        Token::CurlyBracketBlock => {
            let declarations = p.parse_nested_block(|p| Ok(declarations_in(p)))?;
            // A failed selector list drops the rule (its block is skipped above).
            Ok(selectors.ok().map(|selectors| Rule { selectors, declarations }))
        }
        // `@import ...;` and friends.
        _ => Ok(None),
    }
}

/// Parses the contents of a `style="..."` attribute.
pub fn parse_declarations(css: &str) -> Vec<Declaration> {
    let mut parser = Parser::new(css);
    declarations_in(&mut parser)
}

fn declarations_in(p: &mut Parser<'_>) -> Vec<Declaration> {
    let mut out = Vec::new();
    while !p.is_exhausted() {
        let result = p.parse_until_after(Delimiter::Semicolon, |p| {
            let name = p.expect_ident()?.to_ascii_lowercase();
            p.expect_colon()?;
            let mut values = Vec::new();
            while !p.is_exhausted() {
                if let Some(v) = parse_value(p)? {
                    values.push(v);
                }
            }
            Ok::<_, ParseError<()>>(Declaration { name, values })
        });
        if let Ok(decl) = result {
            out.push(decl);
        }
    }
    out
}

fn parse_value<'i>(p: &mut Parser<'i>) -> PResult<Option<Value>> {
    let token = p.next()?.clone();
    Ok(Some(match token {
        Token::Number { value, .. } => Value::Number(value),
        Token::Percentage { unit_value, .. } => Value::Percent(unit_value * 100.0),
        Token::Dimension { value, ref unit, .. } => match unit.to_ascii_lowercase().as_str() {
            "px" => Value::Px(value),
            "em" => Value::Em(value),
            "rem" => Value::Rem(value),
            "pt" => Value::Px(value * 4.0 / 3.0),
            _ => return Ok(None),
        },
        Token::Ident(ref s) => Value::Ident(s.to_ascii_lowercase()),
        Token::QuotedString(ref s) => Value::Str(s.to_string()),
        Token::Hash(ref h) | Token::IDHash(ref h) => match hex_color(h) {
            Some(c) => Value::Color(c),
            None => return Ok(None),
        },
        Token::Function(ref name) => {
            let name = name.to_ascii_lowercase();
            let args = p.parse_nested_block(|p| {
                let mut args = Vec::new();
                while !p.is_exhausted() {
                    match p.next()? {
                        Token::Number { value, .. } => args.push(*value),
                        Token::Percentage { unit_value, .. } => args.push(*unit_value * 255.0),
                        _ => {}
                    }
                }
                Ok::<_, ParseError<()>>(args)
            })?;
            match (name.as_str(), args.as_slice()) {
                ("rgb" | "rgba", [r, g, b, rest @ ..]) => {
                    let a = rest.first().map_or(255.0, |a| if *a <= 1.0 { a * 255.0 } else { *a });
                    Value::Color(Color { r: *r as u8, g: *g as u8, b: *b as u8, a: a as u8 })
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    }))
}

fn hex_color(h: &str) -> Option<Color> {
    let digit = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok();
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        3 | 4 => {
            let r = digit(0)? * 17;
            let g = digit(1)? * 17;
            let b = digit(2)? * 17;
            let a = if h.len() == 4 { digit(3)? * 17 } else { 255 };
            Some(Color { r, g, b, a })
        }
        6 | 8 => {
            let a = if h.len() == 8 { byte(6)? } else { 255 };
            Some(Color { r: byte(0)?, g: byte(2)?, b: byte(4)?, a })
        }
        _ => None,
    }
}

fn named_color(name: &str) -> Option<Color> {
    Some(match name {
        "black" => Color::rgb(0, 0, 0),
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "blue" => Color::rgb(0, 0, 255),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        "orange" => Color::rgb(255, 165, 0),
        "yellow" => Color::rgb(255, 255, 0),
        "purple" => Color::rgb(128, 0, 128),
        "transparent" => Color::TRANSPARENT,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules_and_skips_unsupported() {
        let sheet = Stylesheet::parse(
            "h1, .title { color: #333; margin: 0 auto } div > p:first-child { color: red } \
             a:hover { color: blue } p::before { color: red } @import url(x.css); \
             li:unknown { color: red } #x.y[data-a=b] { gap: 4px }",
        );
        assert_eq!(sheet.rules.len(), 4);
        assert_eq!(sheet.rules[0].selectors.slice().len(), 2);
        assert_eq!(sheet.rules[0].declarations[1].values.len(), 2);
        assert_eq!(sheet.rules[3].declarations[0].name, "gap");
    }

    #[test]
    fn parses_colors() {
        let d = parse_declarations("color: rgba(10, 20, 30, 0.5); background: #fff");
        assert_eq!(d[0].values[0], Value::Color(Color { r: 10, g: 20, b: 30, a: 127 }));
        assert_eq!(d[1].values[0].color(), Some(Color::rgb(255, 255, 255)));
    }
}
