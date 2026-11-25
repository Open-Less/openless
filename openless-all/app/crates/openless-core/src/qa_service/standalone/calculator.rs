use num_bigint::BigInt;
use num_rational::BigRational as Number;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde_json::json;

pub(super) const MAX_EXPRESSION_BYTES: usize = 512;
const MAX_BITS: u64 = 4096;
const MAX_DEPTH: u8 = 32;
const MAX_POWER: i32 = 1024;
const DECIMAL_PLACES: usize = 50;

type Result<T> = std::result::Result<T, &'static str>;

pub(crate) fn calculate(expression: &str) -> String {
    match evaluate(expression) {
        Ok(value) => {
            let (decimal, rounded) = decimal(&value);
            json!({
                "exact": if value.is_integer() { value.to_integer().to_string() } else {
                    format!("{}/{}", value.numer(), value.denom())
                },
                "decimal": decimal,
                "rounded": rounded,
                "maxDecimalPlaces": DECIMAL_PLACES,
            })
        }
        Err(error) => json!({"error": error}),
    }
    .to_string()
}

fn evaluate(expression: &str) -> Result<Number> {
    if expression.len() > MAX_EXPRESSION_BYTES {
        return Err("expression_limit");
    }
    let mut parser = Parser(expression.as_bytes());
    let value = parser.sum(0)?;
    if parser.peek().is_some() {
        return Err("invalid_expression");
    }
    Ok(value)
}

fn bounded(value: Number) -> Result<Number> {
    if value.numer().bits().max(value.denom().bits()) > MAX_BITS {
        return Err("number_limit");
    }
    Ok(value)
}

fn decimal(value: &Number) -> (String, bool) {
    let scaled = value.numer().abs() * BigInt::from(10u8).pow(DECIMAL_PLACES as u32);
    let mut digits = &scaled / value.denom();
    let remainder = scaled % value.denom();
    if &remainder * 2u8 >= *value.denom() {
        digits += 1u8;
    }
    let sign = if value.is_negative() && !digits.is_zero() {
        "-"
    } else {
        ""
    };
    let digits = format!("{:0>width$}", digits, width = DECIMAL_PLACES + 1);
    let (whole, fractional) = digits.split_at(digits.len() - DECIMAL_PLACES);
    let fractional = fractional.trim_end_matches('0');
    let decimal = if fractional.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fractional}")
    };
    (decimal, !remainder.is_zero())
}

struct Parser<'a>(&'a [u8]);

impl<'a> Parser<'a> {
    fn peek(&mut self) -> Option<u8> {
        while self.0.first().is_some_and(u8::is_ascii_whitespace) {
            self.0 = &self.0[1..];
        }
        self.0.first().copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() != Some(byte) {
            return false;
        }
        self.0 = &self.0[1..];
        true
    }

    fn sum(&mut self, depth: u8) -> Result<Number> {
        let mut lhs = self.product(depth)?;
        loop {
            if self.eat(b'+') {
                lhs = bounded(lhs + self.product(depth)?)?;
            } else if self.eat(b'-') {
                lhs = bounded(lhs - self.product(depth)?)?;
            } else {
                return Ok(lhs);
            }
        }
    }

    fn product(&mut self, depth: u8) -> Result<Number> {
        let mut lhs = self.factor(depth)?;
        loop {
            if self.eat(b'*') {
                lhs = bounded(lhs * self.factor(depth)?)?;
            } else if self.eat(b'/') {
                let rhs = self.factor(depth)?;
                if rhs.is_zero() {
                    return Err("division_by_zero");
                }
                lhs = bounded(lhs / rhs)?;
            } else {
                return Ok(lhs);
            }
        }
    }

    fn factor(&mut self, depth: u8) -> Result<Number> {
        if depth >= MAX_DEPTH {
            return Err("expression_limit");
        }
        if self.eat(b'-') {
            return Ok(-self.factor(depth + 1)?);
        }
        if self.eat(b'+') {
            return self.factor(depth + 1);
        }
        let mut lhs = if self.eat(b'(') {
            let value = self.sum(depth + 1)?;
            if !self.eat(b')') {
                return Err("invalid_expression");
            }
            value
        } else {
            self.number()?
        };
        if self.eat(b'^') {
            let power = self.factor(depth + 1)?;
            if !power.is_integer() {
                return Err("non_integer_power");
            }
            let power = power.to_integer().to_i32().ok_or("number_limit")?;
            let exponent = power.unsigned_abs();
            if exponent > MAX_POWER as u32 {
                return Err("number_limit");
            }
            if lhs.is_zero() && power <= 0 {
                return Err(if power == 0 {
                    "undefined_power"
                } else {
                    "division_by_zero"
                });
            }
            if power < 0 {
                lhs = lhs.recip();
            }
            // Reject oversized intermediates before exponentiating.
            let mut result = Number::one();
            let mut remaining = exponent;
            while remaining > 0 {
                if remaining & 1 == 1 {
                    result = bounded(result * &lhs)?;
                }
                remaining >>= 1;
                if remaining > 0 {
                    lhs = bounded(&lhs * &lhs)?;
                }
            }
            return Ok(result);
        }
        Ok(lhs)
    }

    fn digits(&mut self) -> &'a [u8] {
        let count = self
            .0
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        let (digits, rest) = self.0.split_at(count);
        self.0 = rest;
        digits
    }

    fn number(&mut self) -> Result<Number> {
        self.peek();
        let whole = self.digits();
        let fraction = if self.0.first() == Some(&b'.') {
            self.0 = &self.0[1..];
            self.digits()
        } else {
            &[]
        };
        let digits = [whole, fraction].concat();
        let numerator = BigInt::parse_bytes(&digits, 10).ok_or("invalid_expression")?;
        let mut exponent = 0i32;
        if matches!(self.0.first(), Some(b'e' | b'E')) {
            self.0 = &self.0[1..];
            let negative = self.0.first() == Some(&b'-');
            if matches!(self.0.first(), Some(b'+' | b'-')) {
                self.0 = &self.0[1..];
            }
            let exponent_digits = self.digits();
            if exponent_digits.is_empty() {
                return Err("invalid_expression");
            }
            for &digit in exponent_digits {
                exponent = exponent * 10 + i32::from(digit - b'0');
                if exponent > MAX_POWER {
                    return Err("number_limit");
                }
            }
            if negative {
                exponent = -exponent;
            }
        }
        let scale = exponent - fraction.len() as i32;
        let ten = BigInt::from(10u8).pow(scale.unsigned_abs());
        bounded(if scale < 0 {
            Number::new(numerator, ten)
        } else {
            Number::from_integer(numerator * ten)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn answer(expression: &str) -> Value {
        serde_json::from_str(&calculate(expression)).unwrap()
    }

    #[test]
    fn exact_decimal_arithmetic_and_precedence() {
        assert_eq!(answer("0.1 + 0.2")["decimal"], "0.3");
        assert_eq!(
            answer("9007199254740993 + 10^-2")["decimal"],
            "9007199254740993.01"
        );
        assert_eq!(answer("-2^2 + 2^3^2 - (.5 + 2e-1)*10")["exact"], "501");
        assert_eq!(answer("1/3")["exact"], "1/3");
        assert_eq!(answer("1/3")["decimal"], format!("0.{}", "3".repeat(50)));
        assert_eq!(answer("-2/3")["decimal"], format!("-0.{}7", "6".repeat(49)));
        assert_eq!(answer("1/3")["rounded"], true);
    }

    #[test]
    fn invalid_or_excessive_input_returns_a_bounded_error() {
        for (expression, error) in [
            ("1/0", "division_by_zero"),
            ("0^-1", "division_by_zero"),
            ("0^0", "undefined_power"),
            ("3^.5", "non_integer_power"),
            ("1e2000000", "number_limit"),
            ("(2^1024)^1024", "number_limit"),
            ("1; print(2)", "invalid_expression"),
            ("1e -2", "invalid_expression"),
        ] {
            assert_eq!(answer(expression)["error"], error, "{expression}");
        }
        assert_eq!(answer(&"-".repeat(100))["error"], "expression_limit");
        assert_eq!(answer(&"1".repeat(513))["error"], "expression_limit");
    }
}
