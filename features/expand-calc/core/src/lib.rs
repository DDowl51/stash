//! `expand-calc` 的纯逻辑。
//!
//! 文字里以 `=` 开头、中间不带空白的一段是算式：`买 3 个 =3*45` → `买 3 个 135`。
//! 支持 `+ - * / % ^`、`×`、`÷`、括号、小数、负号。算不出来的（不是算式、除以 0）原样留着：
//! `=` 开头不一定是想算东西。

/// 求值。不是合法算式、结果不是有限数时返回 `None`。
pub fn eval(expr: &str) -> Option<f64> {
    let chars: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser { s: &chars, i: 0 };
    let v = p.expr()?;
    (p.i == chars.len() && v.is_finite()).then_some(v)
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.i += 1;
        }
        hit
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat('*') || self.eat('×') {
                v *= self.power()?;
            } else if self.eat('/') || self.eat('÷') {
                let d = self.power()?;
                if d == 0.0 {
                    return None;
                }
                v /= d;
            } else if self.eat('%') {
                let d = self.power()?;
                if d == 0.0 {
                    return None;
                }
                v %= d;
            } else {
                return Some(v);
            }
        }
    }

    /// `^` 右结合，比负号优先：`-2^2` 是 -4。
    fn power(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.power()?);
        }
        if self.eat('+') {
            return self.power();
        }
        let base = self.primary()?;
        if self.eat('^') {
            Some(base.powf(self.power()?))
        } else {
            Some(base)
        }
    }

    fn primary(&mut self) -> Option<f64> {
        if self.eat('(') {
            let v = self.expr()?;
            return self.eat(')').then_some(v);
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.i += 1;
        }
        let number: String = self.s[start..self.i].iter().collect();
        if number.is_empty() {
            return None;
        }
        number.parse().ok()
    }
}

/// 给人看的结果：整数不带小数点，其余最多 10 位小数、去掉末尾的 0。
pub fn format(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        // 已经确认是整数且在范围内
        #[allow(clippy::cast_possible_truncation)]
        let n = v.round() as i64;
        return n.to_string();
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// 文字里的 `=算式` 都换成结果。返回（新文字，算过的式子「3*45 = 135」）。
pub fn expand(text: &str) -> (String, Vec<String>) {
    let mut done = Vec::new();
    let words: Vec<String> = text
        .split(' ')
        .map(|w| {
            let Some(expr) = w.strip_prefix('=').filter(|e| !e.is_empty()) else {
                return w.to_owned();
            };
            match eval(expr) {
                Some(v) => {
                    let result = format(v);
                    done.push(format!("{expr} = {result}"));
                    result
                }
                None => w.to_owned(),
            }
        })
        .collect();
    (words.join(" "), done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        for (expr, want) in [
            ("3*45", "135"),
            ("3*45+12", "147"),
            ("(1+2)*3", "9"),
            ("10/4", "2.5"),
            ("1/3", "0.3333333333"),
            ("2^10", "1024"),
            ("2^3^2", "512"),
            ("-2^2", "-4"),
            ("(-2)^2", "4"),
            ("7%3", "1"),
            ("3×4÷2", "6"),
            ("0.1+0.2", "0.3"),
            ("-5+-3", "-8"),
            ("1.5", "1.5"),
        ] {
            assert_eq!(eval(expr).map(format).as_deref(), Some(want), "{expr}");
        }
    }

    #[test]
    fn not_arithmetic() {
        for expr in [
            "", "abc", "1/0", "5%0", "(1+2", "1+", "1..2", "3*", ")", "1e5", "10^400",
        ] {
            assert_eq!(eval(expr), None, "{expr}");
        }
    }

    #[test]
    fn expanding_text() {
        assert_eq!(
            expand("买 3 个 =3*45 #t"),
            ("买 3 个 135 #t".to_owned(), vec!["3*45 = 135".to_owned()])
        );
        assert_eq!(
            expand("=a+b 不是算式 = 也不是"),
            ("=a+b 不是算式 = 也不是".to_owned(), vec![])
        );
        let (text, done) = expand("=1+1 和 =2*3");
        assert_eq!(text, "2 和 6");
        assert_eq!(done.len(), 2);
    }
}
